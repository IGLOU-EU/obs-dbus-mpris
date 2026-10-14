//! A text too long for its line, drawn once into a strip of its own that
//! the OBS glue slides under the line at each frame (B-36), so that the
//! animation draws nothing on the processor.
//!
//! The strip holds the text, the gap, then enough of the text's copy to
//! fill the line: a window as wide as the line shows a seamless loop at any
//! offset from 0 to the period, and the period itself shows the same as 0.

use std::sync::Arc;

use mpris_core::settings::MAX_SIDE;

use crate::color::Linear;
use crate::frame::Frame;
use crate::layout::Rect;
use crate::text::Fonts;

/// Space between a scrolling text and its copy, relative to its font size:
/// wide enough to read the end of the text apart from its start.
const LOOP_GAP: f32 = 2.0;

/// One scrolling text, ready to become a texture.
#[derive(Debug, PartialEq)]
pub struct Strip {
    /// Where the text shows in the source: the glue draws a window of this
    /// size from the strip, there.
    pub line: Rect,
    /// The text width rounded up, plus the gap: the window at this offset
    /// shows the same as at 0.
    pub period: u32,
    /// `period + line.w` wide, `line.h` tall.
    pub image: Frame,
    /// What the strip shows, so that a redraw with the same finds it again.
    text: String,
    size: u32,
    color: Linear,
}

/// The strips of one source, kept between frames so that a new second on
/// the clock does not draw them again (B-43).
#[derive(Debug, Default)]
pub struct StripCache {
    last: Vec<Arc<Strip>>,
}

impl StripCache {
    pub fn new() -> StripCache {
        StripCache::default()
    }

    /// `text` at `size` in `color` as a strip for `line`, the one kept when
    /// nothing it shows changed. `None` when the text fits its line, and
    /// when its strip would be wider than any frame may be: drawing it in
    /// the frame then cuts it with an ellipsis.
    pub(crate) fn get(
        &self,
        fonts: &mut Fonts,
        text: &str,
        size: u32,
        line: Rect,
        color: Linear,
    ) -> Option<Arc<Strip>> {
        let kept = self.last.iter().find(|strip| {
            (strip.text.as_str(), strip.size, strip.line, strip.color) == (text, size, line, color)
        });
        match kept {
            Some(strip) => Some(Arc::clone(strip)),
            None => draw(fonts, text, size, line, color).map(Arc::new),
        }
    }

    /// Keeps `strips`, those of the frame just drawn, and forgets the rest.
    pub(crate) fn keep(&mut self, strips: &[Arc<Strip>]) {
        self.last = strips.to_vec();
    }
}

fn draw(fonts: &mut Fonts, text: &str, size: u32, line: Rect, color: Linear) -> Option<Strip> {
    let width = fonts.width(text, size)?;
    if width <= line.w as f32 {
        return None;
    }
    let gap = (size as f32 * LOOP_GAP).round() as u32;
    // A whole number of pixels, so that the copy lands on the same fraction
    // of a pixel as the text and the loop shows no seam.
    let period = (width.ceil() as u32).saturating_add(gap);
    let image_width = period.checked_add(line.w).filter(|&w| w <= MAX_SIDE)?;
    let mut image = Frame::transparent(image_width, line.h);
    fonts.draw_loop(&mut image, text, size, period, color);
    Some(Strip {
        line,
        period,
        image,
        text: text.to_owned(),
        size,
        color,
    })
}

#[cfg(test)]
mod tests {
    use mpris_core::model::Rgba;

    use super::*;
    use crate::text::tests::test_fonts;

    const LONG: &str = "A title far too long to fit in the narrow line it is given";
    const LINE: Rect = Rect::new(30, 40, 200, 25);

    fn white() -> Linear {
        Linear::from(Rgba::new(255, 255, 255, 255))
    }

    fn strip(fonts: &mut Fonts, text: &str) -> Option<Arc<Strip>> {
        StripCache::new().get(fonts, text, 20, LINE, white())
    }

    /// The columns of `frame` from `x`, `width` of them, row by row.
    fn columns(frame: &Frame, x: u32, width: u32) -> Vec<[u8; 4]> {
        (0..frame.height())
            .flat_map(|y| (x..x + width).map(move |x| (x, y)))
            .map(|(x, y)| frame.pixel(x, y).expect("inside"))
            .collect()
    }

    fn inked_columns(frame: &Frame) -> Vec<u32> {
        (0..frame.width())
            .filter(|&x| (0..frame.height()).any(|y| frame.pixel(x, y).is_some_and(|p| p[3] > 0)))
            .collect()
    }

    #[test]
    fn a_text_that_fits_has_no_strip() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        assert_eq!(strip(&mut fonts, "Title"), None);
    }

    #[test]
    fn a_text_too_long_gets_a_strip_for_its_line() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let strip = strip(&mut fonts, LONG).expect("a strip");
        let width = fonts.width(LONG, 20).expect("a width").ceil() as u32;
        assert!(width > LINE.w);
        assert_eq!(strip.line, LINE);
        assert_eq!(strip.period, width + 40, "a gap of twice the font size");
        assert_eq!(strip.image.width(), strip.period + LINE.w);
        assert_eq!(strip.image.height(), LINE.h);
    }

    #[test]
    fn the_window_at_the_period_shows_the_start_again() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let strip = strip(&mut fonts, LONG).expect("a strip");
        assert_eq!(
            columns(&strip.image, 0, LINE.w),
            columns(&strip.image, strip.period, LINE.w)
        );
    }

    #[test]
    fn the_text_starts_at_the_left_edge_and_its_copy_after_the_gap() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let strip = strip(&mut fonts, LONG).expect("a strip");
        let width = fonts.width(LONG, 20).expect("a width").ceil() as u32;
        let inked = inked_columns(&strip.image);
        assert!(inked[0] < 4, "{inked:?}");
        let in_gap: Vec<u32> = inked
            .iter()
            .copied()
            .filter(|x| (width + 2..strip.period - 2).contains(x))
            .collect();
        assert_eq!(in_gap, Vec::<u32>::new());
        assert!(inked.contains(&(strip.period + inked[0])), "the copy");
    }

    #[test]
    fn the_strip_takes_the_text_color() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let red = Linear::from(Rgba::new(255, 0, 0, 255));
        let strip = StripCache::new()
            .get(&mut fonts, LONG, 20, LINE, red)
            .expect("a strip");
        let pixels = strip.image.pixels().chunks_exact(4);
        let solid: Vec<&[u8]> = pixels.filter(|p| p[3] == 255).collect();
        assert!(!solid.is_empty());
        assert!(solid.iter().all(|p| *p == [255, 0, 0, 255]));
    }

    #[test]
    fn a_strip_wider_than_a_frame_is_left_to_the_ellipsis() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let huge = "x".repeat(2000);
        assert!(fonts.width(&huge, 20).is_some_and(|w| w > MAX_SIDE as f32));
        assert_eq!(strip(&mut fonts, &huge), None);
    }

    #[test]
    fn nothing_to_draw_has_no_strip() {
        assert_eq!(strip(&mut Fonts::none(), LONG), None);
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        assert_eq!(strip(&mut fonts, "  \n "), None);
    }

    // Cache (B-43)

    #[test]
    fn a_kept_strip_is_handed_back_without_drawing() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut cache = StripCache::new();
        let first = cache
            .get(&mut fonts, LONG, 20, LINE, white())
            .expect("a strip");
        cache.keep(&[Arc::clone(&first)]);
        let again = cache.get(&mut Fonts::none(), LONG, 20, LINE, white());
        assert!(again.is_some_and(|again| Arc::ptr_eq(&again, &first)));
    }

    #[test]
    fn a_change_of_what_shows_draws_a_new_strip() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut cache = StripCache::new();
        let first = cache
            .get(&mut fonts, LONG, 20, LINE, white())
            .expect("a strip");
        cache.keep(&[Arc::clone(&first)]);
        let red = Linear::from(Rgba::new(255, 0, 0, 255));
        let other = format!("{LONG}!");
        let changes = [
            cache.get(&mut fonts, &other, 20, LINE, white()),
            cache.get(&mut fonts, LONG, 21, LINE, white()),
            cache.get(&mut fonts, LONG, 20, Rect { w: 190, ..LINE }, white()),
            cache.get(&mut fonts, LONG, 20, Rect { y: 0, ..LINE }, white()),
            cache.get(&mut fonts, LONG, 20, LINE, red),
        ];
        for (i, changed) in changes.into_iter().enumerate() {
            let changed = changed.expect("a strip");
            assert!(!Arc::ptr_eq(&changed, &first), "change {i}");
        }
    }

    #[test]
    fn strips_not_kept_are_forgotten() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut cache = StripCache::new();
        let first = cache
            .get(&mut fonts, LONG, 20, LINE, white())
            .expect("a strip");
        cache.keep(&[Arc::clone(&first)]);
        cache.keep(&[]);
        assert_eq!(cache.get(&mut Fonts::none(), LONG, 20, LINE, white()), None);
    }
}
