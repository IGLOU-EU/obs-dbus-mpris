//! Prints, once per second, what a source would show for the players of
//! the session bus and the entries of its player selector, to check the
//! core against real players without OBS.
//!
//! `cargo run --example watch [bus-name] [--strict] [--online <service>]`,
//! the bus name standing for the instance remembered in the source
//! settings (B-03). The `org.mpris.MediaPlayer2.` prefix may be left out,
//! so `watch mpv` follows any mpv and `watch mpv.instance-XYZ` that
//! instance while it runs. `--strict`, anywhere in the arguments, turns
//! automatic selection off (B-05), so only the exact remembered instance is
//! shown, never a fallback. `--online` looks a missing cover up with
//! `musicbrainz`, `wikipedia` or `youtube` (B-18), and prints each request.
//! Like a source, it keeps to the player it shows (B-08) for as long as it
//! runs.

use std::thread;
use std::time::{Duration, Instant};

use mpris_core::content::{Content, content};
use mpris_core::dbus::Core;
use mpris_core::model::{Art, Settings};
use mpris_core::online::wish;
use mpris_core::player::MPRIS_PREFIX;
use mpris_core::selection::{player_choices, select};
use mpris_core::settings::SettingValue;
use mpris_core::time::format_time;

/// Shows the core's own logs, so a missing bus is not mistaken for an
/// empty one, and its requests when it looks covers up online.
struct StderrLog {
    requests: bool,
}

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
            || (self.requests && metadata.target().starts_with("mpris_core::network"))
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

const STRICT: &str = "--strict";
const ONLINE: &str = "--online";

fn main() {
    let mut settings = Settings::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            STRICT => settings.auto_select = false,
            ONLINE => settings.art_online = Some(service(args.next())),
            _ => settings.player = Some(bus_name(arg)),
        }
    }
    let logger = StderrLog {
        requests: settings.art_online.is_some(),
    };
    // Lives as long as the program, as a logger must.
    let logger = Box::leak(Box::new(logger));
    let _ = log::set_logger(logger).map(|()| log::set_max_level(log::LevelFilter::Debug));
    let core = Core::start();
    let mut art_wish = core.art_wish();
    let mut followed = None;
    loop {
        thread::sleep(Duration::from_secs(1));
        let snapshot = core.snapshot();
        let selection = select(&snapshot.players, &settings, followed.as_deref());
        let player = selection.player;
        followed = selection.followed;
        art_wish.set(wish(player, &settings));
        match player {
            Some(p) => println!(
                "{} ({}) {:?}, {} player(s) on the bus",
                p.key.0,
                p.bus_name,
                p.status,
                snapshot.players.len()
            ),
            None => println!("no player"),
        }
        if let Some(url) = player.and_then(|p| p.track.url.as_deref()) {
            println!("  page    {url}");
        }
        let own_art = player.is_some_and(|p| matches!(p.art, Art::Ready(_)));
        print_content(&content(player, &settings, Instant::now()), own_art);
        for choice in player_choices(&snapshot.players, settings.player.as_deref()) {
            let unavailable = if choice.available {
                ""
            } else {
                " (unavailable)"
            };
            println!(
                "  choice  {} [{}]{unavailable}",
                choice.label, choice.bus_name
            );
        }
    }
}

/// The service named as the setting stores it, read the way a source reads
/// its setting.
fn service(name: Option<String>) -> mpris_core::model::Service {
    let name = name.unwrap_or_default();
    let stored = |key: &str| (key == "art_online").then(|| SettingValue::Text(name.clone()));
    Settings::from_lookup(stored).art_online.unwrap_or_else(|| {
        eprintln!("{ONLINE} takes musicbrainz, wikipedia or youtube, not {name:?}");
        std::process::exit(2)
    })
}

/// The short form saves typing the prefix shared by every MPRIS name.
fn bus_name(arg: String) -> String {
    if arg.starts_with(MPRIS_PREFIX) {
        arg
    } else {
        format!("{MPRIS_PREFIX}{arg}")
    }
}

fn print_content(content: &Content, own_art: bool) {
    match content {
        Content::Nothing => println!("  (transparent)"),
        Content::Message(text) => println!("  {text}"),
        Content::Track(track) => {
            let line = |label: &str, value: &Option<String>| {
                if let Some(value) = value {
                    println!("  {label:<8}{value}");
                }
            };
            line("title", &track.title);
            line("artist", &track.artist);
            line("album", &track.album);
            let position = track.position.map(format_time).unwrap_or_default();
            let duration = track
                .right_time
                .map(|time| format!(" / {}", time.text()))
                .unwrap_or_default();
            let progress = track
                .progress
                .map(|p| format!(" ({:.0}%)", p * 100.0))
                .unwrap_or_default();
            let paused = if track.paused { " [paused]" } else { "" };
            println!("  time    {position}{duration}{progress}{paused}");
            let origin = if own_art { "the player's" } else { "online" };
            match &track.art {
                Some(art) => println!("  art     {}x{}, {origin}", art.width, art.height),
                None => println!("  art     none"),
            }
        }
    }
}
