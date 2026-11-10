use super::Shared;
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use serde_json::{json, Value};
use sha2::Sha256;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

pub(super) const MAX_FRAME: usize = 16 * 1024 * 1024;
const IDENTITY: &[u8] = b"betterpod.xyz/library/v1";

pub(super) struct Channel {
    stream: TcpStream,
    outgoing: ChaCha20Poly1305,
    incoming: ChaCha20Poly1305,
    sent: u64,
    received: u64,
}

fn nonce(counter: u64) -> [u8; 12] {
    let mut nonce = [0; 12];
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    nonce
}
fn key(shared: &[u8], direction: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0; 32];
    Hkdf::<Sha256>::new(None, shared)
        .expand(direction, &mut key)
        .map_err(|_| "Invalid session key")?;
    Ok(key)
}
impl Channel {
    fn new(stream: TcpStream, shared: &[u8], server: bool) -> Result<Self, String> {
        let client = key(shared, b"betterpod-library-v1/client")?;
        let host = key(shared, b"betterpod-library-v1/server")?;
        Ok(Self {
            stream,
            outgoing: ChaCha20Poly1305::new_from_slice(if server { &host } else { &client })
                .map_err(|_| "Invalid key")?,
            incoming: ChaCha20Poly1305::new_from_slice(if server { &client } else { &host })
                .map_err(|_| "Invalid key")?,
            sent: 0,
            received: 0,
        })
    }
    pub(super) fn send(&mut self, value: &Value, shared: &Shared) -> Result<(), String> {
        let clear = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if clear.len() > MAX_FRAME - 16 {
            return Err("Library response is too large".into());
        }
        let nonce = Nonce::from(nonce(self.sent));
        let encrypted = self
            .outgoing
            .encrypt(&nonce, clear.as_slice())
            .map_err(|_| "Cannot encrypt library response")?;
        self.sent = self
            .sent
            .checked_add(1)
            .ok_or("Session message counter exhausted")?;
        write_frame(&mut self.stream, &encrypted, shared)
    }
    pub(super) fn receive(&mut self, shared: &Shared, timeout: Duration) -> Result<Value, String> {
        let bytes = read_frame(&mut self.stream, shared, timeout, MAX_FRAME)?;
        let nonce = Nonce::from(nonce(self.received));
        let clear = self
            .incoming
            .decrypt(&nonce, bytes.as_slice())
            .map_err(|_| "The library session could not be authenticated")?;
        self.received = self
            .received
            .checked_add(1)
            .ok_or("Session message counter exhausted")?;
        serde_json::from_slice(&clear).map_err(|_| "Invalid encrypted library request".into())
    }
}

pub(super) fn accept(
    mut stream: TcpStream,
    code: &str,
    id: &str,
    name: &str,
    shared: &Shared,
) -> Result<(Channel, String), String> {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    let hello: Value = serde_json::from_slice(&read_frame(
        &mut stream,
        shared,
        Duration::from_secs(8),
        4096,
    )?)
    .map_err(|_| "Invalid library handshake")?;
    if hello.get("v").and_then(Value::as_u64) != Some(1) {
        return Err("Unsupported library protocol".into());
    }
    let peer = STANDARD
        .decode(
            hello
                .get("spake")
                .and_then(Value::as_str)
                .ok_or("Missing authentication message")?,
        )
        .map_err(|_| "Invalid authentication message")?;
    let (state, outbound) =
        Spake2::<Ed25519Group>::start_symmetric(&Password::new(code), &Identity::new(IDENTITY));
    let response = serde_json::to_vec(&json!({"v":1,"spake":STANDARD.encode(outbound)}))
        .map_err(|e| e.to_string())?;
    write_frame(&mut stream, &response, shared)?;
    let secret = state
        .finish(&peer)
        .map_err(|_| "Invalid authentication message")?;
    let mut channel = Channel::new(stream, &secret, true)?;
    let confirm = channel.receive(shared, Duration::from_secs(8))?;
    if confirm.get("op").and_then(Value::as_str) != Some("confirm") {
        return Err("Confirm pairing before using the library".into());
    }
    let peer_name =
        super::files::safe_name(confirm.get("name").and_then(Value::as_str).unwrap_or("Mac"));
    channel.send(&json!({"ok":true,"name":name,"session":id}), shared)?;
    Ok((channel, peer_name))
}

fn read_some(
    stream: &mut TcpStream,
    out: &mut [u8],
    shared: &Shared,
    until: Instant,
) -> Result<(), String> {
    let mut filled = 0;
    while filled < out.len() {
        if shared.cancelled.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        if Instant::now() >= until {
            return Err("The Wi-Fi library connection timed out".into());
        }
        match stream.read(&mut out[filled..]) {
            Ok(0) => return Err("Connection closed".into()),
            Ok(length) => filled += length,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
pub(super) fn read_frame(
    stream: &mut TcpStream,
    shared: &Shared,
    timeout: Duration,
    cap: usize,
) -> Result<Vec<u8>, String> {
    let until = Instant::now() + timeout;
    let mut header = [0; 4];
    read_some(stream, &mut header, shared, until)?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > cap {
        return Err("Library frame exceeds its limit".into());
    }
    let mut body = vec![0; length];
    read_some(stream, &mut body, shared, until)?;
    Ok(body)
}
pub(super) fn write_frame(
    stream: &mut TcpStream,
    bytes: &[u8],
    shared: &Shared,
) -> Result<(), String> {
    if shared.cancelled.load(Ordering::Acquire) {
        return Err("Cancelled".into());
    }
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err("Library frame exceeds its limit".into());
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .and_then(|_| stream.write_all(bytes))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
pub(super) fn connect(
    mut stream: TcpStream,
    code: &str,
    shared: &Shared,
) -> Result<Channel, String> {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let (state, outbound) =
        Spake2::<Ed25519Group>::start_symmetric(&Password::new(code), &Identity::new(IDENTITY));
    write_frame(
        &mut stream,
        &serde_json::to_vec(&json!({"v":1,"spake":STANDARD.encode(outbound)})).unwrap(),
        shared,
    )?;
    let reply: Value = serde_json::from_slice(&read_frame(
        &mut stream,
        shared,
        Duration::from_secs(8),
        4096,
    )?)
    .unwrap();
    let key = state
        .finish(&STANDARD.decode(reply["spake"].as_str().unwrap()).unwrap())
        .map_err(|e| e.to_string())?;
    let mut channel = Channel::new(stream, &key, false)?;
    channel.send(&json!({"op":"confirm","name":"Fixture Mac"}), shared)?;
    let reply = channel.receive(shared, Duration::from_secs(8))?;
    if reply["ok"] != true {
        return Err("Pairing was not confirmed".into());
    }
    Ok(channel)
}
