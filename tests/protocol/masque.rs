use proptest::prelude::*;
use quiche::h3::{Header, NameValue};
use remora::masque::*;
fn headers() -> Vec<Header> {
    request_headers(
        "proxy.test",
        "/.well-known/masque/udp/127.0.0.1/9000/",
        &[b'a'; 64],
    )
}
#[test]
fn request_shape_routes_credentials_and_injection_fail_closed() {
    let req = request(&headers(), "proxy.test", "/.well-known/masque/udp/").unwrap();
    assert_eq!(req.host, "127.0.0.1");
    assert_eq!(req.port, 9000);
    for (name, value) in [
        (b":method".as_slice(), b"GET".as_slice()),
        (b":protocol", b"connect-ip"),
        (b":scheme", b"http"),
        (b":authority", b"target.test"),
        (b":path", b"/other/"),
    ] {
        let mut h = headers();
        let index = match name {
            b":method" => 0,
            b":protocol" => 1,
            b":scheme" => 2,
            b":authority" => 3,
            _ => 4,
        };
        h[index] = Header::new(name, value);
        assert!(request(&h, "proxy.test", "/.well-known/masque/udp/").is_err());
    }
    let mut h = headers();
    h.push(Header::new(b"authorization", b"Bearer SECRET-SENTINEL"));
    assert!(request(&h, "proxy.test", "/.well-known/masque/udp/").is_err());
    for name in [
        b"content-length".as_slice(),
        b"content-type",
        b"transfer-encoding",
    ] {
        let mut h = headers();
        h.push(Header::new(name, b"0"));
        assert_eq!(
            request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
            Some(WireError::Message)
        );
    }
    for path in [
        "/.well-known/masque/udp/127.1/80/",
        "/.well-known/masque/udp/127.0.0.1/0/",
        "/.well-known/masque/udp/127.0.0.1/080/",
        "/.well-known/masque/udp/%253A/80/",
        "/.well-known/masque/udp/a%2Fb/80/",
        "/.well-known/masque/udp/a%00b/80/",
        "/.well-known/masque/udp/127.0.0.1/80/extra/",
        "/.well-known/masque/udp/127.0.0.1/80/?x=1",
    ] {
        assert!(
            parse_target(path, "/.well-known/masque/udp/").is_err(),
            "{path}"
        );
    }
}
#[test]
fn response_requires_final_success_and_capsule_message_rules() {
    for status in [200, 201, 202, 203, 207, 299] {
        assert_eq!(
            response(&response_headers(status), false),
            Ok(Response::Accepted)
        );
    }
    for status in [204, 205, 206] {
        assert_eq!(
            response(&response_headers(status), false),
            Err(WireError::Message)
        );
    }
    for name in [
        b"content-length".as_slice(),
        b"content-type",
        b"transfer-encoding",
    ] {
        let mut h = response_headers(200);
        h.push(Header::new(name, b"0"));
        assert_eq!(response(&h, false), Err(WireError::Message));
    }
    assert_eq!(
        response(&response_headers(103), false),
        Ok(Response::Informational)
    );
    assert_eq!(
        response(&response_headers(401), false),
        Ok(Response::Rejected(401))
    );
    assert_eq!(
        response(&response_headers(200), true),
        Err(WireError::Message)
    );
    assert_eq!(
        response(&[Header::new(b":status", b"200")], false),
        Ok(Response::Accepted)
    );
    for value in [b"?1;x=7".as_slice(), b"?0;unknown=token", b"not-boolean"] {
        let h = vec![
            Header::new(b":status", b"200"),
            Header::new(b"capsule-protocol", value),
        ];
        assert_eq!(response(&h, false), Ok(Response::Accepted));
    }
    assert_eq!(
        capsule_protocol(&[Header::new(b"capsule-protocol", b"?1;unknown=token")]),
        Some(true)
    );
    assert_eq!(
        capsule_protocol(&[
            Header::new(b"capsule-protocol", b"?1"),
            Header::new(b"capsule-protocol", b"?1")
        ]),
        None
    );
    let h = vec![
        Header::new(b":status", b"200"),
        Header::new(b":status", b"200"),
    ];
    assert_eq!(response(&h, false), Err(WireError::Message));
}
#[test]
fn templates_percent_encode_once_and_support_query_expressions() {
    let t = UriTemplate::parse(
        "https://proxy.test/.well-known/masque/udp/{target_host}/{target_port}/",
        "proxy.test",
    )
    .unwrap();
    let p = t.expand("2001:db8::42", 443).unwrap();
    assert!(p.contains("2001%3Adb8%3A%3A42"));
    assert_eq!(
        parse_target(&p, "/.well-known/masque/udp/").unwrap(),
        ("2001:db8::42".into(), 443)
    );
    for template in [
        "HTTPS://proxy.test/{target_host}/{target_port}/{target_host}",
        "https://proxy.test/masque{?target_host,target_port}",
        "https://proxy.test/masque?h={target_host}&p={target_port}",
    ] {
        let t = UriTemplate::parse(template, "proxy.test").unwrap();
        assert!(
            t.expand("Remora.Test.", 9000)
                .unwrap()
                .contains("remora.test")
        );
    }
    for template in [
        "http://proxy.test/{target_host}/{target_port}/",
        "https://proxy.test/{+target_host}/{target_port}/",
        "https://proxy.test/{target_host:3}/{target_port}/",
        "https://proxy.test/{target_host*}/{target_port}/",
        "https://proxy.test/{target_host}/{target_port}/#x",
        "https://proxy.test/{target_host}/{missing}/",
        "https://proxy.test/{target_host}/{target_port}/%Q0",
    ] {
        assert!(UriTemplate::parse(template, "proxy.test").is_err());
    }
}
#[test]
fn association_context_and_empty_payload_boundaries() {
    let mut out = [0; 32];
    let n = encode_datagram(4, b"", &mut out).unwrap();
    let d = decode_datagram(&out[..n]).unwrap().unwrap();
    assert_eq!(d.stream, 4);
    assert_eq!(d.payload, b"");
    assert!(decode_datagram(&[0]).unwrap().is_none());
    assert!(decode_datagram(&[0, 0xc0, 1]).unwrap().is_none());
    assert_eq!(decode_datagram(&[]).err(), Some(WireError::Datagram));
    let mut invalid = [0; 8];
    put_varint(MAX_VARINT, &mut invalid).unwrap();
    assert_eq!(decode_datagram(&invalid).err(), Some(WireError::Datagram));
    let d = decode_datagram(&[0, 7, 42]).unwrap().unwrap();
    assert_eq!(d.context, 7);
    let mut huge = vec![0; 65530];
    let n = encode_datagram(0, &vec![1; 65527], &mut huge).unwrap();
    assert_eq!(
        decode_datagram(&huge[..n]).unwrap().unwrap().payload.len(),
        65527
    );
    assert_eq!(
        encode_datagram(0, &vec![1; 65528], &mut huge),
        Err(WireError::Datagram)
    );
}
proptest! {
 #[test]fn datagram_roundtrip_preserves_boundaries(stream in 0u64..(1u64<<59),data in proptest::collection::vec(any::<u8>(),0..2048)){let stream=stream*4;let mut out=vec![0;data.len()+16];let n=encode_datagram(stream,&data,&mut out).unwrap();let d=decode_datagram(&out[..n]).unwrap().unwrap();prop_assert_eq!(d.stream,stream);prop_assert_eq!(d.payload,&data);}
 #[test]fn arbitrary_varints_datagrams_and_paths_do_not_panic(data in proptest::collection::vec(any::<u8>(),0..256)){let _=get_varint(&data);let _=decode_datagram(&data);if let Ok(path)=std::str::from_utf8(&data){let _=parse_target(path,"/.well-known/masque/udp/");let _=UriTemplate::parse(path,"proxy.test");}}
}

#[test]
fn field_semantics_reject_te_host_conflicts_controls_and_edge_whitespace() {
    for value in [b"gzip".as_slice(), b"trailers, gzip", b"", b"trailers; q=1"] {
        let mut h = headers();
        h.push(Header::new(b"te", value));
        assert_eq!(
            request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
            Some(WireError::Message)
        );
    }
    for value in [b"trailers".as_slice(), b"Trailers"] {
        let mut h = headers();
        h.push(Header::new(b"te", value));
        h.push(Header::new(b"host", b"proxy.test"));
        assert!(request(&h, "proxy.test", "/.well-known/masque/udp/").is_ok());
        let mut response_fields = response_headers(200);
        response_fields.push(Header::new(b"te", value));
        assert_eq!(response(&response_fields, false), Err(WireError::Message));
    }
    let mut h = headers();
    h.push(Header::new(b"host", b"other.test"));
    assert_eq!(
        request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
        Some(WireError::Message)
    );
    // Semantic errors precede configured-route/method rejection, including
    // missing/empty authority and conflicting Host on a foreign authority.
    for authority in [b"foreign.test".as_slice(), b""] {
        let mut h = headers();
        h.retain(|h| h.name() != b":authority");
        h.insert(3, Header::new(b":authority", authority));
        h.push(Header::new(b"host", b"other.test"));
        assert_eq!(
            request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
            Some(WireError::Message)
        );
    }
    let mut h = headers();
    h.retain(|h| h.name() != b":authority");
    h.push(Header::new(b"host", b"proxy.test"));
    assert_eq!(
        request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
        Some(WireError::Message)
    );
    let mut h = headers();
    h.push(Header::new(b"host", b"proxy.test"));
    h.push(Header::new(b"host", b"proxy.test"));
    assert_eq!(
        request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
        Some(WireError::Message)
    );
    for value in [b"a\x01b".as_slice(), b"a\x7fb", b" leading", b"trailing\t"] {
        let mut h = headers();
        h.push(Header::new(b"x-test", value));
        assert_eq!(
            request(&h, "proxy.test", "/.well-known/masque/udp/").err(),
            Some(WireError::Message)
        );
        let mut h = response_headers(200);
        h.push(Header::new(b"x-test", value));
        assert_eq!(response(&h, false), Err(WireError::Message));
    }
    for value in [b"a\tb".as_slice(), b"a b", b"a\x80b", b""] {
        let mut h = headers();
        h.push(Header::new(b"x-test", value));
        assert!(request(&h, "proxy.test", "/.well-known/masque/udp/").is_ok());
    }
}
