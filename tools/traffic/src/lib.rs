pub mod baseline;
pub mod measurement;
use serde::Serialize;

pub const PREFIX: usize = 40;
fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5u32, |v, b| {
        v.wrapping_mul(0x1000193) ^ u32::from(*b)
    })
}
pub fn packet(
    size: usize,
    session: u64,
    direction: u8,
    sequence: u64,
    stamp: u64,
) -> Result<Vec<u8>, &'static str> {
    if !(PREFIX..=1200).contains(&size) || direction > 2 {
        return Err("invalid payload profile");
    }
    let mut out = vec![0x5a; size];
    out[..4].copy_from_slice(b"RMTR");
    out[4] = direction;
    out[8..16].copy_from_slice(&session.to_be_bytes());
    out[16..24].copy_from_slice(&sequence.to_be_bytes());
    out[24..32].copy_from_slice(&stamp.to_be_bytes());
    let hash = checksum(&out[..size - 4]);
    out[size - 4..].copy_from_slice(&hash.to_be_bytes());
    Ok(out)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub session: u64,
    pub direction: u8,
    pub sequence: u64,
    pub stamp: u64,
}
pub fn decode(bytes: &[u8]) -> Result<Header, &'static str> {
    if bytes.len() < PREFIX || &bytes[..4] != b"RMTR" || bytes[4] > 2 {
        return Err("invalid traffic header");
    }
    if checksum(&bytes[..bytes.len() - 4])
        != u32::from_be_bytes(bytes[bytes.len() - 4..].try_into().unwrap())
    {
        return Err("integrity failure");
    }
    Ok(Header {
        session: u64::from_be_bytes(bytes[8..16].try_into().unwrap()),
        direction: bytes[4],
        sequence: u64::from_be_bytes(bytes[16..24].try_into().unwrap()),
        stamp: u64::from_be_bytes(bytes[24..32].try_into().unwrap()),
    })
}
#[derive(Serialize)]
pub struct Collection {
    pub unique: u64,
    pub bytes: u64,
    pub duplicates: u64,
    pub reordered: u64,
    pub corrupt: u64,
    pub wrong_direction: u64,
    pub overflow: u64,
    pub one_way_delay_min_us: Option<u64>,
    pub one_way_delay_max_us: u64,
    pub latency_samples: u64,
    pub clock_errors: u64,
    #[serde(skip)]
    seen: Vec<u64>,
    #[serde(skip)]
    sequence_limit: u64,
    #[serde(skip)]
    highest: Option<u64>,
}
impl Default for Collection {
    fn default() -> Self {
        Self::with_limit(1_000_000).expect("bounded default")
    }
}
impl Collection {
    /// Exact bounded bitmap: release workloads need hundreds of millions of
    /// sequences, not an unbounded HashSet or silent million-packet truncation.
    pub fn with_limit(limit: u64) -> Result<Self, &'static str> {
        if limit == 0 || limit > 400_000_000 {
            return Err("invalid sequence bound");
        }
        Ok(Self {
            unique: 0,
            bytes: 0,
            duplicates: 0,
            reordered: 0,
            corrupt: 0,
            wrong_direction: 0,
            overflow: 0,
            one_way_delay_min_us: None,
            one_way_delay_max_us: 0,
            latency_samples: 0,
            clock_errors: 0,
            seen: Vec::new(),
            highest: None,
            sequence_limit: limit,
        })
    }
    pub fn observe(&mut self, bytes: &[u8], session: u64, direction: u8) {
        let _ = self.observe_checked(bytes, session, direction);
    }
    /// Shared-clock test tooling only. Production payloads are never timestamped.
    pub fn observe_at(&mut self, bytes: &[u8], session: u64, direction: u8, received_us: u64) {
        if let Some(header) = self.observe_checked(bytes, session, direction) {
            if let Some(delay) = received_us.checked_sub(header.stamp) {
                self.latency_samples += 1;
                self.one_way_delay_min_us = Some(
                    self.one_way_delay_min_us
                        .map_or(delay, |old| old.min(delay)),
                );
                self.one_way_delay_max_us = self.one_way_delay_max_us.max(delay);
            } else {
                self.clock_errors += 1;
            }
        }
    }
    /// Unique deliveries grouped by original offered sequence slots, including
    /// delayed/reordered packets received during drain in their original slot.
    pub fn sequence_seconds(&self, rate: u64, seconds: usize) -> Result<Vec<u64>, &'static str> {
        if seconds > 1800 {
            return Err("bounded interval exceeded");
        }
        let mut counts = vec![0; seconds];
        if rate == 0 {
            return Ok(counts);
        }
        for (word_index, &word) in self.seen.iter().enumerate() {
            let mut remaining = word;
            while remaining != 0 {
                let seq = word_index as u64 * 64 + u64::from(remaining.trailing_zeros());
                if let Some(slot) = counts.get_mut((seq / rate) as usize) {
                    *slot += 1;
                }
                remaining &= remaining - 1;
            }
        }
        Ok(counts)
    }
    pub fn sequence_intervals(&self, rates: &[u64]) -> Result<Vec<u64>, &'static str> {
        if rates.len() > 1800 {
            return Err("bounded interval exceeded");
        }
        let mut total = 0u64;
        let mut ends = Vec::new();
        for &rate in rates {
            total = total.checked_add(rate).ok_or("interval overflow")?;
            ends.push(total);
        }
        let mut counts = vec![0; rates.len()];
        for (word_index, &word) in self.seen.iter().enumerate() {
            let mut remaining = word;
            while remaining != 0 {
                let seq = word_index as u64 * 64 + u64::from(remaining.trailing_zeros());
                if let Some(slot) = counts.get_mut(ends.partition_point(|end| *end <= seq)) {
                    *slot += 1;
                }
                remaining &= remaining - 1;
            }
        }
        Ok(counts)
    }
    fn observe_checked(&mut self, bytes: &[u8], session: u64, direction: u8) -> Option<Header> {
        let Ok(header) = decode(bytes) else {
            self.corrupt += 1;
            return None;
        };
        if header.session != session || header.direction != direction {
            self.wrong_direction += 1;
            return None;
        }
        if header.sequence >= self.sequence_limit {
            self.overflow += 1;
            return None;
        }
        let word = (header.sequence / 64) as usize;
        if word >= self.seen.len() {
            if self
                .seen
                .try_reserve_exact(word + 1 - self.seen.len())
                .is_err()
            {
                self.overflow += 1;
                return None;
            }
            self.seen.resize(word + 1, 0);
        }
        let bit = 1u64 << (header.sequence % 64);
        if self.seen[word] & bit != 0 {
            self.duplicates += 1;
            return None;
        }
        self.seen[word] |= bit;
        if self.highest.is_some_and(|h| header.sequence < h) {
            self.reordered += 1;
        }
        self.highest = Some(
            self.highest
                .map_or(header.sequence, |old| old.max(header.sequence)),
        );
        self.unique += 1;
        self.bytes += bytes.len() as u64;
        Some(header)
    }
}
