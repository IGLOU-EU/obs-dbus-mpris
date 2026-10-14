//! Text, through cosmic-text (D-04): shaping, fallback to a font that has
//! the glyphs, Japanese included (B-35), and rasterization through swash.

use std::path::PathBuf;

use cosmic_text::fontdb::Database;
use cosmic_text::{
    Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Wrap,
};

use crate::color::Linear;
use crate::frame::Frame;
use crate::layout::Rect;

/// The fonts and the glyphs rasterized so far. One for the whole plugin:
/// loading the fonts is slow and the glyphs serve every source.
pub struct Fonts {
    system: FontSystem,
    glyphs: SwashCache,
}

/// Which edge of its line a text keeps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Right,
}

/// Locale of the fonts given by file. It only orders the fallbacks, which a
/// few chosen files do not need.
const FILES_LOCALE: &str = "en-US";

const ELLIPSIS: &str = "…";

/// The glyph a font gives for a character it lacks.
const MISSING_GLYPH: u16 = 0;

impl Fonts {
    /// Every font of the system. Slow, it reads each installed font file:
    /// never call it on OBS's graphics thread (B-41).
    pub fn system() -> Fonts {
        Fonts::with(FontSystem::new())
    }

    /// Only the fonts in `files`, so that a test does not depend on the
    /// fonts of the machine. A file that is not a font is ignored.
    pub fn from_files(files: impl IntoIterator<Item = PathBuf>) -> Fonts {
        let mut db = Database::new();
        for file in files {
            // A bad file adds no face, which is all that matters here.
            let _ = db.load_font_file(file);
        }
        Fonts::with(FontSystem::new_with_locale_and_db(FILES_LOCALE.into(), db))
    }

    /// No font at all, with which text draws nothing.
    pub fn none() -> Fonts {
        Fonts::from_files([])
    }

    fn with(system: FontSystem) -> Fonts {
        Fonts {
            system,
            glyphs: SwashCache::new(),
        }
    }

    /// `text` on one line, as it fits in `width` pixels at `size`: whole,
    /// or cut and ended with an ellipsis (B-36). `None` when there is
    /// nothing to draw. Line breaks and other control characters read as
    /// spaces, since a player may send them.
    pub(crate) fn fit(&mut self, text: &str, size: u32, width: u32) -> Option<String> {
        let line = one_line(text);
        let text = line.trim();
        let shaped = self.shape(text, size, size)?;
        let max = width as f32;
        if line_width(&shaped) <= max {
            return Some(text.to_owned());
        }
        // A first guess from the glyphs already shaped, then checked by
        // shaping the result, since a cut can change the shaping around it.
        let mut cut = prefix_within(&shaped, max - self.measure(ELLIPSIS, size));
        loop {
            let candidate = format!("{}{ELLIPSIS}", text[..cut].trim_end());
            if self.measure(&candidate, size) <= max {
                return Some(candidate);
            }
            if cut == 0 {
                return None;
            }
            cut = text[..cut].char_indices().last().map_or(0, |(i, _)| i);
        }
    }

    /// Width in pixels of `text` on one line at `size`.
    pub(crate) fn measure(&mut self, text: &str, size: u32) -> f32 {
        self.shape(text, size, size)
            .map_or(0.0, |shaped| line_width(&shaped))
    }

    /// Draws `text` fitted to `line`, against its `align` edge and centered
    /// vertically, with ink clipped to `clip`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        frame: &mut Frame,
        text: &str,
        size: u32,
        line: Rect,
        align: Align,
        color: Linear,
        clip: Rect,
    ) {
        let Some(fitted) = self.fit(text, size, line.w) else {
            return;
        };
        let Some(shaped) = self.shape(&fitted, size, line.h) else {
            return;
        };
        let x = match align {
            Align::Left => line.x as f32,
            Align::Right => line.right() as f32 - line_width(&shaped),
        };
        self.paint(frame, &shaped, (x, line.y as f32), color, clip, false);
    }

    /// Draws `symbol` centered in `area`, every glyph in `color` alone, emoji
    /// included. Draws nothing when no font has the symbol, rather than the
    /// empty box that stands for a missing glyph.
    pub(crate) fn draw_symbol(
        &mut self,
        frame: &mut Frame,
        symbol: &str,
        size: u32,
        area: Rect,
        color: Linear,
    ) {
        let Some(shaped) = self.shape(symbol, size, area.h) else {
            return;
        };
        let x = area.x as f32 + (area.w as f32 - line_width(&shaped)) / 2.0;
        self.paint(frame, &shaped, (x, area.y as f32), color, area, true);
    }

    /// Rasterizes `shaped` with its first line at `origin`. `one_color`
    /// draws color glyphs by their coverage alone, in `color`.
    fn paint(
        &mut self,
        frame: &mut Frame,
        shaped: &Buffer,
        (x, y): (f32, f32),
        color: Linear,
        clip: Rect,
        one_color: bool,
    ) {
        for run in shaped.layout_runs() {
            for glyph in run.glyphs {
                if one_color && glyph.glyph_id == MISSING_GLYPH {
                    continue;
                }
                let glyph = glyph.physical((x, y + run.line_y), 1.0);
                let Some(image) = self.glyphs.get_image(&mut self.system, glyph.cache_key) else {
                    continue;
                };
                let origin = (
                    glyph.x + image.placement.left,
                    glyph.y - image.placement.top,
                );
                let width = image.placement.width;
                match image.content {
                    SwashContent::Mask => frame.blend_mask(origin, width, &image.data, color, clip),
                    SwashContent::Color if one_color => {
                        let coverage: Vec<u8> = image.data.chunks_exact(4).map(|p| p[3]).collect();
                        frame.blend_mask(origin, width, &coverage, color, clip);
                    }
                    SwashContent::Color => {
                        frame.blend_image(origin, width, &image.data, color.a, clip);
                    }
                    // Only produced on request, which this crate never makes.
                    SwashContent::SubpixelMask => {}
                }
            }
        }
    }

    /// `text` shaped on one line `line_height` pixels high. `None` for an
    /// empty text, and without any font, where cosmic-text would panic.
    fn shape(&mut self, text: &str, size: u32, line_height: u32) -> Option<Buffer> {
        if text.is_empty() || self.system.db().is_empty() {
            return None;
        }
        let size = size.max(1) as f32;
        let mut buffer = Buffer::new_empty(Metrics::new(size, line_height.max(1) as f32));
        buffer.set_wrap(&mut self.system, Wrap::None);
        let attrs = Attrs::new().family(Family::SansSerif);
        buffer.set_text(&mut self.system, text, &attrs, Shaping::Advanced, None);
        Some(buffer)
    }
}

/// `text` with every control character, line breaks included, as a space.
fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn line_width(shaped: &Buffer) -> f32 {
    shaped
        .layout_runs()
        .map(|run| run.line_w)
        .fold(0.0, f32::max)
}

/// The longest start of the shaped text, as a byte length, whose glyphs
/// advance at most `budget` pixels. Glyphs are taken in text order, so
/// that right-to-left text is cut at its logical end too.
fn prefix_within(shaped: &Buffer, budget: f32) -> usize {
    let mut glyphs: Vec<(usize, usize, f32)> = shaped
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|g| (g.start, g.end, g.w)))
        .collect();
    glyphs.sort_by_key(|&(start, ..)| start);
    let mut advance = 0.0;
    let mut cut = 0;
    for (_, end, w) in glyphs {
        advance += w;
        if advance > budget {
            break;
        }
        cut = cut.max(end);
    }
    cut
}

#[cfg(test)]
pub(crate) mod tests {
    use std::env;
    use std::path::Path;

    use mpris_core::model::Rgba;

    use super::*;

    /// Font files common on Linux distributions, tried in order when
    /// `RENDER_TEST_FONT` does not name one.
    const CANDIDATES: [&str; 5] = [
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        "/usr/share/fonts/noto/NotoSans-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    ];

    /// One known font, or `None` with a message saying why the test that
    /// asked is skipped.
    pub(crate) fn test_fonts() -> Option<Fonts> {
        let chosen = env::var_os("RENDER_TEST_FONT")
            .map(PathBuf::from)
            .or_else(|| CANDIDATES.iter().map(PathBuf::from).find(|p| p.is_file()));
        match chosen {
            Some(path) => Some(Fonts::from_files([path])),
            None => {
                eprintln!("skipped: no test font found, set RENDER_TEST_FONT to a font file");
                None
            }
        }
    }

    /// Color emoji fonts common on Linux distributions.
    const EMOJI_CANDIDATES: [&str; 2] = [
        "/usr/share/fonts/noto/NotoColorEmoji.ttf",
        "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    ];

    /// A color emoji font, or `None` with a message saying why the test that
    /// asked is skipped.
    pub(crate) fn emoji_fonts() -> Option<Fonts> {
        match EMOJI_CANDIDATES
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file())
        {
            Some(path) => Some(Fonts::from_files([path])),
            None => {
                eprintln!("skipped: no color emoji font found");
                None
            }
        }
    }

    const WHITE: Rgba = Rgba::new(255, 255, 255, 255);
    const LONG: &str = "A title far too long to fit in the narrow line it is given";

    fn white() -> Linear {
        Linear::from(WHITE)
    }

    /// Pixels with any ink.
    fn inked(frame: &Frame) -> Vec<(u32, u32)> {
        (0..frame.height())
            .flat_map(|y| (0..frame.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| frame.pixel(x, y).is_some_and(|p| p[3] > 0))
            .collect()
    }

    fn drawn(fonts: &mut Fonts, text: &str, line: Rect, align: Align, clip: Rect) -> Frame {
        let mut frame = Frame::transparent(200, 60);
        fonts.draw(&mut frame, text, 20, line, align, white(), clip);
        frame
    }

    #[test]
    fn fonts_can_move_to_the_thread_that_loads_them() {
        fn send<T: Send>() {}
        send::<Fonts>();
    }

    #[test]
    fn text_that_fits_is_kept_whole() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        assert_eq!(fonts.fit("Title", 20, 400).as_deref(), Some("Title"));
    }

    #[test]
    fn text_too_long_ends_with_an_ellipsis_within_the_width() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        for width in [40, 100, 160, 300] {
            let fitted = fonts.fit(LONG, 20, width).expect("something fits");
            assert!(fitted.ends_with('…'), "{fitted:?}");
            let kept = fitted.trim_end_matches('…');
            assert!(LONG.starts_with(kept), "{fitted:?}");
            assert!(!kept.ends_with(' '), "{fitted:?}");
            assert!(
                fonts.measure(&fitted, 20) <= width as f32,
                "{width} {fitted:?}"
            );
        }
    }

    #[test]
    fn a_wider_line_keeps_more_of_the_text() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let narrow = fonts.fit(LONG, 20, 100).expect("fits");
        let wide = fonts.fit(LONG, 20, 300).expect("fits");
        assert!(wide.len() > narrow.len());
    }

    #[test]
    fn a_line_narrower_than_the_ellipsis_shows_nothing() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        assert_eq!(fonts.fit(LONG, 20, 1), None);
        assert_eq!(fonts.fit(LONG, 20, 0), None);
    }

    #[test]
    fn blank_text_shows_nothing() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        for blank in ["", " ", "\n\t "] {
            assert_eq!(fonts.fit(blank, 20, 400), None, "{blank:?}");
        }
    }

    #[test]
    fn line_breaks_read_as_spaces() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        assert_eq!(
            fonts.fit("Two\nlines\r\n", 20, 400).as_deref(),
            Some("Two lines")
        );
        assert_eq!(fonts.fit("a\tb\u{7}c", 20, 400).as_deref(), Some("a b c"));
    }

    #[test]
    fn text_in_a_script_the_font_lacks_does_not_panic() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let fitted = fonts.fit("愛をとりもどせ!! (MOVIE ver.)", 20, 80);
        assert!(fitted.is_some());
        drawn(
            &mut fonts,
            "愛をとりもどせ!!",
            Rect::new(0, 0, 200, 30),
            Align::Left,
            Rect::new(0, 0, 200, 60),
        );
    }

    #[test]
    fn huge_and_tiny_sizes_do_not_panic() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut frame = Frame::transparent(50, 50);
        let all = Rect::new(0, 0, 50, 50);
        fonts.draw(&mut frame, "Huge", 512, all, Align::Left, white(), all);
        fonts.draw(&mut frame, "Tiny", 1, all, Align::Left, white(), all);
        fonts.draw(&mut frame, "Zero", 0, all, Align::Left, white(), all);
    }

    #[test]
    fn ink_stays_within_the_clip() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let line = Rect::new(10, 10, 120, 25);
        let clip = Rect::new(10, 12, 60, 20);
        let frame = drawn(&mut fonts, LONG, line, Align::Left, clip);
        let ink = inked(&frame);
        assert!(!ink.is_empty());
        for (x, y) in ink {
            assert!(clip.contains(&Rect::new(x, y, 1, 1)), "({x}, {y})");
        }
    }

    #[test]
    fn left_text_starts_at_the_left_edge() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let line = Rect::new(20, 10, 160, 25);
        let frame = drawn(
            &mut fonts,
            "Left",
            line,
            Align::Left,
            Rect::new(0, 0, 200, 60),
        );
        let ink = inked(&frame);
        let left = ink.iter().map(|p| p.0).min().expect("ink");
        assert!((20..26).contains(&left), "{left}");
    }

    #[test]
    fn right_text_ends_at_the_right_edge() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let line = Rect::new(20, 10, 160, 25);
        let frame = drawn(
            &mut fonts,
            "1:56",
            line,
            Align::Right,
            Rect::new(0, 0, 200, 60),
        );
        let right = inked(&frame).iter().map(|p| p.0).max().expect("ink");
        assert!((174..180).contains(&right), "{right}");
    }

    #[test]
    fn text_is_centered_on_its_line() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let line = Rect::new(0, 20, 200, 25);
        let frame = drawn(
            &mut fonts,
            "HHHH",
            line,
            Align::Left,
            Rect::new(0, 0, 200, 60),
        );
        let ink = inked(&frame);
        let top = ink.iter().map(|p| p.1).min().expect("ink");
        let bottom = ink.iter().map(|p| p.1).max().expect("ink") + 1;
        assert!(top >= line.y && bottom <= line.bottom(), "{top} {bottom}");
        let (above, below) = (top - line.y, line.bottom() - bottom);
        assert!(above.abs_diff(below) <= 6, "{above} {below}");
    }

    #[test]
    fn text_takes_the_color_given() {
        let Some(mut fonts) = test_fonts() else {
            return;
        };
        let mut frame = Frame::transparent(200, 60);
        let red = Linear::from(Rgba::new(255, 0, 0, 255));
        let all = Rect::new(0, 0, 200, 60);
        fonts.draw(
            &mut frame,
            "I",
            40,
            Rect::new(10, 0, 100, 50),
            Align::Left,
            red,
            all,
        );
        let solid = inked(&frame)
            .into_iter()
            .filter_map(|(x, y)| frame.pixel(x, y))
            .filter(|p| p[3] == 255);
        assert!(solid.into_iter().all(|p| p == [255, 0, 0, 255]));
    }

    #[test]
    fn no_font_draws_nothing() {
        let mut fonts = Fonts::none();
        assert_eq!(fonts.fit("Title", 20, 400), None);
        assert_eq!(fonts.measure("Title", 20), 0.0);
        let frame = drawn(
            &mut fonts,
            "Title",
            Rect::new(0, 0, 200, 30),
            Align::Left,
            Rect::new(0, 0, 200, 60),
        );
        assert_eq!(frame, Frame::transparent(200, 60));
    }

    #[test]
    fn a_file_that_is_not_a_font_gives_no_font() {
        let mut fonts =
            Fonts::from_files([Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")]);
        assert_eq!(fonts.fit("Title", 20, 400), None);
    }
}
