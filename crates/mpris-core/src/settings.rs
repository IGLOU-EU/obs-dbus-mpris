//! The single settings schema: keys, kinds and defaults (besoins.md, section 5).
//!
//! The OBS glue walks [`FIELDS`] to declare properties and defaults, and
//! [`Settings::from_lookup`] reads the same table, so each default is
//! written once, here.

use crate::model::{Rgba, Settings};

/// What a setting holds, and so which OBS property declares it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bool,
    /// Pixel size, kept within `[min, max]`.
    Int {
        min: u32,
        max: u32,
    },
    /// Color with alpha (B-31).
    Color,
    Text,
    /// Player instance chosen from those present on the bus, stored as
    /// its full bus name (B-03).
    Player,
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
const MAX_SIDE: u32 = 8192;
/// Same purpose as [`MAX_SIDE`], for glyph rasterization.
const MAX_FONT_SIZE: u32 = 512;

// Flags, colors and sizes come from the constructor of their kind, so their
// default fits their kind by construction. `PLAYER` and `FALLBACK_TEXT` are
// alone of their kind, and a constructor would serve a single call.
const PLAYER: Field = Field {
    key: "player",
    kind: Kind::Player,
    default: DefaultValue::Text(""),
};
const SHOW_ART: Field = flag("show_art", true);
const FETCH_ART_ONLINE: Field = flag("fetch_art_online", false);
const SHOW_TITLE: Field = flag("show_title", true);
const SHOW_ARTIST: Field = flag("show_artist", true);
const SHOW_ALBUM: Field = flag("show_album", true);
const SHOW_PROGRESS: Field = flag("show_progress", true);
const SHOW_POSITION: Field = flag("show_position", true);
const SHOW_DURATION: Field = flag("show_duration", true);
const ART_RIGHT: Field = flag("art_right", false);
const COLOR_BACKGROUND: Field = color("color_background", Rgba::new(0x00, 0x00, 0x00, 153));
const COLOR_TEXT: Field = color("color_text", Rgba::new(0xFF, 0xFF, 0xFF, 255));
const COLOR_BAR_FILL: Field = color("color_bar_fill", Rgba::new(0xFF, 0xFF, 0xFF, 255));
const COLOR_BAR_TRACK: Field = color("color_bar_track", Rgba::new(0xFF, 0xFF, 0xFF, 64));
const WIDTH: Field = size("width", 600, MAX_SIDE);
const HEIGHT: Field = size("height", 120, MAX_SIDE);
const FONT_SIZE_TITLE: Field = size("font_size_title", 32, MAX_FONT_SIZE);
const FONT_SIZE_SUBTITLE: Field = size("font_size_subtitle", 24, MAX_FONT_SIZE);
const FONT_SIZE_TIME: Field = size("font_size_time", 20, MAX_FONT_SIZE);
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
    SHOW_ART,
    FETCH_ART_ONLINE,
    SHOW_TITLE,
    SHOW_ARTIST,
    SHOW_ALBUM,
    SHOW_PROGRESS,
    SHOW_POSITION,
    SHOW_DURATION,
    ART_RIGHT,
    COLOR_BACKGROUND,
    COLOR_TEXT,
    COLOR_BAR_FILL,
    COLOR_BAR_TRACK,
    WIDTH,
    HEIGHT,
    FONT_SIZE_TITLE,
    FONT_SIZE_SUBTITLE,
    FONT_SIZE_TIME,
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

/// At least one pixel: a zero size would make the source or a text vanish
/// with no visible reason.
const fn size(key: &'static str, default: u32, max: u32) -> Field {
    Field {
        key,
        kind: Kind::Int { min: 1, max },
        default: DefaultValue::Int(default),
    }
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
            show_art: read_bool(&get, &SHOW_ART),
            fetch_art_online: read_bool(&get, &FETCH_ART_ONLINE),
            show_title: read_bool(&get, &SHOW_TITLE),
            show_artist: read_bool(&get, &SHOW_ARTIST),
            show_album: read_bool(&get, &SHOW_ALBUM),
            show_progress: read_bool(&get, &SHOW_PROGRESS),
            show_position: read_bool(&get, &SHOW_POSITION),
            show_duration: read_bool(&get, &SHOW_DURATION),
            art_right: read_bool(&get, &ART_RIGHT),
            color_background: read_color(&get, &COLOR_BACKGROUND),
            color_text: read_color(&get, &COLOR_TEXT),
            color_bar_fill: read_color(&get, &COLOR_BAR_FILL),
            color_bar_track: read_color(&get, &COLOR_BAR_TRACK),
            width: read_size(&get, &WIDTH),
            height: read_size(&get, &HEIGHT),
            font_size_title: read_size(&get, &FONT_SIZE_TITLE),
            font_size_subtitle: read_size(&get, &FONT_SIZE_SUBTITLE),
            font_size_time: read_size(&get, &FONT_SIZE_TIME),
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
// constructors ensure it for flags, colors and sizes, and the literals
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

fn read_size(get: &impl Fn(&str) -> Option<SettingValue>, field: &Field) -> u32 {
    let (Kind::Int { min, max }, DefaultValue::Int(default)) = (field.kind, field.default) else {
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

    use super::*;

    const EXPECTED_KEYS: [&str; 20] = [
        "player",
        "show_art",
        "fetch_art_online",
        "show_title",
        "show_artist",
        "show_album",
        "show_progress",
        "show_position",
        "show_duration",
        "art_right",
        "color_background",
        "color_text",
        "color_bar_fill",
        "color_bar_track",
        "width",
        "height",
        "font_size_title",
        "font_size_subtitle",
        "font_size_time",
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
            Some(Kind::Int { min, max }) => (min, max),
            other => panic!("{key} is not an integer field: {other:?}"),
        }
    }

    fn with_player(value: &str) -> Settings {
        Settings::from_lookup(|key| (key == "player").then(|| SettingValue::Text(value.to_owned())))
    }

    fn table_defaults() -> Settings {
        Settings {
            player: None,
            show_art: true,
            fetch_art_online: false,
            show_title: true,
            show_artist: true,
            show_album: true,
            show_progress: true,
            show_position: true,
            show_duration: true,
            art_right: false,
            color_background: Rgba::new(0x00, 0x00, 0x00, 153),
            color_text: Rgba::new(0xFF, 0xFF, 0xFF, 255),
            color_bar_fill: Rgba::new(0xFF, 0xFF, 0xFF, 255),
            color_bar_track: Rgba::new(0xFF, 0xFF, 0xFF, 64),
            width: 600,
            height: 120,
            font_size_title: 32,
            font_size_subtitle: 24,
            font_size_time: 20,
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
                k if k.starts_with("show_") || k == "fetch_art_online" || k == "art_right" => {
                    "bool"
                }
                k if k.starts_with("color_") => "color",
                "width" | "height" | "font_size_title" | "font_size_subtitle"
                | "font_size_time" => "int",
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
        assert_eq!(ints.len(), 5);
        for f in ints {
            let (min, max) = int_bounds(f.key);
            assert!(min >= 1, "{} allows sizes below 1", f.key);
            assert!((512..=16384).contains(&max), "{} max {max}", f.key);
        }
    }

    #[test]
    fn stored_values_are_taken() {
        use SettingValue::{Bool, Color, Int, Text};
        let stored = |key: &str| {
            Some(match key {
                "player" => Text("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA".to_owned()),
                "show_art" => Bool(false),
                "fetch_art_online" => Bool(true),
                "show_title" => Bool(false),
                "show_artist" => Bool(false),
                "show_album" => Bool(false),
                "show_progress" => Bool(false),
                "show_position" => Bool(false),
                "show_duration" => Bool(false),
                "art_right" => Bool(true),
                "color_background" => Color(Rgba::new(1, 2, 3, 4)),
                "color_text" => Color(Rgba::new(5, 6, 7, 8)),
                "color_bar_fill" => Color(Rgba::new(9, 10, 11, 12)),
                "color_bar_track" => Color(Rgba::new(13, 14, 15, 16)),
                "width" => Int(800),
                "height" => Int(200),
                "font_size_title" => Int(40),
                "font_size_subtitle" => Int(30),
                "font_size_time" => Int(10),
                "fallback_text" => Text("Aucune lecture".to_owned()),
                _ => return None,
            })
        };
        let expected = Settings {
            player: Some("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA".to_owned()),
            show_art: false,
            fetch_art_online: true,
            show_title: false,
            show_artist: false,
            show_album: false,
            show_progress: false,
            show_position: false,
            show_duration: false,
            art_right: true,
            color_background: Rgba::new(1, 2, 3, 4),
            color_text: Rgba::new(5, 6, 7, 8),
            color_bar_fill: Rgba::new(9, 10, 11, 12),
            color_bar_track: Rgba::new(13, 14, 15, 16),
            width: 800,
            height: 200,
            font_size_title: 40,
            font_size_subtitle: 30,
            font_size_time: 10,
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
        let sizes: [(&str, Getter); 5] = [
            ("width", |s| s.width),
            ("height", |s| s.height),
            ("font_size_title", |s| s.font_size_title),
            ("font_size_subtitle", |s| s.font_size_subtitle),
            ("font_size_time", |s| s.font_size_time),
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
