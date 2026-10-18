use super::forwarding::Tunnel;
use crate::common::Fixtures;
use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};

fn configure(f: &Fixtures, identity: &str, pinned: bool, san: &str) {
    let mut c: toml::Table =
        toml::from_str(&fs::read_to_string(f.file("client.toml")).unwrap()).unwrap();
    let client = c.get_mut("client").unwrap().as_table_mut().unwrap();
    let address = client.remove("connect_address").unwrap();
    let port = address.as_str().unwrap().rsplit(':').next().unwrap();
    let proxy = format!("https://{identity}:{port}");
    client.insert("proxy".into(), proxy.clone().into());
    if pinned {
        client.insert("connect_address".into(), address);
    }
    fs::write(
        f.file("client.toml"),
        toml::to_string(&c).unwrap() + "\n[recovery]\nsetup_timeout_ms=1000\n",
    )
    .unwrap();
    let mut s: toml::Table =
        toml::from_str(&fs::read_to_string(f.file("server.toml")).unwrap()).unwrap();
    let server = s.get_mut("server").unwrap().as_table_mut().unwrap();
    server.remove("path_prefix");
    server.insert("proxy".into(), proxy.into());
    fs::write(f.file("server.toml"), toml::to_string(&s).unwrap()).unwrap();
    let san_extension = if san.is_empty() {
        String::new()
    } else {
        format!("subjectAltName={san}\n")
    };
    fs::write(
        f.file("server.ext"),
        format!("{san_extension}basicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\n"),
    )
    .unwrap();
    assert!(
        Command::new("openssl")
            .current_dir(f.path())
            .args([
                "x509",
                "-req",
                "-in",
                "server.csr",
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca-key.pem",
                "-set_serial",
                "42",
                "-out",
                "server.pem",
                "-days",
                "2",
                "-extfile",
                "server.ext"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn dns_and_numeric_override_verify_identity_and_forward() {
    for (identity, pinned, san) in [
        ("localhost", false, "DNS:localhost"),
        ("remora.test", true, "DNS:remora.test"),
        ("127.0.0.1", false, "IP:127.0.0.1"),
    ] {
        let mut t = Tunnel::start(false, |f| configure(f, identity, pinned, san));
        let sender = t.sender();
        t.ready(&sender, b"through-verified-proxy");
        t.stop();
    }
}

#[test]
fn dns_and_ip_reference_identities_reject_wrong_san() {
    for (identity, pinned, san) in [
        ("localhost", false, "DNS:wrong.test"),
        ("127.0.0.1", false, "IP:127.0.0.2"),
        ("127.0.0.1", false, "DNS:127.0.0.1"),
        ("remora.test", true, ""),
    ] {
        let mut t = Tunnel::start(false, |f| configure(f, identity, pinned, san));
        let deadline = Instant::now() + Duration::from_secs(5);
        let failure = loop {
            let error = fs::read_to_string(t.fixtures.file("client.err")).unwrap();
            if error.contains("\"failure\":\"trust\"") {
                break error;
            }
            assert!(
                Instant::now() < deadline,
                "expected explicit TLS trust failure: {error}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(!failure.contains("authorization"));
        assert_eq!(
            t.client.finish(Duration::from_secs(5)).unwrap().code(),
            Some(1)
        );
        t.server.signal(rustix::process::Signal::TERM).unwrap();
        assert!(t.server.finish(Duration::from_secs(5)).unwrap().success());
    }
}

#[test]
fn equivalent_https_origins_interoperate_over_a_numeric_transport_override() {
    for (client_origin, server_origin) in [
        ("HTTPS://REMORA.TEST:443/", "https://remora.test"),
        ("https://remora.test/", "https://remora.test:443"),
    ] {
        let mut t = Tunnel::start(false, |f| {
            configure(f, "remora.test", true, "DNS:remora.test");
            for (role, origin) in [("client", client_origin), ("server", server_origin)] {
                let file = f.file(&format!("{role}.toml"));
                let mut document: toml::Table =
                    toml::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
                document[role]
                    .as_table_mut()
                    .unwrap()
                    .insert("proxy".into(), origin.into());
                fs::write(file, toml::to_string(&document).unwrap()).unwrap();
            }
        });
        let sender = t.sender();
        t.ready(&sender, b"normalized-https-origin");
        t.stop();
    }
}
