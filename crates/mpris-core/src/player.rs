//! Stable player key (B-04) and tolerant readers for the properties of the
//! MPRIS `Player` and root interfaces.
//!
//! Every reader returns `None` on a value it cannot trust, so the caller
//! keeps what it had: a misbehaving player must never crash OBS (B-42).

use std::time::Duration;

use zbus::zvariant::Value;

use crate::model::{PlaybackStatus, PlayerKey};

/// Well-known name prefix every MPRIS player owns on the session bus.
pub const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// Key of the player owning `bus_name`, or `None` if the name is not an
/// MPRIS player name. Only the first segment counts: the spec reserves the
/// next ones for instances, which change on every launch (B-04).
pub fn player_key(bus_name: &str) -> Option<PlayerKey> {
    let rest = bus_name.strip_prefix(MPRIS_PREFIX)?;
    let key = rest.split('.').next()?;
    (!key.is_empty()).then(|| PlayerKey(key.to_owned()))
}

/// `PlaybackStatus`, limited to the three values of the spec.
pub fn read_status(value: &Value<'_>) -> Option<PlaybackStatus> {
    match read_str(value)? {
        "Playing" => Some(PlaybackStatus::Playing),
        "Paused" => Some(PlaybackStatus::Paused),
        "Stopped" => Some(PlaybackStatus::Stopped),
        _ => None,
    }
}

/// `Rate`. A non-finite rate would poison the position extrapolation.
pub fn read_rate(value: &Value<'_>) -> Option<f64> {
    match *unwrap_variant(value) {
        Value::F64(rate) if rate.is_finite() => Some(rate),
        _ => None,
    }
}

/// `Position`, floored at zero since a track has no time before its start.
pub fn read_position(value: &Value<'_>) -> Option<Duration> {
    let micros = read_micros(value)?;
    Some(u64::try_from(micros).map_or(Duration::ZERO, Duration::from_micros))
}

/// `Identity`. A blank name is refused because the selector needs a
/// visible label (B-02).
pub fn read_identity(value: &Value<'_>) -> Option<String> {
    read_str(value)
        .filter(|identity| !identity.trim().is_empty())
        .map(str::to_owned)
}

/// The value inside any number of variant wrappers, since some players
/// wrap property values twice.
pub(crate) fn unwrap_variant<'r, 'v>(value: &'r Value<'v>) -> &'r Value<'v> {
    let mut value = value;
    while let Value::Value(inner) = value {
        value = inner;
    }
    value
}

/// A string, wrapped in variants or not.
pub(crate) fn read_str<'r>(value: &'r Value<'_>) -> Option<&'r str> {
    match unwrap_variant(value) {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    }
}

/// A count of microseconds. The spec says `x`, players also send `t`, `i`
/// or `u`. `i128` holds every one of them without loss.
pub(crate) fn read_micros(value: &Value<'_>) -> Option<i128> {
    match *unwrap_variant(value) {
        Value::I64(us) => Some(us.into()),
        Value::U64(us) => Some(us.into()),
        Value::I32(us) => Some(us.into()),
        Value::U32(us) => Some(us.into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::ObjectPath;

    fn key(s: &str) -> Option<PlayerKey> {
        Some(PlayerKey(s.into()))
    }

    fn nested(value: Value<'static>) -> Value<'static> {
        Value::Value(Box::new(value))
    }

    fn micros(us: u64) -> Option<Duration> {
        Some(Duration::from_micros(us))
    }

    #[test]
    fn key_is_the_segment_after_the_prefix() {
        assert_eq!(player_key("org.mpris.MediaPlayer2.mpv"), key("mpv"));
        assert_eq!(player_key("org.mpris.MediaPlayer2.spotify"), key("spotify"));
    }

    #[test]
    fn key_ignores_instance_segments() {
        let cases = [
            ("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA", "mpv"),
            ("org.mpris.MediaPlayer2.firefox.instance_1_42", "firefox"),
            ("org.mpris.MediaPlayer2.chromium.instance12345", "chromium"),
            ("org.mpris.MediaPlayer2.vlc.a.b.c", "vlc"),
        ];
        for (bus_name, expected) in cases {
            assert_eq!(player_key(bus_name), key(expected), "{bus_name}");
        }
    }

    #[test]
    fn key_survives_a_restart_with_a_new_instance() {
        let before = player_key("org.mpris.MediaPlayer2.mpv.instance-mWyyJbmA");
        let after = player_key("org.mpris.MediaPlayer2.mpv.instance-Qx81bZcd");
        assert_eq!(before, after);
    }

    #[test]
    fn key_keeps_case() {
        assert_eq!(player_key("org.mpris.MediaPlayer2.VLC"), key("VLC"));
        assert_ne!(player_key("org.mpris.MediaPlayer2.VLC"), key("vlc"));
    }

    #[test]
    fn key_rejects_names_outside_mpris() {
        let cases = [
            "",
            "org.freedesktop.DBus",
            ":1.42",
            "org.mpris.MediaPlayer2",
            "org.mpris.MediaPlayer2Extra.mpv",
            "org.MPRIS.MediaPlayer2.mpv",
            "com.example.org.mpris.MediaPlayer2.mpv",
        ];
        for bus_name in cases {
            assert_eq!(player_key(bus_name), None, "{bus_name}");
        }
    }

    #[test]
    fn key_rejects_an_empty_first_segment() {
        assert_eq!(player_key("org.mpris.MediaPlayer2."), None);
        assert_eq!(player_key("org.mpris.MediaPlayer2..x"), None);
    }

    #[test]
    fn status_reads_the_three_spec_values() {
        let cases = [
            ("Playing", PlaybackStatus::Playing),
            ("Paused", PlaybackStatus::Paused),
            ("Stopped", PlaybackStatus::Stopped),
        ];
        for (text, expected) in cases {
            assert_eq!(read_status(&Value::from(text)), Some(expected), "{text}");
        }
    }

    #[test]
    fn status_unwraps_nested_variants() {
        assert_eq!(
            read_status(&nested(Value::from("Playing"))),
            Some(PlaybackStatus::Playing)
        );
        assert_eq!(
            read_status(&nested(nested(Value::from("Paused")))),
            Some(PlaybackStatus::Paused)
        );
    }

    #[test]
    fn status_rejects_values_outside_the_spec() {
        for text in [
            "",
            "playing",
            "PLAYING",
            " Playing",
            "Playing ",
            "Buffering",
        ] {
            assert_eq!(read_status(&Value::from(text)), None, "{text:?}");
        }
    }

    #[test]
    fn status_rejects_wrong_types() {
        let cases = [
            Value::from(1i32),
            Value::from(true),
            Value::from(ObjectPath::from_static_str_unchecked("/Playing")),
            Value::from(vec!["Playing"]),
        ];
        for value in cases {
            assert_eq!(read_status(&value), None, "{value:?}");
        }
    }

    #[test]
    fn rate_reads_a_double() {
        assert_eq!(read_rate(&Value::from(1.0f64)), Some(1.0));
        assert_eq!(read_rate(&Value::from(0.5f64)), Some(0.5));
        assert_eq!(read_rate(&nested(Value::from(2.0f64))), Some(2.0));
    }

    #[test]
    fn rate_keeps_negative_values() {
        // MPRIS allows a negative MinimumRate, and the position
        // extrapolation already floors the result at zero.
        assert_eq!(read_rate(&Value::from(-1.0f64)), Some(-1.0));
    }

    #[test]
    fn rate_rejects_non_finite_values() {
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(read_rate(&Value::from(rate)), None, "{rate}");
        }
    }

    #[test]
    fn rate_rejects_wrong_types() {
        let cases = [Value::from(1i64), Value::from(1u32), Value::from("1.0")];
        for value in cases {
            assert_eq!(read_rate(&value), None, "{value:?}");
        }
    }

    #[test]
    fn position_reads_microseconds() {
        assert_eq!(
            read_position(&Value::from(83_500_000i64)),
            micros(83_500_000)
        );
        assert_eq!(read_position(&Value::from(0i64)), Some(Duration::ZERO));
    }

    #[test]
    fn position_tolerates_other_integer_types() {
        assert_eq!(
            read_position(&Value::from(42_000_000u64)),
            micros(42_000_000)
        );
        assert_eq!(
            read_position(&Value::from(42_000_000i32)),
            micros(42_000_000)
        );
        assert_eq!(
            read_position(&Value::from(42_000_000u32)),
            micros(42_000_000)
        );
    }

    #[test]
    fn position_unwraps_nested_variants() {
        assert_eq!(
            read_position(&nested(Value::from(1_000_000i64))),
            micros(1_000_000)
        );
        assert_eq!(
            read_position(&nested(nested(Value::from(1_000_000i64)))),
            micros(1_000_000)
        );
    }

    #[test]
    fn negative_position_is_zero() {
        assert_eq!(read_position(&Value::from(-1i64)), Some(Duration::ZERO));
        assert_eq!(read_position(&Value::from(i64::MIN)), Some(Duration::ZERO));
        assert_eq!(read_position(&Value::from(-5i32)), Some(Duration::ZERO));
    }

    #[test]
    fn position_extremes_do_not_panic() {
        assert_eq!(
            read_position(&Value::from(i64::MAX)),
            micros(i64::MAX as u64)
        );
        assert_eq!(read_position(&Value::from(u64::MAX)), micros(u64::MAX));
    }

    #[test]
    fn position_rejects_wrong_types() {
        let cases = [Value::from(1.5f64), Value::from("1000"), Value::from(true)];
        for value in cases {
            assert_eq!(read_position(&value), None, "{value:?}");
        }
    }

    #[test]
    fn identity_reads_the_player_name() {
        assert_eq!(read_identity(&Value::from("mpv")), Some("mpv".into()));
        assert_eq!(
            read_identity(&nested(Value::from("Google Chrome"))),
            Some("Google Chrome".into())
        );
    }

    #[test]
    fn identity_rejects_blank_names() {
        for text in ["", " ", "\t\n "] {
            assert_eq!(read_identity(&Value::from(text)), None, "{text:?}");
        }
    }

    #[test]
    fn identity_rejects_wrong_types() {
        let cases = [
            Value::from(7i32),
            Value::from(ObjectPath::from_static_str_unchecked("/mpv")),
            Value::from(vec!["mpv"]),
        ];
        for value in cases {
            assert_eq!(read_identity(&value), None, "{value:?}");
        }
    }
}
