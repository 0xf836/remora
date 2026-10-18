//! Test-only in-memory TLS/QUIC peer; configs are provisioned once per campaign.
use crate::{
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
}
impl Pair {
    pub fn new(client: &ValidatedConfig, server: &ValidatedConfig) -> Self {
        let mut cc = transport::config(client).unwrap();
        let mut sc = transport::config(server).unwrap();
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
        let mut buffer = [0; 1350];
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
            client: Connection::new(c, Generation(1), false, client, Instant::now()),
            server: Connection::new(q, Generation(2), true, server, Instant::now()),
            pool: BufferPool::new(1350, 1350 * 128).unwrap(),
        }
    }
    pub fn pump(&mut self) {
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
            let mut buffer = [0; 1350];
            for _ in 0..32 {
                match from.quic.send(&mut buffer) {
                    Ok((n, info)) => {
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
        for _ in 0..300 {
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
    pub fn requests(&mut self) -> Vec<u64> {
        self.negotiate().unwrap();
        let headers = masque::request_headers(
            "remora.test",
            "/.well-known/masque/udp/127.0.0.1/53/",
            &[b'0'; 64],
        );
        let h3 = self.client.h3.as_mut().unwrap();
        let streams = (0..16)
            .map(|_| {
                h3.send_request(&mut self.client.quic, &headers, false)
                    .unwrap()
            })
            .collect();
        for _ in 0..20 {
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
        streams
    }
}
