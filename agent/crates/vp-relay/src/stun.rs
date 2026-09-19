//! STUN message codec (RFC 8489) with the TURN attributes of RFC 8656.

use hmac::{Hmac, Mac};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use subtle::ConstantTimeEq;

pub const MAGIC: u32 = 0x2112_A442;
pub const HEADER_LEN: usize = 20;
const FINGERPRINT_XOR: u32 = 0x5354_554E;

pub mod method {
    pub const BINDING: u16 = 0x001;
    pub const ALLOCATE: u16 = 0x003;
    pub const REFRESH: u16 = 0x004;
    pub const SEND: u16 = 0x006;
    pub const DATA: u16 = 0x007;
    pub const CREATE_PERMISSION: u16 = 0x008;
    pub const CHANNEL_BIND: u16 = 0x009;
}

pub mod attr {
    pub const MAPPED_ADDRESS: u16 = 0x0001;
    pub const USERNAME: u16 = 0x0006;
    pub const MESSAGE_INTEGRITY: u16 = 0x0008;
    pub const ERROR_CODE: u16 = 0x0009;
    pub const UNKNOWN_ATTRIBUTES: u16 = 0x000A;
    pub const CHANNEL_NUMBER: u16 = 0x000C;
    pub const LIFETIME: u16 = 0x000D;
    pub const XOR_PEER_ADDRESS: u16 = 0x0012;
    pub const DATA: u16 = 0x0013;
    pub const REALM: u16 = 0x0014;
    pub const NONCE: u16 = 0x0015;
    pub const XOR_RELAYED_ADDRESS: u16 = 0x0016;
    pub const REQUESTED_ADDRESS_FAMILY: u16 = 0x0017;
    pub const EVEN_PORT: u16 = 0x0018;
    pub const REQUESTED_TRANSPORT: u16 = 0x0019;
    pub const DONT_FRAGMENT: u16 = 0x001A;
    pub const MESSAGE_INTEGRITY_SHA256: u16 = 0x001C;
    pub const USERHASH: u16 = 0x001E;
    pub const XOR_MAPPED_ADDRESS: u16 = 0x0020;
    pub const RESERVATION_TOKEN: u16 = 0x0022;
    pub const PRIORITY: u16 = 0x0024;
    pub const USE_CANDIDATE: u16 = 0x0025;
    pub const SOFTWARE: u16 = 0x8022;
    pub const FINGERPRINT: u16 = 0x8028;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Request,
    Indication,
    Success,
    Error,
}

pub fn message_type(method: u16, class: Class) -> u16 {
    let c: u16 = match class {
        Class::Request => 0,
        Class::Indication => 1,
        Class::Success => 2,
        Class::Error => 3,
    };
    ((method & 0x0F80) << 2)
        | ((method & 0x0070) << 1)
        | (method & 0x000F)
        | ((c & 0x2) << 7)
        | ((c & 0x1) << 4)
}

fn split_type(t: u16) -> (u16, Class) {
    let method = (t & 0x000F) | ((t & 0x00E0) >> 1) | ((t & 0x3E00) >> 2);
    let class = match ((t >> 7) & 0x2) | ((t >> 4) & 0x1) {
        0 => Class::Request,
        1 => Class::Indication,
        2 => Class::Success,
        _ => Class::Error,
    };
    (method, class)
}

/// True when the buffer starts like a STUN message (the two top bits are zero).
pub fn looks_like_stun(buf: &[u8]) -> bool {
    buf.len() >= HEADER_LEN
        && buf[0] & 0xC0 == 0
        && u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) == MAGIC
}

#[derive(Debug, Clone, Copy)]
pub struct RawAttr<'a> {
    pub kind: u16,
    pub value: &'a [u8],
    /// Offset of the attribute header within the message.
    pub offset: usize,
}

#[derive(Debug)]
pub struct Message<'a> {
    pub method: u16,
    pub class: Class,
    pub txid: [u8; 12],
    pub raw: &'a [u8],
    pub attrs: Vec<RawAttr<'a>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    Short,
    NotStun,
    Length,
    Attribute,
}

impl<'a> Message<'a> {
    /// Parses exactly one message; the buffer length must match the header length field.
    pub fn parse(buf: &'a [u8]) -> Result<Message<'a>, ParseError> {
        if buf.len() < HEADER_LEN {
            return Err(ParseError::Short);
        }
        if !looks_like_stun(buf) {
            return Err(ParseError::NotStun);
        }
        let length = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        if !length.is_multiple_of(4) || HEADER_LEN + length != buf.len() {
            return Err(ParseError::Length);
        }
        let (method, class) = split_type(u16::from_be_bytes([buf[0], buf[1]]));
        let mut txid = [0u8; 12];
        txid.copy_from_slice(&buf[8..20]);
        let mut attrs = Vec::new();
        let mut offset = HEADER_LEN;
        while offset < buf.len() {
            if offset + 4 > buf.len() || attrs.len() >= 64 {
                return Err(ParseError::Attribute);
            }
            let kind = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
            let len = u16::from_be_bytes([buf[offset + 2], buf[offset + 3]]) as usize;
            let start = offset + 4;
            let end = start + len;
            let padded = start + len.div_ceil(4) * 4;
            if padded > buf.len() {
                return Err(ParseError::Attribute);
            }
            attrs.push(RawAttr {
                kind,
                value: &buf[start..end],
                offset,
            });
            offset = padded;
        }
        Ok(Message {
            method,
            class,
            txid,
            raw: buf,
            attrs,
        })
    }

    pub fn get(&self, kind: u16) -> Option<&'a [u8]> {
        self.attrs.iter().find(|a| a.kind == kind).map(|a| a.value)
    }

    pub fn all(&self, kind: u16) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.attrs
            .iter()
            .filter(move |a| a.kind == kind)
            .map(|a| a.value)
    }

    pub fn text(&self, kind: u16) -> Option<&'a str> {
        self.get(kind).and_then(|v| std::str::from_utf8(v).ok())
    }

    pub fn u32_attr(&self, kind: u16) -> Option<u32> {
        self.get(kind)
            .filter(|v| v.len() == 4)
            .map(|v| u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
    }

    pub fn xor_address(&self, kind: u16) -> Option<SocketAddr> {
        self.get(kind)
            .and_then(|v| decode_xor_address(v, &self.txid))
    }

    /// Verifies MESSAGE-INTEGRITY (HMAC-SHA1). Attributes after it other than FINGERPRINT are
    /// not covered, so their presence makes the message invalid.
    pub fn verify_integrity(&self, key: &[u8]) -> bool {
        let Some(index) = self
            .attrs
            .iter()
            .position(|a| a.kind == attr::MESSAGE_INTEGRITY)
        else {
            return false;
        };
        if self.attrs[index + 1..]
            .iter()
            .any(|a| a.kind != attr::FINGERPRINT)
        {
            return false;
        }
        let integrity = self.attrs[index];
        if integrity.value.len() != 20 {
            return false;
        }
        let expected = hmac_sha1(key, self.raw, integrity.offset, integrity.offset + 24);
        expected.ct_eq(integrity.value).into()
    }

    /// Checks FINGERPRINT when present. Absent fingerprints are accepted.
    pub fn fingerprint_ok(&self) -> bool {
        match self.attrs.iter().find(|a| a.kind == attr::FINGERPRINT) {
            None => true,
            Some(fp) => {
                fp.value.len() == 4
                    && fp.offset + 8 == self.raw.len()
                    && crc_fingerprint(self.raw, fp.offset)
                        == u32::from_be_bytes([fp.value[0], fp.value[1], fp.value[2], fp.value[3]])
            }
        }
    }
}

/// HMAC-SHA1 over `raw[..end]` with the header length patched to `covered_end - 20`.
fn hmac_sha1(key: &[u8], raw: &[u8], end: usize, covered_end: usize) -> [u8; 20] {
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(key).expect("hmac accepts any key");
    let mut header = [0u8; HEADER_LEN];
    header.copy_from_slice(&raw[..HEADER_LEN]);
    header[2..4].copy_from_slice(&((covered_end - HEADER_LEN) as u16).to_be_bytes());
    mac.update(&header);
    mac.update(&raw[HEADER_LEN..end]);
    mac.finalize().into_bytes().into()
}

fn crc_fingerprint(raw: &[u8], end: usize) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    let mut header = [0u8; HEADER_LEN];
    header.copy_from_slice(&raw[..HEADER_LEN]);
    header[2..4].copy_from_slice(&((end + 8 - HEADER_LEN) as u16).to_be_bytes());
    hasher.update(&header);
    hasher.update(&raw[HEADER_LEN..end]);
    hasher.finalize() ^ FINGERPRINT_XOR
}

/// Long-term credential key: MD5(username ":" realm ":" password).
pub fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(username.as_bytes());
    hasher.update(b":");
    hasher.update(realm.as_bytes());
    hasher.update(b":");
    hasher.update(password.as_bytes());
    hasher.finalize().into()
}

pub fn decode_xor_address(value: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if value.len() < 4 {
        return None;
    }
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC >> 16) as u16;
    let cookie = MAGIC.to_be_bytes();
    match value[1] {
        0x01 if value.len() == 8 => {
            let mut octets = [0u8; 4];
            for i in 0..4 {
                octets[i] = value[4 + i] ^ cookie[i];
            }
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port))
        }
        0x02 if value.len() == 20 => {
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&cookie);
            mask[4..].copy_from_slice(txid);
            let mut octets = [0u8; 16];
            for i in 0..16 {
                octets[i] = value[4 + i] ^ mask[i];
            }
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(octets)), port))
        }
        _ => None,
    }
}

pub fn encode_xor_address(addr: SocketAddr, txid: &[u8; 12]) -> Vec<u8> {
    let cookie = MAGIC.to_be_bytes();
    let port = addr.port() ^ (MAGIC >> 16) as u16;
    let mut out = Vec::with_capacity(20);
    out.push(0);
    match addr.ip() {
        IpAddr::V4(ip) => {
            out.push(0x01);
            out.extend_from_slice(&port.to_be_bytes());
            for (i, b) in ip.octets().iter().enumerate() {
                out.push(b ^ cookie[i]);
            }
        }
        IpAddr::V6(ip) => {
            out.push(0x02);
            out.extend_from_slice(&port.to_be_bytes());
            let mut mask = [0u8; 16];
            mask[..4].copy_from_slice(&cookie);
            mask[4..].copy_from_slice(txid);
            for (i, b) in ip.octets().iter().enumerate() {
                out.push(b ^ mask[i]);
            }
        }
    }
    out
}

pub struct Builder {
    buf: Vec<u8>,
    txid: [u8; 12],
}

impl Builder {
    pub fn new(method: u16, class: Class, txid: [u8; 12]) -> Builder {
        let mut buf = Vec::with_capacity(128);
        buf.extend_from_slice(&message_type(method, class).to_be_bytes());
        buf.extend_from_slice(&[0, 0]);
        buf.extend_from_slice(&MAGIC.to_be_bytes());
        buf.extend_from_slice(&txid);
        Builder { buf, txid }
    }

    fn set_length(&mut self, length: usize) {
        self.buf[2..4].copy_from_slice(&(length as u16).to_be_bytes());
    }

    pub fn attr(&mut self, kind: u16, value: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(&kind.to_be_bytes());
        self.buf
            .extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.buf.extend_from_slice(value);
        let padding = (4 - value.len() % 4) % 4;
        self.buf.extend(std::iter::repeat_n(0u8, padding));
        self
    }

    pub fn u32_attr(&mut self, kind: u16, value: u32) -> &mut Self {
        self.attr(kind, &value.to_be_bytes())
    }

    pub fn xor_address(&mut self, kind: u16, addr: SocketAddr) -> &mut Self {
        let value = encode_xor_address(addr, &self.txid);
        self.attr(kind, &value)
    }

    pub fn error(&mut self, code: u16, reason: &str) -> &mut Self {
        let mut value = vec![0, 0, (code / 100) as u8, (code % 100) as u8];
        value.extend_from_slice(reason.as_bytes());
        self.attr(attr::ERROR_CODE, &value)
    }

    pub fn integrity(&mut self, key: &[u8]) -> &mut Self {
        let end = self.buf.len();
        let mac = hmac_sha1(key, &self.buf, end, end + 24);
        self.attr(attr::MESSAGE_INTEGRITY, &mac)
    }

    pub fn fingerprint(&mut self) -> &mut Self {
        let end = self.buf.len();
        let crc = crc_fingerprint(&self.buf, end);
        self.attr(attr::FINGERPRINT, &crc.to_be_bytes())
    }

    pub fn finish(&mut self) -> Vec<u8> {
        let length = self.buf.len() - HEADER_LEN;
        self.set_length(length);
        std::mem::take(&mut self.buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).unwrap())
            .collect()
    }

    const PASSWORD: &[u8] = b"VOkJxbRl1RmTxUk/WvJxBt";

    // RFC 5769 §2.1
    fn sample_request() -> Vec<u8> {
        hex(
            "000100582112a442b7e7a701bc34d686fa87dfae802200105354554e207465737420636c69656e74
             002400046e0001ff80290008932ff9b151263b36000600096576746a3a68367659202020
             000800149aeaa70cbfd8cb56781ef2b5b2d3f249c1b571a280280004e57a3bcf",
        )
    }

    // RFC 5769 §2.2
    fn sample_response() -> Vec<u8> {
        hex(
            "0101003c2112a442b7e7a701bc34d686fa87dfae8022000b7465737420766563746f7220
             002000080001a147e112a643000800142b91f599fd9e90c38c7489f92af9ba53f06be7d7
             80280004c07d4c96",
        )
    }

    #[test]
    fn rfc5769_request_integrity_and_fingerprint() {
        let raw = sample_request();
        let message = Message::parse(&raw).unwrap();
        assert_eq!(message.method, method::BINDING);
        assert_eq!(message.class, Class::Request);
        assert_eq!(message.text(attr::USERNAME), Some("evtj:h6vY"));
        assert!(message.verify_integrity(PASSWORD));
        assert!(!message.verify_integrity(b"wrong"));
        assert!(message.fingerprint_ok());
    }

    #[test]
    fn rfc5769_response_address() {
        let raw = sample_response();
        let message = Message::parse(&raw).unwrap();
        assert_eq!(message.class, Class::Success);
        assert_eq!(
            message.xor_address(attr::XOR_MAPPED_ADDRESS),
            Some("192.0.2.1:32853".parse().unwrap())
        );
        assert!(message.verify_integrity(PASSWORD));
        assert!(message.fingerprint_ok());
    }

    #[test]
    fn builder_reproduces_rfc5769_response() {
        let original = sample_response();
        let txid: [u8; 12] = original[8..20].try_into().unwrap();
        let mut builder = Builder::new(method::BINDING, Class::Success, txid);
        builder.attr(attr::SOFTWARE, b"test vector");
        // RFC 5769 pads SOFTWARE with a space; our zero padding changes the MAC, so compare structure.
        builder.xor_address(attr::XOR_MAPPED_ADDRESS, "192.0.2.1:32853".parse().unwrap());
        builder.integrity(PASSWORD);
        builder.fingerprint();
        let built = builder.finish();
        assert_eq!(built.len(), original.len());
        let parsed = Message::parse(&built).unwrap();
        assert!(parsed.verify_integrity(PASSWORD));
        assert!(parsed.fingerprint_ok());
        // Identical up to SOFTWARE's padding byte (a space in the RFC, zero here), and the address.
        assert_eq!(&built[..0x23], &original[..0x23]);
        assert_eq!(&built[0x24..0x30], &original[0x24..0x30]);
    }

    #[test]
    fn type_encoding_roundtrip() {
        for m in [
            method::BINDING,
            method::ALLOCATE,
            method::REFRESH,
            method::SEND,
            method::DATA,
            method::CREATE_PERMISSION,
            method::CHANNEL_BIND,
        ] {
            for c in [
                Class::Request,
                Class::Indication,
                Class::Success,
                Class::Error,
            ] {
                assert_eq!(split_type(message_type(m, c)), (m, c));
            }
        }
        assert_eq!(message_type(method::ALLOCATE, Class::Success), 0x0103);
        assert_eq!(message_type(method::ALLOCATE, Class::Error), 0x0113);
        assert_eq!(message_type(method::SEND, Class::Indication), 0x0016);
        assert_eq!(message_type(method::DATA, Class::Indication), 0x0017);
    }

    #[test]
    fn ipv6_xor_roundtrip() {
        let txid = [7u8; 12];
        let addr: SocketAddr = "[2001:db8::1234]:443".parse().unwrap();
        assert_eq!(
            decode_xor_address(&encode_xor_address(addr, &txid), &txid),
            Some(addr)
        );
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(Message::parse(&[0u8; 8]).unwrap_err(), ParseError::Short);
        let mut raw = sample_request();
        raw[3] = 0x5c;
        assert_eq!(Message::parse(&raw).unwrap_err(), ParseError::Length);
        let mut raw = sample_request();
        raw[23] = 0xff; // SOFTWARE length overflows the message
        assert_eq!(Message::parse(&raw).unwrap_err(), ParseError::Attribute);
    }

    #[test]
    fn long_term_key_is_md5_of_credentials() {
        assert_eq!(
            long_term_key("alice", "vision-pro", "secret").to_vec(),
            hex("cb32b86e6dbf94124e9296295ac434d0")
        );
    }
}
