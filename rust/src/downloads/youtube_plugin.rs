//! App-owned yt-dlp extractor extension for media-only metadata requests.
use super::*;
const SCRIPT: &[u8] = include_bytes!("youtube_media_plugin.py");

pub(super) fn configure(command: &mut Command, data: &Path) -> Result<(), String> {
    let directory = private_dir(
        data,
        &[
            "tools",
            "betterpod-plugins",
            "media",
            "yt_dlp_plugins",
            "extractor",
        ],
    )?;
    let path = directory.join("betterpod_media.py");
    let current = match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("YouTube metadata plugin is not a private file".into());
        }
        Ok(meta) if meta.len() == SCRIPT.len() as u64 => {
            fs::read(&path).map_err(|e| e.to_string())? == SCRIPT
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.to_string()),
    };
    if !current {
        crate::storage::atomic_write(&path, SCRIPT).map_err(|e| e.to_string())?;
    }
    command
        .arg("--plugin-dirs")
        .arg(data.join("tools/betterpod-plugins"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_install_is_private_idempotent_and_rejects_symlinks() {
        let root =
            std::env::temp_dir().join(format!("betterpod-youtube-plugin-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path =
            root.join("tools/betterpod-plugins/media/yt_dlp_plugins/extractor/betterpod_media.py");
        configure(&mut Command::new("yt-dlp"), &root).unwrap();
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        configure(&mut Command::new("yt-dlp"), &root).unwrap();
        assert_eq!(fs::read(&path).unwrap(), SCRIPT);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
        fs::remove_file(&path).unwrap();
        let outside = root.join("user-file");
        fs::write(&outside, b"preserve").unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        assert!(configure(&mut Command::new("yt-dlp"), &root).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"preserve");
        fs::remove_dir_all(root).unwrap();
    }
}
