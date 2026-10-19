//! MTProxy client transport: obfuscated2 + intermediate/padded intermediate + Fake TLS.
//! Local atarustia extension, MIT OR Apache-2.0 (same as this vendored crate).
use aes::Aes256;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use ctr::cipher::{KeyIvInit, StreamCipher};
use grammers_crypto::DequeBuffer;
use grammers_mtproto::transport::{self, Transport, UnpackedOffset};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::{
    fmt, io,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use zeroize::{Zeroize, Zeroizing};

type Cipher = ctr::Ctr128BE<Aes256>;
type HmacSha256 = Hmac<Sha256>;
const MAX_PACKET: usize = 1024 * 1024 + 4096;
const TLS_CHUNK: usize = 2878;
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Abridged,
    Intermediate,
    Padded,
    FakeTls,
}
#[derive(Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    key: Zeroizing<[u8; 16]>,
    pub mode: Mode,
    pub domain: Option<String>,
}
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MtProxy")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("mode", &self.mode)
            .field("secret", &"[REDACTED]")
            .finish()
    }
}
impl Config {
    /// Canonical mtproxy://host:port?secret=..., tg://proxy and https://t.me/proxy links.
    pub fn parse(input: &str) -> io::Result<Self> {
        if input.len() > 4096 {
            return Err(invalid("proxy configuration too long"));
        }
        let url = url::Url::parse(input).map_err(|_| invalid("invalid proxy URL"))?;
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(invalid("MTProxy URL must not contain userinfo or fragment"));
        }
        let native = url.scheme() == "mtproxy";
        let telegram = (url.scheme() == "tg"
            && url.host_str() == Some("proxy")
            && matches!(url.path(), "" | "/"))
            || (url.scheme() == "https"
                && matches!(url.host_str(), Some("t.me" | "telegram.me"))
                && url.path() == "/proxy"
                && url.port().is_none());
        if !native && !telegram {
            return Err(invalid(
                "expected mtproxy://, tg://proxy or https://t.me/proxy",
            ));
        }
        if native && !matches!(url.path(), "" | "/") {
            return Err(invalid("MTProxy URL cannot contain a path"));
        }
        let mut secret = None;
        let mut server = None;
        let mut port = None;
        let mut transport = None;
        for (name, value) in url.query_pairs() {
            let slot = match name.as_ref() {
                "secret" => &mut secret,
                "server" if telegram => &mut server,
                "port" if telegram => &mut port,
                "transport" => &mut transport,
                _ => return Err(invalid("unknown MTProxy URL parameter")),
            };
            if slot.replace(value.into_owned()).is_some() {
                return Err(invalid("duplicate MTProxy URL parameter"));
            }
        }
        let host = if native {
            url.host_str()
                .ok_or_else(|| invalid("missing MTProxy host"))?
                .trim_matches(['[', ']'])
                .to_owned()
        } else {
            server
                .ok_or_else(|| invalid("missing MTProxy server"))?
                .trim_matches(['[', ']'])
                .to_owned()
        };
        let port = if native {
            url.port().ok_or_else(|| invalid("missing MTProxy port"))?
        } else {
            port.ok_or_else(|| invalid("missing MTProxy port"))?
                .parse()
                .map_err(|_| invalid("invalid MTProxy port"))?
        };
        if port == 0
            || host.is_empty()
            || host.len() > 253
            || host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '?' | '#' | '@'))
        {
            return Err(invalid("invalid MTProxy host or port"));
        }
        let encoded = Zeroizing::new(
            secret
                .ok_or_else(|| invalid("missing MTProxy secret"))?
                .replace(' ', "+"),
        );
        let (key, mut mode, domain) = parse_secret(&encoded)?;
        if let Some(transport) = transport {
            if mode != Mode::Intermediate {
                return Err(invalid(
                    "transport selection requires a classic 16-byte secret",
                ));
            }
            mode = match transport.as_str() {
                "abridged" => Mode::Abridged,
                "intermediate" => Mode::Intermediate,
                _ => {
                    return Err(invalid(
                        "MTProxy transport must be abridged or intermediate",
                    ));
                }
            };
        }
        Ok(Self {
            host,
            port,
            key: Zeroizing::new(key),
            mode,
            domain,
        })
    }
    pub(crate) fn key(&self) -> &[u8; 16] {
        &self.key
    }
}
fn parse_secret(value: &str) -> io::Result<([u8; 16], Mode, Option<String>)> {
    let bytes = Zeroizing::new(
        if value.len().is_multiple_of(2) && value.bytes().all(|b| b.is_ascii_hexdigit()) {
            (0..value.len())
                .step_by(2)
                .map(|n| {
                    u8::from_str_radix(&value[n..n + 2], 16)
                        .map_err(|_| invalid("invalid MTProxy secret"))
                })
                .collect::<io::Result<Vec<_>>>()?
        } else {
            [URL_SAFE_NO_PAD, URL_SAFE, STANDARD_NO_PAD, STANDARD]
                .iter()
                .find_map(|engine| engine.decode(value).ok())
                .ok_or_else(|| invalid("invalid MTProxy secret encoding"))?
        },
    );
    let (key, mode, domain) = match bytes.as_slice() {
        key if key.len() == 16 => (key, Mode::Intermediate, None),
        [0xdd, key @ ..] if key.len() == 16 => (key, Mode::Padded, None),
        [0xee, rest @ ..] if rest.len() > 16 => {
            let domain = std::str::from_utf8(&rest[16..])
                .map_err(|_| invalid("Fake TLS domain must be ASCII DNS name"))?;
            if domain.len() > 253
                || !domain.is_ascii()
                || !domain.split('.').all(|s| {
                    !s.is_empty()
                        && s.len() <= 63
                        && !s.starts_with('-')
                        && !s.ends_with('-')
                        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
            {
                return Err(invalid("invalid Fake TLS SNI domain"));
            }
            (&rest[..16], Mode::FakeTls, Some(domain.to_owned()))
        }
        _ => {
            return Err(invalid(
                "MTProxy secret must be 16 bytes, dd+16 bytes, or ee+16 bytes+domain",
            ));
        }
    };
    Ok((key.try_into().expect("validated key length"), mode, domain))
}

pub(crate) enum PoolTransport {
    Full(transport::Full),
    Abridged,
    Proxy { padded: bool },
}
impl PoolTransport {
    pub(crate) fn new(config: Option<&Config>) -> Self {
        match config {
            Some(c) if c.mode == Mode::Abridged => Self::Abridged,
            Some(c) => Self::Proxy {
                padded: c.mode != Mode::Intermediate,
            },
            None => Self::Full(transport::Full::new()),
        }
    }
}
impl Transport for PoolTransport {
    fn pack(&mut self, buffer: &mut DequeBuffer<u8>) {
        match self {
            Self::Full(full) => full.pack(buffer),
            Self::Abridged => {
                assert_eq!(buffer.len() % 4, 0);
                let words = buffer.len() / 4;
                assert!(words < 1 << 24);
                if words < 127 {
                    buffer.extend_front(&[words as u8]);
                } else {
                    buffer.extend_front(&[
                        127,
                        words as u8,
                        (words >> 8) as u8,
                        (words >> 16) as u8,
                    ]);
                }
            }
            Self::Proxy { padded } => {
                assert_eq!(buffer.len() % 4, 0);
                if *padded {
                    let mut random = [0; 16];
                    rand::rngs::OsRng.fill_bytes(&mut random);
                    let len = usize::from(random[0] & 15);
                    buffer.extend(&random[1..1 + len]);
                }
                let len = buffer.len();
                buffer.extend_front(&(len as u32).to_le_bytes());
            }
        }
    }
    fn unpack(&mut self, buffer: &mut [u8]) -> Result<UnpackedOffset, transport::Error> {
        match self {
            Self::Full(full) => full.unpack(buffer),
            Self::Abridged => {
                if buffer.is_empty() {
                    return Err(transport::Error::MissingBytes);
                }
                let (words, prefix) = if buffer[0] < 127 {
                    (usize::from(buffer[0]), 1)
                } else if buffer[0] == 127 {
                    if buffer.len() < 4 {
                        return Err(transport::Error::MissingBytes);
                    }
                    (
                        usize::from(buffer[1])
                            | (usize::from(buffer[2]) << 8)
                            | (usize::from(buffer[3]) << 16),
                        4,
                    )
                } else {
                    return Err(transport::Error::BadLen {
                        got: i32::from(buffer[0]),
                    });
                };
                let len = words * 4;
                if len == 0 || len > MAX_PACKET {
                    return Err(transport::Error::BadLen { got: len as i32 });
                }
                if buffer.len() < prefix + len {
                    return Err(transport::Error::MissingBytes);
                }
                if len == 4 {
                    let status = i32::from_le_bytes(buffer[prefix..prefix + 4].try_into().unwrap());
                    if status < 0 {
                        return Err(transport::Error::BadStatus {
                            status: status.unsigned_abs(),
                        });
                    }
                }
                Ok(UnpackedOffset {
                    data_range: prefix..prefix + len,
                    next_offset: prefix + len,
                })
            }
            Self::Proxy { padded } => {
                if buffer.len() < 4 {
                    return Err(transport::Error::MissingBytes);
                }
                let len = i32::from_le_bytes(buffer[..4].try_into().unwrap());
                if len <= 0 {
                    return Err(transport::Error::BadLen { got: len });
                }
                if len as usize > MAX_PACKET {
                    return Err(transport::Error::BadLen { got: len });
                }
                if buffer.len() < 4 + len as usize {
                    return Err(transport::Error::MissingBytes);
                }
                if (4..=19).contains(&len) {
                    let status = i32::from_le_bytes(buffer[4..8].try_into().unwrap());
                    if status < 0 {
                        return Err(transport::Error::BadStatus {
                            status: status.unsigned_abs(),
                        });
                    }
                }
                if !*padded && len % 4 != 0 {
                    return Err(transport::Error::BadLen { got: len });
                }
                // Padded transport adds 0–15 bytes. Encrypted MTProto is 24+16*n,
                // plain auth-key exchange has an explicit body length at offset 16.
                let payload = if *padded {
                    if len < 20 {
                        return Err(transport::Error::BadLen { got: len });
                    }
                    if buffer[4..12] == [0; 8] {
                        let body = i32::from_le_bytes(buffer[20..24].try_into().unwrap());
                        if body <= 0 || body % 4 != 0 {
                            return Err(transport::Error::BadLen { got: body });
                        }
                        let size = 20 + body as usize;
                        if size > len as usize || len as usize - size > 15 {
                            return Err(transport::Error::BadLen { got: len });
                        }
                        size
                    } else {
                        if len < 40 {
                            return Err(transport::Error::BadLen { got: len });
                        }
                        24 + ((len as usize - 24) & !15)
                    }
                } else {
                    len as usize
                };
                if payload < 8 {
                    return Err(transport::Error::BadLen { got: len });
                }
                Ok(UnpackedOffset {
                    data_range: 4..4 + payload,
                    next_offset: 4 + len as usize,
                })
            }
        }
    }
}

/// Return wire header and continuous ciphers. Header consumes the first 64 outgoing CTR bytes.
#[cfg(test)]
fn header(key: &[u8; 16], dc: i16, padded: bool, nonce: [u8; 64]) -> ([u8; 64], Cipher, Cipher) {
    header_tag(key, dc, [if padded { 0xdd } else { 0xee }; 4], nonce)
}
fn header_tag(
    key: &[u8; 16],
    dc: i16,
    tag: [u8; 4],
    mut nonce: [u8; 64],
) -> ([u8; 64], Cipher, Cipher) {
    let mut reverse = nonce[8..56].to_vec();
    reverse.reverse();
    let out_key = Zeroizing::new(Sha256::digest([&nonce[8..40], key].concat()).to_vec());
    let in_key = Zeroizing::new(Sha256::digest([&reverse[..32], key].concat()).to_vec());
    let mut encrypt = Cipher::new_from_slices(&out_key, &nonce[40..56]).unwrap();
    let decrypt = Cipher::new_from_slices(&in_key, &reverse[32..]).unwrap();
    nonce[56..60].copy_from_slice(&tag);
    nonce[60..62].copy_from_slice(&dc.to_le_bytes());
    let mut encrypted = nonce;
    encrypt.apply_keystream(&mut encrypted);
    nonce[56..64].copy_from_slice(&encrypted[56..64]);
    reverse.zeroize();
    encrypted.zeroize();
    (nonce, encrypt, decrypt)
}
fn good_nonce(n: &[u8; 64]) -> bool {
    n[0] != 0xef
        && n[4..8] != [0; 4]
        && !matches!(
            &n[..4],
            b"HEAD"
                | b"POST"
                | b"GET "
                | b"PVrG"
                | b"\xee\xee\xee\xee"
                | b"\xdd\xdd\xdd\xdd"
                | b"\x16\x03\x01\x02"
        )
}
pub(crate) async fn connect(
    config: &Config,
    dc: i16,
) -> io::Result<(tokio::io::DuplexStream, tokio::task::JoinHandle<()>)> {
    tokio::time::timeout(Duration::from_secs(15), async {
        if dc == 0 || dc.unsigned_abs() > 5 {
            return Err(invalid("invalid MTProxy datacenter"));
        }
        let mut socket = TcpStream::connect((config.host.as_str(), config.port)).await?;
        socket.set_nodelay(true)?;
        let tls = config.mode == Mode::FakeTls;
        if tls {
            fake_tls_handshake(&mut socket, config).await?;
        }
        let mut nonce = [0; 64];
        loop {
            rand::rngs::OsRng.fill_bytes(&mut nonce);
            if good_nonce(&nonce) {
                break;
            }
        }
        let tag = match config.mode {
            Mode::Abridged => 0xef,
            Mode::Intermediate => 0xee,
            Mode::Padded | Mode::FakeTls => 0xdd,
        };
        let (header, encrypt, decrypt) = header_tag(config.key(), dc, [tag; 4], nonce);
        if tls {
            socket.write_all(b"\x14\x03\x03\x00\x01\x01").await?;
            write_record(&mut socket, &header).await?;
        } else {
            socket.write_all(&header).await?;
        }
        let (client, bridge) = tokio::io::duplex(64 * 1024);
        let task = tokio::spawn(async move {
            let (mut local_r, mut local_w) = tokio::io::split(bridge);
            let (mut remote_r, mut remote_w) = socket.into_split();
            let outgoing = pump_out(&mut local_r, &mut remote_w, encrypt, tls);
            let incoming = pump_in(&mut remote_r, &mut local_w, decrypt, tls);
            tokio::select! {_ = outgoing=>{},_ = incoming=>{}}
        });
        Ok((client, task))
    })
    .await
    .map_err(|_| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "MTProxy connect/handshake timed out",
        )
    })?
}
async fn pump_out<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    mut cipher: Cipher,
    tls: bool,
) -> io::Result<()> {
    let mut buffer = Zeroizing::new(vec![0; 16384]);
    loop {
        let n = reader.read(&mut buffer).await?;
        if n == 0 {
            return writer.shutdown().await;
        }
        cipher.apply_keystream(&mut buffer[..n]);
        if tls {
            for chunk in buffer[..n].chunks(TLS_CHUNK) {
                write_record(writer, chunk).await?;
            }
        } else {
            writer.write_all(&buffer[..n]).await?;
        }
    }
}
async fn pump_in<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    reader: &mut R,
    writer: &mut W,
    mut cipher: Cipher,
    tls: bool,
) -> io::Result<()> {
    let mut buffer = Zeroizing::new(vec![0; 65535]);
    loop {
        let n = if tls {
            let (kind, data) = read_record(reader).await?;
            if kind != 23 {
                return Err(invalid("unexpected Fake TLS record"));
            }
            buffer[..data.len()].copy_from_slice(&data);
            data.len()
        } else {
            reader.read(&mut buffer).await?
        };
        if n == 0 {
            if tls {
                continue;
            }
            return writer.shutdown().await;
        }
        cipher.apply_keystream(&mut buffer[..n]);
        writer.write_all(&buffer[..n]).await?;
    }
}
async fn write_record<W: AsyncWrite + Unpin>(writer: &mut W, data: &[u8]) -> io::Result<()> {
    writer
        .write_all(&[23, 3, 3, (data.len() >> 8) as u8, data.len() as u8])
        .await?;
    writer.write_all(data).await
}
async fn read_record<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0; 5];
    reader.read_exact(&mut header).await?;
    if header[1..3] != [3, 3] {
        return Err(invalid("invalid Fake TLS record version"));
    }
    let len = u16::from_be_bytes([header[3], header[4]]) as usize;
    let mut data = vec![0; len];
    reader.read_exact(&mut data).await?;
    Ok((header[0], data))
}
fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(data);
    mac.finalize().into_bytes().into()
}
fn extension(out: &mut Vec<u8>, kind: u16, data: &[u8]) {
    out.extend(kind.to_be_bytes());
    out.extend((data.len() as u16).to_be_bytes());
    out.extend(data);
}
fn client_hello(key: &[u8; 16], domain: &str, now: u32) -> (Vec<u8>, [u8; 32]) {
    let mut extensions = vec![];
    let mut sni = vec![];
    sni.extend(((domain.len() + 3) as u16).to_be_bytes());
    sni.push(0);
    sni.extend((domain.len() as u16).to_be_bytes());
    sni.extend(domain.as_bytes());
    extension(&mut extensions, 0, &sni);
    extension(&mut extensions, 10, &[0, 6, 0, 29, 0, 23, 0, 24]);
    extension(&mut extensions, 11, &[1, 0]);
    extension(&mut extensions, 13, &[0, 8, 8, 4, 4, 3, 8, 5, 5, 3]);
    extension(&mut extensions, 43, &[4, 3, 4, 3, 3]);
    extension(&mut extensions, 45, &[1, 1]);
    let mut private = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut private);
    let public = x25519_dalek::x25519(private, x25519_dalek::X25519_BASEPOINT_BYTES);
    private.zeroize();
    let mut share = vec![0, 36, 0, 29, 0, 32];
    share.extend(public);
    extension(&mut extensions, 51, &share);
    extension(&mut extensions, 16, b"\x00\x0c\x02h2\x08http/1.1");
    extension(&mut extensions, 23, &[]);
    extension(&mut extensions, 65281, &[0]);
    let mut body = vec![3, 3];
    body.extend([0; 32]);
    body.push(32);
    let mut session = [0; 32];
    rand::rngs::OsRng.fill_bytes(&mut session);
    body.extend(session);
    body.extend([0, 8, 0x13, 1, 0x13, 2, 0xc0, 0x2f, 0xc0, 0x30, 1, 0]);
    let padding = 517usize.saturating_sub(5 + 4 + body.len() + 2 + extensions.len() + 4);
    extension(&mut extensions, 21, &vec![0; padding]);
    body.extend((extensions.len() as u16).to_be_bytes());
    body.extend(extensions);
    let mut hello = vec![22, 3, 1];
    hello.extend(((body.len() + 4) as u16).to_be_bytes());
    hello.extend([1, 0, ((body.len() >> 8) & 255) as u8, body.len() as u8]);
    hello.extend(body);
    let mut digest = hmac(key, &hello);
    for (byte, time) in digest[28..].iter_mut().zip(now.to_le_bytes()) {
        *byte ^= time;
    }
    hello[11..43].copy_from_slice(&digest);
    (hello, digest)
}
async fn fake_tls_handshake(stream: &mut TcpStream, config: &Config) -> io::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| invalid("system clock before Unix epoch"))?
        .as_secs() as u32;
    let (hello, digest) = client_hello(
        config.key(),
        config
            .domain
            .as_deref()
            .ok_or_else(|| invalid("missing Fake TLS domain"))?,
        now,
    );
    stream.write_all(&hello).await?;
    let mut response = vec![];
    for expected in [22, 20, 23] {
        let (kind, data) = read_record(stream).await?;
        if kind != expected || response.len() + 5 + data.len() > 65536 {
            return Err(invalid("invalid Fake TLS server handshake"));
        }
        if expected == 22 && (data.len() < 38 || data[0] != 2) {
            return Err(invalid("short Fake TLS server hello"));
        }
        if expected == 20 && data != [1] {
            return Err(invalid("invalid Fake TLS change cipher spec"));
        }
        response.extend([kind, 3, 3, (data.len() >> 8) as u8, data.len() as u8]);
        response.extend(data);
    }
    verify_server(config.key(), &digest, &response)
}
fn verify_server(key: &[u8; 16], client_random: &[u8; 32], response: &[u8]) -> io::Result<()> {
    if response.len() < 43 || response[5] != 2 {
        return Err(invalid("invalid Fake TLS server hello"));
    }
    let mut message = Vec::with_capacity(32 + response.len());
    message.extend(client_random);
    message.extend(response);
    message[43..75].fill(0);
    let mut mac = HmacSha256::new_from_slice(key).unwrap();
    mac.update(&message);
    mac.verify_slice(&response[11..43])
        .map_err(|_| invalid("Fake TLS server authentication failed"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
    fn config(mode: Mode, port: u16) -> Config {
        Config {
            host: "127.0.0.1".into(),
            port,
            key: Zeroizing::new([7; 16]),
            mode,
            domain: if mode == Mode::FakeTls {
                Some("example.com".into())
            } else {
                None
            },
        }
    }
    #[test]
    fn python_header_and_continuous_ctr_vectors() {
        for (padded, expected) in [
            (
                false,
                "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738b33d0479cb902755",
            ),
            (
                true,
                "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738800e374acb902755",
            ),
        ] {
            let (wire, mut enc, mut dec) = header(
                &std::array::from_fn(|i| i as u8),
                2,
                padded,
                std::array::from_fn(|i| (i + 1) as u8),
            );
            assert_eq!(hex(&wire), expected);
            let mut data: Vec<u8> = (0..80).collect();
            enc.apply_keystream(&mut data);
            assert_eq!(
                hex(&data),
                "f4f3e3ac40fa93716d32d046a0efbc8b66b40c18941127966db49c764e6859fa19880d95192cd86f67c2b191ff8a450f7ee6e2feadb726722b0f349f046505b0a155fe2b1d0208b691b510b92cceff5a"
            );
            let mut data: Vec<u8> = (0..80).collect();
            dec.apply_keystream(&mut data[..3]);
            dec.apply_keystream(&mut data[3..]);
            assert_eq!(
                hex(&data),
                "61416a68f3cca870d67562f8a1e44019e218969c85a719f314d8235b6211a0eae5ae467bef887f72ef8641369a73ff0b3a6111dd28deb722d6a1dc5167a44181508afdd71100e28ae1083bfedb14de35"
            );
        }
    }
    #[test]
    fn urls_secrets_and_redacted_debug() {
        for (secret, mode) in [
            ("07".repeat(16), Mode::Intermediate),
            (format!("dd{}", "07".repeat(16)), Mode::Padded),
            (
                format!("ee{}6578616d706c652e636f6d", "07".repeat(16)),
                Mode::FakeTls,
            ),
        ] {
            for url in [
                format!("mtproxy://localhost:443?secret={secret}"),
                format!("tg://proxy?server=localhost&port=443&secret={secret}"),
                format!("https://t.me/proxy?server=localhost&port=443&secret={secret}"),
            ] {
                let parsed = Config::parse(&url).unwrap();
                assert_eq!(parsed.mode, mode);
                assert_eq!(parsed.key(), &[7; 16]);
                assert!(!format!("{parsed:?}").contains(&secret));
            }
        }
        let raw = [&[0xee][..], &[7; 16], b"example.com"].concat();
        let b64 = URL_SAFE_NO_PAD.encode(raw);
        assert_eq!(
            Config::parse(&format!("mtproxy://[::1]:443?secret={b64}"))
                .unwrap()
                .mode,
            Mode::FakeTls
        );
        for bad in [
            "mtproxy://host:0?secret=00",
            "mtproxy://host:443?secret=ee",
            "mtproxy://host:443?secret=07070707070707070707070707070707&secret=00",
            "tg://proxy?server=host&port=443&secret=00&x=1",
            "https://evil.example/proxy?server=host&port=443&secret=00",
            "mtproxy://user:pass@host:443?secret=00",
        ] {
            assert!(Config::parse(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn transport_fragmentation_padding_and_error_bounds() {
        for padded in [false, true] {
            let mut transport = PoolTransport::Proxy { padded };
            let mut buffer = DequeBuffer::with_capacity(136, 0);
            buffer.extend([1; 136]);
            transport.pack(&mut buffer);
            let bytes = buffer[..].to_vec();
            for n in 0..bytes.len() {
                let mut partial = bytes[..n].to_vec();
                assert_eq!(
                    transport.unpack(&mut partial),
                    Err(transport::Error::MissingBytes)
                );
            }
            let offset = transport.unpack(&mut buffer[..]).unwrap();
            assert_eq!(offset.next_offset, bytes.len());
            assert_eq!(&buffer[offset.data_range], &[1; 136]);
            let mut status = [4i32.to_le_bytes(), (-404i32).to_le_bytes()].concat();
            assert_eq!(
                transport.unpack(&mut status),
                Err(transport::Error::BadStatus { status: 404 })
            );
            assert!(transport.unpack(&mut (i32::MAX).to_le_bytes()).is_err());
        }
    }
    // Independent server-side direction derivation: decrypt the client's transmitted header.
    fn server_ciphers(wire: &[u8; 64], key: &[u8; 16], dc: i16, mode: Mode) -> (Cipher, Cipher) {
        let forward = Sha256::digest([&wire[8..40], key].concat());
        let mut incoming = Cipher::new_from_slices(&forward, &wire[40..56]).unwrap();
        let mut plain = *wire;
        incoming.apply_keystream(&mut plain);
        let tag = match mode {
            Mode::Abridged => 0xef,
            Mode::Intermediate => 0xee,
            _ => 0xdd,
        };
        assert_eq!(&plain[56..60], &[tag; 4]);
        assert_eq!(&plain[60..62], &dc.to_le_bytes());
        let reverse: Vec<_> = wire[8..56].iter().copied().rev().collect();
        let back = Sha256::digest([&reverse[..32], key].concat());
        let outgoing = Cipher::new_from_slices(&back, &reverse[32..]).unwrap();
        (incoming, outgoing)
    }
    async fn server_tls(stream: &mut TcpStream, key: &[u8; 16]) -> [u8; 32] {
        let mut prefix = [0; 5];
        stream.read_exact(&mut prefix).await.unwrap();
        assert_eq!(&prefix[..3], &[22, 3, 1]);
        let mut body = vec![0; u16::from_be_bytes([prefix[3], prefix[4]]) as usize];
        stream.read_exact(&mut body).await.unwrap();
        let mut hello = prefix.to_vec();
        hello.extend(body);
        assert!(hello.windows(11).any(|w| w == b"example.com"));
        let random: [u8; 32] = hello[11..43].try_into().unwrap();
        hello[11..43].fill(0);
        let digest = hmac(key, &hello);
        assert_eq!(&digest[..28], &random[..28]);
        let stamp = u32::from_le_bytes(std::array::from_fn(|i| digest[28 + i] ^ random[28 + i]));
        assert!(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .abs_diff(u64::from(stamp))
                < 10
        );
        let mut server_body = vec![2, 0, 0, 38, 3, 3];
        server_body.extend([0; 32]);
        server_body.extend([0, 0x13, 1, 0]);
        let mut response = vec![22, 3, 3, 0, server_body.len() as u8];
        response.extend(server_body);
        response.extend(b"\x14\x03\x03\x00\x01\x01\x17\x03\x03\x00\x03abc");
        let mac = hmac(key, &[&random[..], &response].concat());
        response[11..43].copy_from_slice(&mac);
        for chunk in response.chunks(3) {
            stream.write_all(chunk).await.unwrap();
        }
        let (kind, data) = read_record(stream).await.unwrap();
        assert_eq!((kind, data), (20, vec![1]));
        random
    }
    #[tokio::test]
    async fn loopback_plain_dd_and_fake_tls_streams_fragmented_bidirectional() {
        for mode in [
            Mode::Abridged,
            Mode::Intermediate,
            Mode::Padded,
            Mode::FakeTls,
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let tls = mode == Mode::FakeTls;
                if tls {
                    server_tls(&mut socket, &[7; 16]).await;
                }
                let wire: [u8; 64] = if tls {
                    let (kind, data) = read_record(&mut socket).await.unwrap();
                    assert_eq!(kind, 23);
                    data.try_into().unwrap()
                } else {
                    let mut wire = [0; 64];
                    socket.read_exact(&mut wire).await.unwrap();
                    wire
                };
                let (mut incoming, mut outgoing) = server_ciphers(&wire, &[7; 16], 3, mode);
                let mut received = vec![];
                while received.len() < 40000 {
                    let mut data = if tls {
                        let (_, data) = read_record(&mut socket).await.unwrap();
                        data
                    } else {
                        let mut data = vec![0; 997];
                        let n = socket.read(&mut data).await.unwrap();
                        data.truncate(n);
                        data
                    };
                    assert!(!data.is_empty());
                    incoming.apply_keystream(&mut data);
                    received.extend(data);
                }
                assert_eq!(received, vec![42; 40000]);
                outgoing.apply_keystream(&mut received);
                for chunk in received.chunks(613) {
                    if tls {
                        write_record(&mut socket, chunk).await.unwrap();
                    } else {
                        socket.write_all(chunk).await.unwrap();
                    }
                }
                let mut byte = [0];
                let _ = socket.read(&mut byte).await;
            });
            let (mut stream, task) = connect(&config(mode, port), 3).await.unwrap();
            stream.write_all(&vec![42; 40000]).await.unwrap();
            let mut response = vec![0; 40000];
            tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response, vec![42; 40000]);
            task.abort();
            drop(stream);
            server.await.unwrap();
        }
    }
    #[tokio::test]
    async fn fake_tls_invalid_server_fails_without_obfuscated_header() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut h = [0; 5];
            s.read_exact(&mut h).await.unwrap();
            let mut body = vec![0; u16::from_be_bytes([h[3], h[4]]) as usize];
            s.read_exact(&mut body).await.unwrap();
            s.write_all(b"\x16\x03\x03\x00\x00\x14\x03\x03\x00\x01\x01\x17\x03\x03\x00\x00")
                .await
                .unwrap();
            let mut next = [0];
            match s.read(&mut next).await {
                Ok(0) => {}
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                other => panic!("unexpected data after rejected handshake: {other:?}"),
            }
        });
        assert!(connect(&config(Mode::FakeTls, port), 2).await.is_err());
        server.await.unwrap();
    }
    #[test]
    fn fake_tls_authentication_rejects_modified_server_random() {
        let mut response = vec![0; 43];
        response[5] = 2;
        let digest = [1; 32];
        let mac = hmac(&[7; 16], &[&digest[..], &response].concat());
        response[11..43].copy_from_slice(&mac);
        verify_server(&[7; 16], &digest, &response).unwrap();
        response[12] ^= 1;
        assert!(verify_server(&[7; 16], &digest, &response).is_err());
    }

    #[test]
    fn all_padding_lengths_strip_exact_plain_and_encrypted_payloads() {
        for padding in 0..16 {
            let mut transport = PoolTransport::Proxy { padded: true };
            let mut plain = vec![0; 20];
            plain[8..16].copy_from_slice(&1i64.to_le_bytes());
            plain[16..20].copy_from_slice(&16i32.to_le_bytes());
            plain.extend([3; 16]);
            for payload in [plain, vec![7; 24 + 16 * 5]] {
                let mut packet = ((payload.len() + padding) as i32).to_le_bytes().to_vec();
                packet.extend(&payload);
                packet.extend(vec![0xab; padding]);
                let offset = transport.unpack(&mut packet).unwrap();
                assert_eq!(&packet[offset.data_range], &payload);
                assert_eq!(offset.next_offset, packet.len());
            }
            let mut packet = ((4 + padding) as i32).to_le_bytes().to_vec();
            packet.extend((-404i32).to_le_bytes());
            packet.extend(vec![0xab; padding]);
            assert_eq!(
                transport.unpack(&mut packet),
                Err(transport::Error::BadStatus { status: 404 })
            );
        }
    }

    #[test]
    fn direct_full_transport_is_unchanged_and_nonce_filters_reserved_prefixes() {
        let mut original = transport::Full::new();
        let mut pool = PoolTransport::new(None);
        for size in [40, 56, 1024] {
            let mut a = DequeBuffer::with_capacity(size, 0);
            a.extend(vec![7; size]);
            let mut b = a.clone();
            original.pack(&mut a);
            pool.pack(&mut b);
            assert_eq!(&a[..], &b[..]);
        }
        for prefix in [
            b"HEAD",
            b"POST",
            b"GET ",
            b"PVrG",
            b"\xee\xee\xee\xee",
            b"\xdd\xdd\xdd\xdd",
        ] {
            let mut nonce = [1; 64];
            nonce[..4].copy_from_slice(prefix);
            assert!(!good_nonce(&nonce));
        }
        let mut nonce = [1; 64];
        nonce[0] = 0xef;
        assert!(!good_nonce(&nonce));
        nonce[0] = 1;
        nonce[4..8].fill(0);
        assert!(!good_nonce(&nonce));
    }

    #[test]
    fn abridged_short_long_frames_and_secret_transport_selection() {
        let c = Config::parse(&format!(
            "mtproxy://localhost:443?secret={}&transport=abridged",
            "07".repeat(16)
        ))
        .unwrap();
        assert_eq!(c.mode, Mode::Abridged);
        for size in [4, 40, 504, 508, 512, 1024] {
            let mut codec = PoolTransport::Abridged;
            let mut data = DequeBuffer::with_capacity(size, 0);
            data.extend(vec![7; size]);
            codec.pack(&mut data);
            let bytes = data[..].to_vec();
            let prefix = if size < 508 { 1 } else { 4 };
            assert_eq!(bytes.len(), size + prefix);
            for n in 0..bytes.len() {
                assert_eq!(
                    codec.unpack(&mut bytes[..n].to_vec()),
                    Err(transport::Error::MissingBytes)
                );
            }
            let result = codec.unpack(&mut data[..]).unwrap();
            assert_eq!(&data[result.data_range], &vec![7; size]);
        }
        assert!(
            Config::parse(&format!(
                "mtproxy://host:443?secret=dd{}&transport=abridged",
                "07".repeat(16)
            ))
            .is_err()
        );
    }

    #[tokio::test]
    #[cfg(feature = "proxy")]
    async fn local_socks5_transport_still_forwards_without_mtproxy_header() {
        use crate::{ServerAddr, net::NetStream};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut greeting = [0; 3];
            socket.read_exact(&mut greeting).await.unwrap();
            assert_eq!(greeting, [5, 1, 0]);
            socket.write_all(&[5, 0]).await.unwrap();
            let mut request = [0; 10];
            socket.read_exact(&mut request).await.unwrap();
            assert_eq!(&request[..4], &[5, 1, 0, 1]);
            socket
                .write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 80])
                .await
                .unwrap();
            let mut data = [0; 4];
            socket.read_exact(&mut data).await.unwrap();
            assert_eq!(&data, b"ping");
            socket.write_all(b"pong").await.unwrap();
        });
        let mut stream = NetStream::connect(&ServerAddr::Proxied {
            address: "127.0.0.1:80".parse().unwrap(),
            proxy: format!("socks5://{addr}"),
        })
        .await
        .unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut data = [0; 4];
        stream.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"pong");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn pool_routes_every_dc_through_mtproxy_in_all_modes() {
        use crate::{ConnectionParams, NoRetries, SenderPool};
        use grammers_session::storages::MemorySession;
        use std::sync::Arc;
        for mode in [
            Mode::Abridged,
            Mode::Intermediate,
            Mode::Padded,
            Mode::FakeTls,
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = tokio::spawn(async move {
                for dc in 1..=5 {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    if mode == Mode::FakeTls {
                        server_tls(&mut stream, &[7; 16]).await;
                    }
                    let wire: [u8; 64] = if mode == Mode::FakeTls {
                        let (_, bytes) = read_record(&mut stream).await.unwrap();
                        bytes.try_into().unwrap()
                    } else {
                        let mut bytes = [0; 64];
                        stream.read_exact(&mut bytes).await.unwrap();
                        bytes
                    };
                    let _ = server_ciphers(&wire, &[7; 16], dc, mode);
                    // Deliberately reject authorization; this test exercises routing, not a fake Telegram login.
                    stream.shutdown().await.unwrap();
                }
            });
            let pool = SenderPool::with_configuration(
                Arc::new(MemorySession::default()),
                123,
                ConnectionParams {
                    mtproxy: Some(config(mode, port)),
                    retry_policy: Box::new(NoRetries),
                    ..Default::default()
                },
            );
            let handle = pool.handle;
            let runner = tokio::spawn(pool.runner.run());
            for dc in 1..=5 {
                let result = tokio::time::timeout(
                    Duration::from_secs(5),
                    handle.raw_invoke_in_dc(dc, vec![]),
                )
                .await
                .expect("pool failed to return on proxy closure");
                assert!(result.is_err());
            }
            handle.quit();
            runner.await.unwrap();
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn saved_auth_key_reconnect_keeps_proxy_and_padded_encrypted_frame() {
        use crate::{ConnectionParams, NoRetries, SenderPool};
        use grammers_session::{Session, storages::MemorySession};
        use std::sync::Arc;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut wire = [0; 64];
                stream.read_exact(&mut wire).await.unwrap();
                let (mut incoming, _) = server_ciphers(&wire, &[7; 16], 2, Mode::Padded);
                let mut length = [0; 4];
                stream.read_exact(&mut length).await.unwrap();
                incoming.apply_keystream(&mut length);
                let len = u32::from_le_bytes(length) as usize;
                assert!((40..MAX_PACKET).contains(&len));
                let mut packet = vec![0; len];
                stream.read_exact(&mut packet).await.unwrap();
                incoming.apply_keystream(&mut packet);
                assert_ne!(&packet[..8], &[0; 8]);
                let mut frame = length.to_vec();
                frame.extend(packet);
                let mut codec = PoolTransport::Proxy { padded: true };
                let offset = codec.unpack(&mut frame).unwrap();
                assert_eq!((offset.data_range.len() - 24) % 16, 0);
                stream.shutdown().await.unwrap();
            }
        });
        let session = Arc::new(MemorySession::default());
        let mut dc = session.dc_option(2).unwrap().unwrap();
        dc.auth_key = Some([9; 256]);
        session.set_dc_option(&dc).await.unwrap();
        let pool = SenderPool::with_configuration(
            session,
            123,
            ConnectionParams {
                mtproxy: Some(config(Mode::Padded, port)),
                retry_policy: Box::new(NoRetries),
                ..Default::default()
            },
        );
        let handle = pool.handle;
        let runner = tokio::spawn(pool.runner.run());
        for _ in 0..2 {
            assert!(
                tokio::time::timeout(Duration::from_secs(5), handle.raw_invoke_in_dc(2, vec![]))
                    .await
                    .unwrap()
                    .is_err()
            );
            handle.disconnect_from_dc(2);
        }
        handle.quit();
        runner.await.unwrap();
        server.await.unwrap();
    }
}
