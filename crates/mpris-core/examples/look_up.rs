//! Looks one cover up online, as a source would for a track whose own
//! cover is missing (B-18), and prints each request and the outcome.
//!
//! `cargo run --example look_up -- <service> <title> <artist> [album]`, or
//! `cargo run --example look_up -- youtube <page url>`, the service being
//! `musicbrainz`, `wikipedia` or `youtube`. It also runs in the sandbox of
//! OBS Flatpak (`flatpak run --command=<absolute path of the example>
//! com.obsproject.Studio <arguments>`), to check TLS where OBS runs.

use std::process;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mpris_core::model::{Service, Settings, Track};
use mpris_core::network::look_up_online;
use mpris_core::online::{decode, request};
use mpris_core::settings::SettingValue;

/// Prints the requests of the network module.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info || metadata.target().starts_with("mpris_core::network")
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

const USAGE: &str = "usage: look_up <musicbrainz|wikipedia> <title> <artist> [album]\n       look_up youtube <page url>";

fn main() {
    let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Debug));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (service, track) = match args.as_slice() {
        [service, page] => (
            service_named(service),
            Track {
                url: Some(page.clone()),
                ..Track::default()
            },
        ),
        [service, title, artist, album @ ..] if album.len() <= 1 => (
            service_named(service),
            Track {
                title: Some(title.clone()),
                artists: vec![artist.clone()],
                album: album.first().cloned(),
                ..Track::default()
            },
        ),
        _ => fail(USAGE),
    };
    let Some(request) = request(service, &track) else {
        fail("nothing to look up: no artist, or no YouTube page");
    };
    let started = Instant::now();
    let found = look_up_online(&request, &AtomicBool::new(false));
    let elapsed = started.elapsed();
    match found.map(|cover| decode(&request, &cover)) {
        Some(Ok(image)) => println!(
            "found a {}x{} cover in {elapsed:?}",
            image.width, image.height
        ),
        Some(Err(error)) => println!("found no image but {error}, after {elapsed:?}"),
        None => println!("no cover, after {elapsed:?}"),
    }
}

/// The service named as the setting stores it, read the way a source reads
/// its setting.
fn service_named(name: &str) -> Service {
    let stored = |key: &str| (key == "art_online").then(|| SettingValue::Text(name.to_owned()));
    Settings::from_lookup(stored)
        .art_online
        .unwrap_or_else(|| fail(USAGE))
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    process::exit(2)
}
