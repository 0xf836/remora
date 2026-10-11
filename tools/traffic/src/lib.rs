pub mod baseline;
pub mod measurement;
use serde::Serialize;
use std::collections::HashSet;
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
#[derive(Default, Serialize)]
pub struct Collection {
    pub unique: u64,
    pub bytes: u64,
    pub duplicates: u64,
    pub reordered: u64,
    pub corrupt: u64,
    pub wrong_direction: u64,
    pub overflow: u64,
    #[serde(skip)]
    seen: HashSet<u64>,
    #[serde(skip)]
    highest: Option<u64>,
}
impl Collection {
    pub fn observe(&mut self, bytes: &[u8], session: u64, direction: u8) {
        let Ok(header) = decode(bytes) else {
            self.corrupt += 1;
            return;
        };
        if header.session != session || header.direction != direction {
            self.wrong_direction += 1;
            return;
        }
        if header.sequence >= 1_000_000 || self.seen.len() >= 1_000_000 {
            self.overflow += 1;
            return;
        }
        if !self.seen.insert(header.sequence) {
            self.duplicates += 1;
            return;
        }
        if self.highest.is_some_and(|h| header.sequence < h) {
            self.reordered += 1;
        }
        self.highest = Some(
            self.highest
                .map_or(header.sequence, |old| old.max(header.sequence)),
        );
        self.unique += 1;
        self.bytes += bytes.len() as u64;
    }
}
