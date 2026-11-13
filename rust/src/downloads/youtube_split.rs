//! Cuts one long YouTube video of a whole album into the songs of a chosen
//! release, at the pauses between them. Audio is copied, never re-encoded.
use super::album_split::{self, Loudness, SplitAlbum};
use super::*;

pub(super) fn youtube_track_list(
    context: &Context,
    data: &Path,
    url: &str,
    list: &SplitAlbum,
    cover: Option<&Path>,
) -> Result<Vec<PathBuf>, String> {
    validate_http(url)?;
    if crate::online::youtube_playlist_url(url) {
        return Err("Choose a single video to split into songs".into());
    }
    let info = youtube_video_info(context, data, url)?;
    if info["entries"].is_array() || text(&info, "_type") == "playlist" {
        return Err("Choose a single video to split into songs".into());
    }
    let seconds = number(&info, "duration");
    if seconds > 0.0 && !list.fits(seconds) {
        return Err(mismatch(seconds, list));
    }
    let artist = if list.artist.is_empty() {
        "Various Artists".to_string()
    } else {
        list.artist.clone()
    };
    let album = if list.album.is_empty() {
        text(&info, "title")
    } else {
        list.album.clone()
    };
    let dir = private_dir(data, &["downloads", "YouTube", &artist, &album])?;
    let video = if text(&info, "id").is_empty() {
        url.to_string()
    } else {
        text(&info, "id")
    };
    // A retry reuses songs already saved from this video and this release.
    let identity = format!("split:{video}:{}", list.id);
    let total = list.tracks.len();
    let keys = list
        .tracks
        .iter()
        .enumerate()
        .map(|(index, track)| youtube_receipts::key(&identity, index + 1, &track.title))
        .collect::<Vec<_>>();
    let mut receipts = youtube_receipts::Album::open(data, &dir)?;
    let mut saved = std::collections::BTreeMap::new();
    for (index, key) in keys.iter().enumerate() {
        if let Some(path) = receipts.completed(key, total, context)? {
            saved.insert(index, path);
        }
    }
    if saved.len() == total {
        return Ok(saved.into_values().collect());
    }
    let staging = youtube_stage(context, data)?;
    let stage = &staging.0;
    let (source, _tools) = youtube_fetch(context, data, url, &info, false, stage)?;
    let size = fs::metadata(&source).map(|m| m.len()).unwrap_or(0).max(1);
    let release_cover = stage.join("cover.jpg");
    let cover = match cover {
        Some(cover) => Some(cover),
        None if !list.cover.is_empty() => {
            match download(context, &list.cover, &release_cover, None) {
                Ok(()) => Some(release_cover.as_path()),
                Err(error) => {
                    context.check()?;
                    eprintln!("[YouTube] split: release cover unavailable: {error}");
                    None
                }
            }
        }
        None => None,
    };

    let frames = loudness(context, data, &source, seconds, size)?;
    let listed = list
        .tracks
        .iter()
        .map(|track| track.seconds)
        .collect::<Vec<_>>();
    let plan = album_split::plan(&frames, &listed)?;
    eprintln!(
        "[YouTube] split {album} ({}): {total} songs, {} of {} cuts on clear pauses: {}",
        list.source,
        plan.clear_cuts(),
        total - 1,
        plan.describe(&listed)
    );

    // One pass cuts every song at packet boundaries. A first piece before the
    // music starts is leading silence and is dropped.
    let extension = source
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let lead = plan.starts[0] > 0.05;
    let times = plan
        .starts
        .iter()
        .skip(usize::from(!lead))
        .map(|start| format!("{start:.3}"))
        .collect::<Vec<_>>()
        .join(",");
    context.progress(size, Some(size), "Cutting the songs");
    let mut ffmpeg = Command::new(ffmpeg_path(data));
    ffmpeg
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(&source)
        .args([
            "-map",
            "0:a:0",
            "-c:a",
            "copy",
            "-f",
            "segment",
            "-segment_times",
            &times,
            "-reset_timestamps",
            "1",
        ])
        .arg(stage.join(format!("song-%03d.{extension}")));
    run_command(context, &mut ffmpeg, Duration::from_secs(600))?;

    let discs = list.tracks.iter().map(|t| t.disc).max().unwrap_or(1);
    let mut out = Vec::with_capacity(total);
    for (index, track) in list.tracks.iter().enumerate() {
        context.check()?;
        let done = size.saturating_mul(index as u64 + 1) / total as u64;
        if let Some(path) = saved.remove(&index) {
            out.push(path);
            context.progress(
                done,
                Some(size),
                format!("{} of {total} songs ready", index + 1),
            );
            continue;
        }
        let prepared = stage.join(format!("song-{:03}.{extension}", index + usize::from(lead)));
        if !prepared.is_file() {
            return Err(format!("Song {} could not be cut", index + 1));
        }
        let number = if track.number > 0 {
            track.number
        } else {
            index as u32 + 1
        };
        let name = if discs > 1 {
            format!("{}-{number:02} {}.{extension}", track.disc, track.title)
        } else {
            format!("{number:02} {}.{extension}", track.title)
        };
        let target = youtube_receipts::free_file(&dir, &name)?;
        let on_disc = list.tracks.iter().filter(|t| t.disc == track.disc).count();
        tag_download(
            context,
            &prepared,
            &TagEdit {
                title: Some(track.title.clone()),
                artist: Some(if track.artist.is_empty() {
                    artist.clone()
                } else {
                    track.artist.clone()
                }),
                album: Some(album.clone()),
                album_artist: Some(artist.clone()),
                genre: (!list.genre.is_empty()).then(|| list.genre.clone()),
                year: list.year,
                track: Some(number),
                track_total: Some(on_disc as u32),
                disc: (discs > 1).then_some(track.disc),
                cover: cover.map(Path::to_path_buf),
            },
        )?;
        receipts.remember(&keys[index], total, &prepared, &target, context)?;
        youtube_receipts::commit(context, &prepared, &target)?;
        out.push(target);
        context.progress(
            done,
            Some(size),
            format!("{} of {total} songs ready", index + 1),
        );
    }
    Ok(out)
}

/// Decodes the audio once, as 8 kHz mono, into loudness frames.
fn loudness(
    context: &Context,
    data: &Path,
    source: &Path,
    seconds: f64,
    size: u64,
) -> Result<Vec<f32>, String> {
    let status = "Finding the pauses between songs";
    context.progress(0, Some(size), status);
    let mut command = Command::new(ffmpeg_path(data));
    command
        .args(["-nostdin", "-v", "error", "-i"])
        .arg(source)
        .args(["-vn", "-ac", "1", "-ar"])
        .arg(album_split::SAMPLE_RATE.to_string())
        .args(["-f", "s16le", "-"]);
    let mut meter = Loudness::default();
    let mut reported = Instant::now();
    process::stream_output(context, &mut command, Duration::from_secs(1800), |bytes| {
        meter.push(bytes);
        if seconds > 0.0 && reported.elapsed() >= Duration::from_millis(250) {
            let part = (meter.seconds() / seconds).clamp(0.0, 1.0);
            context.progress((size as f64 * part) as u64, Some(size), status);
            reported = Instant::now();
        }
        Ok(())
    })?;
    Ok(meter.finish())
}

fn mismatch(seconds: f64, list: &SplitAlbum) -> String {
    match list.listed_seconds() {
        Some(listed) => format!(
            "This video lasts {}, the track list {}",
            clock(seconds),
            clock(listed)
        ),
        None => "The track list doesn't match this video".into(),
    }
}

fn clock(seconds: f64) -> String {
    let total = seconds.round().max(0.0) as u64;
    let (hours, minutes, rest) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}
