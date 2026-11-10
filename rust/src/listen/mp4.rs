//! Original Listen SIDX grouping: complete AAC fragments, 20 s then 180 s.
//! The downloaded compressed audio is remuxed locally with stream copy only.
use super::*;
#[derive(Clone, Debug)]
pub(super) struct Part {
    pub offset: u64,
    pub size: u64,
    pub start: f64,
    pub duration: f64,
}
pub(super) struct FragmentPlan {
    init: Vec<u8>,
    pub parts: Vec<Part>,
    pub duration: f64,
}
#[derive(Clone, Copy)]
struct BoxInfo {
    kind: [u8; 4],
    body: usize,
    end: usize,
}
fn boxes(bytes: &[u8], mut at: usize) -> Vec<BoxInfo> {
    let mut out = Vec::new();
    while at.checked_add(8).is_some_and(|n| n <= bytes.len()) {
        let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let (size, head) = if size == 1 {
            if at + 16 > bytes.len() {
                break;
            }
            let size = u64::from_be_bytes(bytes[at + 8..at + 16].try_into().unwrap());
            let Ok(size) = usize::try_from(size) else {
                break;
            };
            (size, 16)
        } else if size == 0 {
            (bytes.len() - at, 8)
        } else {
            (size, 8)
        };
        let Some(end) = at
            .checked_add(size)
            .filter(|end| *end <= bytes.len() && size >= head)
        else {
            break;
        };
        out.push(BoxInfo {
            kind: bytes[at + 4..at + 8].try_into().unwrap(),
            body: at + head,
            end,
        });
        at = end;
    }
    out
}
pub(super) fn open(
    context: &Context,
    source: &StreamSource,
) -> Result<Option<FragmentPlan>, String> {
    let mut head = range(context, source, 0, 256 * 1024, false)?;
    let top = boxes(&head, 0);
    let Some(moov) = top.iter().find(|b| b.kind == *b"moov").copied() else {
        return Ok(None);
    };
    let mut sidx = top.iter().find(|b| b.kind == *b"sidx").copied();
    if sidx.is_none() && moov.end + 8 <= head.len() && &head[moov.end + 4..moov.end + 8] == b"sidx"
    {
        let size = u32::from_be_bytes(head[moov.end..moov.end + 4].try_into().unwrap()) as usize;
        if !(32..=4 * 1024 * 1024).contains(&size) {
            return Ok(None);
        }
        let more = range(context, source, moov.end as u64, size as u64, true)?;
        head.truncate(moov.end);
        head.extend_from_slice(&more);
        sidx = Some(BoxInfo {
            kind: *b"sidx",
            body: moov.end + 8,
            end: moov.end + size,
        });
    }
    let Some(sidx) = sidx else {
        return Ok(None);
    };
    let Some(parts) = plan(&head[sidx.body..sidx.end], sidx.end as u64) else {
        return Ok(None);
    };
    let first = parts[0].offset as usize;
    if first + 64 <= head.len() {
        if let Some(moof) = boxes(&head, first).first().filter(|b| b.kind == *b"moof") {
            let end = moof.end.min(head.len());
            if let Some(i) = head[first..end].windows(4).position(|w| w == b"tfhd") {
                let p = first + i + 4;
                if p + 4 <= head.len()
                    && u32::from_be_bytes(head[p..p + 4].try_into().unwrap()) & 1 != 0
                {
                    return Ok(None);
                }
            }
        }
    }
    let duration = parts.iter().map(|p| p.duration).sum();
    Ok(Some(FragmentPlan {
        init: head[..moov.end].to_vec(),
        parts,
        duration,
    }))
}
fn plan(bytes: &[u8], sidx_end: u64) -> Option<Vec<Part>> {
    if bytes.len() < 24 {
        return None;
    }
    let scale = u32::from_be_bytes(bytes[8..12].try_into().ok()?) as f64;
    let (first, mut at) = match bytes[0] {
        0 => (
            u32::from_be_bytes(bytes[16..20].try_into().ok()?) as u64,
            20,
        ),
        1 if bytes.len() >= 32 => (u64::from_be_bytes(bytes[20..28].try_into().ok()?), 28),
        _ => return None,
    };
    let n = u16::from_be_bytes(bytes.get(at + 2..at + 4)?.try_into().ok()?) as usize;
    at += 4;
    if scale <= 0.0 || n == 0 || at.checked_add(n * 12)? > bytes.len() {
        return None;
    }
    let mut offset = sidx_end.checked_add(first)?;
    let mut time = 0.;
    let mut target = FIRST;
    let mut current = Part {
        offset,
        size: 0,
        start: 0.,
        duration: 0.,
    };
    let mut parts = Vec::new();
    for k in 0..n {
        let reference = u32::from_be_bytes(bytes[at..at + 4].try_into().ok()?);
        if reference >> 31 != 0 {
            return None;
        }
        let size = (reference & 0x7fff_ffff) as u64;
        let duration = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().ok()?) as f64 / scale;
        if size == 0 || duration <= 0.0 {
            return None;
        }
        at += 12;
        current.size = current.size.checked_add(size)?;
        current.duration += duration;
        if current.size > 256 * 1024 * 1024 {
            return None;
        }
        offset = offset.checked_add(size)?;
        time += duration;
        if current.duration >= target || k == n - 1 {
            parts.push(current);
            current = Part {
                offset,
                size: 0,
                start: time,
                duration: 0.,
            };
            target = NEXT;
        }
    }
    Some(parts)
}
fn range(
    context: &Context,
    source: &StreamSource,
    offset: u64,
    length: u64,
    exact: bool,
) -> Result<Vec<u8>, String> {
    context.check()?;
    online::validate_http(&source.video_url)?;
    if length == 0 || length > 4 * 1024 * 1024 {
        return Err("Listen range is out of bounds".into());
    }
    let mut request = online::http_agent(false)
        .get(&source.video_url)
        .header("User-Agent", online::USER_AGENT);
    for (name, value) in &source.headers {
        if name.contains(['\r', '\n']) || value.contains(['\r', '\n']) {
            return Err("Invalid Listen header".into());
        }
        request = request.header(name, value);
    }
    let mut response = request
        .header("Range", &format!("bytes={offset}-{}", offset + length - 1))
        .call()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 206 && !(response.status().as_u16() == 200 && offset == 0) {
        return Err("Server did not supply the Listen range".into());
    }
    if response.status().as_u16() == 206 {
        let expected = format!("bytes {offset}-");
        if !response
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with(&expected))
        {
            return Err("Server supplied a different Listen range".into());
        }
    }
    let mut out = Vec::with_capacity(length as usize);
    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0; 64 * 1024];
    while out.len() < (length as usize) {
        context.check()?;
        let remaining = length as usize - out.len();
        let n = reader
            .read(&mut buffer[..remaining.min(64 * 1024)])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buffer[..n]);
    }
    if exact && out.len() != length as usize {
        return Err("Listen range was incomplete".into());
    }
    Ok(out)
}
pub(super) fn fetch(
    context: &Context,
    data: &Path,
    source: &StreamSource,
    plan: &FragmentPlan,
    index: usize,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<(), String> {
    let part = plan
        .parts
        .get(index)
        .ok_or("Listen fragment is out of range")?;
    let fragment = dest.with_extension("frag");
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&fragment)
            .map_err(|e| e.to_string())?;
        file.write_all(&plan.init).map_err(|e| e.to_string())?;
        let mut got = 0;
        progress(0, Some(part.size));
        while got < part.size {
            let n = (part.size - got).min(4 * 1024 * 1024);
            file.write_all(&range(context, source, part.offset + got, n, true)?)
                .map_err(|e| e.to_string())?;
            got += n;
            progress(got, Some(part.size));
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let mut command = Command::new(downloads::ffmpeg_path(data));
        command
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
            .arg(&fragment)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-c:a",
                "copy",
                "-avoid_negative_ts",
                "make_zero",
                "-f",
                "matroska",
            ])
            .arg(dest);
        downloads::run_command(context, &mut command, Duration::from_secs(60)).map(|_| ())
    })();
    let _ = fs::remove_file(fragment);
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_groups_whole_fragments_and_rejects_indirect_indexes() {
        let mut bytes = vec![0; 24];
        bytes[8..12].copy_from_slice(&1000u32.to_be_bytes());
        bytes[22..24].copy_from_slice(&25u16.to_be_bytes());
        for _ in 0..25 {
            bytes.extend_from_slice(&123u32.to_be_bytes());
            bytes.extend_from_slice(&10_000u32.to_be_bytes());
            bytes.extend_from_slice(&0u32.to_be_bytes());
        }
        let parts = plan(&bytes, 1000).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            (parts[0].start, parts[0].duration, parts[0].size),
            (0., 20., 246)
        );
        assert_eq!(
            (parts[1].start, parts[1].duration, parts[1].size),
            (20., 180., 2214)
        );
        assert_eq!(
            (parts[2].start, parts[2].duration, parts[2].offset),
            (200., 50., 3460)
        );
        bytes[24] = 0x80;
        assert!(plan(&bytes, 1000).is_none());
    }
}
