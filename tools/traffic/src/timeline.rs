//! Bounded endpoint timelines for laboratory recovery diagnostics only.
pub const BIN_US: u64 = 10_000;
pub fn probe_epoch(epoch_us: u64, sent_us: u64, observed_us: u64) -> Option<u64> {
    if sent_us > observed_us {
        return None;
    }
    epoch_us.checked_add(sent_us)
}
pub struct Bins {
    start_us: u64,
    pub values: Vec<u64>,
}
impl Bins {
    pub fn new(start_us: u64, seconds: u64) -> Self {
        Self {
            start_us,
            values: vec![0; seconds.min(30) as usize * 100],
        }
    }
    fn slot(&mut self, stamp_us: u64) -> Option<&mut u64> {
        let offset = stamp_us.checked_sub(self.start_us)?;
        self.values.get_mut((offset / BIN_US) as usize)
    }
    pub fn increment(&mut self, stamp_us: u64) {
        if let Some(value) = self.slot(stamp_us) {
            *value += 1;
        }
    }
    pub fn maximum(&mut self, stamp_us: u64, sample: u64) {
        if let Some(value) = self.slot(stamp_us) {
            *value = (*value).max(sample);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_bins_keep_boundary_and_retention_limits() {
        let mut bins = Bins::new(1000, 1800);
        for stamp in [999, 1000, 10999, 11000, 30_000_999, 30_001_000] {
            bins.increment(stamp);
        }
        assert_eq!(bins.values.len(), 3000);
        assert_eq!(&bins.values[..2], &[2, 1]);
        assert_eq!(bins.values[2999], 1);
        assert_eq!(bins.values.iter().sum::<u64>(), 4);
    }
    #[test]
    fn delayed_probe_peaks_stay_in_their_original_submission_bin() {
        let mut bins = Bins::new(0, 2);
        bins.maximum(5_000, 100_000);
        bins.maximum(5_000, 90_000);
        bins.maximum(15_000, 55_000);
        assert_eq!(&bins.values[..2], &[100_000, 55_000]);
    }
    #[test]
    fn untrusted_probe_stamp_cannot_overflow_or_invent_a_future_submission() {
        assert_eq!(probe_epoch(1000, 50, 100), Some(1050));
        assert_eq!(probe_epoch(1000, 101, 100), None);
        assert_eq!(probe_epoch(u64::MAX, 1, 100), None);
    }
}
