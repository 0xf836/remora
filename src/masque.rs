//! Bounded CONNECT-UDP codecs. No application payload uses the Capsule carrier.
use crate::config::canonical_host;
use quiche::h3::{Header, NameValue};
use std::collections::HashSet;

pub const H3_MESSAGE_ERROR: u64 = 0x10e;
pub const H3_DATAGRAM_ERROR: u64 = 0x33;
pub const MAX_VARINT: u64 = (1u64 << 62) - 1;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WireError {
    Message,
    Datagram,
    Status(u16),
    Capacity,
    Template,
}
impl WireError {
    pub fn code(self) -> Option<u64> {
        match self {
            Self::Message => Some(H3_MESSAGE_ERROR),
            Self::Datagram => Some(H3_DATAGRAM_ERROR),
            _ => None,
        }
    }
}
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MASQUE validation failed: {self:?}")
    }
}
impl std::error::Error for WireError {}

pub fn varint_len(value: u64) -> Result<usize, WireError> {
    match value {
        0..=63 => Ok(1),
        64..=16383 => Ok(2),
        16384..=1073741823 => Ok(4),
        1073741824..=MAX_VARINT => Ok(8),
        _ => Err(WireError::Datagram),
    }
}
pub fn put_varint(value: u64, out: &mut [u8]) -> Result<usize, WireError> {
    let n = varint_len(value)?;
    if out.len() < n {
        return Err(WireError::Capacity);
    }
    out[..n].copy_from_slice(&value.to_be_bytes()[8 - n..]);
    out[0] |= match n {
        1 => 0,
        2 => 0x40,
        4 => 0x80,
        _ => 0xc0,
    };
    Ok(n)
}
pub fn get_varint(input: &[u8]) -> Result<(u64, usize), WireError> {
    let first = *input.first().ok_or(WireError::Message)?;
    let n = 1usize << usize::from(first >> 6);
    if input.len() < n {
        return Err(WireError::Message);
    }
    let mut v = u64::from(first & 63);
    for b in &input[1..n] {
        v = (v << 8) | u64::from(*b);
    }
    Ok((v, n))
}
pub struct HttpDatagram<'a> {
    pub stream: u64,
    pub context: u64,
    pub payload: &'a [u8],
}
pub fn decode_datagram(bytes: &[u8]) -> Result<Option<HttpDatagram<'_>>, WireError> {
    let (quarter, n) = get_varint(bytes).map_err(|_| WireError::Datagram)?;
    let stream = quarter
        .checked_mul(4)
        .filter(|v| *v <= MAX_VARINT)
        .ok_or(WireError::Datagram)?;
    let Ok((context, m)) = get_varint(&bytes[n..]) else {
        return Ok(None);
    };
    let payload = &bytes[n + m..];
    if context == 0 && payload.len() > 65527 {
        return Err(WireError::Datagram);
    }
    Ok(Some(HttpDatagram {
        stream,
        context,
        payload,
    }))
}
pub fn encode_datagram(stream: u64, payload: &[u8], out: &mut [u8]) -> Result<usize, WireError> {
    if !stream.is_multiple_of(4) || stream > MAX_VARINT || payload.len() > 65527 {
        return Err(WireError::Datagram);
    }
    let n = put_varint(stream / 4, out)?;
    let total = n
        .checked_add(1)
        .and_then(|v| v.checked_add(payload.len()))
        .ok_or(WireError::Capacity)?;
    if out.len() < total {
        return Err(WireError::Capacity);
    }
    out[n] = 0;
    out[n + 1..total].copy_from_slice(payload);
    Ok(total)
}
fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len() * 3);
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(char::from(b));
        } else {
            const HEX: &[u8] = b"0123456789ABCDEF";
            out.push('%');
            out.push(char::from(HEX[(b >> 4) as usize]));
            out.push(char::from(HEX[(b & 15) as usize]));
        }
    }
    out
}
fn percent_decode(value: &str) -> Result<String, WireError> {
    let mut out = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(WireError::Status(400));
            }
            let hex = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            let v = hex(bytes[i + 1])
                .zip(hex(bytes[i + 2]))
                .map(|(a, b)| a * 16 + b)
                .ok_or(WireError::Status(400))?;
            i += 3;
            v
        } else {
            let v = bytes[i];
            i += 1;
            v
        };
        if b == b'%' || b <= 32 || b >= 127 || b == b'/' || b == b'\\' {
            return Err(WireError::Status(400));
        }
        out.push(b);
    }
    String::from_utf8(out).map_err(|_| WireError::Status(400))
}
/// Supported level-1/3 expressions: simple target variables and ?/& query
/// expansion. Unknown/unbound variables and level-4 modifiers fail at startup.
pub struct UriTemplate {
    path: String,
}
impl UriTemplate {
    pub fn parse(input: &str, authority: &str) -> Result<Self, WireError> {
        if input.len() > 4096
            || !input.bytes().all(|b| (0x21..=0x7e).contains(&b))
            || input.contains(['#', '\\'])
            || authority.contains(['@', '{', '}'])
        {
            return Err(WireError::Template);
        }
        let (scheme, rest) = input.split_once("://").ok_or(WireError::Template)?;
        if !scheme.eq_ignore_ascii_case("https") {
            return Err(WireError::Template);
        }
        let (host, path) = rest.split_once('/').ok_or(WireError::Template)?;
        if host != authority || host.is_empty() {
            return Err(WireError::Template);
        }
        let path = format!("/{path}");
        let mut seen = [false; 2];
        let mut pos = 0;
        while let Some(offset) = path[pos..].find('{') {
            let start = pos + offset;
            if path[pos..start].contains('}') {
                return Err(WireError::Template);
            }
            let end = start + path[start..].find('}').ok_or(WireError::Template)?;
            let expr = &path[start + 1..end];
            let vars = expr.strip_prefix(['?', '&']).unwrap_or(expr);
            for variable in vars.split(',') {
                let slot = match variable {
                    "target_host" => 0,
                    "target_port" => 1,
                    _ => return Err(WireError::Template),
                };
                seen[slot] = true;
            }
            pos = end + 1;
        }
        if !seen.iter().all(|v| *v) || path[pos..].contains('}') {
            return Err(WireError::Template);
        }
        // Literal percent escapes must be syntactically complete; expansion
        // always escapes substitutions exactly once.
        let bytes = path.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                if i + 2 >= bytes.len() || !bytes[i + 1..i + 3].iter().all(u8::is_ascii_hexdigit) {
                    return Err(WireError::Template);
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        Ok(Self { path })
    }
    pub fn expand(&self, host: &str, port: u16) -> Result<String, WireError> {
        if port == 0 {
            return Err(WireError::Template);
        }
        let host = percent_encode(&canonical_host(host).map_err(|_| WireError::Template)?);
        let port = port.to_string();
        let mut out = String::with_capacity(self.path.len() + host.len());
        let mut pos = 0;
        while let Some(off) = self.path[pos..].find('{') {
            let start = pos + off;
            out.push_str(&self.path[pos..start]);
            let end = start + self.path[start..].find('}').ok_or(WireError::Template)?;
            let expr = &self.path[start + 1..end];
            let op = expr
                .as_bytes()
                .first()
                .copied()
                .filter(|c| *c == b'?' || *c == b'&');
            let vars = if op.is_some() { &expr[1..] } else { expr };
            if let Some(op) = op {
                out.push(char::from(op));
            }
            for (index, var) in vars.split(',').enumerate() {
                if index > 0 {
                    out.push(if op.is_some() { '&' } else { ',' });
                }
                if op.is_some() {
                    out.push_str(var);
                    out.push('=');
                }
                out.push_str(if var == "target_host" { &host } else { &port });
            }
            pos = end + 1;
        }
        out.push_str(&self.path[pos..]);
        if out.len() > 8192 {
            return Err(WireError::Capacity);
        }
        Ok(out)
    }
}
pub fn parse_target(path: &str, prefix: &str) -> Result<(String, u16), WireError> {
    if path.len() > 8192 || path.contains(['?', '#']) {
        return Err(WireError::Status(400));
    }
    let rest = path.strip_prefix(prefix).ok_or(WireError::Status(404))?;
    let rest = rest.strip_suffix('/').ok_or(WireError::Status(400))?;
    let (host, port) = rest.split_once('/').ok_or(WireError::Status(400))?;
    if host.len() > 765 || port.len() > 15 || port.contains('/') {
        return Err(WireError::Status(400));
    }
    let host = canonical_host(&percent_decode(host)?).map_err(|_| WireError::Status(400))?;
    let port = percent_decode(port)?;
    let parsed = port.parse::<u16>().map_err(|_| WireError::Status(400))?;
    if parsed == 0 || port != parsed.to_string() {
        return Err(WireError::Status(400));
    }
    Ok((host, parsed))
}
fn validate_headers(headers: &[Header], request: bool) -> Result<(), WireError> {
    if headers.len() > 32 {
        return Err(WireError::Message);
    }
    let mut bytes = 0usize;
    let mut pseudo = HashSet::new();
    let mut ordinary = false;
    for h in headers {
        bytes = bytes
            .checked_add(h.name().len() + h.value().len())
            .ok_or(WireError::Message)?;
        if bytes > 16384
            || h.name().is_empty()
            || h.name().iter().skip(1).any(|b| *b == b':')
            || !h.name().iter().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || b"!#$%&'*+-.^_`|~:".contains(b)
            })
            || h.value()
                .iter()
                .any(|b| (*b < 0x20 && *b != b'\t') || *b == 0x7f)
            || h.value().first().is_some_and(|b| matches!(b, b' ' | b'\t'))
            || h.value().last().is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            return Err(WireError::Message);
        }
        // RFC 9114 permits TE only in a request, with the trailers token.
        if h.name() == b"te" && (!request || !h.value().eq_ignore_ascii_case(b"trailers")) {
            return Err(WireError::Message);
        }
        if h.name().starts_with(b":") {
            if ordinary || !pseudo.insert(h.name()) {
                return Err(WireError::Message);
            }
        } else {
            ordinary = true;
        }
        if [
            b"connection".as_slice(),
            b"keep-alive",
            b"proxy-connection",
            b"upgrade",
        ]
        .contains(&h.name())
        {
            return Err(WireError::Message);
        }
    }
    Ok(())
}
fn field<'a>(headers: &'a [Header], name: &[u8]) -> Option<&'a [u8]> {
    headers.iter().find(|h| h.name() == name).map(|h| h.value())
}
fn forbidden_capsule_fields(headers: &[Header]) -> bool {
    headers.iter().any(|h| {
        [
            b"content-length".as_slice(),
            b"content-type",
            b"transfer-encoding",
        ]
        .contains(&h.name())
    })
}
/// Negotiation is advisory for CONNECT-UDP: absence/non-Boolean/duplicates
/// cannot turn an otherwise valid success into a private capability failure.
pub fn capsule_protocol(headers: &[Header]) -> Option<bool> {
    let mut fields = headers.iter().filter(|h| h.name() == b"capsule-protocol");
    let value = fields.next()?.value();
    if fields.next().is_some() || value.len() > 16384 {
        return None;
    }
    let value = value.strip_prefix(b" ").unwrap_or(value);
    let mut at = 0;
    if value.get(at) != Some(&b'?') {
        return None;
    }
    at += 1;
    let boolean = match value.get(at) {
        Some(b'1') => true,
        Some(b'0') => false,
        _ => return None,
    };
    at += 1;
    // RFC 8941 parameters: parse their syntax and ignore their values. No
    // parameter can enable application forwarding or allocate declared data.
    while at < value.len() {
        if value[at] == b' ' {
            if value[at..].iter().all(|b| *b == b' ') {
                break;
            }
            return None;
        }
        if value[at] != b';' {
            return None;
        }
        at += 1;
        while value.get(at) == Some(&b' ') {
            at += 1;
        }
        let key = *value.get(at)?;
        if !key.is_ascii_lowercase() && key != b'*' {
            return None;
        }
        at += 1;
        while value
            .get(at)
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.*-".contains(b))
        {
            at += 1;
        }
        if value.get(at) == Some(&b'=') {
            at += 1;
            at += structured_item(&value[at..])?;
        }
    }
    Some(boolean)
}
fn structured_item(bytes: &[u8]) -> Option<usize> {
    let first = *bytes.first()?;
    if first == b'?' {
        return matches!(bytes.get(1), Some(b'0' | b'1')).then_some(2);
    }
    if first == b'"' {
        let mut at = 1;
        while let Some(&b) = bytes.get(at) {
            at += 1;
            if b == b'"' {
                return Some(at);
            }
            if b == b'\\' {
                if !matches!(bytes.get(at), Some(b'"' | b'\\')) {
                    return None;
                }
                at += 1;
            } else if !(0x20..=0x7e).contains(&b) {
                return None;
            }
        }
        return None;
    }
    if first == b':' {
        let mut at = 1;
        while let Some(&b) = bytes.get(at) {
            at += 1;
            if b == b':' {
                return Some(at);
            }
            if !b.is_ascii_alphanumeric() && !b"+/=".contains(&b) {
                return None;
            }
        }
        return None;
    }
    if first == b'-' || first.is_ascii_digit() {
        let mut at = usize::from(first == b'-');
        let start = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        let digits = at - start;
        if digits == 0 {
            return None;
        }
        if bytes.get(at) == Some(&b'.') {
            if digits > 12 {
                return None;
            }
            at += 1;
            let start = at;
            while bytes.get(at).is_some_and(u8::is_ascii_digit) {
                at += 1;
            }
            if !(1..=3).contains(&(at - start)) {
                return None;
            }
        } else if digits > 15 {
            return None;
        }
        return Some(at);
    }
    if first.is_ascii_alphabetic() || first == b'*' {
        let mut at = 1;
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~:/".contains(b))
        {
            at += 1;
        }
        return Some(at);
    }
    None
}
pub struct Request {
    pub host: String,
    pub port: u16,
    pub token: [u8; 64],
}
pub fn request(headers: &[Header], authority: &str, prefix: &str) -> Result<Request, WireError> {
    validate_headers(headers, true)?;
    if forbidden_capsule_fields(headers) {
        return Err(WireError::Message);
    }
    if headers.iter().any(|h| {
        h.name().starts_with(b":")
            && ![
                b":method".as_slice(),
                b":protocol",
                b":scheme",
                b":authority",
                b":path",
            ]
            .contains(&h.name())
    }) {
        return Err(WireError::Message);
    }
    let mut hosts = headers.iter().filter(|h| h.name() == b"host");
    if let Some(host) = hosts.next()
        && (hosts.next().is_some()
            || field(headers, b":authority")
                .is_none_or(|value| value.is_empty() || host.value() != value))
    {
        return Err(WireError::Message);
    }
    if field(headers, b":method") != Some(b"CONNECT".as_slice()) {
        return Err(WireError::Status(405));
    }
    if field(headers, b":protocol") != Some(b"connect-udp".as_slice())
        || field(headers, b":scheme") != Some(b"https".as_slice())
        || field(headers, b":authority") != Some(authority.as_bytes())
    {
        return Err(WireError::Status(400));
    }
    let path = std::str::from_utf8(field(headers, b":path").ok_or(WireError::Status(400))?)
        .map_err(|_| WireError::Status(400))?;
    let (host, port) = parse_target(path, prefix)?;
    if headers
        .iter()
        .filter(|h| h.name() == b"authorization")
        .count()
        != 1
    {
        return Err(WireError::Status(401));
    }
    let auth = field(headers, b"authorization").ok_or(WireError::Status(401))?;
    if auth.len() != 71 || !auth[..7].eq_ignore_ascii_case(b"Bearer ") {
        return Err(WireError::Status(401));
    }
    let token: [u8; 64] = auth[7..].try_into().map_err(|_| WireError::Status(401))?;
    if !token
        .iter()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(WireError::Status(401));
    }
    Ok(Request { host, port, token })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    Informational,
    Accepted,
    Rejected(u16),
}
pub fn response(headers: &[Header], closed: bool) -> Result<Response, WireError> {
    validate_headers(headers, false)?;
    if headers
        .iter()
        .any(|h| h.name().starts_with(b":") && h.name() != b":status")
    {
        return Err(WireError::Message);
    }
    let status = field(headers, b":status").ok_or(WireError::Message)?;
    if status.len() != 3 || !status.iter().all(u8::is_ascii_digit) {
        return Err(WireError::Message);
    }
    let status = (u16::from(status[0] - b'0') * 100)
        + (u16::from(status[1] - b'0') * 10)
        + u16::from(status[2] - b'0');
    if !(100..=599).contains(&status) || status == 101 {
        return Err(WireError::Message);
    }
    if status < 200 {
        return Ok(Response::Informational);
    }
    if status >= 300 {
        return Ok(Response::Rejected(status));
    }
    if [204, 205, 206].contains(&status) || forbidden_capsule_fields(headers) {
        return Err(WireError::Message);
    }
    if closed {
        return Err(WireError::Message);
    }
    Ok(Response::Accepted)
}
pub fn request_headers(authority: &str, path: &str, token: &[u8; 64]) -> Vec<Header> {
    let mut auth = Vec::with_capacity(71);
    auth.extend_from_slice(b"Bearer ");
    auth.extend_from_slice(token);
    vec![
        Header::new(b":method", b"CONNECT"),
        Header::new(b":protocol", b"connect-udp"),
        Header::new(b":scheme", b"https"),
        Header::new(b":authority", authority.as_bytes()),
        Header::new(b":path", path.as_bytes()),
        Header::new(b"authorization", &auth),
        Header::new(b"capsule-protocol", b"?1"),
    ]
}
pub fn response_headers(status: u16) -> Vec<Header> {
    let mut headers = vec![Header::new(b":status", status.to_string().as_bytes())];
    if (200..300).contains(&status) {
        headers.push(Header::new(b"capsule-protocol", b"?1"));
    } else if status == 401 {
        headers.push(Header::new(b"www-authenticate", b"Bearer"));
    }
    headers
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Type,
    Length(u64),
    Context(u64),
    Skip(u64, Discard),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Discard {
    Unsupported,
    Context,
    Unknown,
}
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapsuleCounts {
    pub unsupported: u64,
    pub unknown_context: u64,
}
pub struct CapsuleParser {
    phase: Phase,
    prefix: [u8; 8],
    used: usize,
    counts: CapsuleCounts,
    failed: bool,
}
impl Default for CapsuleParser {
    fn default() -> Self {
        Self {
            phase: Phase::Type,
            prefix: [0; 8],
            used: 0,
            counts: CapsuleCounts::default(),
            failed: false,
        }
    }
}
impl CapsuleParser {
    pub fn retained_bytes(&self) -> usize {
        self.used
    }
    fn count(&mut self, reason: Discard) -> Result<(), WireError> {
        let slot = match reason {
            Discard::Unsupported => &mut self.counts.unsupported,
            Discard::Context => &mut self.counts.unknown_context,
            Discard::Unknown => return Ok(()),
        };
        *slot = slot.checked_add(1).ok_or(WireError::Capacity)?;
        Ok(())
    }
    fn consume(&mut self, mut input: &[u8], fin: bool) -> Result<CapsuleCounts, WireError> {
        loop {
            if let Phase::Skip(remaining, why) = self.phase {
                let take = usize::try_from(remaining)
                    .unwrap_or(usize::MAX)
                    .min(input.len());
                input = &input[take..];
                let remaining = remaining - take as u64;
                if remaining == 0 {
                    self.count(why)?;
                    self.phase = Phase::Type;
                } else {
                    self.phase = Phase::Skip(remaining, why);
                }
                if input.is_empty() {
                    break;
                }
                continue;
            }
            if input.is_empty() {
                break;
            }
            let byte = input[0];
            input = &input[1..];
            if let Phase::Context(remaining) = self.phase {
                if remaining == 0 {
                    return Err(WireError::Message);
                }
                self.phase = Phase::Context(remaining - 1);
            }
            self.prefix[self.used] = byte;
            self.used += 1;
            let need = 1usize << usize::from(self.prefix[0] >> 6);
            if self.used < need {
                if self.phase == Phase::Context(0) {
                    return Err(WireError::Message);
                }
                continue;
            }
            let (value, _) = get_varint(&self.prefix[..need])?;
            self.used = 0;
            self.phase = match self.phase {
                Phase::Type => Phase::Length(value),
                Phase::Length(0) => {
                    if value == 0 {
                        return Err(WireError::Message);
                    }
                    Phase::Context(value)
                }
                Phase::Length(_) => Phase::Skip(value, Discard::Unknown),
                Phase::Context(remaining) => {
                    if value == 0 && remaining > 65527 {
                        return Err(WireError::Datagram);
                    }
                    Phase::Skip(
                        remaining,
                        if value == 0 {
                            Discard::Unsupported
                        } else {
                            Discard::Context
                        },
                    )
                }
                Phase::Skip(..) => unreachable!(),
            };
            if let Phase::Skip(0, why) = self.phase {
                self.count(why)?;
                self.phase = Phase::Type;
            }
        }
        if fin && (self.phase != Phase::Type || self.used != 0) {
            return Err(WireError::Message);
        }
        Ok(self.counts)
    }
    pub fn feed(&mut self, input: &[u8], fin: bool) -> Result<CapsuleCounts, WireError> {
        if self.failed {
            return Err(WireError::Message);
        }
        let result = self.consume(input, fin);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
