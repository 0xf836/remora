use remora_traffic::{Collection, baseline, decode, measurement, packet};
use std::{
    collections::HashMap,
    io,
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
fn args() -> Result<(String, HashMap<String, String>), &'static str> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or("command required")?;
    let mut fields = HashMap::new();
    while let Some(flag) = args.next() {
        if !flag.starts_with("--")
            || fields
                .insert(flag, args.next().ok_or("value required")?)
                .is_some()
        {
            return Err("invalid or duplicate option");
        }
    }
    Ok((cmd, fields))
}
fn required<'a>(a: &'a HashMap<String, String>, key: &str) -> Result<&'a str, &'static str> {
    a.get(key).map(String::as_str).ok_or("missing option")
}
fn number(a: &HashMap<String, String>, key: &str, default: u64) -> Result<u64, &'static str> {
    a.get(key)
        .map_or(Ok(default), |v| v.parse().map_err(|_| "invalid number"))
}
fn millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn slot_offset(tick: u64, rate: u64, quantum_us: u64) -> Duration {
    let nanos = tick * 1_000_000_000 / rate;
    let nanos = if quantum_us == 0 {
        nanos
    } else {
        nanos / (quantum_us * 1000) * (quantum_us * 1000)
    };
    Duration::from_nanos(nanos)
}
fn scheduled_offset(
    tick: u64,
    rate: u64,
    warmup: u64,
    seconds: u64,
    quantum_us: u64,
    guard_ms: u64,
) -> Duration {
    if guard_ms == 0 || tick < warmup * rate {
        return slot_offset(tick, rate, quantum_us);
    }
    let measured_tick = tick - warmup * rate;
    if measured_tick >= seconds * rate {
        return Duration::from_secs(warmup + seconds);
    }
    let active_ns = seconds * 1_000_000_000 - guard_ms * 1_000_000;
    let offset_ns = (measured_tick as u128 * active_ns as u128 / (seconds * rate) as u128) as u64;
    let offset_ns = if quantum_us == 0 {
        offset_ns
    } else {
        offset_ns / (quantum_us * 1000) * (quantum_us * 1000)
    };
    Duration::from_secs(warmup) + Duration::from_nanos(offset_ns)
}
fn receive_visit_allowed(
    now: Instant,
    next_send: Instant,
    finish: Instant,
    generating: bool,
) -> bool {
    // Receiving one packet can cross a slot deadline. Recheck between packets,
    // rather than finishing the entire 32-packet visit before servicing output.
    !generating || now >= finish || next_send >= finish || now < next_send
}
fn generator_pause(
    now: Instant,
    next_send: Instant,
    finish: Instant,
    generating: bool,
) -> Duration {
    // A service visit is bounded to 32 slots. Eligible backlog needs another
    // visit immediately; sleeping here changes the declared offered load.
    // Only the final <=2ms uses a bounded no-sleep guard against timer overshoot.
    // No slot eligibility/cutoff is changed, and drain-only work still sleeps.
    if generating
        && now < finish
        && next_send < finish
        && (next_send <= now || finish.duration_since(now) <= Duration::from_millis(2))
    {
        Duration::ZERO
    } else {
        Duration::from_micros(50)
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("traffic tool failed: {e}");
        std::process::exit(1);
    }
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (cmd, a) = args()?;
    let known = [
        "--listen",
        "--to",
        "--bind",
        "--io",
        "--seconds",
        "--pps",
        "--reverse-pps",
        "--pacing-quantum-us",
        "--completion-guard-ms",
        "--size",
        "--session",
        "--start-ms",
        "--drain-ms",
        "--sessions",
        "--batch-max",
        "--application-pool-bytes",
        "--outer-socket-buffer-bytes",
        "--warmup-seconds",
        "--input",
        "--output",
    ];
    if a.keys().any(|key| !known.contains(&key.as_str())) {
        return Err("unknown option".into());
    }
    if cmd == "validate-results" {
        let path = std::path::Path::new(required(&a, "--input")?);
        if std::fs::metadata(path)?.len() > 32 * 1024 * 1024 {
            return Err("oversized measurement input".into());
        }
        let suite: measurement::Suite = serde_json::from_slice(&std::fs::read(path)?)?;
        let result = suite.evaluate();
        let bytes = serde_json::to_vec_pretty(&result)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(required(&a, "--output")?)?;
        std::io::Write::write_all(&mut file, &bytes)?;
        println!("{}", serde_json::to_string(&result)?);
        if result.status != "passed" {
            return Err("measurement failed or invalid; original evidence retained".into());
        }
        return Ok(());
    }
    if cmd == "exercise" {
        return exercise(
            required(&a, "--to")?.parse()?,
            number(&a, "--sessions", 1)? as usize,
        );
    }
    if cmd.starts_with("baseline-") {
        return baseline::run(
            cmd == "baseline-server",
            required(&a, "--listen")?.parse()?,
            required(&a, "--to")?.parse()?,
            a.get("--io").map_or("mmsg", String::as_str),
            number(&a, "--batch-max", 32)? as usize,
            number(&a, "--application-pool-bytes", 8 * 1024 * 1024)? as usize,
            usize::try_from(number(
                &a,
                "--outer-socket-buffer-bytes",
                remora::config::IoSettings::default().outer_socket_buffer_bytes as u64,
            )?)?,
        )
        .map_err(Into::into);
    }
    if !["sender", "receiver", "responder"].contains(&cmd.as_str()) {
        return Err("unknown command".into());
    }
    let seconds = number(&a, "--seconds", 3)?;
    let warmup = number(&a, "--warmup-seconds", 0)?;
    let sessions = number(&a, "--sessions", 1)?;
    let pps = number(&a, "--pps", 1000)?;
    let reverse = number(&a, "--reverse-pps", 0)?;
    let completion_guard = number(&a, "--completion-guard-ms", 0)?;
    let pacing_quantum = number(&a, "--pacing-quantum-us", 0)?;
    let size = number(&a, "--size", 128)? as usize;
    let session = number(&a, "--session", 1)?;
    let drain = number(&a, "--drain-ms", 200)?;
    if ![0, 20].contains(&completion_guard)
        || ![0, 100].contains(&pacing_quantum)
        || seconds == 0
        || seconds > 300
        || warmup > 60
        || ![1, 32].contains(&sessions)
        || pps > 1_000_000 / sessions
        || reverse > 1_000_000 / sessions
        || !(40..=1200).contains(&size)
        || drain == 0
        || drain > 5000
    {
        return Err("bounded traffic profile exceeded".into());
    }
    let start_ms = number(&a, "--start-ms", millis() + 1000)?;
    let wall = millis();
    if start_ms < wall || start_ms > wall + 120_000 {
        return Err("start clock outside bounded window".into());
    }
    let base = Instant::now();
    let start = base + Duration::from_millis(start_ms - wall);
    let warm_start = start
        .checked_sub(Duration::from_secs(warmup))
        .ok_or("warmup clock range")?;
    if warmup > 0 && warm_start < base {
        return Err("incomplete warmup start".into());
    }
    let finish = start + Duration::from_secs(seconds);
    let end = finish + Duration::from_millis(drain);
    let sender = cmd == "sender";
    let address = if sender {
        a.get("--bind").map_or("127.0.0.1:0", String::as_str)
    } else {
        required(&a, "--listen")?
    };
    let socket = UdpSocket::bind(address)?;
    socket.set_nonblocking(true)?;
    socket.set_read_timeout(None)?;
    let peer: Option<SocketAddr> = if sender {
        Some(required(&a, "--to")?.parse()?)
    } else {
        None
    };
    let mut peers: HashMap<u64, SocketAddr> = HashMap::new();
    let mut sequences: HashMap<u64, u64> = HashMap::new();
    let mut collected: HashMap<u64, Collection> = HashMap::new();
    let sequence_limit = (pps.max(reverse) * seconds + 1).max(1);
    let mut reverse_collection = Collection::with_limit(sequence_limit)?;
    let mut rtt = Vec::new();
    let mut sent = 0u64;
    let mut enqueue_failures = 0u64;
    let mut offered = 0u64;
    let mut next_send = warm_start;
    let mut send_ticks = 0u64;
    let mut warm_sequence = 0;
    let mut warm_sequences: HashMap<u64, u64> = HashMap::new();
    let mut warm_offered = 0u64;
    let mut warm_failures = 0u64;
    const WARM: u64 = 1 << 63;
    let mut next_probe = base;
    let mut probe_seq = 0u64;
    let mut sequence = 0u64;
    let mut buffer = [0; 65536];
    let mut previous_visit = base;
    let mut tail_max_visit_gap_us = 0u128;
    let mut last_measured_submission_remaining_us: Option<i128> = None;
    while Instant::now() < end {
        let now = Instant::now();
        if previous_visit < finish && now >= finish - Duration::from_millis(20) {
            tail_max_visit_gap_us =
                tail_max_visit_gap_us.max(now.duration_since(previous_visit).as_micros());
        }
        previous_visit = now;
        if sender && now < finish && now >= next_probe {
            let bytes = packet(
                128,
                session,
                2,
                probe_seq,
                base.elapsed().as_micros() as u64,
            )?;
            let _ = socket.send_to(&bytes, peer.unwrap());
            probe_seq += 1;
            next_probe += Duration::from_millis(100);
        }
        let rate = if sender { pps } else { reverse };
        if rate > 0 && now >= warm_start && now < finish && now >= next_send {
            // A generator catches up at most 32 packets; enqueue errors and scheduling
            // inability remain evidence, rather than changing offered rate silently.
            for _ in 0..32 {
                let visit_now = Instant::now();
                if visit_now < next_send || visit_now >= finish || next_send >= finish {
                    break;
                }
                let measured = next_send >= start;
                if sender {
                    let bytes = packet(
                        size,
                        session,
                        0,
                        if measured {
                            sequence
                        } else {
                            WARM | warm_sequence
                        },
                        base.elapsed().as_micros() as u64,
                    )?;
                    if measured {
                        offered += 1;
                    } else {
                        warm_offered += 1;
                    }
                    if socket
                        .send_to(&bytes, peer.unwrap())
                        .is_ok_and(|n| n == bytes.len())
                    {
                        if measured {
                            sent += 1;
                        }
                    } else if measured {
                        enqueue_failures += 1;
                    } else {
                        warm_failures += 1;
                    }
                    if measured {
                        sequence += 1;
                    } else {
                        warm_sequence += 1;
                    }
                } else {
                    for (id, to) in &peers {
                        if Instant::now() >= finish {
                            break;
                        }
                        let seq = if measured {
                            sequences.entry(*id).or_default()
                        } else {
                            warm_sequences.entry(*id).or_default()
                        };
                        let bytes =
                            packet(size, *id, 1, if measured { *seq } else { WARM | *seq }, 0)?;
                        if measured {
                            offered += 1;
                        } else {
                            warm_offered += 1;
                        }
                        if socket.send_to(&bytes, *to).is_ok_and(|n| n == bytes.len()) {
                            if measured {
                                sent += 1;
                            }
                        } else if measured {
                            enqueue_failures += 1;
                        } else {
                            warm_failures += 1;
                        }
                        *seq += 1;
                    }
                }
                if measured {
                    let submitted_at = Instant::now();
                    last_measured_submission_remaining_us = Some(if submitted_at < finish {
                        finish.duration_since(submitted_at).as_micros() as i128
                    } else {
                        -(submitted_at.duration_since(finish).as_micros() as i128)
                    });
                }
                send_ticks += 1;
                next_send = warm_start
                    + scheduled_offset(
                        send_ticks,
                        rate,
                        warmup,
                        seconds,
                        pacing_quantum,
                        completion_guard,
                    );
            }
        }
        for _ in 0..32 {
            if !receive_visit_allowed(Instant::now(), next_send, finish, rate > 0) {
                break;
            }
            match socket.recv_from(&mut buffer) {
                Ok((n, from)) => {
                    let bytes = &buffer[..n];
                    if sender {
                        if Some(from) != peer {
                            continue;
                        }
                        match decode(bytes) {
                            Ok(h) if h.direction == 2 && h.session == session => {
                                let age = base.elapsed().as_micros() as u64;
                                if now >= start
                                    && h.stamp >= (start - base).as_micros() as u64
                                    && rtt.len() < 65536
                                {
                                    rtt.push(age.saturating_sub(h.stamp));
                                }
                            }
                            Ok(h)
                                if h.sequence & WARM != 0
                                    && h.direction == 1
                                    && h.session == session => {}
                            _ => reverse_collection.observe(bytes, session, 1),
                        }
                    } else if let Ok(h) = decode(bytes) {
                        if peers.len() < sessions as usize || peers.contains_key(&h.session) {
                            if peers.get(&h.session).is_some_and(|old| *old != from) {
                                continue;
                            }
                            peers.insert(h.session, from);
                        } else {
                            continue;
                        }
                        if h.direction == 2 {
                            socket.send_to(bytes, from)?;
                        } else if h.sequence & WARM == 0 {
                            collected
                                .entry(h.session)
                                .or_insert_with(|| {
                                    Collection::with_limit(sequence_limit).expect("validated bound")
                                })
                                .observe(bytes, h.session, 0);
                        }
                    } else if cmd == "responder" {
                        socket.send_to(bytes, from)?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        let pause = generator_pause(Instant::now(), next_send, finish, rate > 0);
        if pause.is_zero() {
            std::hint::spin_loop();
        } else {
            std::thread::sleep(pause);
        }
    }
    let rate = if sender { pps } else { reverse };
    let expected = rate * seconds * if sender { 1 } else { peers.len() as u64 };
    let generator_unscheduled = expected.saturating_sub(offered);
    let p95 = measurement::percentile(&mut rtt.clone(), 95);
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"completion_guard_ms":completion_guard,"tail_max_visit_gap_us":tail_max_visit_gap_us,"last_measured_submission_remaining_us":last_measured_submission_remaining_us,"pacing_quantum_us":pacing_quantum,"role":cmd,"size":size,"sessions":if sender{1}else{peers.len()},"interval_seconds":seconds,"drain_ms":drain,"warmup_seconds":warmup,"warmup_offered":warm_offered,"warmup_failures":warm_failures,"offered":offered,"submitted":sent,"enqueue_failures":enqueue_failures,"generator_unscheduled":generator_unscheduled,"generator_excess":offered.saturating_sub(expected),"reverse_received":reverse_collection,"forward_received":collected,"rtt_samples_us":rtt,"rtt_p95_us":p95,"start_ms":start_ms,"elapsed_ms":base.elapsed().as_millis()})
    );
    Ok(())
}

fn exercise(to: SocketAddr, sessions: usize) -> Result<(), Box<dyn std::error::Error>> {
    if ![1, 2, 32].contains(&sessions) {
        return Err("unsupported exercise session count".into());
    }
    let mut delivered = 0;
    let mut attempts = 0;
    for session in 0..sessions {
        let sender = UdpSocket::bind(if to.is_ipv4() {
            "127.0.0.1:0"
        } else {
            "[::1]:0"
        })?;
        sender.set_read_timeout(Some(Duration::from_millis(100)))?;
        let probe = packet(128, session as u64, 2, 0, 0)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buffer = [0; 65536];
        loop {
            attempts += 1;
            sender.send_to(&probe, to)?;
            match sender.recv_from(&mut buffer) {
                Ok((n, peer)) if peer == to && buffer[..n] == probe => break,
                _ => {
                    if Instant::now() > deadline {
                        return Err("exercise setup deadline".into());
                    }
                }
            }
        }
        for (sequence, size) in [0, 1, 128, 512, 1200].into_iter().enumerate() {
            let payload = if size >= 40 {
                packet(size, session as u64, 2, sequence as u64 + 1, 0)?
            } else {
                vec![session as u8; size]
            };
            sender.send_to(&payload, to)?;
            let (n, peer) = sender.recv_from(&mut buffer)?;
            if peer != to || buffer[..n] != payload {
                return Err("exercise boundary/integrity mismatch".into());
            }
            delivered += 1;
        }
    }
    println!(
        "{}",
        serde_json::json!({"event":"exercise","sessions":sessions,"delivered_datagrams":delivered,"setup_attempts":attempts,"sizes":[0,1,128,512,1200],"capacity_claim":false})
    );
    Ok(())
}

#[cfg(test)]
mod pacing_tests {
    use super::*;
    #[test]
    fn completion_reserve_preserves_phase_counts_and_exact_cutoff() {
        for rate in [1, 3814, 213623, 1_000_000] {
            for seconds in [8, 15, 60, 300] {
                let warmup = 2;
                let first = warmup * rate;
                let after = (warmup + seconds) * rate;
                assert_eq!(
                    scheduled_offset(first, rate, warmup, seconds, 100, 20),
                    Duration::from_secs(warmup)
                );
                assert!(
                    scheduled_offset(first - 1, rate, warmup, seconds, 100, 20)
                        < Duration::from_secs(warmup)
                );
                assert!(
                    scheduled_offset(after - 1, rate, warmup, seconds, 100, 20)
                        < Duration::from_secs(warmup + seconds) - Duration::from_millis(20)
                );
                assert_eq!(
                    scheduled_offset(after, rate, warmup, seconds, 100, 20),
                    Duration::from_secs(warmup + seconds)
                );
                let mid = first + seconds * rate / 2;
                let scheduled = scheduled_offset(mid, rate, warmup, seconds, 100, 20);
                assert!(
                    scheduled >= Duration::from_secs(warmup)
                        && scheduled < Duration::from_secs(warmup + seconds)
                );
                assert_eq!(
                    scheduled_offset(mid, rate, warmup, seconds, 100, 0),
                    slot_offset(mid, rate, 100)
                );
            }
        }
    }
    #[test]
    fn receive_visit_yields_at_send_deadline_and_keeps_drain_available() {
        let now = Instant::now();
        let finish = now + Duration::from_millis(1);
        assert!(!receive_visit_allowed(now, now, finish, true));
        assert!(!receive_visit_allowed(
            now,
            now - Duration::from_micros(1),
            finish,
            true
        ));
        assert!(receive_visit_allowed(
            now,
            now + Duration::from_micros(1),
            finish,
            true
        ));
        assert!(receive_visit_allowed(finish, now, finish, true));
        assert!(receive_visit_allowed(now, finish, finish, true));
        assert!(receive_visit_allowed(now, now, finish, false));
    }
    #[test]
    fn micro_batches_keep_counts_boundaries_and_bounded_advance() {
        for rate in [1, 39062, 124511, 1_000_000] {
            let mut previous = Duration::ZERO;
            let mut ticks = [0, 1, rate - 1, rate, rate + 1, 2 * rate - 1, 2 * rate];
            ticks.sort_unstable();
            for tick in ticks {
                let exact = slot_offset(tick, rate, 0);
                let batched = slot_offset(tick, rate, 100);
                assert!(batched >= previous);
                assert!(batched <= exact && exact - batched < Duration::from_micros(100));
                assert_eq!(batched.as_nanos() % 100_000, 0);
                assert_eq!(batched >= Duration::from_secs(1), tick >= rate);
                assert_eq!(batched >= Duration::from_secs(2), tick >= 2 * rate);
                previous = batched;
            }
            if rate >= 10000 {
                assert_eq!(
                    slot_offset(2 * rate - 1, rate, 100),
                    Duration::from_secs(2) - Duration::from_micros(100)
                );
            }
        }
    }
    #[test]
    fn overdue_slots_are_not_delayed_by_an_idle_sleep() {
        let now = Instant::now();
        assert!(
            generator_pause(
                now,
                now - Duration::from_micros(1),
                now + Duration::from_secs(1),
                true
            )
            .is_zero()
        );
    }
    #[test]
    fn final_interval_guard_is_finite_and_does_not_extend_sending() {
        let now = Instant::now();
        let finish = now + Duration::from_micros(900);
        assert!(generator_pause(now, now + Duration::from_micros(100), finish, true).is_zero());
        assert_eq!(
            generator_pause(finish, now, finish, true),
            Duration::from_micros(50)
        );
        assert_eq!(
            generator_pause(now, finish, finish, true),
            Duration::from_micros(50)
        );
        assert_eq!(
            generator_pause(now, now, finish, false),
            Duration::from_micros(50)
        );
    }
    #[test]
    fn future_slots_outside_guard_keep_bounded_idle_wait() {
        let now = Instant::now();
        assert_eq!(
            generator_pause(
                now,
                now + Duration::from_millis(5),
                now + Duration::from_secs(1),
                true
            ),
            Duration::from_micros(50)
        );
    }
}
