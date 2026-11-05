//! Five Letters: all guesses, duplicate-letter scoring, fixed green slots,
//! dictionary validation, six attempts, history and persistent statistics.
use super::{centered, color, text, Rng};
use crate::{input::Action, render::Renderer};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
const ANSWERS: &str = include_str!(concat!(env!("BETTERPOD_WORDS_SOURCE"), "/answers.txt"));
const ALLOWED: &str = include_str!(concat!(env!("BETTERPOD_WORDS_SOURCE"), "/allowed.txt"));
const ROWS: [&str; 3] = ["QWERTYUIOP", "ASDFGHJKL", "+ZXCVBNM-"];
struct Dictionary {
    answers: Vec<&'static str>,
    valid: HashSet<&'static str>,
}
fn dictionary() -> &'static Dictionary {
    static DICT: OnceLock<Dictionary> = OnceLock::new();
    DICT.get_or_init(|| {
        let answers: Vec<_> = ANSWERS
            .split_ascii_whitespace()
            .filter(|w| w.len() == 5 && w.bytes().all(|c| c.is_ascii_lowercase()))
            .collect();
        let valid = answers
            .iter()
            .copied()
            .chain(ALLOWED.split_ascii_whitespace())
            .collect();
        Dictionary { answers, valid }
    })
}
#[derive(Serialize, Deserialize)]
struct Past {
    answer: String,
    won: bool,
    tries: usize,
    gave_up: bool,
    at: u64,
}
#[derive(Default, Serialize, Deserialize)]
struct Stats {
    played: u32,
    wins: u32,
    streak: u32,
    best_streak: u32,
    distribution: [u32; 6],
    history: Vec<Past>,
}
#[derive(Serialize, Deserialize)]
pub(super) struct WordGame {
    answer: String,
    guesses: Vec<String>,
    typed: [u8; 5],
    locked: [bool; 5],
    keys: [u8; 26],
    stats: Stats,
    over: bool,
    won: bool,
    row: usize,
    col: usize,
    rng: Rng,
    menu: bool,
    menu_index: usize,
    history: bool,
    reveal: f32,
    status: String,
    status_time: f32,
}
impl WordGame {
    pub(super) fn new(seed: u64) -> Self {
        let mut game = Self {
            answer: String::new(),
            guesses: Vec::with_capacity(6),
            typed: [b' '; 5],
            locked: [false; 5],
            keys: [0; 26],
            stats: Stats::default(),
            over: false,
            won: false,
            row: 0,
            col: 0,
            rng: Rng(seed),
            menu: false,
            menu_index: 0,
            history: false,
            reveal: 2.0,
            status: String::new(),
            status_time: 0.0,
        };
        game.choose_word();
        game
    }
    fn choose_word(&mut self) {
        let answers = &dictionary().answers;
        self.answer = answers[self.rng.next() as usize % answers.len()].to_ascii_uppercase();
        self.guesses.clear();
        self.typed = [b' '; 5];
        self.locked = [false; 5];
        self.keys = [0; 26];
        self.over = false;
        self.won = false;
        self.reveal = 2.0;
        self.menu = false;
        self.history = false;
    }
    fn record(&mut self, gave_up: bool) {
        self.stats.played = self.stats.played.saturating_add(1);
        if self.won {
            self.stats.wins = self.stats.wins.saturating_add(1);
            self.stats.streak = self.stats.streak.saturating_add(1);
            self.stats.best_streak = self.stats.best_streak.max(self.stats.streak);
            let bucket = self.guesses.len().saturating_sub(1).min(5);
            self.stats.distribution[bucket] = self.stats.distribution[bucket].saturating_add(1);
        } else {
            self.stats.streak = 0;
        }
        if self.stats.history.len() == 300 {
            self.stats.history.remove(0);
        }
        self.stats.history.push(Past {
            answer: self.answer.clone(),
            won: self.won,
            tries: self.guesses.len(),
            gave_up,
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |t| t.as_secs()),
        });
    }
    fn new_word(&mut self) {
        if !self.over && (!self.guesses.is_empty() || self.typed.iter().any(|v| *v != b' ')) {
            self.record(true);
        }
        self.choose_word();
    }
    fn tell(&mut self, value: &str) {
        self.status.clear();
        self.status.push_str(value);
        self.status_time = 2.0;
    }
    fn type_letter(&mut self, key: u8) {
        if self.over {
            return;
        }
        if let Some(i) = (0..5).find(|i| self.typed[*i] == b' ' && !self.locked[*i]) {
            self.typed[i] = key;
        }
    }
    fn delete(&mut self) {
        if self.over {
            return;
        }
        if let Some(i) = (0..5)
            .rev()
            .find(|i| self.typed[*i] != b' ' && !self.locked[*i])
        {
            self.typed[i] = b' ';
        }
    }
    fn submit(&mut self) {
        if self.over || self.reveal < 1.1 {
            return;
        }
        if self.typed.contains(&b' ') {
            self.tell("Fill all five letters");
            return;
        }
        let guess = String::from_utf8(self.typed.to_vec()).unwrap();
        if !dictionary()
            .valid
            .contains(guess.to_ascii_lowercase().as_str())
        {
            self.tell("Not in the word list");
            return;
        }
        let scored = score(self.answer.as_bytes(), guess.as_bytes());
        for (i, value) in scored.iter().enumerate() {
            let k = (self.typed[i] - b'A') as usize;
            self.keys[k] = self.keys[k].max(*value);
            if *value == 3 {
                self.locked[i] = true;
            }
        }
        self.won = guess == self.answer;
        self.guesses.push(guess);
        self.over = self.won || self.guesses.len() == 6;
        if self.over {
            self.record(false);
        } else {
            for i in 0..5 {
                self.typed[i] = if self.locked[i] {
                    self.answer.as_bytes()[i]
                } else {
                    b' '
                };
            }
        }
        self.reveal = 0.0;
    }
    pub(super) fn valid(&self) -> bool {
        self.answer.len() == 5
            && self.answer.bytes().all(|c| c.is_ascii_uppercase())
            && dictionary()
                .valid
                .contains(self.answer.to_ascii_lowercase().as_str())
            && self.guesses.len() <= 6
            && self
                .guesses
                .iter()
                .all(|g| g.len() == 5 && g.bytes().all(|c| c.is_ascii_uppercase()))
            && self
                .typed
                .iter()
                .all(|c| *c == b' ' || c.is_ascii_uppercase())
            && self.keys.iter().all(|v| *v <= 3)
            && self.row < 3
            && self.col < ROWS[self.row].len()
            && self.stats.history.len() <= 300
            && self.stats.history.iter().all(|p| {
                p.answer.len() == 5
                    && p.answer.bytes().all(|c| c.is_ascii_uppercase())
                    && p.tries <= 6
            })
            && self.menu_index < 3
            && self.reveal.is_finite()
            && self.status_time.is_finite()
    }
    pub(super) fn back_consumed(&mut self) -> bool {
        if self.history {
            self.history = false;
            true
        } else if self.menu {
            self.menu = false;
            true
        } else if !self.over && (0..5).any(|i| !self.locked[i] && self.typed[i] != b' ') {
            self.delete();
            true
        } else {
            false
        }
    }

    pub(super) fn tick(&mut self, dt: f32) {
        self.reveal = (self.reveal + dt).min(2.0);
        self.status_time = (self.status_time - dt).max(0.0);
    }
    pub(super) fn handle(&mut self, action: Action) {
        if self.history {
            match action {
                Action::Select | Action::Options | Action::PlayPause | Action::Back => {
                    self.history = false
                }
                _ => {}
            }
            return;
        }
        if self.menu {
            match action {
                Action::Up => self.menu_index = (self.menu_index + 2) % 3,
                Action::Down => self.menu_index = (self.menu_index + 1) % 3,
                Action::Options | Action::PlayPause | Action::Back => self.menu = false,
                Action::Select => match self.menu_index {
                    0 => self.menu = false,
                    1 => self.new_word(),
                    _ => {
                        self.history = true;
                        self.menu = false;
                    }
                },
                _ => {}
            }
            return;
        }
        match action {
            Action::Up => {
                self.row = (self.row + 2) % 3;
                self.col = self.col.min(ROWS[self.row].len() - 1);
            }
            Action::Down => {
                self.row = (self.row + 1) % 3;
                self.col = self.col.min(ROWS[self.row].len() - 1);
            }
            Action::Left => self.col = (self.col + ROWS[self.row].len() - 1) % ROWS[self.row].len(),
            Action::Right => self.col = (self.col + 1) % ROWS[self.row].len(),
            Action::Select if self.over => self.new_word(),
            Action::Select => {
                if !self.typed.contains(&b' ') {
                    self.submit();
                } else {
                    match ROWS[self.row].as_bytes()[self.col] {
                        b'+' => self.submit(),
                        b'-' => self.delete(),
                        key => self.type_letter(key),
                    }
                }
            }
            Action::Options => self.submit(),
            Action::Previous | Action::Back => self.delete(),
            Action::PlayPause => {
                self.menu = true;
                self.menu_index = 0;
            }
            _ => {}
        }
    }
    pub(super) fn draw(&self, r: &mut Renderer, dark: bool) {
        let bg = if dark {
            color(24, 27, 33)
        } else {
            color(247, 248, 251)
        };
        let ink = if dark {
            color(232, 235, 242)
        } else {
            color(38, 42, 49)
        };
        r.rect(0., 40., 640., 440., bg);
        text(r, "Find the word", 14., 82., 20, ink, 170.);
        text(r, "in six tries.", 14., 110., 18, ink, 170.);
        text(
            r,
            &format!("Played {}", self.stats.played),
            493.,
            86.,
            16,
            ink,
            140.,
        );
        text(
            r,
            &format!("Wins {}", self.stats.wins),
            493.,
            113.,
            16,
            ink,
            140.,
        );
        text(
            r,
            &format!("Streak {}", self.stats.streak),
            493.,
            140.,
            16,
            ink,
            140.,
        );
        text(
            r,
            &format!("Best {}", self.stats.best_streak),
            493.,
            167.,
            16,
            ink,
            140.,
        );
        for row in 0..6 {
            for col in 0..5 {
                let x = 204. + col as f32 * 47.;
                let y = 57. + row as f32 * 47.;
                let (letter, state) = if let Some(guess) = self.guesses.get(row) {
                    let scored = score(self.answer.as_bytes(), guess.as_bytes());
                    let state = if row + 1 == self.guesses.len()
                        && self.reveal < (col as f32 * 0.18 + 0.15)
                    {
                        0
                    } else {
                        scored[col]
                    };
                    (guess.as_bytes()[col], state)
                } else if row == self.guesses.len() && !self.over {
                    (self.typed[col], if self.locked[col] { 3 } else { 0 })
                } else {
                    (b' ', 0)
                };
                let fill = letter_color(state, dark);
                r.rect(
                    x - 1.,
                    y - 1.,
                    44.,
                    44.,
                    if dark {
                        color(72, 77, 87)
                    } else {
                        color(195, 201, 211)
                    },
                );
                r.rect(x, y, 42., 42., fill);
                if letter != b' ' {
                    centered(
                        r,
                        &(letter as char).to_string(),
                        x,
                        y + 31.,
                        42.,
                        24,
                        if state == 0 { ink } else { [1.; 4] },
                    );
                }
            }
        }
        let message = if self.status_time > 0. {
            self.status.clone()
        } else if self.over && self.reveal >= 1.1 {
            if self.won {
                "Found it! A: new word".into()
            } else {
                format!("The word was {}. A: new word", self.answer)
            }
        } else {
            "A: type / submit    Y: submit    L: delete    START: menu".into()
        };
        centered(r, &message, 4., 354., 632., 16, ink);
        for (row, keys) in ROWS.iter().enumerate() {
            let start = (640. - keys.len() as f32 * 44.) * 0.5;
            for (col, key) in keys.bytes().enumerate() {
                let x = start + col as f32 * 44.;
                let y = 365. + row as f32 * 33.;
                let state = if key.is_ascii_uppercase() {
                    self.keys[(key - b'A') as usize]
                } else {
                    0
                };
                if self.row == row && self.col == col {
                    r.rect(x - 2., y - 2., 44., 32., color(47, 133, 229));
                }
                r.rect(x, y, 40., 28., letter_color(state, dark));
                let label = match key {
                    b'+' => "OK".to_string(),
                    b'-' => "DEL".into(),
                    v => (v as char).to_string(),
                };
                centered(
                    r,
                    &label,
                    x,
                    y + 20.,
                    40.,
                    16,
                    if state == 0 { ink } else { [1.; 4] },
                );
            }
        }
        if self.menu {
            r.rect(150., 160., 340., 157., [0.1, 0.13, 0.18, 0.98]);
            for (i, label) in ["Continue", "New word", "History"].iter().enumerate() {
                let y = 170. + i as f32 * 47.;
                if i == self.menu_index {
                    r.rect(158., y, 324., 43., color(35, 118, 218));
                }
                text(r, label, 174., y + 29., 20, [1.; 4], 290.);
            }
        }
        if self.history {
            r.rect(
                60.,
                65.,
                520.,
                365.,
                if dark {
                    color(33, 38, 47)
                } else {
                    color(235, 239, 246)
                },
            );
            centered(r, "History", 70., 98., 500., 24, ink);
            for (i, past) in self.stats.history.iter().rev().take(10).enumerate() {
                let value = format!(
                    "{}   {}   {} tries{}",
                    past.answer,
                    if past.won { "Found" } else { "Missed" },
                    past.tries,
                    if past.gave_up { " (new word)" } else { "" }
                );
                text(r, &value, 82., 132. + i as f32 * 27., 18, ink, 480.);
            }
            centered(r, "A: return", 70., 418., 500., 16, ink);
        }
    }
}
fn letter_color(state: u8, dark: bool) -> [f32; 4] {
    match state {
        1 => color(106, 112, 121),
        2 => color(199, 154, 44),
        3 => color(62, 151, 99),
        _ => {
            if dark {
                color(44, 49, 59)
            } else {
                color(226, 230, 237)
            }
        }
    }
}
fn score(answer: &[u8], guess: &[u8]) -> [u8; 5] {
    let mut result = [1; 5];
    let mut remaining = [0u8; 26];
    for i in 0..5 {
        if guess[i] == answer[i] {
            result[i] = 3;
        } else {
            remaining[(answer[i] - b'A') as usize] += 1;
        }
    }
    for i in 0..5 {
        if result[i] != 3 {
            let index = (guess[i] - b'A') as usize;
            if remaining[index] > 0 {
                result[i] = 2;
                remaining[index] -= 1;
            }
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_letters_are_not_overcredited() {
        assert_eq!(score(b"APPLE", b"ALLEY"), [3, 2, 1, 2, 1]);
        assert_eq!(score(b"LEVEL", b"LEECH"), [3, 3, 2, 1, 1]);
    }
    #[test]
    fn greens_are_kept_and_delete_does_not_remove_them() {
        let mut g = WordGame::new(3);
        g.answer = "APPLE".into();
        g.typed = *b"ALLEY";
        g.submit();
        assert!(g.locked[0]);
        assert_eq!(g.typed[0], b'A');
        g.type_letter(b'B');
        g.delete();
        assert_eq!(g.typed[0], b'A');
        assert_eq!(g.typed[1], b' ');
    }
    #[test]
    fn invalid_words_do_not_consume_attempts() {
        let mut g = WordGame::new(5);
        g.typed = *b"ZZZZZ";
        g.submit();
        assert!(g.guesses.is_empty());
        assert!(g.status_time > 0.);
    }
    #[test]
    fn win_updates_statistics_once_and_new_word_preserves_them() {
        let mut g = WordGame::new(7);
        g.answer = "APPLE".into();
        g.typed = *b"APPLE";
        g.submit();
        assert!(g.over && g.won);
        assert_eq!(g.stats.played, 1);
        assert_eq!(g.stats.wins, 1);
        g.submit();
        assert_eq!(g.stats.played, 1);
        g.new_word();
        assert!(!g.over);
        assert_eq!(g.stats.played, 1);
    }
    #[test]
    fn giving_up_counts_as_a_miss_and_resets_streak() {
        let mut g = WordGame::new(9);
        g.stats.streak = 3;
        g.type_letter(b'A');
        g.new_word();
        assert_eq!(g.stats.played, 1);
        assert_eq!(g.stats.streak, 0);
        assert!(g.stats.history[0].gave_up);
    }
}
