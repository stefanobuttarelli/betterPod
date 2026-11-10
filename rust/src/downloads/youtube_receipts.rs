//! YouTube album retries reuse only verified, fully tagged tracks at the same position.
use super::*;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{collections::BTreeMap, path::Component};

const RECEIPTS: &str = ".youtube-receipts.json";
const MAX_RECEIPTS: u64 = 2 * 1024 * 1024;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Receipt {
    file: PathBuf,
    size: u64,
    sha256: String,
    total: usize,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Manifest {
    tracks: BTreeMap<String, Receipt>,
}

pub(super) struct Album {
    root: PathBuf,
    directory: PathBuf,
    manifest: Manifest,
    _operation: DestinationGuard,
}

pub(super) fn key(playlist: &str, position: usize, video: &str) -> String {
    let bytes = serde_json::to_vec(&(playlist, position, video)).unwrap_or_default();
    format!("{:x}", Sha256::digest(bytes))
}

fn reader(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    options.open(path).map_err(|e| e.to_string())
}

fn hash(path: &Path, context: &Context) -> Result<String, String> {
    let mut file = reader(path)?;
    let mut digest = Sha256::new();
    let mut bytes = [0u8; 65536];
    loop {
        context.check()?;
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&bytes[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

impl Album {
    pub(super) fn open(data: &Path, directory: &Path) -> Result<Self, String> {
        let root = fs::canonicalize(data).map_err(|e| e.to_string())?;
        let directory = directory.to_path_buf();
        if !ACTIVE_DOWNLOADS
            .lock()
            .map_err(|e| e.to_string())?
            .insert(directory.clone())
        {
            return Err("This YouTube album is already being downloaded".into());
        }
        let operation = DestinationGuard(directory.clone());
        let mut album = Self {
            root,
            directory,
            manifest: Manifest::default(),
            _operation: operation,
        };
        album.validate_directory()?;
        let path = album.directory.join(RECEIPTS);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(album),
            Err(e) => return Err(e.to_string()),
            Ok(m) if !m.is_file() || m.file_type().is_symlink() || m.len() > MAX_RECEIPTS => {
                return Err("YouTube receipts are unsafe or too large".into());
            }
            Ok(_) => {}
        }
        let mut bytes = Vec::new();
        reader(&path)?
            .take(MAX_RECEIPTS + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_RECEIPTS {
            return Err("YouTube receipts exceed their limit".into());
        }
        album.manifest =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid YouTube receipts: {e}"))?;
        Ok(album)
    }

    fn validate_directory(&self) -> Result<(), String> {
        if !self
            .directory
            .starts_with(self.root.join("downloads/YouTube"))
        {
            return Err("YouTube receipt path escaped private downloads".into());
        }
        let mut path = self.root.clone();
        for component in self
            .directory
            .strip_prefix(&self.root)
            .map_err(|e| e.to_string())?
            .components()
        {
            if !matches!(component, Component::Normal(_)) {
                return Err("Invalid YouTube album path".into());
            }
            path.push(component.as_os_str());
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err("YouTube album is not a private directory".into());
            }
        }
        Ok(())
    }

    pub(super) fn completed(
        &self,
        key: &str,
        total: usize,
        context: &Context,
    ) -> Result<Option<PathBuf>, String> {
        context.check()?;
        self.validate_directory()?;
        let Some(receipt) = self.manifest.tracks.get(key) else {
            return Ok(None);
        };
        if receipt.total != total
            || receipt.size == 0
            || receipt.sha256.len() != 64
            || !receipt.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || receipt.file.components().count() != 1
            || !matches!(receipt.file.components().next(), Some(Component::Normal(_)))
        {
            return Ok(None);
        }
        let path = self.directory.join(&receipt.file);
        let Ok(meta) = fs::symlink_metadata(&path) else {
            return Ok(None);
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() != receipt.size {
            return Ok(None);
        }
        Ok((hash(&path, context)? == receipt.sha256).then_some(path))
    }

    pub(super) fn remember(
        &mut self,
        key: &str,
        total: usize,
        stage: &Path,
        target: &Path,
        context: &Context,
    ) -> Result<(), String> {
        context.check()?;
        self.validate_directory()?;
        if target.parent() != Some(self.directory.as_path()) {
            return Err("Invalid YouTube track destination".into());
        }
        let meta = fs::symlink_metadata(stage).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Invalid YouTube staging file".into());
        }
        self.manifest.tracks.insert(
            key.into(),
            Receipt {
                file: PathBuf::from(target.file_name().ok_or("Missing YouTube track name")?),
                size: meta.len(),
                sha256: hash(stage, context)?,
                total,
            },
        );
        let bytes = serde_json::to_vec(&self.manifest).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_RECEIPTS {
            return Err("Too many YouTube download receipts".into());
        }
        let path = self.directory.join(RECEIPTS);
        if fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file() || m.file_type().is_symlink()) {
            return Err("YouTube receipts are not a private file".into());
        }
        context.check()?;
        crate::storage::atomic_write(&path, &bytes).map_err(|e| e.to_string())
    }
}

pub(super) fn free_file(directory: &Path, name: &str) -> Result<PathBuf, String> {
    let name = safe_name(name);
    let file = Path::new(&name);
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let ext = file.extension().and_then(|v| v.to_str()).unwrap_or("");
    for n in 1.. {
        let name = if n == 1 {
            name.clone()
        } else if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let path = directory.join(name);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Err(e) => return Err(e.to_string()),
            Ok(_) => {}
        }
    }
    unreachable!()
}

pub(super) fn commit(context: &Context, stage: &Path, target: &Path) -> Result<(), String> {
    context.check()?;
    File::open(stage)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())?;
    let _commit = COMMIT_LOCK.lock().map_err(|e| e.to_string())?;
    context.check()?;
    match fs::symlink_metadata(target) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(_) => return Err("Download destination already exists".into()),
    }
    fs::rename(stage, target).map_err(|e| e.to_string())
}
