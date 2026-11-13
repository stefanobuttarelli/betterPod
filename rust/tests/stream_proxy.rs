//! Real loopback sockets exercise the public proxy without remote services.
use betterpod_rust::stream_proxy::StreamProxy;
use std::{
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
#[derive(Clone, Copy)]
enum Behavior {
    Range,
    Ignore,
    TruncateOnce,
    Shifted,
    Stall,
}
struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<(u64, u64)>>>,
    active: Arc<AtomicUsize>,
}
impl Server {
    fn new(data: Vec<u8>, behavior: Behavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let stopped = stop.clone();
        let observed = requests.clone();
        let serving = active.clone();
        let data = Arc::new(data);
        let worker = thread::spawn(move || {
            let mut clients = Vec::new();
            let first = Arc::new(AtomicBool::new(true));
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let data = data.clone();
                        let stopped = stopped.clone();
                        let observed = observed.clone();
                        let serving = serving.clone();
                        let first = first.clone();
                        clients.push(thread::spawn(move||{serving.fetch_add(1,Ordering::AcqRel);struct Active(Arc<AtomicUsize>);impl Drop for Active{fn drop(&mut self){self.0.fetch_sub(1,Ordering::AcqRel);}}let _active=Active(serving);stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();stream.set_write_timeout(Some(Duration::from_secs(5))).unwrap();let mut req=Vec::new();let mut buf=[0u8;2048];while !req.windows(4).any(|w|w==b"\r\n\r\n"){if stopped.load(Ordering::Acquire){return;}match stream.read(&mut buf){Ok(0)=>return,Ok(n)=>req.extend_from_slice(&buf[..n]),Err(e)if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>continue,Err(_)=>return,}if req.len()>16384{return;}}
                let request=String::from_utf8_lossy(&req);let range=request.lines().find_map(|l|l.split_once(':').filter(|(k,_)|k.eq_ignore_ascii_case("range")).map(|(_,v)|v.trim()));
                let (from,to)=range.and_then(|v|v.strip_prefix("bytes=")).and_then(|v|v.split_once('-')).map(|(a,b)|(a.parse::<u64>().unwrap(),b.parse::<u64>().unwrap())).unwrap_or((0,data.len()as u64-1));observed.lock().unwrap().push((from,to));
                if matches!(behavior,Behavior::Stall){let _=write!(stream,"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()-1,data.len(),data.len());let _=stream.write_all(&data[..1]);loop{if stopped.load(Ordering::Acquire){return;}match stream.read(&mut buf){Ok(0)=>return,Ok(_)=>{},Err(e)if matches!(e.kind(),std::io::ErrorKind::TimedOut|std::io::ErrorKind::WouldBlock)=>{},Err(_)=>return,}}}
                if matches!(behavior,Behavior::Ignore){let _=write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len());let _=stream.write_all(&data);return;}
                if from>=data.len()as u64{let _=write!(stream,"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",data.len());return;}
                let actual_from=if matches!(behavior,Behavior::Shifted){(from+1).min(data.len()as u64-1)}else{from};let to=to.min(data.len()as u64-1);let count=to.saturating_sub(actual_from)+1;
                let _=write!(stream,"HTTP/1.1 206 Partial Content\r\nContent-Type: audio/mp4\r\nContent-Range: bytes {actual_from}-{to}/{}\r\nContent-Length: {count}\r\nConnection: close\r\n\r\n",data.len());
                let count=if matches!(behavior,Behavior::TruncateOnce)&&first.swap(false,Ordering::AcqRel){count/2}else{count};let _=stream.write_all(&data[actual_from as usize..(actual_from+count)as usize]);let _=stream.shutdown(Shutdown::Both);
            }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(_) => break,
                }
            }
            for client in clients {
                let _ = client.join();
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
            requests,
            active,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn bytes(n: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 17 + i / 251) % 251) as u8).collect()
}
fn connect(url: &str) -> (TcpStream, String) {
    let rest = url.strip_prefix("http://").unwrap();
    let (host, path) = rest.split_once('/').unwrap();
    let stream = TcpStream::connect(host).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    (stream, format!("/{path}"))
}
fn request(url: &str, method: &str, range: Option<&str>) -> (String, Vec<u8>) {
    let (mut stream, path) = connect(url);
    let range = range.map(|r| format!("Range: {r}\r\n")).unwrap_or_default();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{range}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    if let Some(split) = response.windows(4).position(|w| w == b"\r\n\r\n") {
        (
            String::from_utf8_lossy(&response[..split]).into_owned(),
            response[split + 4..].to_vec(),
        )
    } else {
        (String::from_utf8_lossy(&response).into_owned(), Vec::new())
    }
}
#[test]
fn full_stream_crosses_bounded_chunks_without_byte_loss() {
    let data = bytes(10 * 1024 * 1024 + 257);
    let server = Server::new(data.clone(), Behavior::Range);
    let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
    let (headers, body) = request(&proxy.url(), "GET", None);
    assert!(headers.starts_with("HTTP/1.1 200"), "{headers}");
    assert_eq!(body, data);
    let ranges = server.requests.lock().unwrap();
    assert!(ranges.len() >= 2);
    assert!(ranges.iter().all(|(a, b)| b - a < 10 * 1024 * 1024));
}
#[test]
fn partial_head_and_range_ignoring_sources_are_correct() {
    for mode in [Behavior::Range, Behavior::Ignore] {
        let data = bytes(32768);
        let server = Server::new(data.clone(), mode);
        let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
        let (h, b) = request(&proxy.url(), "GET", Some("bytes=123-456"));
        assert!(h.starts_with("HTTP/1.1 206"), "{h}");
        assert!(h.contains("Content-Range: bytes 123-456/32768"), "{h}");
        assert_eq!(b, data[123..457]);
        let (h, b) = request(&proxy.url(), "HEAD", Some("bytes=123-456"));
        assert!(h.contains("Content-Length: 334"));
        assert!(b.is_empty());
    }
}
#[test]
fn truncated_upstream_resumes_from_exact_consumed_offset() {
    let data = bytes(65536);
    let server = Server::new(data.clone(), Behavior::TruncateOnce);
    let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
    let (_, body) = request(&proxy.url(), "GET", None);
    assert_eq!(body, data);
    let ranges = server.requests.lock().unwrap();
    assert!(ranges.len() >= 2);
    assert_eq!(ranges[1].0, 32768);
}
#[test]
fn shifted_content_range_is_rejected_before_success_header() {
    let server = Server::new(bytes(4096), Behavior::Shifted);
    let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
    let (h, _) = request(&proxy.url(), "GET", Some("bytes=12-34"));
    assert!(
        !h.starts_with("HTTP/1.1 20"),
        "Proxy accepted a mismatched Content-Range: {h}"
    );
}
#[test]
fn out_of_bounds_range_returns_http_416() {
    let server = Server::new(bytes(4096), Behavior::Range);
    let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
    let (h, b) = request(&proxy.url(), "GET", Some("bytes=5000-6000"));
    assert!(h.starts_with("HTTP/1.1 416"), "Expected416, got{h}");
    assert!(h.contains("Content-Range: bytes */4096"));
    assert!(b.is_empty());
}
#[test]
fn dropping_proxy_cancels_inflight_upstream_read() {
    let server = Server::new(bytes(4096), Behavior::Stall);
    let proxy = StreamProxy::new(server.url.clone(), vec![]).unwrap();
    let (mut socket, path) = connect(&proxy.url());
    write!(
        socket,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut b = [0u8; 4096];
    let _ = socket.read(&mut b).unwrap();
    let started = Instant::now();
    drop(proxy);
    let deadline = started + Duration::from_millis(750);
    while server.active.load(Ordering::Acquire) > 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        server.active.load(Ordering::Acquire),
        0,
        "Proxy drop left an upstream request detached"
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}
