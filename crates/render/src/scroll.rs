//! Where each text too long for its line stands at a given instant (B-36).
//!
//! A cycle starts with the pause, every text at its start. Then the texts
//! move left together at the same speed, until the longest has travelled
//! its whole period, its width and the gap to its copy. A shorter text
//! reaches its period first and waits there, which draws exactly like its
//! start, so that the pause that follows holds every text still at once.
//!
//! Only arithmetic: it runs for every frame OBS draws.

use std::time::Duration;

/// The motion shared by the texts that scroll together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scroll {
    /// The period that sets the length of the motion.
    longest: u32,
    /// Pixels per second, 0 for no motion at all.
    speed: u32,
    pause: Duration,
}

impl Scroll {
    /// The motion of texts of `periods` pixels, at `speed` pixels per
    /// second, stopping `pause` at their start before each round.
    pub fn new(periods: impl IntoIterator<Item = u32>, speed: u32, pause: Duration) -> Scroll {
        Scroll {
            longest: periods.into_iter().max().unwrap_or(0),
            speed,
            pause,
        }
    }

    /// How many whole pixels a text of `period` has moved left of its start,
    /// `elapsed` after the cycle started. An offset of `period` draws like
    /// 0, so the loop never jumps.
    pub fn offset(&self, elapsed: Duration, period: u32) -> u32 {
        // Whole nanoseconds, so that hours of scrolling never drift the way
        // a float would.
        let speed = u128::from(self.speed);
        if speed == 0 {
            return 0;
        }
        let pause = self.pause.as_nanos();
        // Rounded up, so that the motion takes the longest text to its period.
        let motion = (u128::from(self.longest) * NANOS_PER_SECOND).div_ceil(speed);
        let cycle = pause + motion;
        if cycle == 0 {
            return 0;
        }
        let Some(moving) = (elapsed.as_nanos() % cycle).checked_sub(pause) else {
            return 0;
        };
        let travelled = moving * speed / NANOS_PER_SECOND;
        u32::try_from(travelled).unwrap_or(u32::MAX).min(period)
    }
}

const NANOS_PER_SECOND: u128 = 1_000_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// 100 px/s and a two second pause: a 300 px period moves for three
    /// seconds, so a cycle lasts five.
    fn scroll(periods: &[u32]) -> Scroll {
        Scroll::new(periods.iter().copied(), 100, ms(2000))
    }

    #[test]
    fn the_cycle_starts_with_the_pause() {
        let s = scroll(&[300]);
        for t in [0, 1, 1000, 1999] {
            assert_eq!(s.offset(ms(t), 300), 0, "{t} ms");
        }
    }

    #[test]
    fn the_text_moves_at_constant_speed_after_the_pause() {
        let s = scroll(&[300]);
        assert_eq!(s.offset(ms(2000), 300), 0);
        assert_eq!(s.offset(ms(2010), 300), 1);
        assert_eq!(s.offset(ms(2500), 300), 50);
        assert_eq!(s.offset(ms(3000), 300), 100);
        assert_eq!(s.offset(ms(4000), 300), 200);
    }

    #[test]
    fn offsets_are_whole_pixels_rounded_down() {
        let s = scroll(&[300]);
        assert_eq!(s.offset(ms(2019), 300), 1);
        assert_eq!(s.offset(ms(2020), 300), 2);
    }

    #[test]
    fn the_text_comes_back_to_its_start_and_pauses_again() {
        let s = scroll(&[300]);
        assert_eq!(s.offset(ms(4999), 300), 299);
        assert_eq!(
            s.offset(ms(5000), 300),
            0,
            "a period away draws like the start"
        );
        assert_eq!(s.offset(ms(6999), 300), 0);
        assert_eq!(s.offset(ms(7000), 300), 0);
        assert_eq!(s.offset(ms(7500), 300), 50);
    }

    #[test]
    fn the_longest_text_moves_by_one_pixel_at_most_across_the_loop() {
        let s = scroll(&[300]);
        let last = s.offset(ms(5000) - Duration::from_nanos(1), 300);
        assert_eq!(
            last, 299,
            "one pixel short of its period, which is its start"
        );
    }

    #[test]
    fn the_shorter_text_waits_at_its_start_for_the_longer() {
        let s = scroll(&[300, 100]);
        assert_eq!(s.offset(ms(2500), 100), 50);
        assert_eq!(s.offset(ms(2500), 300), 50, "together at the same speed");
        assert_eq!(s.offset(ms(3000), 100), 100, "back at its start");
        assert_eq!(s.offset(ms(4000), 100), 100, "and waiting there");
        assert_eq!(s.offset(ms(4000), 300), 200);
        assert_eq!(s.offset(ms(4999), 100), 100);
    }

    #[test]
    fn both_texts_share_the_pause() {
        let s = scroll(&[300, 100]);
        for t in [5000, 6000, 6999] {
            assert_eq!(s.offset(ms(t), 300), 0, "{t} ms");
            assert_eq!(s.offset(ms(t), 100), 0, "{t} ms");
        }
        assert_eq!(s.offset(ms(7500), 300), 50);
        assert_eq!(s.offset(ms(7500), 100), 50);
    }

    #[test]
    fn the_order_of_the_periods_does_not_matter() {
        assert_eq!(scroll(&[100, 300]), scroll(&[300, 100]));
    }

    #[test]
    fn the_cycle_repeats_without_drift() {
        let s = scroll(&[300, 100]);
        let cycle = ms(5000);
        for rounds in [1u32, 1000, 1_000_000, 1_000_000_000] {
            for t in [0, 2010, 2500, 4999] {
                let later = cycle * rounds + ms(t);
                for period in [300, 100] {
                    assert_eq!(
                        s.offset(later, period),
                        s.offset(ms(t), period),
                        "{rounds} rounds, {t} ms, {period} px"
                    );
                }
            }
        }
    }

    #[test]
    fn huge_elapsed_times_stay_within_the_period() {
        let s = scroll(&[300, 100]);
        for elapsed in [Duration::MAX, Duration::from_secs(u64::MAX / 3)] {
            assert!(s.offset(elapsed, 300) < 300, "{elapsed:?}");
            assert!(s.offset(elapsed, 100) <= 100, "{elapsed:?}");
        }
    }

    #[test]
    fn offsets_never_leave_their_period() {
        let s = Scroll::new([317, 41], 73, ms(250));
        for t in (0..20_000).step_by(7) {
            assert!(s.offset(ms(t), 317) < 317, "{t} ms");
            assert!(s.offset(ms(t), 41) <= 41, "{t} ms");
        }
    }

    #[test]
    fn a_speed_that_does_not_divide_the_period_still_reaches_its_end() {
        let s = Scroll::new([10, 3], 7, Duration::ZERO);
        // 10 px at 7 px/s take 1.428... s, rounded up to the nanosecond.
        let motion = Duration::from_nanos(1_428_571_429);
        assert_eq!(s.offset(motion - Duration::from_nanos(1), 10), 9);
        assert_eq!(s.offset(motion, 10), 0);
        assert_eq!(s.offset(Duration::from_secs(1), 3), 3);
    }

    #[test]
    fn zero_speed_never_moves() {
        let s = Scroll::new([300, 100], 0, ms(2000));
        for t in [0, 1999, 2000, 2500, 10_000, 1_000_000] {
            assert_eq!(s.offset(ms(t), 300), 0, "{t} ms");
            assert_eq!(s.offset(ms(t), 100), 0, "{t} ms");
        }
    }

    #[test]
    fn zero_pause_scrolls_without_stopping() {
        let s = Scroll::new([300], 100, Duration::ZERO);
        assert_eq!(s.offset(ms(0), 300), 0);
        assert_eq!(s.offset(ms(1000), 300), 100);
        assert_eq!(s.offset(ms(2990), 300), 299);
        assert_eq!(s.offset(ms(3000), 300), 0);
        assert_eq!(s.offset(ms(3500), 300), 50);
    }

    #[test]
    fn no_text_has_no_motion() {
        let s = Scroll::new([], 100, ms(2000));
        for t in [0, 2500, 10_000] {
            assert_eq!(s.offset(ms(t), 0), 0);
        }
    }

    #[test]
    fn no_text_and_no_pause_do_not_divide_by_zero() {
        for s in [
            Scroll::new([], 100, Duration::ZERO),
            Scroll::new([0], 100, Duration::ZERO),
            Scroll::new([0], 0, Duration::ZERO),
        ] {
            assert_eq!(s.offset(ms(1234), 0), 0);
        }
    }
}
