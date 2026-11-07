//! Loopback range proxy. ffmpeg does not need TLS or remote authorization
//! headers; each upstream read is bounded to 10 MiB with resumable retries.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
const CHUNK: u64 = 10 * 1024 * 1024;
static CLIENTS: AtomicUsize = AtomicUsize::new(0);
struct Source {
    url: String,
    headers: Vec<(String, String)>,
    token: String,
    size: AtomicU64,
    stop: Arc<AtomicBool>,
    agent: ureq::Agent,
}
pub struct StreamProxy {
    base: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    agent: ureq::Agent,
}
impl StreamProxy {
    pub fn new(url: String, headers: Vec<(String, String)>) -> Result<Self, String> {
        crate::online::validate_http(&url)?;
        if headers
            .iter()
            .any(|(k, v)| k.contains(['\r', '\n']) || v.contains(['\r', '\n']))
        {
            return Err("Invalid stream header".into());
        }
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let mut random = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut random))
            .map_err(|e| e.to_string())?;
        let token = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let base = format!("http://{address}/{token}");
        let stop = Arc::new(AtomicBool::new(false));
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(25)))
            .timeout_connect(Some(Duration::from_secs(2)))
            .timeout_resolve(Some(Duration::from_secs(2)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        let source = Arc::new(Source {
            url,
            headers,
            token,
            size: AtomicU64::new(0),
            stop: stop.clone(),
            agent: agent.clone(),
        });
        let stopped = stop.clone();
        let worker = thread::Builder::new()
            .name("stream-proxy".into())
            .spawn(move || {
                let mut clients: Vec<(JoinHandle<()>, TcpStream)> = Vec::new();
                while !stopped.load(Ordering::Acquire) {
                    let mut i = 0;
                    while i < clients.len() {
                        if clients[i].0.is_finished() {
                            let (worker, _) = clients.swap_remove(i);
                            let _ = worker.join();
                        } else {
                            i += 1;
                        }
                    }
                    match listener.accept() {
                        Ok((socket, _)) => {
                            if CLIENTS.fetch_add(1, Ordering::AcqRel) >= 8 {
                                CLIENTS.fetch_sub(1, Ordering::AcqRel);
                                drop(socket);
                                continue;
                            }
                            let interrupt = match socket.try_clone() {
                                Ok(s) => s,
                                Err(_) => {
                                    CLIENTS.fetch_sub(1, Ordering::AcqRel);
                                    continue;
                                }
                            };
                            let source = source.clone();
                            let result = thread::Builder::new().name("stream-range".into()).spawn(
                                move || {
                                    struct Slot;
                                    impl Drop for Slot {
                                        fn drop(&mut self) {
                                            CLIENTS.fetch_sub(1, Ordering::AcqRel);
                                        }
                                    }
                                    let _slot = Slot;
                                    if let Err(error) = serve(socket, &source) {
                                        if !source.stop.load(Ordering::Acquire) {
                                            eprintln!("Stream proxy: {error}");
                                        }
                                    }
                                },
                            );
                            match result {
                                Ok(worker) => clients.push((worker, interrupt)),
                                Err(_) => {
                                    CLIENTS.fetch_sub(1, Ordering::AcqRel);
                                }
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(_) => break,
                    }
                }
                for (_, socket) in &clients {
                    let _ = socket.shutdown(Shutdown::Both);
                }
                for (worker, _) in clients {
                    let _ = worker.join();
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            base,
            stop,
            worker: Some(worker),
            agent,
        })
    }
    pub fn url(&self) -> String {
        self.base.clone()
    }
}
impl Drop for StreamProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.agent.clone().shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn serve(mut socket: TcpStream, source: &Source) -> Result<(), String> {
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    socket
        .set_write_timeout(Some(Duration::from_secs(20)))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(socket.try_clone().map_err(|e| e.to_string())?).take(16 * 1024);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if !matches!(method, "GET" | "HEAD") || path != format!("/{}", source.token) {
        let _ = socket
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return Ok(());
    }
    let head = method == "HEAD";
    let mut range = None;
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if n == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range = match parse_range(value.trim()) {
                    Ok(value) => Some(value),
                    Err(_) => {
                        socket.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").map_err(|e|e.to_string())?;
                        return Ok(());
                    }
                };
            }
        }
    }
    let (from, to) = range.unwrap_or((0, None));
    let agent = &source.agent;
    let known_size = source.size.load(Ordering::Acquire);
    if known_size > 0 && from >= known_size {
        return range_error(&mut socket, known_size);
    }
    let mut response = open_retry(
        agent,
        source,
        from,
        from.saturating_add(CHUNK - 1).min(to.unwrap_or(u64::MAX)),
    )
    .map_err(|error| {
        let _ = socket.write_all(
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        error
    })?;
    let size = total(&response).max(source.size.load(Ordering::Acquire));
    if size > 0 {
        source.size.store(size, Ordering::Release);
    }
    if response.status().as_u16() == 416 || (size > 0 && from >= size) {
        return range_error(&mut socket, size);
    }
    let last = if size > 0 {
        Some(to.unwrap_or(size - 1).min(size - 1))
    } else {
        to
    };
    let ranged = range.is_some() && size > 0;
    let mut headers=format!("HTTP/1.1 {}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n",if ranged{"206 Partial Content"}else{"200 OK"});
    if let Some(last) = last {
        headers.push_str(&format!(
            "Content-Length: {}\r\n",
            last.saturating_sub(from) + 1
        ));
        if ranged {
            headers.push_str(&format!("Content-Range: bytes {from}-{last}/{size}\r\n"));
        }
    }
    headers.push_str("\r\n");
    socket
        .write_all(headers.as_bytes())
        .map_err(|e| e.to_string())?;
    if head {
        return Ok(());
    }
    let mut offset = from;
    let mut retries = 0u32;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if source.stop.load(Ordering::Acquire) {
            return Ok(());
        }
        let piece_end = offset
            .saturating_add(CHUNK - 1)
            .min(last.unwrap_or(u64::MAX));
        if response.status().as_u16() == 200 && offset > 0 {
            let skipped = std::io::copy(
                &mut response.body_mut().as_reader().take(offset),
                &mut std::io::sink(),
            )
            .map_err(|e| e.to_string())?;
            if skipped != offset {
                return Err("Range-ignoring source ended before the requested position".into());
            }
        }
        let mut remaining = piece_end.saturating_sub(offset) + 1;
        let mut broken = false;
        {
            let mut body = response.body_mut().as_reader();
            while remaining > 0 && !source.stop.load(Ordering::Acquire) {
                let want = remaining.min(buffer.len() as u64) as usize;
                match body.read(&mut buffer[..want]) {
                    Ok(0) => {
                        if last.is_none() {
                            return Ok(());
                        }
                        broken = true;
                        break;
                    }
                    Ok(n) => {
                        socket.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
                        offset += n as u64;
                        remaining -= n as u64;
                        retries = 0;
                    }
                    Err(_) => {
                        broken = true;
                        break;
                    }
                }
            }
        }
        if source.stop.load(Ordering::Acquire) || last.is_some_and(|last| offset > last) {
            return Ok(());
        }
        if broken {
            retries += 1;
            if retries > 5 {
                return Err("Stream stopped after bounded range retries".into());
            }
            for _ in 0..retries * 10 {
                if source.stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        response = open_retry(
            agent,
            source,
            offset,
            offset
                .saturating_add(CHUNK - 1)
                .min(last.unwrap_or(u64::MAX)),
        )?;
    }
}
fn range_error(socket: &mut TcpStream, size: u64) -> Result<(), String> {
    write!(socket,"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").map_err(|e|e.to_string())
}
fn validate_response(
    response: &ureq::http::Response<ureq::Body>,
    from: u64,
    to: u64,
) -> Result<(), String> {
    if response.status().as_u16() != 206 {
        return Ok(());
    }
    let value = response
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("bytes "))
        .ok_or("Missing upstream Content-Range")?;
    let (range, total) = value
        .split_once('/')
        .ok_or("Invalid upstream Content-Range")?;
    let (a, b) = range.split_once('-').ok_or("Invalid upstream byte range")?;
    let a = a
        .parse::<u64>()
        .map_err(|_| "Invalid upstream range start")?;
    let b = b.parse::<u64>().map_err(|_| "Invalid upstream range end")?;
    let size = total.parse::<u64>().map_err(|_| "Invalid upstream size")?;
    if a != from || b < a || b > to || b >= size {
        return Err("Upstream Content-Range does not match requested bytes".into());
    }
    if let Some(length) = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if length != b - a + 1 {
            return Err("Upstream length does not match its range".into());
        }
    }
    Ok(())
}
fn open_retry(
    agent: &ureq::Agent,
    source: &Source,
    from: u64,
    to: u64,
) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut error = String::new();
    for attempt in 0..3 {
        if source.stop.load(Ordering::Acquire) {
            return Err("Stream cancelled".into());
        }
        let mut request = agent.get(&source.url);
        for (name, value) in &source.headers {
            if !name.eq_ignore_ascii_case("range") && !name.eq_ignore_ascii_case("host") {
                request = request.header(name, value);
            }
        }
        match request
            .header("Range", &format!("bytes={from}-{to}"))
            .call()
        {
            Ok(response)
                if response.status().as_u16() == 200 || response.status().as_u16() == 206 =>
            {
                validate_response(&response, from, to)?;
                let known = source.size.load(Ordering::Acquire);
                let observed = total(&response);
                if known > 0 && observed > 0 && known != observed {
                    return Err("Stream source changed length during playback".into());
                }
                return Ok(response);
            }
            Ok(response) => {
                error = format!(
                    "Stream source refused HTTP {}; refresh its link",
                    response.status().as_u16()
                );
                if response.status().as_u16() == 416 {
                    return Ok(response);
                }
            }
            Err(e) => error = e.to_string(),
        }
        for _ in 0..(attempt + 1) * 10 {
            if source.stop.load(Ordering::Acquire) {
                return Err("Stream cancelled".into());
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
    Err(error)
}
fn total(response: &ureq::http::Response<ureq::Body>) -> u64 {
    response
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit_once('/'))
        .and_then(|(_, n)| n.parse().ok())
        .or_else(|| {
            if response.status().as_u16() == 200 {
                response
                    .headers()
                    .get("content-length")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
            } else {
                None
            }
        })
        .unwrap_or(0)
}
fn parse_range(value: &str) -> Result<(u64, Option<u64>), String> {
    let value = value
        .strip_prefix("bytes=")
        .ok_or("Unsupported HTTP range")?;
    let (a, b) = value.split_once('-').ok_or("Invalid HTTP range")?;
    let start = a.parse::<u64>().map_err(|_| "Invalid range start")?;
    let end = if b.is_empty() {
        None
    } else {
        Some(b.parse::<u64>().map_err(|_| "Invalid range end")?)
    };
    if end.is_some_and(|end| end < start) {
        return Err("Reversed HTTP range".into());
    }
    Ok((start, end))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_ranges_are_bounded_and_unambiguous() {
        assert_eq!(parse_range("bytes=12-34").unwrap(), (12, Some(34)));
        assert_eq!(parse_range("bytes=10-").unwrap(), (10, None));
        for invalid in [
            "bytes=-10",
            "bytes=20-10",
            "bytes=1-2,3-4",
            "bytes=0-18446744073709551616",
        ] {
            assert!(parse_range(invalid).is_err());
        }
    }
}
