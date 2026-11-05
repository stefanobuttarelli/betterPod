//! betterPod's eight-digit Magic Wormhole protocol, interoperable with Share for Mac.
//! The application protocol uses directory ZIP offers (not the crate's TAR helper).
use crate::downloads::{private_dir, safe_name};
use magic_wormhole::{transit, AppConfig, AppID, Code, MailboxConnection, Password, Wormhole};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
const APP_ID: &str = "betterpod.xyz/share";
const MANIFEST: &str = "betterpod-share.json";
const MAX_UNPACKED: u64 = 64 * 1024 * 1024 * 1024;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ShareInfo {
    pub v: u8,
    pub from: String,
    pub album: String,
    pub artist: String,
    pub title: String,
    pub songs: usize,
    pub bytes: u64,
}
pub enum Command {
    Send {
        paths: Vec<PathBuf>,
        info: ShareInfo,
        cover: Option<PathBuf>,
    },
    Receive {
        code: String,
        name: String,
    },
}
#[derive(Clone, Debug)]
pub enum Event {
    Code(String),
    Progress {
        done: u64,
        total: u64,
        phase: String,
        peer: Option<String>,
    },
    Received {
        paths: Vec<PathBuf>,
        info: Option<ShareInfo>,
    },
    Sent,
    Error(String),
}
pub struct ShareClient {
    data: PathBuf,
    tx: mpsc::SyncSender<Event>,
    rx: Option<mpsc::Receiver<Event>>,
    worker: Option<thread::JoinHandle<()>>,
    active: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Clone)]
struct State {
    events: mpsc::SyncSender<Event>,
    cancelled: Arc<AtomicBool>,
}
impl State {
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) {
            Err("Cancelled".into())
        } else {
            Ok(())
        }
    }
    fn progress(&self, done: u64, total: u64, phase: &str, peer: Option<String>) {
        let _ = self.events.try_send(Event::Progress {
            done,
            total,
            phase: phase.into(),
            peer,
        });
    }
}
impl ShareClient {
    pub fn new(data: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel(16);
        Self {
            data,
            tx,
            rx: Some(rx),
            worker: None,
            active: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn submit(&mut self, command: Command) -> Result<(), String> {
        if self.rx.is_none() {
            return Err("Share worker is shut down".into());
        }
        if self.active.swap(true, Ordering::AcqRel) {
            return Err("Another share is already running".into());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.cancelled = Arc::new(AtomicBool::new(false));
        let state = State {
            events: self.tx.clone(),
            cancelled: self.cancelled.clone(),
        };
        let active = self.active.clone();
        let data = self.data.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("share".into())
                // SPAKE2/transit futures and crypto require more than Rust's
                // default worker stack in unoptimized diagnostic builds.
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    let result = match command {
                        Command::Send { paths, info, cover } => {
                            send(&data, &state, paths, info, cover)
                        }
                        Command::Receive { code, name } => receive(&data, &state, &code, &name),
                    };
                    let event = result.unwrap_or_else(Event::Error);
                    let _ = state.events.send(event);
                    active.store(false, Ordering::Release);
                })
                .map_err(|e| {
                    self.active.store(false, Ordering::Release);
                    e.to_string()
                })?,
        );
        Ok(())
    }
    pub fn poll(&self) -> Option<Event> {
        self.rx.as_ref()?.try_recv().ok()
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn busy(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub fn shutdown(&mut self) {
        self.cancel();
        self.rx.take();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                eprintln!("Share worker stopped unexpectedly");
            }
        }
        self.active.store(false, Ordering::Release);
    }
}
impl Drop for ShareClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}
fn code(digits: &str) -> Result<Code, String> {
    if digits.len() != 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter eight digits".into());
    }
    // Preserve all three legacy digits, including "000". The generic crate
    // parser rejects zero, while betterPod's protocol permits that nameplate.
    let nameplate = unsafe { magic_wormhole::Nameplate::new_unchecked(&digits[..3]) };
    // Existing betterPod devices use a five-digit SPAKE2 secret. Preserve this
    // wire protocol; a different password policy would break Mac interoperability.
    let password = unsafe { Password::new_unchecked(digits[3..].to_string()) };
    Ok(Code::from_components(nameplate, password))
}
fn random_code() -> Result<String, String> {
    let mut source = File::open("/dev/urandom").map_err(|e| e.to_string())?;
    loop {
        let mut digits = String::new();
        while digits.len() < 8 {
            let mut byte = [0u8; 1];
            source.read_exact(&mut byte).map_err(|e| e.to_string())?;
            if byte[0] < 250 {
                digits.push(char::from(b'0' + byte[0] % 10));
            }
        }
        if digits != "00000000" && digits != "31415926" {
            return Ok(digits);
        }
    }
}
fn config() -> AppConfig<Value> {
    AppConfig {
        id: AppID::new(APP_ID),
        rendezvous_url: Cow::Owned(
            std::env::var("BETTERPOD_RUST_WORMHOLE_URL")
                .unwrap_or_else(|_| magic_wormhole::rendezvous::DEFAULT_RENDEZVOUS_SERVER.into()),
        ),
        app_version: json!({}),
    }
}
async fn bounded<T>(
    state: &State,
    future: impl std::future::Future<Output = Result<T, String>>,
    timeout: Duration,
) -> Result<T, String> {
    let state = state.clone();
    futures_lite::future::or(future, async move {
        let start = Instant::now();
        loop {
            state.check()?;
            if start.elapsed() > timeout {
                return Err("Share timed out".into());
            }
            async_std::task::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
}
async fn peer_message(wormhole: &mut Wormhole) -> Result<Value, String> {
    let message = wormhole
        .receive_json::<Value>()
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if let Some(error) = message["error"].as_str() {
        return Err(error.into());
    }
    Ok(message)
}
async fn connector() -> Result<transit::TransitConnector, String> {
    let relay = std::env::var("BETTERPOD_RUST_WORMHOLE_RELAY")
        .unwrap_or_else(|_| transit::DEFAULT_RELAY_SERVER.into());
    let relay = if relay.contains("://") {
        relay
    } else {
        format!("tcp://{relay}")
    };
    let hint = transit::RelayHint::from_urls(
        None,
        [relay
            .parse()
            .map_err(|e| format!("Invalid relay URL: {e}"))?],
    )
    .map_err(|e| e.to_string())?;
    let abilities = if std::env::var("BETTERPOD_RUST_WORMHOLE_RELAY_ONLY").as_deref() == Ok("1") {
        transit::Abilities::FORCE_RELAY
    } else {
        transit::Abilities::ALL
    };
    transit::init(abilities, None, vec![hint])
        .await
        .map_err(|e| e.to_string())
}
async fn announce(
    wormhole: &mut Wormhole,
    connector: &transit::TransitConnector,
) -> Result<(), String> {
    wormhole.send_json(&json!({"transit":{"abilities-v1":connector.our_abilities(),"hints-v1":&**connector.our_hints()}})).await.map_err(|e|e.to_string())
}
fn parse_transit(message: &Value) -> Result<(transit::Abilities, transit::Hints), String> {
    let transit = &message["transit"];
    Ok((
        serde_json::from_value(transit["abilities-v1"].clone()).map_err(|e| e.to_string())?,
        serde_json::from_value(transit["hints-v1"].clone()).map_err(|e| e.to_string())?,
    ))
}
fn send(
    data: &Path,
    state: &State,
    paths: Vec<PathBuf>,
    mut info: ShareInfo,
    cover: Option<PathBuf>,
) -> Result<Event, String> {
    if paths.is_empty() {
        return Err("Choose at least one song".into());
    }
    let code_string = random_code()?;
    state.progress(0, 0, "Preparing", None);
    let tmp = private_dir(data, &["tmp", "share"])?;
    let archive = tmp.join(format!("send-{}-{code_string}.zip", std::process::id()));
    info.v = 1;
    info.songs = paths.len();
    info.bytes = paths
        .iter()
        .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();
    let name = safe_name(if info.title.is_empty() {
        &info.album
    } else {
        &info.title
    });
    let extensions = paths
        .iter()
        .filter_map(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| format!(".{}", s.to_lowercase()))
        })
        .collect::<std::collections::BTreeSet<_>>();
    if let Err(error) = build_zip(&archive, &name, &paths, &info, cover.as_deref(), state) {
        let _ = fs::remove_file(&archive);
        return Err(error);
    }
    let result = async_std::task::block_on(bounded(
        state,
        async {
            let mailbox = MailboxConnection::connect(config(), code(&code_string)?, true)
                .await
                .map_err(|e| e.to_string())?;
            let _ = state.events.send(Event::Code(code_string.clone()));
            state.progress(0, 0, "Waiting for receiver", None);
            let mut wormhole = Wormhole::connect(mailbox)
                .await
                .map_err(|e| e.to_string())?;
            let connector = connector().await?;
            announce(&mut wormhole, &connector).await?;
            let length = fs::metadata(&archive).map_err(|e| e.to_string())?.len();
            wormhole.send_json(&json!({"offer":{"directory":{"dirname":name,"mode":"zipfile/deflated","zipsize":length,"numbytes":info.bytes,"numfiles":paths.len()+1+usize::from(cover.is_some()),"info":{"exts":extensions}}}})).await.map_err(|e|e.to_string())?;
            let mut their_transit = None;
            let mut accepted = false;
            for _ in 0..8 {
                let message = peer_message(&mut wormhole).await?;
                if message.get("transit").is_some() {
                    their_transit = Some(parse_transit(&message)?);
                }
                if message["answer"]["file_ack"] == "ok" {
                    accepted = true;
                    let peer = message["answer"]["betterpod"]["name"]
                        .as_str()
                        .map(str::to_string);
                    state.progress(0, length, "Connecting", peer);
                }
                if their_transit.is_some() && accepted {
                    break;
                }
            }
            if !accepted {
                return Err("Receiver did not accept the music".into());
            }
            let (abilities, hints) = their_transit.ok_or("Receiver sent no transit hints")?;
            let key = wormhole
                .key()
                .derive_subkey_from_purpose::<transit::TransitKey>(&format!(
                    "{APP_ID}/transit-key"
                ));
            let (mut transit, _) = connector
                .connect(
                    transit::TransitRole::Leader,
                    key,
                    abilities,
                    Arc::new(hints),
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut file = File::open(&archive).map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            let mut done = 0;
            loop {
                state.check()?;
                let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
                transit
                    .send_record(&buffer[..count])
                    .await
                    .map_err(|e| e.to_string())?;
                done += count as u64;
                state.progress(done, length, "Sending", None);
            }
            transit.flush().await.map_err(|e| e.to_string())?;
            let acknowledgement = transit.receive_record().await.map_err(|e| e.to_string())?;
            let acknowledgement: Value =
                serde_json::from_slice(&acknowledgement).map_err(|e| e.to_string())?;
            if acknowledgement["ack"] != "ok"
                || acknowledgement["sha256"] != format!("{:x}", hash.finalize())
            {
                return Err("Receiver checksum did not match".into());
            }
            wormhole.close().await.map_err(|e| e.to_string())?;
            Ok(Event::Sent)
        },
        Duration::from_secs(3600),
    ));
    let _ = fs::remove_file(archive);
    result
}
fn receive(data: &Path, state: &State, digits: &str, name: &str) -> Result<Event, String> {
    let code = code(digits)?;
    let tmp = private_dir(data, &["tmp", "share"])?;
    let archive = tmp.join(format!("receive-{}-{digits}.zip", std::process::id()));
    let result = async_std::task::block_on(bounded(
        state,
        async {
            state.progress(0, 0, "Finding sender", None);
            let mailbox = MailboxConnection::connect(config(), code, false)
                .await
                .map_err(|e| e.to_string())?;
            let mut wormhole = Wormhole::connect(mailbox)
                .await
                .map_err(|e| e.to_string())?;
            let connector = connector().await?;
            announce(&mut wormhole, &connector).await?;
            let mut their_transit = None;
            let mut offer = None;
            for _ in 0..8 {
                let message = peer_message(&mut wormhole).await?;
                if message.get("transit").is_some() {
                    their_transit = Some(parse_transit(&message)?);
                }
                if message["offer"].is_object() {
                    offer = Some(message["offer"].clone());
                }
                if offer.is_some() && their_transit.is_some() {
                    break;
                }
            }
            let offer = offer.ok_or("Sender sent no music offer")?;
            let directory = &offer["directory"];
            if directory["mode"] != "zipfile/deflated" {
                let _ = wormhole
                    .send_json(&json!({"error":"That was not a betterPod music directory"}))
                    .await;
                return Err("That was not a betterPod music directory".into());
            }
            let size = directory["zipsize"]
                .as_u64()
                .ok_or("Missing transfer size")?;
            let unpacked = directory["numbytes"].as_u64().ok_or("Missing music size")?;
            if size > MAX_UNPACKED
                || unpacked > MAX_UNPACKED
                || directory["numfiles"].as_u64().unwrap_or(u64::MAX) > 10000
            {
                return Err("Share is too large".into());
            }
            ensure_space(
                data,
                size.saturating_add(unpacked)
                    .saturating_add(64 * 1024 * 1024),
            )?;
            for extension in directory["info"]["exts"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                if !extension
                    .as_str()
                    .is_some_and(|s| audio_extension(s.trim_start_matches('.')))
                {
                    wormhole
                        .send_json(&json!({"error":format!("Can't play {} on Knulli",extension)}))
                        .await
                        .map_err(|e| e.to_string())?;
                    return Err("This share contains an unsupported audio format".into());
                }
            }
            let album = directory["dirname"]
                .as_str()
                .unwrap_or("Received Music")
                .to_string();
            wormhole
                .send_json(&json!({"answer":{"file_ack":"ok","betterpod":{"name":name}}}))
                .await
                .map_err(|e| e.to_string())?;
            let (abilities, hints) = their_transit.ok_or("Sender sent no transit hints")?;
            let key = wormhole
                .key()
                .derive_subkey_from_purpose::<transit::TransitKey>(&format!(
                    "{APP_ID}/transit-key"
                ));
            let (mut transit, _) = connector
                .connect(
                    transit::TransitRole::Follower,
                    key,
                    abilities,
                    Arc::new(hints),
                )
                .await
                .map_err(|e| e.to_string())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&archive)
                .map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut done = 0u64;
            while done < size {
                state.check()?;
                let record = transit.receive_record().await.map_err(|e| e.to_string())?;
                if record.is_empty() || record.len() as u64 > size - done {
                    return Err("Invalid share record length".into());
                }
                output.write_all(&record).map_err(|e| e.to_string())?;
                hash.update(&record);
                done += record.len() as u64;
                state.progress(done, size, "Receiving", None);
            }
            output.sync_all().map_err(|e| e.to_string())?;
            drop(output);
            transit
                .send_record(
                    &serde_json::to_vec(
                        &json!({"ack":"ok","sha256":format!("{:x}",hash.finalize())}),
                    )
                    .map_err(|e| e.to_string())?,
                )
                .await
                .map_err(|e| e.to_string())?;
            transit.flush().await.map_err(|e| e.to_string())?;
            let _ = wormhole.close().await;
            state.progress(size, size, "Saving", None);
            let (paths, info) = unpack(&archive, data, &album, state)?;
            Ok(Event::Received { paths, info })
        },
        Duration::from_secs(3600),
    ));
    let _ = fs::remove_file(archive);
    result
}
fn build_zip(
    archive: &Path,
    name: &str,
    paths: &[PathBuf],
    info: &ShareInfo,
    cover: Option<&Path>,
    state: &State,
) -> Result<(), String> {
    use zip::write::SimpleFileOptions;
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(archive)
        .map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1))
        .unix_permissions(0o644);
    zip.start_file(format!("{name}/{MANIFEST}"), options)
        .map_err(|e| e.to_string())?;
    zip.write_all(&serde_json::to_vec(info).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if let Some(cover) = cover {
        if fs::metadata(cover).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
            return Err("Share cover is too large".into());
        }
        let mut reader = image::ImageReader::open(cover)
            .map_err(|e| e.to_string())?
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(128 * 1024 * 1024);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|e| e.to_string())?
            .thumbnail(150, 150);
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .map_err(|e| e.to_string())?;
        zip.start_file(format!("{name}/cover.jpg"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&jpeg.into_inner())
            .map_err(|e| e.to_string())?;
    }
    let mut names = std::collections::HashSet::new();
    let mut done = 0;
    for path in paths {
        state.check()?;
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !audio_extension(&extension) {
            return Err(format!("Not a supported audio file: {}", path.display()));
        }
        let mut filename = safe_name(&path.file_name().unwrap_or_default().to_string_lossy());
        let original = filename.clone();
        let mut index = 2;
        while !names.insert(filename.to_lowercase()) {
            let stem = Path::new(&original)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            filename = format!("{stem} ({index}).{extension}");
            index += 1;
        }
        zip.start_file(format!("{name}/{filename}"), options)
            .map_err(|e| e.to_string())?;
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 65536];
        loop {
            state.check()?;
            let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            zip.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
            done += count as u64;
            state.progress(done, info.bytes, "Preparing", None);
        }
    }
    zip.finish()
        .and_then(|f| f.sync_all().map(|_| f).map_err(zip::result::ZipError::Io))
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn unpack(
    archive: &Path,
    data: &Path,
    name: &str,
    state: &State,
) -> Result<(Vec<PathBuf>, Option<ShareInfo>), String> {
    let staging = private_dir(data, &["tmp", "share-unpack"])?;
    let work = staging.join(format!("work-{}", std::process::id()));
    if work.exists() {
        fs::remove_dir_all(&work).map_err(|e| e.to_string())?;
    }
    fs::create_dir(&work).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut zip = zip::ZipArchive::new(File::open(archive).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if zip.len() > 10000 {
            return Err("Too many shared files".into());
        }
        let mut total = 0u64;
        let mut manifest = None;
        let mut music = Vec::new();
        let mut used = std::collections::HashSet::new();
        for index in 0..zip.len() {
            state.check()?;
            let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            if entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
            {
                return Err("Share contains a symlink".into());
            }
            let enclosed = entry
                .enclosed_name()
                .ok_or("Share contains an unsafe path")?;
            if enclosed
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            {
                return Err("Share contains an unsafe path".into());
            }
            total = total.saturating_add(entry.size());
            if total > MAX_UNPACKED {
                return Err("Unpacked share is too large".into());
            }
            let filename = enclosed.file_name().unwrap_or_default().to_string_lossy();
            if filename.eq_ignore_ascii_case(MANIFEST) {
                if entry.size() > 65536 {
                    return Err("Share manifest is too large".into());
                }
                let mut bytes = Vec::new();
                entry
                    .by_ref()
                    .take(65537)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 65536 || bytes.len() as u64 != entry.size() {
                    return Err("Invalid share manifest size".into());
                }
                manifest = serde_json::from_slice(&bytes).ok();
                continue;
            }
            let ext = enclosed
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !audio_extension(&ext)
                && !matches!(
                    ext.as_str(),
                    "jpg" | "jpeg" | "png" | "webp" | "lrc" | "cue"
                )
            {
                continue;
            }
            let mut filename = safe_name(&filename);
            let original = filename.clone();
            let mut n = 2;
            while !used.insert(filename.to_lowercase()) {
                filename = format!(
                    "{} ({n}).{ext}",
                    Path::new(&original)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                );
                n += 1;
            }
            let path = work.join(filename);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            let mut buffer = [0u8; 65536];
            let expected = entry.size();
            let mut written = 0u64;
            loop {
                state.check()?;
                let count = entry.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                written = written.saturating_add(count as u64);
                if written > expected {
                    return Err("Shared file exceeded its declared size".into());
                }
                output
                    .write_all(&buffer[..count])
                    .map_err(|e| e.to_string())?;
            }
            if written != expected {
                return Err("Shared file was truncated".into());
            }
            output.sync_all().map_err(|e| e.to_string())?;
            if audio_extension(&ext) {
                music.push(path.file_name().unwrap().to_owned());
            }
        }
        if music.is_empty() {
            return Err("Share contained no playable music".into());
        }
        let shared = private_dir(data, &["downloads", "Shared"])?;
        let base = safe_name(name);
        let mut destination = shared.join(&base);
        let mut n = 2;
        while destination.exists() {
            destination = shared.join(format!("{base} ({n})"));
            n += 1;
        }
        state.check()?;
        fs::rename(&work, &destination).map_err(|e| e.to_string())?;
        Ok((
            music
                .into_iter()
                .map(|name| destination.join(name))
                .collect(),
            manifest,
        ))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(work);
    }
    result
}
fn audio_extension(extension: &str) -> bool {
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp3"
            | "flac"
            | "m4a"
            | "m4b"
            | "aac"
            | "ogg"
            | "oga"
            | "opus"
            | "wav"
            | "aif"
            | "aiff"
            | "ape"
            | "wv"
    )
}
fn ensure_space(data: &Path, needed: u64) -> Result<(), String> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(data.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let stat = unsafe { stat.assume_init() };
    let available = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
    if available < needed {
        Err(format!("Not enough free space; {needed} bytes needed"))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eight_digit_wire_code() {
        assert_eq!(code("12345678").unwrap().to_string(), "123-45678");
        assert_eq!(code("00012345").unwrap().to_string(), "000-12345");
        assert!(code("1234").is_err());
        assert!(code("12345abc").is_err());
    }
    #[test]
    fn zip_share_roundtrip_is_private_and_rejects_traversal() {
        let root =
            std::env::temp_dir().join(format!("betterpod-share-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let song = root.join("song.flac");
        fs::write(&song, b"unchanged audio bytes").unwrap();
        let archive = root.join("share.zip");
        let (tx, _rx) = mpsc::sync_channel(16);
        let state = State {
            events: tx,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let info = ShareInfo {
            v: 1,
            songs: 1,
            title: "Shared Album".into(),
            ..Default::default()
        };
        build_zip(
            &archive,
            "Album",
            std::slice::from_ref(&song),
            &info,
            None,
            &state,
        )
        .unwrap();
        let data = root.join("rust");
        let (files, received) = unpack(&archive, &data, "Shared", &state).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].starts_with(fs::canonicalize(data.join("downloads")).unwrap()));
        assert_eq!(fs::read(&files[0]).unwrap(), b"unchanged audio bytes");
        assert_eq!(received.unwrap().title, "Shared Album");
        let malicious = root.join("unsafe.zip");
        let mut zip = zip::ZipWriter::new(File::create(&malicious).unwrap());
        zip.start_file("../escape.flac", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"escape").unwrap();
        zip.finish().unwrap();
        assert!(unpack(&malicious, &data, "Unsafe", &state).is_err());
        assert!(!root.join("escape.flac").exists());
        assert_eq!(fs::read(song).unwrap(), b"unchanged audio bytes");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shutdown_cancels_and_joins_preparation_with_a_full_event_queue() {
        let root =
            std::env::temp_dir().join(format!("betterpod-share-shutdown-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let unsupported = root.join("unsupported.txt");
        fs::write(&unsupported, b"never sent").unwrap();
        let data = root.join("rust");
        let mut client = ShareClient::new(data.clone());
        for _ in 0..16 {
            client.tx.try_send(Event::Sent).unwrap();
        }
        // The unsupported extension rejects preparation before any connection.
        client
            .submit(Command::Send {
                paths: vec![unsupported],
                info: ShareInfo::default(),
                cover: None,
            })
            .unwrap();
        client.shutdown();
        assert!(!client.busy());
        assert!(client
            .submit(Command::Receive {
                code: "12345678".into(),
                name: String::new()
            })
            .is_err());
        if let Ok(files) = fs::read_dir(data.join("tmp/share")) {
            assert_eq!(files.count(), 0);
        }
        fs::remove_dir_all(&root).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(!root.exists());
    }
}
