//! The network thread's life. A binary of its own, so no other test adds
//! threads to the process while this one counts them.

mod common;

use std::sync::atomic::AtomicBool;
use std::thread;
use std::time::Duration;

use common::{
    Fake, FakePlayer, PrivateBus, count, find, thread_names, track, wait_for, wait_until,
};
use mpris_core::dbus::Core;
use mpris_core::model::{ArtImage, Service};
use mpris_core::online::{Request, Wish};

const FAKE: &str = "org.mpris.MediaPlayer2.fake.instance1";
const THREAD: &str = "mpris-online";

/// Finds a cover for anything, without network.
fn finds(_request: &Request, _stopped: &AtomicBool) -> Option<ArtImage> {
    Some(ArtImage {
        width: 1,
        height: 1,
        rgba: vec![9; 4],
    })
}

/// B-48: the thread waits for the first lookup, and stops with the core.
#[test]
fn the_network_thread_starts_with_the_first_lookup_and_ends_with_the_core() {
    let bus = PrivateBus::start();
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new("Fake Player", "Playing", track("/fake/1", "Title", None)),
    );
    let core = Core::start_with(&bus.address, finds);
    wait_for(&core, "the fake player", |s| find(s, FAKE).is_some());
    let mut wish = core.art_wish();
    thread::sleep(Duration::from_millis(100));
    assert_eq!(count(THREAD), 0, "{:?}", thread_names());

    wish.set(Some(Wish {
        bus_name: FAKE.into(),
        service: Service::MusicBrainz,
    }));
    wait_for(&core, "the cover found online", |s| {
        find(s, FAKE).is_some_and(|p| !p.online.is_empty())
    });
    assert_eq!(count(THREAD), 1, "{:?}", thread_names());

    drop(core);
    wait_until("the network thread to end", || count(THREAD) == 0);
}
