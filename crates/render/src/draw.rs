//! The whole source in one frame: panel, thumbnail, texts, bar and pause
//! icon, at the places the layout gives (B-37).

use mpris_core::content::{Content, TrackContent};
use mpris_core::model::Settings;

use crate::art::ArtCache;
use crate::color::Linear;
use crate::frame::Frame;
use crate::layout::Layout;

/// Draws `content` at the size of `layout`. Nothing lands outside the
/// frame, and nothing is drawn for [`Content::Nothing`] (B-23). `art` keeps
/// the scaled cover of one source between frames.
pub fn draw(layout: &Layout, content: &Content, settings: &Settings, art: &mut ArtCache) -> Frame {
    let mut frame = Frame::transparent(layout.width, layout.height);
    let background = Linear::from(settings.color_background);
    match content {
        Content::Nothing => {}
        Content::Message(_) => frame.fill(layout.panel, layout.corner_radius, |_, _| background),
        Content::Track(track) => {
            frame.fill(layout.panel, layout.corner_radius, |_, _| background);
            draw_track(&mut frame, layout, track, settings, art);
        }
    }
    frame
}

/// Filled part of a bar `width` pixels wide, in whole pixels, so that a
/// progress change smaller than a pixel draws the same frame (B-43).
pub fn bar_fill(width: u32, progress: f32) -> u32 {
    // A NaN casts to 0.
    (f64::from(progress.clamp(0.0, 1.0)) * f64::from(width)).round() as u32
}

fn draw_track(
    frame: &mut Frame,
    layout: &Layout,
    track: &TrackContent,
    settings: &Settings,
    art: &mut ArtCache,
) {
    let radius = layout.corner_radius;
    match (layout.art, &track.art) {
        (Some(slot), Some(image)) => frame.put((slot.x, slot.y), art.get(image, slot.w, radius)),
        // The slot keeps its place while no cover is ready (B-10).
        (Some(slot), None) => {
            let background = Linear::from(settings.color_background);
            frame.fill(slot, radius, |_, _| background);
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mpris_core::model::{ArtImage, Rgba};

    use super::*;
    use crate::layout::{Rect, layout};

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
            duration: None,
            progress,
            art: None,
        })
    }

    fn render(settings: &Settings, content: &Content) -> (Layout, Frame) {
        let layout = layout(settings);
        let frame = draw(&layout, content, settings, &mut ArtCache::new());
        (layout, frame)
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
}
