use serde::{Deserialize, Serialize};
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub topology: String,
    pub io: String,
    pub size: usize,
    pub sessions: usize,
    pub interval_seconds: u64,
    pub drain_ms: u64,
    pub diagnostics: String,
}
impl Profile {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.topology != "two-leg"
            || !["single", "mmsg"].contains(&self.io.as_str())
            || !(40..=1200).contains(&self.size)
            || ![1, 32].contains(&self.sessions)
            || self.interval_seconds == 0
            || self.drain_ms == 0
            || self.diagnostics != "info"
        {
            return Err("invalid or incomplete measurement coverage");
        }
        Ok(())
    }
}
pub fn matching(a: &Profile, b: &Profile) -> bool {
    a == b && a.validate().is_ok()
}
pub fn percentile(samples: &mut [u64], percent: usize) -> Option<u64> {
    if samples.is_empty() || percent == 0 || percent > 100 {
        return None;
    }
    samples.sort_unstable();
    Some(samples[(samples.len() * percent).div_ceil(100) - 1])
}
#[derive(Deserialize)]
pub struct Regression {
    pub frozen: bool,
    pub reference: String,
    pub workload: String,
    pub throughput_min: f64,
    pub loss_max_pp: f64,
    pub rtt_p95_max_ms: f64,
    pub cpu_max: f64,
    pub rss_max_bytes: u64,
}
impl Regression {
    pub fn validate(&self, reference: &str, workload: &str) -> Result<(), &'static str> {
        if !self.frozen
            || self.reference != reference
            || self.workload != workload
            || !self.throughput_min.is_finite()
            || self.throughput_min <= 0.0
            || !self.loss_max_pp.is_finite()
            || self.loss_max_pp < 0.0
            || !self.rtt_p95_max_ms.is_finite()
            || self.rtt_p95_max_ms < 0.0
            || !self.cpu_max.is_finite()
            || self.cpu_max < 0.0
            || self.rss_max_bytes == 0
        {
            return Err("unfrozen or unmatched regression reference");
        }
        Ok(())
    }
}
