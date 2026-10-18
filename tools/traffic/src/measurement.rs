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

/// Release validation is separate from the early MVP comparator. Every row is
/// validated before any numerical threshold; no average can conceal a bad run.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub average_cpu_cores: f64,
    pub owner_average_cpu_cores: f64,
    pub peak_rss_bytes: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub offered: [u64; 2],
    pub submitted: [u64; 2],
    pub unique: [u64; 2],
    /// Unique delivery by source Session, in the same order as RTT samples.
    /// The aggregate must equal `unique`; resource qualification also requires
    /// every Session to carry traffic in each direction.
    pub unique_per_session: Vec<[u64; 2]>,
    pub generator_errors: u64,
    pub integrity_errors: u64,
    pub generator_cpu_max: f64,
    pub clock_drift_ms: f64,
    pub same_clock_rtt: bool,
    pub diagnostics: String,
    pub coverage_lost: u64,
    pub counter_overflow: bool,
    pub sample_count: usize,
    pub rtt_samples_per_session: Vec<usize>,
    pub rtt_p95_ms: f64,
    pub resources: [Resource; 2],
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PairedRun {
    pub pair: usize,
    pub phase: String,
    pub sessions: usize,
    pub payload_bytes: usize,
    pub direction: String,
    pub baseline_first: bool,
    pub warmup_seconds: u64,
    pub measurement_seconds: u64,
    pub offered_pps_per_direction: [u64; 2],
    pub sustainable_pps_per_direction: [u64; 2],
    pub baseline: Run,
    pub tunnel: Run,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchStep {
    pub payload_mbit_s: f64,
    pub loss_percent: [f64; 2],
    pub generator_valid: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub sessions: usize,
    pub payload_bytes: usize,
    pub direction: String,
    pub steps: Vec<SearchStep>,
    pub selected_step: usize,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Churn {
    pub cycles: usize,
    pub settled_seconds: f64,
    pub rss_growth_bytes: Vec<i64>,
    pub valid: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub schema_version: u32,
    #[serde(default)]
    pub methodology: Option<String>,
    #[serde(default)]
    pub completion_guard_ms: Option<u64>,
    #[serde(default)]
    pub capacity_32_simultaneous_seconds: Option<u64>,
    #[serde(default)]
    pub elapsed_seconds: Option<f64>,
    #[serde(default)]
    pub wall_budget_seconds: Option<u64>,
    pub topology: String,
    pub link_mbit_s: u64,
    pub reference_rtt_ms: u64,
    pub architecture: String,
    pub role_available_cores: [usize; 2],
    pub network_settings_recorded: bool,
    pub pair_order_seed: u64,
    pub repetitions: usize,
    pub searches: Vec<Search>,
    pub runs: Vec<PairedRun>,
    pub churn: Churn,
}
#[derive(Debug, Serialize)]
pub struct Outcome {
    pub status: String,
    pub invalid: Vec<String>,
    pub failed: Vec<String>,
}
fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}
fn active(direction: &str, index: usize) -> bool {
    direction == "simultaneous" || direction == if index == 0 { "forward" } else { "reverse" }
}
/// Stable SplitMix64 selection binds randomized mode order to the recorded
/// seed/workload, without relying on platform-dependent RNG implementations.
pub fn baseline_first(
    seed: u64,
    pair: usize,
    phase: &str,
    sessions: usize,
    bytes: usize,
    direction: &str,
) -> bool {
    let phase = match phase {
        "capacity" => 1,
        "subcapacity" => 2,
        "resource" => 3,
        _ => 0,
    };
    let direction = match direction {
        "forward" => 1,
        "reverse" => 2,
        "simultaneous" => 3,
        _ => 0,
    };
    let mut value = seed
        ^ ((pair as u64) << 3)
        ^ ((phase as u64) << 1)
        ^ ((sessions as u64) << 11)
        ^ ((bytes as u64) << 18)
        ^ ((direction as u64) << 32);
    value = value.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((value ^ (value >> 31)) & 1) == 1
}
impl Suite {
    pub fn evaluate(&self) -> Outcome {
        use std::collections::HashSet;
        let mut invalid = Vec::new();
        let mut failed = Vec::new();
        let bounded = (self.schema_version == 3
            && self.methodology.as_deref() == Some("bounded-reference-v1")
            && self.completion_guard_ms.is_none())
            || (self.schema_version == 4
                && self.methodology.as_deref() == Some("bounded-reference-v2")
                && self.completion_guard_ms == Some(20));
        if !(bounded || self.schema_version == 2 && self.methodology.is_none())
            || self.capacity_32_simultaneous_seconds.is_some()
                && (self.schema_version != 4 || self.capacity_32_simultaneous_seconds != Some(60))
            || bounded
                && (self.wall_budget_seconds != Some(5400)
                    || self
                        .elapsed_seconds
                        .is_none_or(|v| !nonnegative(v) || v > 5400.0))
            || !bounded
                && (self.wall_budget_seconds.is_some()
                    || self.elapsed_seconds.is_some()
                    || self.completion_guard_ms.is_some())
            || self.topology != "two-leg"
            || self.link_mbit_s != 1000
            || self.reference_rtt_ms != 20
            || !matches!(self.architecture.as_str(), "x86_64" | "aarch64")
            || self.role_available_cores.iter().any(|n| *n < 4)
            || !self.network_settings_recorded
            || if bounded {
                self.repetitions != 3
            } else {
                !(5..=100).contains(&self.repetitions)
            }
        {
            invalid.push("reference topology/platform/pairs/settings".into());
        }
        let mut search_keys = HashSet::new();
        for search in &self.searches {
            let key = (
                search.sessions,
                search.payload_bytes,
                search.direction.as_str(),
            );
            if !search_keys.insert(key)
                || ![1, 32].contains(&search.sessions)
                || ![128, 512, 1200].contains(&search.payload_bytes)
                || !["forward", "reverse", "simultaneous"].contains(&search.direction.as_str())
                || search.steps.is_empty()
                || search.steps.len() > 100
                || search.selected_step >= search.steps.len()
            {
                invalid.push("invalid/duplicate sustainable search".into());
                continue;
            }
            if bounded {
                let mut lower = 0.0;
                let mut upper: Option<f64> = None;
                let mut selected = None;
                for (i, step) in search.steps.iter().enumerate() {
                    let expected = if i == 0 {
                        125.0
                    } else {
                        upper.map_or((lower * 2.0_f64).min(1000.0), |upper| (lower + upper) / 2.0)
                    };
                    if !step.generator_valid
                        || !nonnegative(step.payload_mbit_s)
                        || (step.payload_mbit_s - expected).abs() > 1e-6
                        || step
                            .loss_percent
                            .iter()
                            .any(|v| !nonnegative(*v) || *v > 100.0)
                        || i > 0 && lower == 1000.0
                    {
                        invalid.push("invalid bounded search step/generator".into());
                    }
                    let sustainable = (0..2)
                        .filter(|d| active(&search.direction, *d))
                        .all(|d| step.loss_percent[d] <= 0.1);
                    if sustainable {
                        lower = step.payload_mbit_s;
                        selected = Some(i);
                    } else {
                        upper = Some(step.payload_mbit_s);
                    }
                }
                if selected != Some(search.selected_step)
                    || !(search.steps.len() == 6 || search.steps.len() < 6 && lower == 1000.0)
                {
                    invalid.push("unmatched bounded search stopping policy".into());
                }
            } else {
                for (i, step) in search.steps.iter().enumerate() {
                    if !step.generator_valid
                        || !nonnegative(step.payload_mbit_s)
                        || (step.payload_mbit_s - (i + 1) as f64 * 10.0).abs() > 1e-6
                        || step
                            .loss_percent
                            .iter()
                            .any(|v| !nonnegative(*v) || *v > 100.0)
                    {
                        invalid.push("invalid search step/generator/link mismatch".into());
                    }
                    let sustainable = (0..2)
                        .filter(|d| active(&search.direction, *d))
                        .all(|d| step.loss_percent[d] <= 0.1);
                    if i <= search.selected_step && !sustainable
                        || i > search.selected_step && sustainable
                        || i > search.selected_step + 1
                    {
                        invalid.push("unmatched sustainable search stopping policy".into());
                    }
                }
                if search.selected_step + 1 == search.steps.len() && search.steps.len() != 100 {
                    invalid.push("missing rejected search bracket".into());
                }
            }
        }
        if search_keys.len() != 18 {
            invalid.push("incomplete search coverage".into());
        }
        let mut keys = HashSet::new();
        let mut orders = HashSet::new();
        for p in &self.runs {
            let label = format!(
                "{}:{}:{}:{}:{}",
                p.phase, p.sessions, p.payload_bytes, p.direction, p.pair
            );
            let resource = p.phase == "resource";
            if !keys.insert((
                p.phase.as_str(),
                p.sessions,
                p.payload_bytes,
                p.direction.as_str(),
                p.pair,
            )) || p.pair >= self.repetitions
                || !["capacity", "subcapacity", "resource"].contains(&p.phase.as_str())
                || ![1, 32].contains(&p.sessions)
                || ![128, 512, 1200].contains(&p.payload_bytes)
                || !["forward", "reverse", "simultaneous"].contains(&p.direction.as_str())
                || resource
                    && (p.sessions != 32
                        || p.payload_bytes != 1200
                        || p.direction != "simultaneous")
                || bounded && p.phase == "capacity" && p.payload_bytes != 1200
                || p.warmup_seconds != if bounded { 3 } else { 60 }
                || p.measurement_seconds
                    != if bounded {
                        if resource {
                            60
                        } else if p.phase == "capacity"
                            && p.sessions == 32
                            && p.direction == "simultaneous"
                        {
                            self.capacity_32_simultaneous_seconds.unwrap_or(15)
                        } else {
                            15
                        }
                    } else {
                        300
                    }
            {
                invalid.push(format!(
                    "{label}: incomplete/duplicate workload or interval"
                ));
                continue;
            }
            orders.insert(p.baseline_first);
            if p.baseline_first
                != baseline_first(
                    self.pair_order_seed,
                    p.pair,
                    &p.phase,
                    p.sessions,
                    p.payload_bytes,
                    &p.direction,
                )
            {
                invalid.push(format!("{label}: unmatched randomized mode order/seed"));
            }
            for d in 0..2 {
                let rate = p.offered_pps_per_direction[d];
                let sustainable = p.sustainable_pps_per_direction[d];
                let selected = self
                    .searches
                    .iter()
                    .find(|s| {
                        s.sessions == p.sessions
                            && s.payload_bytes == p.payload_bytes
                            && s.direction == p.direction
                    })
                    .and_then(|s| s.steps.get(s.selected_step))
                    .map(|s| s.payload_mbit_s);
                if active(&p.direction, d) {
                    let payload_mbit = sustainable as f64 * p.payload_bytes as f64 * 8.0 / 1e6;
                    let rounding_mbit = p.sessions as f64 * p.payload_bytes as f64 * 8.0 / 1e6;
                    if sustainable == 0
                        || selected.is_none_or(|s| (s - payload_mbit).abs() > rounding_mbit)
                        || if resource {
                            rate as f64 * 1200.0 * 8.0 / 1e6 < 500.0
                        } else if p.phase == "capacity" {
                            rate != sustainable
                        } else {
                            rate.abs_diff(sustainable / 2) > p.sessions as u64
                        }
                    {
                        invalid.push(format!("{label}: unmatched offered/sustainable load"));
                    }
                } else if rate != 0 || sustainable != 0 {
                    invalid.push(format!("{label}: unexpected inactive load"));
                }
            }
            let mut valid = true;
            for (mode, r) in [("baseline", &p.baseline), ("tunnel", &p.tunnel)] {
                if r.generator_errors != 0
                    || r.integrity_errors != 0
                    || !nonnegative(r.generator_cpu_max)
                    || r.generator_cpu_max >= 0.9
                    || !nonnegative(r.clock_drift_ms)
                    || r.clock_drift_ms >= 10.0
                    || !r.same_clock_rtt
                    || r.diagnostics != "info"
                    || r.coverage_lost != 0
                    || r.counter_overflow
                    || r.sample_count < p.measurement_seconds as usize * 10
                    || r.rtt_samples_per_session.len() != p.sessions
                    || r.rtt_samples_per_session
                        .iter()
                        .any(|n| *n < if bounded { 100 } else { 1000 })
                    || !nonnegative(r.rtt_p95_ms)
                    || r.resources.iter().any(|r| {
                        !nonnegative(r.average_cpu_cores)
                            || !nonnegative(r.owner_average_cpu_cores)
                            || r.peak_rss_bytes == 0
                    })
                    || (0..2).any(|d| {
                        p.offered_pps_per_direction[d].checked_mul(p.measurement_seconds)
                            != Some(r.offered[d])
                            || r.submitted[d] != r.offered[d]
                            || r.unique[d] > r.submitted[d]
                    })
                {
                    invalid.push(format!(
                        "{label}:{mode}: generator/clock/diagnostics/sample evidence"
                    ));
                    valid = false;
                }
                if r.unique_per_session.len() != p.sessions
                    || (0..2).any(|d| {
                        r.unique_per_session
                            .iter()
                            .try_fold(0u64, |n, row| n.checked_add(row[d]))
                            != Some(r.unique[d])
                    })
                {
                    invalid.push(format!(
                        "{label}:{mode}: missing/unmatched per-session delivery"
                    ));
                    valid = false;
                }
            }
            if !valid {
                continue;
            }
            for d in (0..2).filter(|d| active(&p.direction, *d)) {
                let b = &p.baseline;
                let t = &p.tunnel;
                let loss = |r: &Run| {
                    100.0 * (r.offered[d] - r.unique[d]) as f64 / r.offered[d].max(1) as f64
                };
                if loss(b) > 0.1 {
                    invalid.push(format!("{label}: baseline not sustainable"));
                }
                if p.phase == "capacity"
                    && p.payload_bytes == 1200
                    && (b.unique[d] == 0 || (t.unique[d] as f64) < 0.8 * b.unique[d] as f64)
                {
                    failed.push(format!("{label}:SC-003:direction-{d}"));
                }
                if p.phase == "subcapacity" && loss(t) - loss(b) > 0.1 + 1e-9 {
                    failed.push(format!("{label}:SC-004:direction-{d}"));
                }
            }
            if p.phase == "subcapacity" && p.tunnel.rtt_p95_ms - p.baseline.rtt_p95_ms > 5.0 {
                failed.push(format!("{label}:SC-005"));
            }
            if resource {
                let required_bits = 500_000_000u128 * p.measurement_seconds as u128;
                for (mode, run) in [("baseline", &p.baseline), ("tunnel", &p.tunnel)] {
                    if run.unique_per_session.iter().any(|row| row.contains(&0)) {
                        invalid.push(format!("{label}:{mode}: inactive resource Session"));
                    }
                    for d in 0..2 {
                        let delivered_bits = run.unique[d] as u128 * p.payload_bytes as u128 * 8;
                        if delivered_bits < required_bits {
                            if mode == "baseline" {
                                invalid.push(format!(
                                    "{label}: baseline resource load below 500 Mbit/s:direction-{d}"
                                ));
                            } else {
                                failed.push(format!("{label}:SC-007:delivered-load:direction-{d}"));
                            }
                        }
                    }
                }
                for (i, r) in p.tunnel.resources.iter().enumerate() {
                    if r.peak_rss_bytes > 128 * 1024 * 1024 || r.average_cpu_cores > 2.0 {
                        failed.push(format!("{label}:SC-007:role-{i}"));
                    }
                }
            }
        }
        let repetitions = self.repetitions.min(100);
        for pair in 0..repetitions {
            for phase in ["capacity", "subcapacity"] {
                for sessions in [1, 32] {
                    for bytes in [128, 512, 1200] {
                        if bounded && phase == "capacity" && bytes != 1200 {
                            continue;
                        }
                        for dir in ["forward", "reverse", "simultaneous"] {
                            if !keys.contains(&(phase, sessions, bytes, dir, pair)) {
                                invalid.push(format!(
                                    "missing {phase}/{sessions}/{bytes}/{dir}/{pair}"
                                ));
                            }
                        }
                    }
                }
            }
            if !keys.contains(&("resource", 32, 1200, "simultaneous", pair)) {
                invalid.push(format!("missing resource/{pair}"));
            }
        }
        if keys.len() != repetitions * if bounded { 25 } else { 37 } || orders.len() != 2 {
            invalid.push("matrix or randomized order coverage".into());
        }
        if !self.churn.valid
            || self.churn.cycles != 1000
            || !nonnegative(self.churn.settled_seconds)
            || self.churn.rss_growth_bytes.is_empty()
        {
            invalid.push("invalid/missing churn evidence".into());
        } else if self.churn.settled_seconds > 60.0
            || self
                .churn
                .rss_growth_bytes
                .iter()
                .any(|v| *v > 10 * 1024 * 1024)
        {
            failed.push("SC-007:churn".into());
        }
        Outcome {
            status: if !invalid.is_empty() {
                "invalid"
            } else if !failed.is_empty() {
                "failed"
            } else {
                "passed"
            }
            .into(),
            invalid,
            failed,
        }
    }
}
