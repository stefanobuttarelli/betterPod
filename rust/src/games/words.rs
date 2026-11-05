//! Five Letters: all guesses, duplicate-letter scoring, fixed green slots,
//! dictionary validation, six attempts, history and persistent statistics.
use super::{draw, GameHistoryRow, Rng};
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
    #[serde(alias = "word")]
    answer: String,
    won: bool,
    tries: usize,
    #[serde(default, alias = "gave")]
    gave_up: bool,
    #[serde(alias = "when")]
    at: u64,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Stats {
    played: u32,
    wins: u32,
    streak: u32,
    #[serde(alias = "maxStreak")]
    best_streak: u32,
    #[serde(alias = "dist")]
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
    #[serde(skip)]
    typed_elapsed: f32,
    #[serde(skip)]
    shake_elapsed: f32,
    #[serde(skip)]
    last_at: usize,
}
impl WordGame {
    pub(super) fn restore_go_fixture(&mut self, value: &serde_json::Value) -> Result<(), String> {
        let answer = value
            .get("answer")
            .and_then(|v| v.as_str())
            .ok_or("Missing reference answer")?;
        if answer.len() != 5 || !answer.bytes().all(|c| c.is_ascii_uppercase()) {
            return Err("Invalid reference answer".into());
        }
        let guesses: Vec<String> = serde_json::from_value(
            value
                .get("guesses")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|e| e.to_string())?;
        if guesses.len() > 6
            || guesses
                .iter()
                .any(|g| g.len() != 5 || !g.bytes().all(|c| c.is_ascii_uppercase()))
        {
            return Err("Invalid reference guesses".into());
        }
        self.answer = answer.into();
        self.guesses = guesses;
        self.typed = [b' '; 5];
        self.locked = [false; 5];
        self.keys = [0; 26];
        for guess in &self.guesses {
            for (i, &state) in score(self.answer.as_bytes(), guess.as_bytes())
                .iter()
                .enumerate()
            {
                let key = (guess.as_bytes()[i] - b'A') as usize;
                self.keys[key] = self.keys[key].max(state);
                if state == 3 {
                    self.locked[i] = true;
                    self.typed[i] = self.answer.as_bytes()[i];
                }
            }
        }
        if let Some(typed) = value.get("typed").and_then(|v| v.as_str()) {
            if typed.len() == 5 {
                for (i, c) in typed.bytes().enumerate() {
                    if !self.locked[i] && (c == b' ' || c.is_ascii_uppercase()) {
                        self.typed[i] = c;
                    }
                }
            }
        }
        if let Some(stats) = value.get("stats") {
            self.stats = serde_json::from_value(stats.clone()).map_err(|e| e.to_string())?;
        }
        self.won = self.guesses.last().is_some_and(|g| g == &self.answer);
        self.over = self.won || self.guesses.len() == 6;
        self.row = value.get("kr").and_then(|v| v.as_u64()).unwrap_or(0) as usize % 3;
        self.col = (value.get("kc").and_then(|v| v.as_u64()).unwrap_or(0) as usize)
            .min(ROWS[self.row].len() - 1);
        self.reveal = value
            .get("revealElapsed")
            .and_then(|v| v.as_f64())
            .unwrap_or(2.0) as f32;
        self.typed_elapsed = 1.0;
        self.shake_elapsed = 1.0;
        self.status_time = 0.0;
        if !self.valid() {
            return Err("Invalid reference word state".into());
        }
        Ok(())
    }
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
            typed_elapsed: 1.0,
            shake_elapsed: 1.0,
            last_at: 0,
        };
        game.choose_word();
        game
    }
    fn choose_word(&mut self) {
        let answers = &dictionary().answers;
        let previous = self.answer.clone();
        loop {
            self.answer = answers[self.rng.next() as usize % answers.len()].to_ascii_uppercase();
            if self.answer != previous {
                break;
            }
        }
        self.guesses.clear();
        self.typed = [b' '; 5];
        self.locked = [false; 5];
        self.keys = [0; 26];
        self.over = false;
        self.won = false;
        self.reveal = 2.0;
        self.menu = false;
        self.history = false;
        self.status.clear();
        self.status_time = 0.0;
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
    pub(super) fn new_word(&mut self) {
        let gave_up = !self.over && !self.guesses.is_empty();
        let answer = self.answer.clone();
        if gave_up {
            self.record(true);
        }
        self.choose_word();
        if gave_up {
            self.tell(&format!("It was {answer}"));
        }
    }
    pub(super) fn new_word_label(&self) -> &str {
        if !self.over && !self.guesses.is_empty() {
            "New Word (this one is missed)"
        } else {
            "New Word"
        }
    }
    pub(super) fn show_history(&mut self) {
        self.history = true;
        self.menu = false;
    }
    fn tell(&mut self, value: &str) {
        self.status.clear();
        self.status.push_str(value);
        self.status_time = 1.8;
    }
    fn type_letter(&mut self, key: u8) {
        if self.over {
            return;
        }
        if let Some(i) = (0..5).find(|i| self.typed[*i] == b' ' && !self.locked[*i]) {
            self.typed[i] = key;
            self.typed_elapsed = 0.0;
            self.last_at = i;
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
        if self.over || self.reveal < 1.02 {
            return;
        }
        if self.typed.contains(&b' ') {
            self.tell("Not enough letters");
            self.shake_elapsed = 0.0;
            return;
        }
        let guess = String::from_utf8(self.typed.to_vec()).unwrap();
        if !dictionary()
            .valid
            .contains(guess.to_ascii_lowercase().as_str())
        {
            self.tell("Not in the word list");
            self.shake_elapsed = 0.0;
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
        self.delete();
        true
    }
    pub(super) fn tick(&mut self, dt: f32) {
        self.reveal = (self.reveal + dt).min(2.0);
        self.status_time = (self.status_time - dt).max(0.0);
        self.typed_elapsed += dt;
        self.shake_elapsed += dt;
    }
    pub(super) fn handle(&mut self, action: Action) {
        match action {
            Action::Up | Action::Down => {
                let center = key_center(self.row, self.col);
                self.row = if action == Action::Up {
                    (self.row + 2) % 3
                } else {
                    (self.row + 1) % 3
                };
                self.col = (0..ROWS[self.row].len())
                    .min_by(|a, b| {
                        (key_center(self.row, *a) - center)
                            .abs()
                            .total_cmp(&(key_center(self.row, *b) - center).abs())
                    })
                    .unwrap_or(0);
            }
            Action::Left => self.col = (self.col + ROWS[self.row].len() - 1) % ROWS[self.row].len(),
            Action::Right => self.col = (self.col + 1) % ROWS[self.row].len(),
            Action::Options => self.submit(),
            Action::Back => self.delete(),
            Action::Select => {
                let key = ROWS[self.row].as_bytes()[self.col];
                if self.over && self.reveal >= 1.02 {
                    self.new_word();
                } else if !self.typed.contains(&b' ') && key != b'-' {
                    self.submit();
                } else if !self.over && self.reveal >= 1.02 {
                    match key {
                        b'+' => self.submit(),
                        b'-' => self.delete(),
                        key => self.type_letter(key),
                    }
                }
            }
            _ => {}
        }
    }
    pub(super) fn history_rows(&self) -> Vec<GameHistoryRow> {
        let mut rows = Vec::with_capacity(self.stats.history.len() + 1);
        if !self.stats.history.is_empty() {
            let count = self.stats.history.len();
            let found = self.stats.history.iter().filter(|past| past.won).count();
            rows.push(GameHistoryRow {
                label: format!(
                    "{} {} · {} found",
                    count,
                    if count == 1 { "word" } else { "words" },
                    found
                ),
                detail: String::new(),
                header: true,
            });
        }
        for past in self.stats.history.iter().rev() {
            let date = history_date(past.at);
            let detail = if past.won && past.tries == 1 {
                format!("Found at the first try · {date}")
            } else if past.won {
                format!("Found in {} tries · {date}", past.tries)
            } else if past.gave_up {
                format!(
                    "Missed · a new word after {} {} · {date}",
                    past.tries,
                    if past.tries == 1 { "try" } else { "tries" }
                )
            } else {
                format!("Missed · {date}")
            };
            let mut letters = past.answer.chars();
            let label = letters.next().map_or_else(String::new, |first| {
                format!("{}{}", first, letters.as_str().to_ascii_lowercase())
            });
            rows.push(GameHistoryRow {
                label,
                detail,
                header: false,
            });
        }
        rows
    }
    pub(super) fn after_restore(&mut self) {
        if self.over {
            self.choose_word();
        }
        self.reveal = 2.0;
        self.typed_elapsed = 1.0;
        self.shake_elapsed = 1.0;
    }
    pub(super) fn draw(&self, r: &mut Renderer, _dark: bool) {
        use crate::render::line_box;
        use draw::{centered, rgb, rounded, stroke_rounded};
        r.push_clip(0.0, 40.0, 640.0, 440.0);
        r.push_transform(0.0, 40.0, 1.0, 1.0, 1.0);
        r.gradient(
            0.0,
            0.0,
            640.0,
            440.0,
            rgb(238, 241, 245),
            rgb(206, 212, 221),
        );
        let x0 = 220.0;
        for row in 0..6 {
            let cy = 10.0 + row as f32 * 41.0 + 18.0;
            let shown = row < self.guesses.len();
            let word = if shown {
                self.guesses[row].as_bytes()
            } else if row == self.guesses.len() && !self.over {
                &self.typed
            } else {
                b"     "
            };
            let scored = if shown {
                score(self.answer.as_bytes(), word)
            } else {
                [0; 5]
            };
            let dx = if row == self.guesses.len() && self.shake_elapsed < 0.420 {
                let p = self.shake_elapsed / 0.420;
                10.0 * (p * std::f32::consts::PI * 6.0).sin() * (1.0 - p)
            } else {
                0.0
            };
            for column in 0..5 {
                let cx = x0 + column as f32 * 41.0 + 18.0 + dx;
                let ch = word[column];
                let mut state = if shown { scored[column] } else { 0 };
                let mut half_height = 18.0;
                let mut half_width = 18.0;
                if shown && row + 1 == self.guesses.len() {
                    let elapsed = self.reveal - column as f32 * 0.180;
                    if elapsed < 0.0 {
                        state = 0;
                    } else if elapsed < 0.300 {
                        let p = elapsed / 0.300;
                        half_height = 18.0 * (p * std::f32::consts::PI).cos().abs();
                        if p < 0.5 {
                            state = 0;
                        }
                    }
                }
                if !shown && ch != b' ' && column == self.last_at && self.typed_elapsed < 0.150 {
                    let scale =
                        1.0 + 0.1 * (std::f32::consts::PI * self.typed_elapsed / 0.150).sin();
                    half_width *= scale;
                    half_height *= scale;
                }
                if state == 0 && ch == b' ' {
                    let current = row == self.guesses.len()
                        && !self.over
                        && self.typed.iter().position(|b| *b == b' ') == Some(column);
                    stroke_rounded(
                        r,
                        cx - half_width,
                        cy - half_height,
                        half_width * 2.0,
                        half_height * 2.0,
                        7.0,
                        if current { 3.0 } else { 2.0 },
                        if current {
                            rgb(40, 130, 230)
                        } else {
                            rgb(186, 194, 206)
                        },
                    );
                    continue;
                }
                word_tile(
                    r,
                    cx,
                    cy,
                    half_width,
                    half_height,
                    state,
                    if ch == b' ' { None } else { Some(ch) },
                    26.0,
                );
            }
        }
        if !self.over
            && self.guesses.len() < 6
            && !self.typed.contains(&b' ')
            && self.reveal >= 1.02
            && self.shake_elapsed >= 0.420
        {
            let y = 10.0 + self.guesses.len() as f32 * 41.0;
            stroke_rounded(
                r,
                x0 - 5.0,
                y - 5.0,
                210.0,
                46.0,
                10.0,
                3.0,
                rgb(40, 130, 230),
            );
            r.text_baseline(
                "[A]",
                431.0,
                y + 18.0 + line_box(24.0).1 * 0.5 - 1.0,
                24.0,
                600,
                rgb(40, 130, 230),
                80.0,
            );
        }
        let mut key_states = [0u8; 26];
        let shown = self
            .guesses
            .len()
            .saturating_sub(usize::from(self.reveal < 1.02));
        for guess in self.guesses.iter().take(shown) {
            let values = score(self.answer.as_bytes(), guess.as_bytes());
            for (i, key) in guess.bytes().enumerate() {
                key_states[(key - b'A') as usize] =
                    key_states[(key - b'A') as usize].max(values[i]);
            }
        }
        for (row, keys) in ROWS.iter().enumerate() {
            for (column, key) in keys.bytes().enumerate() {
                let (x, y, w) = key_rect(row, column);
                let state = if key.is_ascii_uppercase() {
                    key_states[(key - b'A') as usize]
                } else {
                    0
                };
                let selected = self.row == row && self.col == column;
                let (mut top, mut bottom, mut ink) = word_colors(state);
                if state == 0 {
                    top = rgb(252, 253, 255);
                    bottom = rgb(222, 227, 235);
                    if selected {
                        top = rgb(88, 172, 248);
                        bottom = rgb(28, 110, 214);
                        ink = [1.0; 4];
                    }
                }
                rounded(r, x, y, w, 42.0, 8.0, &[(0.0, top), (1.0, bottom)]);
                if state == 0 && !selected {
                    stroke_rounded(r, x, y, w, 42.0, 8.0, 1.0, rgb(170, 178, 192));
                }
                rounded(
                    r,
                    x + 2.0,
                    y + 2.0,
                    w - 4.0,
                    19.0,
                    7.0,
                    &[(0.0, [1.0, 1.0, 1.0, 0.4]), (1.0, [1.0, 1.0, 1.0, 0.04])],
                );
                if selected && state != 0 {
                    stroke_rounded(
                        r,
                        x - 3.0,
                        y - 3.0,
                        w + 6.0,
                        48.0,
                        10.0,
                        3.0,
                        rgb(28, 120, 230),
                    );
                }
                let cx = x + w * 0.5;
                let cy = y + 21.0;
                if key == b'-' {
                    word_delete(r, cx, cy, ink);
                } else {
                    let label = if key == b'+' {
                        "ENTER".into()
                    } else {
                        (key as char).to_string()
                    };
                    let size = if key == b'+' { 20.0 } else { 26.0 };
                    centered(
                        r,
                        &label,
                        cx,
                        cy + line_box(size).1 * 0.5 - 2.0,
                        size,
                        700,
                        ink,
                    );
                }
            }
        }
        let hints = if self.over {
            ["[A] new word", "", "", "[START] menu"]
        } else {
            ["[A] type", "[Y] send", "[B] delete", "[START] menu"]
        };
        for (i, hint) in hints.iter().enumerate() {
            r.text_baseline(
                hint,
                470.0,
                40.0 + i as f32 * 28.0 + line_box(20.0).1 * 0.5,
                20.0,
                600,
                rgb(110, 118, 132),
                170.0,
            );
        }
        let percent = if self.stats.played > 0 {
            self.stats.wins * 100 / self.stats.played
        } else {
            0
        };
        for (y, value, label) in [
            (44.0, self.stats.played.to_string(), "PLAYED"),
            (124.0, format!("{percent}%"), "WON"),
            (204.0, self.stats.streak.to_string(), "STREAK"),
        ] {
            centered(
                r,
                &value,
                92.0,
                y + line_box(40.0).1 * 0.5,
                40.0,
                700,
                rgb(58, 64, 76),
            );
            centered(
                r,
                label,
                92.0,
                y + 30.0 + line_box(18.0).1 * 0.5,
                18.0,
                600,
                rgb(110, 118, 132),
            );
        }
        if self.over && self.reveal >= 1.02 {
            let title = if self.won {
                [
                    "Genius!",
                    "Magnificent!",
                    "Impressive!",
                    "Splendid!",
                    "Great!",
                    "Phew!",
                ][self.guesses.len().saturating_sub(1).min(5)]
                .into()
            } else {
                format!("The word was {}", self.answer)
            };
            word_banner(r, &title, self.reveal - 1.02, self.guesses.len());
        } else if self.status_time > 0.0 {
            word_banner(
                r,
                &self.status,
                1.8 - self.status_time,
                self.guesses.len() + 1,
            );
        }
        r.pop_transform();
        r.pop_clip();
    }
}
fn key_rect(row: usize, column: usize) -> (f32, f32, f32) {
    let widths: Vec<f32> = ROWS[row]
        .bytes()
        .map(|key| {
            if key == b'+' || key == b'-' {
                86.0
            } else {
                56.0
            }
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + widths.len().saturating_sub(1) as f32 * 6.0;
    let x = (640.0 - total) * 0.5 + widths.iter().take(column).sum::<f32>() + column as f32 * 6.0;
    (x, 294.0 + row as f32 * 48.0, widths[column])
}
fn key_center(row: usize, column: usize) -> f32 {
    let (x, _, width) = key_rect(row, column);
    x + width * 0.5
}
fn word_colors(
    state: u8,
) -> (
    crate::render::Color,
    crate::render::Color,
    crate::render::Color,
) {
    use draw::rgb;
    match state {
        3 => (rgb(118, 206, 120), rgb(40, 150, 62), [1.0; 4]),
        2 => (rgb(255, 196, 92), rgb(230, 132, 16), [1.0; 4]),
        1 => (rgb(150, 158, 172), rgb(104, 112, 128), [1.0; 4]),
        _ => ([1.0; 4], rgb(232, 236, 242), rgb(40, 44, 52)),
    }
}
#[allow(clippy::too_many_arguments)]
fn word_tile(
    r: &mut Renderer,
    cx: f32,
    cy: f32,
    half_width: f32,
    half_height: f32,
    state: u8,
    letter: Option<u8>,
    size: f32,
) {
    use draw::{centered, rgb, rounded, stroke_rounded};
    if half_height < 0.5 {
        return;
    }
    let (top, bottom, ink) = word_colors(state);
    let radius = 7.0f32.min(half_height);
    rounded(
        r,
        cx - half_width,
        cy - half_height,
        half_width * 2.0,
        half_height * 2.0,
        radius,
        &[(0.0, top), (1.0, bottom)],
    );
    if state == 0 {
        stroke_rounded(
            r,
            cx - half_width,
            cy - half_height,
            half_width * 2.0,
            half_height * 2.0,
            radius,
            2.0,
            rgb(150, 160, 176),
        );
    } else {
        rounded(
            r,
            cx - half_width + 2.0,
            cy - half_height + 2.0,
            half_width * 2.0 - 4.0,
            half_height - 2.0,
            (radius - 1.0).max(0.0),
            &[(0.0, [1.0, 1.0, 1.0, 0.42]), (1.0, [1.0, 1.0, 1.0, 0.05])],
        );
    }
    if let Some(letter) = letter {
        if half_height > half_width * 0.45 {
            centered(
                r,
                &(letter as char).to_string(),
                cx,
                cy + crate::render::line_box(size).1 * 0.5 - 2.0,
                size,
                700,
                ink,
            );
        }
    }
}
fn word_banner(r: &mut Renderer, text: &str, elapsed: f32, row: usize) {
    let opacity = (elapsed / 0.150).clamp(0.0, 1.0);
    if opacity <= 0.0 {
        return;
    }
    let width = r.text_width_weighted(text, 28.0, 700) + 44.0;
    let cy = 10.0 + row.min(5) as f32 * 41.0 + 18.0;
    draw::rounded(
        r,
        320.0 - width * 0.5,
        cy - 20.0,
        width,
        40.0,
        20.0,
        &[
            (
                0.0,
                [62.0 / 255.0, 68.0 / 255.0, 80.0 / 255.0, 0.94 * opacity],
            ),
            (
                1.0,
                [30.0 / 255.0, 34.0 / 255.0, 42.0 / 255.0, 0.94 * opacity],
            ),
        ],
    );
    draw::centered(
        r,
        text,
        320.0,
        cy + crate::render::line_box(28.0).1 * 0.5 - 2.0,
        28.0,
        700,
        [1.0, 1.0, 1.0, opacity],
    );
}
fn word_delete(r: &mut Renderer, cx: f32, cy: f32, color: crate::render::Color) {
    let pts = [
        (cx - 15.0, cy),
        (cx - 6.0, cy - 10.0),
        (cx + 15.0, cy - 10.0),
        (cx + 15.0, cy + 10.0),
        (cx - 6.0, cy + 10.0),
    ];
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        r.line(a.0, a.1, b.0, b.1, 2.4, color);
    }
    r.line(cx - 2.0, cy - 5.0, cx + 8.0, cy + 5.0, 2.4, color);
    r.line(cx + 8.0, cy - 5.0, cx - 2.0, cy + 5.0, 2.4, color);
}
fn history_date(at: u64) -> String {
    let timestamp = at.min(i64::MAX as u64) as libc::time_t;
    let mut out = std::mem::MaybeUninit::<libc::tm>::uninit();
    let ptr = unsafe { libc::localtime_r(&timestamp, out.as_mut_ptr()) };
    if ptr.is_null() {
        return String::new();
    }
    let time = unsafe { out.assume_init() };
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .get(time.tm_mon as usize)
    .copied()
    .unwrap_or("");
    format!("{} {}", time.tm_mday, month)
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
        assert_eq!(g.stats.played, 0); // No submitted guess: Go does not count a miss.
        assert_eq!(g.stats.streak, 3);
        g.answer = "APPLE".into();
        g.typed = *b"ALLEY";
        g.submit();
        assert_eq!(g.guesses.len(), 1);
        g.new_word();
        assert_eq!(g.stats.played, 1);
        assert_eq!(g.stats.streak, 0);
        assert!(g.stats.history[0].gave_up);
    }
}
