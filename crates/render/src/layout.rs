//! Where each element of the source goes (B-33, B-37), from the settings
//! alone. The track never enters here, so the source keeps its size
//! whatever plays.

use mpris_core::model::Settings;
use mpris_core::settings::{MAX_ART_GAP, MAX_CORNER_RADIUS, MAX_FONT_SIZE, MAX_SIDE};

/// A pixel rectangle of the source, origin at its top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Saturates rather than overflow, so that no rectangle panics.
    pub const fn right(&self) -> u32 {
        self.x.saturating_add(self.w)
    }

    pub const fn bottom(&self) -> u32 {
        self.y.saturating_add(self.h)
    }

    /// Whether `other` lies entirely within this rectangle.
    pub const fn contains(&self, other: &Rect) -> bool {
        other.x >= self.x
            && other.y >= self.y
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }
}

/// Font sizes in pixels, as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontSizes {
    pub title: u32,
    pub subtitle: u32,
    pub time: u32,
}

/// The pause indicator (B-21): two bars at the end of the title line, and
/// the room left to the title beside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pause {
    pub bars: [Rect; 2],
    pub title: Rect,
}

/// Every rectangle of the source. A hidden element has no rectangle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// The thumbnail slot, a square as tall as the source, apart from the
    /// panel by the gap of the settings (B-39).
    pub art: Option<Rect>,
    /// The text panel, its background drawn with rounded corners.
    pub panel: Rect,
    pub title: Option<Rect>,
    /// "Album - Artist" (B-37).
    pub subtitle: Option<Rect>,
    pub bar: Option<Rect>,
    /// Position on the left, duration on the right.
    pub times: Option<Rect>,
    pub pause: Option<Pause>,
    /// The fallback message line (B-23), centered in the panel.
    pub message: Rect,
    pub sizes: FontSizes,
    /// Of the panel and the bar (B-38). Drawing caps it at half the shorter
    /// side of each block.
    pub corner_radius: u32,
    /// Of the thumbnail slot: the corner radius, or half its side for a disc
    /// (B-38).
    pub art_radius: u32,
}

// Proportions, each relative to a font size so that the whole source scales
// with its text (B-37). Chosen to look like the mockup.

/// Height of a text row: room for the ascenders and descenders of Latin and
/// Japanese fonts alike.
const LINE_HEIGHT: f32 = 1.25;
/// Panel margin on all four sides, relative to the title size.
const MARGIN: f32 = 0.6;
/// Space between the text rows and the progress rows, relative to the
/// title size.
const BLOCK_GAP: f32 = 0.4;
/// Bar thickness, relative to the time size.
const BAR: f32 = 0.3;
/// Space between the bar and the times, relative to the time size.
const BAR_GAP: f32 = 0.2;
/// Narrowest text column, relative to the title size: an ellipsis and a
/// few glyphs, so that a tiny width still shows something.
const MIN_TEXT: f32 = 2.0;
/// Pause icon height relative to the title size, a little under the
/// height of capitals.
const PAUSE: f32 = 0.6;
/// Width of one pause bar and of the gap between bars, relative to the
/// icon height.
const PAUSE_BAR: f32 = 0.3;
const PAUSE_BAR_GAP: f32 = 0.25;
/// Space between the title and the pause icon, relative to the title size.
const PAUSE_MARGIN: f32 = 0.3;

/// Places every element for `settings`. Sizes beyond the schema bounds are
/// brought back within them, and a width too small for any text widens to
/// a minimal panel, so that any input gives a drawable layout.
pub fn layout(settings: &Settings) -> Layout {
    let sizes = FontSizes {
        title: settings.font_size_title.clamp(1, MAX_FONT_SIZE),
        subtitle: settings.font_size_subtitle.clamp(1, MAX_FONT_SIZE),
        time: settings.font_size_time.clamp(1, MAX_FONT_SIZE),
    };
    let title_line = scale(sizes.title, LINE_HEIGHT).max(1);
    let margin = scale(sizes.title, MARGIN).max(1);

    let title_h = settings.show_title.then_some(title_line);
    let subtitle_h = (settings.show_artist || settings.show_album)
        .then(|| scale(sizes.subtitle, LINE_HEIGHT).max(1));
    let bar_h = settings
        .show_progress
        .then(|| scale(sizes.time, BAR).max(1));
    let times_h = (settings.show_position || settings.show_duration)
        .then(|| scale(sizes.time, LINE_HEIGHT).max(1));
    let has_text = title_h.is_some() || subtitle_h.is_some();
    let has_progress = bar_h.is_some() || times_h.is_some();
    let block_gap = if has_text && has_progress {
        scale(sizes.title, BLOCK_GAP)
    } else {
        0
    };
    let bar_gap = if bar_h.is_some() && times_h.is_some() {
        scale(sizes.time, BAR_GAP)
    } else {
        0
    };
    let content = [title_h, subtitle_h, bar_h, times_h]
        .into_iter()
        .flatten()
        .sum::<u32>()
        + block_gap
        + bar_gap;
    // At least one title line, for the fallback message (B-23).
    let inner = content.max(title_line);
    let height = 2 * margin + inner;

    let icon = PauseIcon::new(sizes.title);
    let min_text = scale(sizes.title, MIN_TEXT).max(icon.width + icon.margin + 1);
    let (art_side, gap) = if settings.show_art {
        (height, settings.art_gap.min(MAX_ART_GAP))
    } else {
        (0, 0)
    };
    let width = settings
        .width
        .min(MAX_SIDE)
        .max(art_side + gap + 2 * margin + min_text);
    let panel_w = width - art_side - gap;
    let (art_x, panel_x) = if settings.art_right {
        (panel_w + gap, 0)
    } else {
        (0, art_side + gap)
    };
    let corner_radius = settings.corner_radius.min(MAX_CORNER_RADIUS);

    let mut column = Column {
        x: panel_x + margin,
        w: panel_w - 2 * margin,
        y: margin + (inner - content) / 2,
    };
    let title = column.row(title_h);
    let subtitle = column.row(subtitle_h);
    column.y += block_gap;
    let bar = column.row(bar_h);
    column.y += bar_gap;
    let times = column.row(times_h);

    Layout {
        width,
        height,
        art: settings
            .show_art
            .then(|| Rect::new(art_x, 0, art_side, art_side)),
        panel: Rect::new(panel_x, 0, panel_w, height),
        title,
        subtitle,
        bar,
        times,
        pause: title.map(|title| icon.place(title)),
        message: Rect::new(column.x, (height - title_line) / 2, column.w, title_line),
        sizes,
        corner_radius,
        art_radius: if settings.art_round {
            art_side / 2
        } else {
            corner_radius
        },
    }
}

/// `size * factor`, to the nearest pixel.
fn scale(size: u32, factor: f32) -> u32 {
    (size as f32 * factor).round() as u32
}

/// The text column, filled from the top.
struct Column {
    x: u32,
    w: u32,
    y: u32,
}

impl Column {
    fn row(&mut self, height: Option<u32>) -> Option<Rect> {
        let h = height?;
        let row = Rect::new(self.x, self.y, self.w, h);
        self.y += h;
        Some(row)
    }
}

/// Size of the pause icon for a title size.
struct PauseIcon {
    bar: u32,
    height: u32,
    width: u32,
    margin: u32,
}

impl PauseIcon {
    fn new(title_size: u32) -> PauseIcon {
        let height = scale(title_size, PAUSE).max(1);
        let bar = scale(height, PAUSE_BAR).max(1);
        PauseIcon {
            bar,
            height,
            width: 2 * bar + scale(height, PAUSE_BAR_GAP).max(1),
            margin: scale(title_size, PAUSE_MARGIN),
        }
    }

    /// The icon at the end of `title`, which the layout made wide enough.
    fn place(&self, title: Rect) -> Pause {
        let y = title.y + title.h.saturating_sub(self.height) / 2;
        let h = self.height.min(title.h);
        Pause {
            bars: [
                Rect::new(title.right() - self.width, y, self.bar, h),
                Rect::new(title.right() - self.bar, y, self.bar, h),
            ],
            title: Rect {
                w: title.w - self.width - self.margin,
                ..title
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::default()
    }

    fn with(edit: impl FnOnce(&mut Settings)) -> Settings {
        let mut s = settings();
        edit(&mut s);
        s
    }

    fn every_text_off(s: &mut Settings) {
        s.show_title = false;
        s.show_artist = false;
        s.show_album = false;
        s.show_progress = false;
        s.show_position = false;
        s.show_duration = false;
    }

    fn frame(l: &Layout) -> Rect {
        Rect::new(0, 0, l.width, l.height)
    }

    /// Every rectangle of `l`, with a name for the assertion messages.
    fn rects(l: &Layout) -> Vec<(&'static str, Rect)> {
        let mut all = vec![("panel", l.panel), ("message", l.message)];
        let optional = [
            ("art", l.art),
            ("title", l.title),
            ("subtitle", l.subtitle),
            ("bar", l.bar),
            ("times", l.times),
        ];
        all.extend(optional.into_iter().filter_map(|(n, r)| Some((n, r?))));
        if let Some(p) = l.pause {
            all.extend([("pause bar", p.bars[0]), ("pause bar", p.bars[1])]);
            all.push(("paused title", p.title));
        }
        all
    }

    /// The rectangles that hold text or drawing inside the panel.
    fn panel_content(l: &Layout) -> Vec<(&'static str, Rect)> {
        rects(l)
            .into_iter()
            .filter(|(name, _)| !matches!(*name, "panel" | "art"))
            .collect()
    }

    fn assert_sane(l: &Layout) {
        assert!(l.width > 0 && l.height > 0, "{l:?}");
        for (name, r) in rects(l) {
            assert!(
                frame(l).contains(&r),
                "{name} {r:?} outside the frame {l:?}"
            );
        }
        for (name, r) in panel_content(l) {
            assert!(l.panel.contains(&r), "{name} {r:?} outside the panel {l:?}");
        }
        assert_eq!(l.panel.y, 0);
        assert_eq!(l.panel.h, l.height);
        match l.art {
            Some(art) => {
                assert_eq!(art.w, art.h, "art is square");
                assert_eq!(art.h, l.height, "art is as tall as the source");
                assert_eq!(art.y, 0);
                assert_eq!(
                    art.w + gap(l) + l.panel.w,
                    l.width,
                    "art, gap and panel fill the width"
                );
            }
            None => assert_eq!(l.panel, frame(l)),
        }
    }

    /// The pixels between the art and the panel.
    fn gap(l: &Layout) -> u32 {
        let art = some(l.art, "art");
        if art.right() <= l.panel.x {
            l.panel.x - art.right()
        } else {
            assert!(l.panel.right() <= art.x, "art and panel overlap {l:?}");
            art.x - l.panel.right()
        }
    }

    fn some(r: Option<Rect>, what: &str) -> Rect {
        r.unwrap_or_else(|| panic!("no {what}"))
    }

    // Size (B-33)

    #[test]
    fn width_is_the_total_width_art_included() {
        let l = layout(&settings());
        assert_eq!(l.width, 600);
        assert_sane(&l);
        assert_eq!(some(l.art, "art").w + l.panel.w, 600);
    }

    #[test]
    fn art_is_a_square_slot_on_the_left_by_default() {
        let l = layout(&settings());
        let art = some(l.art, "art");
        assert_eq!(art, Rect::new(0, 0, l.height, l.height));
        assert_eq!(l.panel.x, l.height);
    }

    #[test]
    fn art_right_puts_the_slot_on_the_right() {
        let l = layout(&with(|s| s.art_right = true));
        assert_sane(&l);
        assert_eq!(l.panel.x, 0);
        assert_eq!(some(l.art, "art").right(), l.width);
    }

    #[test]
    fn flipping_the_art_mirrors_the_panel_content() {
        let left = layout(&settings());
        let right = layout(&with(|s| s.art_right = true));
        assert_eq!((left.width, left.height), (right.width, right.height));
        for ((name, a), (_, b)) in panel_content(&left).into_iter().zip(panel_content(&right)) {
            assert_eq!(a.x - left.panel.x, b.x - right.panel.x, "{name}");
            assert_eq!((a.y, a.w, a.h), (b.y, b.w, b.h), "{name}");
        }
    }

    #[test]
    fn zero_gap_glues_the_art_to_the_panel() {
        for art_right in [false, true] {
            let l = layout(&with(|s| s.art_right = art_right));
            assert_sane(&l);
            assert_eq!(gap(&l), 0, "{art_right}");
        }
    }

    // Gap (B-39)

    #[test]
    fn gap_sits_between_the_art_and_the_panel() {
        let l = layout(&with(|s| s.art_gap = 24));
        assert_sane(&l);
        let art = some(l.art, "art");
        assert_eq!(art.x, 0);
        assert_eq!(l.panel.x, art.right() + 24);
        assert_eq!(l.panel.right(), 600);
    }

    #[test]
    fn gap_sits_between_the_panel_and_the_art_on_the_right() {
        let l = layout(&with(|s| {
            s.art_gap = 24;
            s.art_right = true;
        }));
        assert_sane(&l);
        let art = some(l.art, "art");
        assert_eq!(l.panel.x, 0);
        assert_eq!(art.x, l.panel.right() + 24);
        assert_eq!(art.right(), 600);
    }

    #[test]
    fn gap_counts_in_the_total_width() {
        let glued = layout(&settings());
        let apart = layout(&with(|s| s.art_gap = 24));
        assert_eq!(apart.width, glued.width);
        assert_eq!(apart.height, glued.height);
        assert_eq!(apart.art.map(|a| a.w), glued.art.map(|a| a.w));
        assert_eq!(apart.panel.w + 24, glued.panel.w);
    }

    #[test]
    fn gap_moves_the_panel_content_with_the_panel() {
        let glued = layout(&settings());
        let apart = layout(&with(|s| s.art_gap = 24));
        let title = some(apart.title, "title");
        assert_eq!(title.x, some(glued.title, "title").x + 24);
        assert_eq!(title.w + 24, some(glued.title, "title").w);
        assert_eq!(apart.message.x, glued.message.x + 24);
    }

    #[test]
    fn gap_without_art_is_ignored() {
        let l = layout(&with(|s| {
            s.show_art = false;
            s.art_gap = 100;
        }));
        assert_sane(&l);
        assert_eq!(l.panel, frame(&l));
        assert_eq!(l.width, 600);
    }

    #[test]
    fn gap_is_kept_within_the_schema_bounds() {
        let l = layout(&with(|s| s.art_gap = u32::MAX));
        assert_sane(&l);
        assert_eq!(gap(&l), 512);
    }

    #[test]
    fn a_wide_gap_in_a_tiny_width_keeps_a_minimal_panel() {
        let l = layout(&with(|s| {
            s.width = 1;
            s.art_gap = 512;
        }));
        assert_sane(&l);
        assert_eq!(gap(&l), 512);
        assert!(some(l.title, "title").w >= l.sizes.title, "{l:?}");
    }

    // Round art (B-38)

    #[test]
    fn square_art_takes_the_corner_radius() {
        for radius in [0, 12, 256] {
            let l = layout(&with(|s| s.corner_radius = radius));
            assert_eq!(l.art_radius, radius);
        }
    }

    #[test]
    fn round_art_is_a_disc_whatever_the_corner_radius() {
        for radius in [0, 12, 256] {
            let l = layout(&with(|s| {
                s.corner_radius = radius;
                s.art_round = true;
            }));
            assert_sane(&l);
            assert_eq!(l.art_radius, some(l.art, "art").w / 2, "{radius}");
            assert_eq!(l.corner_radius, radius, "the panel keeps its radius");
        }
    }

    #[test]
    fn round_art_does_not_move_anything() {
        let square = layout(&settings());
        let round = layout(&with(|s| s.art_round = true));
        assert_eq!(
            Layout {
                art_radius: square.art_radius,
                ..round
            },
            square
        );
    }

    #[test]
    fn hiding_the_art_gives_the_panel_the_whole_width() {
        let l = layout(&with(|s| s.show_art = false));
        assert_sane(&l);
        assert_eq!(l.art, None);
        assert_eq!(l.width, 600);
    }

    #[test]
    fn height_ignores_the_art_and_the_width() {
        let h = layout(&settings()).height;
        assert_eq!(layout(&with(|s| s.show_art = false)).height, h);
        assert_eq!(layout(&with(|s| s.width = 2000)).height, h);
        assert_eq!(layout(&with(|s| s.art_right = true)).height, h);
    }

    #[test]
    fn height_grows_with_each_shown_row() {
        let full = layout(&settings()).height;
        let hidden: [fn(&mut Settings); 3] = [
            |s| s.show_title = false,
            |s| s.show_progress = false,
            |s| {
                s.show_position = false;
                s.show_duration = false;
            },
        ];
        for hide in hidden {
            assert!(layout(&with(hide)).height < full);
        }
        let no_subtitle = layout(&with(|s| {
            s.show_artist = false;
            s.show_album = false;
        }));
        assert!(no_subtitle.height < full);
    }

    #[test]
    fn one_subtitle_field_is_enough_for_the_subtitle_row() {
        let full = layout(&settings());
        for hide in [
            (|s: &mut Settings| s.show_artist = false) as fn(&mut Settings),
            |s| s.show_album = false,
        ] {
            let l = layout(&with(hide));
            assert_eq!(l.subtitle, full.subtitle);
            assert_eq!(l.height, full.height);
        }
    }

    #[test]
    fn one_time_field_is_enough_for_the_time_row() {
        let full = layout(&settings());
        for hide in [
            (|s: &mut Settings| s.show_position = false) as fn(&mut Settings),
            |s| s.show_duration = false,
        ] {
            assert_eq!(layout(&with(hide)).times, full.times);
        }
    }

    #[test]
    fn height_grows_with_the_font_sizes() {
        let base = layout(&settings()).height;
        for grow in [
            (|s: &mut Settings| s.font_size_title += 10) as fn(&mut Settings),
            |s| s.font_size_subtitle += 10,
            |s| s.font_size_time += 10,
        ] {
            assert!(layout(&with(grow)).height > base);
        }
    }

    #[test]
    fn margins_scale_with_the_font_sizes() {
        let base = layout(&settings());
        let double = layout(&with(|s| {
            s.font_size_title *= 2;
            s.font_size_subtitle *= 2;
            s.font_size_time *= 2;
        }));
        // Each row and gap rounds on its own, hence the slack.
        assert!(
            double.height.abs_diff(2 * base.height) <= 4,
            "{base:?} {double:?}"
        );
        let title = some(base.title, "title");
        let doubled = some(double.title, "title");
        assert!((doubled.x - double.panel.x).abs_diff(2 * (title.x - base.panel.x)) <= 1);
        assert!(doubled.y.abs_diff(2 * title.y) <= 1);
    }

    // Rows (B-37)

    #[test]
    fn rows_stack_in_order_without_overlapping() {
        let l = layout(&settings());
        assert_sane(&l);
        let rows = [l.title, l.subtitle, l.bar, l.times].map(|r| some(r, "row"));
        for pair in rows.windows(2) {
            assert!(pair[0].bottom() <= pair[1].y, "{pair:?}");
        }
        let text = rows[0];
        for row in rows {
            assert_eq!(
                (row.x, row.w),
                (text.x, text.w),
                "rows share the text column"
            );
        }
    }

    #[test]
    fn rows_have_their_font_line_height() {
        let l = layout(&settings());
        assert_eq!(
            l.sizes,
            FontSizes {
                title: 32,
                subtitle: 24,
                time: 20
            }
        );
        assert_eq!(some(l.title, "title").h, 40);
        assert_eq!(some(l.subtitle, "subtitle").h, 30);
        assert_eq!(some(l.times, "times").h, 25);
    }

    #[test]
    fn text_column_keeps_a_margin_inside_the_panel() {
        let l = layout(&settings());
        let title = some(l.title, "title");
        let left = title.x - l.panel.x;
        let right = l.panel.right() - title.right();
        let top = title.y;
        let bottom = l.height - some(l.times, "times").bottom();
        assert!(left > 0);
        assert_eq!(left, right);
        assert_eq!(top, bottom);
        assert_eq!(left, top);
    }

    #[test]
    fn the_bar_is_thinner_than_the_time_row() {
        let l = layout(&settings());
        let bar = some(l.bar, "bar");
        assert!(bar.h >= 1 && bar.h < some(l.times, "times").h);
    }

    #[test]
    fn each_toggle_removes_only_its_row() {
        let full = layout(&settings());
        let l = layout(&with(|s| s.show_title = false));
        assert_eq!(l.title, None);
        assert!(l.subtitle.is_some() && l.bar.is_some() && l.times.is_some());
        let l = layout(&with(|s| s.show_progress = false));
        assert_eq!(l.bar, None);
        assert!(l.title.is_some() && l.subtitle.is_some() && l.times.is_some());
        let l = layout(&with(|s| {
            s.show_position = false;
            s.show_duration = false;
        }));
        assert_eq!(l.times, None);
        assert!(l.bar.is_some());
        assert!(full.title.is_some());
    }

    #[test]
    fn title_alone_matches_the_first_mockup() {
        let l = layout(&with(|s| {
            every_text_off(s);
            s.show_title = true;
        }));
        assert_sane(&l);
        let title = some(l.title, "title");
        assert_eq!(title.y, l.height - title.bottom());
        assert_eq!((l.subtitle, l.bar, l.times), (None, None, None));
    }

    // Fallback message (B-23)

    #[test]
    fn message_is_a_title_line_centered_in_the_panel() {
        for s in [
            settings(),
            with(every_text_off),
            with(|s| s.show_art = false),
        ] {
            let l = layout(&s);
            assert_sane(&l);
            assert_eq!(l.message.h, 40);
            assert!(l.message.y.abs_diff(l.height - l.message.bottom()) <= 1);
            let title = layout(&settings())
                .title
                .map(|t| t.x - layout(&settings()).panel.x);
            assert_eq!(Some(l.message.x - l.panel.x), title);
        }
    }

    #[test]
    fn every_text_off_still_holds_one_title_line() {
        let l = layout(&with(every_text_off));
        assert_sane(&l);
        assert_eq!(
            (l.title, l.subtitle, l.bar, l.times, l.pause),
            (None, None, None, None, None)
        );
        let title_only = layout(&with(|s| {
            every_text_off(s);
            s.show_title = true;
        }));
        assert_eq!(l.height, title_only.height);
    }

    #[test]
    fn a_short_panel_still_fits_the_message() {
        let l = layout(&with(|s| {
            every_text_off(s);
            s.show_artist = true;
            s.font_size_subtitle = 8;
        }));
        assert_sane(&l);
        let subtitle = some(l.subtitle, "subtitle");
        assert!(l.message.h > subtitle.h);
        assert!(
            subtitle.y.abs_diff(l.height - subtitle.bottom()) <= 1,
            "content centered"
        );
    }

    // Pause (B-21)

    #[test]
    fn pause_bars_sit_at_the_end_of_the_title_line() {
        let l = layout(&settings());
        let title = some(l.title, "title");
        let pause = l.pause.expect("pause");
        let [first, second] = pause.bars;
        assert_eq!(second.right(), title.right());
        assert!(first.right() < second.x, "a gap between the bars");
        assert_eq!(first.w, second.w);
        assert_eq!((first.y, first.h), (second.y, second.h));
        assert!(title.contains(&first) && title.contains(&second));
        assert!(first.h < title.h && first.h > first.w);
        assert!(
            (first.y - title.y).abs_diff(title.bottom() - first.bottom()) <= 1,
            "centered"
        );
        assert_eq!(
            (pause.title.x, pause.title.y, pause.title.h),
            (title.x, title.y, title.h)
        );
        assert!(
            pause.title.right() < first.x,
            "the title stops before the bars"
        );
    }

    #[test]
    fn no_title_no_pause_icon() {
        assert_eq!(layout(&with(|s| s.show_title = false)).pause, None);
    }

    // Limits

    #[test]
    fn tiny_width_keeps_a_minimal_panel() {
        for show_art in [true, false] {
            let l = layout(&with(|s| {
                s.width = 1;
                s.show_art = show_art;
            }));
            assert_sane(&l);
            let title = some(l.title, "title");
            assert!(title.w >= l.sizes.title, "{l:?}");
            assert!(l.width > 1);
            assert!(l.width < 600);
        }
    }

    #[test]
    fn width_zero_does_not_panic() {
        assert_sane(&layout(&with(|s| s.width = 0)));
    }

    #[test]
    fn tiny_fonts_keep_every_row_visible() {
        let l = layout(&with(|s| {
            s.font_size_title = 1;
            s.font_size_subtitle = 1;
            s.font_size_time = 1;
        }));
        assert_sane(&l);
        for r in [l.title, l.subtitle, l.bar, l.times] {
            let r = some(r, "row");
            assert!(r.w > 0 && r.h > 0, "{l:?}");
        }
    }

    #[test]
    fn zero_font_sizes_are_read_as_one_pixel() {
        let zero = layout(&with(|s| {
            s.font_size_title = 0;
            s.font_size_subtitle = 0;
            s.font_size_time = 0;
        }));
        assert_sane(&zero);
        assert_eq!(
            zero.sizes,
            FontSizes {
                title: 1,
                subtitle: 1,
                time: 1
            }
        );
    }

    #[test]
    fn huge_settings_stay_within_the_schema_bounds() {
        let l = layout(&with(|s| {
            s.width = u32::MAX;
            s.font_size_title = u32::MAX;
            s.font_size_subtitle = u32::MAX;
            s.font_size_time = u32::MAX;
        }));
        assert_sane(&l);
        assert_eq!(
            l.sizes,
            FontSizes {
                title: 512,
                subtitle: 512,
                time: 512
            }
        );
        assert!(l.width <= 8192 && l.height <= 8192, "{l:?}");
    }

    #[test]
    fn corner_radius_is_kept_within_the_schema_bounds() {
        assert_eq!(layout(&settings()).corner_radius, 12);
        assert_eq!(layout(&with(|s| s.corner_radius = 0)).corner_radius, 0);
        assert_eq!(
            layout(&with(|s| s.corner_radius = u32::MAX)).corner_radius,
            256
        );
    }

    #[test]
    fn corner_radius_does_not_move_anything() {
        let base = layout(&settings());
        let round = layout(&with(|s| s.corner_radius = 256));
        assert_eq!(
            Layout {
                corner_radius: 12,
                art_radius: 12,
                ..round
            },
            base
        );
    }

    #[test]
    fn huge_fonts_in_a_tiny_width_do_not_panic() {
        let l = layout(&with(|s| {
            s.width = 1;
            s.font_size_title = 512;
            s.font_size_subtitle = 512;
            s.font_size_time = 512;
        }));
        assert_sane(&l);
    }
}
