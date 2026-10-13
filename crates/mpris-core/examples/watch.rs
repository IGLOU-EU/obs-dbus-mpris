//! Prints, once per second, what a source would show for the players of
//! the session bus and the entries of its player selector, to check the
//! core against real players without OBS.
//!
//! `cargo run --example watch [bus-name] [--strict]`, the bus name standing
//! for the instance remembered in the source settings (B-03). The
//! `org.mpris.MediaPlayer2.` prefix may be left out, so `watch mpv` follows
//! any mpv and `watch mpv.instance-XYZ` that instance alone. `--strict`,
//! anywhere in the arguments, turns automatic selection off (B-05), so only
//! the exact remembered instance is shown.

use std::thread;
use std::time::{Duration, Instant};

use mpris_core::content::{Content, content};
use mpris_core::dbus::Core;
use mpris_core::model::Settings;
use mpris_core::player::MPRIS_PREFIX;
use mpris_core::selection::{player_choices, select_player};
use mpris_core::time::format_time;

/// Shows the core's own logs, so a missing bus is not mistaken for an
/// empty one.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

const STRICT: &str = "--strict";

fn main() {
    let _ = log::set_logger(&StderrLog).map(|()| log::set_max_level(log::LevelFilter::Info));
    let (strict, names): (Vec<String>, Vec<String>) =
        std::env::args().skip(1).partition(|arg| arg == STRICT);
    let settings = Settings {
        player: names.into_iter().next().map(bus_name),
        auto_select: strict.is_empty(),
        ..Settings::default()
    };
    let core = Core::start();
    loop {
        thread::sleep(Duration::from_secs(1));
        let snapshot = core.snapshot();
        let player = select_player(&snapshot.players, &settings);
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
        print_content(&content(player, &settings, Instant::now()));
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

/// The short form saves typing the prefix shared by every MPRIS name.
fn bus_name(arg: String) -> String {
    if arg.starts_with(MPRIS_PREFIX) {
        arg
    } else {
        format!("{MPRIS_PREFIX}{arg}")
    }
}

fn print_content(content: &Content) {
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
                .duration
                .map(|d| format!(" / {}", format_time(d)))
                .unwrap_or_default();
            let progress = track
                .progress
                .map(|p| format!(" ({:.0}%)", p * 100.0))
                .unwrap_or_default();
            let paused = if track.paused { " [paused]" } else { "" };
            println!("  time    {position}{duration}{progress}{paused}");
            match &track.art {
                Some(art) => println!("  art     {}x{}", art.width, art.height),
                None => println!("  art     none"),
            }
        }
    }
}
