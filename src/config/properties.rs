//! Bounded parser properties run as normal CI tests, without files or networking.
use super::*;
use proptest::{prelude::*, test_runner::RngSeed};
use std::net::{Ipv4Addr, Ipv6Addr};

fn credential_text(range: &str) -> String {
    format!(
        "[[identities]]\nid='property'\ntoken_sha256='{}'\n\
         [[identities.targets]]\nhost='destination.test'\nport=1234\naddresses=['{range}']\n",
        "0".repeat(64)
    )
}

fn parse_range(range: &str) -> AddressRange {
    let identities = parse_credentials(credential_text(range).as_bytes()).unwrap();
    identities[0].targets[0].addresses[0]
}

fn bounded_text() -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<char>(), 0..512).prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 128,
        // A fixed seed makes ordinary native/container runs reproducible;
        // failures still shrink and retain their normal regression seed.
        rng_seed: RngSeed::Fixed(0x52454d4f524132),
        .. ProptestConfig::default()
    })]

    #[test]
    fn arbitrary_document_bytes_are_panic_free_with_fixed_errors(
        bytes in proptest::collection::vec(any::<u8>(), 0..4096),
        text in bounded_text(),
    ) {
        for input in [&bytes[..], text.as_bytes()] {
            match parse_document(input) {
                Ok(doc) => prop_assert!(doc.schema_version == 1),
                Err(err) => {
                    prop_assert_eq!(err.reason, ConfigReason::Schema);
                    prop_assert!(["config", "schema_version"].contains(&err.field()));
                    prop_assert!(format!("{err} {err:?}").len() < 384, "parser property failed");
                }
            }
        }
    }

    #[test]
    fn schema_versions_and_unknown_secret_fields_fail_closed(version in any::<u32>(), secret in "[a-zA-Z0-9]{0,64}") {
        let input = format!("schema_version={version}\n");
        prop_assert_eq!(parse_document(input.as_bytes()).is_ok(), version == 1);
        let input = format!("schema_version=1\nsecret='SECRET-SENTINEL-{secret}'\n");
        let err = parse_document(input.as_bytes()).err().unwrap();
        prop_assert_eq!(err.field(), "config");
        prop_assert!(!format!("{err} {err:?}").contains("SECRET-SENTINEL"), "parser property failed");
    }

    #[test]
    fn arbitrary_credentials_are_panic_free_bounded_and_privacy_safe(
        bytes in proptest::collection::vec(any::<u8>(), 0..4096),
        text in bounded_text(),
    ) {
        for input in [&bytes[..], text.as_bytes()] {
            match parse_credentials(input) {
                Ok(identities) => {
                    prop_assert!(!identities.is_empty() && identities.len() <= 128);
                    for identity in identities {
                        prop_assert!(identity.id.len() <= 64);
                        prop_assert!(!identity.targets.is_empty() && identity.targets.len() <= 128);
                        for rule in identity.targets {
                            prop_assert!(!rule.addresses.is_empty() && rule.addresses.len() <= 128);
                        }
                    }
                }
                Err(err) => {
                    prop_assert!(["credentials_file", "host", "target_host", "target_port"].contains(&err.field()));
                    prop_assert!(matches!(err.reason, ConfigReason::Format | ConfigReason::Range));
                    prop_assert!(format!("{err} {err:?}").len() < 384, "parser property failed");
                }
            }
        }
        let secret = format!("SECRET-SENTINEL-{}", bytes.iter().take(32).map(|b| format!("{b:02x}")).collect::<String>());
        let malformed = format!("identities='{secret}'");
        let err = parse_credentials(malformed.as_bytes()).err().unwrap();
        prop_assert_eq!(err, error("credentials_file", ConfigReason::Format));
        prop_assert!(!format!("{err} {err:?}").contains("SECRET-SENTINEL"), "parser property failed");
    }

    #[test]
    fn host_and_authority_arbitrary_text_have_fixed_errors_and_stable_normalization(input in bounded_text()) {
        match canonical_host(&input) {
            Ok(host) => {
                prop_assert!(host.is_ascii() && host.len() <= 253);
                prop_assert_eq!(canonical_host(&host).unwrap(), host);
            }
            Err(err) => prop_assert_eq!(err, error("host", ConfigReason::Format)),
        }
        match endpoint(&input, "proxy") {
            Ok((host, port)) => {
                prop_assert!(port > 0);
                prop_assert_eq!(canonical_host(&host).unwrap(), host);
            }
            Err(err) => {
                prop_assert!(["proxy"].contains(&err.field()));
                prop_assert_eq!(err.reason, ConfigReason::Format);
            }
        }
    }

    #[test]
    fn arbitrary_proxy_origins_have_bounded_privacy_safe_errors(input in bounded_text()) {
        match proxy_origin(&input, "client.proxy") {
            Ok((host, port)) => {
                prop_assert_eq!(canonical_host(&host).unwrap(), host);
                prop_assert!(port > 0);
            }
            Err(err) => {
                prop_assert_eq!(err.field(), "client.proxy");
                prop_assert_eq!(err.reason, ConfigReason::Format);
                prop_assert!(format!("{err} {err:?}").len() < 384, "proxy parser error must be bounded");
            }
        }
    }

    #[test]
    fn dns_case_trailing_dot_and_authority_port_preserve_identity(
        labels in proptest::collection::vec("[a-z][a-z0-9]{0,15}", 2..5),
        port in 1u16..=u16::MAX,
    ) {
        let dns = labels.join(".");
        let input = format!("{}.", dns.to_ascii_uppercase());
        prop_assert_eq!(canonical_host(&input).unwrap(), dns.clone());
        prop_assert_eq!(endpoint(&format!("{input}:{port}"), "proxy").unwrap(), (dns.clone(), port));
        prop_assert!(endpoint(&input, "proxy").is_err());
        for forbidden in [format!("{dns}%eth0"), format!("*.{dns}"), format!("{dns}.é"), format!("{dns} "), format!("{dns}\0")] {
            prop_assert!(canonical_host(&forbidden).is_err());
        }
    }

    #[test]
    fn ipv4_cidr_containment_and_mapped_ipv6_normalize_equally(
        net in any::<u32>(), candidate in any::<u32>(), prefix in 0u8..=32,
    ) {
        let net = Ipv4Addr::from(net);
        let ip = Ipv4Addr::from(candidate);
        let range = parse_range(&format!("{net}/{prefix}"));
        let mapped_range = parse_range(&format!("{}/{}", net.to_ipv6_mapped(), prefix + 96));
        prop_assert_eq!(range, mapped_range);
        let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
        let expected = (u32::from(net) & mask) == (candidate & mask);
        prop_assert_eq!(range.contains(IpAddr::V4(ip)), expected);
        prop_assert_eq!(range.contains(IpAddr::V6(ip.to_ipv6_mapped())), expected);
        prop_assert!(range.contains(IpAddr::V4(net)));
        prop_assert!(!range.contains(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        prop_assert_eq!(canonical_host(&ip.to_ipv6_mapped().to_string()).unwrap(), ip.to_string());
        prop_assert!(parse_credentials(credential_text(&format!("{net}/33")).as_bytes()).is_err(), "parser property failed");
        prop_assert!(parse_credentials(credential_text(&format!("{}/95", net.to_ipv6_mapped())).as_bytes()).is_err(), "parser property failed");
    }

    #[test]
    fn ipv6_cidr_containment_matches_prefix_and_rejects_other_family(
        net in any::<u128>(), candidate in any::<u128>(), prefix in 0u8..=128,
    ) {
        // Set the top bit to exercise ordinary IPv6 separately from mapped IPv4.
        let net = net | (1u128 << 127);
        let candidate = candidate | (1u128 << 127);
        let address = Ipv6Addr::from(net);
        let range = parse_range(&format!("{address}/{prefix}"));
        let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
        prop_assert_eq!(range.contains(IpAddr::V6(Ipv6Addr::from(candidate))), (net & mask) == (candidate & mask));
        prop_assert!(range.contains(IpAddr::V6(address)));
        prop_assert!(!range.contains(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        prop_assert!(parse_credentials(credential_text(&format!("{address}/129")).as_bytes()).is_err(), "parser property failed");
    }

    #[test]
    fn arbitrary_cidr_text_inside_valid_credentials_is_bounded(input in bounded_text()) {
        let encoded = serde_json::to_string(&input).unwrap();
        let doc = credential_text("127.0.0.1/32").replace("'127.0.0.1/32'", &encoded);
        match parse_credentials(doc.as_bytes()) {
            Ok(identities) => {
                let range = identities[0].targets[0].addresses[0];
                let max_prefix = if range.address().is_ipv4() { 32 } else { 128 };
                prop_assert!(range.prefix() <= max_prefix);
                prop_assert!(range.contains(range.address()));
            }
            Err(err) => prop_assert_eq!(err, error("credentials_file", ConfigReason::Format)),
        }
    }
}
