//! Renders the source in each variant of the mockup (besoins.md, B-37) to
//! PNG files, to check the drawing by eye, and times the drawing.
//!
//! Each PNG shows the source as OBS shows it at the start of the scroll
//! cycle, strips laid over the frame. A variant with scrolling texts also
//! gets its frame alone (`-frame`), each strip (`-strip-<n>`), and the
//! source at a few instants of the cycle (`-at-<ms>ms`), as OBS draws it at
//! that time (B-36).
//!
//! Usage: `cargo run --release -p render --example render_png -- <out-dir>`
//!
//! The fonts OBS Flatpak sees may differ, so run the built example there too:
//! `flatpak run --command=<absolute path of the example> com.obsproject.Studio <out-dir>`,
//! adding `--filesystem=<out-dir>` for a folder outside the home folder.
//! The PNGs keep the alpha of the source, straight rather than premultiplied
//! as image viewers expect.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mpris_core::content::content;
use mpris_core::model::{
    Art, ArtImage, PlaybackStatus, Player, PlayerKey, PositionAnchor, Rgba, Settings, Track,
};
use render::art::ArtCache;
use render::draw::{Picture, draw};
use render::frame::Frame;
use render::layout::{Rect, layout};
use render::scroll::Scroll;
use render::strip::StripCache;
use render::text::Fonts;

/// OBS Flatpak runs on a glibc older than the `acosf` the host links by
/// default. The build script wraps the symbol, so that this example runs in
/// the sandbox, and the portable version stands in for it.
#[unsafe(no_mangle)]
pub extern "C" fn __wrap_acosf(x: f32) -> f32 {
    libm::acosf(x)
}

struct Variant {
    name: &'static str,
    settings: Settings,
    player: Option<Player>,
}

fn main() -> ExitCode {
    let Some(out) = env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: render_png <out-dir>");
        return ExitCode::FAILURE;
    };
    if let Err(error) = fs::create_dir_all(&out) {
        eprintln!("cannot create {}: {error}", out.display());
        return ExitCode::FAILURE;
    }

    let started = Instant::now();
    let mut fonts = Fonts::system();
    println!("fonts loaded in {:?}", started.elapsed());

    let now = Instant::now();
    for variant in variants(now) {
        let layout = layout(&variant.settings);
        let shown = content(variant.player.as_ref(), &variant.settings, now);
        let picture = draw(
            &layout,
            &shown,
            &variant.settings,
            &mut fonts,
            &mut ArtCache::new(),
            &mut StripCache::new(),
        );
        if let Err(error) = save_variant(&out, variant.name, &picture, &variant.settings) {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    }

    time_full_panel(&mut fonts, now);
    time_strips(&mut fonts, now);
    ExitCode::SUCCESS
}

/// The source at the start of the cycle, then, when texts scroll, its parts
/// and a few instants of the cycle.
fn save_variant(
    out: &Path,
    name: &str,
    picture: &Picture,
    settings: &Settings,
) -> Result<(), String> {
    let write = |suffix: &str, frame: &Frame| {
        let path = out.join(format!("{name}{suffix}.png"));
        save(frame, &path).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        println!("{} ({}x{})", path.display(), frame.width(), frame.height());
        Ok::<(), String>(())
    };
    write("", &composed(picture, settings, Duration::ZERO))?;
    if picture.strips.is_empty() {
        return Ok(());
    }
    write("-frame", &picture.frame)?;
    for (i, strip) in picture.strips.iter().enumerate() {
        println!(
            "  strip {i}: period {} px, line {:?}",
            strip.period, strip.line
        );
        write(&format!("-strip-{i}"), &strip.image)?;
    }
    for at in instants(picture, settings) {
        write(
            &format!("-at-{}ms", at.as_millis()),
            &composed(picture, settings, at),
        )?;
    }
    Ok(())
}

/// The frame with each strip laid over its line at the offset that OBS
/// draws `elapsed` after the cycle started.
fn composed(picture: &Picture, settings: &Settings, elapsed: Duration) -> Frame {
    let periods = picture.strips.iter().map(|strip| strip.period);
    let scroll = Scroll::new(periods, settings.scroll_speed, settings.scroll_pause);
    let mut frame = picture.frame.clone();
    for strip in &picture.strips {
        let offset = scroll.offset(elapsed, strip.period);
        let window = Rect::new(offset, 0, strip.line.w, strip.line.h);
        frame.over((strip.line.x, strip.line.y), &strip.image, window);
    }
    frame
}

/// Moments that tell the cycle: in the pause, under way, the shorter text
/// waiting at its start, the longer near its end, and the next pause.
fn instants(picture: &Picture, settings: &Settings) -> Vec<Duration> {
    let speed = f64::from(settings.scroll_speed.max(1));
    let travel = |period: u32| Duration::from_secs_f64(f64::from(period) / speed);
    let periods = picture.strips.iter().map(|strip| strip.period);
    let (shortest, longest) = (periods.clone().min(), periods.max());
    let (Some(shortest), Some(longest)) = (shortest, longest) else {
        return Vec::new();
    };
    let pause = settings.scroll_pause;
    let mut at = vec![
        Duration::from_millis(500),
        pause + Duration::from_millis(1500),
        pause + travel(longest) - Duration::from_millis(250),
        pause + travel(longest) + Duration::from_millis(100),
    ];
    if shortest < longest {
        at.push(pause + travel(shortest) + Duration::from_millis(500));
    }
    at.sort();
    at
}

/// The cost of the strips: the first draw of two long texts, which
/// rasterizes them, then a new second, which finds them drawn.
fn time_strips(fonts: &mut Fonts, now: Instant) {
    let settings = Settings::default();
    let layout = layout(&settings);
    let player = long_player(now);
    let (mut art, mut strips) = (ArtCache::new(), StripCache::new());
    let started = Instant::now();
    let shown = content(Some(&player), &settings, now);
    let picture = draw(&layout, &shown, &settings, fonts, &mut art, &mut strips);
    println!(
        "first draw with {} strips: {:?}",
        picture.strips.len(),
        started.elapsed()
    );
    let started = Instant::now();
    let shown = content(Some(&player), &settings, now + Duration::from_secs(1));
    draw(&layout, &shown, &settings, fonts, &mut art, &mut strips);
    println!(
        "redraw of a new second, strips kept: {:?}",
        started.elapsed()
    );
}

/// The cost of a redraw of the full panel with its cover: the first one,
/// which scales the cover, then those of each new second.
fn time_full_panel(fonts: &mut Fonts, now: Instant) {
    let settings = Settings::default();
    let layout = layout(&settings);
    let player = player(PlaybackStatus::Playing, now);
    let (mut art, mut strips) = (ArtCache::new(), StripCache::new());

    let started = Instant::now();
    draw(
        &layout,
        &content(Some(&player), &settings, now),
        &settings,
        fonts,
        &mut art,
        &mut strips,
    );
    println!("first draw, cover scaled: {:?}", started.elapsed());

    let rounds = 200u32;
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    for second in 0..rounds {
        let at = now + Duration::from_secs(u64::from(second % 30));
        let shown = content(Some(&player), &settings, at);
        let one = Instant::now();
        draw(&layout, &shown, &settings, fonts, &mut art, &mut strips);
        slowest = slowest.max(one.elapsed());
    }
    let mean = started.elapsed() / rounds;
    println!("redraw of a new second: mean {mean:?}, slowest {slowest:?} over {rounds}");
}

fn variants(now: Instant) -> Vec<Variant> {
    let playing = || Some(player(PlaybackStatus::Playing, now));
    let no_art = Settings {
        show_art: false,
        ..Settings::default()
    };
    let japanese = {
        let mut p = player(PlaybackStatus::Playing, now);
        p.track.title = Some("愛をとりもどせ!! (MOVIE ver.)".into());
        p.track.artists = vec![];
        p.track.album_artists = vec!["CRYSTAL KING".into()];
        p.track.album = None;
        p
    };
    let long = long_player(now);
    let long_paused = Player {
        status: PlaybackStatus::Paused,
        ..long_player(now)
    };
    let pending = {
        let mut p = player(PlaybackStatus::Playing, now);
        p.art = Art::Pending;
        p
    };
    let pending_round = pending.clone();
    vec![
        Variant {
            name: "01-title-only",
            settings: Settings {
                show_artist: false,
                show_album: false,
                show_progress: false,
                show_position: false,
                show_duration: false,
                ..no_art.clone()
            },
            player: playing(),
        },
        Variant {
            name: "02-title-subtitle",
            settings: Settings {
                show_progress: false,
                show_position: false,
                show_duration: false,
                ..no_art.clone()
            },
            player: playing(),
        },
        Variant {
            name: "03-full-panel",
            settings: no_art,
            player: playing(),
        },
        Variant {
            name: "04-art-left",
            settings: Settings::default(),
            player: playing(),
        },
        Variant {
            name: "05-art-right",
            settings: Settings {
                art_right: true,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "06-paused",
            settings: Settings::default(),
            player: Some(player(PlaybackStatus::Paused, now)),
        },
        Variant {
            name: "07-fallback-message",
            settings: Settings::default(),
            player: None,
        },
        Variant {
            name: "08-japanese",
            settings: Settings::default(),
            player: Some(japanese),
        },
        Variant {
            name: "09-long-title",
            settings: Settings::default(),
            player: Some(long),
        },
        Variant {
            name: "10-radius-0",
            settings: Settings {
                corner_radius: 0,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "11-transparent-background",
            settings: Settings {
                color_background: Rgba::new(0, 0, 0, 0),
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "12-cover-pending",
            settings: Settings::default(),
            player: Some(pending),
        },
        Variant {
            name: "13-narrow",
            settings: Settings {
                width: 1,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "14-large-fonts",
            settings: Settings {
                font_size_title: 48,
                font_size_subtitle: 36,
                font_size_time: 30,
                width: 900,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "15-gap-24",
            settings: Settings {
                art_gap: 24,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "16-gap-24-art-right",
            settings: Settings {
                art_gap: 24,
                art_right: true,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "17-round-art",
            settings: Settings {
                art_round: true,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "18-round-art-no-cover",
            settings: Settings {
                art_round: true,
                ..Settings::default()
            },
            player: Some(pending_round),
        },
        Variant {
            name: "19-round-art-gap-radius-0",
            settings: Settings {
                art_round: true,
                art_gap: 24,
                corner_radius: 0,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "20-remaining-time",
            settings: Settings {
                show_remaining: true,
                ..Settings::default()
            },
            player: playing(),
        },
        Variant {
            name: "21-long-title-ellipsis",
            settings: Settings {
                scroll_enabled: false,
                ..Settings::default()
            },
            player: Some(long_player(now)),
        },
        Variant {
            name: "22-long-title-paused",
            settings: Settings::default(),
            player: Some(long_paused),
        },
        Variant {
            name: "23-long-message",
            settings: Settings {
                fallback_text: "Aucune lecture en cours sur aucun des lecteurs du bus de session"
                    .into(),
                ..Settings::default()
            },
            player: None,
        },
    ]
}

/// A title and a subtitle too long for their lines, the subtitle the
/// shorter of the two.
fn long_player(now: Instant) -> Player {
    let mut p = player(PlaybackStatus::Playing, now);
    p.track.title = Some(
        "🎵 Une vidéo au titre interminable, bien trop long pour tenir sur une seule ligne".into(),
    );
    p.track.album = Some("Un album au nom tout aussi long que le titre".into());
    p.track.artists = vec!["Premier artiste".into(), "Second artiste".into()];
    p
}

/// A player at 1:21 of a 1:56 track, with a 16:9 cover.
fn player(status: PlaybackStatus, now: Instant) -> Player {
    Player {
        unique_name: ":1.1".into(),
        bus_name: "org.mpris.MediaPlayer2.mpv".into(),
        key: PlayerKey("mpv".into()),
        identity: "mpv".into(),
        status,
        rate: 1.0,
        track: Track {
            id: Some("/track/1".into()),
            title: Some("Titre".into()),
            artists: vec!["Artist".into()],
            album_artists: vec![],
            album: Some("Nom Album".into()),
            length: Some(Duration::from_secs(116)),
            art_url: Some("data:".into()),
            url: None,
        },
        anchor: PositionAnchor {
            position: Duration::from_secs(81),
            at: now,
        },
        art: Art::Ready(Arc::new(cover())),
        online: Vec::new(),
    }
}

/// 16:9 like VLC's covers: a ringed disc on the central square, and grey
/// sides that must not show once cropped.
fn cover() -> ArtImage {
    let (width, height) = (640u32, 360u32);
    let left = (width - height) / 2;
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            if x < left || x >= left + height {
                return [128, 128, 128, 255];
            }
            let (dx, dy) = (
                x as f32 - width as f32 / 2.0,
                y as f32 - height as f32 / 2.0,
            );
            let r = (dx * dx + dy * dy).sqrt() / (height as f32 / 2.0);
            if (0.70..0.78).contains(&r) {
                [255, 255, 255, 255]
            } else {
                let t = r.min(1.0);
                [
                    (255.0 * (1.0 - t)) as u8 + (90.0 * t) as u8,
                    (140.0 * (1.0 - t)) as u8,
                    (40.0 + 150.0 * t) as u8,
                    255,
                ]
            }
        })
        .collect();
    ArtImage {
        width,
        height,
        rgba,
    }
}

fn save(frame: &Frame, path: &Path) -> image::ImageResult<()> {
    image::RgbaImage::from_raw(frame.width(), frame.height(), frame.straight())
        .expect("a buffer of the frame size")
        .save(path)
}
