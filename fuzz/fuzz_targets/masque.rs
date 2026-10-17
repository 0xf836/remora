#![no_main]
use libfuzzer_sys::fuzz_target;
use quiche::h3::Header;
use remora::masque::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    let _ = get_varint(data);
    if let Ok(Some(d)) = decode_datagram(data) {
        assert!(d.stream % 4 == 0);
    }
    let mut p = CapsuleParser::default();
    let chunk = data.first().map_or(1, |n| usize::from(*n) + 1);
    for bytes in data.chunks(chunk) {
        let _ = p.feed(bytes, false);
        assert!(p.retained_bytes() <= 8);
    }
    let _ = p.feed(&[], true);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = parse_target(text, "/.well-known/masque/udp/");
        let _ = UriTemplate::parse(text, "remora.test:443");
    }
    let headers: Vec<_> = data
        .split(|b| *b == 0)
        .take(33)
        .map(|v| {
            let split = v.iter().position(|b| *b == b'=').unwrap_or(v.len());
            Header::new(&v[..split], v.get(split + 1..).unwrap_or_default())
        })
        .collect();
    let _ = request(&headers, "remora.test:443", "/.well-known/masque/udp/");
    let _ = response(&headers, data.first().is_some_and(|b| b & 1 != 0));
});
