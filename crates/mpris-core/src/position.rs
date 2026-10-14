//! Displayed position, derived from the last known anchor (B-17).

use std::time::{Duration, Instant};

use crate::model::{PlaybackStatus, Player};

/// Position to show at `now`. The anchor only moves while playing, scaled
/// by the playback rate, and the result never leaves `[0, length]`.
pub fn display_position(player: &Player, now: Instant) -> Duration {
    let anchor = player.anchor;
    let position = match player.status {
        PlaybackStatus::Playing => {
            let elapsed = now.saturating_duration_since(anchor.at).as_secs_f64();
            offset(anchor.position, elapsed * player.rate)
        }
        PlaybackStatus::Paused | PlaybackStatus::Stopped => anchor.position,
    };
    match player.track.length {
        Some(length) => position.min(length),
        None => position,
    }
}

/// `base + seconds`, floored at zero. A non-finite or unrepresentable
/// offset leaves `base` untouched: a misbehaving player must not panic the
/// render thread.
fn offset(base: Duration, seconds: f64) -> Duration {
    if !seconds.is_finite() {
        return base;
    }
    let total = base.as_secs_f64() + seconds;
    if total <= 0.0 {
        return Duration::ZERO;
    }
    Duration::try_from_secs_f64(total).unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Art, PlayerKey, PositionAnchor, Track};

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn player(status: PlaybackStatus, rate: f64, position: Duration, at: Instant) -> Player {
        Player {
            unique_name: ":1.1".into(),
            bus_name: "org.mpris.MediaPlayer2.test".into(),
            key: PlayerKey("test".into()),
            identity: "Test".into(),
            status,
            rate,
            track: Track::default(),
            anchor: PositionAnchor { position, at },
            art: Art::None,
            online: Vec::new(),
        }
    }

    #[test]
    fn playing_moves_forward_with_elapsed_time() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, 1.0, secs(10), t0);
        assert_eq!(display_position(&p, t0 + secs(5)), secs(15));
    }

    #[test]
    fn rate_scales_elapsed_time() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, 2.0, secs(10), t0);
        assert_eq!(display_position(&p, t0 + secs(5)), secs(20));
    }

    #[test]
    fn paused_stays_on_anchor() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Paused, 1.0, secs(10), t0);
        assert_eq!(display_position(&p, t0 + secs(60)), secs(10));
    }

    #[test]
    fn stopped_stays_on_anchor() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Stopped, 1.0, secs(3), t0);
        assert_eq!(display_position(&p, t0 + secs(60)), secs(3));
    }

    #[test]
    fn never_exceeds_track_length() {
        let t0 = Instant::now();
        let mut p = player(PlaybackStatus::Playing, 1.0, secs(170), t0);
        p.track.length = Some(secs(180));
        assert_eq!(display_position(&p, t0 + secs(60)), secs(180));
    }

    #[test]
    fn anchor_beyond_length_is_clamped_even_when_paused() {
        let t0 = Instant::now();
        let mut p = player(PlaybackStatus::Paused, 1.0, secs(200), t0);
        p.track.length = Some(secs(180));
        assert_eq!(display_position(&p, t0), secs(180));
    }

    #[test]
    fn unknown_length_is_unbounded() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, 1.0, secs(3600), t0);
        assert_eq!(display_position(&p, t0 + secs(7200)), secs(10800));
    }

    #[test]
    fn negative_rate_floors_at_zero() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, -1.0, secs(5), t0);
        assert_eq!(display_position(&p, t0 + secs(60)), Duration::ZERO);
    }

    #[test]
    fn now_before_anchor_does_not_move() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, 1.0, secs(10), t0 + secs(5));
        assert_eq!(display_position(&p, t0), secs(10));
    }

    #[test]
    fn non_finite_rate_keeps_anchor() {
        let t0 = Instant::now();
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let p = player(PlaybackStatus::Playing, rate, secs(10), t0);
            assert_eq!(display_position(&p, t0 + secs(1)), secs(10), "rate {rate}");
        }
    }

    #[test]
    fn huge_rate_does_not_panic() {
        let t0 = Instant::now();
        let p = player(PlaybackStatus::Playing, 1e300, secs(10), t0);
        let _ = display_position(&p, t0 + secs(1));
    }
}
