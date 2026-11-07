//! Full resolver metadata stays in a small private disk cache, outside UI clones.
use super::*;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const MAX_ENTRY: u64 = 16 * 1024 * 1024;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 8;
const TTL: Duration = Duration::from_secs(20 * 60);
static CACHE: Mutex<()> = Mutex::new(());

fn identity(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let (host, rest) = rest.split_once('/')?;
    let id = if host.eq_ignore_ascii_case("youtu.be") {
        rest.split(['?', '#', '/']).next()?
    } else if [
        "youtube.com",
        "www.youtube.com",
        "m.youtube.com",
        "music.youtube.com",
    ]
    .iter()
    .any(|v| host.eq_ignore_ascii_case(v))
    {
        let query = rest.split_once('?')?.1;
        query
            .split('&')
            .find_map(|part| part.strip_prefix("v="))?
            .split('#')
            .next()?
    } else {
        return None;
    };
    (id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
    .then_some(id)
}

fn directory(data: &Path) -> Result<PathBuf, String> {
    let path = private_dir(data, &["tmp", "youtube-info"])?;
    #[cfg(unix)]
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    Ok(path)
}

fn filename(id: &str) -> String {
    format!("{:x}.json", Sha256::digest(id.as_bytes()))
}

pub(super) fn load(
    context: &Context,
    data: &Path,
    url: &str,
) -> Result<Option<(Value, PathBuf)>, String> {
    context.check()?;
    let Some(id) = identity(url) else {
        return Ok(None);
    };
    let _cache = CACHE.lock().map_err(|e| e.to_string())?;
    let path = directory(data)?.join(filename(id));
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return Ok(None);
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_ENTRY {
        return Ok(None);
    }
    if meta
        .modified()
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_none_or(|age| age > TTL)
    {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(&path).map_err(|e| e.to_string())?;
    let info: Value = match serde_json::from_reader(file.take(MAX_ENTRY + 1)) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    context.check()?;
    if text(&info, "id") != id || !info["formats"].is_array() {
        return Ok(None);
    }
    Ok(Some((info, path)))
}

struct Bounded<'a> {
    file: &'a mut File,
    bytes: u64,
}
impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.saturating_add(bytes.len() as u64) > MAX_ENTRY {
            return Err(io::Error::other(
                "YouTube metadata cache entry exceeds 16 MB",
            ));
        }
        let n = self.file.write(bytes)?;
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub(super) fn store(context: &Context, data: &Path, url: &str, info: &Value) -> Result<(), String> {
    context.check()?;
    let Some(id) = identity(url) else {
        return Ok(());
    };
    if text(info, "id") != id || !info["formats"].is_array() {
        return Ok(());
    }
    let _cache = CACHE.lock().map_err(|e| e.to_string())?;
    let dir = directory(data)?;
    let path = dir.join(filename(id));
    if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
        return Err("YouTube metadata cache is not a private file".into());
    }
    crate::storage::atomic_write_with(&path, |file| {
        serde_json::to_writer(Bounded { file, bytes: 0 }, info).map_err(io::Error::other)
    })
    .map_err(|e| e.to_string())?;
    context.check()?;
    let mut files = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if name.len() != 69
                || !name.ends_with(".json")
                || !name.as_bytes()[..64].iter().all(|b| b.is_ascii_hexdigit())
            {
                return None;
            }
            let meta = fs::symlink_metadata(entry.path()).ok()?;
            (meta.is_file() && !meta.file_type().is_symlink()).then(|| {
                (
                    entry.path(),
                    meta.modified().unwrap_or(UNIX_EPOCH),
                    meta.len(),
                )
            })
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(_, time, _)| *time);
    let mut bytes: u64 = files.iter().map(|(_, _, size)| size).sum();
    let mut count = files.len();
    for (file, time, size) in files {
        context.check()?;
        if count > MAX_ENTRIES || bytes > MAX_BYTES || time.elapsed().is_ok_and(|age| age > TTL) {
            fs::remove_file(file).map_err(|e| e.to_string())?;
            count -= 1;
            bytes = bytes.saturating_sub(size);
        }
    }
    Ok(())
}

pub(super) fn invalidate(data: &Path, url: &str) -> Result<(), String> {
    let Some(id) = identity(url) else {
        return Ok(());
    };
    let _cache = CACHE.lock().map_err(|e| e.to_string())?;
    let path = directory(data)?.join(filename(id));
    if fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink()) {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) fn stale_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "http error 403",
        "http error 410",
        "http error 404",
        "url has expired",
        "signature has expired",
        "requested format is not available",
        "invalid info",
        "unable to load",
    ]
    .iter()
    .any(|text| error.contains(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_cache_has_private_permissions_ttl_count_and_symlink_bounds() {
        let data = std::env::temp_dir().join(format!("youtube-info-cache-{}", std::process::id()));
        fs::create_dir_all(&data).unwrap();
        let context = Context::standalone();
        let url = "https://www.youtube.com/watch?v=first000001";
        let info = json!({"id":"first000001","title":"Album","formats":[]});
        store(&context, &data, url, &info).unwrap();
        let (_, path) = load(&context, &data, "https://youtu.be/first000001?t=5")
            .unwrap()
            .unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        File::open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - TTL - Duration::from_secs(1))
            .unwrap();
        assert!(load(&context, &data, url).unwrap().is_none());
        assert!(!path.exists());
        store(&context, &data, url, &info).unwrap();
        fs::write(&path, b"not JSON").unwrap();
        assert!(load(&context, &data, url).unwrap().is_none());
        fs::remove_file(&path).unwrap();
        let outside = data.join("outside");
        fs::write(&outside, b"preserve").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(load(&context, &data, url).unwrap().is_none());
        assert!(store(&context, &data, url, &info).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"preserve");
        fs::remove_file(&path).unwrap();
        for index in 0..12 {
            let id = format!("video{index:06}");
            store(
                &context,
                &data,
                &format!("https://www.youtube.com/watch?v={id}"),
                &json!({"id":id,"formats":[]}),
            )
            .unwrap();
        }
        assert!(fs::read_dir(directory(&data).unwrap()).unwrap().count() <= MAX_ENTRIES);
        assert!(
            !stale_error("Cancelled") && !stale_error("Not enough free memory to open YouTube")
        );
        fs::remove_dir_all(data).unwrap();
    }
}
