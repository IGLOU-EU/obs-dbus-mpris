//! D-Bus adapter: watches MPRIS players and feeds the store.
//!
//! One core thread owns the bus connection and the [`Store`]. It turns
//! signals and property reads into [`Event`]s, applies them, loads the art
//! the store asks for, and publishes an immutable [`Snapshot`] that any
//! thread reads without waiting on the bus (B-41).
//!
//! The loop is async on a single thread: the zbus streams, the poll timer
//! and the stop signal are awaited together, so a stop request ends the
//! thread even while a call to a frozen player is pending. Blocking feeder
//! threads could not be woken out of their reads, and shutdown would wait
//! on them. The connection runs without zbus's own executor thread, so
//! joining the core thread is enough to know nothing of ours still runs.

use std::collections::{BTreeSet, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::Timer;
use futures_lite::{FutureExt, StreamExt};
use zbus::message::Type as MessageType;
use zbus::zvariant::{DynamicDeserialize, DynamicType, OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream, connection};

use crate::art::load_art;
use crate::metadata::parse_track;
use crate::model::{PlaybackStatus, Snapshot};
use crate::player::{
    player_key, read_identity, read_position, read_rate, read_status, unwrap_variant,
};
use crate::store::{Event, Store};

const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";
const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";

/// Long enough for a busy player to send a large `Metadata` (mpv inlines
/// its cover), short enough that a frozen one stalls the loop only briefly.
const CALL_TIMEOUT: Duration = Duration::from_secs(1);

/// Players that omit `Seeked` drift by at most this much (D-05, B-17).
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The one slot shared between threads: replaced whole, held only to clone.
type Shared = Arc<Mutex<Arc<Snapshot>>>;

/// Properties of one interface, as `GetAll` and `PropertiesChanged` send them.
type Props = HashMap<String, OwnedValue>;

/// Handle on the core thread. Dropping it stops the thread.
pub struct Core {
    shared: Shared,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl Core {
    /// Watches the players of the user's session bus.
    pub fn start() -> Core {
        Core::spawn(None)
    }

    /// Watches the players of the bus at `address`, so tests never touch
    /// the user's session.
    pub fn start_at(address: &str) -> Core {
        Core::spawn(Some(address.to_owned()))
    }

    /// Latest published state. Never waits on the bus.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        lock(&self.shared).clone()
    }

    fn spawn(address: Option<String>) -> Core {
        let shared = Shared::default();
        let (stop, stopped) = async_channel::bounded(1);
        let thread = thread::Builder::new().name("mpris-core".into()).spawn({
            let shared = shared.clone();
            move || run(address.as_deref(), &shared, stopped)
        });
        // Without the thread the snapshot stays empty, which the sources
        // already show as "nothing playing" (B-22).
        let thread = thread
            .inspect_err(|error| log::error!("MPRIS: cannot start the core thread: {error}"))
            .ok();
        Core {
            shared,
            stop,
            thread,
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // Closing the channel is the stop signal.
        self.stop.close();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(shared: &Shared) -> MutexGuard<'_, Arc<Snapshot>> {
    // The guard is only held to swap or clone an Arc, so a poisoned slot
    // still holds a whole snapshot.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish(shared: &Shared, snapshot: Arc<Snapshot>) {
    *lock(shared) = snapshot;
}

/// Body of the core thread. A panic stops it here instead of reaching OBS
/// (B-42), and leaves an empty snapshot rather than a frozen one.
fn run(address: Option<&str>, shared: &Shared, stopped: Receiver<()>) {
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        async_io::block_on(async {
            let stop = async {
                let _ = stopped.recv().await;
            };
            stop.or(serve(address, shared)).await
        })
    }));
    if outcome.is_err() {
        log::error!("MPRIS: the core thread panicked, players are no longer watched");
        publish(shared, Arc::default());
    }
}

async fn serve(address: Option<&str>, shared: &Shared) {
    let conn = match connect(address).await {
        Ok(conn) => conn,
        Err(error) => {
            log::error!("MPRIS: cannot connect to the D-Bus session bus: {error}");
            return;
        }
    };
    let executor = conn.executor();
    let drive = async {
        loop {
            executor.tick().await;
        }
    };
    match watch(&conn, shared).or(drive).await {
        Ok(()) => log::warn!("MPRIS: lost the D-Bus session bus"),
        Err(error) => log::error!("MPRIS: cannot watch the D-Bus session bus: {error}"),
    }
    // Players on a lost bus are gone as far as anyone can tell (B-22).
    publish(shared, Arc::default());
}

async fn connect(address: Option<&str>) -> zbus::Result<Connection> {
    let builder = match address {
        Some(address) => connection::Builder::address(address)?,
        None => connection::Builder::session()?,
    };
    builder
        .internal_executor(false)
        .method_timeout(CALL_TIMEOUT)
        .build()
        .await
}

/// What woke the loop.
enum Input {
    Owner(Message),
    Player(Message),
    Poll,
    Lost,
}

async fn watch(conn: &Connection, shared: &Shared) -> zbus::Result<()> {
    // Subscribed before the first scan, so a player starting in between is
    // not missed.
    let mut owners = subscribe(conn, owner_rule()?).await?;
    let mut players = subscribe(conn, player_rule()?).await?;
    let mut watcher = Watcher::new(conn, shared);
    watcher.discover().await;

    let mut poll = Timer::after(POLL_INTERVAL);
    loop {
        let input = next(&mut owners, Input::Owner)
            .or(next(&mut players, Input::Player))
            .or(async {
                (&mut poll).await;
                Input::Poll
            })
            .await;
        match input {
            Input::Owner(msg) => watcher.on_owner_changed(&msg).await,
            Input::Player(msg) => watcher.on_player_signal(&msg).await,
            Input::Poll => {
                watcher.poll_positions().await;
                // Counted from the end of the poll, so a slow player never
                // makes polls run back to back.
                poll.set_after(POLL_INTERVAL);
            }
            Input::Lost => return Ok(()),
        }
    }
}

async fn subscribe(conn: &Connection, rule: MatchRule<'static>) -> zbus::Result<MessageStream> {
    MessageStream::for_match_rule(rule, conn, None).await
}

/// Next message of `stream`, or [`Input::Lost`] once the connection closed.
async fn next(stream: &mut MessageStream, input: fn(Message) -> Input) -> Input {
    loop {
        match stream.next().await {
            Some(Ok(msg)) => return input(msg),
            Some(Err(error)) => log::debug!("MPRIS: unreadable message: {error}"),
            None => return Input::Lost,
        }
    }
}

fn owner_rule() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(BUS_NAME)?
        .interface(BUS_NAME)?
        .member("NameOwnerChanged")?
        // Leaves the filtering of non-MPRIS names to the bus.
        .arg0ns(ROOT_IFACE)?
        .build())
}

/// Every signal of the MPRIS object, in one stream: a player's
/// `PropertiesChanged` and `Seeked` must apply in the order it sent them,
/// which streams polled by priority do not keep.
fn player_rule() -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(MessageType::Signal)
        .path(MPRIS_PATH)?
        .build())
}

/// The loop's state: the store, its only writer, and the last snapshot.
struct Watcher<'a> {
    conn: &'a Connection,
    shared: &'a Shared,
    store: Store,
    /// Last published snapshot. Tells which processes are players.
    current: Arc<Snapshot>,
}

impl<'a> Watcher<'a> {
    fn new(conn: &'a Connection, shared: &'a Shared) -> Self {
        Watcher {
            conn,
            shared,
            store: Store::new(),
            current: Arc::default(),
        }
    }

    /// Every player already on the bus.
    async fn discover(&mut self) {
        let names: Vec<String> = call(self.conn, BUS_NAME, BUS_PATH, BUS_NAME, "ListNames", &())
            .await
            .unwrap_or_default();
        let mut changed = false;
        for name in names.iter().filter(|name| player_key(name).is_some()) {
            let owner: Option<String> = call(
                self.conn,
                BUS_NAME,
                BUS_PATH,
                BUS_NAME,
                "GetNameOwner",
                name,
            )
            .await;
            if let Some(owner) = owner {
                changed |= self.appear(name, &owner).await;
            }
        }
        self.commit(changed).await;
    }

    async fn on_owner_changed(&mut self, msg: &Message) {
        let Ok((name, old_owner, new_owner)) = msg.body().deserialize::<(String, String, String)>()
        else {
            return;
        };
        let changed = match owner_change(&name, &old_owner, &new_owner) {
            None => return,
            Some(OwnerChange::Vanished) => self.vanish(&name),
            Some(OwnerChange::Appeared { owner }) => self.appear(&name, &owner).await,
            // Applied together, so no snapshot ever shows the player gone.
            Some(OwnerChange::Replaced { owner }) => {
                self.vanish(&name) | self.appear(&name, &owner).await
            }
        };
        self.commit(changed).await;
    }

    /// Ignores the signals of interfaces the store does not follow, which
    /// the one rule for the whole object lets through.
    async fn on_player_signal(&mut self, msg: &Message) {
        let header = msg.header();
        let interface = header.interface().map(|name| name.as_str());
        let member = header.member().map(|name| name.as_str());
        match (interface, member) {
            (Some(PROPERTIES_IFACE), Some("PropertiesChanged")) => {
                self.on_properties_changed(msg).await
            }
            (Some(PLAYER_IFACE), Some("Seeked")) => self.on_seeked(msg).await,
            _ => {}
        }
    }

    async fn on_properties_changed(&mut self, msg: &Message) {
        let Some(owner) = sender(msg) else { return };
        // Saves the calls below for processes that are not players.
        if !self.knows(&owner) {
            return;
        }
        let Ok((interface, mut props, invalidated)) =
            msg.body().deserialize::<(String, Props, Vec<String>)>()
        else {
            return;
        };
        let stale = invalidated
            .iter()
            .filter(|name| watched(&interface).contains(&name.as_str()));
        for name in stale {
            if let Some(value) = get(self.conn, &owner, &interface, name).await {
                props.insert(name.clone(), value);
            }
        }
        let Some(event) = changed(&owner, &interface, &props, Instant::now()) else {
            return;
        };
        let reread = moves_position(&event);
        let mut changed = self.store.apply(event);
        // Even when the store saw nothing new: a track restarted on repeat
        // sends the same metadata, and the adapter cannot tell a new track
        // from the tags (Firefox keeps one track id for every track).
        if reread {
            changed |= self.read_position(&owner).await;
        }
        self.commit(changed).await;
    }

    async fn on_seeked(&mut self, msg: &Message) {
        let Some(owner) = sender(msg) else { return };
        let Ok(micros) = msg.body().deserialize::<i64>() else {
            return;
        };
        let event = Event::PositionRead {
            unique_name: owner,
            position: read_position(&Value::from(micros)).unwrap_or_default(),
            at: Instant::now(),
        };
        let changed = self.store.apply(event);
        self.commit(changed).await;
    }

    /// Re-reads the position of playing players, for those that omit
    /// `Seeked` (D-05, B-17).
    async fn poll_positions(&mut self) {
        let playing: BTreeSet<String> = self
            .current
            .players
            .iter()
            .filter(|player| player.status == PlaybackStatus::Playing)
            .map(|player| player.unique_name.clone())
            .collect();
        let mut changed = false;
        for owner in playing {
            changed |= self.read_position(&owner).await;
        }
        self.commit(changed).await;
    }

    async fn appear(&mut self, bus_name: &str, owner: &str) -> bool {
        let player: Props = call(
            self.conn,
            owner,
            MPRIS_PATH,
            PROPERTIES_IFACE,
            "GetAll",
            &PLAYER_IFACE,
        )
        .await
        .unwrap_or_default();
        let identity = get(self.conn, owner, ROOT_IFACE, "Identity").await;
        match appeared(
            owner,
            bus_name,
            &player,
            identity.as_deref(),
            Instant::now(),
        ) {
            Some(event) => self.store.apply(event),
            None => false,
        }
    }

    fn vanish(&mut self, bus_name: &str) -> bool {
        self.store.apply(Event::Vanished {
            bus_name: bus_name.to_owned(),
        })
    }

    async fn read_position(&mut self, owner: &str) -> bool {
        let value = get(self.conn, owner, PLAYER_IFACE, "Position").await;
        let Some(position) = value.as_deref().and_then(read_position) else {
            return false;
        };
        self.store.apply(Event::PositionRead {
            unique_name: owner.to_owned(),
            position,
            at: Instant::now(),
        })
    }

    fn knows(&self, owner: &str) -> bool {
        self.current
            .players
            .iter()
            .any(|player| player.unique_name == owner)
    }

    /// Loads the art the store waits for, then publishes, if anything
    /// changed. Loading never runs on a render thread (B-41). It runs on
    /// the `blocking` pool, awaited within the loop that races the stop
    /// signal, because a cover on a hung network mount can block its reader
    /// for good: a stop abandons such a load instead of waiting for it.
    async fn commit(&mut self, changed: bool) {
        if !changed {
            return;
        }
        for (owner, url) in self.store.pending_art() {
            let loading = url.clone();
            let image = blocking::unblock(move || load_art(&loading))
                .await
                .inspect_err(|error| log::debug!("MPRIS: no art for {owner}: {error}"))
                .ok()
                .map(Arc::new);
            self.store.apply(Event::ArtLoaded {
                unique_name: owner,
                url,
                image,
            });
        }
        self.current = Arc::new(self.store.snapshot());
        publish(self.shared, self.current.clone());
    }
}

/// Unique name of the process that sent `msg`.
fn sender(msg: &Message) -> Option<String> {
    msg.header().sender().map(|sender| sender.to_string())
}

/// A method call whose failure, timeout included, only means "no answer":
/// a misbehaving player must never stop the loop (B-42).
async fn call<B, R>(
    conn: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    method: &str,
    body: &B,
) -> Option<R>
where
    B: serde::Serialize + DynamicType,
    R: for<'d> DynamicDeserialize<'d>,
{
    let reply = conn
        .call_method(Some(destination), path, Some(interface), method, body)
        .await
        .inspect_err(|error| log::debug!("MPRIS: {method} on {destination} failed: {error}"))
        .ok()?;
    reply
        .body()
        .deserialize()
        .inspect_err(|error| log::debug!("MPRIS: {method} on {destination}: {error}"))
        .ok()
}

/// One property, read with `org.freedesktop.DBus.Properties.Get`.
async fn get(conn: &Connection, owner: &str, interface: &str, name: &str) -> Option<OwnedValue> {
    call(
        conn,
        owner,
        MPRIS_PATH,
        PROPERTIES_IFACE,
        "Get",
        &(interface, name),
    )
    .await
}

/// What a `NameOwnerChanged` means for a player name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum OwnerChange {
    Vanished,
    Appeared {
        owner: String,
    },
    /// The name moved to another process without a gap.
    Replaced {
        owner: String,
    },
}

/// `None` for names that are not MPRIS players, and for the change without
/// any owner that the bus never sends.
fn owner_change(name: &str, old_owner: &str, new_owner: &str) -> Option<OwnerChange> {
    player_key(name)?;
    match (old_owner.is_empty(), new_owner.is_empty()) {
        (false, true) => Some(OwnerChange::Vanished),
        (true, false) => Some(OwnerChange::Appeared {
            owner: new_owner.to_owned(),
        }),
        (false, false) => Some(OwnerChange::Replaced {
            owner: new_owner.to_owned(),
        }),
        (true, true) => None,
    }
}

/// A player as read on arrival. What it did not answer reads as a silent
/// player: stopped, at normal speed, with no track, named after its key so
/// the selector still has a label (B-02).
fn appeared(
    owner: &str,
    bus_name: &str,
    player: &Props,
    identity: Option<&Value<'_>>,
    at: Instant,
) -> Option<Event> {
    let key = player_key(bus_name)?;
    let get = |name: &str| player.get(name).map(|value| &**value);
    Some(Event::Appeared {
        unique_name: owner.to_owned(),
        bus_name: bus_name.to_owned(),
        identity: identity
            .and_then(read_identity)
            .unwrap_or_else(|| key.0.clone()),
        key,
        status: get("PlaybackStatus")
            .and_then(read_status)
            .unwrap_or_default(),
        rate: get("Rate").and_then(read_rate).unwrap_or(1.0),
        track: get("Metadata")
            .and_then(read_metadata)
            .map(|metadata| parse_track(&metadata))
            .unwrap_or_default(),
        position: get("Position").and_then(read_position).unwrap_or_default(),
        at,
    })
}

/// The `Changed` event a `PropertiesChanged` stands for, `None` for an
/// interface the store does not follow.
fn changed(owner: &str, interface: &str, props: &Props, at: Instant) -> Option<Event> {
    let get = |name: &str| props.get(name).map(|value| &**value);
    let (status, rate, track, identity) = match interface {
        PLAYER_IFACE => (
            get("PlaybackStatus").and_then(read_status),
            get("Rate").and_then(read_rate),
            get("Metadata")
                .and_then(read_metadata)
                .map(|metadata| parse_track(&metadata)),
            None,
        ),
        ROOT_IFACE => (None, None, None, get("Identity").and_then(read_identity)),
        _ => return None,
    };
    Some(Event::Changed {
        unique_name: owner.to_owned(),
        status,
        rate,
        track,
        identity,
        at,
    })
}

/// Properties the store uses, the only ones worth a call when a player
/// invalidates them.
fn watched(interface: &str) -> &'static [&'static str] {
    match interface {
        PLAYER_IFACE => &["PlaybackStatus", "Rate", "Metadata"],
        ROOT_IFACE => &["Identity"],
        _ => &[],
    }
}

/// Whether a report may come with a position jump the player does not
/// signal: Play after Stop restarts from zero, and new metadata may be a
/// new track or the same one restarted.
fn moves_position(event: &Event) -> bool {
    matches!(
        event,
        Event::Changed {
            status: Some(_),
            ..
        } | Event::Changed { track: Some(_), .. }
    )
}

/// The `a{sv}` of `Metadata`, `None` when the player sent something else.
fn read_metadata(value: &Value<'_>) -> Option<Props> {
    let owned = unwrap_variant(value).try_to_owned().ok()?;
    Props::try_from(owned).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::metadata::parse_track;
    use crate::model::{PlaybackStatus, PlayerKey, Track};

    const OWNER: &str = ":1.42";
    const BUS: &str = "org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA";

    fn owned(value: Value<'static>) -> OwnedValue {
        OwnedValue::try_from(value).expect("test values hold no fd")
    }

    fn props(entries: Vec<(&str, Value<'static>)>) -> Props {
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), owned(value)))
            .collect()
    }

    /// `Metadata` as it comes off the bus: an `a{sv}` dictionary.
    fn metadata_value(title: &str) -> Value<'static> {
        let entries = props(vec![
            ("xesam:title", Value::from(title.to_owned())),
            ("xesam:artist", Value::from(vec!["CRYSTAL KING"])),
        ]);
        Value::from(entries)
    }

    fn titled(title: &str) -> Track {
        Track {
            title: Some(title.into()),
            artists: vec!["CRYSTAL KING".into()],
            ..Track::default()
        }
    }

    fn change(
        status: Option<PlaybackStatus>,
        rate: Option<f64>,
        track: Option<Track>,
        identity: Option<&str>,
        at: Instant,
    ) -> Event {
        Event::Changed {
            unique_name: OWNER.into(),
            status,
            rate,
            track,
            identity: identity.map(Into::into),
            at,
        }
    }

    fn empty_change(at: Instant) -> Event {
        change(None, None, None, None, at)
    }

    // owner_change.

    #[test]
    fn owner_change_tells_appearance_vanishing_and_replacement() {
        assert_eq!(
            owner_change(BUS, "", ":1.42"),
            Some(OwnerChange::Appeared {
                owner: ":1.42".into()
            })
        );
        assert_eq!(owner_change(BUS, ":1.42", ""), Some(OwnerChange::Vanished));
        assert_eq!(
            owner_change(BUS, ":1.42", ":1.99"),
            Some(OwnerChange::Replaced {
                owner: ":1.99".into()
            })
        );
    }

    #[test]
    fn owner_change_ignores_names_outside_mpris() {
        for name in [
            ":1.42",
            "org.freedesktop.Notifications",
            "org.mpris.MediaPlayer2",
            "org.mpris.MediaPlayer2.",
        ] {
            assert_eq!(owner_change(name, "", ":1.42"), None, "{name}");
            assert_eq!(owner_change(name, ":1.42", ""), None, "{name}");
        }
    }

    #[test]
    fn owner_change_without_any_owner_is_ignored() {
        assert_eq!(owner_change(BUS, "", ""), None);
    }

    // appeared.

    #[test]
    fn appeared_reads_every_property() {
        let at = Instant::now();
        let player = props(vec![
            ("PlaybackStatus", Value::from("Playing")),
            ("Rate", Value::from(2.0f64)),
            ("Metadata", metadata_value("愛をとりもどせ!!")),
            ("Position", Value::from(83_500_000i64)),
            ("CanSeek", Value::from(true)),
        ]);
        let identity = Value::from("mpv");
        assert_eq!(
            appeared(OWNER, BUS, &player, Some(&identity), at),
            Some(Event::Appeared {
                unique_name: OWNER.into(),
                bus_name: BUS.into(),
                key: PlayerKey("mpv".into()),
                identity: "mpv".into(),
                status: PlaybackStatus::Playing,
                rate: 2.0,
                track: titled("愛をとりもどせ!!"),
                position: Duration::from_micros(83_500_000),
                at,
            })
        );
    }

    #[test]
    fn appeared_without_answers_falls_back_to_a_silent_player() {
        let at = Instant::now();
        let expected = Some(Event::Appeared {
            unique_name: OWNER.into(),
            bus_name: BUS.into(),
            key: PlayerKey("mpv".into()),
            identity: "mpv".into(),
            status: PlaybackStatus::Stopped,
            rate: 1.0,
            track: Track::default(),
            position: Duration::ZERO,
            at,
        });
        assert_eq!(appeared(OWNER, BUS, &Props::new(), None, at), expected);

        let wrong_types = props(vec![
            ("PlaybackStatus", Value::from(3i32)),
            ("Rate", Value::from("fast")),
            ("Metadata", Value::from("not a dictionary")),
            ("Position", Value::from(1.5f64)),
        ]);
        let blank = Value::from("  ");
        assert_eq!(
            appeared(OWNER, BUS, &wrong_types, Some(&blank), at),
            expected
        );
    }

    #[test]
    fn appeared_refuses_names_outside_mpris() {
        let at = Instant::now();
        assert_eq!(
            appeared(OWNER, "org.example.Player", &Props::new(), None, at),
            None
        );
    }

    // changed.

    #[test]
    fn player_interface_change_carries_status_rate_and_track() {
        let at = Instant::now();
        let changes = props(vec![
            ("PlaybackStatus", Value::from("Paused")),
            ("Rate", Value::from(0.5f64)),
            ("Metadata", metadata_value("Next")),
            ("Volume", Value::from(0.3f64)),
        ]);
        assert_eq!(
            changed(OWNER, PLAYER_IFACE, &changes, at),
            Some(change(
                Some(PlaybackStatus::Paused),
                Some(0.5),
                Some(titled("Next")),
                None,
                at
            ))
        );
    }

    #[test]
    fn player_interface_change_leaves_out_what_is_absent_or_invalid() {
        let at = Instant::now();
        let changes = props(vec![
            ("PlaybackStatus", Value::from("Buffering")),
            ("Rate", Value::from(f64::NAN)),
            ("Metadata", Value::from(vec!["not", "a", "dictionary"])),
            ("Identity", Value::from("belongs to the root interface")),
        ]);
        assert_eq!(
            changed(OWNER, PLAYER_IFACE, &changes, at),
            Some(empty_change(at))
        );
    }

    #[test]
    fn root_interface_change_carries_identity_only() {
        let at = Instant::now();
        let changes = props(vec![
            ("Identity", Value::from("mpv Media Player")),
            ("PlaybackStatus", Value::from("Playing")),
        ]);
        assert_eq!(
            changed(OWNER, ROOT_IFACE, &changes, at),
            Some(change(None, None, None, Some("mpv Media Player"), at))
        );
        let blank = props(vec![("Identity", Value::from(""))]);
        assert_eq!(
            changed(OWNER, ROOT_IFACE, &blank, at),
            Some(empty_change(at))
        );
    }

    #[test]
    fn other_interfaces_are_ignored() {
        let at = Instant::now();
        let changes = props(vec![("PlaybackStatus", Value::from("Playing"))]);
        for interface in [
            "org.mpris.MediaPlayer2.TrackList",
            "org.mpris.MediaPlayer2.Playlists",
            "",
        ] {
            assert_eq!(changed(OWNER, interface, &changes, at), None, "{interface}");
        }
    }

    // watched.

    #[test]
    fn watched_lists_the_properties_the_store_uses() {
        assert_eq!(
            watched(PLAYER_IFACE),
            ["PlaybackStatus", "Rate", "Metadata"]
        );
        assert_eq!(watched(ROOT_IFACE), ["Identity"]);
        assert!(watched("org.mpris.MediaPlayer2.TrackList").is_empty());
    }

    // moves_position.

    #[test]
    fn status_or_track_reports_call_for_a_position_read() {
        let at = Instant::now();
        let status = change(Some(PlaybackStatus::Paused), None, None, None, at);
        let track = change(None, None, Some(Track::default()), None, at);
        assert!(moves_position(&status));
        assert!(moves_position(&track));
    }

    #[test]
    fn other_reports_do_not_call_for_a_position_read() {
        let at = Instant::now();
        let events = [
            empty_change(at),
            change(None, Some(2.0), None, None, at),
            change(None, None, None, Some("mpv"), at),
            Event::Vanished {
                bus_name: BUS.into(),
            },
            Event::PositionRead {
                unique_name: OWNER.into(),
                position: Duration::ZERO,
                at,
            },
        ];
        for event in events {
            assert!(!moves_position(&event), "{event:?}");
        }
    }

    // read_metadata.

    #[test]
    fn metadata_is_read_from_a_dictionary_wrapped_or_not() {
        let expected = titled("Title");
        let plain = read_metadata(&metadata_value("Title")).map(|m| parse_track(&m));
        assert_eq!(plain, Some(expected.clone()));
        let wrapped = Value::Value(Box::new(metadata_value("Title")));
        let unwrapped = read_metadata(&wrapped).map(|m| parse_track(&m));
        assert_eq!(unwrapped, Some(expected));
    }

    #[test]
    fn empty_metadata_is_an_empty_track() {
        let empty = Value::from(Props::new());
        assert_eq!(read_metadata(&empty).map(|m| m.len()), Some(0));
    }

    #[test]
    fn metadata_of_another_type_is_refused() {
        let cases = [
            Value::from("title"),
            Value::from(vec!["title"]),
            Value::from(42i64),
        ];
        for value in cases {
            assert!(read_metadata(&value).is_none(), "{value:?}");
        }
    }
}
