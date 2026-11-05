//! Guess Song owns question state only. Audio remains on the dedicated engine.
use super::{Action, AudioCommand, AudioSnapshot, Color, Library, Renderer, BLUE, WHITE};
use std::collections::HashSet;

pub(super) enum Effect {
    Play(AudioCommand),
    Pause,
    Exit,
}
pub(super) struct Quiz {
    pool: Vec<usize>,
    rng: u64,
    pub round: usize,
    score: usize,
    answers: Vec<usize>,
    correct: usize,
    selected: usize,
    answered: Option<usize>,
    start: f64,
    finished: bool,
    clip_done: bool,
    expected: std::path::PathBuf,
    armed: bool,
}
impl Quiz {
    pub fn new(library: &Library, seed: u64) -> Result<Self, String> {
        let mut titles = HashSet::new();
        let pool = library
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                !t.genre.eq_ignore_ascii_case("podcast")
                    && !t.title.trim().is_empty()
                    && titles.insert(t.title.trim().to_lowercase())
            })
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        if pool.len() < 4 {
            return Err("Guess Song needs at least four different song titles".into());
        }
        Ok(Self {
            pool,
            rng: seed.max(1),
            round: 0,
            score: 0,
            answers: Vec::new(),
            correct: 0,
            selected: 0,
            answered: None,
            start: 0.0,
            finished: false,
            clip_done: false,
            expected: std::path::PathBuf::new(),
            armed: false,
        })
    }
    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
    pub fn next(&mut self, library: &Library) -> Effect {
        if self.round >= 10 {
            self.finished = true;
            return Effect::Pause;
        }
        for i in (1..self.pool.len()).rev() {
            let j = self.random() as usize % (i + 1);
            self.pool.swap(i, j);
        }
        self.answers = self.pool[..4].to_vec();
        self.correct = self.random() as usize % 4;
        self.selected = 0;
        self.answered = None;
        self.clip_done = false;
        self.armed = false;
        self.round += 1;
        let track = &library.tracks[self.answers[self.correct]];
        self.expected = track.path.clone();
        self.start = if track.duration > 18.0 {
            (self.random() % 10000) as f64 / 10000.0 * (track.duration - 17.0)
        } else {
            0.0
        };
        Effect::Play(AudioCommand::Play {
            path: track.path.clone(),
            sample_rate: track.sample_rate,
            start: self.start,
        })
    }
    pub fn handle(&mut self, action: Action, library: &Library) -> Option<Effect> {
        if action == Action::Back {
            return Some(Effect::Exit);
        }
        if self.finished {
            return if action == Action::Select {
                Some(Effect::Exit)
            } else {
                None
            };
        }
        if self.answered.is_some() {
            return if action == Action::Select {
                Some(self.next(library))
            } else {
                None
            };
        }
        match action {
            Action::Up => self.selected = self.selected.saturating_sub(1),
            Action::Down => self.selected = (self.selected + 1).min(3),
            Action::Select => {
                self.answered = Some(self.selected);
                if self.selected == self.correct {
                    self.score += 1;
                }
                return Some(Effect::Pause);
            }
            _ => {}
        }
        None
    }
    pub fn tick(&mut self, snapshot: &AudioSnapshot) -> Option<Effect> {
        if snapshot.path.as_ref() != Some(&self.expected) {
            return None;
        }
        if !self.armed {
            if !snapshot.loading && snapshot.playing && (snapshot.position - self.start).abs() < 2.0
            {
                self.armed = true;
            } else {
                return None;
            }
        }
        if !self.finished
            && self.answered.is_none()
            && !self.clip_done
            && !snapshot.loading
            && snapshot.path.is_some()
            && snapshot.position >= self.start + 15.0
        {
            self.clip_done = true;
            return Some(Effect::Pause);
        }
        None
    }
    pub fn draw(&self, r: &mut Renderer, library: &Library, snapshot: &AudioSnapshot, dark: bool) {
        let fg: Color = if dark { WHITE } else { [0.1, 0.11, 0.13, 1.0] };
        let muted = [0.48, 0.50, 0.55, 1.0];
        if self.finished {
            r.centered("Quiz Complete", 20.0, 130.0, 600.0, 32, fg);
            r.centered(
                &format!("{} / 10 correct", self.score),
                20.0,
                210.0,
                600.0,
                30,
                BLUE,
            );
            r.centered("A: Return to your music", 20.0, 330.0, 600.0, 20, muted);
            return;
        }
        r.centered(
            &format!("Round {} of 10     Score {}", self.round, self.score),
            20.0,
            65.0,
            600.0,
            22,
            fg,
        );
        r.centered(
            if snapshot.loading {
                "Loading the 15-second clip..."
            } else if self.answered.is_some() {
                if self.answered == Some(self.correct) {
                    "Correct!"
                } else {
                    "The correct song is highlighted"
                }
            } else if self.clip_done {
                "Choose your answer"
            } else {
                "Which song is playing?"
            },
            20.0,
            105.0,
            600.0,
            20,
            muted,
        );
        for (i, &track) in self.answers.iter().enumerate() {
            let y = 159.0 + i as f32 * 58.0;
            let correct = self.answered.is_some() && i == self.correct;
            let selected = i == self.selected;
            let color = if correct {
                [0.13, 0.59, 0.32, 1.0]
            } else if selected {
                BLUE
            } else if dark {
                [0.16, 0.17, 0.20, 1.0]
            } else {
                [0.86, 0.88, 0.91, 1.0]
            };
            r.rect(28.0, y, 584.0, 49.0, color);
            r.text(
                &library.tracks[track].title,
                42.0,
                y + 13.0,
                21,
                if correct || selected { WHITE } else { fg },
                552.0,
            );
        }
        if let Some(error) = &snapshot.error {
            r.text(error, 25.0, 401.0, 17, [0.87, 0.23, 0.18, 1.0], 590.0);
        }
        r.centered(
            if self.answered.is_some() {
                "A: Next round     B: Exit and restore music"
            } else {
                "A: Answer     X: Pause clip     B: Exit"
            },
            15.0,
            444.0,
            610.0,
            17,
            muted,
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn library() -> Library {
        Library {
            tracks: (0..8)
                .map(|i| super::super::Track {
                    path: format!("/music/{i}.flac").into(),
                    title: format!("Song {i}"),
                    duration: 120.0,
                    sample_rate: 44100,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    #[test]
    fn requires_four_distinct_nonpodcast_titles() {
        let mut library = library();
        for t in &mut library.tracks {
            t.title = "Same Title".into();
        }
        assert!(Quiz::new(&library, 1).is_err());
    }
    #[test]
    fn ten_rounds_have_distinct_choices_and_correct_score() {
        let library = library();
        let mut quiz = Quiz::new(&library, 9).unwrap();
        for round in 1..=10 {
            assert!(matches!(quiz.next(&library), Effect::Play(_)));
            assert_eq!(quiz.round, round);
            assert_eq!(quiz.answers.iter().collect::<HashSet<_>>().len(), 4);
            quiz.selected = quiz.correct;
            assert!(matches!(
                quiz.handle(Action::Select, &library),
                Some(Effect::Pause)
            ));
        }
        assert!(matches!(quiz.next(&library), Effect::Pause));
        assert!(quiz.finished);
        assert_eq!(quiz.score, 10);
    }
    #[test]
    fn clip_timer_waits_for_correct_decoder_and_pauses_at_fifteen_seconds() {
        let library = library();
        let mut quiz = Quiz::new(&library, 23).unwrap();
        quiz.next(&library);
        let mut snapshot = AudioSnapshot {
            path: Some("/other.flac".into()),
            playing: true,
            position: 100.0,
            ..Default::default()
        };
        assert!(quiz.tick(&snapshot).is_none());
        snapshot.path = Some(quiz.expected.clone());
        snapshot.position = quiz.start + 0.1;
        assert!(quiz.tick(&snapshot).is_none());
        snapshot.position = quiz.start + 14.9;
        assert!(quiz.tick(&snapshot).is_none());
        snapshot.position = quiz.start + 15.0;
        assert!(matches!(quiz.tick(&snapshot), Some(Effect::Pause)));
        assert!(quiz.tick(&snapshot).is_none());
    }
}
