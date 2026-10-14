//! The whole source in one frame: panel, thumbnail, texts, bar and pause
//! icon, at the places the layout gives (B-37), with the texts too long
//! for their line apart in strips that scroll over it (B-36).

use std::sync::Arc;

use mpris_core::content::{Content, TrackContent};
use mpris_core::model::{Rgba, Settings};
use mpris_core::time::format_time;

use crate::art::ArtCache;
use crate::color::Linear;
use crate::frame::Frame;
use crate::layout::{Layout, Rect};
use crate::strip::{Strip, StripCache};
use crate::text::{Align, Fonts};

/// Stands in for a missing cover (B-10), drawn in the text color alone so
/// that it sits on the background like the texts do.
const NO_COVER: &str = "\u{1F3BC}";

/// Size of that symbol against the side of its slot.
const NO_COVER_SCALE: f32 = 0.5;

/// What a source shows: the frame, and the strips of its scrolling texts,
/// each laid over the frame at its line.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub frame: Frame,
    pub strips: Vec<Arc<Strip>>,
}

/// Draws `content` at the size of `layout`. Nothing lands outside the
/// frame, and nothing is drawn for [`Content::Nothing`] (B-23). `art` and
/// `strips` keep the scaled cover and the scrolling texts of one source
/// between frames.
pub fn draw(
    layout: &Layout,
    content: &Content,
    settings: &Settings,
    fonts: &mut Fonts,
    art: &mut ArtCache,
    strips: &mut StripCache,
) -> Picture {
    let mut frame = Frame::transparent(layout.width, layout.height);
    let background = Linear::from(settings.color_background);
    let mut texts = Texts {
        fonts,
        cache: strips,
        strips: Vec::new(),
        scrolls: scrolls(settings),
        color: Linear::from(settings.color_text),
        clip: layout.panel,
    };
    match content {
        Content::Nothing => {}
        Content::Message(message) => {
            frame.fill(layout.panel, layout.corner_radius, |_, _| background);
            texts.scrolling(&mut frame, message, layout.sizes.title, layout.message);
        }
        Content::Track(track) => {
            frame.fill(layout.panel, layout.corner_radius, |_, _| background);
            draw_track(&mut frame, layout, track, settings, texts.fonts, art);
            draw_texts(&mut frame, layout, track, &mut texts);
        }
    }
    let drawn = texts.strips;
    strips.keep(&drawn);
    Picture {
        frame,
        strips: drawn,
    }
}

/// Whether a text too long for its line scrolls, rather than ends with an
/// ellipsis (B-36).
fn scrolls(settings: &Settings) -> bool {
    settings.scroll_enabled
}

/// Filled part of a bar `width` pixels wide, in whole pixels, so that a
/// progress change smaller than a pixel draws the same frame (B-43).
pub fn bar_fill(width: u32, progress: f32) -> u32 {
    // A NaN casts to 0.
    (f64::from(progress.clamp(0.0, 1.0)) * f64::from(width)).round() as u32
}

/// What a frame shows, so that a source draws again only when it changes
/// (B-43). Two equal keys always give the same frame.
#[derive(Debug, Clone, PartialEq)]
pub struct RedrawKey {
    layout: Layout,
    colors: [Rgba; 4],
    /// Whether long texts go to strips. Their speed and pause only move
    /// the strips, which the glue does at each frame.
    scrolls: bool,
    /// With its exact progress left out, which `bar_fill` stands for.
    content: Content,
    bar_fill: Option<u32>,
}

/// The key of the frame [`draw`] would give for the same arguments.
/// Cheap: no shaping, no drawing, and the cover compared by address.
pub fn redraw_key(layout: &Layout, content: &Content, settings: &Settings) -> RedrawKey {
    let (content, bar_fill) = match content {
        Content::Track(track) => {
            let fill = layout
                .bar
                .zip(track.progress)
                .map(|(bar, p)| bar_fill(bar.w, p));
            let track = TrackContent {
                progress: None,
                ..track.clone()
            };
            (Content::Track(track), fill)
        }
        other => (other.clone(), None),
    };
    RedrawKey {
        layout: layout.clone(),
        colors: [
            settings.color_background,
            settings.color_text,
            settings.color_bar_fill,
            settings.color_bar_track,
        ],
        scrolls: scrolls(settings),
        content,
        bar_fill,
    }
}

/// "Album - Artist", or whichever of the two is known (B-37).
pub(crate) fn subtitle(album: Option<&str>, artist: Option<&str>) -> Option<String> {
    match (album, artist) {
        (Some(album), Some(artist)) => Some(format!("{album} - {artist}")),
        (one, other) => one.or(other).map(str::to_owned),
    }
}

fn draw_track(
    frame: &mut Frame,
    layout: &Layout,
    track: &TrackContent,
    settings: &Settings,
    fonts: &mut Fonts,
    art: &mut ArtCache,
) {
    let radius = layout.corner_radius;
    let art_radius = layout.art_radius;
    match (layout.art, &track.art) {
        (Some(slot), Some(image)) => {
            frame.put((slot.x, slot.y), art.get(image, slot.w, art_radius));
        }
        // The slot keeps its place and its shape while no cover is ready
        // (B-10, B-38).
        (Some(slot), None) => {
            let background = Linear::from(settings.color_background);
            frame.fill(slot, art_radius, |_, _| background);
            let size = (slot.w as f32 * NO_COVER_SCALE).round() as u32;
            let text = Linear::from(settings.color_text);
            fonts.draw_symbol(frame, NO_COVER, size, slot, text);
        }
        (None, _) => {}
    }
    if let (Some(bar), Some(progress)) = (layout.bar, track.progress) {
        let split = bar.x + bar_fill(bar.w, progress);
        let (fill, rest) = (
            Linear::from(settings.color_bar_fill),
            Linear::from(settings.color_bar_track),
        );
        frame.fill(bar, radius, |x, _| if x < split { fill } else { rest });
    }
    if let (true, Some(pause)) = (track.paused, layout.pause) {
        let text = Linear::from(settings.color_text);
        for bar in pause.bars {
            frame.fill(bar, radius, |_, _| text);
        }
    }
}

fn draw_texts(frame: &mut Frame, layout: &Layout, track: &TrackContent, texts: &mut Texts) {
    let sizes = layout.sizes;
    let title_line = match (track.paused, layout.pause) {
        (true, Some(pause)) => Some(pause.title),
        _ => layout.title,
    };
    if let (Some(line), Some(title)) = (title_line, &track.title) {
        texts.scrolling(frame, title, sizes.title, line);
    }
    let subtitle = subtitle(track.album.as_deref(), track.artist.as_deref());
    if let (Some(line), Some(subtitle)) = (layout.subtitle, subtitle) {
        texts.scrolling(frame, &subtitle, sizes.subtitle, line);
    }
    if let Some(row) = layout.times {
        // Each time keeps to its half, so that neither covers the other.
        let half = row.w / 2;
        if let Some(position) = track.position {
            let left = Rect { w: half, ..row };
            texts.fixed(frame, &format_time(position), sizes.time, left, Align::Left);
        }
        if let Some(time) = track.right_time {
            let right = Rect::new(row.x + half, row.y, row.w - half, row.h);
            texts.fixed(frame, &time.text(), sizes.time, right, Align::Right);
        }
    }
}

/// Where the texts of one picture go: into the frame, or into strips of
/// their own for those too long for their line while the source scrolls.
struct Texts<'a> {
    fonts: &'a mut Fonts,
    cache: &'a StripCache,
    /// The strips of this picture, in the order their lines were drawn.
    strips: Vec<Arc<Strip>>,
    scrolls: bool,
    color: Linear,
    clip: Rect,
}

impl Texts<'_> {
    /// A text that scrolls when too long for `line`, and then leaves the
    /// line of the frame empty for its strip.
    fn scrolling(&mut self, frame: &mut Frame, text: &str, size: u32, line: Rect) {
        let strip = self
            .scrolls
            .then(|| self.cache.get(self.fonts, text, size, line, self.color))
            .flatten();
        match strip {
            Some(strip) => self.strips.push(strip),
            None => self.fixed(frame, text, size, line, Align::Left),
        }
    }

    /// A text cut with an ellipsis when too long for `line`.
    fn fixed(&mut self, frame: &mut Frame, text: &str, size: u32, line: Rect, align: Align) {
        self.fonts
            .draw(frame, text, size, line, align, self.color, self.clip);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use mpris_core::content::RightTime;
    use mpris_core::model::ArtImage;

    use super::*;
    use crate::layout::layout;
    use crate::strip::StripCache;
    use crate::text::tests::{emoji_fonts, test_fonts};

    const BACKGROUND: Rgba = Rgba::new(0, 0, 0, 255);
    const TEXT: Rgba = Rgba::new(255, 255, 255, 255);
    const FILL: Rgba = Rgba::new(255, 0, 0, 255);
    const TRACK: Rgba = Rgba::new(0, 0, 255, 255);

    /// Opaque colors, each told apart by one channel.
    fn settings() -> Settings {
        Settings {
            color_background: BACKGROUND,
            color_text: TEXT,
            color_bar_fill: FILL,
            color_bar_track: TRACK,
            ..Settings::default()
        }
    }

    fn with(edit: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = settings();
        edit(&mut s);
        s
    }

    /// A track with no text, so that only shapes are drawn.
    fn track(progress: Option<f32>, paused: bool) -> Content {
        Content::Track(TrackContent {
            paused,
            title: None,
            artist: None,
            album: None,
            position: None,
            right_time: None,
            progress,
            art: None,
        })
    }

    /// Without fonts, so that only shapes are drawn.
    fn render(settings: &Settings, content: &Content) -> (Layout, Frame) {
        render_with(&mut Fonts::none(), settings, content)
    }

    fn render_with(fonts: &mut Fonts, settings: &Settings, content: &Content) -> (Layout, Frame) {
        let (layout, picture) = picture_of(fonts, settings, content);
        (layout, picture.frame)
    }

    fn picture_of(fonts: &mut Fonts, settings: &Settings, content: &Content) -> (Layout, Picture) {
        let layout = layout(settings);
        let picture = draw(
            &layout,
            content,
            settings,
            fonts,
            &mut ArtCache::new(),
            &mut StripCache::new(),
        );
        (layout, picture)
    }

    /// A track with every text, the progress bar hidden so that any
    /// colored pixel of the panel is text.
    fn texts(title: &str, paused: bool) -> Content {
        Content::Track(TrackContent {
            paused,
            title: Some(title.into()),
            artist: Some("Artist".into()),
            album: Some("Album".into()),
            position: Some(Duration::from_secs(81)),
            right_time: Some(RightTime::Duration(Duration::from_secs(116))),
            progress: None,
            art: None,
        })
    }

    /// Pixels inked over the background within `area`.
    fn ink(frame: &Frame, area: Rect) -> Vec<(u32, u32)> {
        (area.y..area.bottom())
            .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                // The smoothed corners of the panel are not opaque.
                frame
                    .pixel(x, y)
                    .is_some_and(|p| p[3] == 255 && p != rgba(BACKGROUND))
            })
            .collect()
    }

    /// Smoothing may spill a glyph one pixel past its advance.
    const SPILL: u32 = 1;

    fn span(points: &[(u32, u32)]) -> (u32, u32, u32, u32) {
        let xs = points.iter().map(|p| p.0);
        let ys = points.iter().map(|p| p.1);
        (
            xs.clone().min().expect("ink"),
            xs.max().expect("ink"),
            ys.clone().min().expect("ink"),
            ys.max().expect("ink"),
        )
    }

    fn no_bar() -> Settings {
        with(|s| s.show_progress = false)
    }

    /// Long texts cut with an ellipsis rather than scrolled.
    fn no_bar_no_scroll() -> Settings {
        with(|s| {
            s.show_progress = false;
            s.scroll_enabled = false;
        })
    }

    fn with_art(content: Content, image: ArtImage) -> Content {
        match content {
            Content::Track(track) => Content::Track(TrackContent {
                art: Some(Arc::new(image)),
                ..track
            }),
            other => other,
        }
    }

    /// A 16:9 image, red in its central square, blue on the sides.
    fn cover() -> ArtImage {
        let (width, height) = (16, 9);
        let rgba = (0..height)
            .flat_map(|_| 0..width)
            .flat_map(|x| {
                if (3..13).contains(&x) {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        ArtImage {
            width,
            height,
            rgba,
        }
    }

    fn at(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
        frame.pixel(x, y).expect("inside the frame")
    }

    fn center(r: Rect) -> (u32, u32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    fn rgba(c: Rgba) -> [u8; 4] {
        [c.r, c.g, c.b, c.a]
    }

    fn some(r: Option<Rect>) -> Rect {
        r.expect("a rectangle")
    }

    #[test]
    fn frame_has_the_layout_size_whatever_the_content() {
        for content in [
            Content::Nothing,
            Content::Message("x".into()),
            track(Some(0.3), true),
        ] {
            for s in [
                settings(),
                with(|s| s.show_art = false),
                with(|s| s.width = 1),
            ] {
                let (layout, frame) = render(&s, &content);
                assert_eq!(
                    (frame.width(), frame.height()),
                    (layout.width, layout.height)
                );
                assert_eq!(
                    frame.pixels().len(),
                    (layout.width * layout.height * 4) as usize
                );
            }
        }
    }

    #[test]
    fn nothing_draws_a_transparent_frame() {
        let (layout, frame) = render(&settings(), &Content::Nothing);
        assert_eq!(frame, Frame::transparent(layout.width, layout.height));
    }

    #[test]
    fn message_draws_the_panel_and_leaves_the_art_slot_transparent() {
        let (layout, frame) = render(&settings(), &Content::Message(String::new()));
        let (x, y) = center(layout.panel);
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
        let (x, y) = center(some(layout.art));
        assert_eq!(at(&frame, x, y), [0; 4]);
    }

    #[test]
    fn art_slot_without_image_shows_the_background() {
        let (layout, frame) = render(&settings(), &track(None, false));
        let (x, y) = center(some(layout.art));
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
        assert_eq!(at(&frame, 0, 0), [0; 4], "the slot has rounded corners");
    }

    #[test]
    fn art_slot_without_image_shows_a_score_in_the_text_color() {
        let Some(mut fonts) = emoji_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &settings(), &track(None, false));
        let slot = some(layout.art);
        let ink = ink(&frame, slot);
        assert!(!ink.is_empty(), "no symbol drawn");
        // White over black blends to greys only when the emoji keeps one color.
        for &(x, y) in &ink {
            let [r, g, b, _] = at(&frame, x, y);
            assert!(r == g && g == b, "({x}, {y}) is [{r}, {g}, {b}]");
        }
        let (left, right, top, bottom) = span(&ink);
        let (cx, cy) = center(slot);
        let tolerance = slot.w / 8;
        assert!(
            ((left + right) / 2).abs_diff(cx) <= tolerance,
            "{left}..{right}"
        );
        assert!(
            ((top + bottom) / 2).abs_diff(cy) <= tolerance,
            "{top}..{bottom}"
        );
        assert!(right - left < slot.w * 3 / 4, "{left}..{right}");
        assert!(bottom - top < slot.h * 3 / 4, "{top}..{bottom}");
    }

    #[test]
    fn art_slot_stays_plain_when_no_font_has_the_score() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &settings(), &track(None, false));
        assert_eq!(ink(&frame, some(layout.art)), Vec::new());
    }

    #[test]
    fn a_cover_hides_the_score() {
        let Some(mut fonts) = emoji_fonts() else {
            return;
        };
        let content = with_art(track(None, false), cover());
        let (layout, frame) = render_with(&mut fonts, &settings(), &content);
        let (x, y) = center(some(layout.art));
        assert_eq!(at(&frame, x, y), [255, 0, 0, 255]);
    }

    #[test]
    fn art_fills_its_slot_with_the_center_of_the_cover() {
        for art_right in [false, true] {
            let s = with(|s| s.art_right = art_right);
            let (layout, frame) = render(&s, &with_art(track(None, false), cover()));
            let art = some(layout.art);
            for x in [
                art.x + art.w / 8,
                art.x + art.w / 2,
                art.right() - art.w / 8,
            ] {
                assert_eq!(
                    at(&frame, x, art.h / 2),
                    [255, 0, 0, 255],
                    "{art_right} {x}"
                );
            }
            assert_eq!(at(&frame, art.x, 0), [0; 4], "rounded corner");
            let (x, y) = center(layout.panel);
            assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
        }
    }

    #[test]
    fn hidden_art_draws_no_cover() {
        let content = with_art(track(None, false), cover());
        let (layout, frame) = render(&with(|s| s.show_art = false), &content);
        assert_eq!(
            at(&frame, layout.height / 2, layout.height / 2),
            rgba(BACKGROUND)
        );
    }

    #[test]
    fn hidden_art_leaves_the_panel_alone() {
        let (layout, frame) = render(&with(|s| s.show_art = false), &track(None, false));
        assert_eq!(layout.art, None);
        assert_eq!(
            at(&frame, layout.width / 2, layout.height / 2),
            rgba(BACKGROUND)
        );
    }

    #[test]
    fn panel_and_art_each_have_rounded_corners() {
        let (layout, frame) = render(&settings(), &track(None, false));
        let art = some(layout.art);
        let corners = [
            (0, 0),
            (art.right() - 1, 0),
            (art.right(), 0),
            (layout.width - 1, layout.height - 1),
        ];
        for (x, y) in corners {
            assert_eq!(at(&frame, x, y), [0; 4], "({x}, {y})");
        }
    }

    #[test]
    fn gap_leaves_the_space_between_art_and_panel_transparent() {
        for art_right in [false, true] {
            let s = with(|s| {
                s.art_gap = 24;
                s.art_right = art_right;
            });
            let (layout, frame) = render(&s, &with_art(track(None, false), cover()));
            let art = some(layout.art);
            let between = if art_right {
                layout.panel.right()..art.x
            } else {
                art.right()..layout.panel.x
            };
            assert_eq!(between.len(), 24);
            for x in between {
                assert_eq!(at(&frame, x, layout.height / 2), [0; 4], "{art_right} {x}");
            }
            let (x, y) = center(art);
            assert_eq!(at(&frame, x, y), [255, 0, 0, 255]);
            let (x, y) = center(layout.panel);
            assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
        }
    }

    /// A point of the slot inside a square with rounded corners of the
    /// default radius, but outside the disc the slot holds.
    fn off_the_disc(art: Rect) -> (u32, u32) {
        (art.x + art.w / 8, art.y + art.h / 8)
    }

    #[test]
    fn round_art_cuts_a_disc_out_of_the_cover() {
        let s = with(|s| {
            s.art_round = true;
            s.corner_radius = 0;
        });
        let (layout, frame) = render(&s, &with_art(track(None, false), cover()));
        let art = some(layout.art);
        let (x, y) = off_the_disc(art);
        assert_eq!(at(&frame, x, y), [0; 4], "outside the disc");
        let (x, y) = center(art);
        assert_eq!(at(&frame, x, y), [255, 0, 0, 255]);
        for (x, y) in [
            (art.x, art.h / 2),
            (art.right() - 1, art.h / 2),
            (art.x + art.w / 2, 0),
        ] {
            assert!(at(&frame, x, y)[3] > 200, "the disc reaches ({x}, {y})");
        }
        assert_eq!(at(&frame, art.x, 0), [0; 4], "corner");
    }

    #[test]
    fn round_art_leaves_the_panel_its_corner_radius() {
        let s = with(|s| {
            s.art_round = true;
            s.corner_radius = 0;
        });
        let (layout, frame) = render(&s, &with_art(track(None, false), cover()));
        let corner = (layout.panel.right() - 1, layout.height - 1);
        assert_eq!(at(&frame, corner.0, corner.1), rgba(BACKGROUND));
    }

    #[test]
    fn square_art_keeps_the_corners_off_the_disc() {
        let (layout, frame) = render(&settings(), &with_art(track(None, false), cover()));
        let (x, y) = off_the_disc(some(layout.art));
        assert_eq!(at(&frame, x, y), [255, 0, 0, 255]);
    }

    #[test]
    fn round_art_gives_the_empty_slot_the_same_disc() {
        let round = with(|s| s.art_round = true);
        let (layout, frame) = render(&round, &track(None, false));
        let art = some(layout.art);
        let (x, y) = off_the_disc(art);
        assert_eq!(at(&frame, x, y), [0; 4]);
        let (x, y) = center(art);
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
        let (layout, frame) = render(&settings(), &track(None, false));
        let (x, y) = off_the_disc(some(layout.art));
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND), "square slot");
    }

    #[test]
    fn zero_radius_gives_square_corners() {
        let (layout, frame) = render(&with(|s| s.corner_radius = 0), &track(None, false));
        let art = some(layout.art);
        for (x, y) in [
            (0, 0),
            (art.right(), 0),
            (layout.width - 1, layout.height - 1),
        ] {
            assert_eq!(at(&frame, x, y), rgba(BACKGROUND), "({x}, {y})");
        }
    }

    #[test]
    fn radius_larger_than_the_blocks_is_capped() {
        let (layout, frame) = render(&with(|s| s.corner_radius = 256), &track(None, false));
        assert_eq!(at(&frame, 0, 0), [0; 4]);
        let (x, y) = center(layout.panel);
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
    }

    #[test]
    fn transparent_background_leaves_the_panel_transparent() {
        let s = with(|s| s.color_background = Rgba::new(255, 255, 255, 0));
        let (layout, frame) = render(&s, &track(None, false));
        let (x, y) = center(layout.panel);
        assert_eq!(at(&frame, x, y), [0; 4]);
        let (x, y) = center(some(layout.art));
        assert_eq!(at(&frame, x, y), [0; 4]);
    }

    /// The colors along the middle row of the bar, its rounded ends left out.
    fn bar_row(progress: Option<f32>) -> (Rect, Vec<[u8; 4]>) {
        let (layout, frame) = render(&settings(), &track(progress, false));
        let bar = some(layout.bar);
        let y = bar.y + bar.h / 2;
        let row = (bar.x + bar.h..bar.right() - bar.h)
            .map(|x| at(&frame, x, y))
            .collect();
        (bar, row)
    }

    #[test]
    fn bar_fills_the_progress_and_tracks_the_rest() {
        let (bar, row) = bar_row(Some(0.5));
        let split = (bar.w / 2 - bar.h) as usize;
        assert!(row[..split].iter().all(|p| *p == rgba(FILL)), "{row:?}");
        assert!(row[split..].iter().all(|p| *p == rgba(TRACK)), "{row:?}");
    }

    #[test]
    fn bar_at_the_ends_of_the_track() {
        let (_, row) = bar_row(Some(0.0));
        assert!(row.iter().all(|p| *p == rgba(TRACK)));
        let (_, row) = bar_row(Some(1.0));
        assert!(row.iter().all(|p| *p == rgba(FILL)));
    }

    #[test]
    fn unknown_progress_draws_no_bar() {
        let (_, row) = bar_row(None);
        assert!(row.iter().all(|p| *p == rgba(BACKGROUND)));
    }

    #[test]
    fn translucent_track_blends_over_the_background() {
        let s = with(|s| s.color_bar_track = Rgba::new(255, 255, 255, 128));
        let (layout, frame) = render(&s, &track(Some(0.0), false));
        let (x, y) = center(some(layout.bar));
        assert_eq!(at(&frame, x, y), [188, 188, 188, 255]);
    }

    #[test]
    fn bar_fill_rounds_to_whole_pixels() {
        assert_eq!(bar_fill(100, 0.0), 0);
        assert_eq!(bar_fill(100, 0.5), 50);
        assert_eq!(bar_fill(100, 0.504), 50);
        assert_eq!(bar_fill(100, 0.506), 51);
        assert_eq!(bar_fill(100, 1.0), 100);
        assert_eq!(bar_fill(0, 0.5), 0);
    }

    #[test]
    fn bar_fill_stays_within_the_bar() {
        assert_eq!(bar_fill(100, -0.5), 0);
        assert_eq!(bar_fill(100, 1.5), 100);
        assert_eq!(bar_fill(100, f32::NAN), 0);
        assert_eq!(bar_fill(100, f32::INFINITY), 100);
        assert_eq!(bar_fill(u32::MAX, 1.0), u32::MAX);
    }

    #[test]
    fn pause_draws_two_bars_in_the_text_color() {
        let (layout, frame) = render(&settings(), &track(None, true));
        let pause = layout.pause.expect("pause");
        for bar in pause.bars {
            let (x, y) = center(bar);
            assert_eq!(at(&frame, x, y), rgba(TEXT));
        }
        let gap = (pause.bars[0].right() + pause.bars[1].x) / 2;
        assert_eq!(at(&frame, gap, center(pause.bars[0]).1), rgba(BACKGROUND));
    }

    #[test]
    fn playing_draws_no_pause_icon() {
        let (layout, frame) = render(&settings(), &track(None, false));
        let (x, y) = center(layout.pause.expect("pause").bars[0]);
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
    }

    #[test]
    fn every_toggle_off_still_draws_the_panel() {
        let s = with(|s| {
            s.show_title = false;
            s.show_artist = false;
            s.show_album = false;
            s.show_progress = false;
            s.show_position = false;
            s.show_duration = false;
            s.show_art = false;
        });
        let (layout, frame) = render(&s, &track(Some(0.5), true));
        let (x, y) = center(layout.panel);
        assert_eq!(at(&frame, x, y), rgba(BACKGROUND));
    }

    // Text

    #[test]
    fn subtitle_joins_album_and_artist() {
        let both = subtitle(Some("Album"), Some("Artist"));
        assert_eq!(both.as_deref(), Some("Album - Artist"));
        assert_eq!(subtitle(Some("Album"), None).as_deref(), Some("Album"));
        assert_eq!(subtitle(None, Some("Artist")).as_deref(), Some("Artist"));
        assert_eq!(subtitle(None, None), None);
    }

    #[test]
    fn each_text_lands_in_its_row() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &no_bar(), &texts("Title", false));
        let all = ink(&frame, layout.panel);
        let rows = [layout.title, layout.subtitle, layout.times].map(some);
        for row in rows {
            let inside: Vec<_> = all
                .iter()
                .copied()
                .filter(|&(_, y)| (row.y..row.bottom()).contains(&y))
                .collect();
            assert!(!inside.is_empty(), "{row:?}");
            let (left, right, ..) = span(&inside);
            assert!(
                left + SPILL >= row.x && right < row.right() + SPILL,
                "{row:?} {left} {right}"
            );
        }
    }

    #[test]
    fn times_sit_left_and_right_under_the_bar() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &no_bar(), &texts("Title", false));
        let times = some(layout.times);
        let (left, right, ..) = span(&ink(&frame, times));
        assert!(
            left < times.x + 4,
            "position starts at the left edge: {left}"
        );
        assert!(
            right + 4 >= times.right(),
            "duration ends at the right edge: {right}"
        );
        let middle = Rect::new(times.x + times.w / 3, times.y, times.w / 3, times.h);
        assert!(
            ink(&frame, middle).is_empty(),
            "nothing between the two times"
        );
    }

    #[test]
    fn remaining_time_ends_at_the_right_edge_behind_a_minus() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let with_right = |right_time| match texts("Title", false) {
            Content::Track(track) => Content::Track(TrackContent {
                right_time: Some(right_time),
                ..track
            }),
            _ => unreachable!(),
        };
        let seconds = Duration::from_secs(35);
        let mut right_ink = |right_time| {
            let (layout, frame) = render_with(&mut fonts, &no_bar(), &with_right(right_time));
            let times = some(layout.times);
            let half = Rect::new(times.x + times.w / 2, times.y, times.w / 2, times.h);
            span(&ink(&frame, half))
        };
        let (duration_left, duration_right, ..) = right_ink(RightTime::Duration(seconds));
        let (remaining_left, remaining_right, ..) = right_ink(RightTime::Remaining(seconds));
        assert_eq!(
            remaining_right, duration_right,
            "both end at the right edge"
        );
        assert!(
            remaining_left + 3 < duration_left,
            "a minus before the time: {remaining_left} {duration_left}"
        );
    }

    #[test]
    fn a_long_title_stays_in_its_row() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &no_bar_no_scroll(), &texts(LONG, false));
        let title = some(layout.title);
        let row = Rect::new(layout.panel.x, title.y, layout.panel.w, title.h);
        let (left, right, ..) = span(&ink(&frame, row));
        assert!(
            left + SPILL >= title.x && right < title.right() + SPILL,
            "{left} {right} {title:?}"
        );
        assert!(
            right + layout.sizes.title > title.right(),
            "the ellipsis reaches the end"
        );
    }

    #[test]
    fn a_paused_title_stops_before_the_pause_icon() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &no_bar_no_scroll(), &texts(LONG, true));
        let pause = layout.pause.expect("pause");
        let before = Rect::new(
            layout.panel.x,
            pause.title.y,
            pause.bars[0].x - layout.panel.x,
            pause.title.h,
        );
        let (_, right, ..) = span(&ink(&frame, before));
        assert!(right < pause.title.right() + SPILL, "{right} {pause:?}");
    }

    // Scrolling (B-36)

    const LONG: &str =
        "An extremely long title that goes on and on, far beyond the width of any panel";

    fn lines(picture: &Picture) -> Vec<Rect> {
        picture.strips.iter().map(|strip| strip.line).collect()
    }

    fn with_album(content: Content, album: &str) -> Content {
        match content {
            Content::Track(track) => Content::Track(TrackContent {
                album: Some(album.into()),
                ..track
            }),
            other => other,
        }
    }

    #[test]
    fn a_long_title_scrolls_in_a_strip_instead_of_the_frame() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, picture) = picture_of(&mut fonts, &no_bar(), &texts(LONG, false));
        let title = some(layout.title);
        assert_eq!(lines(&picture), [title]);
        assert_eq!(ink(&picture.frame, title), Vec::new(), "left to the strip");
        let subtitle = some(layout.subtitle);
        assert!(!ink(&picture.frame, subtitle).is_empty(), "fits, so drawn");
    }

    #[test]
    fn texts_that_fit_have_no_strip() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (_, picture) = picture_of(&mut fonts, &no_bar(), &texts("Title", false));
        assert!(picture.strips.is_empty());
    }

    #[test]
    fn scrolling_off_cuts_a_long_title_with_an_ellipsis_instead() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, picture) = picture_of(&mut fonts, &no_bar_no_scroll(), &texts(LONG, false));
        assert!(picture.strips.is_empty());
        assert!(!ink(&picture.frame, some(layout.title)).is_empty());
    }

    #[test]
    fn the_slowest_speed_still_scrolls() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let slowest = with(|s| {
            s.show_progress = false;
            s.scroll_speed = 1;
        });
        let (layout, picture) = picture_of(&mut fonts, &slowest, &texts(LONG, false));
        assert_eq!(lines(&picture), [some(layout.title)]);
    }

    #[test]
    fn title_and_subtitle_too_long_get_a_strip_each() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let content = with_album(texts(LONG, false), LONG);
        let (layout, picture) = picture_of(&mut fonts, &no_bar(), &content);
        assert_eq!(lines(&picture), [some(layout.title), some(layout.subtitle)]);
        for strip in &picture.strips {
            assert_eq!(strip.image.height(), strip.line.h);
            assert!(layout.panel.contains(&strip.line));
        }
    }

    #[test]
    fn a_paused_long_title_scrolls_before_the_pause_icon() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, picture) = picture_of(&mut fonts, &no_bar(), &texts(LONG, true));
        let pause = layout.pause.expect("pause");
        assert_eq!(lines(&picture), [pause.title]);
        for bar in pause.bars {
            let (x, y) = center(bar);
            assert_eq!(at(&picture.frame, x, y), rgba(TEXT), "the icon stays");
        }
    }

    #[test]
    fn a_long_message_scrolls_like_a_single_text() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let message = Content::Message(LONG.into());
        let (layout, picture) = picture_of(&mut fonts, &settings(), &message);
        assert_eq!(lines(&picture), [layout.message]);
        assert_eq!(ink(&picture.frame, layout.panel), Vec::new());
    }

    #[test]
    fn a_new_second_keeps_the_strips() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let s = no_bar();
        let layout = layout(&s);
        let (mut art, mut strips) = (ArtCache::new(), StripCache::new());
        let mut picture = |seconds| {
            let content = match with_album(texts(LONG, false), LONG) {
                Content::Track(track) => Content::Track(TrackContent {
                    position: Some(Duration::from_secs(seconds)),
                    ..track
                }),
                _ => unreachable!(),
            };
            draw(&layout, &content, &s, &mut fonts, &mut art, &mut strips)
        };
        let (first, next) = (picture(81), picture(82));
        assert_ne!(first.frame, next.frame);
        assert_eq!(first.strips.len(), 2);
        for (a, b) in first.strips.iter().zip(&next.strips) {
            assert!(Arc::ptr_eq(a, b), "drawn once");
        }
    }

    #[test]
    fn a_strip_at_offset_zero_shows_the_title_where_the_frame_would() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, picture) = picture_of(&mut fonts, &no_bar(), &texts(LONG, false));
        let strip = &picture.strips[0];
        let mut composed = picture.frame.clone();
        let window = Rect::new(0, 0, strip.line.w, strip.line.h);
        composed.over((strip.line.x, strip.line.y), &strip.image, window);
        let (_, cut) = render_with(&mut fonts, &no_bar_no_scroll(), &texts(LONG, false));
        let title = some(layout.title);
        let (left, right, top, bottom) = span(&ink(&composed, title));
        let (cut_left, _, cut_top, cut_bottom) = span(&ink(&cut, title));
        assert_eq!(left, cut_left);
        assert!(top.abs_diff(cut_top) <= 1 && bottom.abs_diff(cut_bottom) <= 1);
        assert!(right < title.right(), "nothing leaves the line");
    }

    #[test]
    fn message_is_drawn_centered_in_the_title_font() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let message = Content::Message("Nothing playing".into());
        let (layout, frame) = render_with(&mut fonts, &settings(), &message);
        let (left, _, top, bottom) = span(&ink(&frame, layout.panel));
        assert!(left + SPILL >= layout.message.x && left < layout.message.x + 4);
        assert!(top >= layout.message.y && bottom < layout.message.bottom());
        let (above, below) = (top, layout.height - 1 - bottom);
        assert!(
            above.abs_diff(below) <= layout.sizes.title / 4,
            "{above} {below}"
        );
        assert!(ink(&frame, some(layout.art)).is_empty());
    }

    #[test]
    fn hidden_texts_are_not_drawn() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let s = with(|s| {
            s.show_progress = false;
            s.show_title = false;
            s.show_position = false;
            s.show_duration = false;
            s.show_artist = false;
            s.show_album = false;
        });
        let (layout, frame) = render_with(&mut fonts, &s, &texts("Title", false));
        assert!(ink(&frame, layout.panel).is_empty());
    }

    #[test]
    fn nothing_stays_transparent_with_fonts() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let (layout, frame) = render_with(&mut fonts, &settings(), &Content::Nothing);
        assert_eq!(frame, Frame::transparent(layout.width, layout.height));
    }

    // Redraw key (B-43)

    fn key(settings: &Settings, content: &Content) -> RedrawKey {
        redraw_key(&layout(settings), content, settings)
    }

    fn playing(position: u64, progress: f32) -> Content {
        Content::Track(TrackContent {
            position: Some(Duration::from_secs(position)),
            ..match texts("Title", false) {
                Content::Track(track) => TrackContent {
                    progress: Some(progress),
                    ..track
                },
                _ => unreachable!(),
            }
        })
    }

    #[test]
    fn same_inputs_give_the_same_key() {
        let s = settings();
        assert_eq!(key(&s, &playing(10, 0.25)), key(&s, &playing(10, 0.25)));
        assert_eq!(key(&s, &Content::Nothing), key(&s, &Content::Nothing));
    }

    #[test]
    fn progress_within_one_bar_pixel_keeps_the_key() {
        let s = settings();
        let bar = some(layout(&s).bar);
        let pixel = 1.0 / bar.w as f32;
        let base = 50.0 * pixel;
        assert_eq!(
            key(&s, &playing(10, base)),
            key(&s, &playing(10, base + pixel * 0.4))
        );
        assert_ne!(
            key(&s, &playing(10, base)),
            key(&s, &playing(10, base + pixel))
        );
    }

    #[test]
    fn equal_keys_draw_equal_frames() {
        let s = settings();
        let bar = some(layout(&s).bar);
        let pixel = 1.0 / bar.w as f32;
        let (a, b) = (playing(10, 50.0 * pixel), playing(10, 50.4 * pixel));
        assert_eq!(key(&s, &a), key(&s, &b));
        assert_eq!(render(&s, &a).1, render(&s, &b).1);
    }

    #[test]
    fn hidden_bar_ignores_the_progress() {
        let s = no_bar();
        assert_eq!(key(&s, &playing(10, 0.1)), key(&s, &playing(10, 0.9)));
    }

    #[test]
    fn each_visible_change_changes_the_key() {
        let s = settings();
        let base = key(&s, &playing(10, 0.5));
        assert_ne!(key(&s, &playing(11, 0.5)), base, "position");
        assert_ne!(key(&s, &texts("Title", false)), base, "no progress");
        let changes: [fn(&mut Settings); 10] = [
            |s| s.color_background.a = 1,
            |s| s.color_text.r = 1,
            |s| s.color_bar_fill.g = 1,
            |s| s.color_bar_track.b = 1,
            |s| s.width += 1,
            |s| s.corner_radius += 1,
            |s| s.art_right = true,
            |s| s.art_round = true,
            |s| s.art_gap = 10,
            |s| s.bar_height = 10,
        ];
        for (i, change) in changes.iter().enumerate() {
            assert_ne!(key(&with(change), &playing(10, 0.5)), base, "change {i}");
        }
    }

    #[test]
    fn turning_the_scrolling_on_or_off_changes_the_key() {
        let base = key(&settings(), &playing(10, 0.5));
        let still = with(|s| s.scroll_enabled = false);
        assert_ne!(key(&still, &playing(10, 0.5)), base);
    }

    #[test]
    fn speed_and_pause_of_the_scrolling_keep_the_key() {
        let base = key(&settings(), &playing(10, 0.5));
        let other = with(|s| {
            s.scroll_speed = 80;
            s.scroll_pause = Duration::from_millis(500);
        });
        assert_eq!(key(&other, &playing(10, 0.5)), base);
    }

    #[test]
    fn settings_that_show_nothing_keep_the_key() {
        let base = key(&settings(), &playing(10, 0.5));
        let unseen = with(|s| {
            s.player = Some("org.mpris.MediaPlayer2.vlc".into());
            s.auto_select = false;
            s.art_online = Some(mpris_core::model::Service::YouTube);
            s.fallback_text = "Other".into();
        });
        assert_eq!(key(&unseen, &playing(10, 0.5)), base);
    }

    #[test]
    fn messages_and_nothing_have_their_own_keys() {
        let s = settings();
        let keys = [
            key(&s, &Content::Nothing),
            key(&s, &Content::Message("A".into())),
            key(&s, &Content::Message("B".into())),
            key(&s, &track(None, false)),
        ];
        for (i, a) in keys.iter().enumerate() {
            for (j, b) in keys.iter().enumerate() {
                assert_eq!(a == b, i == j, "{i} {j}");
            }
        }
    }
}
