use crate::common::quic::Pair;
use remora::{
    buffers::{BufferPool, Generation},
    transport::TransportError,
};
use std::time::{Duration, Instant};
#[test]
fn bbr_pacing_waits_before_generation_and_keeps_native_data_cancellable() {
    let mut p = Pair::with_transport_config(
        true,
        |f| {
            let path = f.file("client.toml");
            let mut text = std::fs::read_to_string(&path).unwrap();
            text.push_str("\n[transport]\ncc = \"bbr2_gcongestion\"\n[limits]\napplication_residence_ms = 200\n");
            std::fs::write(path, text).unwrap();
        },
        |config| {
            // This fixture must stop on pacing, not exhausted congestion
            // credit. With the default window a 14-packet burst can consume
            // all credit; then ACK/PTO, rather than a pacing slot, is the
            // valid next wakeup. Keep room for the entire offered batch.
            config.set_initial_congestion_window_packets(128);
            // The pinned gcongestion backend converts this setting with
            // Bandwidth::from_mbits_per_second (unlike legacy byte/s pacing).
            // Use 10 Mbit/s with a controlled RTT: this cap also limits CWND
            // to a multiple of bandwidth*RTT in the pinned backend.
            config.set_max_pacing_rate(10);
        },
    );
    p.one_way_delay = Duration::from_millis(50);
    let (stream, _) = p.requests();
    assert!(p.client.operational_stats().cwnd_total_bytes > 32 * 1452);
    let now = Instant::now();
    for _ in 0..32 {
        p.client
            .enqueue(stream, &[7; 1200], now, now, &mut [0; 1452])
            .unwrap();
    }
    let generated = p.client.generate(&p.pool, now, 32).unwrap();
    assert!(
        generated < 32,
        "slow-paced data must stay cancellable instead of precommitting a full FIFO"
    );
    assert!(
        !p.client
            .output
            .iter()
            .any(|packet| packet.at() > now + p.client.quic.max_release_into_future())
    );
    let release = p
        .client
        .quic
        .get_next_release_time()
        .unwrap()
        .time(now)
        .expect("pacer must retain a future slot");
    let remaining = p.client.quic.dgram_send_queue_len();
    assert!(remaining > 0);
    assert_eq!(p.client.generate(&p.pool, now, 32).unwrap(), 0);
    assert_eq!(p.client.quic.dgram_send_queue_len(), remaining);
    assert!(
        p.client.deadline(now).unwrap() <= release,
        "owner must wake for deferred generation: generated={generated}, output={}, release={release:?}, deadline={:?}, remaining={remaining}",
        p.client.output.len(),
        p.client.deadline(now),
    );
    assert_eq!(
        p.client
            .expire_native(now + Duration::from_millis(200))
            .unwrap(),
        remaining
    );
    assert_eq!(p.client.take_expired_datagrams(), remaining);
    assert_eq!(p.client.take_expired_datagrams(), 0);
}
#[test]
fn closing_one_stream_purges_only_cancellable_datagrams_and_keeps_committed_output() {
    let mut p = Pair::new(true);
    let (a, b) = p.requests();
    let now = Instant::now();
    let mut scratch = [0; 1350];
    p.client.enqueue(a, b"one", now, now, &mut scratch).unwrap();
    p.client.enqueue(b, b"two", now, now, &mut scratch).unwrap();
    assert_eq!(p.client.quic.dgram_send_queue_len(), 2);
    p.client.close_stream(a, 0x100);
    assert_eq!(p.client.quic.dgram_send_queue_len(), 1);
    p.client.generate(&p.pool, now, 8).unwrap();
    assert!(!p.client.output.is_empty());
    let before: Vec<_> = p
        .client
        .output
        .iter()
        .map(|o| {
            (
                o.bytes().to_vec(),
                o.sequence(),
                o.generated_at(),
                o.expires_at(),
                o.connection(),
            )
        })
        .collect();
    p.client.close_stream(b, 0x100);
    let after: Vec<_> = p
        .client
        .output
        .iter()
        .map(|o| {
            (
                o.bytes().to_vec(),
                o.sequence(),
                o.generated_at(),
                o.expires_at(),
                o.connection(),
            )
        })
        .collect();
    assert!(before == after);
    assert!(
        p.client
            .output
            .iter()
            .all(|o| o.connection() == Generation(1))
    );
    p.client.stop();
    assert!(p.client.output.is_empty());
}
#[test]
fn native_expiry_cipher_reservation_and_original_stall_deadline() {
    let mut p = Pair::new(true);
    let (a, b) = p.requests();
    let now = Instant::now();
    let mut scratch = [0; 1350];
    p.client.enqueue(a, b"a", now, now, &mut scratch).unwrap();
    p.client
        .enqueue(
            b,
            b"b",
            now + Duration::from_millis(10),
            now + Duration::from_millis(10),
            &mut scratch,
        )
        .unwrap();
    assert_eq!(
        p.client
            .expire_native(now + Duration::from_millis(20))
            .unwrap(),
        1
    );
    assert_eq!(p.client.quic.dgram_send_queue_len(), 1);
    let empty = BufferPool::new(1350, 1350).unwrap();
    let held = empty.acquire().unwrap();
    let count = p.client.quic.stats().sent;
    assert_eq!(p.client.generate(&empty, now, 1).unwrap(), 0);
    assert_eq!(p.client.quic.stats().sent, count);
    drop(held);
    p.client.enqueue(b, b"c", now, now, &mut scratch).unwrap();
    p.client.generate(&p.pool, now, 4).unwrap();
    let expiry = p.client.output.front().unwrap().expires_at();
    assert_eq!(
        p.client.generate(&p.pool, expiry, 1),
        Err(TransportError::Expired)
    );
}

#[test]
fn native_expiry_keeps_identical_younger_datagrams_and_advances_after_send_or_close() {
    for retire_old_by_send in [false, true] {
        let mut p = Pair::new(true);
        let (a, b) = p.requests();
        let now = Instant::now();
        let young = now + Duration::from_millis(10);
        let payload = [7; 1000];
        p.client
            .enqueue(a, &payload, now, now, &mut [0; 1452])
            .unwrap();
        p.client
            .enqueue(b, &payload, young, young, &mut [0; 1452])
            .unwrap();
        if retire_old_by_send {
            assert_eq!(p.client.generate(&p.pool, young, 1).unwrap(), 1);
        } else {
            p.client.close_stream(a, 0x100);
        }
        assert_eq!(p.client.quic.dgram_send_queue_len(), 1);
        assert_eq!(
            p.client
                .expire_native(now + Duration::from_millis(20))
                .unwrap(),
            0
        );
        assert_eq!(p.client.quic.dgram_send_queue_len(), 1);
        assert_eq!(
            p.client
                .expire_native(now + Duration::from_millis(30))
                .unwrap(),
            1
        );
        assert_eq!(p.client.quic.dgram_send_queue_len(), 0);
    }
    let mut p = Pair::new(true);
    let (a, _) = p.requests();
    let now = Instant::now();
    p.client
        .enqueue(a, b"same", now, now, &mut [0; 1452])
        .unwrap();
    let young = now + Duration::from_millis(10);
    p.client
        .enqueue(a, b"same", young, young, &mut [0; 1452])
        .unwrap();
    assert_eq!(
        p.client
            .expire_native(now + Duration::from_millis(20))
            .unwrap(),
        1
    );
    assert_eq!(p.client.quic.dgram_send_queue_len(), 1);
    assert_eq!(
        p.client
            .expire_native(now + Duration::from_millis(29))
            .unwrap(),
        0
    );
    assert_eq!(
        p.client
            .expire_native(now + Duration::from_millis(30))
            .unwrap(),
        1
    );
}

#[test]
fn incoming_native_epoch_is_not_refreshed_by_later_packets() {
    let mut p = Pair::new(true);
    let (a, _) = p.requests();
    let now = Instant::now();
    let mut scratch = [0; 1350];
    p.client.enqueue(a, b"old", now, now, &mut scratch).unwrap();
    p.pump();
    let first = p.server.incoming_expiry().unwrap();
    p.client.enqueue(a, b"new", now, now, &mut scratch).unwrap();
    p.pump();
    assert_eq!(p.server.incoming_expiry(), Some(first));
    while p.server.quic.dgram_recv(&mut scratch).is_ok() {}
    p.server.incoming_consumed();
    assert!(p.server.incoming_expiry().is_none());
    p.client
        .enqueue(a, b"fresh", now, now, &mut scratch)
        .unwrap();
    p.pump();
    assert!(p.server.incoming_expiry().unwrap() >= first);
}

#[test]
fn generation_expiry_is_retained_for_exactly_one_diagnostic_handoff_even_on_error() {
    for committed_output in [false, true] {
        let mut p = Pair::new(true);
        let (a, _) = p.requests();
        let now = Instant::now();
        if committed_output {
            p.client
                .enqueue(a, b"committed", now, now, &mut [0; 1452])
                .unwrap();
            p.client.generate(&p.pool, now, 1).unwrap();
            assert!(!p.client.output.is_empty());
        }
        p.client
            .enqueue(a, b"expires", now, now, &mut [0; 1452])
            .unwrap();
        let at = if committed_output {
            p.client.output.front().unwrap().expires_at()
        } else {
            now + Duration::from_millis(20)
        };
        let result = p.client.generate(&p.pool, at, 0);
        assert_eq!(
            result,
            if committed_output {
                Err(TransportError::Expired)
            } else {
                Ok(0)
            }
        );
        p.client.stop();
        assert_eq!(p.client.take_expired_datagrams(), 1);
        assert_eq!(p.client.take_expired_datagrams(), 0);
    }
}
