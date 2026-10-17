#![no_main]
use libfuzzer_sys::fuzz_target;
use remora::udp::{decode_control, decode_error_control, receive_fault};
fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    for local in ["127.0.0.1:9999", "[::1]:9999"] {
        let address = local.parse().unwrap();
        let (_, _, malformed) = decode_control(data, address);
        let _ = decode_error_control(data, address);
        let flags = data.first().map_or(0, |v| i32::from(*v));
        let _ = receive_fault(flags, malformed, true);
    }
});
