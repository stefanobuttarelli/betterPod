//! Receipts identify an Archive source file independently of a filtered UI
//! track list. A complete, post-tag hash is persisted before publishing media;
//! retries reuse reserved staging names and never infer completion from size.
use super::*;
use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Component;
const RECEIPTS: &str = ".archive-receipts.json";
const MAX_RECEIPTS: u64 = 32 * 1024 * 1024;
static OPERATION: Mutex<()> = Mutex::new(());
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Receipt {
    path: PathBuf,
    size: u64,
    sha256: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Receipts {
    files: BTreeMap<String, Receipt>,
}
fn key(item: &OnlineItem, format: &str, file: &Value) -> String {
    let bytes = serde_json::to_vec(&(
        &item.id,
        format,
        text(file, "name"),
        text(file, "download_url"),
    ))
    .unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}
fn load(root: &Path) -> Result<Receipts, String> {
    let path = root.join(RECEIPTS);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Receipts::default()),
        Err(e) => return Err(e.to_string()),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_RECEIPTS {
        return Err("Archive receipts are unsafe or too large".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut bytes = Vec::new();
    options
        .open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_RECEIPTS + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECEIPTS {
        return Err("Archive receipts exceed their limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("Invalid Archive receipts: {e}"))
}
fn save(root: &Path, receipts: &Receipts, context: &Context) -> Result<(), String> {
    context.check()?;
    let path = root.join(RECEIPTS);
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err("Archive receipts are not a private file".into());
    }
    let bytes = serde_json::to_vec(receipts).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECEIPTS {
        return Err("Too many Archive download receipts".into());
    }
    crate::storage::atomic_write(&path, &bytes).map_err(|e| e.to_string())
}
fn receipt_path(data: &Path, relative: &Path) -> Option<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let path = data.join(relative);
    if !path.starts_with(data.join("downloads/Internet Archive")) {
        return None;
    }
    let parent = path.parent()?;
    let mut current = data.to_path_buf();
    for part in parent.strip_prefix(data).ok()?.components() {
        current.push(part.as_os_str());
        let metadata = fs::symlink_metadata(&current).ok()?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return None;
        }
    }
    Some(path)
}
fn hash(path: &Path, context: &Context) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        context.check()?;
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn completed(data: &Path, receipt: &Receipt, context: &Context) -> Result<Option<PathBuf>, String> {
    context.check()?;
    if receipt.size == 0
        || receipt.sha256.len() != 64
        || !receipt.sha256.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Ok(None);
    }
    let Some(path) = receipt_path(data, &receipt.path) else {
        return Ok(None);
    };
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != receipt.size {
        return Ok(None);
    }
    Ok((hash(&path, context)? == receipt.sha256).then_some(path))
}
pub fn status(
    context: &Context,
    data: &Path,
    item: &OnlineItem,
) -> Result<BTreeMap<String, (usize, u64)>, String> {
    // Receipts are atomically replaced. Reading their last committed snapshot
    // must not wait behind a multi-gigabyte album transfer. A file not yet
    // published is conservatively reported as incomplete.
    context.check()?;
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let directory = root.join("downloads/Internet Archive");
    let receipts = if fs::symlink_metadata(&directory)
        .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
    {
        load(&directory)?
    } else {
        Receipts::default()
    };
    let groups = item.raw["groups"]
        .as_object()
        .ok_or("Open the Archive album first")?;
    let mut result = BTreeMap::new();
    for (format, files) in groups {
        let Some(files) = files.as_array() else {
            continue;
        };
        let (mut have, mut left) = (0, 0u64);
        for file in files {
            let valid = match receipts.files.get(&key(item, format, file)) {
                Some(receipt) => completed(&root, receipt, context)?.is_some(),
                None => false,
            };
            if valid {
                have += 1;
            } else {
                left = left.saturating_add(number(file, "size").max(0.0) as u64);
            }
        }
        result.insert(format.clone(), (have, left));
    }
    Ok(result)
}
fn tags(
    item: &OnlineItem,
    file: &Value,
    index: usize,
    total: usize,
    cover: Option<PathBuf>,
) -> TagEdit {
    let title = text(file, "title");
    let explicit = text(file, "track")
        .split('/')
        .next()
        .and_then(|n| n.trim().parse::<u32>().ok())
        .filter(|n| *n > 0);
    let original = number(file, "_betterpod_track") as u32;
    let original_total = number(file, "_betterpod_total") as u32;
    TagEdit {
        title: (!title.is_empty()).then_some(title),
        artist: (!item.artist.is_empty()).then_some(item.artist.clone()),
        album: Some(item.title.clone()),
        track: Some(explicit.unwrap_or(if original > 0 {
            original
        } else {
            index as u32 + 1
        })),
        track_total: Some(if original_total > 0 {
            original_total
        } else {
            total as u32
        }),
        year: item.year.parse().ok(),
        cover,
        ..Default::default()
    }
}
pub(super) fn run(
    context: &Context,
    data: &Path,
    item: &OnlineItem,
    format: &str,
) -> Result<Vec<PathBuf>, String> {
    let _guard = OPERATION.lock().map_err(|e| e.to_string())?;
    context.check()?;
    let files = item.raw["groups"][format]
        .as_array()
        .ok_or("Requested audio format is unavailable")?;
    if files.is_empty() || files.len() > 10_000 {
        return Err("Choose a valid Archive audio group".into());
    }
    let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
    let receipt_dir = private_dir(&root, &["downloads", "Internet Archive"])?;
    let dir = private_dir(
        &root,
        &["downloads", "Internet Archive", &item.artist, &item.title],
    )?;
    let mut receipts = load(&receipt_dir)?;
    let cover = if !item.cover.is_empty() {
        let path = dir.join("cover.jpg");
        if !path.exists() {
            let artwork = context.progress_segment(0, None, "Preparing album artwork".into());
            let _ = download(&artwork, &item.cover, &path, None);
        }
        path.is_file().then_some(path)
    } else {
        None
    };
    let total = files.iter().try_fold(0u64, |sum, file| {
        let size = number(file, "size").max(0.0) as u64;
        (size > 0).then(|| sum.saturating_add(size))
    });
    let mut base = 0u64;
    let mut out = Vec::with_capacity(files.len());
    for (index, file) in files.iter().enumerate() {
        context.check()?;
        let id = key(item, format, file);
        if let Some(receipt) = receipts.files.get(&id) {
            if let Some(path) = completed(&root, receipt, context)? {
                out.push(path);
                base = base.saturating_add(number(file, "size").max(0.0) as u64);
                context.progress(
                    base,
                    total,
                    format!("{} of {} tracks ready", index + 1, files.len()),
                );
                continue;
            }
        }
        let name = text(file, "name");
        let name = Path::new(&name)
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .ok_or("Archive file has no valid name")?;
        let target = receipts
            .files
            .get(&id)
            .and_then(|r| receipt_path(&root, &r.path))
            .filter(|path| !path.exists())
            .unwrap_or_else(|| free_file(&dir, name));
        let relative = target
            .strip_prefix(&root)
            .map_err(|e| e.to_string())?
            .to_path_buf();
        receipts.files.insert(
            id.clone(),
            Receipt {
                path: relative.clone(),
                ..Default::default()
            },
        );
        save(&receipt_dir, &receipts, context)?;
        let edit = tags(item, file, index, files.len(), cover.clone());
        let url = text(file, "download_url");
        let mut before_commit = |stage: &Path| {
            let receipt = Receipt {
                path: relative.clone(),
                size: fs::metadata(stage).map_err(|e| e.to_string())?.len(),
                sha256: hash(stage, context)?,
            };
            receipts.files.insert(id.clone(), receipt);
            save(&receipt_dir, &receipts, context)
        };
        let segment = context.progress_segment(
            base,
            total,
            format!("Track {} of {}", index + 1, files.len()),
        );
        segment.progress(0, None, "Starting download");
        download_tagged_before_commit(
            &segment,
            &url,
            &target,
            None,
            Some(&edit),
            Some(&mut before_commit),
        )?;
        out.push(target);
        base = base.saturating_add(number(file, "size").max(0.0) as u64);
        context.progress_important(
            base,
            total,
            format!("{} of {} tracks ready", index + 1, files.len()),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wave() -> Vec<u8> {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&436u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&22050u32.to_le_bytes());
        bytes.extend_from_slice(&44100u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&400u32.to_le_bytes());
        bytes.resize(444, 0);
        bytes
    }
    fn server(requests: usize, partial_first: bool) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let body = wave();
            let mut seen = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(8);
            while seen.len() < requests {
                let (mut stream, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "missing archive fixture request");
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut buffer = [0; 8192];
                let n = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..n]).into_owned();
                if partial_first && seen.is_empty() {
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"original-fixture\"\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                    stream.write_all(&body[..200]).unwrap();
                } else if partial_first {
                    assert!(request.to_ascii_lowercase().contains("range: bytes=200-"));
                    write!(stream,"HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes 200-{}/{}\r\nETag: \"original-fixture\"\r\nConnection: close\r\n\r\n",body.len()-200,body.len()-1,body.len()).unwrap();
                    stream.write_all(&body[200..]).unwrap();
                } else {
                    write!(stream,"HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"original-fixture\"\r\nConnection: close\r\n\r\n",body.len()).unwrap();
                    stream.write_all(&body).unwrap();
                }
                seen.push(request);
            }
            seen
        });
        (format!("http://{address}"), server)
    }
    fn item(url: &str) -> OnlineItem {
        let mut item = OnlineItem::new(crate::online::Provider::Archive);
        item.id = "synthetic-album".into();
        item.title = "Receipt Fixture".into();
        item.artist = "Synthetic Artist".into();
        item.raw = json!({"groups":{"WAV":[{"name":"01 One.wav","download_url":format!("{url}/one"),"size":444,"_betterpod_track":1,"_betterpod_total":2},{"name":"02 Two.wav","download_url":format!("{url}/two"),"size":444,"_betterpod_track":2,"_betterpod_total":2}]}});
        item
    }
    #[test]
    fn receipt_status_does_not_wait_for_the_album_download_lock() {
        let data = std::env::temp_dir().join(format!(
            "betterpod-archive-status-live-{}",
            std::process::id()
        ));
        fs::create_dir_all(&data).unwrap();
        let album = item("https://example.invalid/not-requested");
        let guard = OPERATION.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let source = data.clone();
        let worker = thread::spawn(move || {
            tx.send(status(&Context::standalone(), &source, &album))
                .unwrap();
        });
        let result = rx.recv_timeout(Duration::from_millis(500));
        drop(guard);
        worker.join().unwrap();
        assert_eq!(result.unwrap().unwrap()["WAV"], (0, 888));
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn completed_subset_and_deleted_track_resume_without_duplicate_files() {
        let data =
            std::env::temp_dir().join(format!("betterpod-archive-loopback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();
        let (url, server) = server(3, false);
        let album = item(&url);
        let mut subset = album.clone();
        subset.raw["groups"]["WAV"] = json!([album.raw["groups"]["WAV"][1].clone()]);
        let context = Context::standalone();
        let second = run(&context, &data, &subset, "WAV").unwrap().pop().unwrap();
        assert_eq!(status(&context, &data, &album).unwrap()["WAV"], (1, 444));
        let first = run(&context, &data, &album, "WAV").unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(first[1], second);
        assert_eq!(run(&context, &data, &album, "WAV").unwrap(), first);
        assert_eq!(status(&context, &data, &album).unwrap()["WAV"], (2, 0));
        fs::remove_file(&first[0]).unwrap();
        assert_eq!(status(&context, &data, &album).unwrap()["WAV"], (1, 444));
        assert_eq!(run(&context, &data, &album, "WAV").unwrap(), first);
        assert!(first
            .iter()
            .all(|p| !p.file_name().unwrap().to_string_lossy().contains("(2)")));
        assert_eq!(fs::read(&first[0]).unwrap(), wave());
        assert_eq!(fs::read(&first[1]).unwrap(), wave());
        assert_eq!(server.join().unwrap().len(), 3);
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn interrupted_transfer_reuses_reserved_name_and_http_range() {
        let data =
            std::env::temp_dir().join(format!("betterpod-archive-range-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        fs::create_dir_all(&data).unwrap();
        let (url, server) = server(2, true);
        let mut album = item(&url);
        album.raw["groups"]["WAV"] = json!([album.raw["groups"]["WAV"][0].clone()]);
        let context = Context::standalone();
        assert!(run(&context, &data, &album, "WAV").is_err());
        assert_eq!(status(&context, &data, &album).unwrap()["WAV"], (0, 444));
        let files = run(&context, &data, &album, "WAV").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(fs::read(&files[0]).unwrap(), wave());
        assert!(!files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("(2)"));
        assert_eq!(server.join().unwrap().len(), 2);
        fs::remove_dir_all(data).unwrap();
    }
    #[test]
    fn source_identity_and_track_numbers_survive_a_single_track_subset() {
        let mut item = OnlineItem::new(crate::online::Provider::Archive);
        item.id = "fixture".into();
        let file = json!({"name":"nested/07 Song.flac","download_url":"https://example.invalid/original/07","_betterpod_track":7,"_betterpod_total":12});
        let identity = key(&item, "FLAC", &file);
        item.raw["groups"] = json!({"FLAC":[file.clone()]});
        assert_eq!(identity, key(&item, "FLAC", &item.raw["groups"]["FLAC"][0]));
        let edit = tags(&item, &file, 0, 1, None);
        assert_eq!(edit.track, Some(7));
        assert_eq!(edit.track_total, Some(12));
    }
    #[test]
    fn receipt_validation_rejects_missing_changed_traversal_and_symlink_files() {
        let data =
            std::env::temp_dir().join(format!("betterpod-archive-receipts-{}", std::process::id()));
        let _ = fs::remove_dir_all(&data);
        let dir =
            private_dir(&data, &["downloads", "Internet Archive", "Artist", "Album"]).unwrap();
        let root = fs::canonicalize(&data).unwrap();
        let file = dir.join("one.wav");
        fs::write(&file, b"completed bytes").unwrap();
        let context = Context::standalone();
        let receipt = Receipt {
            path: file.strip_prefix(&root).unwrap().into(),
            size: 15,
            sha256: hash(&file, &context).unwrap(),
        };
        assert!(completed(&root, &receipt, &context).unwrap().is_some());
        fs::write(&file, b"modified bytes!").unwrap();
        assert!(completed(&root, &receipt, &context).unwrap().is_none());
        assert!(receipt_path(
            &root,
            Path::new("downloads/Internet Archive/../outside.wav")
        )
        .is_none());
        fs::remove_file(&file).unwrap();
        assert!(completed(&root, &receipt, &context).unwrap().is_none());
        #[cfg(unix)]
        {
            let outside = root.join("outside.wav");
            fs::write(&outside, b"completed bytes").unwrap();
            std::os::unix::fs::symlink(outside, &file).unwrap();
            assert!(completed(&root, &receipt, &context).unwrap().is_none());
        }
        fs::remove_dir_all(data).unwrap();
    }
}
