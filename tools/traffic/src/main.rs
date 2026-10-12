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
        "--size",
        "--session",
        "--start-ms",
        "--drain-ms",
        "--sessions",
        "--batch-max",
        "--application-pool-bytes",
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
    let size = number(&a, "--size", 128)? as usize;
    let session = number(&a, "--session", 1)?;
    let drain = number(&a, "--drain-ms", 200)?;
    if seconds == 0
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
    while Instant::now() < end {
        let now = Instant::now();
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
                if Instant::now() < next_send || next_send >= finish {
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
                send_ticks += 1;
                next_send = warm_start + Duration::from_nanos(send_ticks * 1_000_000_000 / rate);
            }
        }
        for _ in 0..32 {
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
        std::thread::sleep(Duration::from_micros(50));
    }
    let rate = if sender { pps } else { reverse };
    let expected = rate * seconds * if sender { 1 } else { peers.len() as u64 };
    let generator_unscheduled = expected.saturating_sub(offered);
    let p95 = measurement::percentile(&mut rtt.clone(), 95);
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"role":cmd,"size":size,"sessions":if sender{1}else{peers.len()},"interval_seconds":seconds,"drain_ms":drain,"warmup_seconds":warmup,"warmup_offered":warm_offered,"warmup_failures":warm_failures,"offered":offered,"submitted":sent,"enqueue_failures":enqueue_failures,"generator_unscheduled":generator_unscheduled,"generator_excess":offered.saturating_sub(expected),"reverse_received":reverse_collection,"forward_received":collected,"rtt_samples_us":rtt,"rtt_p95_us":p95,"start_ms":start_ms,"elapsed_ms":base.elapsed().as_millis()})
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
