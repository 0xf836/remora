use crate::common::Fixtures;
use remora::{
    buffers::Generation,
    config::{RoleConfig, ValidatedConfig},
    policy::*,
};
use sha2::{Digest, Sha256};
use std::{cell::Cell, fs, net::SocketAddr, time::Duration};
struct Fake {
    calls: Cell<usize>,
    answer: Result<Vec<SocketAddr>, PolicyError>,
    delay: Duration,
}
impl Resolver for Fake {
    fn resolve<'a>(&'a self, _: &'a str, _: u16) -> ResolveFuture<'a> {
        self.calls.set(self.calls.get() + 1);
        Box::pin(async move {
            tokio::time::sleep(self.delay).await;
            self.answer.clone()
        })
    }
}
fn key() -> SetupKey {
    SetupKey {
        connection: Generation(1),
        session: Generation(2),
    }
}
#[test]
fn authentication_hashes_ascii_and_rejects_missing_invalid_revoked_secrets() {
    let f = Fixtures::new();
    let c = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    let RoleConfig::Server(s) = c.role() else {
        panic!()
    };
    let token = fs::read(f.file("client.token")).unwrap();
    let token = &token[..64];
    assert_eq!(authenticate(&s.identities, token), Ok(0));
    for bad in [b"".as_slice(), b"SECRET-SENTINEL", &[b'0'; 64], &[b'A'; 64]] {
        assert_eq!(
            authenticate(&s.identities, bad),
            Err(PolicyError::Authentication)
        );
    }
    assert_eq!(authenticate(&[], token), Err(PolicyError::Authentication));
    let decoded: Vec<_> = token
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect();
    assert_ne!(
        Sha256::digest(&decoded).as_slice(),
        s.identities[0].token_sha256
    );
}
#[test]
fn exact_all_answer_permissions_pin_only_approved_target_and_ignore_stale_generations() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("credentials.toml")).unwrap();
    fs::write(
        f.file("credentials.toml"),
        original.replace("host = \"127.0.0.1\"", "host = \"destination.test\""),
    )
    .unwrap();
    let config = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    let RoleConfig::Server(s) = config.role() else {
        panic!()
    };
    let identity = &s.identities[0];
    let port = identity.targets[0].port;
    let a = SocketAddr::new("127.0.0.1".parse().unwrap(), port);
    let b = SocketAddr::new("127.0.0.2".parse().unwrap(), port);
    assert_eq!(
        authorize_answers(identity, "Destination.Test.", port, &[a]),
        Ok(a)
    );
    for (host, p, answers) in [
        ("alias.test", port, vec![a]),
        ("destination.test", port + 1, vec![a]),
        ("destination.test", port, vec![a, b]),
        ("destination.test", port, vec![]),
        ("destination.test", port, vec![b]),
    ] {
        assert_eq!(
            authorize_answers(identity, host, p, &answers),
            Err(PolicyError::Denied)
        );
    }
    let pinned = ResolvedTarget {
        address: a,
        key: key(),
    };
    assert_eq!(pinned.for_current(key()), Ok(a));
    assert_eq!(
        pinned.for_current(SetupKey {
            connection: Generation(3),
            ..key()
        }),
        Err(PolicyError::Stale)
    );
    for ip in [
        "0.0.0.0",
        "224.0.0.1",
        "255.255.255.255",
        "::",
        "ff02::1",
        "fe80::1",
    ] {
        assert!(!valid_destination(ip.parse().unwrap()));
    }
}
#[test]
fn resolution_is_after_auth_and_exact_rule_with_timeout_failure_and_cancellation() {
    let f = Fixtures::new();
    let original = fs::read_to_string(f.file("credentials.toml")).unwrap();
    fs::write(
        f.file("credentials.toml"),
        original.replace("host = \"127.0.0.1\"", "host = \"destination.test\""),
    )
    .unwrap();
    let config = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    let RoleConfig::Server(s) = config.role() else {
        panic!()
    };
    let port = s.identities[0].targets[0].port;
    let token = fs::read(f.file("client.token")).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let resolver = Fake {
            calls: Cell::new(0),
            answer: Err(PolicyError::Resolution),
            delay: Duration::ZERO,
        };
        assert_eq!(
            prepare(
                &s.identities,
                b"bad",
                "destination.test",
                port,
                &resolver,
                key(),
                Duration::from_secs(1)
            )
            .await,
            Err(PolicyError::Authentication)
        );
        assert_eq!(resolver.calls.get(), 0);
        assert_eq!(
            prepare(
                &s.identities,
                &token[..64],
                "alias.test",
                port,
                &resolver,
                key(),
                Duration::from_secs(1)
            )
            .await,
            Err(PolicyError::Denied)
        );
        assert_eq!(resolver.calls.get(), 0);
        assert_eq!(
            prepare(
                &s.identities,
                &token[..64],
                "destination.test",
                port,
                &resolver,
                key(),
                Duration::from_secs(1)
            )
            .await,
            Err(PolicyError::Resolution)
        );
        let slow = Fake {
            calls: Cell::new(0),
            answer: Ok(vec![]),
            delay: Duration::from_secs(10),
        };
        assert_eq!(
            prepare(
                &s.identities,
                &token[..64],
                "destination.test",
                port,
                &slow,
                key(),
                Duration::from_millis(1)
            )
            .await,
            Err(PolicyError::Timeout)
        );
        let mut pending = Box::pin(prepare(
            &s.identities,
            &token[..64],
            "destination.test",
            port,
            &slow,
            key(),
            Duration::from_secs(20),
        ));
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(pending);
    });
}
proptest::proptest! {#[test]fn arbitrary_tokens_fail_without_parser_panics(bytes in proptest::collection::vec(proptest::prelude::any::<u8>(),0..128)){proptest::prop_assert_eq!(authenticate(&[],&bytes),Err(PolicyError::Authentication));}}
#[test]
fn explicitly_authorized_scoped_link_local_answer_keeps_interface_scope() {
    let f = Fixtures::new();
    f.rewrite(
        "credentials.toml",
        r#"host = "127.0.0.1""#,
        r#"host = "link.test""#,
    );
    f.rewrite("credentials.toml", "127.0.0.1/32", "fe80::/64");
    let c = ValidatedConfig::load(&f.file("server.toml")).unwrap();
    let RoleConfig::Server(s) = c.role() else {
        panic!()
    };
    let identity = &s.identities[0];
    let port = identity.targets[0].port;
    let address = SocketAddr::V6(std::net::SocketAddrV6::new(
        "fe80::1".parse().unwrap(),
        port,
        0,
        2,
    ));
    assert_eq!(
        authorize_answers(identity, "link.test", port, &[address]),
        Ok(address)
    );
    let unscoped = SocketAddr::new(address.ip(), port);
    assert_eq!(
        authorize_answers(identity, "link.test", port, &[unscoped]),
        Err(PolicyError::Denied)
    );
}
