//! Tolerant reading of the MPRIS `Metadata` dictionary into a `Track`.
//!
//! Players disagree with the spec on types, so each key accepts the
//! variants seen in practice. Anything else leaves the field empty rather
//! than failing the whole track (B-42).

use std::collections::HashMap;
use std::time::Duration;

use zbus::zvariant::{OwnedValue, Value};

use crate::model::Track;
use crate::player::{read_micros, read_str, unwrap_variant};

/// Track id the spec reserves for "no current track".
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// `Track` described by the `Metadata` property (`a{sv}`).
pub fn parse_track(metadata: &HashMap<String, OwnedValue>) -> Track {
    let get = |key: &str| metadata.get(key).map(|value| &**value);
    Track {
        id: get("mpris:trackid").and_then(read_track_id),
        title: get("xesam:title").and_then(read_text),
        artists: get("xesam:artist").map(read_text_list).unwrap_or_default(),
        album_artists: get("xesam:albumArtist")
            .map(read_text_list)
            .unwrap_or_default(),
        album: get("xesam:album").and_then(read_text),
        length: get("mpris:length").and_then(read_length),
        art_url: get("mpris:artUrl").and_then(read_text),
    }
}

/// `mpris:trackid`. The spec says `o`, some players send `s`.
fn read_track_id(value: &Value<'_>) -> Option<String> {
    let id = match unwrap_variant(value) {
        Value::ObjectPath(path) => path.as_str(),
        other => read_str(other)?,
    };
    (!id.is_empty() && id != NO_TRACK).then(|| id.to_owned())
}

/// A non-empty string, kept as sent: `mpris:artUrl` may be a `data:` URI
/// that any change would corrupt.
fn read_text(value: &Value<'_>) -> Option<String> {
    read_str(value)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

/// An `as` list. A lone `s` is accepted as a list of one, since some
/// players send it that way.
fn read_text_list(value: &Value<'_>) -> Vec<String> {
    match unwrap_variant(value) {
        Value::Array(items) => items.iter().filter_map(read_text).collect(),
        single => read_text(single).into_iter().collect(),
    }
}

/// `mpris:length`, in microseconds. Some players send a double. Zero or
/// less means unknown, as for a stream (B-16).
fn read_length(value: &Value<'_>) -> Option<Duration> {
    let length = match *unwrap_variant(value) {
        Value::F64(us) => Duration::try_from_secs_f64(us / 1e6).ok()?,
        _ => Duration::from_micros(u64::try_from(read_micros(value)?).ok()?),
    };
    (!length.is_zero()).then_some(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::ObjectPath;

    const ALBUM: &str = "愛をとりもどせ!! (MOVIE ver.)";
    const ART_URL: &str = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgH";
    const TRACK_ID: &str = "/mpv/mpris/Track/35";

    fn metadata(entries: Vec<(&str, Value<'static>)>) -> HashMap<String, OwnedValue> {
        entries
            .into_iter()
            .map(|(key, value)| {
                let owned = OwnedValue::try_from(value).expect("test values hold no fd");
                (key.to_owned(), owned)
            })
            .collect()
    }

    fn parse(entries: Vec<(&str, Value<'static>)>) -> Track {
        parse_track(&metadata(entries))
    }

    fn nested(value: Value<'static>) -> Value<'static> {
        Value::Value(Box::new(value))
    }

    fn path(p: &'static str) -> Value<'static> {
        Value::from(ObjectPath::from_static_str_unchecked(p))
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Metadata as mpv sends it, every key with its spec type.
    fn mpv_entries() -> Vec<(&'static str, Value<'static>)> {
        vec![
            ("mpris:trackid", path(TRACK_ID)),
            ("xesam:title", Value::from("愛をとりもどせ!!")),
            ("xesam:artist", Value::from(vec!["CRYSTAL KING"])),
            ("xesam:albumArtist", Value::from(vec!["CRYSTAL KING"])),
            ("xesam:album", Value::from(ALBUM)),
            ("mpris:length", Value::from(219_533_000i64)),
            ("mpris:artUrl", Value::from(ART_URL)),
        ]
    }

    fn mpv_track() -> Track {
        Track {
            id: Some(TRACK_ID.into()),
            title: Some("愛をとりもどせ!!".into()),
            artists: strings(&["CRYSTAL KING"]),
            album_artists: strings(&["CRYSTAL KING"]),
            album: Some(ALBUM.into()),
            length: Some(Duration::from_micros(219_533_000)),
            art_url: Some(ART_URL.into()),
        }
    }

    #[test]
    fn reads_every_key_of_real_mpv_metadata() {
        assert_eq!(parse(mpv_entries()), mpv_track());
    }

    #[test]
    fn empty_metadata_gives_an_empty_track() {
        assert_eq!(parse(vec![]), Track::default());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let track = parse(vec![
            ("xesam:genre", Value::from(vec!["Anime"])),
            ("mpv:custom", Value::from(3i32)),
            ("xesam:Title", Value::from("wrong case")),
        ]);
        assert_eq!(track, Track::default());
    }

    #[test]
    fn unwraps_nested_variants_on_every_key() {
        let once = mpv_entries()
            .into_iter()
            .map(|(k, v)| (k, nested(v)))
            .collect();
        assert_eq!(parse(once), mpv_track());
        let twice = mpv_entries()
            .into_iter()
            .map(|(k, v)| (k, nested(nested(v))))
            .collect();
        assert_eq!(parse(twice), mpv_track());
    }

    #[test]
    fn wrong_types_leave_every_field_empty() {
        let track = parse(vec![
            ("mpris:trackid", Value::from(35i32)),
            ("xesam:title", Value::from(1i64)),
            ("xesam:artist", Value::from(true)),
            ("xesam:albumArtist", Value::from(2.0f64)),
            ("xesam:album", Value::from(vec!["album"])),
            ("mpris:length", Value::from("219533000")),
            ("mpris:artUrl", path("/art")),
        ]);
        assert_eq!(track, Track::default());
    }

    #[test]
    fn track_id_accepts_a_string() {
        let track = parse(vec![("mpris:trackid", Value::from(TRACK_ID))]);
        assert_eq!(track.id, Some(TRACK_ID.into()));
    }

    #[test]
    fn no_track_path_means_no_track() {
        let no_track = "/org/mpris/MediaPlayer2/TrackList/NoTrack";
        assert_eq!(parse(vec![("mpris:trackid", path(no_track))]).id, None);
        assert_eq!(
            parse(vec![("mpris:trackid", Value::from(no_track))]).id,
            None
        );
    }

    #[test]
    fn track_id_rejects_an_empty_string() {
        assert_eq!(parse(vec![("mpris:trackid", Value::from(""))]).id, None);
    }

    #[test]
    fn empty_strings_are_absent() {
        let track = parse(vec![
            ("xesam:title", Value::from("")),
            ("xesam:album", Value::from("")),
            ("mpris:artUrl", Value::from("")),
        ]);
        assert_eq!(track, Track::default());
    }

    #[test]
    fn art_url_is_kept_untouched() {
        let long = format!("data:image/jpeg;base64,{}==", "A".repeat(500_000));
        let file = "file:///tmp/cover%20art%E3%81%82.jpg";
        for url in [long.as_str(), file] {
            let track = parse(vec![("mpris:artUrl", Value::from(url.to_owned()))]);
            assert_eq!(track.art_url.as_deref(), Some(url));
        }
    }

    #[test]
    fn artist_lists_keep_order() {
        let track = parse(vec![
            (
                "xesam:artist",
                Value::from(vec!["CRYSTAL KING", "Tetsuya Kakihara"]),
            ),
            (
                "xesam:albumArtist",
                Value::from(vec!["Various", "CRYSTAL KING"]),
            ),
        ]);
        assert_eq!(
            track.artists,
            strings(&["CRYSTAL KING", "Tetsuya Kakihara"])
        );
        assert_eq!(track.album_artists, strings(&["Various", "CRYSTAL KING"]));
    }

    #[test]
    fn artist_lists_tolerate_a_single_string() {
        let track = parse(vec![
            ("xesam:artist", Value::from("CRYSTAL KING")),
            ("xesam:albumArtist", nested(Value::from("CRYSTAL KING"))),
        ]);
        assert_eq!(track.artists, strings(&["CRYSTAL KING"]));
        assert_eq!(track.album_artists, strings(&["CRYSTAL KING"]));
    }

    #[test]
    fn artist_lists_tolerate_an_array_of_variants() {
        let artists = Value::from(vec![Value::from("CRYSTAL KING")]);
        assert_eq!(artists.value_signature().to_string(), "av");
        let track = parse(vec![("xesam:artist", artists)]);
        assert_eq!(track.artists, strings(&["CRYSTAL KING"]));
    }

    #[test]
    fn artist_lists_drop_empty_entries() {
        let track = parse(vec![
            ("xesam:artist", Value::from(vec!["", "CRYSTAL KING", ""])),
            ("xesam:albumArtist", Value::from("")),
        ]);
        assert_eq!(track.artists, strings(&["CRYSTAL KING"]));
        assert!(track.album_artists.is_empty());
    }

    #[test]
    fn artist_lists_drop_entries_of_the_wrong_type() {
        let empty: Vec<&str> = vec![];
        let track = parse(vec![
            ("xesam:artist", Value::from(vec![1i32, 2])),
            ("xesam:albumArtist", Value::from(empty)),
        ]);
        assert!(track.artists.is_empty());
        assert!(track.album_artists.is_empty());
    }

    #[test]
    fn length_tolerates_other_numeric_types() {
        let expected = Some(Duration::from_secs(215));
        let cases = [
            Value::from(215_000_000u64),
            Value::from(215_000_000i32),
            Value::from(215_000_000u32),
            Value::from(215_000_000.0f64),
        ];
        for value in cases {
            let label = format!("{value:?}");
            assert_eq!(
                parse(vec![("mpris:length", value)]).length,
                expected,
                "{label}"
            );
        }
    }

    #[test]
    fn unknown_length_is_absent() {
        let cases = [
            Value::from(0i64),
            Value::from(-1i64),
            Value::from(i64::MIN),
            Value::from(0u64),
            Value::from(-5i32),
            Value::from(0.0f64),
            Value::from(-0.0f64),
            Value::from(-215_000_000.0f64),
            Value::from(f64::NAN),
            Value::from(f64::INFINITY),
            Value::from(f64::NEG_INFINITY),
            Value::from(1e-300f64),
        ];
        for value in cases {
            let label = format!("{value:?}");
            assert_eq!(parse(vec![("mpris:length", value)]).length, None, "{label}");
        }
    }

    #[test]
    fn length_extremes_do_not_panic() {
        let huge = parse(vec![("mpris:length", Value::from(u64::MAX))]);
        assert_eq!(huge.length, Some(Duration::from_micros(u64::MAX)));
        let overflow = parse(vec![("mpris:length", Value::from(1e300f64))]);
        assert_eq!(overflow.length, None);
    }
}
