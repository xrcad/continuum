//! Session code encoding and decoding.
//!
//! A session code is a compact URL-safe base64 string containing enough information for
//! two peers to connect without any server involvement:
//!
//! ```text
//! xrcad:<base64url( session_id[16] | addr_flag[1] | ip[4 or 16] | port[2] | version[1] )>
//! ```
//!
//! The code can be embedded in a shareable link:
//! ```text
//! https://xrcad.app/join/<code>
//! ```
//!
//! The WASM build of xrcad handles the `/join/<code>` path — no server logic required.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::{SessionId, error::NetError};
use uuid::Uuid;

const PREFIX:          &str = "xrcad:";
const VERSION:         u8   = 1;
const FLAG_IPV4:       u8   = 4;
const FLAG_IPV6:       u8   = 6;

/// Encode a session endpoint into a shareable session code string.
///
/// # Errors
/// Returns [`NetError::SessionCode`] if encoding fails (in practice this should not happen
/// for valid inputs).
pub fn encode(session_id: SessionId, endpoint: SocketAddr) -> String {
    let mut buf = Vec::with_capacity(34);

    // 16 bytes: session UUID
    buf.extend_from_slice(session_id.0.as_bytes());

    // 1 + 4 or 16 bytes: IP address with flag
    match endpoint.ip() {
        IpAddr::V4(v4) => {
            buf.push(FLAG_IPV4);
            buf.extend_from_slice(&v4.octets());
        }
        IpAddr::V6(v6) => {
            buf.push(FLAG_IPV6);
            buf.extend_from_slice(&v6.octets());
        }
    }

    // 2 bytes: port (big-endian)
    buf.extend_from_slice(&endpoint.port().to_be_bytes());

    // 1 byte: protocol version
    buf.push(VERSION);

    format!("{}{}", PREFIX, base64_url_encode(&buf))
}

/// Decode a session code into a session ID and socket address.
///
/// # Errors
/// Returns [`NetError::SessionCode`] with a description if the code is malformed.
pub fn decode(code: &str) -> Result<(SessionId, SocketAddr), NetError> {
    let inner = code
        .strip_prefix(PREFIX)
        .ok_or_else(|| NetError::SessionCode("missing 'xrcad:' prefix".into()))?;

    let bytes = base64_url_decode(inner)
        .map_err(|e| NetError::SessionCode(format!("base64 decode failed: {e}")))?;

    let mut cursor = 0usize;

    // 16 bytes: session UUID
    if bytes.len() < cursor + 16 {
        return Err(NetError::SessionCode("truncated: missing session ID".into()));
    }
    let session_id = SessionId(Uuid::from_bytes(
        bytes[cursor..cursor + 16].try_into().unwrap(),
    ));
    cursor += 16;

    // 1 byte: address family flag
    if bytes.len() < cursor + 1 {
        return Err(NetError::SessionCode("truncated: missing address flag".into()));
    }
    let flag = bytes[cursor];
    cursor += 1;

    // IP address
    let ip: IpAddr = match flag {
        FLAG_IPV4 => {
            if bytes.len() < cursor + 4 {
                return Err(NetError::SessionCode("truncated: missing IPv4 address".into()));
            }
            let octets: [u8; 4] = bytes[cursor..cursor + 4].try_into().unwrap();
            cursor += 4;
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        FLAG_IPV6 => {
            if bytes.len() < cursor + 16 {
                return Err(NetError::SessionCode("truncated: missing IPv6 address".into()));
            }
            let octets: [u8; 16] = bytes[cursor..cursor + 16].try_into().unwrap();
            cursor += 16;
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        other => {
            return Err(NetError::SessionCode(format!(
                "unknown address flag: {other}"
            )));
        }
    };

    // 2 bytes: port
    if bytes.len() < cursor + 2 {
        return Err(NetError::SessionCode("truncated: missing port".into()));
    }
    let port = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]);
    cursor += 2;

    // 1 byte: version (checked for future compatibility, not enforced yet)
    if bytes.len() < cursor + 1 {
        return Err(NetError::SessionCode("truncated: missing version byte".into()));
    }
    let _version = bytes[cursor];

    Ok((session_id, SocketAddr::new(ip, port)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Minimal URL-safe base64 (no-std-compatible, no extra dep)
// ─────────────────────────────────────────────────────────────────────────────

fn base64_url_encode(input: &[u8]) -> String {
    // Use the alphabet: A-Z a-z 0-9 - _  (URL-safe, no padding)
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    let mut chunks = input.chunks_exact(3);
    for chunk in &mut chunks {
        let b = (chunk[0] as u32) << 16 | (chunk[1] as u32) << 8 | chunk[2] as u32;
        out.push(ALPHABET[(b >> 18) as usize] as char);
        out.push(ALPHABET[(b >> 12 & 0x3f) as usize] as char);
        out.push(ALPHABET[(b >> 6 & 0x3f) as usize] as char);
        out.push(ALPHABET[(b & 0x3f) as usize] as char);
    }
    match chunks.remainder() {
        [a] => {
            let b = (*a as u32) << 16;
            out.push(ALPHABET[(b >> 18) as usize] as char);
            out.push(ALPHABET[(b >> 12 & 0x3f) as usize] as char);
        }
        [a, b] => {
            let v = (*a as u32) << 16 | (*b as u32) << 8;
            out.push(ALPHABET[(v >> 18) as usize] as char);
            out.push(ALPHABET[(v >> 12 & 0x3f) as usize] as char);
            out.push(ALPHABET[(v >> 6 & 0x3f) as usize] as char);
        }
        _ => {}
    }
    out
}

fn base64_url_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let decode_char = |c: u8| -> Result<u8, &'static str> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'-' => Ok(62),
            b'_' => Ok(63),
            _ => Err("invalid base64url character"),
        }
    };

    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 3 < bytes.len() {
        let a = decode_char(bytes[i])?;
        let b = decode_char(bytes[i + 1])?;
        let c = decode_char(bytes[i + 2])?;
        let d = decode_char(bytes[i + 3])?;
        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
        out.push((c << 6) | d);
        i += 4;
    }
    match bytes.len() - i {
        2 => {
            let a = decode_char(bytes[i])?;
            let b = decode_char(bytes[i + 1])?;
            out.push((a << 2) | (b >> 4));
        }
        3 => {
            let a = decode_char(bytes[i])?;
            let b = decode_char(bytes[i + 1])?;
            let c = decode_char(bytes[i + 2])?;
            out.push((a << 2) | (b >> 4));
            out.push((b << 4) | (c >> 2));
        }
        _ => {}
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

    #[test]
    fn roundtrip_ipv4() {
        let session = SessionId::generate();
        let addr    = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42)), 7890);
        let code    = encode(session, addr);
        assert!(code.starts_with("xrcad:"));
        let (s2, a2) = decode(&code).expect("decode failed");
        assert_eq!(session, s2);
        assert_eq!(addr, a2);
    }

    #[test]
    fn roundtrip_ipv6() {
        let session = SessionId::generate();
        let addr    = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 7890);
        let code    = encode(session, addr);
        let (s2, a2) = decode(&code).expect("decode failed");
        assert_eq!(session, s2);
        assert_eq!(addr, a2);
    }

    #[test]
    fn rejects_missing_prefix() {
        assert!(decode("notaxrcadcode").is_err());
    }
}
