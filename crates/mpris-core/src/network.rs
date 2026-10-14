//! The network thread (D-08): it runs the online lookups the core thread
//! hands it, one at a time, and sends each outcome back to the core loop,
//! which applies it to the store as an event. It never touches the store,
//! and it starts with the first lookup, never before (B-48).
//!
//! Requests go through `ureq`, blocking, with TLS by rustls and the root
//! certificates of `webpki-roots` built in, so that they work the same in
//! the Flatpak sandbox, whose own certificate store differs from the
//! host's. Every failure, of the network or of a service, is a lookup
//! that found nothing, which the store keeps for the session (B-18).

use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};

use crate::art::MAX_FILE_BYTES;
use crate::online::{self, Request};

/// Where the plugin can be reached, which MusicBrainz asks every client
/// to give in its User-Agent. A placeholder until the forge is chosen
/// (phase 7 of the roadmap): replace it with the address of the
/// repository.
const REPOSITORY: &str = "https://forge.invalid/obs-dbus-mpris";

/// A request slower than this counts as failed, so that one lookup never
/// holds the next ones for long. The slowest request of the study took
/// 4.1 s, the Cover Art Archive's redirect to the Internet Archive
/// included (study, section 8).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Between two requests to one host. MusicBrainz allows one per second on
/// average, and still answered 503 at 1.2 s in the study, where one retry
/// was enough.
const MIN_INTERVAL: Duration = Duration::from_millis(1100);
const RETRY_DELAY: Duration = Duration::from_secs(2);
/// The longest wait a service may ask for before a retry, beyond which the
/// lookup gives up rather than hold the others up.
const MAX_RETRY_DELAY: Duration = Duration::from_secs(10);

/// How often a wait checks whether the core is stopping.
const STOP_CHECK: Duration = Duration::from_millis(100);

/// What runs one lookup and answers the cover as downloaded:
/// [`look_up_online`] in the plugin, a stand-in in tests, which must never
/// reach the network. It gives up early once the flag is set.
pub type LookUp = fn(&Request, &AtomicBool) -> Option<Vec<u8>>;

/// The outcome of a lookup, as the network thread sends it back. The core
/// decodes the cover when a track shows it.
pub type Looked = (Request, Option<Arc<[u8]>>);

/// The core thread's handle on the network thread, which starts on the
/// first request (B-48). Dropping it stops the thread at its next check,
/// without waiting for it: a request in flight lasts until its timeout,
/// and the thread holds no player state.
pub struct Network {
    look_up: LookUp,
    results: Sender<Looked>,
    thread: Thread,
}

enum Thread {
    NotStarted,
    Running {
        requests: Sender<Request>,
        stopped: Arc<AtomicBool>,
    },
    /// The system refused a thread, so every lookup fails.
    Failed,
}

impl Network {
    /// A handle that sends outcomes to `results`, the network thread not
    /// started yet.
    pub fn new(look_up: LookUp, results: Sender<Looked>) -> Network {
        Network {
            look_up,
            results,
            thread: Thread::NotStarted,
        }
    }

    /// Hands `request` to the network thread, started on the first call.
    /// Its outcome comes back on the results channel, failures included.
    pub fn send(&mut self, request: Request) {
        if matches!(self.thread, Thread::NotStarted) {
            self.thread = self.start();
        }
        let unsent = match &self.thread {
            Thread::Running { requests, .. } => {
                requests.try_send(request).err().map(|e| e.into_inner())
            }
            Thread::NotStarted | Thread::Failed => Some(request),
        };
        if let Some(request) = unsent {
            let _ = self.results.try_send((request, None));
        }
    }

    fn start(&self) -> Thread {
        let (requests, received) = async_channel::unbounded();
        let stopped = Arc::new(AtomicBool::new(false));
        let spawned = thread::Builder::new().name("mpris-online".into()).spawn({
            let (results, stopped, look_up) = (self.results.clone(), stopped.clone(), self.look_up);
            move || serve(&received, &results, &stopped, look_up)
        });
        match spawned {
            Ok(_) => Thread::Running { requests, stopped },
            Err(error) => {
                log::error!(
                    "MPRIS: cannot start the network thread, no cover is looked up: {error}"
                );
                Thread::Failed
            }
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        // The channel closes with the sender, which ends the wait for the
        // next request. The flag ends the lookup in progress sooner.
        if let Thread::Running { stopped, .. } = &self.thread {
            stopped.store(true, Ordering::Relaxed);
        }
    }
}

/// Body of the network thread: one lookup after the other, until the core
/// stops. A panic fails that lookup alone (B-42).
fn serve(
    requests: &Receiver<Request>,
    results: &Sender<Looked>,
    stopped: &AtomicBool,
    look_up: LookUp,
) {
    while let Ok(request) = requests.recv_blocking() {
        let cover = panic::catch_unwind(AssertUnwindSafe(|| look_up(&request, stopped)))
            .unwrap_or_else(|_| {
                log::error!("MPRIS: an online lookup panicked, the cover counts as not found");
                None
            });
        match &cover {
            Some(cover) => log::debug!("MPRIS: {request:?} found {} bytes", cover.len()),
            None => log::debug!("MPRIS: {request:?} found no cover"),
        }
        if results
            .send_blocking((request, cover.map(Arc::from)))
            .is_err()
        {
            break;
        }
    }
}

thread_local! {
    /// One client per network thread, which reuses its connections, and
    /// the time of the last request to each host.
    static HTTP: RefCell<Http> = RefCell::new(Http::new());
}

/// Runs `request` against the real services.
pub fn look_up_online(request: &Request, stopped: &AtomicBool) -> Option<Vec<u8>> {
    HTTP.with_borrow_mut(|http| online::look_up(request, |url| http.get(url, stopped)))
}

struct Http {
    agent: ureq::Agent,
    last: HashMap<String, Instant>,
}

impl Http {
    fn new() -> Http {
        let config = ureq::Agent::config_builder()
            .user_agent(user_agent())
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(REQUEST_TIMEOUT))
            // Statuses are read here, to retry a busy service.
            .http_status_as_error(false)
            .build();
        Http {
            agent: config.into(),
            last: HashMap::new(),
        }
    }

    /// The body of a successful GET of `url`, `None` for anything else,
    /// after one retry when the service is busy. A network error or a
    /// timeout is not retried: it has already cost its wait.
    fn get(&mut self, url: &str, stopped: &AtomicBool) -> Option<Vec<u8>> {
        let host = host(url)?.to_owned();
        let mut retried = false;
        loop {
            let wait = wait_before(self.last.get(&host).copied(), Instant::now());
            if !pause(wait, stopped) {
                return None;
            }
            self.last.insert(host.clone(), Instant::now());
            let started = Instant::now();
            let mut response = self
                .agent
                .get(url)
                .call()
                .inspect_err(|error| log::debug!("MPRIS: GET {url} failed: {error}"))
                .ok()?;
            let status = response.status().as_u16();
            log::debug!("MPRIS: GET {url}: {status} in {:?}", started.elapsed());
            if status == 200 {
                return response
                    .body_mut()
                    .with_config()
                    .limit(MAX_FILE_BYTES)
                    .read_to_vec()
                    .inspect_err(|error| log::debug!("MPRIS: GET {url}: {error}"))
                    .ok()
                    .filter(|body| !body.is_empty());
            }
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok());
            match retry_delay(status, retry_after) {
                Some(delay) if !retried => {
                    retried = true;
                    if !pause(delay, stopped) {
                        return None;
                    }
                }
                _ => return None,
            }
        }
    }
}

/// Waits `duration`, `false` as soon as the core stops.
fn pause(duration: Duration, stopped: &AtomicBool) -> bool {
    let end = Instant::now() + duration;
    loop {
        if stopped.load(Ordering::Relaxed) {
            return false;
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return true;
        }
        thread::sleep(left.min(STOP_CHECK));
    }
}

/// The plugin, its version and where to reach its authors, never anything
/// about the user.
fn user_agent() -> String {
    format!(
        "obs-dbus-mpris/{} ( {REPOSITORY} )",
        env!("CARGO_PKG_VERSION")
    )
}

/// The host `url` points to, without its port.
fn host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// How long to wait before the next request to a host last asked at
/// `last`.
fn wait_before(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |last| {
        MIN_INTERVAL.saturating_sub(now.saturating_duration_since(last))
    })
}

/// When to try again after `status`, `None` for never: only a busy or
/// failing service gets a second chance, after the delay it asks for in
/// seconds, if it asks for a reasonable one.
fn retry_delay(status: u16, retry_after: Option<&str>) -> Option<Duration> {
    if !matches!(status, 429 | 500 | 502 | 503 | 504) {
        return None;
    }
    match retry_after.and_then(|value| value.trim().parse::<u64>().ok()) {
        Some(seconds) => {
            let asked = Duration::from_secs(seconds);
            (asked <= MAX_RETRY_DELAY).then_some(asked)
        }
        None => Some(RETRY_DELAY),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MusicBrainz asks for an application, its version and a contact.
    #[test]
    fn the_user_agent_names_the_plugin_its_version_and_where_to_reach_it() {
        let agent = user_agent();
        let version = env!("CARGO_PKG_VERSION");
        assert!(
            agent.starts_with(&format!("obs-dbus-mpris/{version} ( https://")),
            "{agent}"
        );
        assert!(agent.ends_with(" )"), "{agent}");
    }

    #[test]
    fn requests_are_paced_by_host() {
        assert_eq!(
            host("https://musicbrainz.org/ws/2/recording?query=x"),
            Some("musicbrainz.org")
        );
        assert_eq!(
            host("https://coverartarchive.org/release-group/x/front-500"),
            Some("coverartarchive.org")
        );
        assert_eq!(host("https://i.ytimg.com"), Some("i.ytimg.com"));
        assert_eq!(
            host("https://en.wikipedia.org:443/w/api.php"),
            Some("en.wikipedia.org")
        );
        assert_eq!(host("musicbrainz.org/ws/2"), None);
        assert_eq!(host("https:///path"), None);
    }

    /// Over one second between two requests to a host, for MusicBrainz's
    /// limit of one per second, shared by the whole plugin since it has a
    /// single network thread (D-09).
    #[test]
    fn a_host_waits_a_little_over_a_second_between_requests() {
        let now = Instant::now();
        assert_eq!(wait_before(None, now), Duration::ZERO);
        assert_eq!(wait_before(Some(now), now), Duration::from_millis(1100));
        let earlier = now - Duration::from_millis(400);
        assert_eq!(wait_before(Some(earlier), now), Duration::from_millis(700));
        let long_ago = now - Duration::from_secs(5);
        assert_eq!(wait_before(Some(long_ago), now), Duration::ZERO);
    }

    #[test]
    fn a_busy_service_is_tried_again_once_after_two_seconds() {
        for status in [429, 500, 502, 503, 504] {
            assert_eq!(
                retry_delay(status, None),
                Some(Duration::from_secs(2)),
                "{status}"
            );
        }
    }

    #[test]
    fn a_retry_follows_the_delay_the_service_asks_within_reason() {
        assert_eq!(retry_delay(503, Some("1")), Some(Duration::from_secs(1)));
        assert_eq!(retry_delay(503, Some(" 5 ")), Some(Duration::from_secs(5)));
        assert_eq!(retry_delay(429, Some("10")), Some(Duration::from_secs(10)));
        // Longer than a lookup is worth waiting: given up, never retried early.
        assert_eq!(retry_delay(503, Some("11")), None);
        assert_eq!(retry_delay(503, Some("3600")), None);
        // An HTTP date or garbage falls back to the usual delay.
        let date = "Wed, 21 Oct 2015 07:28:00 GMT";
        assert_eq!(retry_delay(503, Some(date)), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(503, Some("-1")), Some(Duration::from_secs(2)));
    }

    #[test]
    fn other_answers_are_never_tried_again() {
        for status in [200, 204, 301, 400, 401, 403, 404, 410, 501] {
            assert_eq!(retry_delay(status, None), None, "{status}");
            assert_eq!(retry_delay(status, Some("1")), None, "{status}");
        }
    }
}
