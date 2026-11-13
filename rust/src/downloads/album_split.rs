//! Splits one long upload of a whole album into its songs.
//!
//! The track list (Discogs, MusicBrainz…) gives each song's length. The audio
//! gives the pauses between songs. Every cut lands on the pause that best fits
//! the listed lengths, measured from the previous cut, so a list that is off by
//! a few seconds still cuts in the silence and errors never add up.
use crate::online::{number, text, OnlineItem};

/// Loudness is measured in 50 ms frames of 8 kHz mono audio.
pub const SAMPLE_RATE: u32 = 8000;
const FRAME: usize = 400;
const FPS: usize = 20;
/// A song starts this long before its first sound, inside the pause.
const LEAD_FRAMES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct SplitTrack {
    pub title: String,
    pub artist: String,
    pub seconds: f64,
    pub disc: u32,
    pub number: u32,
}

/// The release a long video is cut into.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitAlbum {
    /// Provider and release id, which keep retries apart per release.
    pub id: String,
    pub source: String,
    pub album: String,
    pub artist: String,
    pub year: Option<u32>,
    pub genre: String,
    pub cover: String,
    pub tracks: Vec<SplitTrack>,
}

impl SplitAlbum {
    /// A release with full details and at least two songs.
    pub fn from_item(item: &OnlineItem) -> Option<Self> {
        let tracks = item.raw["tracks"]
            .as_array()?
            .iter()
            .map(|track| SplitTrack {
                title: text(track, "title").trim().to_string(),
                artist: text(track, "artist").trim().to_string(),
                seconds: number(track, "seconds").max(0.0),
                disc: number(track, "disc").max(1.0) as u32,
                number: number(track, "number").max(0.0) as u32,
            })
            .filter(|track| !track.title.is_empty())
            .collect::<Vec<_>>();
        if tracks.len() < 2 {
            return None;
        }
        let year = item
            .year
            .get(..4)
            .and_then(|year| year.parse::<u32>().ok())
            .filter(|year| *year > 0);
        Some(Self {
            id: format!("{}:{}", item.provider.name(), item.id),
            source: item.provider.name().to_string(),
            album: item.title.trim().to_string(),
            artist: item.artist.trim().to_string(),
            year,
            genre: text(&item.raw, "genre").trim().to_string(),
            cover: item.cover.clone(),
            tracks,
        })
    }
    /// The listed length of the whole album, when every song has one.
    pub fn listed_seconds(&self) -> Option<f64> {
        self.tracks
            .iter()
            .all(|track| track.seconds > 0.0)
            .then(|| self.tracks.iter().map(|track| track.seconds).sum())
    }
    /// Whether a video of this length can hold the listed songs. Uploads add
    /// a second or two per gap, pressings differ, and some lists miss a few
    /// lengths; a different edition or a different album does not fit.
    pub fn fits(&self, video_seconds: f64) -> bool {
        match self.listed_seconds() {
            Some(listed) => (video_seconds - listed).abs() <= tolerance(video_seconds),
            None => video_seconds >= 10.0 * self.tracks.len() as f64,
        }
    }
}

fn tolerance(seconds: f64) -> f64 {
    (seconds * 0.08).max(60.0)
}

/// Turns 16-bit mono PCM into loudness frames, in pieces of any size.
#[derive(Default)]
pub struct Loudness {
    frames: Vec<f32>,
    power: f64,
    samples: usize,
    odd: Option<u8>,
}

impl Loudness {
    pub fn push(&mut self, mut bytes: &[u8]) {
        if let Some(low) = self.odd.take() {
            let Some((&high, rest)) = bytes.split_first() else {
                self.odd = Some(low);
                return;
            };
            self.sample(i16::from_le_bytes([low, high]));
            bytes = rest;
        }
        let whole = bytes.len() & !1;
        for at in (0..whole).step_by(2) {
            self.sample(i16::from_le_bytes([bytes[at], bytes[at + 1]]));
        }
        if whole < bytes.len() {
            self.odd = Some(bytes[whole]);
        }
    }
    fn sample(&mut self, value: i16) {
        let value = f64::from(value) / 32768.0;
        self.power += value * value;
        self.samples += 1;
        if self.samples == FRAME {
            self.frames.push(decibels(self.power / FRAME as f64));
            self.power = 0.0;
            self.samples = 0;
        }
    }
    pub fn seconds(&self) -> f64 {
        self.frames.len() as f64 / FPS as f64
    }
    pub fn finish(self) -> Vec<f32> {
        self.frames
    }
}

fn decibels(power: f64) -> f32 {
    (10.0 * power.max(1e-12).log10()) as f32
}

/// Where each song starts, in seconds, and whether its start sits on a clear pause.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub starts: Vec<f64>,
    pub clear: Vec<bool>,
    pub end: f64,
}

impl Plan {
    pub fn clear_cuts(&self) -> usize {
        self.clear.iter().skip(1).filter(|clear| **clear).count()
    }
    /// One line for the log: each cut, its length against the list, and its kind.
    pub fn describe(&self, listed: &[f64]) -> String {
        let mut parts = Vec::new();
        for (index, start) in self.starts.iter().enumerate() {
            let end = self.starts.get(index + 1).copied().unwrap_or(self.end);
            let listed = listed.get(index).copied().unwrap_or(0.0);
            parts.push(format!(
                "{}@{start:.2}s {:.1}/{listed:.0}s{}",
                index + 1,
                end - start,
                if self.clear[index] { "" } else { "?" }
            ));
        }
        parts.join(" ")
    }
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    seconds: f64,
    strength: f64,
    pause: bool,
}

/// Plans the cuts for songs with these listed lengths (0 when unknown).
pub fn plan(frames: &[f32], listed: &[f64]) -> Result<Plan, String> {
    let count = listed.len();
    let end = frames.len() as f64 / FPS as f64;
    if count < 2 {
        return Err("The track list needs at least two songs".into());
    }
    if end < 10.0 * count as f64 {
        return Err("This video is too short for the track list".into());
    }
    let expected = expected_lengths(listed, end);
    let (candidates, lead) = candidates(frames);
    let mut points = vec![Candidate {
        seconds: lead,
        strength: 0.0,
        pause: true,
    }];
    points.extend(
        candidates
            .into_iter()
            .filter(|c| c.seconds > lead + 5.0 && c.seconds < end - 5.0),
    );
    // A cut every second as a last resort, where songs run into each other
    // without any dip: there the listed length decides.
    let mut second = (lead + 6.0).floor();
    while second < end - 5.0 {
        points.push(Candidate {
            seconds: second,
            strength: 0.0,
            pause: false,
        });
        second += 1.0;
    }
    points.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    points.push(Candidate {
        seconds: end,
        strength: 0.0,
        pause: true,
    });
    // A wider search when the list and the audio disagree more than usual.
    let path = align(&points, &expected, 1.0)
        .or_else(|| align(&points, &expected, 3.0))
        .ok_or("The track list doesn't match this video")?;
    let mut starts = vec![lead];
    let mut clear = vec![true];
    for &index in &path {
        starts.push(points[index].seconds);
        clear.push(points[index].pause && points[index].strength >= 0.5);
    }
    Ok(Plan { starts, clear, end })
}

/// Listed lengths scaled to the audio. Unknown lengths share what is left.
fn expected_lengths(listed: &[f64], end: f64) -> Vec<(f64, f64)> {
    let known: f64 = listed.iter().filter(|s| **s > 0.0).sum();
    let missing = listed.iter().filter(|s| **s <= 0.0).count();
    let scale = if missing == 0 && known > 0.0 {
        (end / known).clamp(0.9, 1.1)
    } else {
        1.0
    };
    let share = if missing > 0 {
        ((end - known) / missing as f64).max(10.0)
    } else {
        0.0
    };
    listed
        .iter()
        .map(|&seconds| {
            if seconds > 0.0 {
                let mean = seconds * scale;
                (mean, 2.0 + 0.03 * mean)
            } else {
                (share, (share * 0.5).max(30.0))
            }
        })
        .collect()
}

/// The best path of cuts: the listed lengths, measured from cut to cut, with
/// a reward for each clear pause. Returns the indices of the inner cuts.
fn align(points: &[Candidate], expected: &[(f64, f64)], widen: f64) -> Option<Vec<usize>> {
    const REWARD: f64 = 4.0;
    let tracks = expected.len();
    let last = points.len() - 1;
    let mut best = vec![vec![f64::INFINITY; points.len()]; tracks + 1];
    let mut back = vec![vec![usize::MAX; points.len()]; tracks + 1];
    best[0][0] = 0.0;
    for track in 1..=tracks {
        let (mean, spread) = expected[track - 1];
        let spread = spread * widen;
        let final_track = track == tracks;
        for j in 1..points.len() {
            if final_track != (j == last) {
                continue;
            }
            let at = points[j].seconds;
            // Only earlier points at a plausible distance.
            let from = at - mean - 4.0 * spread;
            let to = at - mean + 4.0 * spread;
            let first = points.partition_point(|p| p.seconds < from);
            let reward = if final_track {
                0.0
            } else {
                REWARD * points[j].strength
            };
            for i in first..j {
                let start = points[i].seconds;
                if start > to {
                    break;
                }
                let before = best[track - 1][i];
                if !before.is_finite() {
                    continue;
                }
                let error = ((at - start - mean) / spread).powi(2).min(16.0);
                let cost = before + error - reward;
                if cost < best[track][j] {
                    best[track][j] = cost;
                    back[track][j] = i;
                }
            }
        }
    }
    if !best[tracks][last].is_finite() {
        return None;
    }
    let mut path = Vec::with_capacity(tracks - 1);
    let mut at = back[tracks][last];
    for track in (1..tracks).rev() {
        path.push(at);
        at = back[track][at];
    }
    path.reverse();
    Some(path)
}

/// Possible song starts, and where the first song starts after any leading silence.
fn candidates(raw: &[f32]) -> (Vec<Candidate>, f64) {
    let n = raw.len();
    // A three-frame median removes single clicks from vinyl rips.
    let e: Vec<f32> = (0..n)
        .map(|i| {
            let mut w = [raw[i.saturating_sub(1)], raw[i], raw[(i + 1).min(n - 1)]];
            w.sort_by(f32::total_cmp);
            w[1]
        })
        .collect();
    let mut sums = Vec::with_capacity(n + 1);
    sums.push(0.0f64);
    for value in &e {
        sums.push(sums.last().unwrap() + 10f64.powf(f64::from(*value) / 10.0));
    }
    let level = |a: usize, b: usize| {
        let b = b.min(n);
        let power = (sums[b] - sums[a]) / (b - a).max(1) as f64;
        10.0 * power.max(1e-12).log10()
    };
    // Half-second loudness around each frame.
    const HALF: usize = 5;
    let smooth: Vec<f64> = (0..n)
        .map(|i| level(i.saturating_sub(HALF), i + HALF + 1))
        .collect();
    let blocks: Vec<f64> = smooth
        .chunks(FPS)
        .map(|block| block.iter().copied().fold(f64::MIN, f64::max))
        .collect();
    let reference = |i: usize| {
        let b = i / FPS;
        blocks[b.saturating_sub(6)..(b + 7).min(blocks.len())]
            .iter()
            .copied()
            .fold(f64::MIN, f64::max)
    };
    // Silence is relative to the quietest moments of this recording: digital
    // zero on a CD rip, surface noise on vinyl.
    let mut sorted: Vec<f32> = e.clone();
    sorted.sort_by(f32::total_cmp);
    let floor = f64::from(sorted[sorted.len() / 200]);
    let limit = (floor + 6.0).max((floor + 25.0).min(-65.0));
    let silent: Vec<bool> = e.iter().map(|v| f64::from(*v) < limit).collect();
    let onset = |from: usize, above: f64| {
        (from..(from + 3 * FPS).min(n))
            .find(|&k| f64::from(e[k]) > above)
            .unwrap_or(from)
    };
    let mut found = Vec::new();
    let mut lead = 0.0;
    let mut i = 0;
    while i < n {
        if !silent[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        loop {
            while j < n && silent[j] {
                j += 1;
            }
            let mut k = j;
            while k < n && !silent[k] && k - j < 4 {
                k += 1;
            }
            if k < n && silent[k] && k - j < 4 {
                j = k;
                continue;
            }
            break;
        }
        let length = (j - i) as f64 / FPS as f64;
        if i == 0 && j < n && length <= 10.0 {
            let on = onset(j.saturating_sub(FPS / 2), floor + 15.0);
            lead = on.saturating_sub(LEAD_FRAMES) as f64 / FPS as f64;
        } else if length >= 0.25 && i > 0 && j < n {
            let on = onset(i.max(j.saturating_sub(FPS / 2)), floor + 15.0);
            let depth = reference(i).min(reference(j.min(n - 1))) - floor;
            let quiet = e[i..j].iter().map(|v| f64::from(*v)).sum::<f64>() / (j - i) as f64;
            let closeness = ((limit - quiet) / 6.0).clamp(0.0, 1.0);
            let strength = ((depth - 10.0) / 25.0).clamp(0.0, 1.0)
                * (0.25 + length / 1.2).min(1.0)
                * (0.5 + 0.5 * closeness);
            found.push(Candidate {
                seconds: on.saturating_sub(LEAD_FRAMES).max(i) as f64 / FPS as f64,
                strength,
                pause: true,
            });
        }
        i = j;
    }
    // Songs that run into each other leave only a dip in loudness.
    let mut near = vec![false; n];
    for c in &found {
        let at = (c.seconds * FPS as f64) as usize;
        for flag in &mut near[at.saturating_sub(2 * FPS)..(at + 2 * FPS).min(n)] {
            *flag = true;
        }
    }
    for i in FPS..n.saturating_sub(FPS) {
        if near[i] || silent[i] || smooth[i + 1] <= smooth[i] {
            continue;
        }
        let window = &smooth[i - FPS..=i + FPS];
        if window.iter().any(|v| *v < smooth[i]) {
            continue;
        }
        let before = smooth[i.saturating_sub(6 * FPS)..=i]
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        let after = smooth[i..(i + 6 * FPS).min(n)]
            .iter()
            .copied()
            .fold(f64::MIN, f64::max);
        let depth = before.min(after) - smooth[i];
        if depth < 6.0 {
            continue;
        }
        let on = onset(i, smooth[i] + 10.0);
        found.push(Candidate {
            seconds: on.saturating_sub(LEAD_FRAMES).max(i.saturating_sub(HALF)) as f64 / FPS as f64,
            strength: (depth / 50.0).min(0.8),
            pause: false,
        });
    }
    found.sort_by(|a, b| a.seconds.total_cmp(&b.seconds));
    (found, lead)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic album: songs with a beat, separated by silence or not.
    struct Album {
        frames: Vec<f32>,
        starts: Vec<f64>,
    }

    fn album(lengths: &[f64], gaps: &[f64], noise: f32) -> Album {
        let mut frames = Vec::new();
        let mut starts = Vec::new();
        let mut seed = 7u32;
        let mut random = move || {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            f32::from((seed >> 16) as u16 % 1000) / 1000.0
        };
        let floor = |value: f32, noise: f32| {
            decibels(10f64.powf(f64::from(value) / 10.0) + 10f64.powf(f64::from(noise) / 10.0))
        };
        for _ in 0..30 {
            frames.push(floor(-120.0, noise));
        }
        for (index, length) in lengths.iter().enumerate() {
            starts.push(frames.len() as f64 / FPS as f64);
            let count = (length * FPS as f64) as usize;
            for k in 0..count {
                // A beat every 0.4 s over a steady bed, fading out at the end.
                let beat = if k % 8 < 2 {
                    -8.0
                } else {
                    -30.0 - 15.0 * random()
                };
                let fades = gaps.get(index).is_some_and(|gap| *gap > 0.0);
                let fade = if fades && k + 60 > count {
                    -40.0 * (k + 60 - count) as f32 / 60.0
                } else {
                    0.0
                };
                frames.push(floor(beat + fade, noise));
            }
            let gap = gaps.get(index).copied().unwrap_or(0.0);
            for _ in 0..(gap * FPS as f64) as usize {
                frames.push(floor(-120.0, noise));
            }
        }
        Album { frames, starts }
    }

    fn worst(plan: &Plan, truth: &[f64]) -> f64 {
        plan.starts
            .iter()
            .zip(truth)
            .skip(1)
            .map(|(cut, start)| (cut - (start - LEAD_FRAMES as f64 / FPS as f64)).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn cuts_land_on_pauses_even_when_listed_lengths_are_off() {
        let real = [293.0, 265.0, 284.0, 303.0, 99.0, 220.0, 228.0, 196.0];
        let gaps = [2.0, 1.5, 2.5, 0.6, 2.0, 1.8, 2.2];
        let test = album(&real, &gaps, -120.0);
        // Listed lengths a couple of seconds off, and one ten seconds short.
        let listed = [291.0, 266.0, 283.0, 301.0, 100.0, 222.0, 218.0, 197.0];
        let plan = plan(&test.frames, &listed).unwrap();
        assert!(worst(&plan, &test.starts) < 0.2, "{plan:?}");
        assert_eq!(plan.clear_cuts(), 7);
        assert!((plan.starts[0] - (1.5 - 0.25)).abs() < 0.1, "{plan:?}");
    }

    #[test]
    fn songs_without_a_pause_are_cut_near_their_listed_length() {
        let real = [200.0, 180.0, 240.0, 150.0];
        let gaps = [2.0, 0.0, 2.0];
        let test = album(&real, &gaps, -120.0);
        let plan = plan(&test.frames, &real).unwrap();
        assert!(worst(&plan, &test.starts) < 3.5, "{plan:?}");
        assert!(plan.clear[1] && !plan.clear[2] && plan.clear[3], "{plan:?}");
    }

    #[test]
    fn surface_noise_still_shows_the_pauses() {
        let real = [250.0, 210.0, 300.0, 190.0, 230.0];
        let gaps = [2.5, 2.0, 3.0, 1.5];
        let test = album(&real, &gaps, -52.0);
        let listed = [248.0, 212.0, 299.0, 191.0, 231.0];
        let plan = plan(&test.frames, &listed).unwrap();
        assert!(worst(&plan, &test.starts) < 1.0, "{plan:?}");
    }

    #[test]
    fn a_track_list_for_another_album_is_refused() {
        let test = album(&[200.0, 200.0, 200.0], &[2.0, 2.0], -120.0);
        assert!(plan(&test.frames, &[30.0, 30.0, 30.0]).is_err());
        let mut item = OnlineItem::new(crate::online::Provider::Discogs);
        item.raw = serde_json::json!({"tracks":[
            {"title":"One","seconds":200.0,"number":1},
            {"title":"Two","seconds":200.0,"number":2},
            {"title":"Three","seconds":200.0,"number":3}]});
        let split = SplitAlbum::from_item(&item).unwrap();
        assert!(split.fits(604.0) && split.fits(560.0));
        assert!(!split.fits(900.0) && !split.fits(400.0));
    }

    /// The Basley Click "The Album" upload (YouTube byWrIsWCqR4), decoded as
    /// 8 kHz mono s16le, against Discogs release 2168207. The song starts were
    /// read by ear and from the loudness. Song 9 follows song 8 without a pause.
    #[test]
    #[ignore]
    fn real_album_upload_is_cut_at_its_pauses() {
        let Some(path) = std::env::var_os("BETTERPOD_SPLIT_FIXTURE") else {
            return;
        };
        let mut meter = Loudness::default();
        meter.push(&std::fs::read(path).unwrap());
        let frames = meter.finish();
        let listed = [
            293.0, 265.0, 284.0, 303.0, 303.0, 247.0, 205.0, 184.0, 99.0, 220.0, 220.0, 323.0,
            196.0, 247.0,
        ];
        let truth = [
            1.5, 295.7, 561.3, 846.3, 1150.8, 1454.9, 1704.5, 1910.9, 2095.5, 2196.2, 2418.9,
            2647.7, 2971.2, 3169.2,
        ];
        let plan = plan(&frames, &listed).unwrap();
        eprintln!("{}", plan.describe(&listed));
        for (index, (cut, start)) in plan.starts.iter().zip(truth).enumerate() {
            let allowed = if index == 8 { 1.6 } else { 0.5 };
            assert!(
                (cut - start).abs() <= allowed,
                "song {}: {cut} vs {start}",
                index + 1
            );
        }
        assert!(plan.clear_cuts() >= 11, "{plan:?}");
    }

    #[test]
    fn loudness_accepts_odd_byte_pieces() {
        let samples: Vec<u8> = (0..FRAME * 3)
            .flat_map(|i| (if i < FRAME { 0i16 } else { 16384 }).to_le_bytes())
            .collect();
        let mut whole = Loudness::default();
        whole.push(&samples);
        let mut pieces = Loudness::default();
        for piece in samples.chunks(333) {
            pieces.push(piece);
        }
        let (whole, pieces) = (whole.finish(), pieces.finish());
        assert_eq!(whole, pieces);
        assert_eq!(whole.len(), 3);
        assert!(whole[0] < -100.0 && (whole[1] + 6.02).abs() < 0.1);
    }
}
