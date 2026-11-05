//! Self-contained Rust games. Logic does no disk I/O and uses fixed-size
//! boards and bounded physics steps, so music keeps its own execution budget.
use crate::input::Action;
use crate::render::{Color, Renderer};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
mod words;
use words::WordGame;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKind {
    TwentyFortyEight,
    Brick,
    Solitaire,
    FiveLetters,
}

pub struct Games {
    game: Game,
}

#[derive(Serialize, Deserialize)]
enum Game {
    Tiles(Tiles),
    Brick(Brick),
    Solitaire(Solitaire),
    Words(WordGame),
}

impl Games {
    pub fn new(kind: GameKind) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            game: match kind {
                GameKind::TwentyFortyEight => Game::Tiles(Tiles::new(seed)),
                GameKind::Brick => Game::Brick(Brick::new()),
                GameKind::Solitaire => Game::Solitaire(Solitaire::new(seed)),
                GameKind::FiveLetters => Game::Words(WordGame::new(seed)),
            },
        }
    }

    /// Immutable bytes for the application's background storage worker.
    /// This method does not open or write any files.
    pub fn serialize_snapshot(&self) -> Result<Vec<u8>, String> {
        serde_json::to_vec(&self.game).map_err(|e| e.to_string())
    }

    pub fn restore_snapshot(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > 512 * 1024 {
            return Err("Game save is too large".into());
        }
        let game: Game = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let valid = match &game {
            Game::Tiles(g) => {
                g.board
                    .iter()
                    .all(|v| *v == 0 || (v.is_power_of_two() && *v <= (1 << 30)))
                    && g.board.iter().any(|v| *v != 0)
                    && g.spawned < 16
                    && g.animation.is_finite()
            }
            Game::Brick(g) => {
                g.blocks.len() == 40
                    && g.lives <= 3
                    && g.level >= 1
                    && g.level <= 1_000_000
                    && [
                        g.paddle,
                        g.paddle_velocity,
                        g.ball[0],
                        g.ball[1],
                        g.velocity[0],
                        g.velocity[1],
                        g.accumulator,
                    ]
                    .iter()
                    .all(|v| v.is_finite() && v.abs() < 1_000_000.0)
                    && g.blocks
                        .iter()
                        .all(|b| b.x.is_finite() && b.y.is_finite() && b.row < 5)
            }
            Game::Solitaire(g) => g.valid(),
            Game::Words(g) => g.valid(),
        };
        let same_kind = matches!(
            (&self.game, &game),
            (Game::Tiles(_), Game::Tiles(_))
                | (Game::Brick(_), Game::Brick(_))
                | (Game::Solitaire(_), Game::Solitaire(_))
                | (Game::Words(_), Game::Words(_))
        );
        if !valid || !same_kind {
            return Err("Invalid game save".into());
        }
        self.game = game;
        Ok(())
    }

    pub fn title(&self) -> &str {
        match self.game {
            Game::Tiles(_) => "2048",
            Game::Brick(_) => "Brick",
            Game::Solitaire(_) => "Solitaire",
            Game::Words(_) => "Five Letters",
        }
    }

    pub fn handle(&mut self, action: Action) {
        if action == Action::SelectHeld {
            if let Game::Solitaire(game) = &mut self.game {
                game.undo();
                return;
            }
        }
        let action = if action == Action::SelectHeld {
            Action::Options
        } else if action == Action::Start {
            Action::PlayPause
        } else {
            action
        };
        match &mut self.game {
            Game::Tiles(g) => g.handle(action),
            Game::Brick(g) => g.handle(action),
            Game::Solitaire(g) => g.handle(action),
            Game::Words(g) => g.handle(action),
        }
    }

    pub fn back_consumed(&mut self) -> bool {
        match &mut self.game {
            Game::Tiles(g) if g.paused => {
                g.paused = false;
                true
            }
            Game::Brick(g) if g.paused => {
                g.paused = false;
                true
            }
            Game::Solitaire(g) if g.menu => {
                g.menu = false;
                true
            }
            Game::Solitaire(g) if g.held.is_some() => {
                g.held = None;
                true
            }
            Game::Words(g) => g.back_consumed(),
            _ => false,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        match &mut self.game {
            Game::Tiles(g) => {
                if !g.paused {
                    g.animation = (g.animation + dt).min(1.0);
                }
            }
            Game::Brick(g) => g.tick(dt),
            Game::Solitaire(g) => g.tick(dt),
            Game::Words(g) => g.tick(dt),
        }
    }

    pub fn draw(&self, renderer: &mut Renderer, dark: bool) {
        match &self.game {
            Game::Tiles(g) => g.draw(renderer, dark),
            Game::Brick(g) => g.draw(renderer, dark),
            Game::Solitaire(g) => g.draw(renderer, dark),
            Game::Words(g) => g.draw(renderer, dark),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        if self.0 == 0 {
            self.0 = 0x9e3779b97f4a7c15;
        }
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[derive(Serialize, Deserialize)]
struct Tiles {
    board: [u32; 16],
    score: u64,
    best: u64,
    won: bool,
    keep_playing: bool,
    over: bool,
    paused: bool,
    spawned: usize,
    merged: [bool; 16],
    animation: f32,
    rng: Rng,
}

impl Tiles {
    fn new(seed: u64) -> Self {
        let mut g = Self {
            board: [0; 16],
            score: 0,
            best: 0,
            won: false,
            keep_playing: false,
            over: false,
            paused: false,
            spawned: 0,
            merged: [false; 16],
            animation: 1.0,
            rng: Rng(seed),
        };
        g.spawn();
        g.spawn();
        g
    }

    fn restart(&mut self) {
        let best = self.best;
        let seed = self.rng.next();
        *self = Self::new(seed);
        self.best = best;
    }

    fn spawn(&mut self) {
        let mut free = [0usize; 16];
        let mut count = 0;
        for (i, value) in self.board.iter().enumerate() {
            if *value == 0 {
                free[count] = i;
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        self.spawned = free[self.rng.next() as usize % count];
        self.board[self.spawned] = if self.rng.next() % 10 == 0 { 4 } else { 2 };
    }

    fn move_board(&mut self, action: Action) -> bool {
        if !matches!(
            action,
            Action::Up | Action::Down | Action::Left | Action::Right
        ) {
            return false;
        }
        let before = self.board;
        self.merged.fill(false);
        for line in 0..4 {
            let mut input = [0; 4];
            for pos in 0..4 {
                input[pos] = self.board[cell(action, line, pos)];
            }
            let (output, score, merged) = merge_line(input);
            self.score = self.score.saturating_add(score);
            for pos in 0..4 {
                let index = cell(action, line, pos);
                self.board[index] = output[pos];
                self.merged[index] = merged[pos];
            }
        }
        if before == self.board {
            return false;
        }
        self.best = self.best.max(self.score);
        self.won |= self.board.iter().any(|v| *v >= 2048);
        self.spawn();
        self.over = !can_move(&self.board);
        self.animation = 0.0;
        true
    }

    fn handle(&mut self, action: Action) {
        match action {
            Action::Options => self.restart(),
            Action::PlayPause => self.paused = !self.paused,
            Action::Select if self.over => self.restart(),
            Action::Select if self.won && !self.keep_playing => self.keep_playing = true,
            Action::Select if self.paused => self.paused = false,
            Action::Up | Action::Down | Action::Left | Action::Right
                if !self.over && !self.paused && (!self.won || self.keep_playing) =>
            {
                self.move_board(action);
            }
            _ => {}
        }
    }

    fn draw(&self, r: &mut Renderer, dark: bool) {
        let bg = if dark {
            color(28, 28, 30)
        } else {
            color(250, 248, 239)
        };
        let ink = if dark {
            color(235, 232, 224)
        } else {
            color(95, 87, 78)
        };
        r.rect(0.0, 40.0, 640.0, 440.0, bg);
        text(r, "2048", 20.0, 92.0, 36, ink, 130.0);
        text(r, "SCORE", 510.0, 88.0, 16, ink, 115.0);
        text(r, &self.score.to_string(), 510.0, 116.0, 26, ink, 115.0);
        text(r, "BEST", 510.0, 158.0, 16, ink, 115.0);
        text(r, &self.best.to_string(), 510.0, 186.0, 26, ink, 115.0);
        text(r, "Join equal", 20.0, 157.0, 16, ink, 125.0);
        text(r, "tiles to", 20.0, 179.0, 16, ink, 125.0);
        text(r, "reach 2048.", 20.0, 201.0, 16, ink, 125.0);
        let x = 152.0;
        let y = 79.0;
        let side = 336.0;
        let gap = 7.0;
        let cell = (side - 5.0 * gap) / 4.0;
        r.rect(
            x,
            y,
            side,
            side,
            if dark {
                color(63, 60, 56)
            } else {
                color(187, 173, 160)
            },
        );
        for i in 0..16 {
            let xx = x + gap + (i % 4) as f32 * (cell + gap);
            let yy = y + gap + (i / 4) as f32 * (cell + gap);
            r.rect(
                xx,
                yy,
                cell,
                cell,
                if dark {
                    color(81, 77, 70)
                } else {
                    color(205, 193, 180)
                },
            );
            let value = self.board[i];
            if value == 0 {
                continue;
            }
            let t = (self.animation / 0.15).min(1.0);
            let scale = if i == self.spawned {
                0.75 + 0.25 * (1.0 - (1.0 - t).powi(3))
            } else if self.merged[i] {
                1.0 + 0.08 * (std::f32::consts::PI * t).sin()
            } else {
                1.0
            };
            let offset = cell * (1.0 - scale) * 0.5;
            r.rect(
                xx + offset,
                yy + offset,
                cell * scale,
                cell * scale,
                tile_color(value),
            );
            let size = if value < 100 {
                32
            } else if value < 1000 {
                28
            } else {
                22
            };
            centered(
                r,
                &value.to_string(),
                xx,
                yy + cell * 0.65,
                cell,
                size,
                if value <= 4 {
                    color(95, 87, 78)
                } else {
                    color(255, 255, 255)
                },
            );
        }
        centered(
            r,
            "Arrows: slide    A: continue    Y: new    START: pause",
            0.0,
            452.0,
            640.0,
            16,
            ink,
        );
        if self.paused {
            overlay(r, "Paused", "A / START: resume", 165.0, 184.0, 310.0, 116.0);
        } else if self.over {
            overlay(r, "Game over", "A: play again", 165.0, 184.0, 310.0, 116.0);
        } else if self.won && !self.keep_playing {
            overlay(
                r,
                "You reached 2048!",
                "A: keep going",
                165.0,
                184.0,
                310.0,
                116.0,
            );
        }
    }
}

fn cell(direction: Action, line: usize, pos: usize) -> usize {
    match direction {
        Action::Left => line * 4 + pos,
        Action::Right => line * 4 + 3 - pos,
        Action::Up => pos * 4 + line,
        Action::Down => (3 - pos) * 4 + line,
        _ => unreachable!(),
    }
}

fn merge_line(input: [u32; 4]) -> ([u32; 4], u64, [bool; 4]) {
    let mut packed = [0; 4];
    let mut count = 0;
    for value in input {
        if value != 0 {
            packed[count] = value;
            count += 1;
        }
    }
    let mut output = [0; 4];
    let mut merged = [false; 4];
    let mut score = 0;
    let mut read = 0;
    let mut write = 0;
    while read < count {
        let value = packed[read];
        if read + 1 < count && packed[read + 1] == value {
            // Values are powers of two. Saturation handles theoretical boards
            // beyond u32 without overflowing or wrapping into empty cells.
            output[write] = value.saturating_mul(2);
            score += u64::from(output[write]);
            merged[write] = true;
            read += 2;
        } else {
            output[write] = value;
            read += 1;
        }
        write += 1;
    }
    (output, score, merged)
}

fn can_move(board: &[u32; 16]) -> bool {
    for i in 0..16 {
        if board[i] == 0
            || (i % 4 != 3 && board[i] == board[i + 1])
            || (i < 12 && board[i] == board[i + 4])
        {
            return true;
        }
    }
    false
}

fn tile_color(value: u32) -> Color {
    match value {
        2 => color(238, 228, 218),
        4 => color(237, 224, 200),
        8 => color(242, 177, 121),
        16 => color(245, 149, 99),
        32 => color(246, 124, 95),
        64 => color(246, 94, 59),
        128 => color(237, 207, 114),
        256 => color(237, 204, 97),
        512 => color(237, 200, 80),
        1024 => color(237, 197, 63),
        2048 => color(237, 194, 46),
        _ => color(62, 57, 51),
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
struct Block {
    x: f32,
    y: f32,
    active: bool,
    row: usize,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    Waiting,
    Playing,
    Over,
}
#[derive(Serialize, Deserialize)]
struct Brick {
    blocks: Vec<Block>,
    paddle: f32,
    paddle_velocity: f32,
    ball: [f32; 2],
    velocity: [f32; 2],
    phase: Phase,
    paused: bool,
    lives: u8,
    level: u32,
    score: u32,
    accumulator: f32,
}
const PADDLE_WIDTH: f32 = 108.0;
const PADDLE_Y: f32 = 420.0;
const BALL_RADIUS: f32 = 6.0;
const BLOCK_WIDTH: f32 = 58.0;
const BLOCK_HEIGHT: f32 = 23.0;
impl Brick {
    fn new() -> Self {
        let mut g = Self {
            blocks: vec![
                Block {
                    x: 0.0,
                    y: 0.0,
                    active: true,
                    row: 0,
                };
                40
            ],
            paddle: 266.0,
            paddle_velocity: 0.0,
            ball: [320.0, 410.0],
            velocity: [0.0; 2],
            phase: Phase::Waiting,
            paused: false,
            lives: 3,
            level: 1,
            score: 0,
            accumulator: 0.0,
        };
        g.setup();
        g.reset_ball();
        g
    }
    fn setup(&mut self) {
        for row in 0..5 {
            for col in 0..8 {
                self.blocks[row * 8 + col] = Block {
                    x: 76.0 + col as f32 * (BLOCK_WIDTH + 3.0),
                    y: 100.0 + row as f32 * (BLOCK_HEIGHT + 4.0),
                    active: true,
                    row,
                };
            }
        }
    }
    fn speed(&self) -> f32 {
        (210.0 + 22.0 * self.level.saturating_sub(1) as f32).min(500.0)
    }
    fn reset_ball(&mut self) {
        self.ball = [
            self.paddle + PADDLE_WIDTH * 0.5,
            PADDLE_Y - BALL_RADIUS - 2.0,
        ];
        self.velocity = [self.speed() * 0.6, -self.speed() * 0.8];
    }
    fn handle(&mut self, action: Action) {
        match action {
            Action::Options => *self = Self::new(),
            Action::PlayPause if self.phase != Phase::Over => self.paused = !self.paused,
            Action::Select if self.phase == Phase::Over => *self = Self::new(),
            Action::Select if self.paused => self.paused = false,
            Action::Select if self.phase == Phase::Waiting => self.phase = Phase::Playing,
            Action::Left if !self.paused => self.paddle_velocity = -460.0,
            Action::Right if !self.paused => self.paddle_velocity = 460.0,
            _ => {}
        }
    }
    fn tick(&mut self, dt: f32) {
        if self.paused || self.phase == Phase::Over {
            return;
        }
        self.accumulator += dt;
        // Fixed 240 Hz collision steps prevent tunnelling at a 60 Hz render
        // cadence. Clamp elapsed work; a late UI frame cannot cause a burst.
        let step = 1.0 / 240.0;
        let mut n = 0;
        while self.accumulator >= step && n < 24 {
            self.accumulator -= step;
            self.step(step);
            n += 1;
        }
        if n == 24 {
            self.accumulator = self.accumulator.min(step);
        }
    }
    fn step(&mut self, dt: f32) {
        if self.phase == Phase::Over {
            return;
        }
        self.paddle = (self.paddle + self.paddle_velocity * dt).clamp(12.0, 628.0 - PADDLE_WIDTH);
        self.paddle_velocity *= (-8.0 * dt).exp();
        if self.paddle_velocity.abs() < 0.3 {
            self.paddle_velocity = 0.0;
        }
        if self.phase == Phase::Waiting {
            self.reset_ball();
            return;
        }
        let before = self.ball;
        self.ball[0] += self.velocity[0] * dt;
        self.ball[1] += self.velocity[1] * dt;
        if self.ball[0] < 12.0 + BALL_RADIUS {
            self.ball[0] = 12.0 + BALL_RADIUS;
            self.velocity[0] = self.velocity[0].abs();
        } else if self.ball[0] > 628.0 - BALL_RADIUS {
            self.ball[0] = 628.0 - BALL_RADIUS;
            self.velocity[0] = -self.velocity[0].abs();
        }
        if self.ball[1] < 82.0 + BALL_RADIUS {
            self.ball[1] = 82.0 + BALL_RADIUS;
            self.velocity[1] = self.velocity[1].abs();
        }
        if self.ball[1] > 480.0 + BALL_RADIUS {
            self.lives = self.lives.saturating_sub(1);
            self.phase = if self.lives == 0 {
                Phase::Over
            } else {
                Phase::Waiting
            };
            self.reset_ball();
            return;
        }
        if self.velocity[1] > 0.0
            && circle_rect(self.ball, self.paddle, PADDLE_Y, PADDLE_WIDTH, 10.0)
            && before[1] <= PADDLE_Y + BALL_RADIUS
        {
            let hit = ((self.ball[0] - self.paddle) / PADDLE_WIDTH).clamp(0.0, 1.0);
            let angle = std::f32::consts::PI * (0.15 + 0.7 * hit);
            self.velocity = [-self.speed() * angle.cos(), -self.speed() * angle.sin()];
            self.ball[1] = PADDLE_Y - BALL_RADIUS;
        }
        for block in &mut self.blocks {
            if !block.active || !circle_rect(self.ball, block.x, block.y, BLOCK_WIDTH, BLOCK_HEIGHT)
            {
                continue;
            }
            block.active = false;
            self.score += [7, 5, 3, 1, 1][block.row];
            if before[1] + BALL_RADIUS <= block.y
                || before[1] - BALL_RADIUS >= block.y + BLOCK_HEIGHT
            {
                self.velocity[1] = -self.velocity[1];
            } else {
                self.velocity[0] = -self.velocity[0];
            }
            break;
        }
        if self.blocks.iter().all(|b| !b.active) {
            self.level = self.level.saturating_add(1);
            self.setup();
            self.phase = Phase::Waiting;
            self.reset_ball();
        }
    }
    fn draw(&self, r: &mut Renderer, dark: bool) {
        let bg = if dark {
            color(15, 20, 29)
        } else {
            color(232, 238, 246)
        };
        let ink = if dark {
            color(226, 234, 247)
        } else {
            color(41, 54, 73)
        };
        r.rect(0.0, 40.0, 640.0, 440.0, bg);
        text(
            r,
            &format!("Score {}", self.score),
            18.0,
            70.0,
            18,
            ink,
            240.0,
        );
        centered(
            r,
            &format!("Level {}", self.level),
            230.0,
            70.0,
            180.0,
            18,
            ink,
        );
        text(
            r,
            &format!("Lives {}", self.lives),
            530.0,
            70.0,
            18,
            ink,
            105.0,
        );
        r.rect(10.0, 80.0, 2.0, 359.0, color(95, 114, 139));
        r.rect(628.0, 80.0, 2.0, 359.0, color(95, 114, 139));
        r.rect(10.0, 80.0, 620.0, 2.0, color(95, 114, 139));
        let colors = [
            color(239, 61, 74),
            color(246, 141, 58),
            color(226, 190, 46),
            color(47, 183, 114),
            color(54, 125, 221),
        ];
        for b in &self.blocks {
            if b.active {
                let c = colors[b.row];
                let light = [
                    (c[0] + 0.24).min(1.0),
                    (c[1] + 0.24).min(1.0),
                    (c[2] + 0.24).min(1.0),
                    1.0,
                ];
                r.gradient(b.x, b.y, BLOCK_WIDTH, BLOCK_HEIGHT, light, c);
            }
        }
        r.gradient(
            self.paddle,
            PADDLE_Y,
            PADDLE_WIDTH,
            10.0,
            color(225, 231, 238),
            color(91, 113, 139),
        );
        // An eight-sided raster-free ball; all geometry is batched by the GPU.
        for n in 0..7 {
            let y = n as f32 * 2.0 - 6.0;
            let width = (BALL_RADIUS * BALL_RADIUS - y * y).max(0.0).sqrt();
            r.rect(
                self.ball[0] - width,
                self.ball[1] + y,
                width * 2.0,
                2.0,
                ink,
            );
        }
        centered(
            r,
            "Left / Right: paddle    A: serve    Y: new    START: pause",
            0.0,
            468.0,
            640.0,
            16,
            ink,
        );
        if self.paused {
            overlay(r, "Paused", "A / START: resume", 155.0, 248.0, 330.0, 110.0);
        } else if self.phase == Phase::Waiting {
            overlay(r, "Ready?", "A: serve", 215.0, 275.0, 210.0, 100.0);
        } else if self.phase == Phase::Over {
            overlay(r, "Game over", "A: play again", 155.0, 248.0, 330.0, 110.0);
        }
    }
}

fn circle_rect(ball: [f32; 2], x: f32, y: f32, w: f32, h: f32) -> bool {
    let dx = ball[0] - ball[0].clamp(x, x + w);
    let dy = ball[1] - ball[1].clamp(y, y + h);
    dx * dx + dy * dy <= BALL_RADIUS * BALL_RADIUS
}
fn color(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
fn overlay(r: &mut Renderer, title: &str, hint: &str, x: f32, y: f32, w: f32, h: f32) {
    r.rect(x - 3.0, y - 3.0, w + 6.0, h + 6.0, [0.0, 0.0, 0.0, 0.22]);
    r.rect(x, y, w, h, [0.13, 0.15, 0.19, 0.96]);
    centered(r, title, x + 8.0, y + 42.0, w - 16.0, 26, [1.0; 4]);
    centered(
        r,
        hint,
        x + 8.0,
        y + 77.0,
        w - 16.0,
        18,
        [0.82, 0.86, 0.94, 1.0],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_tile_merges_only_once_per_move() {
        let (line, score, _) = merge_line([2, 2, 2, 2]);
        assert_eq!(line, [4, 4, 0, 0]);
        assert_eq!(score, 8);
        let (line, score, _) = merge_line([4, 0, 4, 8]);
        assert_eq!(line, [8, 8, 0, 0]);
        assert_eq!(score, 8);
    }
    #[test]
    fn moving_into_a_wall_does_not_spawn_an_extra_tile() {
        let mut game = Tiles::new(7);
        game.board = [0; 16];
        game.board[0] = 2;
        assert!(!game.move_board(Action::Left));
        assert_eq!(game.board.iter().filter(|v| **v != 0).count(), 1);
    }
    #[test]
    fn down_moves_from_the_bottom_and_preserves_score() {
        let mut game = Tiles::new(9);
        game.board = [0; 16];
        game.board[0] = 2;
        game.board[4] = 2;
        assert!(game.move_board(Action::Down));
        assert_eq!(game.board[12], 4);
        assert_eq!(game.score, 4);
        assert_eq!(game.board.iter().filter(|v| **v != 0).count(), 2);
    }
    #[test]
    fn full_board_with_a_merge_is_not_game_over() {
        let mut board = [2, 4, 2, 4, 4, 2, 4, 2, 2, 4, 2, 4, 4, 2, 4, 2];
        assert!(!can_move(&board));
        board[1] = 2;
        assert!(can_move(&board));
    }
    #[test]
    fn brick_serve_pause_and_restart_are_complete_transitions() {
        let mut g = Brick::new();
        assert!(g.phase == Phase::Waiting);
        g.handle(Action::Select);
        assert!(g.phase == Phase::Playing);
        g.handle(Action::PlayPause);
        let before = g.ball;
        g.tick(0.1);
        assert_eq!(g.ball, before);
        g.handle(Action::Options);
        assert!(g.phase == Phase::Waiting);
        assert_eq!(g.lives, 3);
        assert_eq!(g.score, 0);
    }
    #[test]
    fn brick_paddle_collision_returns_the_ball_upward() {
        let mut g = Brick::new();
        g.phase = Phase::Playing;
        g.ball = [g.paddle + PADDLE_WIDTH * 0.5, PADDLE_Y - BALL_RADIUS - 0.1];
        g.velocity = [0.0, 200.0];
        g.step(1.0 / 240.0);
        assert!(g.velocity[1] < 0.0);
        assert!(g.ball[1] <= PADDLE_Y - BALL_RADIUS);
    }
    #[test]
    fn brick_does_not_tunnel_through_a_block_at_top_speed() {
        let mut g = Brick::new();
        g.phase = Phase::Playing;
        g.level = 20;
        for b in &mut g.blocks {
            b.active = false;
        }
        g.blocks[0].active = true;
        g.blocks[1].active = true;
        let b = g.blocks[0];
        g.ball = [
            b.x + BLOCK_WIDTH * 0.5,
            b.y + BLOCK_HEIGHT + BALL_RADIUS + 1.0,
        ];
        g.velocity = [0.0, -500.0];
        g.tick(1.0 / 60.0);
        assert!(!g.blocks[0].active);
        assert!(g.velocity[1] > 0.0);
    }
    #[test]
    fn brick_losing_last_ball_ends_game_without_underflow() {
        let mut g = Brick::new();
        g.phase = Phase::Playing;
        g.lives = 1;
        g.ball[1] = 490.0;
        g.velocity[1] = 100.0;
        g.tick(0.02);
        assert_eq!(g.lives, 0);
        assert!(g.phase == Phase::Over);
        g.tick(0.1);
        assert_eq!(g.lives, 0);
    }
}

// Klondike. The UI uses explicit source/destination selection so every
// legal move is possible with only the console's d-pad and face buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Card {
    rank: u8,
    suit: u8,
    face: bool,
}
impl Card {
    fn red(self) -> bool {
        self.suit < 2
    }
    fn label(self) -> &'static str {
        match self.rank {
            1 => "A",
            2 => "2",
            3 => "3",
            4 => "4",
            5 => "5",
            6 => "6",
            7 => "7",
            8 => "8",
            9 => "9",
            10 => "10",
            11 => "J",
            12 => "Q",
            _ => "K",
        }
    }
    fn suit_label(self) -> &'static str {
        ["H", "D", "C", "S"][self.suit as usize]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Pile {
    Stock,
    Waste,
    Foundation(usize),
    Tableau(usize),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Selection {
    pile: Pile,
    index: usize,
}
#[derive(Clone, Serialize, Deserialize)]
struct SolitaireState {
    stock: Vec<Card>,
    waste: Vec<Card>,
    foundations: [Vec<Card>; 4],
    tableau: [Vec<Card>; 7],
    moves: u32,
}
#[derive(Serialize, Deserialize)]
struct Solitaire {
    state: SolitaireState,
    history: Vec<SolitaireState>,
    cursor: Selection,
    held: Option<Selection>,
    draw_count: usize,
    rng: Rng,
    menu: bool,
    menu_index: usize,
    elapsed: f32,
    status: String,
    status_time: f32,
}
impl Solitaire {
    fn new(seed: u64) -> Self {
        let mut g = Self {
            state: SolitaireState {
                stock: Vec::with_capacity(24),
                waste: Vec::with_capacity(24),
                foundations: std::array::from_fn(|_| Vec::with_capacity(13)),
                tableau: std::array::from_fn(|_| Vec::with_capacity(20)),
                moves: 0,
            },
            history: Vec::with_capacity(100),
            cursor: Selection {
                pile: Pile::Stock,
                index: 0,
            },
            held: None,
            draw_count: 1,
            rng: Rng(seed),
            menu: false,
            menu_index: 0,
            elapsed: 0.0,
            status: String::new(),
            status_time: 0.0,
        };
        let mut deck: Vec<Card> = (0..4)
            .flat_map(|suit| {
                (1..=13).map(move |rank| Card {
                    rank,
                    suit,
                    face: false,
                })
            })
            .collect();
        for i in (1..deck.len()).rev() {
            let j = g.rng.next() as usize % (i + 1);
            deck.swap(i, j);
        }
        for column in 0..7 {
            for row in 0..=column {
                let mut card = deck.pop().unwrap();
                card.face = row == column;
                g.state.tableau[column].push(card);
            }
        }
        g.state.stock = deck;
        g
    }
    fn valid(&self) -> bool {
        if !matches!(self.draw_count, 1 | 3)
            || self.menu_index > 4
            || !self.elapsed.is_finite()
            || !self.status_time.is_finite()
            || self.history.len() > 100
        {
            return false;
        }
        if !self.state.valid() || !self.history.iter().all(SolitaireState::valid) {
            return false;
        }
        self.selection_valid(self.cursor) && self.held.map_or(true, |s| self.selection_valid(s))
    }
    fn selection_valid(&self, s: Selection) -> bool {
        match s.pile {
            Pile::Stock => s.index == 0,
            Pile::Waste => s.index == 0,
            Pile::Foundation(i) => i < 4 && s.index == 0,
            Pile::Tableau(i) => i < 7 && s.index < self.state.tableau[i].len().max(1),
        }
    }
    fn won(&self) -> bool {
        self.state.foundations.iter().all(|p| p.len() == 13)
    }
    fn remember(&mut self) {
        if self.history.len() == 100 {
            self.history.remove(0);
        }
        self.history.push(self.state.clone());
    }
    fn tell(&mut self, status: &str) {
        self.status.clear();
        self.status.push_str(status);
        self.status_time = 2.0;
    }
    fn top_cursor(&mut self, pile: Pile) {
        let index = match pile {
            Pile::Tableau(i) => self.state.tableau[i].len().saturating_sub(1),
            _ => 0,
        };
        self.cursor = Selection { pile, index };
    }
    fn draw_stock(&mut self) {
        self.held = None;
        if self.state.stock.is_empty() && self.state.waste.is_empty() {
            self.tell("No cards left in the stock");
            return;
        }
        self.remember();
        if self.state.stock.is_empty() {
            while let Some(mut card) = self.state.waste.pop() {
                card.face = false;
                self.state.stock.push(card);
            }
        } else {
            for _ in 0..self.draw_count {
                if let Some(mut card) = self.state.stock.pop() {
                    card.face = true;
                    self.state.waste.push(card);
                }
            }
        }
        self.state.moves = self.state.moves.saturating_add(1);
    }
    fn selected_cards(&self, selection: Selection) -> Option<&[Card]> {
        let (pile, index) = match selection.pile {
            Pile::Stock => return None,
            Pile::Waste => (&self.state.waste, self.state.waste.len().checked_sub(1)?),
            Pile::Foundation(i) => {
                let pile = self.state.foundations.get(i)?;
                (pile, pile.len().checked_sub(1)?)
            }
            Pile::Tableau(i) => (self.state.tableau.get(i)?, selection.index),
        };
        let cards = pile.get(index..)?;
        if cards.is_empty() || !cards.iter().all(|c| c.face) {
            return None;
        }
        if !cards
            .windows(2)
            .all(|p| p[0].rank == p[1].rank + 1 && p[0].red() != p[1].red())
        {
            return None;
        }
        Some(cards)
    }
    fn accepts(&self, source: Selection, target: Pile) -> bool {
        if source.pile == target {
            return false;
        }
        let Some(cards) = self.selected_cards(source) else {
            return false;
        };
        let card = cards[0];
        match target {
            Pile::Foundation(i) if i < 4 && cards.len() == 1 => {
                let pile = &self.state.foundations[i];
                pile.last().map_or(card.rank == 1, |top| {
                    top.suit == card.suit && top.rank + 1 == card.rank
                })
            }
            Pile::Tableau(i) if i < 7 => {
                self.state.tableau[i].last().map_or(card.rank == 13, |top| {
                    top.face && top.red() != card.red() && top.rank == card.rank + 1
                })
            }
            _ => false,
        }
    }
    fn transfer(&mut self, source: Selection, target: Pile) -> bool {
        if !self.accepts(source, target) {
            return false;
        }
        self.remember();
        let moved = match source.pile {
            Pile::Waste => vec![self.state.waste.pop().unwrap()],
            Pile::Foundation(i) => vec![self.state.foundations[i].pop().unwrap()],
            Pile::Tableau(i) => self.state.tableau[i].split_off(source.index),
            Pile::Stock => unreachable!(),
        };
        match target {
            Pile::Tableau(i) => self.state.tableau[i].extend(moved),
            Pile::Foundation(i) => self.state.foundations[i].extend(moved),
            _ => unreachable!(),
        }
        if let Pile::Tableau(i) = source.pile {
            if let Some(card) = self.state.tableau[i].last_mut() {
                card.face = true;
            }
        }
        self.state.moves = self.state.moves.saturating_add(1);
        self.held = None;
        self.top_cursor(target);
        true
    }
    fn undo(&mut self) {
        if let Some(state) = self.history.pop() {
            self.state = state;
            self.held = None;
            self.top_cursor(Pile::Stock);
            self.tell("Move undone");
        } else {
            self.tell("No move to undo");
        }
    }
    fn auto_foundation(&mut self) {
        let source = self.held.unwrap_or(self.cursor);
        for i in 0..4 {
            if self.transfer(source, Pile::Foundation(i)) {
                return;
            }
        }
        self.tell("That card cannot go to a foundation");
    }
    fn handle(&mut self, action: Action) {
        if self.menu {
            match action {
                Action::Up => self.menu_index = (self.menu_index + 4) % 5,
                Action::Down => self.menu_index = (self.menu_index + 1) % 5,
                Action::Back | Action::Options | Action::PlayPause => self.menu = false,
                Action::Select => match self.menu_index {
                    0 => self.menu = false,
                    1 => {
                        let seed = self.rng.next();
                        let draw_count = self.draw_count;
                        *self = Self::new(seed);
                        self.draw_count = draw_count;
                    }
                    2 => {
                        self.draw_count = if self.draw_count == 1 { 3 } else { 1 };
                        self.menu = false;
                    }
                    3 => {
                        self.undo();
                        self.menu = false;
                    }
                    _ => {
                        self.auto_foundation();
                        self.menu = false;
                    }
                },
                _ => {}
            }
            return;
        }
        match action {
            Action::Options if self.held.is_some() => {
                self.held = None;
                self.tell("Selection cancelled");
            }
            Action::Options | Action::PlayPause => {
                self.menu = true;
                self.menu_index = 0;
            }
            Action::Previous => self.undo(),
            Action::Next => self.auto_foundation(),
            Action::Select if self.won() => {
                let seed = self.rng.next();
                *self = Self::new(seed);
            }
            Action::Select if self.cursor.pile == Pile::Stock => self.draw_stock(),
            Action::Select => {
                if let Some(source) = self.held {
                    if source.pile == self.cursor.pile {
                        self.held = None;
                    } else if !self.transfer(source, self.cursor.pile) {
                        self.tell("Choose an alternating lower card, or a matching foundation");
                    }
                } else if self.selected_cards(self.cursor).is_some() {
                    self.held = Some(self.cursor);
                }
            }
            Action::Left | Action::Right => {
                let delta = if action == Action::Right { 1 } else { -1 };
                let pile = match self.cursor.pile {
                    Pile::Tableau(i) => Pile::Tableau((i as i32 + delta).rem_euclid(7) as usize),
                    p => {
                        let i = match p {
                            Pile::Stock => 0,
                            Pile::Waste => 1,
                            Pile::Foundation(i) => i + 2,
                            _ => unreachable!(),
                        };
                        match (i as i32 + delta).rem_euclid(6) as usize {
                            0 => Pile::Stock,
                            1 => Pile::Waste,
                            n => Pile::Foundation(n - 2),
                        }
                    }
                };
                self.top_cursor(pile);
            }
            Action::Up => {
                if let Pile::Tableau(i) = self.cursor.pile {
                    let first = self.state.tableau[i]
                        .iter()
                        .position(|c| c.face)
                        .unwrap_or(0);
                    if self.cursor.index > first {
                        self.cursor.index -= 1;
                    } else {
                        self.top_cursor(match i {
                            0 => Pile::Stock,
                            1 | 2 => Pile::Waste,
                            _ => Pile::Foundation((i - 3).min(3)),
                        });
                    }
                }
            }
            Action::Down => match self.cursor.pile {
                Pile::Tableau(i) => {
                    self.cursor.index =
                        (self.cursor.index + 1).min(self.state.tableau[i].len().saturating_sub(1));
                }
                p => {
                    let i = match p {
                        Pile::Stock => 0,
                        Pile::Waste => 1,
                        Pile::Foundation(i) => i + 3,
                        _ => unreachable!(),
                    };
                    self.top_cursor(Pile::Tableau(i));
                }
            },
            Action::Back => self.held = None,
            _ => {}
        }
    }
    fn tick(&mut self, dt: f32) {
        if !self.menu && !self.won() {
            self.elapsed += dt;
        }
        self.status_time = (self.status_time - dt).max(0.0);
    }
    fn draw(&self, r: &mut Renderer, dark: bool) {
        let green = if dark {
            color(12, 61, 48)
        } else {
            color(22, 112, 78)
        };
        r.rect(0.0, 40.0, 640.0, 440.0, green);
        text(
            r,
            &format!("Moves {}", self.state.moves),
            14.0,
            64.0,
            16,
            [1.0; 4],
            200.0,
        );
        let seconds = self.elapsed as u32;
        text(
            r,
            &format!("{}:{:02}", seconds / 60, seconds % 60),
            557.0,
            64.0,
            16,
            [1.0; 4],
            80.0,
        );
        let top_y = 76.0;
        for (pile, x, label) in [
            (Pile::Stock, 14.0, "Stock"),
            (Pile::Waste, 102.0, "Waste"),
            (Pile::Foundation(0), 278.0, "A"),
            (Pile::Foundation(1), 366.0, "A"),
            (Pile::Foundation(2), 454.0, "A"),
            (Pile::Foundation(3), 542.0, "A"),
        ] {
            let card = match pile {
                Pile::Stock => self.state.stock.last(),
                Pile::Waste => self.state.waste.last(),
                Pile::Foundation(i) => self.state.foundations[i].last(),
                _ => None,
            };
            draw_card(
                r,
                card.copied(),
                x,
                top_y,
                label,
                self.cursor.pile == pile,
                self.held.map_or(false, |s| s.pile == pile),
            );
        }
        for (column, cards) in self.state.tableau.iter().enumerate() {
            let x = 14.0 + column as f32 * 88.0;
            if cards.is_empty() {
                draw_card(
                    r,
                    None,
                    x,
                    196.0,
                    "K",
                    self.cursor.pile == Pile::Tableau(column),
                    false,
                );
                continue;
            }
            let first_face = cards.iter().position(|c| c.face).unwrap_or(cards.len());
            let natural_height = first_face as f32 * 13.0
                + cards.len().saturating_sub(first_face + 1) as f32 * 22.0
                + 94.0;
            let squeeze = if natural_height > 226.0 {
                (226.0 - 94.0) / (natural_height - 94.0)
            } else {
                1.0
            };
            let mut y = 196.0;
            for (index, card) in cards.iter().enumerate() {
                let selected = self.cursor
                    == Selection {
                        pile: Pile::Tableau(column),
                        index,
                    };
                let held = self.held.map_or(false, |s| {
                    s.pile == Pile::Tableau(column) && index >= s.index
                });
                draw_card(r, Some(*card), x, y, "", selected, held);
                y += if card.face { 22.0 } else { 13.0 } * squeeze;
            }
        }
        if self.status_time > 0.0 {
            centered(r, &self.status, 4.0, 445.0, 632.0, 16, [1.0; 4]);
        } else {
            centered(
                r,
                "A: pick / place / draw    Y: menu / cancel    L: undo    R: to foundation",
                4.0,
                445.0,
                632.0,
                16,
                [1.0; 4],
            );
        }
        if self.won() {
            overlay(r, "You won!", "A: deal again", 150.0, 218.0, 340.0, 110.0);
        }
        if self.menu {
            r.rect(143.0, 130.0, 354.0, 260.0, [0.08, 0.12, 0.15, 0.98]);
            let draw = if self.draw_count == 1 {
                "Draw three cards"
            } else {
                "Draw one card"
            };
            for (i, label) in ["Continue", "New game", draw, "Undo", "Move to foundation"]
                .iter()
                .enumerate()
            {
                let y = 144.0 + i as f32 * 47.0;
                if i == self.menu_index {
                    r.rect(151.0, y, 338.0, 43.0, color(38, 120, 220));
                }
                text(r, label, 163.0, y + 29.0, 20, [1.0; 4], 312.0);
            }
        }
    }
}
impl SolitaireState {
    fn valid(&self) -> bool {
        let mut seen = [false; 52];
        let mut count = 0;
        for card in self
            .stock
            .iter()
            .chain(self.waste.iter())
            .chain(self.foundations.iter().flatten())
            .chain(self.tableau.iter().flatten())
        {
            if card.rank == 0 || card.rank > 13 || card.suit >= 4 {
                return false;
            }
            let i = card.suit as usize * 13 + card.rank as usize - 1;
            if seen[i] {
                return false;
            }
            seen[i] = true;
            count += 1;
        }
        count == 52
            && self.stock.iter().all(|c| !c.face)
            && self.waste.iter().all(|c| c.face)
            && self.foundations.iter().all(|p| {
                p.iter()
                    .enumerate()
                    .all(|(i, c)| c.face && c.rank as usize == i + 1 && c.suit == p[0].suit)
            })
            && self.tableau.iter().all(|p| {
                let first = p.iter().position(|c| c.face).unwrap_or(p.len());
                (p.is_empty() || first < p.len())
                    && p[first..].iter().all(|c| c.face)
                    && p[first..].windows(2).all(|pair| {
                        pair[0].rank == pair[1].rank + 1 && pair[0].red() != pair[1].red()
                    })
            })
    }
}
fn draw_card(
    r: &mut Renderer,
    card: Option<Card>,
    x: f32,
    y: f32,
    empty: &str,
    selected: bool,
    held: bool,
) {
    let border = if held {
        color(255, 205, 55)
    } else if selected {
        color(92, 192, 255)
    } else {
        color(5, 60, 45)
    };
    r.rect(x - 3.0, y - 3.0, 76.0, 100.0, border);
    let Some(card) = card else {
        r.rect(x, y, 70.0, 94.0, [0.02, 0.20, 0.13, 0.65]);
        centered(r, empty, x, y + 55.0, 70.0, 22, [0.55, 0.8, 0.65, 1.0]);
        return;
    };
    if !card.face {
        r.gradient(x, y, 70.0, 94.0, color(57, 129, 205), color(18, 66, 126));
        for n in 0..6 {
            r.rect(
                x + 8.0,
                y + 9.0 + n as f32 * 14.0,
                54.0,
                2.0,
                [0.7, 0.85, 1.0, 0.25],
            );
        }
        return;
    }
    r.rect(x, y, 70.0, 94.0, color(249, 248, 242));
    let ink = if card.red() {
        color(192, 35, 47)
    } else {
        color(29, 34, 43)
    };
    text(r, card.label(), x + 5.0, y + 19.0, 18, ink, 60.0);
    centered(r, card.suit_label(), x, y + 62.0, 70.0, 28, ink);
    let width = r.text_width(card.label(), 18);
    text(r, card.label(), x + 65.0 - width, y + 88.0, 18, ink, 60.0);
}

#[cfg(test)]
mod solitaire_tests {
    use super::*;
    fn card(rank: u8, suit: u8, face: bool) -> Card {
        Card { rank, suit, face }
    }

    #[test]
    fn klondike_deals_each_card_once_with_seven_legal_columns() {
        let g = Solitaire::new(42);
        assert!(g.valid());
        assert_eq!(g.state.stock.len(), 24);
        for i in 0..7 {
            assert_eq!(g.state.tableau[i].len(), i + 1);
            assert!(g.state.tableau[i][i].face);
            assert!(g.state.tableau[i][..i].iter().all(|c| !c.face));
        }
    }

    #[test]
    fn draw_three_and_undo_preserve_the_stock_order() {
        let mut g = Solitaire::new(42);
        g.draw_count = 3;
        let before = g.state.stock.clone();
        g.draw_stock();
        assert_eq!(g.state.stock.len(), 21);
        assert_eq!(g.state.waste.len(), 3);
        assert_eq!(g.state.waste[2].rank, before[21].rank);
        assert!(g.state.waste.iter().all(|c| c.face));
        assert!(g.state.valid());
        g.undo();
        assert_eq!(g.state.stock, before);
        assert!(g.state.waste.is_empty());
    }

    #[test]
    fn empty_stock_recycles_the_waste_without_reversing_the_draw_order() {
        let mut g = Solitaire::new(9);
        let before = g.state.stock.clone();
        for _ in 0..24 {
            g.draw_stock();
        }
        assert!(g.state.stock.is_empty());
        assert_eq!(g.state.waste.len(), 24);
        g.draw_stock();
        assert_eq!(g.state.stock, before);
        assert!(g.state.waste.is_empty());
        assert!(g.state.valid());
    }

    #[test]
    fn tableau_requires_alternating_colours_and_only_kings_fill_empty_columns() {
        let mut g = Solitaire::new(5);
        g.state.tableau[0] = vec![card(8, 2, true)];
        g.state.tableau[1] = vec![card(7, 0, true), card(6, 2, true)];
        g.state.tableau[2].clear();
        let from = Selection {
            pile: Pile::Tableau(1),
            index: 0,
        };
        assert!(g.accepts(from, Pile::Tableau(0)));
        assert!(!g.accepts(from, Pile::Tableau(2)));
        g.state.tableau[0][0].suit = 0;
        assert!(!g.accepts(from, Pile::Tableau(0)));
        g.state.tableau[1] = vec![card(13, 3, true), card(12, 1, true)];
        assert!(g.accepts(from, Pile::Tableau(2)));
    }

    #[test]
    fn foundation_accepts_only_single_matching_cards_in_order() {
        let mut g = Solitaire::new(3);
        g.state.foundations[0].clear();
        g.state.waste = vec![card(1, 2, true)];
        let from = Selection {
            pile: Pile::Waste,
            index: 0,
        };
        assert!(g.transfer(from, Pile::Foundation(0)));
        g.state.waste.push(card(2, 1, true));
        assert!(!g.accepts(from, Pile::Foundation(0)));
        g.state.waste[0].suit = 2;
        assert!(g.transfer(from, Pile::Foundation(0)));
        assert_eq!(g.state.foundations[0].len(), 2);
        g.state.tableau[0] = vec![card(3, 2, true), card(2, 0, true)];
        assert!(!g.accepts(
            Selection {
                pile: Pile::Tableau(0),
                index: 0
            },
            Pile::Foundation(0)
        ));
    }

    #[test]
    fn moving_a_stack_reveals_a_hidden_card_and_undo_restores_it() {
        let mut g = Solitaire::new(7);
        g.state.tableau[0] = vec![card(5, 0, false), card(13, 2, true), card(12, 1, true)];
        g.state.tableau[1].clear();
        let from = Selection {
            pile: Pile::Tableau(0),
            index: 1,
        };
        assert!(g.transfer(from, Pile::Tableau(1)));
        assert!(g.state.tableau[0][0].face);
        assert_eq!(g.state.tableau[1].len(), 2);
        g.undo();
        assert!(!g.state.tableau[0][0].face);
        assert_eq!(g.state.tableau[0].len(), 3);
        assert!(g.state.tableau[1].is_empty());
    }

    #[test]
    fn legal_draws_moves_and_undo_always_conserve_a_complete_deck() {
        let mut g = Solitaire::new(99);
        let mut rng = Rng(31337);
        for _ in 0..1000 {
            match rng.next() % 5 {
                0 => g.draw_stock(),
                1 => g.undo(),
                _ => {
                    let column = rng.next() as usize % 7;
                    let len = g.state.tableau[column].len();
                    let source = if rng.next() % 3 == 0 {
                        Selection {
                            pile: Pile::Waste,
                            index: 0,
                        }
                    } else {
                        Selection {
                            pile: Pile::Tableau(column),
                            index: if len == 0 {
                                0
                            } else {
                                rng.next() as usize % len
                            },
                        }
                    };
                    let target = if rng.next() % 3 == 0 {
                        Pile::Foundation(rng.next() as usize % 4)
                    } else {
                        Pile::Tableau(rng.next() as usize % 7)
                    };
                    g.transfer(source, target);
                }
            }
            assert!(g.state.valid());
        }
    }

    #[test]
    fn every_implemented_game_has_a_valid_round_trip_snapshot() {
        for kind in [
            GameKind::TwentyFortyEight,
            GameKind::Brick,
            GameKind::Solitaire,
        ] {
            let mut game = Games::new(kind);
            let data = game.serialize_snapshot().unwrap();
            game.restore_snapshot(&data).unwrap();
            assert_eq!(data, game.serialize_snapshot().unwrap());
        }
    }

    #[test]
    fn invalid_or_wrong_kind_game_saves_do_not_replace_a_running_game() {
        let mut game = Games::new(GameKind::TwentyFortyEight);
        let before = game.serialize_snapshot().unwrap();
        let other = Games::new(GameKind::Solitaire)
            .serialize_snapshot()
            .unwrap();
        assert!(game.restore_snapshot(&other).is_err());
        assert!(game.restore_snapshot(b"{}").is_err());
        assert_eq!(before, game.serialize_snapshot().unwrap());
        let mut solitaire = Games::new(GameKind::Solitaire);
        if let Game::Solitaire(g) = &mut solitaire.game {
            g.state.stock[0].suit = 4;
        }
        assert!(Games::new(GameKind::Solitaire)
            .restore_snapshot(&solitaire.serialize_snapshot().unwrap())
            .is_err());
    }
}

// Game layouts use text baselines; the shared renderer accepts a top edge.
fn text(r: &mut Renderer, value: &str, x: f32, baseline: f32, size: u16, color: Color, width: f32) {
    r.text(value, x, baseline - size as f32, size, color, width);
}
fn centered(
    r: &mut Renderer,
    value: &str,
    x: f32,
    baseline: f32,
    width: f32,
    size: u16,
    color: Color,
) {
    r.centered(value, x, baseline - size as f32, width, size, color);
}
