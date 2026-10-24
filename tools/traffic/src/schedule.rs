//! Test-only piecewise constant offered-load schedule; no hidden warmup.
use std::time::Duration;

pub struct Schedule {
    pub rates: Vec<u64>,
    ends: Vec<u64>,
}
impl Schedule {
    pub fn steps(seconds: u64, steps: &[u64], max_rate: u64) -> Result<Self, &'static str> {
        if seconds == 0
            || seconds > 1800
            || steps.len() != 4
            || !seconds.is_multiple_of(4)
            || steps.iter().any(|v| *v == 0 || *v > max_rate)
        {
            return Err("invalid bounded rate steps");
        }
        let mut rates = Vec::new();
        for &rate in steps {
            rates.extend(std::iter::repeat_n(rate, (seconds / 4) as usize));
        }
        let mut total = 0u64;
        let mut ends = Vec::new();
        for &rate in &rates {
            total = total.checked_add(rate).ok_or("rate schedule overflow")?;
            ends.push(total);
        }
        Ok(Self { rates, ends })
    }
    pub fn total(&self) -> u64 {
        self.ends.last().copied().unwrap_or(0)
    }
    pub fn offset(&self, tick: u64, quantum_us: u64) -> Duration {
        let second = self.ends.partition_point(|end| *end <= tick);
        if second == self.ends.len() {
            return Duration::from_secs(second as u64);
        }
        let previous = if second == 0 {
            0
        } else {
            self.ends[second - 1]
        };
        let nanos = (tick - previous) * 1_000_000_000 / self.rates[second];
        let nanos = if quantum_us == 0 {
            nanos
        } else {
            nanos / (quantum_us * 1000) * (quantum_us * 1000)
        };
        Duration::from_secs(second as u64) + Duration::from_nanos(nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn steps_keep_every_offered_slot_and_exact_phase_boundaries() {
        let schedule = Schedule::steps(20, &[2, 5, 10, 20], 20).unwrap();
        assert_eq!(schedule.total(), 185);
        for (tick, second) in [(0, 0), (10, 5), (35, 10), (85, 15), (185, 20)] {
            assert_eq!(schedule.offset(tick, 1000), Duration::from_secs(second));
        }
        let offsets: Vec<_> = (0..schedule.total())
            .map(|n| schedule.offset(n, 1000))
            .collect();
        assert!(offsets.windows(2).all(|w| w[0] <= w[1]));
        for (second, expected) in schedule.rates.iter().enumerate() {
            assert_eq!(
                offsets
                    .iter()
                    .filter(|v| v.as_secs() == second as u64)
                    .count() as u64,
                *expected
            );
        }
    }
    #[test]
    fn malformed_or_unbounded_steps_fail_before_traffic() {
        for (seconds, steps) in [
            (0, vec![1, 2, 3, 4]),
            (21, vec![1, 2, 3, 4]),
            (1804, vec![1, 2, 3, 4]),
            (20, vec![1, 2]),
            (20, vec![0, 2, 3, 4]),
            (20, vec![1, 2, 3, 21]),
        ] {
            assert!(Schedule::steps(seconds, &steps, 20).is_err());
        }
    }
}
