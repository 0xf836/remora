#![no_main]
use libfuzzer_sys::fuzz_target;
use remora::{
    config::{RoleConfig, ValidatedConfig, canonical_host, normalize_ip},
    policy::*,
};
use std::{path::Path, sync::LazyLock};
static CONFIG: LazyLock<ValidatedConfig> = LazyLock::new(|| {
    ValidatedConfig::load(Path::new(
        &std::env::var("REMORA_FUZZ_CONFIG").expect("use scripts/validate/fuzz.sh"),
    ))
    .unwrap()
});
fuzz_target!(|data: &[u8]| {
    if data.len() > 4096 {
        return;
    }
    let RoleConfig::Server(server) = CONFIG.role() else {
        panic!("server fixture required");
    };
    let identity = &server.identities[0];
    assert!(authenticate(&[], data).is_err());
    let _ = authenticate(&server.identities, data);
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(host) = canonical_host(text) {
            assert_eq!(canonical_host(&host).unwrap(), host);
        }
        let rule = &identity.targets[0];
        let answer = text
            .parse()
            .unwrap_or_else(|_| "192.0.2.1:9999".parse().unwrap());
        if let Ok(approved) = authorize_answers(identity, &rule.host, rule.port, &[answer]) {
            assert_eq!(approved.port(), rule.port);
            assert!(
                rule.addresses
                    .iter()
                    .any(|range| range.contains(approved.ip()))
            );
            assert_eq!(normalize_ip(approved.ip()), normalize_ip(answer.ip()));
        }
        assert!(authorize_answers(identity, text, 0, &[answer]).is_err());
        assert!(
            authorize_answers(
                identity,
                &rule.host,
                rule.port,
                &[answer, "192.0.2.1:1".parse().unwrap()]
            )
            .is_err()
        );
    }
});
