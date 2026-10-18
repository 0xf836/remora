use proptest::prelude::*;
use remora::masque::*;
fn capsule(ty: u64, body: &[u8]) -> Vec<u8> {
    let mut out = vec![0; body.len() + 16];
    let n = put_varint(ty, &mut out).unwrap();
    let m = put_varint(body.len() as u64, &mut out[n..]).unwrap();
    out[n + m..n + m + body.len()].copy_from_slice(body);
    out.truncate(n + m + body.len());
    out
}
#[test]
fn every_split_and_valid_skip_resynchronization() {
    let mut bytes = capsule(999, &[1; 20]);
    bytes.extend(capsule(0, &[0, 1, 2, 3]));
    bytes.extend(capsule(0, &[7, 1, 2]));
    for split in 0..=bytes.len() {
        let mut p = CapsuleParser::default();
        p.feed(&bytes[..split], false).unwrap();
        let c = p.feed(&bytes[split..], true).unwrap();
        assert_eq!(c.unsupported, 1);
        assert_eq!(c.unknown_context, 1);
        assert!(p.retained_bytes() <= 8);
    }
}
#[test]
fn syntax_and_semantic_errors_precede_carrier_discard() {
    for bytes in [
        vec![0, 0],
        vec![0, 1, 0xc0],
        vec![0, 2, 0x40],
        capsule(0, &[0, 9])[..3].to_vec(),
    ] {
        assert_eq!(
            CapsuleParser::default().feed(&bytes, true),
            Err(WireError::Message)
        );
    }
    for width in [1, 2, 4, 8] {
        let mut prefix = vec![0; 16];
        let n = put_varint(0, &mut prefix).unwrap();
        let m = put_varint(65528 + width as u64, &mut prefix[n..]).unwrap();
        prefix.truncate(n + m);
        let mut context = vec![0; width];
        context[0] = match width {
            1 => 0,
            2 => 0x40,
            4 => 0x80,
            _ => 0xc0,
        };
        prefix.extend(context);
        assert_eq!(
            CapsuleParser::default().feed(&prefix, false),
            Err(WireError::Datagram)
        );
    }
    let bytes = capsule(0, &vec![0; 65528]);
    assert_eq!(
        CapsuleParser::default()
            .feed(&bytes, true)
            .unwrap()
            .unsupported,
        1
    );
    let mut huge = [0; 16];
    let n = put_varint(123, &mut huge).unwrap();
    let m = put_varint(MAX_VARINT, &mut huge[n..]).unwrap();
    let mut p = CapsuleParser::default();
    p.feed(&huge[..n + m], false).unwrap();
    assert_eq!(p.retained_bytes(), 0);
    assert_eq!(p.feed(&[], true), Err(WireError::Message));
}
proptest! {#[test]fn arbitrary_fragmented_input_is_bounded(bytes in proptest::collection::vec(any::<u8>(),0..4096),chunk in 1usize..32){let mut p=CapsuleParser::default();for part in bytes.chunks(chunk){if p.feed(part,false).is_err(){break;}prop_assert!(p.retained_bytes()<=8);}let _=p.feed(&[],true);}}
