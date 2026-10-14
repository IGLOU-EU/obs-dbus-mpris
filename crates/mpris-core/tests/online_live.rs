//! The study's tracks against the real services, for a manual run only:
//!
//! `cargo test -p mpris-core --test online_live -- --ignored --nocapture`
//!
//! Some twenty requests, paced as the plugin paces them. The answers follow
//! what the services hold that day, so a failure here calls for a look at
//! what they answered, which the run prints, before any change of code.

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mpris_core::model::{Service, Track};
use mpris_core::network::look_up_online;
use mpris_core::online::{decode, request};

/// Prints the requests of the network module.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.target().starts_with("mpris_core::network")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("    {}", record.args());
        }
    }

    fn flush(&self) {}
}

fn track(title: &str, artist: &str, album: &str) -> Track {
    let text = |text: &str| (!text.is_empty()).then(|| text.to_owned());
    Track {
        title: text(title),
        artists: text(artist).into_iter().collect(),
        album: text(album),
        ..Track::default()
    }
}

#[test]
#[ignore = "queries MusicBrainz, the Cover Art Archive, Wikipedia and YouTube"]
fn the_study_tracks_against_the_real_services() {
    let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Debug));
    let school = track("School", "Supertramp", "Crime Of The Century (Remastered)");
    let back_in_black = track("Back In Black", "AC/DC", "Back In Black");
    let harmonium = track("Vieilles courroies (Remastered)", "Harmonium", "");
    let bebop = track(
        "Cowboy Bebop (Original Soundtrack) by Seatbelts - 1998",
        "S U N D A Y",
        "",
    );
    let stargate = track(
        "Stargate Atlantis - S1:E17 - Derniers messages (2005)",
        "",
        "",
    );
    let video = Track {
        url: Some("https://www.youtube.com/watch?v=jNQXAC9IVRw".into()),
        ..track("Me at the zoo", "jawed", "")
    };
    // Whether a cover is expected, as the study found.
    let cases = [
        (Service::MusicBrainz, &school, true),
        (Service::MusicBrainz, &back_in_black, true),
        (Service::MusicBrainz, &harmonium, true),
        (Service::MusicBrainz, &bebop, false),
        (Service::MusicBrainz, &stargate, false),
        (Service::Wikipedia, &school, true),
        (Service::Wikipedia, &back_in_black, true),
        (Service::Wikipedia, &harmonium, false),
        (Service::Wikipedia, &bebop, false),
        (Service::Wikipedia, &stargate, false),
        (Service::YouTube, &video, true),
    ];
    let running = AtomicBool::new(false);
    let mut surprises = Vec::new();
    for (service, track, expected) in cases {
        let title = track.title.as_deref().unwrap_or_default();
        let Some(request) = request(service, track) else {
            eprintln!("{service:?} {title:?}: nothing to ask");
            surprises.push((service, title.to_owned()));
            continue;
        };
        eprintln!("{service:?} {title:?}");
        let started = Instant::now();
        let found = look_up_online(&request, &running);
        let image = found.and_then(|cover| decode(&request, &cover).ok());
        let outcome = image.map_or("no cover".to_owned(), |image| {
            format!("a {}x{} cover", image.width, image.height)
        });
        eprintln!("  => {outcome} in {:?}", started.elapsed());
        if (outcome != "no cover") != expected {
            surprises.push((service, title.to_owned()));
        }
    }
    assert!(surprises.is_empty(), "unexpected outcomes: {surprises:?}");
}
