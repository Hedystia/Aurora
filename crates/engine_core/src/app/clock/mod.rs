//! Fixed 60 Hz simulation clock with bounded real-time catch-up.

use std::time::Duration;

const HZ: u64 = 60;
const NANOS: u64 = 1_000_000_000;
const MAX_ELAPSED: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct Clock {
    remainder: u64,
}

impl Clock {
    pub(super) fn advance(&mut self, elapsed: Duration) -> u64 {
        let total = self.remainder + elapsed.min(MAX_ELAPSED).as_nanos() as u64 * HZ;
        self.remainder = total % NANOS;
        total / NANOS
    }

    pub(super) fn until_tick(&self) -> Duration {
        Duration::from_nanos((NANOS - self.remainder).div_ceil(HZ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irregular_frames_accumulate_exact_ticks() {
        let mut clock = Clock::default();
        let ticks: u64 = (0..50)
            .flat_map(|_| [Duration::from_millis(3), Duration::from_millis(17)])
            .map(|elapsed| clock.advance(elapsed))
            .sum();
        assert_eq!(ticks, 60);
        assert_eq!(clock.until_tick(), Duration::from_nanos(16_666_667));
        assert_eq!(clock.advance(Duration::from_secs(5)), 15);
    }
}
