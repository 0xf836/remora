use crate::common::quic::Pair;
use remora::{
    buffers::{BufferPool, Generation},
    transport::TransportError,
};
use std::time::{Duration, Instant};
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
    assert_eq!(p.client.expire_native(now + Duration::from_millis(20)), 2);
    assert_eq!(p.client.quic.dgram_send_queue_len(), 0);
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
