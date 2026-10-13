//! The D-Bus adapter against fake MPRIS players on a private bus.

mod common;

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use common::{
    Fake, FakePlayer, HungFile, PrivateBus, find, metadata, own_name, png_data_url, secs, track,
    wait_for, wait_until,
};
use mpris_core::dbus::Core;
use mpris_core::model::{Art, PlaybackStatus, PlayerKey};
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
