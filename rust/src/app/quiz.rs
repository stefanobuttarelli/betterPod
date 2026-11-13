//! Source-faithful Guess the Song. Question state never owns the audio device.
use super::feedback::{rounded_gradient, stroke_rounded};
use super::{Action, AudioCommand, AudioSnapshot, Color, Library, Renderer, WHITE};
use crate::art::Artwork;
use std::{collections::HashSet, path::PathBuf};
pub(super) enum Effect {
    Play(AudioCommand),
    Pause,
    Exit,
    Menu,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Empty,
    Loading,
    Playing,
    Reveal,
    Done,
}
pub(super) struct Quiz {
    pool: Vec<usize>,
    used: HashSet<usize>,
    rng: u64,
    pub round: usize,
    score: usize,
    best: usize,
    best_pending: Option<usize>,
    answers: Vec<usize>,
    correct: usize,
    selected: usize,
    picked: Option<usize>,
    gained: usize,
    phase: Phase,
    start: f64,
    elapsed: f64,
    expected: PathBuf,
    time: f64,
    shown_at: f64,
    selected_at: f64,
}
impl Quiz {
    pub fn new(library: &Library, seed: u64) -> Result<Self, String> {
        let pool = library
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                !t.title.is_empty()
                    && t.duration >= 30.0
                    && !t.path.to_string_lossy().contains("/Podcasts/")
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        let distinct = pool
            .iter()
            .map(|&i| library.tracks[i].title.trim().to_lowercase())
            .collect::<HashSet<_>>()
            .len();
        Ok(Self {
            pool,
            used: HashSet::new(),
            rng: seed.max(1),
            round: 0,
            score: 0,
            best: 0,
            best_pending: None,
            answers: Vec::new(),
            correct: 0,
            selected: 0,
            picked: None,
            gained: 0,
            phase: if distinct < 4 {
                Phase::Empty
            } else {
                Phase::Loading
            },
            start: 0.0,
            elapsed: 0.0,
            expected: PathBuf::new(),
            time: 0.0,
            shown_at: 0.0,
            selected_at: 0.0,
        })
    }
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    pub fn set_best(&mut self, best: usize) {
        self.best = best;
    }
    pub fn take_best(&mut self) -> Option<usize> {
        self.best_pending.take()
    }
    pub fn has_songs(&self) -> bool {
        self.phase != Phase::Empty
    }
    pub fn new_game(&mut self, library: &Library) -> Effect {
        self.round = 0;
        self.score = 0;
        self.used.clear();
        self.next(library)
    }
    pub fn next(&mut self, library: &Library) -> Effect {
        if self.phase == Phase::Empty {
            return Effect::Pause;
        }
        if self.round >= 10 {
            self.phase = Phase::Done;
            if self.score > self.best {
                self.best = self.score;
                self.best_pending = Some(self.best);
            }
            return Effect::Pause;
        }
        self.round += 1;
        let mut answer = None;
        for _ in 0..50 {
            let random = self.random() as usize;
            let index = self.pool[random % self.pool.len()];
            if !self.used.contains(&index) {
                answer = Some(index);
                break;
            }
        }
        let answer = answer.unwrap_or_else(|| {
            let random = self.random() as usize;
            self.pool[random % self.pool.len()]
        });
        self.used.insert(answer);
        self.answers = vec![answer];
        let mut titles = HashSet::from([library.tracks[answer].title.trim().to_lowercase()]);
        for _ in 0..400 {
            if self.answers.len() >= 4 {
                break;
            }
            let random = self.random() as usize;
            let index = self.pool[random % self.pool.len()];
            if titles.insert(library.tracks[index].title.trim().to_lowercase()) {
                self.answers.push(index);
            }
        }
        // A pathological RNG sequence must not leave an inaccessible answer.
        if self.answers.len() < 4 {
            for &index in &self.pool {
                if titles.insert(library.tracks[index].title.trim().to_lowercase()) {
                    self.answers.push(index);
                }
                if self.answers.len() == 4 {
                    break;
                }
            }
        }
        for i in (1..self.answers.len()).rev() {
            let j = self.random() as usize % (i + 1);
            self.answers.swap(i, j);
        }
        self.correct = self.answers.iter().position(|&i| i == answer).unwrap_or(0);
        self.selected = 0;
        self.picked = None;
        self.gained = 0;
        self.elapsed = 0.0;
        self.phase = Phase::Loading;
        self.shown_at = self.time;
        self.selected_at = self.time;
        let t = &library.tracks[answer];
        self.expected = t.path.clone();
        self.start = t.duration * (0.25 + 0.3 * (self.random() % 1_000_000) as f64 / 1_000_000.0);
        Effect::Play(AudioCommand::Play {
            path: t.path.clone(),
            sample_rate: t.sample_rate,
            start: self.start,
        })
    }
    fn reveal(&mut self, picked: Option<usize>) {
        self.picked = picked;
        self.gained = if picked == Some(self.correct) {
            score_for(self.elapsed)
        } else {
            0
        };
        self.score += self.gained;
        self.phase = Phase::Reveal;
        self.shown_at = self.time;
    }
    pub fn handle(&mut self, action: Action, library: &Library) -> Option<Effect> {
        match action {
            Action::Back => return Some(Effect::Exit),
            Action::Start => return Some(Effect::Menu),
            Action::Up | Action::Down if matches!(self.phase, Phase::Playing | Phase::Loading) => {
                let n = self.answers.len();
                if n > 0 {
                    self.selected = if action == Action::Up {
                        (self.selected + n - 1) % n
                    } else {
                        (self.selected + 1) % n
                    };
                    self.selected_at = self.time;
                }
            }
            Action::Select => match self.phase {
                Phase::Playing => self.reveal(Some(self.selected)),
                Phase::Reveal => return Some(self.next(library)),
                Phase::Done => return Some(self.new_game(library)),
                _ => {}
            },
            _ => {}
        }
        None
    }
    pub fn tick(&mut self, snapshot: &AudioSnapshot) -> Option<Effect> {
        if snapshot.path.as_ref() != Some(&self.expected) {
            return None;
        }
        if self.phase == Phase::Loading
            && !snapshot.loading
            && snapshot.playing
            && (snapshot.position - self.start).abs() < 2.0
        {
            self.phase = Phase::Playing;
            self.elapsed = 0.0;
            self.shown_at = self.time;
        }
        if self.phase == Phase::Playing {
            self.elapsed = (snapshot.position - self.start).max(0.0);
            if self.elapsed >= 15.0 {
                self.reveal(None);
            }
        }
        None
    }
    pub fn draw(
        &mut self,
        r: &mut Renderer,
        library: &Library,
        _snapshot: &AudioSnapshot,
        _dark: bool,
        art: &mut Artwork,
        now: f64,
    ) {
        self.time = now;
        let ink = rgb(36, 40, 48);
        let sub = rgb(96, 104, 118);
        r.gradient(
            0.0,
            40.0,
            640.0,
            440.0,
            rgb(238, 241, 245),
            rgb(210, 216, 225),
        );
        if self.phase == Phase::Empty {
            center(r, "Not enough songs", 320.0, 210.0, 32.0, 700, ink);
            center(
                r,
                "Guess the Song needs at least four songs",
                320.0,
                260.0,
                26.0,
                500,
                sub,
            );
            center(
                r,
                "with different titles in your library.",
                320.0,
                292.0,
                26.0,
                500,
                sub,
            );
            return;
        }
        if self.phase == Phase::Done {
            center(r, "Your score", 320.0, 130.0, 28.0, 700, sub);
            center(r, &self.score.to_string(), 320.0, 170.0, 80.0, 700, ink);
            let text = if self.score >= self.best && self.score > 0 {
                "A new best!".into()
            } else {
                format!("Best: {}", self.best)
            };
            center(r, &text, 320.0, 290.0, 28.0, 600, sub);
            center(
                r,
                "[A] play again · [START] menu",
                320.0,
                370.0,
                26.0,
                500,
                sub,
            );
            return;
        }
        r.text_weighted(
            &format!("Song {} of 10", self.round),
            24.0,
            62.0,
            24.0,
            700,
            sub,
            400.0,
        );
        let score = format!("Score {}", self.score);
        let sw = r.text_width_weighted(&score, 24.0, 700);
        r.text_weighted(&score, 616.0 - sw, 62.0, 24.0, 700, sub, sw + 1.0);
        let left = match self.phase {
            Phase::Playing => (1.0 - self.elapsed / 15.0).clamp(0.0, 1.0),
            Phase::Reveal => 0.0,
            _ => 1.0,
        };
        rounded_gradient(
            r,
            24.0,
            96.0,
            592.0,
            10.0,
            5.0,
            &[(0.0, [0.0, 0.0, 0.0, 0.12]), (1.0, [0.0, 0.0, 0.0, 0.06])],
        );
        if left > 0.0 {
            let g = if left < 0.25 { red() } else { blue() };
            rounded_gradient(
                r,
                24.0,
                96.0,
                (592.0 * left as f32).max(10.0),
                10.0,
                5.0,
                &g,
            );
        }
        let (ax, ay, size) = (24.0, 132.0, 260.0);
        if let Some(&answer) = self.answers.get(self.correct) {
            let texture = art.get(library.tracks[answer].cover.as_deref());
            if self.phase == Phase::Reveal {
                r.image(texture, ax, ay, size, size, 1.0);
            } else {
                rounded_gradient(
                    r,
                    ax,
                    ay,
                    size,
                    size,
                    18.0,
                    &[(0.0, rgb(110, 120, 140)), (1.0, rgb(43, 50, 64))],
                );
                rounded_gradient(
                    r,
                    ax + 3.0,
                    ay + 3.0,
                    size - 6.0,
                    size * 0.5 - 3.0,
                    16.0,
                    &[(0.0, [1.0, 1.0, 1.0, 0.35]), (1.0, [1.0, 1.0, 1.0, 0.04])],
                );
                if self.phase == Phase::Playing {
                    for i in 0..5 {
                        let t = self.elapsed;
                        let h = 30.0
                            + 50.0
                                * (0.5 + 0.5 * (t * 5.3 + i as f64 * 1.7).sin())
                                * (0.6 + 0.4 * (t * 2.1 + i as f64).sin());
                        rounded_gradient(
                            r,
                            ax + 54.0 + i as f32 * 26.0,
                            ay + size - 46.0 - h as f32,
                            16.0,
                            h as f32,
                            5.0,
                            &blue(),
                        );
                    }
                } else {
                    center(r, "?", ax + size * 0.5, ay + 60.0, 104.0, 700, WHITE);
                }
            }
        }
        for (i, &index) in self.answers.iter().enumerate() {
            let track = &library.tracks[index];
            let y = ay + i as f32 * 67.0;
            let mut text = ink;
            let mut secondary = sub;
            let grad = if self.phase == Phase::Reveal && i == self.correct {
                Some(green())
            } else if self.phase == Phase::Reveal && self.picked == Some(i) {
                Some(red())
            } else if self.phase != Phase::Reveal && i == self.selected {
                Some(blue())
            } else {
                None
            };
            if let Some(grad) = grad {
                rounded_gradient(r, 304.0, y, 312.0, 59.0, 10.0, &grad);
                text = WHITE;
                secondary = [1.0, 1.0, 1.0, 0.85];
            } else {
                rounded_gradient(
                    r,
                    304.0,
                    y,
                    312.0,
                    59.0,
                    10.0,
                    &[(0.0, WHITE), (1.0, rgb(244, 246, 249))],
                );
                stroke_rounded(r, 304.0, y, 312.0, 59.0, 10.0, 2.0, [0.0, 0.0, 0.0, 0.12]);
            }
            let asc = crate::render::line_box(26.0).1;
            let ty = if self.phase == Phase::Reveal {
                y + 8.0
            } else {
                y + (59.0 - asc) * 0.5 - 2.0
            };
            let live = (self.phase != Phase::Reveal && i == self.selected)
                || (self.phase == Phase::Reveal && i == self.correct);
            title(
                r,
                &track.title,
                318.0,
                ty + asc,
                284.0,
                26.0,
                700,
                text,
                live,
                now - if self.phase == Phase::Reveal {
                    self.shown_at
                } else {
                    self.selected_at
                },
            );
            if self.phase == Phase::Reveal {
                let text = super::visual_menu::ellipsize(r, &track.artist, 22.0, 500, 284.0);
                r.text_weighted(&text, 318.0, y + 33.0, 22.0, 500, secondary, 284.0);
            }
        }
        let mut message = match self.phase {
            Phase::Loading => "Listen…".into(),
            Phase::Playing => "[↑] [↓] choose · [A] answer".into(),
            Phase::Reveal => {
                if self.picked.is_none() {
                    "Time's up · [A] next song".into()
                } else if self.gained > 0 {
                    format!("Right! +{} · [A] next song", self.gained)
                } else {
                    "Not this one · [A] next song".into()
                }
            }
            _ => String::new(),
        };
        if self.phase == Phase::Reveal && self.round >= 10 {
            message = message.replace("next song", "see your score");
        }
        center(r, &message, 320.0, 412.0, 24.0, 600, sub);
        if self.phase == Phase::Reveal {
            let track = &library.tracks[self.answers[self.correct]];
            let text = if track.album.is_empty() {
                track.artist.clone()
            } else {
                format!("{} · {}", track.artist, track.album)
            };
            let text = super::visual_menu::ellipsize(r, &text, 22.0, 500, 590.0);
            center(r, &text, 320.0, 444.0, 22.0, 500, sub);
        }
    }
}
fn score_for(seconds: f64) -> usize {
    20 + (80.0 * (1.0 - (seconds / 15.0).clamp(0.0, 1.0)) + 0.5).floor() as usize
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
fn blue() -> [(f32, Color); 2] {
    [(0.0, rgb(60, 184, 255)), (1.0, rgb(52, 122, 181))]
}
fn red() -> [(f32, Color); 2] {
    [(0.0, rgb(255, 120, 112)), (1.0, rgb(200, 36, 40))]
}
fn green() -> [(f32, Color); 2] {
    [(0.0, rgb(110, 214, 130)), (1.0, rgb(24, 140, 66))]
}
fn center(r: &mut Renderer, text: &str, x: f32, y: f32, size: f32, weight: u16, color: Color) {
    let w = r.text_width_weighted(text, size, weight);
    r.text_weighted(text, x - w * 0.5, y, size, weight, color, w + 1.0);
}
fn title(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    width: f32,
    size: f32,
    weight: u16,
    color: Color,
    live: bool,
    elapsed: f64,
) {
    let full = r.text_width_weighted(text, size, weight);
    if !live || full <= width {
        let text = super::visual_menu::ellipsize(r, text, size, weight, width);
        r.text_baseline(&text, x, baseline, size, weight, color, width);
        return;
    }
    let offset = if elapsed < 1.5 {
        0.0
    } else {
        ((elapsed - 1.5) * 64.0 % (full as f64 + 80.0)) as f32
    };
    r.push_clip(x, baseline - size, width, size + 12.0);
    r.text_baseline(text, x - offset, baseline, size, weight, color, full + 1.0);
    r.text_baseline(
        text,
        x - offset + full + 80.0,
        baseline,
        size,
        weight,
        color,
        full + 1.0,
    );
    r.pop_clip();
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Track;
    fn library(n: usize) -> Library {
        Library {
            tracks: (0..n)
                .map(|i| Track {
                    path: PathBuf::from(format!("/Music/{i}.wav")),
                    title: format!("Song {i}"),
                    duration: 100.0,
                    ..Track::default()
                })
                .collect(),
            ..Library::default()
        }
    }
    #[test]
    fn source_pool_requires_long_music_but_keeps_duplicate_recordings() {
        let mut l = library(5);
        l.tracks[4].title = l.tracks[0].title.clone();
        assert_eq!(Quiz::new(&l, 1).unwrap().pool.len(), 5);
        l.tracks[0].duration = 29.9;
        l.tracks[1].path = "/Music/Podcasts/episode.wav".into();
        let q = Quiz::new(&l, 1).unwrap();
        assert_eq!(q.phase, Phase::Empty);
    }
    #[test]
    fn source_scoring_uses_elapsed_time() {
        assert_eq!(score_for(0.0), 100);
        assert_eq!(score_for(7.5), 60);
        assert_eq!(score_for(15.0), 20);
    }
    #[test]
    fn questions_do_not_repeat_until_pool_exhausted_and_clip_is_middle() {
        let l = library(10);
        let mut q = Quiz::new(&l, 7).unwrap();
        let mut used = HashSet::new();
        for _ in 0..10 {
            let effect = q.next(&l);
            assert!(matches!(effect, Effect::Play(_)));
            assert!(used.insert(q.answers[q.correct]));
            assert!((25.0..55.0).contains(&q.start));
            assert_eq!(
                q.answers
                    .iter()
                    .map(|&i| l.tracks[i].title.clone())
                    .collect::<HashSet<_>>()
                    .len(),
                4
            );
        }
    }
    #[test]
    fn timeout_reveals_answer_without_stopping_clip() {
        let l = library(4);
        let mut q = Quiz::new(&l, 4).unwrap();
        q.next(&l);
        let mut snapshot = AudioSnapshot {
            path: Some(q.expected.clone()),
            playing: true,
            position: q.start,
            ..AudioSnapshot::default()
        };
        q.tick(&snapshot);
        snapshot.position = q.start + 15.0;
        assert!(q.tick(&snapshot).is_none());
        assert_eq!(q.phase, Phase::Reveal);
        assert_eq!(q.picked, None);
    }
}
