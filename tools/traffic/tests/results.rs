use remora_traffic::{Collection, measurement::*, packet};

fn suite() -> Suite {
    let mut searches = Vec::new();
    let mut runs = Vec::new();
    for sessions in [1, 32] {
        for payload_bytes in [128, 512, 1200] {
            for direction in ["forward", "reverse", "simultaneous"] {
                let steps = (1..=51)
                    .map(|i| SearchStep {
                        payload_mbit_s: i as f64 * 10.,
                        loss_percent: if i <= 50 { [0.; 2] } else { [1.; 2] },
                        generator_valid: true,
                    })
                    .collect();
                searches.push(Search {
                    sessions,
                    payload_bytes,
                    direction: direction.into(),
                    steps,
                    selected_step: 49,
                });
                for phase in ["capacity", "subcapacity"] {
                    for pair in 0..5 {
                        let sustainable =
                            500_000_000 / (payload_bytes as u64 * 8) / sessions as u64
                                * sessions as u64;
                        let rate = if phase == "capacity" {
                            sustainable
                        } else {
                            sustainable / 2 / sessions as u64 * sessions as u64
                        };
                        let active = |d| {
                            direction == "simultaneous"
                                || direction == if d == 0 { "forward" } else { "reverse" }
                        };
                        let offered =
                            std::array::from_fn(|d| if active(d) { rate * 300 } else { 0 });
                        let run = Run {
                            offered,
                            submitted: offered,
                            unique: offered,
                            unique_per_session: vec![
                                offered.map(|n| n / sessions as u64);
                                sessions
                            ],
                            generator_errors: 0,
                            integrity_errors: 0,
                            generator_cpu_max: 0.3,
                            clock_drift_ms: 0.,
                            same_clock_rtt: true,
                            diagnostics: "info".into(),
                            coverage_lost: 0,
                            counter_overflow: false,
                            sample_count: 6000,
                            rtt_samples_per_session: vec![3000; sessions],
                            rtt_p95_ms: 20.,
                            resources: std::array::from_fn(|_| Resource {
                                average_cpu_cores: 1.,
                                owner_average_cpu_cores: 0.8,
                                peak_rss_bytes: 64 * 1024 * 1024,
                            }),
                        };
                        runs.push(PairedRun {
                            pair,
                            phase: phase.into(),
                            sessions,
                            payload_bytes,
                            direction: direction.into(),
                            baseline_first: baseline_first(
                                7,
                                pair,
                                phase,
                                sessions,
                                payload_bytes,
                                direction,
                            ),
                            warmup_seconds: 60,
                            measurement_seconds: 300,
                            offered_pps_per_direction: std::array::from_fn(|d| {
                                if active(d) { rate } else { 0 }
                            }),
                            sustainable_pps_per_direction: std::array::from_fn(|d| {
                                if active(d) { sustainable } else { 0 }
                            }),
                            baseline: run.clone(),
                            tunnel: run,
                        });
                    }
                }
            }
        }
    }
    for pair in 0..5 {
        let mut p = runs
            .iter()
            .find(|p| {
                p.sessions == 32
                    && p.payload_bytes == 1200
                    && p.direction == "simultaneous"
                    && p.phase == "capacity"
                    && p.pair == pair
            })
            .unwrap()
            .clone();
        p.phase = "resource".into();
        p.baseline_first = baseline_first(7, pair, "resource", 32, 1200, "simultaneous");
        let rate = 52_084u64.div_ceil(32) * 32;
        p.offered_pps_per_direction = [rate; 2];
        for r in [&mut p.baseline, &mut p.tunnel] {
            r.offered = [rate * 300; 2];
            r.submitted = r.offered;
            r.unique = r.offered;
            r.unique_per_session = vec![r.offered.map(|n| n / 32); 32];
        }
        runs.push(p);
    }
    Suite {
        capacity_32_simultaneous_seconds: None,
        schema_version: 2,
        methodology: None,
        completion_guard_ms: None,
        elapsed_seconds: None,
        wall_budget_seconds: None,
        topology: "two-leg".into(),
        link_mbit_s: 1000,
        reference_rtt_ms: 20,
        architecture: "x86_64".into(),
        role_available_cores: [4, 4],
        network_settings_recorded: true,
        pair_order_seed: 7,
        repetitions: 5,
        searches,
        runs,
        churn: Churn {
            cycles: 1000,
            settled_seconds: 60.,
            rss_growth_bytes: vec![10 * 1024 * 1024; 5],
            valid: true,
        },
    }
}
fn capacity(s: &mut Suite) -> &mut PairedRun {
    s.runs
        .iter_mut()
        .find(|r| r.phase == "capacity" && r.payload_bytes == 1200 && r.direction == "simultaneous")
        .unwrap()
}
fn subcapacity(s: &mut Suite) -> &mut PairedRun {
    s.runs
        .iter_mut()
        .find(|r| r.phase == "subcapacity" && r.direction == "simultaneous")
        .unwrap()
}
fn resource(s: &mut Suite) -> &mut PairedRun {
    s.runs.iter_mut().find(|r| r.phase == "resource").unwrap()
}
fn delivery(run: &mut Run, direction: usize, unique: u64) {
    run.unique[direction] = unique;
    let sessions = run.unique_per_session.len() as u64;
    for (i, row) in run.unique_per_session.iter_mut().enumerate() {
        row[direction] = unique / sessions + u64::from((i as u64) < unique % sessions);
    }
}

#[test]
fn full_matrix_validates_each_run_and_roundtrips_strict_json() {
    let s = suite();
    assert_eq!(s.runs.len(), 185);
    assert_eq!(s.evaluate().status, "passed");
    let value = serde_json::to_value(&s).unwrap();
    let decoded: Suite = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.evaluate().status, "passed");
    let mut missing_delivery = value.clone();
    missing_delivery["runs"][0]["tunnel"]
        .as_object_mut()
        .unwrap()
        .remove("unique_per_session");
    assert!(serde_json::from_value::<Suite>(missing_delivery).is_err());
    let mut value = value;
    value["unrecognized_limit"] = serde_json::json!(42);
    assert!(serde_json::from_value::<Suite>(value).is_err());
}
#[test]
fn a_single_bad_direction_or_pair_cannot_be_averaged_away() {
    for index in [0, 1] {
        let mut s = suite();
        let p = capacity(&mut s);
        delivery(&mut p.tunnel, index, p.baseline.unique[index] * 79 / 100);
        let result = s.evaluate();
        assert_eq!(result.status, "failed");
        assert!(result.failed.iter().any(|v| v.contains("SC-003")));
    }
    let mut s = suite();
    let p = capacity(&mut s);
    for d in 0..2 {
        delivery(&mut p.tunnel, d, p.baseline.unique[d] * 80 / 100);
    }
    assert_eq!(s.evaluate().status, "passed");
}
#[test]
fn loss_percentage_points_and_rtt_absolute_boundary_are_independent() {
    let mut s = suite();
    let p = subcapacity(&mut s);
    let unique = p.tunnel.unique[0] - p.tunnel.offered[0] / 500;
    delivery(&mut p.tunnel, 0, unique);
    assert!(s.evaluate().failed.iter().any(|v| v.contains("SC-004")));
    let mut s = suite();
    subcapacity(&mut s).tunnel.rtt_p95_ms = 25.;
    assert_eq!(s.evaluate().status, "passed");
    subcapacity(&mut s).tunnel.rtt_p95_ms = 25.001;
    assert!(s.evaluate().failed.iter().any(|v| v.contains("SC-005")));
}
#[test]
fn resource_thresholds_apply_per_role_and_churn_not_means() {
    for role in 0..2 {
        let mut s = suite();
        resource(&mut s).tunnel.resources[role].average_cpu_cores = 2.001;
        assert!(s.evaluate().failed.iter().any(|v| v.contains("SC-007")));
        let mut s = suite();
        resource(&mut s).tunnel.resources[role].peak_rss_bytes = 128 * 1024 * 1024 + 1;
        assert_eq!(s.evaluate().status, "failed");
    }
    let mut s = suite();
    s.churn.settled_seconds = 60.001;
    assert_eq!(s.evaluate().status, "failed");
    let mut s = suite();
    s.churn.rss_growth_bytes[4] += 1;
    assert_eq!(s.evaluate().status, "failed");
}
#[test]
fn resource_qualification_requires_actual_load_in_both_directions() {
    for direction in 0..2 {
        for baseline in [false, true] {
            let mut s = suite();
            let p = resource(&mut s);
            let row = if baseline {
                &mut p.baseline
            } else {
                &mut p.tunnel
            };
            delivery(row, direction, 1);
            assert_ne!(
                s.evaluate().status,
                "passed",
                "baseline={baseline}, direction={direction}"
            );
        }
    }
    let required = (500_000_000u64 * 300).div_ceil(1200 * 8);
    for d in 0..2 {
        let mut s = suite();
        delivery(&mut resource(&mut s).tunnel, d, required);
        assert_eq!(s.evaluate().status, "passed");
        delivery(&mut resource(&mut s).tunnel, d, required - 1);
        let result = s.evaluate();
        assert_eq!(result.status, "failed");
        assert!(
            result
                .failed
                .iter()
                .any(|v| v.contains("SC-007:delivered-load"))
        );
    }
    let mut s = suite();
    let row = &mut resource(&mut s).tunnel;
    row.unique_per_session[1][0] += row.unique_per_session[0][0];
    row.unique_per_session[0][0] = 0;
    assert_eq!(
        s.evaluate().status,
        "invalid",
        "31 Sessions cannot qualify as 32"
    );
}
#[test]
fn evidence_failures_are_invalid_even_with_excellent_numeric_results() {
    let changes: Vec<fn(&mut Suite)> = vec![
        |s| s.topology = "shorter-route".into(),
        |s| s.link_mbit_s = 100,
        |s| s.reference_rtt_ms = 50,
        |s| s.network_settings_recorded = false,
        |s| s.pair_order_seed += 1,
        |s| resource(s).tunnel.unique[0] = 0,
        |s| {
            resource(s).tunnel.unique_per_session.pop();
        },
        |s| resource(s).tunnel.unique_per_session[0][0] += 1,
        |s| s.role_available_cores[1] = 3,
        |s| s.repetitions = 4,
        |s| {
            s.runs.pop();
        },
        |s| s.runs.push(s.runs[0].clone()),
        |s| s.runs[0].tunnel.generator_errors = 1,
        |s| s.runs[0].tunnel.generator_cpu_max = 0.9,
        |s| s.runs[0].tunnel.diagnostics = "error".into(),
        |s| s.runs[0].tunnel.coverage_lost = 1,
        |s| s.runs[0].tunnel.sample_count = 1,
        |s| s.runs[0].tunnel.rtt_samples_per_session[0] = 999,
        |s| s.runs[0].tunnel.same_clock_rtt = false,
        |s| s.runs[0].tunnel.clock_drift_ms = f64::NAN,
        |s| s.runs[0].tunnel.clock_drift_ms = 10.,
        |s| s.runs[0].tunnel.counter_overflow = true,
        |s| s.runs[0].tunnel.unique[0] = u64::MAX,
        |s| s.runs[0].measurement_seconds = 299,
        |s| s.runs[0].warmup_seconds = 59,
        |s| s.runs[0].offered_pps_per_direction[0] += 32,
        |s| s.searches[0].steps[0].generator_valid = false,
        |s| s.searches[0].steps[0].payload_mbit_s = 1.,
        |s| s.searches[0].selected_step = 50,
        |s| {
            s.searches.pop();
        },
        |s| s.churn.cycles = 999,
        |s| s.churn.rss_growth_bytes.clear(),
        |s| {
            for r in &mut s.runs {
                r.baseline_first = true;
            }
        },
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut s = suite();
        change(&mut s);
        assert_eq!(s.evaluate().status, "invalid", "mutation {i}");
    }
}
#[test]
fn release_collection_crosses_old_million_sequence_limit_without_unbounded_hashset() {
    let mut c = Collection::with_limit(1_000_100).unwrap();
    for seq in [999_999, 1_000_002, 1_000_001, 1_000_002] {
        c.observe(&packet(128, 1, 0, seq, 0).unwrap(), 1, 0);
    }
    assert_eq!(
        (c.unique, c.duplicates, c.reordered, c.overflow),
        (3, 1, 1, 0)
    );
    c.observe(&packet(128, 1, 0, 1_000_100, 0).unwrap(), 1, 0);
    assert_eq!(c.overflow, 1);
    assert!(Collection::with_limit(400_000_001).is_err());
}

#[test]
fn both_declared_native_architectures_preserve_the_same_release_thresholds() {
    for architecture in ["x86_64", "aarch64"] {
        let mut s = suite();
        s.architecture = architecture.into();
        assert_eq!(s.evaluate().status, "passed");
    }
    let mut s = suite();
    s.architecture = "emulated-arm64".into();
    assert_eq!(s.evaluate().status, "invalid");
}

fn bounded_suite() -> Suite {
    let mut s = suite();
    s.schema_version = 3;
    s.methodology = Some("bounded-reference-v1".into());
    s.repetitions = 3;
    s.elapsed_seconds = Some(4700.0);
    s.wall_budget_seconds = Some(5400);
    for search in &mut s.searches {
        search.steps = [125., 250., 500., 1000., 750., 625.]
            .into_iter()
            .map(|rate| SearchStep {
                payload_mbit_s: rate,
                loss_percent: if rate <= 500. { [0.; 2] } else { [1.; 2] },
                generator_valid: true,
            })
            .collect();
        search.selected_step = 2;
    }
    s.runs
        .retain(|r| r.pair < 3 && (r.phase != "capacity" || r.payload_bytes == 1200));
    for p in &mut s.runs {
        let seconds = if p.phase == "resource" { 60 } else { 15 };
        p.warmup_seconds = 3;
        p.measurement_seconds = seconds;
        for r in [&mut p.baseline, &mut p.tunnel] {
            r.offered = p.offered_pps_per_direction.map(|rate| rate * seconds);
            r.submitted = r.offered;
            r.unique = r.offered;
            r.unique_per_session = vec![r.offered.map(|n| n / p.sessions as u64); p.sessions];
        }
    }
    s
}
#[test]
fn bounded_method_keeps_thresholds_and_rejects_partial_or_mixed_evidence() {
    let good = bounded_suite();
    assert_eq!(good.runs.len(), 75);
    assert_eq!(good.evaluate().status, "passed");
    let bytes = serde_json::to_vec(&good).unwrap();
    assert_eq!(
        serde_json::from_slice::<Suite>(&bytes)
            .unwrap()
            .evaluate()
            .status,
        "passed"
    );
    let mut bad = good.clone();
    bad.runs[0].tunnel.rtt_samples_per_session[0] = 99;
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.elapsed_seconds = Some(5400.001);
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.methodology = None;
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.runs.pop();
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.runs[0].measurement_seconds = 3;
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.searches[0].steps[2].payload_mbit_s = 700.;
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    bad.searches[0].selected_step = 3;
    assert_eq!(bad.evaluate().status, "invalid");
    let mut bad = good.clone();
    subcapacity(&mut bad).tunnel.rtt_p95_ms += 5.001;
    assert_eq!(bad.evaluate().status, "failed");
    let mut bad = good.clone();
    capacity(&mut bad).tunnel.unique = capacity(&mut bad).tunnel.unique.map(|n| n * 79 / 100);
    let c = capacity(&mut bad);
    c.tunnel.unique_per_session = vec![c.tunnel.unique.map(|n| n / c.sessions as u64); c.sessions];
    assert_eq!(bad.evaluate().status, "failed");
}

#[test]
fn bounded_search_accepts_early_ceiling_and_rejects_other_early_stops() {
    let mut good = bounded_suite();
    let search = &mut good.searches[0];
    search.steps.truncate(4);
    search.steps[3].loss_percent = [0.; 2];
    search.selected_step = 3;
    // Keep the paired evidence consistent with the newly selected 1000 Mbit/s.
    for p in &mut good.runs {
        if p.sessions != search.sessions
            || p.payload_bytes != search.payload_bytes
            || p.direction != search.direction
        {
            continue;
        }
        let sustainable =
            1_000_000_000 / (p.payload_bytes as u64 * 8) / p.sessions as u64 * p.sessions as u64;
        p.sustainable_pps_per_direction = p
            .sustainable_pps_per_direction
            .map(|rate| if rate == 0 { 0 } else { sustainable });
        p.offered_pps_per_direction = p.offered_pps_per_direction.map(|rate| {
            if rate == 0 {
                0
            } else {
                sustainable / 2 / p.sessions as u64 * p.sessions as u64
            }
        });
        for r in [&mut p.baseline, &mut p.tunnel] {
            r.offered = p
                .offered_pps_per_direction
                .map(|rate| rate * p.measurement_seconds);
            r.submitted = r.offered;
            r.unique = r.offered;
            r.unique_per_session = vec![r.offered.map(|n| n / p.sessions as u64); p.sessions];
        }
    }
    assert_eq!(good.evaluate().status, "passed", "{:?}", good.evaluate());
    let mut v2 = good.clone();
    v2.schema_version = 4;
    v2.methodology = Some("bounded-reference-v2".into());
    v2.completion_guard_ms = Some(20);
    assert_eq!(v2.evaluate().status, "passed");
    let stopping = "unmatched bounded search stopping policy";
    for length in 1..4 {
        let mut bad = good.clone();
        bad.searches[0].steps.truncate(length);
        bad.searches[0].selected_step = length - 1;
        assert!(bad.evaluate().invalid.iter().any(|s| s == stopping));
    }
    let mut bad = good.clone();
    bad.searches[0].steps[3].loss_percent = [1.; 2];
    bad.searches[0].selected_step = 2;
    assert!(bad.evaluate().invalid.iter().any(|s| s == stopping));
    for length in [5, 6, 7] {
        let mut bad = good.clone();
        while bad.searches[0].steps.len() < length {
            let extra = bad.searches[0].steps[3].clone();
            bad.searches[0].steps.push(extra);
        }
        bad.searches[0].selected_step = length - 1;
        assert!(
            bad.evaluate()
                .invalid
                .iter()
                .any(|s| s == "invalid bounded search step/generator")
        );
    }
}

#[test]
fn completion_reserve_method_requires_explicit_identity_and_preserves_older_suites() {
    let mut s = bounded_suite();
    assert_eq!(s.evaluate().status, "passed");
    s.schema_version = 4;
    s.methodology = Some("bounded-reference-v2".into());
    s.completion_guard_ms = Some(20);
    assert_eq!(s.evaluate().status, "passed");
    s.completion_guard_ms = Some(21);
    assert_eq!(s.evaluate().status, "invalid");
    s.completion_guard_ms = None;
    assert_eq!(s.evaluate().status, "invalid");
}

#[test]
fn extended_capacity_requires_declared_matched_intervals_and_unchanged_thresholds() {
    let mut s = bounded_suite();
    s.schema_version = 4;
    s.methodology = Some("bounded-reference-v2".into());
    s.completion_guard_ms = Some(20);
    s.capacity_32_simultaneous_seconds = Some(60);
    // A declaration alone cannot qualify the old 15-second rows.
    assert_eq!(s.evaluate().status, "invalid");
    for p in &mut s.runs {
        if p.phase == "capacity" && p.sessions == 32 && p.direction == "simultaneous" {
            p.measurement_seconds = 60;
            for run in [&mut p.baseline, &mut p.tunnel] {
                run.offered = run.offered.map(|n| n * 4);
                run.submitted = run.submitted.map(|n| n * 4);
                run.unique = run.unique.map(|n| n * 4);
                for session in &mut run.unique_per_session {
                    *session = session.map(|n| n * 4);
                }
                run.sample_count = 600;
            }
        }
    }
    assert_eq!(s.evaluate().status, "passed");
    let mut undeclared = s.clone();
    undeclared.capacity_32_simultaneous_seconds = None;
    assert_eq!(undeclared.evaluate().status, "invalid");
    let mut wrong_method = s.clone();
    wrong_method.schema_version = 3;
    wrong_method.methodology = Some("bounded-reference-v1".into());
    wrong_method.completion_guard_ms = None;
    assert_eq!(wrong_method.evaluate().status, "invalid");
    let mut missing_probes = s.clone();
    missing_probes
        .runs
        .iter_mut()
        .find(|p| p.phase == "capacity" && p.sessions == 32 && p.direction == "simultaneous")
        .unwrap()
        .tunnel
        .rtt_samples_per_session[0] = 99;
    assert_eq!(missing_probes.evaluate().status, "invalid");
    let row = s
        .runs
        .iter_mut()
        .find(|p| p.phase == "capacity" && p.sessions == 32 && p.direction == "simultaneous")
        .unwrap();
    delivery(&mut row.tunnel, 0, row.baseline.unique[0] * 79 / 100);
    assert_eq!(s.evaluate().status, "failed");
}
