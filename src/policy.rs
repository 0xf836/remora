//! Per-request authentication and all-answer target authorization.
//! No resolver or destination socket is reached before credential verification.
use crate::{
    buffers::Generation,
    config::{Identity, canonical_host, normalize_ip},
};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    Authentication,
    Denied,
    Resolution,
    Capacity,
    Timeout,
    Stale,
}
impl std::fmt::Display for PolicyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "target setup failed: {self:?}")
    }
}
impl std::error::Error for PolicyError {}

pub fn authenticate(identities: &[Identity], token: &[u8]) -> Result<usize, PolicyError> {
    if token.len() != 64
        || !token
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(PolicyError::Authentication);
    }
    let digest = Sha256::digest(token);
    let mut found = None;
    // No early return for a matching digest; compare every configured identity.
    for (index, identity) in identities.iter().enumerate() {
        if bool::from(identity.token_sha256.ct_eq(digest.as_slice())) {
            found = Some(index);
        }
    }
    found.ok_or(PolicyError::Authentication)
}

pub type ResolveFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SocketAddr>, PolicyError>> + 'a>>;
pub trait Resolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> ResolveFuture<'a>;
}

/// Slots stay captured by the actual blocking lookup even if its async caller
/// times out or is cancelled. Dropping a JoinHandle does not cancel getaddrinfo.
/// Thus repeated timeouts cannot create an unbounded queue of resolver jobs.
pub struct SystemResolver {
    slots: Arc<Semaphore>,
}
impl SystemResolver {
    pub fn new(maximum: usize) -> Result<Self, PolicyError> {
        if maximum == 0 || maximum > Semaphore::MAX_PERMITS {
            return Err(PolicyError::Capacity);
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(maximum)),
        })
    }
    fn dispatch<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<tokio::task::JoinHandle<T>, PolicyError> {
        let permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| PolicyError::Capacity)?;
        Ok(tokio::task::spawn_blocking(move || {
            let _permit = permit;
            work()
        }))
    }
    pub fn available(&self) -> usize {
        self.slots.available_permits()
    }
}
impl Resolver for SystemResolver {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> ResolveFuture<'a> {
        Box::pin(async move {
            if host.len() > 253 {
                return Err(PolicyError::Denied);
            }
            let host = host.to_owned();
            self.dispatch(move || {
                let answers: Vec<_> = (host.as_str(), port)
                    .to_socket_addrs()
                    .map_err(|_| PolicyError::Resolution)?
                    .take(129)
                    .collect();
                if answers.is_empty() || answers.len() > 128 {
                    return Err(PolicyError::Resolution);
                }
                Ok(answers)
            })?
            .await
            .map_err(|_| PolicyError::Resolution)?
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupKey {
    pub connection: Generation,
    pub session: Generation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTarget {
    pub address: SocketAddr,
    pub key: SetupKey,
}
impl ResolvedTarget {
    pub fn for_current(self, current: SetupKey) -> Result<SocketAddr, PolicyError> {
        if self.key != current {
            return Err(PolicyError::Stale);
        }
        Ok(self.address)
    }
}
pub fn valid_destination(ip: IpAddr) -> bool {
    valid_resolved_destination(SocketAddr::new(ip, 1))
}
fn valid_resolved_destination(address: SocketAddr) -> bool {
    let ip = normalize_ip(address.ip());
    !ip.is_unspecified()
        && !ip.is_multicast()
        && !matches!(ip, IpAddr::V4(v) if v.is_broadcast())
        && !matches!(address, SocketAddr::V6(v) if v.ip().is_unicast_link_local() && v.scope_id()==0)
}
pub fn authorize_answers(
    identity: &Identity,
    host: &str,
    port: u16,
    answers: &[SocketAddr],
) -> Result<SocketAddr, PolicyError> {
    let host = canonical_host(host).map_err(|_| PolicyError::Denied)?;
    let rule = identity
        .targets
        .iter()
        .find(|r| r.host == host && r.port == port)
        .ok_or(PolicyError::Denied)?;
    if port == 0 || answers.is_empty() || answers.len() > 128 {
        return Err(PolicyError::Denied);
    }
    let literal = host.parse::<IpAddr>().ok();
    let mut first = None;
    for answer in answers {
        let ip = normalize_ip(answer.ip());
        if answer.port() != port
            || !valid_resolved_destination(*answer)
            || literal.is_some_and(|v| v != ip)
            || !rule.addresses.iter().any(|range| range.contains(ip))
        {
            return Err(PolicyError::Denied);
        }
        first.get_or_insert(
            if matches!(answer,SocketAddr::V6(v) if v.ip().is_unicast_link_local()) {
                *answer
            } else {
                SocketAddr::new(ip, port)
            },
        );
    }
    first.ok_or(PolicyError::Denied)
}
pub async fn prepare<R: Resolver + ?Sized>(
    identities: &[Identity],
    token: &[u8],
    host: &str,
    port: u16,
    resolver: &R,
    key: SetupKey,
    timeout: Duration,
) -> Result<(usize, ResolvedTarget), PolicyError> {
    let index = authenticate(identities, token)?;
    let target =
        prepare_authenticated(&identities[index], host, port, resolver, key, timeout).await?;
    Ok((index, target))
}
/// Resolve only after the owner has authenticated this immutable configured
/// identity and debited its pending-session quota. No token is retained in DNS work.
pub async fn prepare_authenticated<R: Resolver + ?Sized>(
    identity: &Identity,
    host: &str,
    port: u16,
    resolver: &R,
    key: SetupKey,
    timeout: Duration,
) -> Result<ResolvedTarget, PolicyError> {
    let host = canonical_host(host).map_err(|_| PolicyError::Denied)?;
    // Exact permission is checked before DNS, including hostname aliases.
    if !identity
        .targets
        .iter()
        .any(|r| r.host == host && r.port == port)
    {
        return Err(PolicyError::Denied);
    }
    let answers = if let Ok(ip) = host.parse() {
        vec![SocketAddr::new(ip, port)]
    } else {
        tokio::time::timeout(timeout, resolver.resolve(&host, port))
            .await
            .map_err(|_| PolicyError::Timeout)??
    };
    let address = authorize_answers(identity, &host, port, &answers)?;
    Ok(ResolvedTarget { address, key })
}
#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn timed_out_and_cancelled_dns_keep_actual_worker_slots_until_completion() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            for timeout in [false, true] {
                let resolver = SystemResolver::new(1).unwrap();
                let (tx, rx) = std::sync::mpsc::channel();
                let worker = resolver.dispatch(move || rx.recv().unwrap()).unwrap();
                if timeout {
                    assert!(
                        tokio::time::timeout(Duration::from_millis(1), worker)
                            .await
                            .is_err()
                    );
                } else {
                    drop(worker);
                }
                assert_eq!(resolver.available(), 0);
                assert!(matches!(
                    resolver.dispatch(|| ()),
                    Err(PolicyError::Capacity)
                ));
                tx.send(()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(1);
                while resolver.available() == 0 {
                    assert!(Instant::now() < deadline);
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                assert_eq!(resolver.available(), 1);
            }
        });
    }
}
