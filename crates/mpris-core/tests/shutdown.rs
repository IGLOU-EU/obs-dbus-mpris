//! Stopping the core. A binary of its own, so no other test adds threads
//! to the process while this one counts them.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{Fake, FakePlayer, PrivateBus, find, secs, track, wait_for};
use mpris_core::dbus::Core;

const FAKE: &str = "org.mpris.MediaPlayer2.fake.instance1";

/// Names of the threads of this process, as the kernel reports them
/// (truncated to 15 bytes).
fn thread_names() -> Vec<String> {
    fs::read_dir("/proc/self/task")
        .expect("procfs")
        .filter_map(|task| fs::read_to_string(task.ok()?.path().join("comm")).ok())
        .map(|name| name.trim_end().to_owned())
        .collect()
}

fn count(prefix: &str) -> usize {
    thread_names()
        .iter()
        .filter(|name| name.starts_with(prefix))
        .count()
}

#[test]
fn drop_stops_the_core_promptly_and_leaves_no_thread() {
    let bus = PrivateBus::start();
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake {
            position: secs(1),
            ..Fake::new("Fake Player", "Playing", track("/fake/1", "Title", None))
        },
    );
    assert_eq!(count("mpris-core"), 0);

    let core = Core::start_at(&bus.address);
    wait_for(&core, "the fake player", |s| find(s, FAKE).is_some());
    assert_eq!(count("mpris-core"), 1, "one core thread");
    // Only the fake player's executor: the core drives its own connection.
    // Counted once the fake has answered the core, since a thread names
    // itself only once it runs.
    assert_eq!(count("zbus::"), 1, "{:?}", thread_names());

    let start = Instant::now();
    drop(core);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "drop took {elapsed:?}"
    );
    assert_eq!(count("mpris-core"), 0, "{:?}", thread_names());
    assert_eq!(count("zbus::"), 1, "{:?}", thread_names());
}
