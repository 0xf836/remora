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

fn set_access_acl(path: &std::path::Path, users: &[(u32, u16)], group: u16, mask: u16, other: u16) {
    set_access_acl_with_groups(path, users, &[], group, mask, other);
}

fn set_access_acl_with_groups(
    path: &std::path::Path,
    users: &[(u32, u16)],
    groups: &[(u32, u16)],
    group: u16,
    mask: u16,
    other: u16,
) {
    let mut entries = vec![(1_u16, 4_u16, u32::MAX)];
    // Preserve supplied qualifier order, including kernel-valid descending IDs.
    entries.extend(
        users
            .iter()
            .map(|&(uid, permissions)| (2, permissions, uid)),
    );
    entries.push((4, group, u32::MAX));
    entries.extend(
        groups
            .iter()
            .map(|&(gid, permissions)| (8, permissions, gid)),
    );
    entries.extend([(16, mask, u32::MAX), (32, other, u32::MAX)]);
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    for (tag, permissions, id) in entries {
        bytes.extend(tag.to_le_bytes());
        bytes.extend(permissions.to_le_bytes());
        bytes.extend(id.to_le_bytes());
    }
    let file = fs::File::open(path).unwrap();
    rustix::fs::fsetxattr(
        &file,
        c"system.posix_acl_access",
        &bytes,
        rustix::fs::XattrFlags::empty(),
    )
    .expect("private-file ACL tests require a Linux POSIX-ACL-capable temporary filesystem");
}

#[test]
fn private_acl_loader_accepts_unsorted_kernel_user_and_group_ids() {
    let f = Fixtures::new();
    let uid = rustix::process::geteuid().as_raw();
    let foreign = uid.checked_add(2).unwrap();
    let users = [(foreign, 0), (uid + 1, 0), (uid, 4)];
    let gid = rustix::process::getegid().as_raw();
    let groups = [(gid.checked_add(2).unwrap(), 0), (gid + 1, 0)];
    for (file, config) in [
        ("client.token", "client.toml"),
        ("server-key.pem", "server.toml"),
        ("credentials.toml", "server.toml"),
    ] {
        set_access_acl_with_groups(&f.file(file), &users, &groups, 0, 4, 0);
        let held = fs::File::open(f.file(file)).unwrap();
        let mut bytes = [0; 1024];
        let length =
            rustix::fs::fgetxattr(&held, c"system.posix_acl_access", &mut bytes[..]).unwrap();
        let (records, remainder) = bytes[4..length].as_chunks::<8>();
        assert!(remainder.is_empty());
        for (tag, expected) in [
            (2_u16, users.iter().map(|&(id, _)| id).collect::<Vec<_>>()),
            (8_u16, groups.iter().map(|&(id, _)| id).collect()),
        ] {
            let exported: Vec<_> = records
                .iter()
                .filter(|record| u16::from_le_bytes([record[0], record[1]]) == tag)
                .map(|record| u32::from_le_bytes([record[4], record[5], record[6], record[7]]))
                .collect();
            assert_eq!(
                exported, expected,
                "the real kernel xattr must retain the unsorted test IDs"
            );
        }
        ValidatedConfig::load(&f.file(config)).unwrap();
        set_access_acl_with_groups(
            &f.file(file),
            &[(foreign, 4), (uid + 1, 0), (uid, 4)],
            &groups,
            0,
            4,
            0,
        );
        assert_eq!(invalid(&f, config).reason, ConfigReason::Permissions);
        set_access_acl_with_groups(
            &f.file(file),
            &users,
            &[(gid + 2, 4), (gid + 1, 0)],
            0,
            4,
            0,
        );
        assert_eq!(invalid(&f, config).reason, ConfigReason::Permissions);
        rustix::fs::fremovexattr(&held, c"system.posix_acl_access").unwrap();
        fs::set_permissions(f.file(file), fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn private_acl_files_allow_safe_masks_and_reject_effective_group_or_other_access() {
    let f = Fixtures::new();
    let uid = rustix::process::geteuid().as_raw();
    for (file, config) in [
        ("client.token", "client.toml"),
        ("server-key.pem", "server.toml"),
        ("credentials.toml", "server.toml"),
    ] {
        for mode in [0o400, 0o600] {
            fs::set_permissions(f.file(file), fs::Permissions::from_mode(mode)).unwrap();
            ValidatedConfig::load(&f.file(config)).unwrap();
        }
        set_access_acl(&f.file(file), &[(uid, 4)], 0, 4, 0);
        assert_eq!(fs::metadata(f.file(file)).unwrap().mode() & 0o777, 0o440);
        ValidatedConfig::load(&f.file(config)).unwrap();
        for (group, other) in [(4, 0), (2, 0), (1, 0), (0, 4), (0, 2), (0, 1)] {
            set_access_acl(&f.file(file), &[(uid, 4)], group, 7, other);
            let error = invalid(&f, config);
            assert_eq!(error.reason, ConfigReason::Permissions);
            assert!(!format!("{error:?}").contains("system.posix_acl_access"));
        }
        // Real kernel mask semantics: the group entry is present but ineffective.
        set_access_acl(&f.file(file), &[(uid, 4)], 7, 0, 0);
        ValidatedConfig::load(&f.file(config)).unwrap();
        set_access_acl(&f.file(file), &[(uid, 4)], 7, 4, 0);
        assert_eq!(invalid(&f, config).reason, ConfigReason::Permissions);
        rustix::fs::fremovexattr(
            fs::File::open(f.file(file)).unwrap(),
            c"system.posix_acl_access",
        )
        .unwrap();
        fs::set_permissions(f.file(file), fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
#[ignore = "requires root, a POSIX-ACL-capable /tmp and the ability to spawn UID 65533"]
fn root_owned_acl_secrets_delegate_read_to_an_unprivileged_process() {
    use std::os::unix::process::CommandExt;
    assert_eq!(
        rustix::process::geteuid().as_raw(),
        0,
        "run this isolated test as root"
    );
    let f = Fixtures::new();
    // Only disposable fixtures are exposed. Secure open holds RDONLY directory
    // descriptors, so both read and search permissions are required here.
    fs::set_permissions(
        f.path().parent().unwrap(),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fs::set_permissions(f.path(), fs::Permissions::from_mode(0o755)).unwrap();
    for file in ["client.toml", "server.toml", "server.pem", "ca.pem"] {
        fs::set_permissions(f.file(file), fs::Permissions::from_mode(0o644)).unwrap();
    }
    let service_uid = 65533;
    let foreign_uid = 65532;
    // The service UID need not be able to traverse the developer's home.
    fs::copy(env!("CARGO_BIN_EXE_remora"), f.file("remora")).unwrap();
    fs::set_permissions(f.file("remora"), fs::Permissions::from_mode(0o755)).unwrap();
    for file in ["client.token", "server-key.pem", "credentials.toml"] {
        assert_eq!(fs::metadata(f.file(file)).unwrap().uid(), 0);
        set_access_acl(&f.file(file), &[(service_uid, 4)], 0, 4, 0);
    }
    let check = |config: &str, succeeds: bool, field: &str| {
        let output = Command::new(f.file("remora"))
            .args(["check-config", "--config"])
            .arg(f.file(config))
            .uid(service_uid)
            .gid(service_uid)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if succeeds { 0 } else { 2 }),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if !succeeds {
            assert_eq!(
                String::from_utf8(output.stderr).unwrap().trim(),
                format!("invalid configuration: {field} (Permissions)")
            );
        }
    };
    check("client.toml", true, "token_file");
    check("server.toml", true, "private_key_file");
    for (file, config, field) in [
        ("client.token", "client.toml", "token_file"),
        ("server-key.pem", "server.toml", "private_key_file"),
        ("credentials.toml", "server.toml", "credentials_file"),
    ] {
        assert_eq!(fs::metadata(f.file(file)).unwrap().mode() & 0o777, 0o440);
        for permissions in [1, 2, 4] {
            set_access_acl(
                &f.file(file),
                &[(service_uid, 4), (foreign_uid, permissions)],
                0,
                7,
                0,
            );
            check(config, false, field);
        }
        set_access_acl(&f.file(file), &[(service_uid, 4)], 4, 4, 0);
        check(config, false, field);
        for permissions in [5, 6, 7] {
            set_access_acl(&f.file(file), &[(service_uid, permissions)], 0, 7, 0);
            check(config, false, field);
        }
        set_access_acl(&f.file(file), &[(service_uid, 4)], 0, 4, 0);
        check(config, true, field);
        set_access_acl(
            &f.file(file),
            &[(service_uid, 4), (foreign_uid, 0)],
            0,
            4,
            0,
        );
        check(config, true, field);
    }
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
    assert_eq!(config.transport().outer_udp_max_bytes, 1452);
    assert_eq!(config.transport().application_max_bytes, 1400);
    let server = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    assert_eq!(server.transport().outer_udp_max_bytes, 1452);
    assert_eq!(server.transport().application_max_bytes, 1400);
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
