use crate::common::{ChildGuard, Fixtures};
use remora::config::{ConfigReason, Offload, RoleConfig, ValidatedConfig, canonical_host};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    process::Command,
    time::Duration,
};

fn invalid(f: &Fixtures, file: &str) -> remora::config::ConfigError {
    ValidatedConfig::load(&f.file(file))
        .err()
        .expect("configuration must fail closed")
}
#[test]
fn socket_buffer_defaults_and_explicit_legacy_values_are_preserved() {
    let f = Fixtures::new();
    for file in ["client.toml", "server.toml"] {
        let cfg = ValidatedConfig::load(&f.file(file)).unwrap();
        assert_eq!(cfg.io().relay_socket_buffer_bytes, 131072);
        assert_eq!(cfg.io().outer_socket_buffer_bytes, 1048576);
        let text = fs::read_to_string(f.file(file)).unwrap();
        fs::write(
            f.file(file),
            text + "\n[io]\nrelay_socket_buffer_bytes = 65536\nouter_socket_buffer_bytes = 262144\n",
        )
        .unwrap();
        let legacy = ValidatedConfig::load(&f.file(file)).unwrap();
        assert_eq!(legacy.io().relay_socket_buffer_bytes, 65536);
        assert_eq!(legacy.io().outer_socket_buffer_bytes, 262144);
    }
}
#[test]
fn generated_material_is_private_valid_and_hashes_canonical_ascii() {
    let f = Fixtures::new();
    assert_eq!(fs::metadata(f.path()).unwrap().mode() & 0o777, 0o700);
    for path in ["client.token", "server-key.pem", "credentials.toml"] {
        assert_eq!(fs::metadata(f.file(path)).unwrap().mode() & 0o777, 0o600);
    }
    let client = ValidatedConfig::load(&f.file("client.toml")).unwrap();
    let server = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    let RoleConfig::Client(c) = client.role() else {
        panic!("client role")
    };
    let RoleConfig::Server(s) = server.role() else {
        panic!("server role")
    };
    assert_eq!(
        s.identities[0].token_sha256.as_slice(),
        Sha256::digest(c.token.canonical_text()).as_slice()
    );
    assert_eq!(c.target_port, s.identities[0].targets[0].port);
    assert!(s.identities[0].targets[0].addresses[0].contains("127.0.0.1".parse().unwrap()));
    assert!(!s.identities[0].targets[0].addresses[0].contains("127.0.0.2".parse().unwrap()));
    let ports: serde_json::Value =
        serde_json::from_slice(&fs::read(f.file("ports.json")).unwrap()).unwrap();
    let mut seen = std::collections::HashSet::new();
    for port in ports.as_object().unwrap().values() {
        let port = port.as_u64().unwrap();
        assert!(port >= 1024 && seen.insert(port));
    }
    let mut verify = ChildGuard::spawn(
        Command::new("openssl")
            .arg("verify")
            .arg("-CAfile")
            .arg(f.file("ca.pem"))
            .arg("-verify_hostname")
            .arg("remora.test")
            .arg(f.file("server.pem")),
    )
    .unwrap();
    assert!(verify.finish(Duration::from_secs(5)).unwrap().success());
}
#[test]
fn schema_roles_and_unknown_fields_are_rejected_without_echoing_input() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("client.toml")).unwrap();
    for text in [
        original.replace("schema_version = 1", "schema_version = 2"),
        original.replace(
            "schema_version = 1",
            "schema_version = 1\nsecret_sentinel = 'SECRET-SENTINEL'",
        ),
        original.replace("[client]", "[client]\nsecret_sentinel = 'SECRET-SENTINEL'"),
        "schema_version = 1".into(),
        format!(
            "{original}\n{}",
            fs::read_to_string(f.file("server.toml"))
                .unwrap()
                .replace("schema_version = 1", "")
        ),
        "schema_version = 'SECRET-SENTINEL'".into(),
    ] {
        fs::write(f.file("client.toml"), text).unwrap();
        let err = invalid(&f, "client.toml");
        assert!(!format!("{err} {err:?}").contains("SECRET-SENTINEL"));
    }
    for table in ["limits", "io", "transport", "recovery", "diagnostics"] {
        fs::write(
            f.file("client.toml"),
            format!("{original}\n[{table}]\nunknown = 1\n"),
        )
        .unwrap();
        assert_eq!(invalid(&f, "client.toml").reason, ConfigReason::Schema);
    }
}
#[test]
fn secret_permissions_regular_files_and_symlinks_are_checked_at_open() {
    let f = Fixtures::new();
    for file in ["client.token", "server-key.pem", "credentials.toml"] {
        let config = if file == "client.token" {
            "client.toml"
        } else {
            "server.toml"
        };
        fs::set_permissions(f.file(file), fs::Permissions::from_mode(0o640)).unwrap();
        assert_eq!(invalid(&f, config).reason, ConfigReason::Permissions);
        fs::set_permissions(f.file(file), fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(f.file(file), f.file("saved")).unwrap();
        symlink("saved", f.file(file)).unwrap();
        assert_eq!(invalid(&f, config).reason, ConfigReason::File);
        fs::remove_file(f.file(file)).unwrap();
        fs::rename(f.file("saved"), f.file(file)).unwrap();
    }
    fs::rename(f.file("client.token"), f.file("saved")).unwrap();
    fs::create_dir(f.file("client.token")).unwrap();
    assert_eq!(invalid(&f, "client.toml").reason, ConfigReason::File);
    fs::remove_dir(f.file("client.token")).unwrap();
    let fifo = f.file("client.token");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR,
        0,
    )
    .unwrap();
    let before = std::time::Instant::now();
    assert_eq!(invalid(&f, "client.toml").reason, ConfigReason::File);
    assert!(before.elapsed() < Duration::from_secs(1));
}
#[test]
fn intermediate_directory_and_config_symlinks_cannot_substitute_material() {
    let f = Fixtures::new();
    symlink(f.path(), f.file("alias")).unwrap();
    assert_eq!(
        ValidatedConfig::load(&f.file("alias/client.toml"))
            .err()
            .unwrap()
            .reason,
        ConfigReason::File
    );
    symlink("client.toml", f.file("alias.toml")).unwrap();
    assert_eq!(invalid(&f, "alias.toml").reason, ConfigReason::File);
    fs::create_dir(f.file("nested")).unwrap();
    fs::copy(f.file("client.toml"), f.file("nested/client.toml")).unwrap();
    for (from, to) in [("ca.pem", "../ca.pem"), ("client.token", "../client.token")] {
        f.rewrite("nested/client.toml", from, to);
    }
    ValidatedConfig::load(&f.file("nested/client.toml")).unwrap();
}
#[test]
fn token_accepts_only_lowercase_canonical_text_and_one_optional_lf() {
    let f = Fixtures::new();
    for good in ["a".repeat(64), format!("{}\n", "a".repeat(64))] {
        fs::write(f.file("client.token"), good).unwrap();
        ValidatedConfig::load(&f.file("client.toml")).unwrap();
    }
    for bad in [
        "A".repeat(64),
        "g".repeat(64),
        "a".repeat(63),
        format!("{}\n\n", "a".repeat(64)),
        format!("{}\r\n", "a".repeat(64)),
        format!(" {}", "a".repeat(64)),
        "SECRET-SENTINEL".into(),
    ] {
        fs::write(f.file("client.token"), bad).unwrap();
        assert!(ValidatedConfig::load(&f.file("client.toml")).is_err());
    }
}
#[test]
fn invalid_pem_and_mismatched_private_key_fail_without_secret_text() {
    let f = Fixtures::new();
    let saved = fs::read(f.file("server-key.pem")).unwrap();
    fs::write(
        f.file("server-key.pem"),
        fs::read(f.file("ca-key.pem")).unwrap(),
    )
    .unwrap();
    assert_eq!(invalid(&f, "server.toml").field(), "private_key_file");
    fs::write(f.file("server-key.pem"), saved).unwrap();
    for (file, config) in [
        ("server.pem", "server.toml"),
        ("ca.pem", "client.toml"),
        ("server-key.pem", "server.toml"),
    ] {
        let original = fs::read(f.file(file)).unwrap();
        fs::write(f.file(file), "SECRET-SENTINEL invalid PEM").unwrap();
        let error = invalid(&f, config);
        assert_eq!(error.reason, ConfigReason::Format);
        assert!(!format!("{error} {error:?}").contains("SECRET-SENTINEL"));
        fs::write(f.file(file), original).unwrap();
    }
}
#[test]
fn exposure_and_proxy_identity_validation_fail_closed() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("client.toml")).unwrap();
    fs::write(
        f.file("client.toml"),
        original.replacen("127.0.0.1", "0.0.0.0", 1),
    )
    .unwrap();
    assert_eq!(invalid(&f, "client.toml").reason, ConfigReason::Range);
    f.rewrite(
        "client.toml",
        "allow_non_loopback = false",
        "allow_non_loopback = true",
    );
    ValidatedConfig::load(&f.file("client.toml")).unwrap();
    for text in [
        original.replace("proxy = ", "proxy = 'http://invalid' #"),
        original.replace(
            "[client]",
            "[client]\nuri_template = 'http://remora.test/{target_host}/{target_port}/'",
        ),
        original.replace(
            "[client]",
            "[client]\nuri_template = 'https://wrong.test:443/{target_host}/{unknown}/'",
        ),
        original.replace("target = \"127.0.0.1:", "target = \"224.0.0.1:"),
    ] {
        fs::write(f.file("client.toml"), text).unwrap();
        invalid(&f, "client.toml");
    }
}
#[test]
fn defaults_offloads_and_checked_numeric_fd_budgets() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("client.toml")).unwrap();
    let config = ValidatedConfig::load(&f.file("client.toml")).unwrap();
    assert_eq!(config.limits().application_pool_bytes, 8 * 1024 * 1024);
    assert_eq!(config.limits().ciphertext_pool_bytes, 4 * 1024 * 1024);
    assert_eq!(config.limits().scratch_pool_bytes, 2 * 1024 * 1024);
    assert_eq!(
        ValidatedConfig::load_with_fd_limit(&f.file("client.toml"), 16)
            .err()
            .unwrap()
            .reason,
        ConfigReason::FdBudget
    );
    for section in ["[io]\ngso='auto'\ngro='auto'", "[io]\ngso='off'\ngro='off'"] {
        fs::write(f.file("client.toml"), format!("{original}\n{section}\n")).unwrap();
        let c = ValidatedConfig::load(&f.file("client.toml")).unwrap();
        assert_eq!(c.io().gso, c.io().gro);
        assert!(matches!(c.io().gso, Offload::Off | Offload::Auto));
    }
    for invalid_section in [
        "[io]\nbatch_max=0",
        "[io]\nbatch_max=65",
        "[io]\ngso='on'",
        "[transport]\ncc='bbr'",
        "[transport]\npacing='off'",
        "[transport]\nouter_udp_max_bytes=1199",
        "[limits]\nmax_connections=0",
        "[recovery]\nretry_initial_ms=99",
        "[recovery]\nretry_max_ms=2001",
        "[limits]\nmax_sessions=16",
        "[limits]\nconnection_dgram_bytes=1024",
        "[limits]\nquiche_dgram_packets=9223372036854775807",
        "[limits]\napplication_pool_bytes=9223372036854775807",
        "[limits]\nidle_session_ms=18446744073709551615",
        "[recovery]\nretry_initial_ms=99",
        "[recovery]\nretry_max_ms=99",
        "[diagnostics]\nsnapshot_interval_ms=999",
        "[limits]\nmax_connections=-1",
    ] {
        fs::write(
            f.file("client.toml"),
            format!("{original}\n{invalid_section}\n"),
        )
        .unwrap();
        invalid(&f, "client.toml");
    }
}
#[test]
fn credentials_reject_duplicates_wildcards_and_numeric_permission_mismatch() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("credentials.toml")).unwrap();
    for text in [
        format!("{original}\n{original}"),
        original.replace("127.0.0.1/32", "127.0.0.2/32"),
        original.replace("127.0.0.1/32", "127.0.0.1/33"),
        original.replace("host = \"127.0.0.1\"", "host = \"*\""),
        original.replace("id = \"fixture\"", "id = \"fixture\"\nunknown = true"),
        "identities = []".into(),
    ] {
        fs::write(f.file("credentials.toml"), text).unwrap();
        invalid(&f, "server.toml");
    }
}
#[test]
fn canonical_hosts_reject_ambiguous_numeric_and_scoped_non_ascii_forms() {
    assert_eq!(canonical_host("Remora.Test.").unwrap(), "remora.test");
    assert_eq!(canonical_host("::ffff:127.0.0.1").unwrap(), "127.0.0.1");
    for bad in [
        "127.1",
        "0177.0.0.1",
        "2130706433",
        "0x7f000001",
        "0x7f.0.0.1",
        "127.0.0.0x1",
        "fe80::1%eth0",
        "rémora.test",
        "*.test",
        "bad..test",
    ] {
        assert!(canonical_host(bad).is_err(), "{bad}");
    }
}
#[test]
fn fixture_generator_refuses_existing_output_without_modifying_it() {
    let f = Fixtures::new();
    let sentinel = fs::read(f.file("client.token")).unwrap();
    let mut child = ChildGuard::spawn(
        Command::new("bash")
            .arg("scripts/validate/fixtures.sh")
            .arg("--output")
            .arg(f.path()),
    )
    .unwrap();
    assert_eq!(
        child.finish(Duration::from_secs(5)).unwrap().code(),
        Some(2)
    );
    assert_eq!(fs::read(f.file("client.token")).unwrap(), sentinel);
}

#[test]
fn encrypted_private_key_fails_promptly_without_password_input() {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let f = Fixtures::new();
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(f.file("passphrase"))
        .unwrap()
        .write_all(b"fixture-only-SECRET-SENTINEL\n")
        .unwrap();
    let mut child = ChildGuard::spawn(
        Command::new("openssl")
            .arg("pkey")
            .arg("-in")
            .arg(f.file("server-key.pem"))
            .arg("-aes-256-cbc")
            .arg("-passout")
            .arg(format!("file:{}", f.file("passphrase").display()))
            .arg("-out")
            .arg(f.file("encrypted-key.pem")),
    )
    .unwrap();
    assert!(child.finish(Duration::from_secs(5)).unwrap().success());
    f.rewrite("server.toml", "server-key.pem", "encrypted-key.pem");
    let before = std::time::Instant::now();
    let error = invalid(&f, "server.toml");
    assert_eq!(error.field(), "private_key_file");
    assert_eq!(error.reason, ConfigReason::Format);
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(!format!("{error} {error:?}").contains("SECRET-SENTINEL"));

    let key =
        boring::pkey::PKey::private_key_from_pem(&fs::read(f.file("server-key.pem")).unwrap())
            .unwrap();
    let empty_password_key = key
        .private_key_to_pem_pkcs8_passphrase(boring::symm::Cipher::aes_256_cbc(), b"")
        .unwrap();
    // Establish the regression trigger: the backend accepts a zero-length
    // password callback for this material; the configuration loader must not.
    assert!(
        boring::pkey::PKey::private_key_from_pem_callback(&empty_password_key, |_| Ok(0)).is_ok()
    );
    fs::write(f.file("encrypted-key.pem"), empty_password_key).unwrap();
    assert_eq!(invalid(&f, "server.toml").reason, ConfigReason::Format);
}

#[test]
fn bounded_input_and_credential_identity_limits_fail_closed() {
    let f = Fixtures::new();
    fs::write(f.file("huge.toml"), vec![b'x'; 256 * 1024 + 1]).unwrap();
    assert_eq!(invalid(&f, "huge.toml").reason, ConfigReason::File);
    let original = fs::read_to_string(f.file("credentials.toml")).unwrap();
    for duplicate in [
        original.replace("id = \"fixture\"", "id = \"different\""),
        original
            .lines()
            .map(|line| {
                if line.starts_with("token_sha256") {
                    format!("token_sha256 = \"{}\"", "0".repeat(64))
                } else {
                    line.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    ] {
        fs::write(
            f.file("credentials.toml"),
            format!("{original}\n{duplicate}\n"),
        )
        .unwrap();
        invalid(&f, "server.toml");
    }
    let many: String = (0..129)
        .map(|n| {
            original
                .lines()
                .map(|line| {
                    if line.starts_with("id = ") {
                        format!("id = \"identity{n}\"")
                    } else if line.starts_with("token_sha256") {
                        format!("token_sha256 = \"{n:064x}\"")
                    } else {
                        line.into()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        })
        .collect();
    fs::write(f.file("credentials.toml"), many).unwrap();
    invalid(&f, "server.toml");
    let (identity, target) = original.split_once("[[identities.targets]]").unwrap();
    fs::write(
        f.file("credentials.toml"),
        format!(
            "{identity}{}",
            format!("[[identities.targets]]{target}").repeat(129)
        ),
    )
    .unwrap();
    invalid(&f, "server.toml");
}

#[test]
fn configuration_derives_identity_route_and_target_without_dns_at_validation() {
    let f = Fixtures::new();
    let client = "schema_version=1\n[client]\nlisten='127.0.0.1:51821'\nproxy='https://Proxy.Invalid.'\ntarget='[::1]:51820'\nca_file='ca.pem'\ntoken_file='client.token'\n";
    fs::write(f.file("endpoint.toml"), client).unwrap();
    let cfg = ValidatedConfig::load(&f.file("endpoint.toml")).unwrap();
    let RoleConfig::Client(c) = cfg.role() else {
        panic!()
    };
    assert_eq!(c.connect_address, None);
    assert_eq!(c.server_name, "proxy.invalid");
    assert_eq!(c.proxy_authority, "proxy.invalid");
    assert_eq!(
        c.uri_template,
        "https://proxy.invalid/.well-known/masque/udp/{target_host}/{target_port}/"
    );
    assert_eq!((c.target_host.as_str(), c.target_port), ("::1", 51820));
    for (from, to, field) in [
        (
            "proxy='https://Proxy.Invalid.'",
            "proxy='http://proxy.invalid'",
            "client.proxy",
        ),
        (
            "proxy='https://Proxy.Invalid.'",
            "proxy='proxy.invalid'",
            "client.proxy",
        ),
        (
            "proxy='https://Proxy.Invalid.'",
            "proxy='https://proxy.invalid:0'",
            "client.proxy",
        ),
        (
            "target='[::1]:51820'",
            "target='::1:51820'",
            "client.target",
        ),
        (
            "target='[::1]:51820'",
            "target='224.0.0.1:51820'",
            "client.target",
        ),
        ("[client]", "[client]\nunknown='SECRET-SENTINEL'", "client"),
    ] {
        fs::write(f.file("endpoint.toml"), client.replace(from, to)).unwrap();
        assert_eq!(invalid(&f, "endpoint.toml").field(), field);
    }
    fs::write(
        f.file("endpoint.toml"),
        client.replace("[client]", "[client]\nconnect_address='127.0.0.1:7443'"),
    )
    .unwrap();
    let cfg = ValidatedConfig::load(&f.file("endpoint.toml")).unwrap();
    let RoleConfig::Client(c) = cfg.role() else {
        panic!()
    };
    assert_eq!(c.connect_address, Some("127.0.0.1:7443".parse().unwrap()));
    assert_eq!(c.server_name, "proxy.invalid");
    let server = "schema_version=1\n[server]\nlisten=['127.0.0.1:7443']\nproxy='https://Proxy.Invalid.'\ncertificate_file='server.pem'\nprivate_key_file='server-key.pem'\ncredentials_file='credentials.toml'\n";
    fs::write(f.file("endpoint.toml"), server).unwrap();
    let cfg = ValidatedConfig::load(&f.file("endpoint.toml")).unwrap();
    let RoleConfig::Server(s) = cfg.role() else {
        panic!()
    };
    assert_eq!(s.proxy_authority, "proxy.invalid");
    assert_eq!(s.path_prefix, "/.well-known/masque/udp/");
    assert_eq!(s.listen[0].port(), 7443);
}

#[test]
fn prohibited_target_addresses_report_the_public_field_without_echoing_values() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("client.toml")).unwrap();
    for target in [
        "0.0.0.0:51820",
        "[::]:51820",
        "224.0.0.1:51820",
        "[ff02::1]:51820",
        "255.255.255.255:51820",
        "[fe80::1]:51820",
    ] {
        let mut document: toml::Table = toml::from_str(&original).unwrap();
        document["client"]
            .as_table_mut()
            .unwrap()
            .insert("target".into(), target.into());
        fs::write(f.file("client.toml"), toml::to_string(&document).unwrap()).unwrap();
        let error = invalid(&f, "client.toml");
        assert_eq!(error.field(), "client.target");
        assert_eq!(error.reason, ConfigReason::Range);
        let message = error.to_string();
        for category in [
            "unspecified",
            "multicast",
            "broadcast",
            "unscoped IPv6 link-local",
        ] {
            assert!(
                message.contains(category),
                "missing address restriction: {category}"
            );
        }
        assert!(!message.contains(target));
        let output = Command::new(env!("CARGO_BIN_EXE_remora"))
            .args(["check-config", "--config"])
            .arg(f.file("client.toml"))
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!("{message}\n")
        );
    }
}

#[test]
fn field_and_syntax_diagnostics_are_actionable_without_echoing_values() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("client.toml")).unwrap();
    for (suffix, field, hint) in [
        ("[io]\nbatch_max=0", "io.batch_max", "1..64"),
        ("[io]\nmode='SECRET-SENTINEL'", "io.mode", "mmsg or single"),
        (
            "[transport]\napplication_max_bytes='SECRET-SENTINEL'",
            "transport.application_max_bytes",
            "integer",
        ),
        (
            "[limits]\nmax_sessions=16",
            "limits.max_sessions_per_identity",
            "limits.max_sessions",
        ),
    ] {
        fs::write(f.file("bad.toml"), format!("{original}\n{suffix}")).unwrap();
        let error = invalid(&f, "bad.toml");
        assert_eq!(error.field(), field);
        assert!(error.to_string().contains(hint));
        assert!(!format!("{error} {error:?}").contains("SECRET-SENTINEL"));
    }
    fs::write(
        f.file("bad.toml"),
        "schema_version=1\n[client\nsecret='SECRET-SENTINEL'",
    )
    .unwrap();
    let error = invalid(&f, "bad.toml");
    assert!(error.to_string().contains("line 2, column"));
    assert!(!format!("{error} {error:?}").contains("SECRET-SENTINEL"));
}

#[test]
fn command_help_and_successful_validation_are_visible() {
    for command in ["client", "server", "check-config"] {
        let output = Command::new(env!("CARGO_BIN_EXE_remora"))
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains(&format!("Usage: remora {command} --config PATH")));
    }
    let f = Fixtures::new();
    let output = Command::new(env!("CARGO_BIN_EXE_remora"))
        .args(["check-config", "--config"])
        .arg(f.file("client.toml"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"configuration is valid (client)\n");
}
