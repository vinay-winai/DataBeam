use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// Time-weighted throughput over the trailing second. Seed with bytes already
/// present to exclude resumed data; publish fresh estimates about every 100 ms.
#[derive(Debug, Clone)]
pub struct LiveRate {
    bytes: u64,
    at: Instant,
    rate: f64,
    samples: VecDeque<(Instant, u64)>,
}

impl LiveRate {
    pub fn new(bytes: u64, at: Instant) -> Self {
        Self {
            bytes,
            at,
            rate: 0.0,
            samples: VecDeque::from([(at, bytes)]),
        }
    }

    pub fn observe(&mut self, bytes: u64, now: Instant) -> f64 {
        if bytes < self.bytes || now < self.at {
            *self = Self::new(bytes, now);
            return 0.0;
        }
        let elapsed = now.saturating_duration_since(self.at);
        if elapsed >= Duration::from_secs(1) && bytes == self.bytes {
            // Start a fresh measurement after an idle gap. Old throughput and
            // waiting time must not dominate the next active window.
            self.samples.clear();
            self.samples.push_back((now, self.bytes));
            self.bytes = bytes;
            self.at = now;
            self.rate = 0.0;
            return 0.0;
        }
        if elapsed >= Duration::from_millis(100) {
            self.samples.push_back((now, bytes));
            let cutoff = now.checked_sub(Duration::from_secs(1)).unwrap_or(now);
            while self.samples.len() > 1 && self.samples[1].0 <= cutoff {
                self.samples.pop_front();
            }
            let (first_at, first_bytes) = self.samples[0];
            let start = first_at.max(cutoff);
            let mut delta = bytes.saturating_sub(first_bytes) as f64;
            if start > first_at {
                // Retain the part of the oldest interval inside the window.
                // Interpolation avoids weighting irregular updates equally.
                if let Some(&(next_at, next_bytes)) = self.samples.get(1) {
                    let fraction = start.duration_since(first_at).as_secs_f64()
                        / next_at.duration_since(first_at).as_secs_f64();
                    delta -= next_bytes.saturating_sub(first_bytes) as f64 * fraction;
                }
            }
            self.rate = if now > start {
                delta.max(0.0) / now.duration_since(start).as_secs_f64()
            } else {
                0.0
            };
            self.bytes = bytes;
            self.at = now;
        }
        self.current(now)
    }

    pub fn current(&self, now: Instant) -> f64 {
        if now.saturating_duration_since(self.at) >= Duration::from_secs(1) {
            0.0
        } else {
            self.rate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooths_bursts_over_one_second_without_counting_cached_bytes() {
        let now = Instant::now();
        let mut rate = LiveRate::new(1_000_000, now);
        assert_eq!(rate.current(now), 0.0);
        assert_eq!(
            rate.observe(1_010_000, now + Duration::from_millis(100)),
            100_000.0
        );
        assert_eq!(
            rate.observe(1_011_000, now + Duration::from_millis(200)),
            55_000.0
        );
        rate.observe(1_011_000, now + Duration::from_millis(300));
        for tick in 4..=13 {
            rate.observe(
                1_011_000 + (tick - 3) * 1000,
                now + Duration::from_millis(tick * 100),
            );
        }
        assert_eq!(rate.current(now + Duration::from_millis(1300)), 10_000.0);
        assert_eq!(rate.current(now + Duration::from_secs(3)), 0.0);
        assert_eq!(rate.observe(0, now + Duration::from_secs(4)), 0.0);
        assert_eq!(
            rate.observe(1_000, now + Duration::from_millis(4100)),
            10_000.0
        );
    }

    #[test]
    fn clips_irregular_intervals_at_the_one_second_boundary() {
        let now = Instant::now();
        let mut rate = LiveRate::new(0, now);
        for (millis, bytes) in [(100, 10_000), (350, 35_000), (1050, 105_000)] {
            assert_eq!(
                rate.observe(bytes, now + Duration::from_millis(millis)),
                100_000.0
            );
        }
        // The initial 100 ms interval crosses the cutoff at 50 ms.
        assert_eq!(rate.samples.len(), 4);
    }

    #[test]
    fn slow_updates_still_report_throughput() {
        let now = Instant::now();
        let mut rate = LiveRate::new(0, now);
        assert_eq!(rate.observe(2_000, now + Duration::from_secs(2)), 1_000.0);
        assert_eq!(rate.observe(4_000, now + Duration::from_secs(4)), 1_000.0);
    }

    #[test]
    fn discards_old_rate_after_a_stall_and_bounds_history() {
        let now = Instant::now();
        let mut rate = LiveRate::new(0, now);
        for tick in 1..=1000 {
            rate.observe(tick * 1000, now + Duration::from_millis(tick * 100));
            assert!(rate.samples.len() <= 12);
        }
        assert_eq!(rate.current(now + Duration::from_secs(102)), 0.0);
        assert_eq!(rate.observe(1_000_000, now + Duration::from_secs(102)), 0.0);
        assert_eq!(
            rate.observe(1_000_100, now + Duration::from_millis(102100)),
            1_000.0
        );
    }
}
