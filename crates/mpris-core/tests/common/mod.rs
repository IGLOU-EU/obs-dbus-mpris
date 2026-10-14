//! Test support for the D-Bus adapter: a private bus per test and fake MPRIS
//! players served on it, so no test ever touches the user's session bus.

// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Cursor};
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::process::{self, Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mpris_core::dbus::Core;
use mpris_core::model::{Art, Player, Snapshot};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, connection, fdo, interface, names::InterfaceName};

pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
pub const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

/// Long enough for a loaded machine, short enough to fail fast.
const DEADLINE: Duration = Duration::from_secs(5);

/// A `dbus-daemon` of its own, killed on drop even when the test fails.
pub struct PrivateBus {
    child: Child,
    pub address: String,
}

impl PrivateBus {
    pub fn start() -> PrivateBus {
        let child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon is required by the D-Bus tests");
        // Built before reading, so a failed read still kills the daemon.
        let mut bus = PrivateBus {
            child,
            address: String::new(),
        };
        let stdout = bus.child.stdout.take().expect("stdout is piped");
        BufReader::new(stdout)
            .read_line(&mut bus.address)
            .expect("dbus-daemon prints its address");
        bus.address.truncate(bus.address.trim_end().len());
        assert!(!bus.address.is_empty(), "dbus-daemon printed no address");
        bus
    }

    /// Stops the daemon, as when the session bus goes away.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        self.kill();
    }
}

pub type Metadata = HashMap<String, OwnedValue>;

pub fn metadata(entries: Vec<(&str, Value<'static>)>) -> Metadata {
    entries
        .into_iter()
        .map(|(key, value)| {
            let owned = OwnedValue::try_from(value).expect("test values hold no fd");
            (key.to_owned(), owned)
        })
        .collect()
}

/// Metadata of a typical track, `art_url` aside.
pub fn track(id: &str, title: &str, art_url: Option<&str>) -> Metadata {
    let mut entries = vec![
        (
            "mpris:trackid",
            Value::from(zbus::zvariant::ObjectPath::try_from(id.to_owned()).expect("valid path")),
        ),
        ("xesam:title", Value::from(title.to_owned())),
        ("xesam:artist", Value::from(vec!["CRYSTAL KING".to_owned()])),
        ("xesam:album", Value::from("愛をとりもどせ!!".to_owned())),
        ("mpris:length", Value::from(219_533_000i64)),
    ];
    if let Some(url) = art_url {
        entries.push(("mpris:artUrl", Value::from(url.to_owned())));
    }
    metadata(entries)
}

fn copy(metadata: &Metadata) -> Metadata {
    metadata
        .iter()
        .map(|(key, value)| (key.clone(), value.try_clone().expect("no fd")))
        .collect()
}

/// A red PNG of the given size.
pub fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]));
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG encoding");
    bytes.into_inner()
}

/// A `data:` URL holding a red PNG of the given size.
pub fn png_data_url(width: u32, height: u32) -> String {
    format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png(width, height))
    )
}

/// Initial state of a fake player.
pub struct Fake {
    pub identity: String,
    pub status: String,
    pub rate: f64,
    pub metadata: Metadata,
    pub position: Duration,
    /// Never answers a read of the `Player` interface, like a frozen process.
    pub hang: bool,
}

impl Fake {
    pub fn new(identity: &str, status: &str, metadata: Metadata) -> Fake {
        Fake {
            identity: identity.to_owned(),
            status: status.to_owned(),
            rate: 1.0,
            metadata,
            position: Duration::ZERO,
            hang: false,
        }
    }
}

struct Root {
    identity: String,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }
}

struct PlayerIface {
    status: String,
    rate: f64,
    metadata: Metadata,
    position: i64,
    hang: bool,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerIface {
    #[zbus(property)]
    async fn playback_status(&self) -> String {
        if self.hang {
            std::future::pending::<()>().await;
        }
        self.status.clone()
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.rate
    }

    #[zbus(property)]
    fn metadata(&self) -> Metadata {
        copy(&self.metadata)
    }

    // The spec says Position changes are not signalled.
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.position
    }

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

fn micros(position: Duration) -> i64 {
    i64::try_from(position.as_micros()).expect("test positions fit")
}

/// An MPRIS player on its own connection, owning `bus_name`.
pub struct FakePlayer {
    conn: Connection,
    pub bus_name: String,
}

impl FakePlayer {
    pub fn start(bus: &PrivateBus, bus_name: &str, fake: Fake) -> FakePlayer {
        let root = Root {
            identity: fake.identity,
        };
        let player = PlayerIface {
            status: fake.status,
            rate: fake.rate,
            metadata: fake.metadata,
            position: micros(fake.position),
            hang: fake.hang,
        };
        let conn = async_io::block_on(async {
            connection::Builder::address(bus.address.as_str())?
                .name(bus_name)?
                .serve_at(PATH, root)?
                .serve_at(PATH, player)?
                .build()
                .await
        })
        .expect("fake player connects");
        FakePlayer {
            conn,
            bus_name: bus_name.to_owned(),
        }
    }

    pub fn unique_name(&self) -> String {
        self.conn.unique_name().expect("bus connection").to_string()
    }

    fn edit(&self, edit: impl FnOnce(&mut PlayerIface)) {
        async_io::block_on(async {
            let iface = self
                .conn
                .object_server()
                .interface::<_, PlayerIface>(PATH)
                .await
                .expect("player interface");
            edit(&mut *iface.get_mut().await);
        });
    }

    fn emit(&self, interface: &'static str, changed: Vec<(&str, Value<'_>)>, invalidated: &[&str]) {
        async_io::block_on(async {
            let emitter = SignalEmitter::new(&self.conn, PATH).expect("valid path");
            fdo::Properties::properties_changed(
                &emitter,
                InterfaceName::from_static_str_unchecked(interface),
                changed.into_iter().collect(),
                Cow::Borrowed(invalidated),
            )
            .await
            .expect("PropertiesChanged sent");
        });
    }

    /// Sends a `PropertiesChanged` on the `Player` interface as given, valid
    /// or not, without touching the served state.
    pub fn emit_raw(&self, changed: Vec<(&str, Value<'_>)>, invalidated: &[&str]) {
        self.emit(PLAYER_IFACE, changed, invalidated);
    }

    pub fn set_status(&self, status: &str) {
        self.edit(|p| p.status = status.to_owned());
        self.emit(
            PLAYER_IFACE,
            vec![("PlaybackStatus", Value::from(status))],
            &[],
        );
    }

    pub fn set_metadata(&self, metadata: Metadata) {
        let sent = Value::from(copy(&metadata));
        self.edit(|p| p.metadata = metadata);
        self.emit(PLAYER_IFACE, vec![("Metadata", sent)], &[]);
    }

    /// Changes the metadata but only says it is stale, as some players do.
    pub fn invalidate_metadata(&self, metadata: Metadata) {
        self.edit(|p| p.metadata = metadata);
        self.emit(PLAYER_IFACE, vec![], &["Metadata"]);
    }

    pub fn set_identity(&self, identity: &str) {
        async_io::block_on(async {
            let iface = self
                .conn
                .object_server()
                .interface::<_, Root>(PATH)
                .await
                .expect("root interface");
            iface.get_mut().await.identity = identity.to_owned();
        });
        self.emit(ROOT_IFACE, vec![("Identity", Value::from(identity))], &[]);
    }

    /// Moves the position silently, as playback does.
    pub fn set_position(&self, position: Duration) {
        self.edit(|p| p.position = micros(position));
    }

    pub fn seek(&self, position: Duration) {
        self.set_position(position);
        async_io::block_on(async {
            let emitter = SignalEmitter::new(&self.conn, PATH).expect("valid path");
            PlayerIface::seeked(&emitter, micros(position))
                .await
                .expect("Seeked sent");
        });
    }

    /// Leaves the bus, as when the player quits.
    pub fn quit(self) {
        async_io::block_on(self.conn.graceful_shutdown());
    }
}

/// A connection owning a name, for names the core must ignore or for a
/// player that serves nothing.
pub fn own_name(bus: &PrivateBus, name: &str) -> Connection {
    async_io::block_on(async {
        connection::Builder::address(bus.address.as_str())?
            .name(name)?
            .build()
            .await
    })
    .expect("connection owning a name")
}

/// A cover whose `open` blocks until released, as a read on a hung network
/// mount does. It holds a write lease (fcntl(2), "Leases"): the kernel
/// keeps any other open of the file waiting until the lease goes, or for
/// `/proc/sys/fs/lease-break-time` at most.
pub struct HungFile {
    path: PathBuf,
    lease: Option<File>,
}

impl HungFile {
    pub fn new() -> HungFile {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("mpris-core-hung-{}-{unique}.png", process::id());
        let path = env::temp_dir().join(name);
        fs::write(&path, b"").expect("hung file written");
        // Built before the lease is taken, so a failure below still
        // removes the file.
        let mut hung = HungFile { path, lease: None };
        let file = File::open(&hung.path).expect("hung file opened");
        let fd = file.as_raw_fd();
        // SAFETY: fcntl on a descriptor this function owns, with integer
        // arguments only.
        let leased = unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_WRLCK) };
        assert_eq!(
            leased,
            0,
            "write lease refused ({}), leases must be enabled in /proc/sys/fs/leases-enable",
            io::Error::last_os_error()
        );
        // With no owner the kernel sends no SIGIO when the lease is broken,
        // a signal that would kill the test process.
        // SAFETY: as above.
        let unowned = unsafe { libc::fcntl(fd, libc::F_SETOWN, 0) };
        assert_eq!(unowned, 0, "{}", io::Error::last_os_error());
        hung.lease = Some(file);
        hung
    }

    pub fn url(&self) -> String {
        format!("file://{}", self.path.display())
    }

    /// Whether an open of the file waits on the lease: the kernel then
    /// reports the read lease it is breaking the write lease down to.
    pub fn has_waiting_open(&self) -> bool {
        let Some(lease) = &self.lease else {
            return false;
        };
        // SAFETY: fcntl on a descriptor `lease` keeps open.
        unsafe { libc::fcntl(lease.as_raw_fd(), libc::F_GETLEASE) == libc::F_RDLCK }
    }

    /// Lets every waiting open through.
    pub fn release(&mut self) {
        self.lease = None;
    }
}

impl Drop for HungFile {
    fn drop(&mut self) {
        self.release();
        let _ = fs::remove_file(&self.path);
    }
}

/// Names of the threads of this process, as the kernel reports them
/// (truncated to 15 bytes).
pub fn thread_names() -> Vec<String> {
    fs::read_dir("/proc/self/task")
        .expect("procfs")
        .filter_map(|task| fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .map(|name| name.trim_end().to_owned())
        .collect()
}

/// How many threads of this process have a name starting with `prefix`.
pub fn count(prefix: &str) -> usize {
    thread_names()
        .iter()
        .filter(|name| name.starts_with(prefix))
        .count()
}

/// Polls `attempt` until it gives a value, and fails with `what` and the
/// state `seen` describes once the deadline has passed.
fn poll<T>(what: &str, attempt: impl Fn() -> Option<T>, seen: impl Fn() -> String) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        if let Some(value) = attempt() {
            return value;
        }
        if Instant::now() > deadline {
            panic!("timed out waiting for {what}{}", seen());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Polls the core until `done` holds, and fails with what it saw last.
pub fn wait_for(core: &Core, what: &str, done: impl Fn(&Snapshot) -> bool) -> Arc<Snapshot> {
    poll(
        what,
        || Some(core.snapshot()).filter(|snapshot| done(snapshot)),
        || format!(", last snapshot:\n{}", describe(&core.snapshot())),
    )
}

/// Polls `done` until it holds.
pub fn wait_until(what: &str, done: impl Fn() -> bool) {
    poll(what, || done().then_some(()), String::new)
}

pub fn find<'a>(snapshot: &'a Snapshot, bus_name: &str) -> Option<&'a Player> {
    snapshot.players.iter().find(|p| p.bus_name == bus_name)
}

/// One line per player. A snapshot's own `Debug` would dump every pixel.
pub fn describe(snapshot: &Snapshot) -> String {
    if snapshot.players.is_empty() {
        return "  (no player)".into();
    }
    snapshot
        .players
        .iter()
        .map(|p| {
            let art = match &p.art {
                Art::None => "none".to_owned(),
                Art::Pending => "pending".to_owned(),
                Art::Ready(image) => format!("ready {}x{}", image.width, image.height),
                Art::Failed => "failed".to_owned(),
            };
            format!(
                "  {} ({}) key={} identity={:?} {:?} rate={} title={:?} anchor={:?} art={art}",
                p.bus_name,
                p.unique_name,
                p.key.0,
                p.identity,
                p.status,
                p.rate,
                p.track.title,
                p.anchor.position,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}
