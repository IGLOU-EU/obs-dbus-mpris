//! The single settings schema: keys, kinds and defaults (besoins.md, section 5).
//!
//! The OBS glue walks [`FIELDS`] to declare properties and defaults, and
//! [`Settings::from_lookup`] reads the same table, so each default is
//! written once, here.

use std::time::Duration;

use crate::model::{Rgba, Settings};

/// What a setting holds, and so which OBS property declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// Kept within `[min, max]`.
    Int {
        min: u32,
        max: u32,
        unit: Unit,
    },
    /// Color with alpha (B-31).
    Color,
    Text,
    /// Player instance chosen from those present on the bus, stored as
    /// its full bus name (B-03).
    Player,
}

/// What an integer setting counts, shown beside its value so that the
/// properties need no explanation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Pixel,
    PixelPerSecond,
    Millisecond,
}

impl Unit {
    /// The same in every language the plugin speaks (B-46).
    pub const fn symbol(self) -> &'static str {
        match self {
            Unit::Pixel => "px",
            Unit::PixelPerSecond => "px/s",
            Unit::Millisecond => "ms",
        }
    }
}

/// Default value of a setting, in a form usable in a `const` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultValue {
    Bool(bool),
    Int(u32),
    Color(Rgba),
    Text(&'static str),
    /// Text translated by OBS (B-23). The glue asks OBS for `locale_key`,
    /// the core, which has no locale, uses `english`.
    Translated {
        locale_key: &'static str,
        english: &'static str,
    },
}

/// One setting of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub key: &'static str,
    pub kind: Kind,
    pub default: DefaultValue,
}

/// A setting value as read from OBS.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Color(Rgba),
    Text(String),
}

/// Far beyond any real overlay. The bound only stops a typo from asking
/// for a gigantic texture.
pub const MAX_SIDE: u32 = 8192;
/// Same purpose as [`MAX_SIDE`], for glyph rasterization.
pub const MAX_FONT_SIZE: u32 = 512;
/// Rounder than any real overlay needs.
pub const MAX_CORNER_RADIUS: u32 = 256;
/// Wider than any real overlay needs.
pub const MAX_ART_GAP: u32 = 512;
/// Faster than anyone can read.
pub const MAX_SCROLL_SPEED: u32 = 1000;
/// In milliseconds, a minute: longer than any real pause.
pub const MAX_SCROLL_PAUSE: u32 = 60_000;

// Flags, colors and integers come from the constructor of their kind, so
// their default fits their kind by construction. `PLAYER` and
// `FALLBACK_TEXT` are alone of their kind, and a constructor would serve a
// single call.
const PLAYER: Field = Field {
    key: "player",
    kind: Kind::Player,
    default: DefaultValue::Text(""),
};
const AUTO_SELECT: Field = flag("auto_select", true);
const SHOW_ART: Field = flag("show_art", true);
const FETCH_ART_ONLINE: Field = flag("fetch_art_online", false);
const SHOW_TITLE: Field = flag("show_title", true);
const SHOW_ARTIST: Field = flag("show_artist", true);
const SHOW_ALBUM: Field = flag("show_album", true);
const SHOW_PROGRESS: Field = flag("show_progress", true);
const SHOW_POSITION: Field = flag("show_position", true);
const SHOW_DURATION: Field = flag("show_duration", true);
const SHOW_REMAINING: Field = flag("show_remaining", false);
const ART_RIGHT: Field = flag("art_right", false);
const ART_ROUND: Field = flag("art_round", false);
/// Down to 0, which glues the thumbnail to the panel (B-39).
const ART_GAP: Field = int("art_gap", 0, 0, MAX_ART_GAP, Unit::Pixel);
const COLOR_BACKGROUND: Field = color("color_background", Rgba::new(0x00, 0x00, 0x00, 153));
const COLOR_TEXT: Field = color("color_text", Rgba::new(0xFF, 0xFF, 0xFF, 255));
const COLOR_BAR_FILL: Field = color("color_bar_fill", Rgba::new(0xFF, 0xFF, 0xFF, 255));
const COLOR_BAR_TRACK: Field = color("color_bar_track", Rgba::new(0xFF, 0xFF, 0xFF, 64));
const WIDTH: Field = size("width", 600, MAX_SIDE);
const FONT_SIZE_TITLE: Field = size("font_size_title", 32, MAX_FONT_SIZE);
const FONT_SIZE_SUBTITLE: Field = size("font_size_subtitle", 24, MAX_FONT_SIZE);
const FONT_SIZE_TIME: Field = size("font_size_time", 20, MAX_FONT_SIZE);
/// Down to 0, which gives square corners (B-38).
const CORNER_RADIUS: Field = int("corner_radius", 12, 0, MAX_CORNER_RADIUS, Unit::Pixel);
/// Down to 0, which cuts a long text with an ellipsis instead (B-36).
const SCROLL_SPEED: Field = int(
    "scroll_speed",
    40,
    0,
    MAX_SCROLL_SPEED,
    Unit::PixelPerSecond,
);
const SCROLL_PAUSE: Field = int("scroll_pause", 2000, 0, MAX_SCROLL_PAUSE, Unit::Millisecond);
const FALLBACK_TEXT: Field = Field {
    key: "fallback_text",
    kind: Kind::Text,
    default: DefaultValue::Translated {
        locale_key: "FallbackText",
        english: "Nothing playing",
    },
};

/// Every setting, in the order of besoins.md section 5.
pub const FIELDS: &[Field] = &[
    PLAYER,
    AUTO_SELECT,
    SHOW_ART,
    FETCH_ART_ONLINE,
    SHOW_TITLE,
    SHOW_ARTIST,
    SHOW_ALBUM,
    SHOW_PROGRESS,
    SHOW_POSITION,
    SHOW_DURATION,
    SHOW_REMAINING,
    ART_RIGHT,
    ART_ROUND,
    ART_GAP,
    COLOR_BACKGROUND,
    COLOR_TEXT,
    COLOR_BAR_FILL,
    COLOR_BAR_TRACK,
    WIDTH,
    FONT_SIZE_TITLE,
    FONT_SIZE_SUBTITLE,
    FONT_SIZE_TIME,
    CORNER_RADIUS,
    SCROLL_SPEED,
    SCROLL_PAUSE,
    FALLBACK_TEXT,
];

const fn flag(key: &'static str, default: bool) -> Field {
    Field {
        key,
        kind: Kind::Bool,
        default: DefaultValue::Bool(default),
    }
}

const fn color(key: &'static str, default: Rgba) -> Field {
    Field {
        key,
        kind: Kind::Color,
        default: DefaultValue::Color(default),
    }
}

const fn int(key: &'static str, default: u32, min: u32, max: u32, unit: Unit) -> Field {
    Field {
        key,
        kind: Kind::Int { min, max, unit },
        default: DefaultValue::Int(default),
    }
}

/// At least one pixel: a zero size would make the source or a text vanish
/// with no visible reason.
const fn size(key: &'static str, default: u32, max: u32) -> Field {
    int(key, default, 1, max, Unit::Pixel)
}

impl Settings {
    /// Builds the settings from `get`, which returns the stored value of a
    /// key. A missing or wrong-typed value takes the schema default,
    /// integers are clamped to their bounds, and a blank `player` means no
    /// remembered player.
    pub fn from_lookup(get: impl Fn(&str) -> Option<SettingValue>) -> Settings {
        let player = read_text(&get, &PLAYER);
        let player = player.trim();
        Settings {
            player: (!player.is_empty()).then(|| player.to_owned()),
            auto_select: read_bool(&get, &AUTO_SELECT),
            show_art: read_bool(&get, &SHOW_ART),
            fetch_art_online: read_bool(&get, &FETCH_ART_ONLINE),
            show_title: read_bool(&get, &SHOW_TITLE),
            show_artist: read_bool(&get, &SHOW_ARTIST),
            show_album: read_bool(&get, &SHOW_ALBUM),
            show_progress: read_bool(&get, &SHOW_PROGRESS),
            show_position: read_bool(&get, &SHOW_POSITION),
            show_duration: read_bool(&get, &SHOW_DURATION),
            show_remaining: read_bool(&get, &SHOW_REMAINING),
            art_right: read_bool(&get, &ART_RIGHT),
            art_round: read_bool(&get, &ART_ROUND),
            art_gap: read_int(&get, &ART_GAP),
            color_background: read_color(&get, &COLOR_BACKGROUND),
            color_text: read_color(&get, &COLOR_TEXT),
            color_bar_fill: read_color(&get, &COLOR_BAR_FILL),
            color_bar_track: read_color(&get, &COLOR_BAR_TRACK),
            width: read_int(&get, &WIDTH),
            font_size_title: read_int(&get, &FONT_SIZE_TITLE),
            font_size_subtitle: read_int(&get, &FONT_SIZE_SUBTITLE),
            font_size_time: read_int(&get, &FONT_SIZE_TIME),
            corner_radius: read_int(&get, &CORNER_RADIUS),
            scroll_speed: read_int(&get, &SCROLL_SPEED),
            scroll_pause: Duration::from_millis(read_int(&get, &SCROLL_PAUSE).into()),
            fallback_text: read_text(&get, &FALLBACK_TEXT),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::from_lookup(|_| None)
    }
}

impl Rgba {
    /// From the OBS color layout, `0xAABBGGRR`.
    pub const fn from_obs(value: u32) -> Rgba {
        let [r, g, b, a] = value.to_le_bytes();
        Rgba::new(r, g, b, a)
    }

    /// To the OBS color layout, `0xAABBGGRR`.
    pub const fn to_obs(self) -> u32 {
        u32::from_le_bytes([self.r, self.g, self.b, self.a])
    }
}

// The `unreachable!` arms below hold because `from_lookup` gives each reader
// only fields of its kind, and every default matches its field: the
// constructors ensure it for flags, colors and integers, and the literals
// `PLAYER` and `FALLBACK_TEXT` carry a text default, which `read_text`
// accepts. A slip panics in `defaults_match_the_table`, which runs every
// reader on its default.

fn read_bool(get: &impl Fn(&str) -> Option<SettingValue>, field: &Field) -> bool {
    match (get(field.key), field.default) {
        (Some(SettingValue::Bool(v)), _) | (_, DefaultValue::Bool(v)) => v,
        _ => unreachable!("{} is not a bool field", field.key),
    }
}

fn read_color(get: &impl Fn(&str) -> Option<SettingValue>, field: &Field) -> Rgba {
    match (get(field.key), field.default) {
        (Some(SettingValue::Color(v)), _) | (_, DefaultValue::Color(v)) => v,
        _ => unreachable!("{} is not a color field", field.key),
    }
}

fn read_int(get: &impl Fn(&str) -> Option<SettingValue>, field: &Field) -> u32 {
    let (Kind::Int { min, max, .. }, DefaultValue::Int(default)) = (field.kind, field.default)
    else {
        unreachable!("{} is not an integer field", field.key)
    };
    let value = match get(field.key) {
        Some(SettingValue::Int(v)) => u32::try_from(v.max(0)).unwrap_or(u32::MAX),
        _ => default,
    };
    value.clamp(min, max)
}

fn read_text(get: &impl Fn(&str) -> Option<SettingValue>, field: &Field) -> String {
    match (get(field.key), field.default) {
        (Some(SettingValue::Text(v)), _) => v,
        (_, DefaultValue::Text(v) | DefaultValue::Translated { english: v, .. }) => v.to_owned(),
        _ => unreachable!("{} is not a text field", field.key),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::time::Duration;

    use super::*;

    const EXPECTED_KEYS: [&str; 26] = [
        "player",
        "auto_select",
        "show_art",
        "fetch_art_online",
        "show_title",
        "show_artist",
        "show_album",
        "show_progress",
        "show_position",
        "show_duration",
        "show_remaining",
        "art_right",
        "art_round",
        "art_gap",
        "color_background",
        "color_text",
        "color_bar_fill",
        "color_bar_track",
        "width",
        "font_size_title",
        "font_size_subtitle",
        "font_size_time",
        "corner_radius",
        "scroll_speed",
        "scroll_pause",
        "fallback_text",
    ];

    fn field(key: &str) -> Option<&'static Field> {
        FIELDS.iter().find(|f| f.key == key)
    }

    /// Whether `from_lookup` must take `value` for a field of `kind`.
    fn accepts(kind: Kind, value: &SettingValue) -> bool {
        matches!(
            (kind, value),
            (Kind::Bool, SettingValue::Bool(_))
                | (Kind::Int { .. }, SettingValue::Int(_))
                | (Kind::Color, SettingValue::Color(_))
                | (Kind::Text | Kind::Player, SettingValue::Text(_))
        )
    }

    fn int_bounds(key: &str) -> (u32, u32) {
        match field(key).map(|f| f.kind) {
            Some(Kind::Int { min, max, .. }) => (min, max),
            other => panic!("{key} is not an integer field: {other:?}"),
        }
    }

    fn unit(key: &str) -> Unit {
        match field(key).map(|f| f.kind) {
            Some(Kind::Int { unit, .. }) => unit,
            other => panic!("{key} is not an integer field: {other:?}"),
        }
    }

    fn with_player(value: &str) -> Settings {
        Settings::from_lookup(|key| (key == "player").then(|| SettingValue::Text(value.to_owned())))
    }

    fn table_defaults() -> Settings {
        Settings {
            player: None,
            auto_select: true,
            show_art: true,
            fetch_art_online: false,
            show_title: true,
            show_artist: true,
            show_album: true,
            show_progress: true,
            show_position: true,
            show_duration: true,
            show_remaining: false,
            art_right: false,
            art_round: false,
            art_gap: 0,
            color_background: Rgba::new(0x00, 0x00, 0x00, 153),
            color_text: Rgba::new(0xFF, 0xFF, 0xFF, 255),
            color_bar_fill: Rgba::new(0xFF, 0xFF, 0xFF, 255),
            color_bar_track: Rgba::new(0xFF, 0xFF, 0xFF, 64),
            width: 600,
            font_size_title: 32,
            font_size_subtitle: 24,
            font_size_time: 20,
            corner_radius: 12,
            scroll_speed: 40,
            scroll_pause: Duration::from_millis(2000),
            fallback_text: "Nothing playing".to_owned(),
        }
    }

    #[test]
    fn keys_are_exactly_the_documented_ones_in_order() {
        let keys: Vec<&str> = FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(keys, EXPECTED_KEYS);
    }

    #[test]
    fn keys_are_unique() {
        let unique: HashSet<&str> = FIELDS.iter().map(|f| f.key).collect();
        assert_eq!(unique.len(), FIELDS.len());
    }

    #[test]
    fn kinds_match_the_table() {
        for f in FIELDS {
            let expected = match f.key {
                "player" => "player",
                "auto_select" | "fetch_art_online" | "art_right" | "art_round" => "bool",
                k if k.starts_with("show_") => "bool",
                k if k.starts_with("color_") => "color",
                "width" | "font_size_title" | "font_size_subtitle" | "font_size_time"
                | "corner_radius" | "art_gap" | "scroll_speed" | "scroll_pause" => "int",
                "fallback_text" => "text",
                other => panic!("unexpected key {other}"),
            };
            let actual = match f.kind {
                Kind::Bool => "bool",
                Kind::Int { .. } => "int",
                Kind::Color => "color",
                Kind::Text => "text",
                Kind::Player => "player",
            };
            assert_eq!(actual, expected, "{}", f.key);
        }
    }

    #[test]
    fn defaults_match_the_table() {
        assert_eq!(Settings::default(), table_defaults());
    }

    #[test]
    fn missing_values_take_the_defaults() {
        assert_eq!(Settings::from_lookup(|_| None), table_defaults());
    }

    #[test]
    fn fallback_text_default_is_translated_by_obs() {
        assert_eq!(
            field("fallback_text").map(|f| f.default),
            Some(DefaultValue::Translated {
                locale_key: "FallbackText",
                english: "Nothing playing",
            })
        );
    }

    #[test]
    fn player_default_is_empty() {
        assert_eq!(
            field("player").map(|f| f.default),
            Some(DefaultValue::Text(""))
        );
    }

    #[test]
    fn auto_select_is_on_unless_stored_off() {
        use SettingValue::{Bool, Color, Int, Text};
        let read = |stored: Option<SettingValue>| {
            Settings::from_lookup(|key| (key == "auto_select").then(|| stored.clone()).flatten())
                .auto_select
        };
        assert!(read(None));
        assert!(read(Some(Bool(true))));
        assert!(!read(Some(Bool(false))));
        for wrong in [
            Int(0),
            Text("false".to_owned()),
            Text(String::new()),
            Color(Rgba::new(0, 0, 0, 0)),
        ] {
            assert!(read(Some(wrong.clone())), "{wrong:?}");
        }
    }

    #[test]
    fn default_colors_in_obs_layout() {
        let obs = |key: &str| match field(key).map(|f| f.default) {
            Some(DefaultValue::Color(c)) => c.to_obs(),
            other => panic!("{key} has no color default: {other:?}"),
        };
        assert_eq!(obs("color_background"), 0x9900_0000);
        assert_eq!(obs("color_text"), 0xFFFF_FFFF);
        assert_eq!(obs("color_bar_fill"), 0xFFFF_FFFF);
        assert_eq!(obs("color_bar_track"), 0x40FF_FFFF);
    }

    #[test]
    fn integer_bounds_are_sane() {
        let ints: Vec<&Field> = FIELDS
            .iter()
            .filter(|f| matches!(f.kind, Kind::Int { .. }))
            .collect();
        assert_eq!(ints.len(), 8);
        for key in [
            "width",
            "font_size_title",
            "font_size_subtitle",
            "font_size_time",
        ] {
            let (min, max) = int_bounds(key);
            assert!(min >= 1, "{key} allows sizes below 1");
            assert!((512..=16384).contains(&max), "{key} max {max}");
        }
    }

    #[test]
    fn amounts_that_may_be_zero_have_their_documented_bounds() {
        assert_eq!(int_bounds("corner_radius"), (0, 256));
        assert_eq!(int_bounds("art_gap"), (0, 512));
        assert_eq!(int_bounds("scroll_speed"), (0, 1000));
        assert_eq!(int_bounds("scroll_pause"), (0, 60_000));
    }

    #[test]
    fn every_integer_names_its_unit() {
        for key in [
            "width",
            "font_size_title",
            "font_size_subtitle",
            "font_size_time",
            "corner_radius",
            "art_gap",
        ] {
            assert_eq!(unit(key), Unit::Pixel, "{key}");
        }
        assert_eq!(unit("scroll_speed"), Unit::PixelPerSecond);
        assert_eq!(unit("scroll_pause"), Unit::Millisecond);
        assert_eq!(Unit::Pixel.symbol(), "px");
        assert_eq!(Unit::PixelPerSecond.symbol(), "px/s");
        assert_eq!(Unit::Millisecond.symbol(), "ms");
    }

    #[test]
    fn zero_gap_glues_and_zero_speed_stops_the_scrolling() {
        let read = |key: &'static str| {
            Settings::from_lookup(|k| (k == key).then_some(SettingValue::Int(0)))
        };
        assert_eq!(read("art_gap").art_gap, 0);
        assert_eq!(read("scroll_speed").scroll_speed, 0);
        assert_eq!(read("scroll_pause").scroll_pause, Duration::ZERO);
    }

    #[test]
    fn scroll_pause_is_read_in_milliseconds() {
        let read = |v: i64| {
            Settings::from_lookup(|k| (k == "scroll_pause").then_some(SettingValue::Int(v)))
                .scroll_pause
        };
        assert_eq!(read(1500), Duration::from_millis(1500));
        assert_eq!(read(-1), Duration::ZERO);
        assert_eq!(read(i64::MAX), Duration::from_secs(60));
    }

    #[test]
    fn corner_radius_goes_down_to_square_corners() {
        assert_eq!(int_bounds("corner_radius"), (0, 256));
        let read = |v: i64| {
            Settings::from_lookup(|k| (k == "corner_radius").then_some(SettingValue::Int(v)))
                .corner_radius
        };
        assert_eq!(read(0), 0);
        assert_eq!(read(-5), 0);
        assert_eq!(read(1000), 256);
    }

    #[test]
    fn stored_values_are_taken() {
        use SettingValue::{Bool, Color, Int, Text};
        let stored = |key: &str| {
            Some(match key {
                "player" => Text("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA".to_owned()),
                "auto_select" => Bool(false),
                "show_art" => Bool(false),
                "fetch_art_online" => Bool(true),
                "show_title" => Bool(false),
                "show_artist" => Bool(false),
                "show_album" => Bool(false),
                "show_progress" => Bool(false),
                "show_position" => Bool(false),
                "show_duration" => Bool(false),
                "show_remaining" => Bool(true),
                "art_right" => Bool(true),
                "art_round" => Bool(true),
                "art_gap" => Int(24),
                "color_background" => Color(Rgba::new(1, 2, 3, 4)),
                "color_text" => Color(Rgba::new(5, 6, 7, 8)),
                "color_bar_fill" => Color(Rgba::new(9, 10, 11, 12)),
                "color_bar_track" => Color(Rgba::new(13, 14, 15, 16)),
                "width" => Int(800),
                "font_size_title" => Int(40),
                "font_size_subtitle" => Int(30),
                "font_size_time" => Int(10),
                "corner_radius" => Int(30),
                "scroll_speed" => Int(80),
                "scroll_pause" => Int(500),
                "fallback_text" => Text("Aucune lecture".to_owned()),
                _ => return None,
            })
        };
        let expected = Settings {
            player: Some("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA".to_owned()),
            auto_select: false,
            show_art: false,
            fetch_art_online: true,
            show_title: false,
            show_artist: false,
            show_album: false,
            show_progress: false,
            show_position: false,
            show_duration: false,
            show_remaining: true,
            art_right: true,
            art_round: true,
            art_gap: 24,
            color_background: Rgba::new(1, 2, 3, 4),
            color_text: Rgba::new(5, 6, 7, 8),
            color_bar_fill: Rgba::new(9, 10, 11, 12),
            color_bar_track: Rgba::new(13, 14, 15, 16),
            width: 800,
            font_size_title: 40,
            font_size_subtitle: 30,
            font_size_time: 10,
            corner_radius: 30,
            scroll_speed: 80,
            scroll_pause: Duration::from_millis(500),
            fallback_text: "Aucune lecture".to_owned(),
        };
        assert_eq!(Settings::from_lookup(stored), expected);
    }

    #[test]
    fn wrong_typed_values_take_the_defaults() {
        let candidates = [
            SettingValue::Bool(false),
            SettingValue::Bool(true),
            SettingValue::Int(0),
            SettingValue::Int(1),
            SettingValue::Int(0x1234_5678),
            SettingValue::Color(Rgba::new(1, 2, 3, 4)),
            SettingValue::Text(String::new()),
            SettingValue::Text("true".to_owned()),
        ];
        for wrong in candidates {
            let settings = Settings::from_lookup(|key| {
                let f = field(key)?;
                (!accepts(f.kind, &wrong)).then(|| wrong.clone())
            });
            assert_eq!(settings, table_defaults(), "{wrong:?}");
        }
    }

    #[test]
    fn lookup_asks_each_schema_key_once() {
        let asked = RefCell::new(Vec::new());
        let _ = Settings::from_lookup(|key| {
            asked.borrow_mut().push(key.to_owned());
            None
        });
        let mut asked = asked.into_inner();
        asked.sort();
        let mut expected = EXPECTED_KEYS.map(str::to_owned).to_vec();
        expected.sort();
        assert_eq!(asked, expected);
    }

    #[test]
    fn integers_are_clamped_to_their_bounds() {
        type Getter = fn(&Settings) -> u32;
        let sizes: [(&str, Getter); 8] = [
            ("art_gap", |s| s.art_gap),
            ("width", |s| s.width),
            ("font_size_title", |s| s.font_size_title),
            ("font_size_subtitle", |s| s.font_size_subtitle),
            ("font_size_time", |s| s.font_size_time),
            ("corner_radius", |s| s.corner_radius),
            ("scroll_speed", |s| s.scroll_speed),
            ("scroll_pause", |s| {
                u32::try_from(s.scroll_pause.as_millis()).expect("bounded")
            }),
        ];
        for (key, get) in sizes {
            let (min, max) = int_bounds(key);
            let read = |v: i64| {
                get(&Settings::from_lookup(|k| {
                    (k == key).then_some(SettingValue::Int(v))
                }))
            };
            assert_eq!(read(i64::MIN), min, "{key}");
            assert_eq!(read(-1), min, "{key}");
            assert_eq!(read(0), min, "{key}");
            assert_eq!(read(i64::from(min)), min, "{key}");
            assert_eq!(read(i64::from(min) + 1), min + 1, "{key}");
            assert_eq!(read(i64::from(max) - 1), max - 1, "{key}");
            assert_eq!(read(i64::from(max)), max, "{key}");
            assert_eq!(read(i64::from(max) + 1), max, "{key}");
            assert_eq!(read(i64::from(u32::MAX) + 1), max, "{key}");
            assert_eq!(read(i64::MAX), max, "{key}");
        }
    }

    #[test]
    fn blank_player_means_none() {
        for blank in ["", " ", "\t\n "] {
            assert_eq!(with_player(blank).player, None, "{blank:?}");
        }
    }

    #[test]
    fn player_is_the_whole_bus_name_of_the_instance() {
        let bus_name = "org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA";
        assert_eq!(with_player(bus_name).player.as_deref(), Some(bus_name));
    }

    #[test]
    fn player_is_trimmed() {
        assert_eq!(
            with_player(" \torg.mpris.MediaPlayer2.vlc \n")
                .player
                .as_deref(),
            Some("org.mpris.MediaPlayer2.vlc")
        );
    }

    #[test]
    fn player_that_is_not_an_mpris_name_is_kept_for_the_selector() {
        assert_eq!(with_player("mpv").player.as_deref(), Some("mpv"));
    }

    #[test]
    fn empty_fallback_text_is_kept() {
        let s = Settings::from_lookup(|k| {
            (k == "fallback_text").then(|| SettingValue::Text(String::new()))
        });
        assert_eq!(s.fallback_text, "");
    }

    #[test]
    fn obs_color_known_value() {
        assert_eq!(
            Rgba::from_obs(0x8033_2211),
            Rgba::new(0x11, 0x22, 0x33, 0x80)
        );
        assert_eq!(Rgba::new(0x11, 0x22, 0x33, 0x80).to_obs(), 0x8033_2211);
    }

    #[test]
    fn obs_color_round_trip() {
        for value in [
            0,
            0xFFFF_FFFF,
            0x9900_0000,
            0x40FF_FFFF,
            0x0102_0304,
            0xDEAD_BEEF,
        ] {
            assert_eq!(Rgba::from_obs(value).to_obs(), value);
        }
        for color in [Rgba::new(0, 0, 0, 0), Rgba::new(255, 0, 128, 7)] {
            assert_eq!(Rgba::from_obs(color.to_obs()), color);
        }
    }
}
