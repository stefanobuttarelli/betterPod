//! Self-contained Rust games. Logic does no disk I/O and uses fixed-size
//! boards and bounded physics steps, so music keeps its own execution budget.
use crate::input::Action;
use crate::render::{Color, Renderer, Texture};
use crate::visual_assets::VisualAssets;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
mod draw;
mod solitaire;
use solitaire::Solitaire;
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
    effect: Option<GameEffect>,
    clock_shown: bool,
    assets: HashMap<&'static str, Texture>,
    now: f64,
    anchored: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameCommand {
    NewGame,
    NewWord,
    History,
    Resume,
    Undo,
    Redeal,
    ToggleClock,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameEffect {
    Quit,
    ToggleClock,
}
#[derive(Clone, Debug)]
pub struct GameMenuItem {
    pub label: String,
    pub command: GameCommand,
}
#[derive(Clone, Debug)]
pub struct GameHistoryRow {
    pub label: String,
    pub detail: String,
    pub header: bool,
}

#[derive(Serialize, Deserialize)]
enum Game {
    Tiles(Tiles),
    Brick(Brick),
    Solitaire(Solitaire),
    Words(WordGame),
}

impl Games {
    /// Diagnostic fixture adapter for comparing the actual Go 1.1 position.
    /// It reads immutable bytes supplied by the caller, never a filesystem path.
    pub fn restore_reference_fixture(&mut self, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > 512 * 1024 {
            return Err("Reference fixture is too large".into());
        }
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        match &mut self.game {
            Game::Solitaire(game) => game.restore_go_fixture(&value),
            Game::Words(game) => game.restore_go_fixture(&value),
            Game::Tiles(game) => {
                let board: [[u32; 4]; 4] = serde_json::from_value(
                    value.get("board").ok_or("Missing reference board")?.clone(),
                )
                .map_err(|e| e.to_string())?;
                let board: [u32; 16] = board
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap();
                if board
                    .iter()
                    .any(|&v| v != 0 && (!v.is_power_of_two() || v > (1 << 30)))
                {
                    return Err("Invalid reference tile".into());
                }
                game.board = board;
                game.motion.clear();
                game.score = value.get("score").and_then(|v| v.as_u64()).unwrap_or(0);
                game.best = value
                    .get("best")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(game.score);
                game.won = value.get("won").and_then(|v| v.as_bool()).unwrap_or(false);
                game.keep_playing = value
                    .get("keepOn")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                game.over = !can_move(&board);
                game.animation =
                    value.get("elapsed").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                if let Some(tiles) = value.get("tiles").and_then(|v| v.as_array()) {
                    for tile in tiles {
                        let coordinate = |name: &str| {
                            tile.get(name).and_then(|v| v.as_u64()).unwrap_or(0).min(3) as usize
                        };
                        let index = coordinate("r") * 4 + coordinate("c");
                        let gone = tile.get("gone").and_then(|v| v.as_bool()).unwrap_or(false);
                        game.motion.push(MovingTile {
                            value: tile.get("v").and_then(|v| v.as_u64()).unwrap_or(2) as u32,
                            index,
                            from: coordinate("fr") * 4 + coordinate("fc"),
                            gone_to: if gone {
                                Some(coordinate("gr") * 4 + coordinate("gc"))
                            } else {
                                None
                            },
                            born: tile.get("born").and_then(|v| v.as_bool()).unwrap_or(false),
                            merged: tile
                                .get("merged")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                        });
                    }
                }
                Ok(())
            }
            Game::Brick(game) => {
                let number = |name: &str, default: f64| {
                    value
                        .get(name)
                        .and_then(|v| v.as_f64())
                        .filter(|v| v.is_finite())
                        .unwrap_or(default)
                };
                game.level = number("level", 1.0).clamp(1.0, 1_000_000.0) as u32;
                game.level_blocks();
                game.paddle = number("px", game.paddle);
                game.paddle_velocity = number("pvel", 0.0);
                game.ball = [number("bx", game.ball[0]), number("by", game.ball[1])];
                game.velocity = [
                    number("dx", game.velocity[0]),
                    number("dy", game.velocity[1]),
                ];
                game.score = number("score", 0.0).max(0.0) as u64;
                game.lives = number("lives", 3.0).clamp(0.0, 3.0) as u32;
                game.phase = match value.get("phase").and_then(|v| v.as_str()) {
                    Some("playing") => Phase::Playing,
                    Some("gameOver") => Phase::Over,
                    _ => Phase::Waiting,
                };
                game.menu_index = number("menuIndex", 0.0).clamp(0.0, 1.0) as usize;
                if let Some(bricks) = value.get("bricks").and_then(|v| v.as_array()) {
                    for block in &mut game.blocks {
                        block.active = bricks.iter().any(|b| {
                            b.get("x")
                                .and_then(|v| v.as_f64())
                                .is_some_and(|x| (x - block.x).abs() < 0.001)
                                && b.get("y")
                                    .and_then(|v| v.as_f64())
                                    .is_some_and(|y| (y - block.y).abs() < 0.001)
                        });
                    }
                }
                Ok(())
            }
        }
    }
    pub fn new(kind: GameKind) -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Self {
            effect: None,
            clock_shown: false,
            assets: HashMap::new(),
            now: 0.0,
            anchored: false,
            game: match kind {
                GameKind::TwentyFortyEight => Game::Tiles(Tiles::new(seed)),
                GameKind::Brick => Game::Brick(Brick::new()),
                GameKind::Solitaire => Game::Solitaire(Solitaire::new(seed)),
                GameKind::FiveLetters => Game::Words(WordGame::new(seed)),
            },
        }
    }

    pub fn set_assets(&mut self, assets: &VisualAssets) {
        self.assets.clear();
        for name in [
            "sol_apple",
            "sol_club",
            "sol_diamond",
            "sol_heart",
            "sol_spade",
            "sol_recycle",
            "sol_hand",
        ] {
            if let Some(texture) = assets.get(name) {
                self.assets.insert(name, texture);
            }
        }
    }
    pub fn set_time(&mut self, now: f64) {
        if !now.is_finite() {
            return;
        }
        if let Game::Solitaire(game) = &mut self.game {
            if !self.anchored {
                game.shift_time(now - self.now);
            } else {
                game.set_time(now);
            }
        }
        self.now = now;
        self.anchored = true;
    }
    pub fn snap_visual_state(&mut self) {
        match &mut self.game {
            Game::Tiles(game) => game.animation = 1.0,
            Game::Solitaire(game) => game.snap_visual_state(),
            _ => {}
        }
    }

    pub fn set_menu_open(&mut self, open: bool) {
        if let Game::Brick(g) = &mut self.game {
            g.paused = open;
            if open {
                g.accumulator = 0.0;
            }
        }
    }
    pub fn set_clock_shown(&mut self, shown: bool) {
        self.clock_shown = shown;
        if let Game::Solitaire(game) = &mut self.game {
            game.set_clock(shown);
        }
    }
    pub fn take_effect(&mut self) -> Option<GameEffect> {
        self.effect.take()
    }
    pub fn history_rows(&self) -> Vec<GameHistoryRow> {
        match &self.game {
            Game::Words(game) => game.history_rows(),
            _ => Vec::new(),
        }
    }
    pub fn menu_items(&mut self) -> Vec<GameMenuItem> {
        let mut rows = match &mut self.game {
            Game::Tiles(_) => vec![GameMenuItem {
                label: "New Game".into(),
                command: GameCommand::NewGame,
            }],
            Game::Brick(g) => {
                g.paused = true;
                vec![GameMenuItem {
                    label: "New Game".into(),
                    command: GameCommand::NewGame,
                }]
            }
            Game::Solitaire(_) => vec![
                GameMenuItem {
                    label: "Resume".into(),
                    command: GameCommand::Resume,
                },
                GameMenuItem {
                    label: "Undo".into(),
                    command: GameCommand::Undo,
                },
                GameMenuItem {
                    label: "Redeal".into(),
                    command: GameCommand::Redeal,
                },
            ],
            Game::Words(g) => vec![
                GameMenuItem {
                    label: g.new_word_label().into(),
                    command: GameCommand::NewWord,
                },
                GameMenuItem {
                    label: "History".into(),
                    command: GameCommand::History,
                },
            ],
        };
        rows.push(GameMenuItem {
            label: if self.clock_shown {
                "Hide the Time"
            } else {
                "Show the Time"
            }
            .into(),
            command: GameCommand::ToggleClock,
        });
        rows.push(GameMenuItem {
            label: format!("Quit {}", self.title()),
            command: GameCommand::Quit,
        });
        rows
    }
    pub fn command(&mut self, command: GameCommand) {
        match command {
            GameCommand::ToggleClock => {
                self.clock_shown = !self.clock_shown;
                self.effect = Some(GameEffect::ToggleClock);
            }
            GameCommand::Quit => self.effect = Some(GameEffect::Quit),
            _ => match &mut self.game {
                Game::Tiles(g) => {
                    if command == GameCommand::NewGame {
                        g.restart();
                    }
                }
                Game::Brick(g) => {
                    if command == GameCommand::NewGame {
                        *g = Brick::new();
                    }
                    g.paused = false;
                }
                Game::Solitaire(g) => match command {
                    GameCommand::Undo => g.undo(),
                    GameCommand::Redeal => g.redeal(),
                    GameCommand::Resume => g.resume(),
                    _ => {}
                },
                Game::Words(g) => match command {
                    GameCommand::NewWord => g.new_word(),
                    GameCommand::History => g.show_history(),
                    _ => {}
                },
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
        let mut game: Game = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
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
        if let Game::Solitaire(g) = &mut game {
            g.after_restore();
        }
        if let Game::Words(g) = &mut game {
            g.after_restore();
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
        match &mut self.game {
            Game::Tiles(g) => g.handle(action),
            Game::Brick(g) => {
                g.handle(action);
                if g.quit_requested {
                    self.effect = Some(GameEffect::Quit);
                    g.quit_requested = false;
                }
            }
            Game::Solitaire(g) => {
                g.handle(action);
                if let Some(effect) = g.take_effect() {
                    self.effect = Some(effect);
                }
            }
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
            Game::Solitaire(g) => g.back_consumed(),
            Game::Words(g) => g.back_consumed(),
            _ => false,
        }
    }

    pub fn tick(&mut self, dt: f32) {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 10.0 / 60.0)
        } else {
            0.0
        };
        self.now += dt as f64;
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
            Game::Solitaire(g) => g.draw(renderer, dark, &self.assets),
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
    #[serde(skip)]
    motion: Vec<MovingTile>,
}
#[derive(Clone, Copy)]
struct MovingTile {
    value: u32,
    index: usize,
    from: usize,
    gone_to: Option<usize>,
    born: bool,
    merged: bool,
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
            animation: 0.0,
            rng: Rng(seed),
            motion: Vec::with_capacity(20),
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
        self.motion.push(MovingTile {
            value: self.board[self.spawned],
            index: self.spawned,
            from: self.spawned,
            gone_to: None,
            born: true,
            merged: false,
        });
    }

    fn move_board(&mut self, action: Action) -> bool {
        if !matches!(
            action,
            Action::Up | Action::Down | Action::Left | Action::Right
        ) {
            return false;
        }
        let before = self.board;
        self.motion = before
            .iter()
            .enumerate()
            .filter(|(_, value)| **value != 0)
            .map(|(i, &value)| MovingTile {
                value,
                index: i,
                from: i,
                gone_to: None,
                born: false,
                merged: false,
            })
            .collect();
        let mut by_cell = [None; 16];
        for (i, tile) in self.motion.iter().enumerate() {
            by_cell[tile.from] = Some(i);
        }
        self.merged.fill(false);
        let mut moved = false;
        for line in 0..4 {
            let mut pos = 0;
            let mut last: Option<usize> = None;
            for i in 0..4 {
                let Some(tile) = by_cell[cell(action, line, i)] else {
                    continue;
                };
                if let Some(previous) = last {
                    if self.motion[previous].value == self.motion[tile].value
                        && !self.motion[previous].merged
                    {
                        self.motion[tile].gone_to = Some(self.motion[previous].index);
                        self.motion[previous].value = self.motion[previous].value.saturating_mul(2);
                        self.motion[previous].merged = true;
                        self.score = self
                            .score
                            .saturating_add(self.motion[previous].value as u64);
                        self.won |= self.motion[previous].value == 2048;
                        moved = true;
                        continue;
                    }
                }
                let target = cell(action, line, pos);
                moved |= self.motion[tile].index != target;
                self.motion[tile].index = target;
                last = Some(tile);
                pos += 1;
            }
        }
        self.board = [0; 16];
        for tile in &self.motion {
            if tile.gone_to.is_none() {
                self.board[tile.index] = tile.value;
                self.merged[tile.index] = tile.merged;
            }
        }
        if !moved {
            return false;
        }
        self.best = self.best.max(self.score);
        self.spawn();
        self.over = !can_move(&self.board);
        self.animation = 0.0;
        true
    }

    fn handle(&mut self, action: Action) {
        match action {
            Action::Select if self.over => self.restart(),
            Action::Select if self.won && !self.keep_playing => self.keep_playing = true,
            Action::Up | Action::Down | Action::Left | Action::Right
                if !self.over && (!self.won || self.keep_playing) =>
            {
                self.move_board(action);
            }
            _ => {}
        }
    }

    fn draw(&self, r: &mut Renderer, _dark: bool) {
        use crate::motion::Ease;
        use crate::render::line_box;
        use draw::{centered, rgb, rounded, tile_colors};
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
        rounded(
            r,
            24.0,
            20.0,
            400.0,
            400.0,
            14.0,
            &[(0.0, rgb(122, 132, 148)), (1.0, rgb(92, 101, 117))],
        );
        let tile_size = 85.0;
        for row in 0..4 {
            for col in 0..4 {
                let x = 36.0 + col as f32 * 97.0;
                let y = 32.0 + row as f32 * 97.0;
                rounded(
                    r,
                    x,
                    y,
                    tile_size,
                    tile_size,
                    9.0,
                    &[(0.0, rgb(108, 117, 133)), (1.0, rgb(120, 130, 146))],
                );
            }
        }
        let elapsed = self.animation as f64;
        let slide = Ease::OutCubic.sample(elapsed / 0.110) as f32;
        let pop = ((elapsed - 0.110) / 0.140).clamp(0.0, 1.0) as f32;
        let mut draw_tile = |tile: MovingTile| {
            if tile.gone_to.is_some() && slide >= 1.0 {
                return;
            }
            let target = tile.gone_to.unwrap_or(tile.index);
            let row =
                (tile.from / 4) as f32 + ((target / 4) as f32 - (tile.from / 4) as f32) * slide;
            let col =
                (tile.from % 4) as f32 + ((target % 4) as f32 - (tile.from % 4) as f32) * slide;
            let mut scale = 1.0;
            let mut value = tile.value;
            if tile.born {
                if elapsed < 0.110 {
                    return;
                }
                scale = Ease::OutCubic.sample(pop as f64) as f32;
            } else if tile.merged {
                if elapsed < 0.110 {
                    value /= 2;
                } else {
                    scale = 1.0 + 0.12 * (std::f32::consts::PI * pop).sin();
                }
            }
            if scale <= 0.02 {
                return;
            }
            let cx = 36.0 + col * 97.0 + tile_size * 0.5;
            let cy = 32.0 + row * 97.0 + tile_size * 0.5;
            let half = tile_size * 0.5 * scale;
            let (top, bottom, ink) = tile_colors(value);
            rounded(
                r,
                cx - half,
                cy - half,
                half * 2.0,
                half * 2.0,
                9.0 * scale,
                &[(0.0, top), (1.0, bottom)],
            );
            rounded(
                r,
                cx - half + 2.0,
                cy - half + 2.0,
                half * 2.0 - 4.0,
                half - 2.0,
                8.0 * scale,
                &[(0.0, [1.0, 1.0, 1.0, 0.45]), (1.0, [1.0, 1.0, 1.0, 0.05])],
            );
            let label = value.to_string();
            let css = match label.len() {
                1 => 24.0,
                2 => 22.0,
                3 => 18.0,
                4 => 14.0,
                _ => 11.0,
            };
            let size = css * 2.0 * scale;
            centered(
                r,
                &label,
                cx,
                cy + line_box(size).1 * 0.5 - 2.0 * scale,
                size,
                700,
                ink,
            );
        };
        if self.motion.is_empty() {
            for (index, &value) in self.board.iter().enumerate() {
                if value != 0 {
                    draw_tile(MovingTile {
                        value,
                        index,
                        from: index,
                        gone_to: None,
                        born: false,
                        merged: false,
                    });
                }
            }
        } else {
            for tile in self
                .motion
                .iter()
                .filter(|t| t.gone_to.is_some())
                .chain(self.motion.iter().filter(|t| t.gone_to.is_none()))
            {
                draw_tile(*tile);
            }
        }
        for (y, label, value) in [(20.0, "SCORE", self.score), (112.0, "BEST", self.best)] {
            rounded(
                r,
                448.0,
                y,
                170.0,
                78.0,
                12.0,
                &[(0.0, rgb(122, 132, 148)), (1.0, rgb(92, 101, 117))],
            );
            centered(
                r,
                label,
                533.0,
                y + 14.0 + line_box(22.0).1 * 0.5,
                22.0,
                700,
                rgb(230, 234, 240),
            );
            centered(
                r,
                &value.to_string(),
                533.0,
                y + 48.0 + line_box(44.0).1 * 0.5,
                44.0,
                700,
                [1.0; 4],
            );
        }
        for (i, label) in ["[DPAD] slide", "[START] menu", "[B] back"]
            .iter()
            .enumerate()
        {
            r.text_baseline(
                label,
                452.0,
                344.0 + i as f32 * 30.0 + line_box(22.0).1 * 0.5,
                22.0,
                600,
                rgb(90, 98, 112),
                180.0,
            );
        }
        if (self.over || self.won && !self.keep_playing) && elapsed > 0.250 {
            rounded(
                r,
                24.0,
                20.0,
                400.0,
                400.0,
                14.0,
                &[(0.0, [1.0, 1.0, 1.0, 0.72]), (1.0, [1.0, 1.0, 1.0, 0.72])],
            );
            let (title, subtitle) = if self.over {
                ("No more moves", "[A] play again")
            } else {
                ("You made 2048!", "[A] keep going · [START] menu")
            };
            centered(
                r,
                title,
                224.0,
                210.0 + line_box(44.0).1 * 0.5,
                44.0,
                700,
                rgb(40, 44, 52),
            );
            centered(
                r,
                subtitle,
                224.0,
                256.0 + line_box(26.0).1 * 0.5,
                26.0,
                500,
                rgb(70, 76, 88),
            );
        }
        r.pop_transform();
        r.pop_clip();
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
    x: f64,
    y: f64,
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
    paddle: f64,
    paddle_velocity: f64,
    ball: [f64; 2],
    velocity: [f64; 2],
    phase: Phase,
    lives: u32,
    score: u64,
    level: u32,
    paused: bool,
    accumulator: f64,
    #[serde(default)]
    menu_index: usize,
    #[serde(skip)]
    quit_requested: bool,
}
const BRICK_SCALE: f64 = 320.0 / 340.0;
const PADDLE_WIDTH: f64 = 70.0 * BRICK_SCALE;
const PADDLE_Y: f64 = 220.0 - 11.0 * BRICK_SCALE;
const BALL_RADIUS: f64 = 6.0 * BRICK_SCALE;
const BLOCK_WIDTH: f64 = 30.0;
const BLOCK_HEIGHT: f64 = 12.0 * BRICK_SCALE;
impl Brick {
    fn new() -> Self {
        let mut game = Self {
            blocks: Vec::with_capacity(40),
            paddle: 0.0,
            paddle_velocity: 0.0,
            ball: [0.0; 2],
            velocity: [0.0; 2],
            phase: Phase::Waiting,
            lives: 3,
            score: 0,
            level: 1,
            paused: false,
            accumulator: 0.0,
            menu_index: 0,
            quit_requested: false,
        };
        game.reset_player();
        game.reset_ball();
        game.level_blocks();
        game
    }
    fn reset_player(&mut self) {
        self.paddle = (320.0 - PADDLE_WIDTH) * 0.5;
    }
    fn reset_ball(&mut self) {
        self.ball = [
            self.paddle + PADDLE_WIDTH * 0.5,
            PADDLE_Y - BALL_RADIUS - 1.0,
        ];
        self.velocity = [
            std::f64::consts::FRAC_PI_4.cos(),
            -std::f64::consts::FRAC_PI_4.sin(),
        ];
    }
    fn level_blocks(&mut self) {
        self.blocks.clear();
        for row in 0..5 {
            for col in 0..8 {
                self.blocks.push(Block {
                    x: 60.0 * BRICK_SCALE + col as f64 * (BLOCK_WIDTH + 3.0 * BRICK_SCALE),
                    y: 6.0 * BRICK_SCALE + row as f64 * (BLOCK_HEIGHT + 3.0 * BRICK_SCALE),
                    active: true,
                    row,
                });
            }
        }
    }
    fn handle(&mut self, action: Action) {
        match action {
            Action::Left | Action::Right | Action::Up | Action::Down => {
                let direction = if matches!(action, Action::Left | Action::Up) {
                    -1.0
                } else {
                    1.0
                };
                if self.phase == Phase::Over {
                    self.menu_index =
                        (self.menu_index as i32 + direction as i32).clamp(0, 1) as usize;
                } else {
                    let speed = 3.0 * BRICK_SCALE;
                    self.paddle_velocity =
                        (self.paddle_velocity + direction * speed).clamp(-3.0 * speed, 3.0 * speed);
                }
            }
            Action::Select if self.phase == Phase::Over => {
                if self.menu_index == 0 {
                    *self = Self::new();
                } else {
                    self.quit_requested = true;
                }
            }
            Action::Select if self.phase == Phase::Waiting => self.phase = Phase::Playing,
            _ => {}
        }
    }
    fn tick(&mut self, dt: f32) {
        if self.paused {
            self.accumulator = 0.0;
            return;
        }
        self.accumulator = (self.accumulator + dt as f64).min(10.0 / 60.0);
        while self.accumulator >= 1.0 / 60.0 {
            self.step();
            self.accumulator -= 1.0 / 60.0;
        }
        if self.phase != Phase::Playing && self.paddle_velocity == 0.0 {
            self.accumulator = 0.0;
        }
    }
    fn step(&mut self) {
        if self.phase == Phase::Over {
            return;
        }
        self.paddle += self.paddle_velocity;
        self.paddle_velocity *= 0.85;
        if self.paddle_velocity.abs() < 0.1 {
            self.paddle_velocity = 0.0;
        }
        self.paddle = self.paddle.clamp(0.0, 320.0 - PADDLE_WIDTH);
        if self.ball[0] < 0.0 || self.ball[0] > 320.0 {
            self.velocity[0] = -self.velocity[0];
        }
        if self.ball[1] < 0.0 {
            self.velocity[1] = -self.velocity[1];
        }
        if self.ball[1] > 220.0 {
            self.lives = self.lives.saturating_sub(1);
            if self.lives == 0 {
                self.phase = Phase::Over;
                self.menu_index = 0;
            } else {
                self.reset_player();
                self.reset_ball();
                self.phase = Phase::Waiting;
            }
        }
        if self.phase != Phase::Playing {
            return;
        }
        if aabb(
            self.ball[0],
            self.ball[1],
            BALL_RADIUS * 2.0,
            BALL_RADIUS * 2.0,
            self.paddle,
            PADDLE_Y,
            PADDLE_WIDTH,
            8.0 * BRICK_SCALE,
        ) {
            let hit = self.ball[0] - self.paddle;
            let angle = hit * (5.0 * std::f64::consts::PI / 6.0 - std::f64::consts::PI / 6.0)
                / PADDLE_WIDTH
                + std::f64::consts::PI / 6.0;
            self.velocity = [-angle.cos(), -angle.sin()];
        }
        for i in (0..self.blocks.len()).rev() {
            let b = self.blocks[i];
            if !b.active
                || !aabb(
                    self.ball[0],
                    self.ball[1],
                    BALL_RADIUS * 2.0,
                    BALL_RADIUS * 2.0,
                    b.x,
                    b.y,
                    BLOCK_WIDTH,
                    BLOCK_HEIGHT,
                )
            {
                continue;
            }
            self.score += [7, 5, 3, 1, 1][b.row];
            self.blocks[i].active = false;
            if self.blocks.iter().all(|b| !b.active) {
                self.level = self.level.saturating_add(1);
                self.level_blocks();
                self.reset_player();
                self.reset_ball();
                self.phase = Phase::Waiting;
                return;
            }
            let (dx, dy) = (self.velocity[0], self.velocity[1]);
            if dy > 0.0 {
                if self.ball[1] > b.y {
                    self.velocity[0] = -dx;
                } else {
                    self.velocity[1] = -dy;
                }
            } else if dx > 0.0 && dy < 0.0 {
                if self.ball[0] > b.x {
                    self.velocity[1] = -dy;
                } else {
                    self.velocity[0] = -dx;
                }
            } else if dx < 0.0 && dy < 0.0 {
                if self.ball[0] > b.x + BLOCK_WIDTH {
                    self.velocity[0] = -dx;
                } else {
                    self.velocity[1] = -dy;
                }
            }
            break;
        }
        if self.phase == Phase::Playing {
            let speed = (2.0 + (self.level - 1) as f64 * 0.3) * BRICK_SCALE;
            self.ball[0] += speed * self.velocity[0];
            self.ball[1] += speed * self.velocity[1];
        }
    }
    fn draw(&self, r: &mut Renderer, _dark: bool) {
        use draw::{centered, rgb, rounded, soft_shadow, stroke_rect};
        r.push_clip(0.0, 40.0, 640.0, 440.0);
        r.push_transform(0.0, 40.0, 1.0, 1.0, 1.0);
        r.gradient_stops(
            0.0,
            0.0,
            640.0,
            440.0,
            &[
                (0.0, rgb(64, 162, 247)),
                (0.5, rgb(64, 162, 247)),
                (1.0, rgb(141, 204, 254)),
            ],
        );
        let scale = BRICK_SCALE as f32 * 2.0;
        let colors = [
            (rgb(255, 196, 196), rgb(239, 26, 26)),
            (rgb(255, 202, 161), rgb(255, 120, 30)),
            (rgb(254, 242, 189), rgb(240, 195, 31)),
            (rgb(167, 255, 209), rgb(23, 200, 112)),
            (rgb(140, 200, 255), rgb(0, 100, 222)),
        ];
        for block in self.blocks.iter().filter(|b| b.active) {
            let x = block.x as f32 * 2.0;
            let y = block.y as f32 * 2.0;
            let w = BLOCK_WIDTH as f32 * 2.0 - 3.0 * scale;
            let h = BLOCK_HEIGHT as f32 * 2.0 - 3.0 * scale;
            let (top, bottom) = colors[block.row];
            soft_shadow(r, x, y + 2.0 * scale, w, h, 3.0 * scale, 0.3);
            r.gradient_stops(x, y, w, h, &[(0.0, top), (0.5, bottom), (1.0, bottom)]);
            stroke_rect(r, x, y, w, h, 2.0, bottom);
        }
        let paddle = self.paddle as f32 * 2.0;
        let py = PADDLE_Y as f32 * 2.0;
        let pw = PADDLE_WIDTH as f32 * 2.0;
        let ph = 8.0 * scale;
        soft_shadow(r, paddle, py + 2.0 * scale, pw, ph, 4.0 * scale, 0.4);
        r.gradient_stops(
            paddle,
            py,
            pw,
            ph,
            &[
                (0.0, rgb(199, 199, 199)),
                (0.5, rgb(85, 85, 85)),
                (1.0, rgb(111, 111, 111)),
            ],
        );
        stroke_rect(r, paddle, py, pw, ph, scale, [0.0, 0.0, 0.0, 0.5]);
        self.draw_ball(r);
        let mx = 30.0 * scale;
        let font_css = (10.0 * BRICK_SCALE).round() as f32;
        centered(
            r,
            &self.score.to_string(),
            mx,
            (8.0 * BRICK_SCALE as f32 + font_css * 0.8) * 2.0,
            font_css * 2.0,
            700,
            [1.0, 1.0, 1.0, 0.85],
        );
        let total = self.lives.saturating_sub(1) as f32 * 8.0 * scale;
        let first = mx - total * 0.5;
        for i in 0..self.lives {
            r.circle(
                first + i as f32 * 8.0 * scale,
                24.0 * scale,
                2.5 * scale,
                [1.0, 1.0, 1.0, 0.85],
            );
        }
        if self.phase == Phase::Over {
            r.rect(0.0, 0.0, 640.0, 440.0, [0.0, 0.0, 0.0, 0.7]);
            let (title_h, score_h, item_h, pad) =
                (28.0 * scale, 20.0 * scale, 22.0 * scale, 8.0 * scale);
            let panel_h = title_h + score_h + item_h * 2.0 + pad;
            let panel_w = 160.0 * scale;
            let x = (640.0 - panel_w) * 0.5;
            let y = (440.0 - panel_h) * 0.5;
            rounded(
                r,
                x,
                y,
                panel_w,
                panel_h,
                pad,
                &[(0.0, [0.0, 0.0, 0.0, 0.85]), (1.0, [0.0, 0.0, 0.0, 0.85])],
            );
            let fs = (14.0 * BRICK_SCALE).round() as f32 * 2.0;
            centered(
                r,
                "Game Over",
                320.0,
                y + title_h * 0.5 + fs * 0.35,
                fs,
                700,
                [1.0; 4],
            );
            let fs = (11.0 * BRICK_SCALE).round() as f32 * 2.0;
            centered(
                r,
                &format!("Score: {}", self.score),
                320.0,
                y + title_h + score_h * 0.5 + fs * 0.35,
                fs,
                400,
                [1.0, 1.0, 1.0, 0.7],
            );
            for (i, label) in ["New Game", "Quit"].iter().enumerate() {
                let yy = y + title_h + score_h + i as f32 * item_h;
                if i == self.menu_index {
                    rounded(
                        r,
                        x + 10.0 * scale,
                        yy,
                        panel_w - 20.0 * scale,
                        item_h - 2.0 * scale,
                        4.0 * scale,
                        &[
                            (0.0, [64.0 / 255.0, 162.0 / 255.0, 247.0 / 255.0, 0.9]),
                            (1.0, [64.0 / 255.0, 162.0 / 255.0, 247.0 / 255.0, 0.9]),
                        ],
                    );
                }
                let fs = (12.0 * BRICK_SCALE).round() as f32 * 2.0;
                centered(
                    r,
                    label,
                    320.0,
                    yy + item_h * 0.5 + fs * 0.35,
                    fs,
                    700,
                    [1.0; 4],
                );
            }
        }
        r.pop_transform();
        r.pop_clip();
    }
    fn draw_ball(&self, r: &mut Renderer) {
        let (bx, by, rad) = (
            self.ball[0] as f32 * 2.0,
            self.ball[1] as f32 * 2.0,
            BALL_RADIUS as f32 * 2.0,
        );
        let scale = BRICK_SCALE as f32 * 2.0;
        for i in (1..=3).rev() {
            r.circle(
                bx,
                by + 2.0 * scale,
                rad + i as f32 * scale * 0.5,
                [0.0, 0.0, 0.0, 0.1],
            );
        }
        let fx = bx - rad * 0.25;
        let fy = by - rad * 0.25;
        let fr = rad * 0.05;
        for yy in ((by - rad - 2.0).floor() as i32)..=((by + rad + 2.0).ceil() as i32) {
            for xx in ((bx - rad - 2.0).floor() as i32)..=((bx + rad + 2.0).ceil() as i32) {
                let cx = xx as f32 + 0.5;
                let cy = yy as f32 + 0.5;
                let distance = (cx - bx).hypot(cy - by);
                let coverage = (rad - distance + 0.5).clamp(0.0, 1.0);
                if coverage <= 0.0 {
                    continue;
                }
                let t = radial_t(cx, cy, fx, fy, fr, bx, by, rad);
                let shade = if t < 0.3 {
                    171.0 + (98.0 - 171.0) * t / 0.3
                } else {
                    98.0 + (67.0 - 98.0) * (t - 0.3) / 0.7
                };
                r.rect(
                    xx as f32,
                    yy as f32,
                    1.0,
                    1.0,
                    [shade / 255.0, shade / 255.0, shade / 255.0, coverage],
                );
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn aabb(ax: f64, ay: f64, aw: f64, ah: f64, bx: f64, by: f64, bw: f64, bh: f64) -> bool {
    !(ay + ah < by || by + bh < ay || bx + bw < ax || ax + aw < bx)
}
#[allow(clippy::too_many_arguments)]
fn radial_t(x: f32, y: f32, x0: f32, y0: f32, r0: f32, x1: f32, y1: f32, r1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let dr = r1 - r0;
    let px = x - x0;
    let py = y - y0;
    let a = dx * dx + dy * dy - dr * dr;
    let b = px * dx + py * dy + r0 * dr;
    let c = px * px + py * py - r0 * r0;
    if a.abs() < 1e-9 {
        return (c / (2.0 * b)).clamp(0.0, 1.0);
    }
    let disc = b * b - a * c;
    if disc < 0.0 {
        return 1.0;
    }
    let root = disc.sqrt();
    let first = (b + root) / a;
    let second = (b - root) / a;
    let mut t = first.max(second);
    if r0 + t * dr < 0.0 {
        t = first.min(second);
    }
    t.clamp(0.0, 1.0)
}
fn circle_rect(ball: [f64; 2], x: f64, y: f64, w: f64, h: f64) -> bool {
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
        g.paused = true; // The original START sheet owns pause.
        let before = g.ball;
        g.tick(0.1);
        assert_eq!(g.ball, before);
        g = Brick::new();
        assert!(g.phase == Phase::Waiting);
        assert_eq!(g.lives, 3);
        assert_eq!(g.score, 0);
    }
    #[test]
    fn brick_paddle_collision_returns_the_ball_upward() {
        let mut g = Brick::new();
        g.phase = Phase::Playing;
        g.ball = [g.paddle + PADDLE_WIDTH * 0.5, PADDLE_Y - BALL_RADIUS - 0.1];
        g.velocity = [0.0, 1.0];
        g.step();
        assert!(g.velocity[1] < 0.0);
        assert!(g.ball[1] <= PADDLE_Y - BALL_RADIUS);
    }
    #[test]
    fn brick_original_tick_hits_reverse_order_and_uses_row_points() {
        let mut g = Brick::new();
        g.phase = Phase::Playing;
        let block = g.blocks[0];
        g.ball = [block.x, block.y];
        g.velocity = [-0.7071067811865476, -0.7071067811865476];
        g.tick(1.0 / 60.0);
        assert!(!g.blocks[0].active);
        assert_eq!(g.score, 7);
        assert_eq!(g.blocks.iter().filter(|b| b.active).count(), 39);
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn every_game_snapshot_restores_without_changing_persistent_data() {
        for kind in [
            GameKind::TwentyFortyEight,
            GameKind::Brick,
            GameKind::Solitaire,
            GameKind::FiveLetters,
        ] {
            let mut game = Games::new(kind);
            let bytes = game.serialize_snapshot().unwrap();
            game.restore_snapshot(&bytes).unwrap();
            assert_eq!(bytes, game.serialize_snapshot().unwrap());
        }
    }
    #[test]
    fn invalid_or_wrong_kind_save_does_not_replace_running_game() {
        let mut game = Games::new(GameKind::TwentyFortyEight);
        let before = game.serialize_snapshot().unwrap();
        let other = Games::new(GameKind::Solitaire)
            .serialize_snapshot()
            .unwrap();
        assert!(game.restore_snapshot(&other).is_err());
        assert!(game.restore_snapshot(b"{}").is_err());
        assert_eq!(before, game.serialize_snapshot().unwrap());
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
