//! Time formatting for display.

use std::time::Duration;

/// `m:ss` under one hour, `h:mm:ss` from one hour (B-16). The sub-second
/// part is truncated, never rounded: the display ticks once per second and
/// a rounded value would show the next second too early.
pub fn format_time(d: Duration) -> String {
    let total = d.as_secs();
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours == 0 {
        format!("{minutes}:{seconds:02}")
    } else {
        format!("{hours}:{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    fn hms(h: u64, m: u64, s: u64) -> Duration {
        secs(h * 3600 + m * 60 + s)
    }

    #[test]
    fn zero() {
        assert_eq!(format_time(Duration::ZERO), "0:00");
    }

    #[test]
    fn seconds_are_zero_padded() {
        assert_eq!(format_time(secs(5)), "0:05");
        assert_eq!(format_time(secs(65)), "1:05");
    }

    #[test]
    fn minutes_are_not_padded_under_one_hour() {
        assert_eq!(format_time(hms(0, 3, 45)), "3:45");
        assert_eq!(format_time(hms(0, 10, 0)), "10:00");
    }

    #[test]
    fn last_second_before_one_hour() {
        assert_eq!(format_time(hms(0, 59, 59)), "59:59");
    }

    #[test]
    fn one_hour_switches_to_hours() {
        assert_eq!(format_time(hms(1, 0, 0)), "1:00:00");
    }

    #[test]
    fn minutes_and_seconds_are_padded_with_hours() {
        assert_eq!(format_time(hms(1, 0, 5)), "1:00:05");
        assert_eq!(format_time(hms(1, 1, 0)), "1:01:00");
        assert_eq!(format_time(hms(10, 2, 3)), "10:02:03");
    }

    #[test]
    fn hours_are_not_wrapped() {
        assert_eq!(format_time(hms(100, 0, 0)), "100:00:00");
    }

    #[test]
    fn sub_second_part_is_truncated() {
        assert_eq!(format_time(Duration::from_millis(999)), "0:00");
        assert_eq!(format_time(Duration::from_millis(59_999)), "0:59");
    }

    #[test]
    fn truncation_does_not_cross_the_hour() {
        let almost_an_hour = hms(0, 59, 59) + Duration::from_nanos(999_999_999);
        assert_eq!(format_time(almost_an_hour), "59:59");
    }

    #[test]
    fn maximum_duration_does_not_panic() {
        assert_eq!(format_time(Duration::MAX), "5124095576030431:00:15");
    }
}
