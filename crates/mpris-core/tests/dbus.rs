//! The D-Bus adapter against fake MPRIS players on a private bus.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use common::{
    Fake, FakePlayer, HungFile, PrivateBus, find, metadata, own_name, png, png_data_url, secs,
    track, wait_for, wait_until,
};
use mpris_core::dbus::Core;
use mpris_core::model::{Art, PlaybackStatus, PlayerKey, Service};
use mpris_core::online::{Request, Wish};
use zbus::zvariant::Value;

const FAKE: &str = "org.mpris.MediaPlayer2.fake.instance1";

fn playing(position: Duration) -> Fake {
    Fake {
        position,
        ..Fake::new(
            "Fake Player",
            "Playing",
            track("/fake/1", "Title one", None),
        )
    }
}

fn paused(position: Duration) -> Fake {
    Fake {
        position,
        ..Fake::new("Fake Player", "Paused", track("/fake/1", "Title one", None))
    }
}

/// Waits until the core shows `bus_name`, so later checks see a known player.
fn discovered(core: &Core, bus_name: &str) {
    wait_for(core, &format!("{bus_name} to appear"), |s| {
        find(s, bus_name).is_some()
    });
}

#[test]
fn discovers_a_player_present_before_start() {
    let bus = PrivateBus::start();
    let _other = own_name(&bus, "org.example.NotAPlayer");
    let fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake {
            rate: 1.5,
            ..playing(secs(5))
        },
    );
    let before = Instant::now();
    let core = Core::start_at(&bus.address);

    let snapshot = wait_for(&core, "the fake player", |s| find(s, FAKE).is_some());
    assert_eq!(snapshot.players.len(), 1, "only MPRIS names are players");
    let player = &snapshot.players[0];
    assert_eq!(player.unique_name, fake.unique_name());
    assert_eq!(player.key, PlayerKey("fake".into()));
    assert_eq!(player.identity, "Fake Player");
    assert_eq!(player.status, PlaybackStatus::Playing);
    assert_eq!(player.rate, 1.5);
    assert_eq!(player.track.id.as_deref(), Some("/fake/1"));
    assert_eq!(player.track.title.as_deref(), Some("Title one"));
    assert_eq!(player.track.artists, ["CRYSTAL KING"]);
    assert_eq!(player.track.album.as_deref(), Some("愛をとりもどせ!!"));
    assert_eq!(
        player.track.length,
        Some(Duration::from_micros(219_533_000))
    );
    assert_eq!(player.art, Art::None);
    assert_eq!(player.anchor.position, secs(5));
    assert!(
        player.anchor.at >= before,
        "the anchor is taken at read time"
    );
}

#[test]
fn discovers_a_player_appearing_after_start() {
    let bus = PrivateBus::start();
    let early_name = "org.mpris.MediaPlayer2.early";
    let _early = FakePlayer::start(&bus, early_name, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    // The first scan publishes only once it has read every name, so the
    // late player can only come by NameOwnerChanged.
    discovered(&core, early_name);

    let fake = FakePlayer::start(&bus, FAKE, paused(secs(12)));
    let snapshot = wait_for(&core, "the late player", |s| find(s, FAKE).is_some());
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.unique_name, fake.unique_name());
    assert_eq!(player.status, PlaybackStatus::Paused);
    assert_eq!(player.identity, "Fake Player");
    assert_eq!(player.anchor.position, secs(12));
}

#[test]
fn forgets_a_player_that_quits() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.quit();
    wait_for(&core, "the player to vanish", |s| s.players.is_empty());
}

#[test]
fn relaunch_keeps_the_key_with_the_new_bus_name() {
    let bus = PrivateBus::start();
    let first = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    first.quit();
    let relaunched_name = "org.mpris.MediaPlayer2.fake.instance2";
    let second = FakePlayer::start(&bus, relaunched_name, playing(secs(3)));
    let snapshot = wait_for(&core, "only the relaunched instance", |s| {
        s.players.len() == 1 && find(s, relaunched_name).is_some()
    });
    let player = &snapshot.players[0];
    assert_eq!(player.key, PlayerKey("fake".into()));
    assert_eq!(player.unique_name, second.unique_name());
    assert_eq!(player.status, PlaybackStatus::Playing);
}

#[test]
fn metadata_change_updates_the_track_and_rereads_the_position() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(30)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // Only the player knows where the new track starts.
    fake.set_position(secs(7));
    fake.set_metadata(track("/fake/2", "Title two", None));
    let snapshot = wait_for(&core, "the new track at the re-read position", |s| {
        find(s, FAKE).is_some_and(|p| {
            p.track.title.as_deref() == Some("Title two") && p.anchor.position == secs(7)
        })
    });
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.track.id.as_deref(), Some("/fake/2"));
    assert_eq!(player.status, PlaybackStatus::Paused);
}

#[test]
fn same_metadata_sent_again_rereads_the_position() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(30)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // A track restarted on repeat: nothing changes but the position.
    fake.set_position(secs(2));
    fake.set_metadata(track("/fake/1", "Title one", None));
    wait_for(&core, "the restarted position", |s| {
        find(s, FAKE).is_some_and(|p| p.anchor.position == secs(2))
    });
}

#[test]
fn same_status_sent_again_rereads_the_position() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.set_position(secs(20));
    fake.set_status("Paused");
    wait_for(&core, "the re-read position", |s| {
        find(s, FAKE).is_some_and(|p| p.anchor.position == secs(20))
    });
}

#[test]
fn metadata_of_the_wrong_type_keeps_the_track() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // A broken Metadata in the same signal must not hide a valid status.
    fake.emit_raw(
        vec![
            ("Metadata", Value::from("not a dictionary")),
            ("PlaybackStatus", Value::from("Playing")),
        ],
        &[],
    );
    let snapshot = wait_for(&core, "the status", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Playing)
    });
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.track.title.as_deref(), Some("Title one"));
}

#[test]
fn invalidated_metadata_is_read_again() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.invalidate_metadata(track("/fake/3", "Title three", None));
    wait_for(&core, "the invalidated track", |s| {
        find(s, FAKE).is_some_and(|p| p.track.title.as_deref() == Some("Title three"))
    });
}

#[test]
fn identity_change_is_applied() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.set_identity("Renamed Player");
    wait_for(&core, "the new identity", |s| {
        find(s, FAKE).is_some_and(|p| p.identity == "Renamed Player")
    });
}

#[test]
fn pause_and_resume_update_status_and_anchor() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, playing(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // The status and the position re-read on it are published together, so
    // the position is checked on the first snapshot with the new status: a
    // later periodic read cannot stand in for the re-read.
    fake.set_position(secs(42));
    fake.set_status("Paused");
    let snapshot = wait_for(&core, "the pause", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Paused)
    });
    assert_eq!(find(&snapshot, FAKE).unwrap().anchor.position, secs(42));

    fake.set_position(secs(50));
    let resumed_at = Instant::now();
    fake.set_status("Playing");
    let snapshot = wait_for(&core, "the resume", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Playing)
    });
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.anchor.position, secs(50));
    assert!(player.anchor.at >= resumed_at);
}

#[test]
fn seeked_moves_the_anchor() {
    let bus = PrivateBus::start();
    // Paused, so no periodic read can be what moves the anchor.
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.seek(secs(100));
    wait_for(&core, "the seeked position", |s| {
        find(s, FAKE).is_some_and(|p| p.anchor.position == secs(100))
    });
}

#[test]
fn signals_of_a_player_apply_in_the_order_it_sent_them() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // Decoding this cover keeps the loop busy far longer than the next two
    // signals take to queue up behind it, whatever order it reads them in.
    let cover = png_data_url(1500, 1500);
    fake.set_metadata(track("/fake/2", "Title two", Some(&cover)));
    fake.seek(secs(30));
    fake.set_position(Duration::ZERO);
    fake.set_metadata(track("/fake/3", "Title three", None));

    wait_for(&core, "the third track", |s| {
        find(s, FAKE).is_some_and(|p| p.track.title.as_deref() == Some("Title three"))
    });
    // A Seeked handled after the track that followed it lands right after,
    // and nothing else moves the anchor of a paused player.
    thread::sleep(Duration::from_millis(200));
    let snapshot = core.snapshot();
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.track.title.as_deref(), Some("Title three"));
    assert_eq!(player.anchor.position, Duration::ZERO);
}

#[test]
fn playing_position_is_read_again_without_seeked() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, playing(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    // A player that jumps without sending Seeked (D-05).
    fake.set_position(secs(300));
    wait_for(&core, "the periodic read", |s| {
        find(s, FAKE).is_some_and(|p| p.anchor.position == secs(300))
    });
}

#[test]
fn paused_position_is_not_polled() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(10)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    fake.set_position(secs(300));
    thread::sleep(Duration::from_millis(2500));
    let snapshot = core.snapshot();
    assert_eq!(find(&snapshot, FAKE).unwrap().anchor.position, secs(10));
}

#[test]
fn png_data_url_art_becomes_ready() {
    let bus = PrivateBus::start();
    let url = png_data_url(2, 3);
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Playing",
            track("/fake/1", "Title one", Some(&url)),
        ),
    );
    let core = Core::start_at(&bus.address);

    let snapshot = wait_for(&core, "the decoded art", |s| {
        find(s, FAKE).is_some_and(|p| matches!(p.art, Art::Ready(_)))
    });
    let Art::Ready(image) = &find(&snapshot, FAKE).unwrap().art else {
        unreachable!()
    };
    assert_eq!((image.width, image.height), (2, 3));
    assert_eq!(image.rgba.len(), 2 * 3 * 4);
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 255]);
}

#[test]
fn art_of_a_new_track_is_loaded_too() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    let url = png_data_url(4, 4);
    fake.set_metadata(track("/fake/2", "Title two", Some(&url)));
    wait_for(&core, "the art of the new track", |s| {
        find(s, FAKE).is_some_and(|p| matches!(&p.art, Art::Ready(image) if image.width == 4))
    });
}

#[test]
fn unreadable_art_url_becomes_failed() {
    let bus = PrivateBus::start();
    let url = "file:///nonexistent/obs-dbus-mpris/cover.png";
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Playing",
            track("/fake/1", "Title one", Some(url)),
        ),
    );
    let core = Core::start_at(&bus.address);

    wait_for(&core, "the failed art", |s| {
        find(s, FAKE).is_some_and(|p| p.art == Art::Failed)
    });
}

#[test]
fn two_players_are_watched_at_once() {
    let bus = PrivateBus::start();
    let alpha_name = "org.mpris.MediaPlayer2.alpha";
    let beta_name = "org.mpris.MediaPlayer2.beta.instance7";
    let alpha = FakePlayer::start(
        &bus,
        alpha_name,
        Fake::new("Alpha", "Playing", track("/a/1", "Alpha song", None)),
    );
    let _beta = FakePlayer::start(
        &bus,
        beta_name,
        Fake::new("Beta", "Paused", track("/b/1", "Beta song", None)),
    );
    let core = Core::start_at(&bus.address);
    wait_for(&core, "both players", |s| s.players.len() == 2);

    alpha.set_status("Stopped");
    let snapshot = wait_for(&core, "alpha to stop", |s| {
        find(s, alpha_name).is_some_and(|p| p.status == PlaybackStatus::Stopped)
    });
    let beta = find(&snapshot, beta_name).unwrap();
    assert_eq!(beta.key, PlayerKey("beta".into()));
    assert_eq!(beta.identity, "Beta");
    assert_eq!(beta.status, PlaybackStatus::Paused);
    assert_eq!(beta.track.title.as_deref(), Some("Beta song"));
    let alpha = find(&snapshot, alpha_name).unwrap();
    assert_eq!(alpha.track.title.as_deref(), Some("Alpha song"));
}

#[test]
fn a_player_serving_nothing_appears_with_defaults() {
    let bus = PrivateBus::start();
    let _bare = own_name(&bus, "org.mpris.MediaPlayer2.bare");
    let core = Core::start_at(&bus.address);

    let snapshot = wait_for(&core, "the bare player", |s| {
        find(s, "org.mpris.MediaPlayer2.bare").is_some()
    });
    let player = &snapshot.players[0];
    assert_eq!(player.identity, "bare", "identity falls back to the key");
    assert_eq!(player.status, PlaybackStatus::Stopped);
    assert_eq!(player.rate, 1.0);
    assert_eq!(player.track, Default::default());
    assert_eq!(player.anchor.position, Duration::ZERO);
}

#[test]
fn a_hung_player_does_not_block_the_others() {
    let bus = PrivateBus::start();
    let hung_name = "org.mpris.MediaPlayer2.hung";
    let _hung = FakePlayer::start(
        &bus,
        hung_name,
        Fake {
            hang: true,
            ..playing(secs(5))
        },
    );
    let fake = FakePlayer::start(&bus, FAKE, paused(secs(0)));
    let core = Core::start_at(&bus.address);

    let snapshot = wait_for(&core, "both players", |s| s.players.len() == 2);
    let hung = find(&snapshot, hung_name).unwrap();
    assert_eq!(
        hung.identity, "Fake Player",
        "its root interface still answers"
    );
    assert_eq!(
        hung.status,
        PlaybackStatus::Stopped,
        "unanswered means unknown"
    );

    fake.set_status("Playing");
    wait_for(&core, "the other player to keep updating", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Playing)
    });
}

#[test]
fn drop_returns_quickly_while_a_call_hangs() {
    let bus = PrivateBus::start();
    let _hung = FakePlayer::start(
        &bus,
        "org.mpris.MediaPlayer2.hung",
        Fake {
            hang: true,
            ..playing(secs(5))
        },
    );
    let core = Core::start_at(&bus.address);
    // The first scan is now waiting on the hung player.
    thread::sleep(Duration::from_millis(300));

    let start = Instant::now();
    drop(core);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "drop took {elapsed:?}"
    );
}

#[test]
fn drop_returns_quickly_while_an_art_load_hangs() {
    let bus = PrivateBus::start();
    let mut cover = HungFile::new();
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Paused",
            track("/fake/1", "Title one", Some(&cover.url())),
        ),
    );
    let core = Core::start_at(&bus.address);
    wait_until("the core to open the cover", || cover.has_waiting_open());

    let (done, dropped) = mpsc::channel();
    let dropper = thread::spawn(move || {
        drop(core);
        let _ = done.send(());
    });
    let outcome = dropped.recv_timeout(Duration::from_millis(300));
    // Lets a core still stuck in the load go on, so the test ends either way.
    cover.release();
    dropper.join().expect("drop does not panic");
    assert!(outcome.is_ok(), "drop waited for the art load");
}

#[test]
fn a_hung_art_load_gives_up_and_the_loop_moves_on() {
    let bus = PrivateBus::start();
    let mut cover = HungFile::new();
    let fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Paused",
            track("/fake/1", "Title one", Some(&cover.url())),
        ),
    );
    let core = Core::start_at(&bus.address);
    wait_until("the core to open the cover", || cover.has_waiting_open());

    wait_for(&core, "the hung cover to count as missing", |s| {
        find(s, FAKE).is_some_and(|p| p.art == Art::Failed)
    });
    fake.set_status("Playing");
    wait_for(&core, "the player to keep updating", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Playing)
    });
    cover.release();
}

#[test]
fn invalid_addresses_give_an_empty_snapshot() {
    for address in [
        "",
        "not a bus address",
        "unix:path=/nonexistent/obs-dbus-mpris/bus",
    ] {
        let core = Core::start_at(address);
        thread::sleep(Duration::from_millis(100));
        assert!(core.snapshot().players.is_empty(), "{address:?}");
        let start = Instant::now();
        drop(core);
        assert!(start.elapsed() < Duration::from_millis(300), "{address:?}");
    }
}

#[test]
fn losing_the_bus_empties_the_snapshot() {
    let mut bus = PrivateBus::start();
    let _fake = FakePlayer::start(&bus, FAKE, playing(secs(0)));
    let core = Core::start_at(&bus.address);
    discovered(&core, FAKE);

    bus.kill();
    wait_for(&core, "the snapshot to empty", |s| s.players.is_empty());
}

#[test]
fn metadata_entries_of_other_types_are_tolerated() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Paused",
            metadata(vec![
                ("xesam:title", Value::from("Loose types")),
                ("xesam:artist", Value::from("A single string")),
                ("mpris:length", Value::from(5_000_000u64)),
            ]),
        ),
    );
    let core = Core::start_at(&bus.address);

    let snapshot = wait_for(&core, "the loosely typed track", |s| {
        find(s, FAKE).is_some()
    });
    let player = find(&snapshot, FAKE).unwrap();
    assert_eq!(player.unique_name, fake.unique_name());
    assert_eq!(player.track.title.as_deref(), Some("Loose types"));
    assert_eq!(player.track.artists, ["A single string"]);
    assert_eq!(player.track.length, Some(secs(5)));
}

// Online covers (B-18), looked up by stand-ins that never reach the network.

/// Every request the stand-ins got, from every test of this binary, each
/// test telling its own by the title of its track.
static ASKED: Mutex<Vec<Request>> = Mutex::new(Vec::new());

fn asked(request: &Request) {
    ASKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(request.clone());
}

/// How many lookups asked for the track titled `title`.
fn asked_for(title: &str) -> usize {
    ASKED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter(|request| matches!(request, Request::MusicBrainz(query) if query.title == title))
        .count()
}

/// Finds a 2 by 2 cover for anything.
fn finds(request: &Request, _stopped: &AtomicBool) -> Option<Vec<u8>> {
    asked(request);
    Some(png(2, 2))
}

/// Finds something that is no image.
fn finds_garbage(request: &Request, _stopped: &AtomicBool) -> Option<Vec<u8>> {
    asked(request);
    Some(b"<html>Not found</html>".to_vec())
}

fn musicbrainz() -> Option<Wish> {
    Some(Wish {
        bus_name: FAKE.into(),
        service: Service::MusicBrainz,
    })
}

fn without_cover(title: &str) -> Fake {
    Fake::new("Fake Player", "Playing", track("/fake/1", title, None))
}

/// Waits until the core shows a cover found online for the track `title`.
fn shows_online(core: &Core, title: &str) {
    wait_for(core, &format!("the online cover of {title}"), |s| {
        find(s, FAKE).is_some_and(|p| {
            p.track.title.as_deref() == Some(title)
                && p.online
                    .iter()
                    .any(|(service, _)| *service == Service::MusicBrainz)
        })
    });
}

#[test]
fn a_wished_cover_is_looked_up_once_and_shown() {
    let bus = PrivateBus::start();
    let _fake = FakePlayer::start(&bus, FAKE, without_cover("Wished once"));
    let core = Core::start_with(&bus.address, finds);
    discovered(&core, FAKE);

    let mut first = core.art_wish();
    first.set(musicbrainz());
    shows_online(&core, "Wished once");
    // A second source asking the same shares the first lookup.
    let mut second = core.art_wish();
    second.set(musicbrainz());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(asked_for("Wished once"), 1);
}

/// Acceptance scenario 9: changing tracks then coming back asks nothing.
#[test]
fn coming_back_to_a_track_asks_nothing_more() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, without_cover("Going"));
    let core = Core::start_with(&bus.address, finds);
    discovered(&core, FAKE);
    let mut wish = core.art_wish();
    wish.set(musicbrainz());
    shows_online(&core, "Going");

    fake.set_metadata(track("/fake/2", "Elsewhere", None));
    shows_online(&core, "Elsewhere");
    fake.set_metadata(track("/fake/1", "Going", None));
    shows_online(&core, "Going");
    assert_eq!((asked_for("Going"), asked_for("Elsewhere")), (1, 1));
}

/// B-48: no source picked a service, so nothing leaves the machine.
#[test]
fn nothing_is_looked_up_without_a_wish() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, without_cover("Never wished"));
    let core = Core::start_with(&bus.address, finds);
    discovered(&core, FAKE);
    let mut wish = core.art_wish();
    wish.set(None);
    fake.set_status("Paused");
    wait_for(&core, "the pause", |s| {
        find(s, FAKE).is_some_and(|p| p.status == PlaybackStatus::Paused)
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(asked_for("Never wished"), 0);
    assert!(find(&core.snapshot(), FAKE).is_some_and(|p| p.online.is_empty()));
}

#[test]
fn a_withdrawn_wish_looks_nothing_more_up() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, without_cover("Wished then"));
    let core = Core::start_with(&bus.address, finds);
    discovered(&core, FAKE);
    let mut wish = core.art_wish();
    wish.set(musicbrainz());
    shows_online(&core, "Wished then");

    drop(wish);
    thread::sleep(Duration::from_millis(50));
    fake.set_metadata(track("/fake/2", "No longer", None));
    wait_for(&core, "the next track", |s| {
        find(s, FAKE).is_some_and(|p| p.track.title.as_deref() == Some("No longer"))
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(asked_for("No longer"), 0);
}

#[test]
fn a_cover_found_that_is_no_image_is_not_shown_nor_asked_again() {
    let bus = PrivateBus::start();
    let fake = FakePlayer::start(&bus, FAKE, without_cover("Garbage"));
    let core = Core::start_with(&bus.address, finds_garbage);
    discovered(&core, FAKE);
    let mut wish = core.art_wish();
    wish.set(musicbrainz());
    wait_until("the lookup", || asked_for("Garbage") == 1);

    fake.set_metadata(track("/fake/2", "After garbage", None));
    fake.set_metadata(track("/fake/1", "Garbage", None));
    wait_until("the next lookup", || asked_for("After garbage") == 1);
    wait_for(&core, "the track back", |s| {
        find(s, FAKE).is_some_and(|p| p.track.title.as_deref() == Some("Garbage"))
    });
    thread::sleep(Duration::from_millis(100));
    assert_eq!(asked_for("Garbage"), 1);
    assert!(find(&core.snapshot(), FAKE).is_some_and(|p| p.online.is_empty()));
}

#[test]
fn a_player_with_its_own_cover_is_not_looked_up() {
    let bus = PrivateBus::start();
    let cover = png_data_url(4, 4);
    let _fake = FakePlayer::start(
        &bus,
        FAKE,
        Fake::new(
            "Fake Player",
            "Playing",
            track("/fake/1", "Own cover", Some(&cover)),
        ),
    );
    let core = Core::start_with(&bus.address, finds);
    wait_for(&core, "the player's own cover", |s| {
        find(s, FAKE).is_some_and(|p| matches!(p.art, Art::Ready(_)))
    });
    let mut wish = core.art_wish();
    wish.set(musicbrainz());
    thread::sleep(Duration::from_millis(100));
    assert_eq!(asked_for("Own cover"), 0);
}

static HANGING: AtomicBool = AtomicBool::new(false);
static GAVE_UP: AtomicBool = AtomicBool::new(false);

/// A lookup stuck on a service that never answers, until the core stops.
fn hangs(request: &Request, stopped: &AtomicBool) -> Option<Vec<u8>> {
    asked(request);
    HANGING.store(true, Ordering::Relaxed);
    let end = Instant::now() + Duration::from_secs(10);
    while !stopped.load(Ordering::Relaxed) && Instant::now() < end {
        thread::sleep(Duration::from_millis(5));
    }
    GAVE_UP.store(true, Ordering::Relaxed);
    None
}

#[test]
fn drop_returns_quickly_while_a_lookup_hangs() {
    let bus = PrivateBus::start();
    let _fake = FakePlayer::start(&bus, FAKE, without_cover("Hanging"));
    let core = Core::start_with(&bus.address, hangs);
    discovered(&core, FAKE);
    let mut wish = core.art_wish();
    wish.set(musicbrainz());
    wait_until("the lookup to hang", || HANGING.load(Ordering::Relaxed));

    let start = Instant::now();
    drop(core);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "drop took {elapsed:?}"
    );
    wait_until("the lookup to give up", || GAVE_UP.load(Ordering::Relaxed));
}
