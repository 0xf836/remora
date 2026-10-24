use super::Fixtures;
use remora::{
    buffers::{BufferPool, Generation},
    config::ValidatedConfig,
    masque,
    transport::{self, Connection, TrackedFactory, TransportError},
};
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};
pub struct Pair {
    pub client: Connection,
    pub server: Connection,
    #[allow(dead_code)] // Used when this shared harness is included by lifecycle tests.
    pub pool: BufferPool,
    pub fixture: Fixtures,
    pub outer_caps: [usize; 2],
    pub one_way_delay: Duration,
}
impl Pair {
    pub fn new(capabilities: bool) -> Self {
        Self::with_config(capabilities, |_| {})
    }
    pub fn with_config(capabilities: bool, change: impl FnOnce(&Fixtures)) -> Self {
        Self::with_transport_config(capabilities, change, |_| {})
    }
    pub fn with_transport_config(
        capabilities: bool,
        change: impl FnOnce(&Fixtures),
        transport_change: impl FnOnce(&mut quiche::Config),
    ) -> Self {
        let fixture = Fixtures::new();
        change(&fixture);
        let client = ValidatedConfig::load(&fixture.file("client.toml")).unwrap();
        let server = ValidatedConfig::load(&fixture.file("server.toml")).unwrap();
        let mut cc = transport::config(&client).unwrap();
        let mut sc = transport::config(&server).unwrap();
        transport_change(&mut cc);
        if !capabilities {
            sc.enable_dgram(false, 0, 0);
        }
        let ca: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let sa = "127.0.0.1:40001".parse().unwrap();
        let cid = [1; 16];
        let scid = [2; 16];
        let mut c = quiche::connect_with_buffer_factory::<TrackedFactory>(
            Some("remora.test"),
            &quiche::ConnectionId::from_ref(&cid),
            ca,
            sa,
            &mut cc,
        )
        .unwrap();
        let outer_caps = [
            client.transport().outer_udp_max_bytes,
            server.transport().outer_udp_max_bytes,
        ];
        let largest = *outer_caps.iter().max().unwrap();
        let mut buffer = vec![0; largest];
        let (n, _) = c.send(&mut buffer).unwrap();
        let header = quiche::Header::from_slice(&mut buffer[..n], 16).unwrap();
        let mut q = quiche::accept_with_buf_factory::<TrackedFactory>(
            &quiche::ConnectionId::from_ref(&scid),
            Some(&header.dcid),
            sa,
            ca,
            &mut sc,
        )
        .unwrap();
        q.recv(&mut buffer[..n], quiche::RecvInfo { from: ca, to: sa })
            .unwrap();
        Self {
            client: Connection::new(c, Generation(1), false, &client, Instant::now()),
            server: Connection::new(q, Generation(2), true, &server, Instant::now()),
            pool: BufferPool::new(largest, largest * 128).unwrap(),
            fixture,
            outer_caps,
            one_way_delay: Duration::ZERO,
        }
    }
    pub fn pump(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        for direction in 0..2 {
            let (from, to) = if direction == 0 {
                (&mut self.client, &mut self.server)
            } else {
                (&mut self.server, &mut self.client)
            };
            if from
                .quic
                .timeout_instant()
                .is_some_and(|t| t <= Instant::now())
            {
                from.quic.on_timeout();
            }
            let mut buffer = vec![0; self.outer_caps[direction]];
            for _ in 0..32 {
                // Slow gcongestion fixtures must advance physical time before
                // delivery, rather than ACK packets whose recovery timestamp
                // still lies in the future. Capture the pre-send decision.
                if from.quic.pacing_enabled()
                    && from.quic.gcongestion_enabled() == Some(true)
                    && let Some(at) = from
                        .quic
                        .get_next_release_time()
                        .and_then(|decision| decision.time(Instant::now()))
                {
                    assert!(
                        at <= deadline,
                        "fixture pacing exceeds bounded pump deadline"
                    );
                    std::thread::sleep(at.saturating_duration_since(Instant::now()));
                }
                match from.quic.send(&mut buffer) {
                    Ok((n, info)) => {
                        assert!(
                            Instant::now() + self.one_way_delay <= deadline,
                            "fixture delivery exceeds bounded pump deadline"
                        );
                        if !self.one_way_delay.is_zero() {
                            std::thread::sleep(self.one_way_delay);
                        }
                        assert!(
                            n <= self.outer_caps[1 - direction],
                            "output exceeds advertised peer cap"
                        );
                        to.receive(
                            &mut buffer[..n],
                            quiche::RecvInfo {
                                from: info.from,
                                to: info.to,
                            },
                            Instant::now(),
                        )
                        .unwrap();
                    }
                    Err(quiche::Error::Done) => break,
                    Err(e) => panic!("pump {e:?}"),
                }
            }
        }
    }
    pub fn negotiate(&mut self) -> Result<(), TransportError> {
        let deadline = Instant::now() + Duration::from_secs(10);
        for _ in 0..300 {
            assert!(Instant::now() < deadline, "fixture negotiation deadline");
            self.pump();
            let cr = self.client.negotiate()?;
            let sr = self.server.negotiate()?;
            for c in [&mut self.client, &mut self.server] {
                if let Some(h3) = c.h3.as_mut() {
                    let _ = h3.poll(&mut c.quic);
                }
            }
            if cr && sr {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("handshake/settings deadline")
    }
    pub fn requests(&mut self) -> (u64, u64) {
        self.negotiate().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let token = std::fs::read_to_string(self.fixture.file("client.token")).unwrap();
        let headers = masque::request_headers(
            "remora.test",
            "/.well-known/masque/udp/127.0.0.1/53/",
            token.trim().as_bytes().try_into().unwrap(),
        );
        let h3 = self.client.h3.as_mut().unwrap();
        let a = h3
            .send_request(&mut self.client.quic, &headers, false)
            .unwrap();
        let b = h3
            .send_request(&mut self.client.quic, &headers, false)
            .unwrap();
        for _ in 0..20 {
            assert!(Instant::now() < deadline, "fixture request deadline");
            self.pump();
            while self
                .server
                .h3
                .as_mut()
                .unwrap()
                .poll(&mut self.server.quic)
                .is_ok()
            {}
        }
        (a, b)
    }
}
