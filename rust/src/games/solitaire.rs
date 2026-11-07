//! Source port of ij_sol_engine.go and ij_solitaire.go (Go 1.1).
use super::{draw, GameEffect, Rng};
use crate::{
    input::Action,
    motion::Ease,
    render::{Color, Renderer, Texture},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const SCALE: f32 = 220.0 / 232.0;
const CW: f32 = 68.0;
const CH: f32 = 96.0;
const COL_X: f32 = 46.0;
const COL_GAP: f32 = 12.0;
const TOP: f32 = 6.0;
const TABLEAU: f32 = 116.0;
const OVERLAP: f32 = 26.0;
const DOWN_OVERLAP: f32 = 14.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Card {
    rank: u8,
    suit: u8,
    #[serde(alias = "faceUp")]
    face: bool,
}
impl Card {
    fn red(self) -> bool {
        self.suit < 2
    }
    fn identity(self) -> usize {
        self.suit as usize * 13 + self.rank as usize - 1
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct State {
    stock: Vec<Card>,
    waste: Vec<Card>,
    foundations: [Vec<Card>; 4],
    tableau: [Vec<Card>; 7],
    #[serde(default)]
    #[serde(alias = "stockPasses")]
    stock_passes: u32,
}
impl State {
    fn valid(&self) -> bool {
        let mut seen = [false; 52];
        let mut count = 0;
        for card in self
            .stock
            .iter()
            .chain(&self.waste)
            .chain(self.foundations.iter().flatten())
            .chain(self.tableau.iter().flatten())
        {
            if !(1..=13).contains(&card.rank) || card.suit >= 4 {
                return false;
            }
            let id = card.identity();
            if seen[id] {
                return false;
            }
            seen[id] = true;
            count += 1;
        }
        count == 52
    }
    fn won(&self) -> bool {
        self.foundations.iter().all(|pile| pile.len() == 13)
    }
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
enum Phase {
    #[default]
    Browsing,
    Holding,
    Menu,
    Won,
    Lost,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Stock,
    Waste,
    Foundation(usize),
    Tableau(usize, usize),
    Empty(usize),
}
#[derive(Clone, Copy, Default)]
struct Pos {
    x: f32,
    y: f32,
}
#[derive(Clone)]
struct Flight {
    cards: Vec<Card>,
    from: Pos,
    to: Pos,
    start: f64,
    duration: f64,
    overlap: f32,
    back: bool,
    hide: Option<Target>,
}
#[derive(Clone)]
struct Flip {
    card: Card,
    pos: Pos,
    start: f64,
    hide: Target,
}
enum Completion {
    Build,
    Auto {
        skip_waste: bool,
        skip: Vec<usize>,
    },
    CommitAuto {
        state: State,
        skip_waste: bool,
        skip: Vec<usize>,
    },
    Undo(State),
}
#[derive(Clone, Copy)]
struct CursorMotion {
    from: Pos,
    to: Pos,
    start: f64,
    duration: f64,
}
#[derive(Default)]
struct Runtime {
    phase: Phase,
    targets: Vec<Target>,
    cursor: usize,
    previous: usize,
    held: Option<Target>,
    menu_index: usize,
    flights: Vec<Flight>,
    flips: Vec<Flip>,
    completion: Option<Completion>,
    cursor_motion: Option<CursorMotion>,
    now: f64,
    effect: Option<GameEffect>,
    clock: bool,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Solitaire {
    state: State,
    #[serde(default)]
    history: Vec<State>,
    #[serde(default)]
    wins: u32,
    rng: Rng,
    #[serde(skip)]
    rt: Runtime,
}
impl Solitaire {
    pub(super) fn restore_go_fixture(&mut self, value: &serde_json::Value) -> Result<(), String> {
        let mut state_json = value.get("gameState").unwrap_or(value).clone();
        for field in ["stock", "waste"] {
            if state_json
                .get(field)
                .is_some_and(serde_json::Value::is_null)
            {
                state_json[field] = serde_json::json!([]);
            }
        }
        for field in ["foundations", "tableau"] {
            if let Some(piles) = state_json
                .get_mut(field)
                .and_then(serde_json::Value::as_array_mut)
            {
                for pile in piles {
                    if pile.is_null() {
                        *pile = serde_json::json!([]);
                    }
                }
            }
        }
        let state: State = serde_json::from_value(state_json).map_err(|e| e.to_string())?;
        if !state.valid() {
            return Err("Invalid reference solitaire deck".into());
        }
        self.state = state;
        self.history = serde_json::from_value(
            value
                .get("undoStack")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
        .map_err(|e| e.to_string())?;
        self.wins = value
            .get("wins")
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        self.rt = Runtime::default();
        self.build_targets();
        if self.state.won() {
            self.rt.phase = Phase::Won;
        }
        Ok(())
    }
    pub(super) fn shift_time(&mut self, delta: f64) {
        self.rt.now += delta;
        for flight in &mut self.rt.flights {
            flight.start += delta;
        }
        for flip in &mut self.rt.flips {
            flip.start += delta;
        }
        if let Some(cursor) = &mut self.rt.cursor_motion {
            cursor.start += delta;
        }
    }
    pub(super) fn set_time(&mut self, now: f64) {
        self.rt.now = now;
    }
    pub(super) fn snap_visual_state(&mut self) {
        self.rt.flights.clear();
        self.rt.flips.clear();
        self.rt.completion = None;
        self.rt.cursor_motion = None;
        self.build_targets();
    }
    pub(super) fn new(seed: u64) -> Self {
        let mut game = Self {
            state: State::default(),
            history: Vec::new(),
            wins: 0,
            rng: Rng(seed),
            rt: Runtime::default(),
        };
        game.redeal();
        game
    }
    pub(super) fn valid(&self) -> bool {
        self.state.valid() && self.history.len() <= 20 && self.history.iter().all(State::valid)
    }
    pub(super) fn after_restore(&mut self) {
        self.rt = Runtime::default();
        if self.state.won() {
            self.redeal();
        } else {
            self.build_targets();
        }
    }
    pub(super) fn set_clock(&mut self, shown: bool) {
        self.rt.clock = shown;
    }
    pub(super) fn take_effect(&mut self) -> Option<GameEffect> {
        self.rt.effect.take()
    }
    pub(super) fn resume(&mut self) {
        if self.rt.phase == Phase::Menu {
            self.rt.phase = Phase::Browsing;
        }
    }
    pub(super) fn redeal(&mut self) {
        let mut deck: Vec<_> = (0..4)
            .flat_map(|suit| {
                (1..=13).map(move |rank| Card {
                    rank,
                    suit,
                    face: false,
                })
            })
            .collect();
        for i in (1..deck.len()).rev() {
            let j = self.rng.next() as usize % (i + 1);
            deck.swap(i, j);
        }
        self.state = State::default();
        let mut index = 0;
        for column in 0..7 {
            for row in 0..=column {
                let mut card = deck[index];
                card.face = row == column;
                self.state.tableau[column].push(card);
                index += 1;
            }
        }
        self.state.stock.extend_from_slice(&deck[index..]);
        self.history.clear();
        let now = self.rt.now;
        let clock = self.rt.clock;
        self.rt = Runtime::default();
        self.rt.now = now;
        self.rt.clock = clock;
        let from = position(&self.state, Target::Stock);
        let mut n = 0;
        for column in 0..7 {
            for row in 0..=column {
                let card = self.state.tableau[column][row];
                self.rt.flights.push(Flight {
                    cards: vec![card],
                    from,
                    to: position(&self.state, Target::Tableau(column, row)),
                    start: now + n as f64 * 0.030,
                    duration: 0.120,
                    overlap: 0.0,
                    back: !card.face,
                    hide: None,
                });
                n += 1;
            }
        }
        self.rt.completion = Some(Completion::Build);
    }
    fn animating(&self) -> bool {
        !self.rt.flights.is_empty() || !self.rt.flips.is_empty()
    }
    pub(super) fn tick(&mut self, dt: f32) {
        self.rt.now += dt as f64;
        let now = self.rt.now;
        let before = self.animating();
        self.rt
            .flights
            .retain(|flight| now - flight.start < flight.duration);
        self.rt.flips.retain(|flip| now - flip.start < 0.200);
        if before && !self.animating() {
            if let Some(completion) = self.rt.completion.take() {
                match completion {
                    Completion::Build => self.build_targets(),
                    Completion::Auto { skip_waste, skip } => self.auto_move(skip_waste, skip),
                    Completion::CommitAuto {
                        state,
                        skip_waste,
                        skip,
                    } => {
                        self.remember();
                        self.state = state;
                        self.build_targets();
                        if self.state.won() {
                            self.won();
                        } else {
                            self.auto_move(skip_waste, skip);
                        }
                    }
                    Completion::Undo(state) => {
                        self.state = state;
                        self.rt.phase = Phase::Browsing;
                        self.build_targets();
                    }
                }
            }
        }
    }
    fn remember(&mut self) {
        self.history.push(self.state.clone());
        if self.history.len() > 20 {
            self.history.remove(0);
        }
    }
    fn build_targets(&mut self) {
        let mut targets = vec![Target::Stock];
        if !self.state.waste.is_empty() {
            targets.push(Target::Waste);
        }
        for (i, pile) in self.state.foundations.iter().enumerate() {
            if !pile.is_empty() {
                targets.push(Target::Foundation(i));
            }
        }
        for (column, pile) in self.state.tableau.iter().enumerate() {
            if pile.is_empty() {
                targets.push(Target::Empty(column));
                continue;
            }
            for index in 0..pile.len() {
                if valid_run(pile, index) {
                    targets.push(Target::Tableau(column, index));
                }
            }
        }
        self.rt.targets = targets;
        self.rt.cursor = self.rt.cursor.min(self.rt.targets.len().saturating_sub(1));
    }
    fn current(&self) -> Option<Target> {
        self.rt.targets.get(self.rt.cursor).copied()
    }
    fn exit_holding(&mut self) {
        self.rt.phase = Phase::Browsing;
        self.rt.held = None;
        self.rt.cursor = self.rt.previous;
        self.build_targets();
    }
    pub(super) fn back_consumed(&mut self) -> bool {
        if self.animating() {
            return true;
        }
        match self.rt.phase {
            Phase::Holding => {
                let from = self.current().map(|target| self.cursor_center(target));
                self.exit_holding();
                if let (Some(from), Some(to)) =
                    (from, self.current().map(|t| self.cursor_center(t)))
                {
                    self.rt.cursor_motion = Some(CursorMotion {
                        from,
                        to,
                        start: self.rt.now,
                        duration: 0.150,
                    });
                }
            }
            Phase::Menu => self.rt.phase = Phase::Browsing,
            Phase::Browsing => {
                self.rt.phase = Phase::Menu;
                self.rt.menu_index = 0;
            }
            _ => {}
        }
        true
    }
    pub(super) fn handle(&mut self, action: Action) {
        if self.animating() {
            return;
        }
        match action {
            Action::Start => match self.rt.phase {
                Phase::Menu => self.rt.phase = Phase::Browsing,
                Phase::Browsing => {
                    self.rt.phase = Phase::Menu;
                    self.rt.menu_index = 0;
                }
                _ => {}
            },
            Action::SelectHeld => self.undo(),
            Action::Up | Action::Left | Action::Down | Action::Right => {
                let direction = if matches!(action, Action::Up | Action::Left) {
                    -1
                } else {
                    1
                };
                match self.rt.phase {
                    Phase::Menu => {
                        self.rt.menu_index =
                            (self.rt.menu_index as i32 + direction).clamp(0, 4) as usize
                    }
                    Phase::Won | Phase::Lost => {
                        self.rt.menu_index =
                            (self.rt.menu_index as i32 + direction).clamp(0, 1) as usize
                    }
                    _ => {
                        let n = self.rt.targets.len() as i32;
                        if n > 0 {
                            self.rt.cursor =
                                (self.rt.cursor as i32 + direction).rem_euclid(n) as usize;
                        }
                    }
                }
            }
            Action::Select => self.select(),
            _ => {}
        }
    }
    fn select(&mut self) {
        match self.rt.phase {
            Phase::Won | Phase::Lost => {
                if self.rt.menu_index == 0 {
                    self.redeal();
                } else {
                    self.rt.effect = Some(GameEffect::Quit);
                }
                return;
            }
            Phase::Menu => {
                match self.rt.menu_index {
                    0 => self.rt.phase = Phase::Browsing,
                    1 => {
                        self.rt.phase = Phase::Browsing;
                        self.undo();
                    }
                    2 => self.redeal(),
                    3 => {
                        self.rt.clock = !self.rt.clock;
                        self.rt.effect = Some(GameEffect::ToggleClock);
                    }
                    4 => self.rt.effect = Some(GameEffect::Quit),
                    _ => {}
                }
                return;
            }
            Phase::Holding => {
                self.place();
                return;
            }
            _ => {}
        }
        let Some(target) = self.current() else {
            return;
        };
        if target == Target::Stock {
            self.draw_stock();
            return;
        }
        let cards = cards_from(&self.state, target);
        if cards.is_empty() {
            return;
        }
        if let Target::Tableau(column, index) = target {
            if !valid_run(&self.state.tableau[column], index) {
                return;
            }
        }
        let valid = drop_targets(&self.state, &cards, target);
        if valid.is_empty() {
            return;
        }
        let from = self.cursor_center(target);
        self.rt.previous = self.rt.cursor;
        self.rt.phase = Phase::Holding;
        self.rt.held = Some(target);
        self.rt.cursor = 0;
        self.rt.targets = valid;
        if let Some(to) = self.current().map(|target| self.cursor_center(target)) {
            self.rt.cursor_motion = Some(CursorMotion {
                from,
                to,
                start: self.rt.now,
                duration: 0.150,
            });
        }
    }
    fn draw_stock(&mut self) {
        let old = self.state.clone();
        self.remember();
        if let Some(mut card) = self.state.stock.pop() {
            card.face = true;
            self.state.waste.push(card);
            let from = position(&old, Target::Stock);
            let to = position(&self.state, Target::Waste);
            self.rt.flights.push(Flight {
                cards: vec![card],
                from,
                to,
                start: self.rt.now,
                duration: 0.150,
                overlap: 0.0,
                back: false,
                hide: None,
            });
        } else if !self.state.waste.is_empty() {
            self.state.stock = self
                .state
                .waste
                .drain(..)
                .rev()
                .map(|mut card| {
                    card.face = false;
                    card
                })
                .collect();
            self.state.stock_passes = self.state.stock_passes.saturating_add(1);
        }
        self.build_targets();
        self.schedule_auto(false, Vec::new());
    }
    fn place(&mut self) {
        let (Some(from), Some(to)) = (self.rt.held, self.current()) else {
            return;
        };
        let cards = cards_from(&self.state, from);
        let Some(result) = move_cards(&self.state, from, to) else {
            return;
        };
        let old = self.state.clone();
        let from_pos = position(&self.state, from);
        self.remember();
        self.state = result;
        self.exit_holding();
        let destination = match to {
            Target::Foundation(_) => to,
            Target::Tableau(column, _) | Target::Empty(column) => {
                Target::Tableau(column, self.state.tableau[column].len() - cards.len())
            }
            _ => return,
        };
        self.rt.flights.push(Flight {
            cards,
            from: from_pos,
            to: position(&self.state, destination),
            start: self.rt.now,
            duration: 0.200,
            overlap: 24.0 * SCALE,
            back: false,
            hide: Some(destination),
        });
        if self.state.won() {
            self.won();
            return;
        }
        for column in 0..7 {
            let before = &old.tableau[column];
            let after = &self.state.tableau[column];
            if !after.is_empty() && after.len() < before.len() {
                let index = after.len() - 1;
                if after[index].face && !before[index].face {
                    let target = Target::Tableau(column, index);
                    self.rt.flips.push(Flip {
                        card: after[index],
                        pos: position(&self.state, target),
                        start: self.rt.now + 0.200,
                        hide: target,
                    });
                    break;
                }
            }
        }
        let skip = match to {
            Target::Tableau(column, _) | Target::Empty(column) => vec![column],
            _ => Vec::new(),
        };
        self.schedule_auto(false, skip);
    }
    fn schedule_auto(&mut self, skip_waste: bool, skip: Vec<usize>) {
        if self.animating() {
            self.rt.completion = Some(Completion::Auto { skip_waste, skip });
        } else {
            self.auto_move(skip_waste, skip);
        }
    }
    fn auto_move(&mut self, skip_waste: bool, skip: Vec<usize>) {
        if let Some(from) = find_auto(&self.state, skip_waste, &skip) {
            let card = cards_from(&self.state, from)[0];
            let target = Target::Foundation(card.suit as usize);
            if let Some(state) = move_cards(&self.state, from, target) {
                self.rt.flights.push(Flight {
                    cards: vec![card],
                    from: position(&self.state, from),
                    to: position(&state, target),
                    start: self.rt.now,
                    duration: 0.150,
                    overlap: 0.0,
                    back: false,
                    hide: None,
                });
                self.rt.completion = Some(Completion::CommitAuto {
                    state,
                    skip_waste,
                    skip,
                });
                return;
            }
        }
        self.build_targets();
        if !has_moves(&self.state) {
            self.rt.phase = Phase::Lost;
            self.rt.menu_index = 0;
        }
    }
    fn won(&mut self) {
        self.wins = self.wins.saturating_add(1);
        self.rt.phase = Phase::Won;
        self.rt.menu_index = 0;
    }
    pub(super) fn undo(&mut self) {
        if self.animating() || matches!(self.rt.phase, Phase::Won | Phase::Lost) {
            return;
        }
        let Some(previous) = self.history.pop() else {
            return;
        };
        if self.rt.phase == Phase::Holding {
            self.exit_holding();
        } else if self.rt.phase == Phase::Menu {
            self.rt.phase = Phase::Browsing;
        }
        if let Some((cards, from, to)) = find_diff(&previous, &self.state) {
            self.rt.flights.push(Flight {
                overlap: if cards.len() > 1 { 24.0 * SCALE } else { 0.0 },
                cards,
                from: position(&self.state, to),
                to: position(&previous, from),
                start: self.rt.now,
                duration: 0.200,
                back: false,
                hide: None,
            });
            self.rt.completion = Some(Completion::Undo(previous));
        } else {
            self.state = previous;
            self.build_targets();
        }
    }
    fn cursor_center(&self, target: Target) -> Pos {
        let p = position(&self.state, target);
        if let Target::Tableau(column, _) = target {
            let pile = &self.state.tableau[column];
            if self.rt.phase == Phase::Browsing {
                let bottom = card_y(pile, pile.len() - 1) + CH;
                return Pos {
                    x: p.x + CW * 0.5,
                    y: p.y + (bottom - p.y) * 0.5,
                };
            }
            if self.rt.phase == Phase::Holding && !pile.is_empty() {
                return Pos {
                    x: p.x + CW * 0.5,
                    y: card_y(pile, pile.len() - 1) + OVERLAP + CH * 0.5,
                };
            }
        }
        Pos {
            x: p.x + CW * 0.5,
            y: p.y + CH * 0.5,
        }
    }
}

impl Solitaire {
    pub(super) fn draw(
        &self,
        r: &mut Renderer,
        _dark: bool,
        assets: &HashMap<&'static str, Texture>,
    ) {
        use draw::rgb;
        r.push_clip(0.0, 40.0, 640.0, 440.0);
        r.push_transform(0.0, 40.0, 1.0, 1.0, 1.0);
        r.gradient_stops(
            0.0,
            0.0,
            640.0,
            440.0,
            &[
                (0.0, rgb(45, 94, 63)),
                (0.5, rgb(30, 77, 48)),
                (1.0, rgb(22, 61, 37)),
            ],
        );
        let mut hidden: Vec<Target> = self
            .rt
            .flights
            .iter()
            .filter_map(|flight| flight.hide)
            .collect();
        hidden.extend(self.rt.flips.iter().map(|flip| flip.hide));
        let stock = position(&self.state, Target::Stock);
        if !self.state.stock.is_empty() {
            draw_card(r, None, stock.x, stock.y, assets);
            highlight(r, stock.x, stock.y, CW, CH, self.highlight(Target::Stock));
        } else {
            empty(r, stock.x, stock.y, None, assets);
            if !self.state.waste.is_empty() {
                if let Some(texture) = assets.get("sol_recycle") {
                    let side = CW.min(CH) * 0.45;
                    r.image(
                        *texture,
                        stock.x + (CW - side) * 0.5,
                        stock.y + (CH - side) * 0.5,
                        side,
                        side,
                        0.5,
                    );
                }
                highlight(r, stock.x, stock.y, CW, CH, self.highlight(Target::Stock));
            }
        }
        if !self.state.waste.is_empty() {
            let pos = position(&self.state, Target::Waste);
            let hidden_top = hidden.contains(&Target::Waste);
            let index =
                self.state.waste.len() - 1 - usize::from(hidden_top && self.state.waste.len() > 1);
            if !hidden_top || self.state.waste.len() > 1 {
                draw_card(r, Some(self.state.waste[index]), pos.x, pos.y, assets);
            }
            if !hidden_top {
                highlight(
                    r,
                    pos.x,
                    pos.y,
                    CW,
                    CH,
                    if self.rt.phase == Phase::Holding && self.rt.held == Some(Target::Waste) {
                        Some(false)
                    } else {
                        self.highlight(Target::Waste)
                    },
                );
            }
        }
        for i in 0..4 {
            let pos = position(&self.state, Target::Foundation(i));
            let pile = &self.state.foundations[i];
            let hidden_top = hidden.contains(&Target::Foundation(i));
            let visible = pile.len().saturating_sub(usize::from(hidden_top));
            if visible == 0 {
                empty(r, pos.x, pos.y, Some(i), assets);
            } else {
                draw_card(r, Some(pile[visible - 1]), pos.x, pos.y, assets);
            }
            highlight(
                r,
                pos.x,
                pos.y,
                CW,
                CH,
                if self.rt.phase == Phase::Holding && self.rt.held == Some(Target::Foundation(i)) {
                    Some(false)
                } else {
                    self.highlight(Target::Foundation(i))
                },
            );
        }
        for column in 0..7 {
            let pile = &self.state.tableau[column];
            let x = COL_X + column as f32 * (CW + COL_GAP);
            if pile.is_empty() {
                empty(r, x, TABLEAU, None, assets);
                highlight(r, x, TABLEAU, CW, CH, self.highlight(Target::Empty(column)));
                continue;
            }
            let hidden_from = hidden
                .iter()
                .filter_map(|target| match target {
                    Target::Tableau(c, index) if *c == column => Some(*index),
                    _ => None,
                })
                .min()
                .unwrap_or(usize::MAX);
            for (index, &card) in pile.iter().enumerate() {
                if index < hidden_from {
                    draw_card(
                        r,
                        if card.face { Some(card) } else { None },
                        x,
                        card_y(pile, index),
                        assets,
                    );
                }
            }
            let bottom = card_y(pile, pile.len() - 1) + CH;
            if self.rt.phase == Phase::Browsing {
                if let Some(Target::Tableau(c, index)) = self.current() {
                    if c == column {
                        let y = card_y(pile, index);
                        highlight(r, x, y, CW, bottom - y, Some(false));
                    }
                }
            }
            if self.rt.phase == Phase::Holding {
                if let Some(Target::Tableau(c, index)) = self.rt.held {
                    if c == column {
                        let y = card_y(pile, index);
                        highlight(r, x, y, CW, bottom - y, Some(true));
                    }
                }
                if let Some(Target::Tableau(c, _)) = self.current() {
                    if c == column {
                        highlight(
                            r,
                            x,
                            card_y(pile, pile.len() - 1) + OVERLAP,
                            CW,
                            CH,
                            Some(true),
                        );
                    }
                }
            }
        }
        for flip in &self.rt.flips {
            let elapsed = self.rt.now - flip.start;
            if elapsed < 0.0 {
                continue;
            }
            let phase = (elapsed / 0.200).clamp(0.0, 1.0) * 2.0;
            let back = phase <= 1.0;
            let scale = if back {
                1.0 - Ease::OutCubic.sample(phase)
            } else {
                Ease::OutCubic.sample(phase - 1.0)
            }
            .max(0.01) as f32;
            r.push_transform(flip.pos.x + CW * 0.5, flip.pos.y, scale, 1.0, 1.0);
            draw_card(
                r,
                if back { None } else { Some(flip.card) },
                -CW * 0.5,
                0.0,
                assets,
            );
            r.pop_transform();
        }
        for flight in &self.rt.flights {
            let elapsed = self.rt.now - flight.start;
            if elapsed < 0.0 {
                continue;
            }
            let p = Ease::OutCubic.sample(elapsed / flight.duration) as f32;
            let x = flight.from.x + (flight.to.x - flight.from.x) * p;
            let y = flight.from.y + (flight.to.y - flight.from.y) * p;
            for (index, &card) in flight.cards.iter().enumerate() {
                let y = y + index as f32 * flight.overlap;
                draw::soft_shadow(
                    r,
                    x + 2.0 * SCALE,
                    y + 4.0 * SCALE,
                    CW,
                    CH,
                    8.0 * SCALE,
                    0.3,
                );
                draw_card(r, if flight.back { None } else { Some(card) }, x, y, assets);
            }
        }
        if matches!(self.rt.phase, Phase::Browsing | Phase::Holding) {
            if let Some(target) = self.current() {
                let mut pos = self.cursor_center(target);
                if let Some(motion) = self.rt.cursor_motion {
                    if self.rt.now - motion.start < motion.duration {
                        let p = Ease::OutCubic
                            .sample((self.rt.now - motion.start) / motion.duration)
                            as f32;
                        pos = Pos {
                            x: motion.from.x + (motion.to.x - motion.from.x) * p,
                            y: motion.from.y + (motion.to.y - motion.from.y) * p,
                        };
                    }
                }
                if let Some(hand) = assets.get("sol_hand") {
                    let h = 52.0 * SCALE;
                    let w = h * 435.0 / 512.0;
                    r.image(
                        *hand,
                        pos.x - w * 0.4,
                        pos.y - h * 0.55 + 40.0 * SCALE,
                        w,
                        h,
                        1.0,
                    );
                }
            }
        }
        match self.rt.phase {
            Phase::Menu => {
                let labels = [
                    "Resume",
                    "Undo",
                    "Redeal",
                    if self.rt.clock {
                        "Hide the Time"
                    } else {
                        "Show the Time"
                    },
                    "Quit",
                ];
                let sub = if self.wins == 1 {
                    "1 win".into()
                } else {
                    format!("{} wins", self.wins)
                };
                draw_menu(r, &labels, self.rt.menu_index, "Paused", &sub);
            }
            Phase::Won => {
                let sub = if self.wins == 1 {
                    "1 win total".into()
                } else {
                    format!("{} wins total", self.wins)
                };
                draw_menu(
                    r,
                    &["New Game", "Quit"],
                    self.rt.menu_index,
                    "You Win!",
                    &sub,
                );
            }
            Phase::Lost => draw_menu(
                r,
                &["Redeal", "Quit"],
                self.rt.menu_index,
                "No More Moves",
                "",
            ),
            _ => {}
        }
        r.pop_transform();
        r.pop_clip();
    }
    /// Orange is true; blue is false.
    fn highlight(&self, target: Target) -> Option<bool> {
        if matches!(self.rt.phase, Phase::Browsing | Phase::Holding)
            && self.current() == Some(target)
        {
            Some(true)
        } else {
            None
        }
    }
}

fn suit(
    r: &mut Renderer,
    assets: &HashMap<&'static str, Texture>,
    suit: usize,
    cx: f32,
    cy: f32,
    size: f32,
    opacity: f32,
) {
    let name = ["sol_heart", "sol_diamond", "sol_club", "sol_spade"][suit];
    if let Some(texture) = assets.get(name) {
        r.image(
            *texture,
            cx - size * 0.5,
            cy - size * 0.5,
            size,
            size,
            opacity,
        );
    }
}
fn draw_card(
    r: &mut Renderer,
    card: Option<Card>,
    x: f32,
    y: f32,
    assets: &HashMap<&'static str, Texture>,
) {
    use draw::{rgb, rounded};
    draw::soft_shadow(r, x, y + 2.0 * SCALE, CW, CH, 6.0 * SCALE, 0.2);
    rounded(r, x, y, CW, CH, 6.0, &[(0.0, [1.0; 4]), (1.0, [1.0; 4])]);
    if let Some(card) = card {
        rounded(
            r,
            x,
            y,
            CW,
            CH,
            6.0,
            &[(0.0, [1.0; 4]), (1.0, rgb(232, 232, 232))],
        );
        stroke_round(
            r,
            x - 1.0,
            y - 1.0,
            CW + 2.0,
            CH + 2.0,
            7.0,
            2.0,
            rgb(170, 170, 170),
        );
        let color = if card.red() {
            rgb(204, 0, 0)
        } else {
            rgb(0, 0, 0)
        };
        let font = (10.0 * SCALE).floor() * 2.0;
        let small = (9.0 * SCALE).floor() * 2.0;
        let large = (12.0 * SCALE).floor() * 2.0;
        let left = x + 6.0 * SCALE;
        let top = y + 4.0 * SCALE;
        let baseline = top + font * 0.96875 * 0.84;
        let label = match card.rank {
            1 => "A".into(),
            11 => "J".into(),
            12 => "Q".into(),
            13 => "K".into(),
            n => n.to_string(),
        };
        let width = r.text_width_weighted(&label, font, 700);
        r.text_baseline(&label, left, baseline, font, 700, color, width + 1.0);
        let bounds = r.text_bounds_weighted(&label, font, 700);
        suit(
            r,
            assets,
            card.suit as usize,
            left + width + 2.0 * SCALE + small * 0.5,
            baseline + (bounds.0 + bounds.1) * 0.5,
            small,
            1.0,
        );
        suit(
            r,
            assets,
            card.suit as usize,
            x + CW * 0.5,
            y + CH * 0.5,
            large,
            1.0,
        );
    } else {
        let border = 4.0 * SCALE;
        let inner_width = CW - 2.0 * border;
        let inner_height = CH - 2.0 * border;
        rounded(
            r,
            x + border,
            y + border,
            inner_width,
            inner_height,
            4.0,
            &[(0.0, rgb(74, 144, 217)), (1.0, rgb(27, 90, 175))],
        );
        let stripe = 3.0 * SCALE;
        let gap = 5.0 * SCALE;
        let mut sy = y + border + 2.0 * SCALE;
        while sy < y + border + inner_height - 2.0 * SCALE {
            let height = stripe.min(y + border + inner_height - 2.0 * SCALE - sy);
            if height > 0.0 {
                r.rect(
                    x + border + 2.0 * SCALE,
                    sy,
                    inner_width - border,
                    height,
                    [106.0 / 255.0, 171.0 / 255.0, 224.0 / 255.0, 0.3],
                );
            }
            sy += stripe + gap;
        }
        let logo_height = inner_height * 0.34;
        let logo_width = logo_height * 0.814;
        if let Some(logo) = assets.get("sol_apple") {
            r.image(
                *logo,
                x + (CW - logo_width) * 0.5,
                y + (CH - logo_height) * 0.5,
                logo_width,
                logo_height,
                0.3,
            );
        }
    }
}
fn empty(
    r: &mut Renderer,
    x: f32,
    y: f32,
    suit_id: Option<usize>,
    assets: &HashMap<&'static str, Texture>,
) {
    dashed_round(r, x, y, CW, CH, 6.0, [1.0, 1.0, 1.0, 0.3]);
    if let Some(id) = suit_id {
        suit(
            r,
            assets,
            id,
            x + CW * 0.5,
            y + CH * 0.5,
            (22.0 * SCALE).floor() * 2.0,
            0.2,
        );
    }
}
fn highlight(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, kind: Option<bool>) {
    let Some(orange) = kind else {
        return;
    };
    let color = if orange {
        draw::rgb(255, 140, 0)
    } else {
        draw::rgb(46, 107, 230)
    };
    let pad = 4.0 * SCALE;
    let line = 4.0 * SCALE;
    let mut fill = color;
    fill[3] = 0.15;
    draw::rounded(
        r,
        x - pad,
        y - pad,
        w + 2.0 * pad,
        h + 2.0 * pad,
        8.0,
        &[(0.0, fill), (1.0, fill)],
    );
    stroke_round(
        r,
        x - pad - line * 0.5,
        y - pad - line * 0.5,
        w + 2.0 * pad + line,
        h + 2.0 * pad + line,
        8.0 + line * 0.5,
        line,
        color,
    );
}
#[allow(clippy::too_many_arguments)]
fn stroke_round(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    line: f32,
    color: Color,
) {
    r.stroke_rounded(
        x + line * 0.5,
        y + line * 0.5,
        (w - line).max(0.0),
        (h - line).max(0.0),
        (radius - line * 0.5).max(0.0),
        line,
        color,
    );
}

fn dashed_round(r: &mut Renderer, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
    let perimeter = 2.0 * (w + h) - 8.0 * radius + std::f32::consts::TAU * radius;
    let point = |mut distance: f32| {
        distance = distance.rem_euclid(perimeter);
        let segments = [
            w - 2.0 * radius,
            radius * std::f32::consts::FRAC_PI_2,
            h - 2.0 * radius,
            radius * std::f32::consts::FRAC_PI_2,
            w - 2.0 * radius,
            radius * std::f32::consts::FRAC_PI_2,
            h - 2.0 * radius,
            radius * std::f32::consts::FRAC_PI_2,
        ];
        for (i, length) in segments.into_iter().enumerate() {
            if distance <= length {
                return match i {
                    0 => (x + radius + distance, y),
                    1 => {
                        let a = -std::f32::consts::FRAC_PI_2 + distance / radius;
                        (
                            x + w - radius + radius * a.cos(),
                            y + radius + radius * a.sin(),
                        )
                    }
                    2 => (x + w, y + radius + distance),
                    3 => {
                        let a = distance / radius;
                        (
                            x + w - radius + radius * a.cos(),
                            y + h - radius + radius * a.sin(),
                        )
                    }
                    4 => (x + w - radius - distance, y + h),
                    5 => {
                        let a = distance / radius + std::f32::consts::FRAC_PI_2;
                        (
                            x + radius + radius * a.cos(),
                            y + h - radius + radius * a.sin(),
                        )
                    }
                    6 => (x, y + h - radius - distance),
                    _ => {
                        let a = distance / radius + std::f32::consts::PI;
                        (x + radius + radius * a.cos(), y + radius + radius * a.sin())
                    }
                };
            }
            distance -= length;
        }
        (x, y)
    };
    let mut distance = 0.0;
    while distance < perimeter {
        let mut d = distance;
        while d < (distance + 8.0).min(perimeter) {
            let a = point(d);
            let b = point((d + 1.0).min(perimeter));
            r.line(a.0, a.1, b.0, b.1, 2.0, color);
            d += 1.0;
        }
        distance += 14.0;
    }
}
fn draw_menu(r: &mut Renderer, labels: &[&str], selected: usize, title: &str, subtitle: &str) {
    use draw::{centered, rgb, rounded};
    r.rect(0.0, 0.0, 640.0, 440.0, [0.0, 0.0, 0.0, 0.7]);
    let item_h = 48.0 * SCALE;
    let title_h = 56.0 * SCALE;
    let sub_h = if subtitle.is_empty() {
        0.0
    } else {
        36.0 * SCALE
    };
    let total = title_h + sub_h + labels.len() as f32 * item_h + 16.0 * SCALE;
    let width = 320.0 * SCALE;
    let x = (640.0 - width) * 0.5;
    let y = (440.0 - total) * 0.5;
    rounded(
        r,
        x,
        y,
        width,
        total,
        12.0,
        &[
            (0.0, [40.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 0.95]),
            (1.0, [40.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 0.95]),
        ],
    );
    let font = (12.0 * SCALE).floor() * 2.0;
    centered(
        r,
        title,
        320.0,
        y + title_h * 0.5 + font * 0.35,
        font,
        700,
        [1.0; 4],
    );
    if !subtitle.is_empty() {
        let font = (9.0 * SCALE).floor() * 2.0;
        centered(
            r,
            subtitle,
            320.0,
            y + title_h + sub_h * 0.5 + font * 0.35,
            font,
            400,
            [1.0, 1.0, 1.0, 0.6],
        );
    }
    for (i, label) in labels.iter().enumerate() {
        let yy = y + title_h + sub_h + i as f32 * item_h;
        if i == selected {
            rounded(
                r,
                x + 12.0 * SCALE,
                yy + 4.0 * SCALE,
                width - 24.0 * SCALE,
                item_h - 8.0 * SCALE,
                8.0,
                &[(0.0, rgb(74, 144, 217)), (1.0, rgb(74, 144, 217))],
            );
        }
        let font = (11.0 * SCALE).floor() * 2.0;
        centered(
            r,
            label,
            320.0,
            yy + item_h * 0.5 + font * 0.35,
            font,
            400,
            [1.0; 4],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn card(rank: u8, suit: u8) -> Card {
        Card {
            rank,
            suit,
            face: true,
        }
    }
    #[test]
    fn original_layout_matches_six_eighty_pixel_columns() {
        let mut game = Solitaire::new(7);
        game.snap_visual_state();
        assert!(game.valid());
        assert_eq!(position(&game.state, Target::Stock).x, 46.0);
        assert_eq!(position(&game.state, Target::Foundation(3)).x, 526.0);
        assert_eq!(CW, 68.0);
        assert_eq!(CH, 96.0);
        assert_eq!(TABLEAU, 116.0);
    }
    #[test]
    fn foundations_have_fixed_suits_and_auto_moves_are_conservative() {
        let mut state = State::default();
        state.stock.push(card(13, 3));
        assert!(can_foundation(card(1, 2), &[], 2));
        assert!(!can_foundation(card(1, 2), &[], 0));
        assert!(!safe_auto(card(4, 2), &state));
        state.foundations[0] = vec![card(2, 0)];
        state.foundations[1] = vec![card(2, 1)];
        assert!(safe_auto(card(4, 2), &state));
    }
    #[test]
    fn stock_recycle_and_twenty_undos_conserve_the_deck() {
        let mut game = Solitaire::new(27);
        game.snap_visual_state();
        for _ in 0..60 {
            game.draw_stock();
            game.tick(2.0);
            assert!(game.valid());
            assert!(game.history.len() <= 20);
        }
        for _ in 0..30 {
            game.undo();
            game.tick(2.0);
            assert!(game.valid());
        }
    }
    #[test]
    fn picking_a_stack_offers_only_legal_destinations() {
        let mut state = State::default();
        state.tableau[0] = vec![card(8, 2)];
        state.tableau[1] = vec![card(7, 0), card(6, 2)];
        let cards = cards_from(&state, Target::Tableau(1, 0));
        assert!(
            drop_targets(&state, &cards, Target::Tableau(1, 0)).contains(&Target::Tableau(0, 0))
        );
        assert!(move_cards(&state, Target::Tableau(1, 0), Target::Empty(2)).is_none());
    }
    #[test]
    fn menu_back_and_long_press_follow_original_phases() {
        let mut game = Solitaire::new(19);
        game.snap_visual_state();
        assert!(game.back_consumed());
        assert_eq!(game.rt.phase, Phase::Menu);
        game.handle(Action::Start);
        assert_eq!(game.rt.phase, Phase::Browsing);
        game.draw_stock();
        for _ in 0..60 {
            if !game.animating() {
                break;
            }
            game.tick(2.0);
        }
        let before = game.history.len();
        game.handle(Action::SelectHeld);
        game.tick(2.0);
        assert_eq!(game.history.len(), before.saturating_sub(1));
        assert!(game.valid());
    }
}

fn valid_run(pile: &[Card], from: usize) -> bool {
    from < pile.len()
        && pile[from..].iter().all(|c| c.face)
        && pile[from..]
            .windows(2)
            .all(|pair| pair[0].red() != pair[1].red() && pair[0].rank == pair[1].rank + 1)
}
fn cards_from(state: &State, target: Target) -> Vec<Card> {
    match target {
        Target::Waste => state.waste.last().copied().into_iter().collect(),
        Target::Foundation(i) => state.foundations[i].last().copied().into_iter().collect(),
        Target::Tableau(c, i) => state.tableau[c].get(i..).unwrap_or_default().to_vec(),
        _ => Vec::new(),
    }
}
fn can_foundation(card: Card, pile: &[Card], suit: usize) -> bool {
    card.suit as usize == suit && card.rank == pile.last().map_or(1, |c| c.rank + 1)
}
fn can_tableau(cards: &[Card], pile: &[Card]) -> bool {
    let Some(first) = cards.first() else {
        return false;
    };
    pile.last().map_or(first.rank == 13, |last| {
        last.face && first.red() != last.red() && first.rank + 1 == last.rank
    })
}
fn drop_targets(state: &State, cards: &[Card], from: Target) -> Vec<Target> {
    let mut targets = Vec::new();
    if cards.is_empty() {
        return targets;
    }
    if cards.len() == 1 {
        for i in 0..4 {
            if from != Target::Foundation(i) && can_foundation(cards[0], &state.foundations[i], i) {
                targets.push(Target::Foundation(i));
            }
        }
    }
    for column in 0..7 {
        if matches!(from,Target::Tableau(c,_)|Target::Empty(c) if c==column) {
            continue;
        }
        let pile = &state.tableau[column];
        if can_tableau(cards, pile) {
            targets.push(if pile.is_empty() {
                Target::Empty(column)
            } else {
                Target::Tableau(column, pile.len() - 1)
            });
        }
    }
    targets
}
fn move_cards(state: &State, from: Target, to: Target) -> Option<State> {
    let cards = cards_from(state, from);
    if cards.is_empty() {
        return None;
    }
    let mut result = state.clone();
    match to {
        Target::Foundation(i)
            if cards.len() == 1 && can_foundation(cards[0], &state.foundations[i], i) =>
        {
            result.foundations[i].push(cards[0])
        }
        Target::Tableau(c, _) | Target::Empty(c) if can_tableau(&cards, &state.tableau[c]) => {
            result.tableau[c].extend(&cards)
        }
        _ => return None,
    }
    match from {
        Target::Waste => {
            result.waste.pop();
        }
        Target::Foundation(i) => {
            result.foundations[i].pop();
        }
        Target::Tableau(c, i) => {
            result.tableau[c].truncate(i);
            if let Some(card) = result.tableau[c].last_mut() {
                card.face = true;
            }
        }
        _ => return None,
    }
    Some(result)
}
fn guaranteed_win(state: &State) -> bool {
    state.stock.is_empty()
        && state.waste.is_empty()
        && state.tableau.iter().flatten().all(|card| card.face)
}
fn safe_auto(card: Card, state: &State) -> bool {
    guaranteed_win(state)
        || card.rank <= 3
        || (0..4)
            .filter(|&i| (i < 2) != card.red())
            .all(|i| state.foundations[i].last().map_or(0, |c| c.rank) >= card.rank - 2)
}
fn find_auto(state: &State, skip_waste: bool, skip: &[usize]) -> Option<Target> {
    let mut sources = Vec::with_capacity(8);
    if !skip_waste && !state.waste.is_empty() {
        sources.push(Target::Waste);
    }
    for column in 0..7 {
        if !skip.contains(&column) {
            if let Some(card) = state.tableau[column].last() {
                if card.face {
                    sources.push(Target::Tableau(column, state.tableau[column].len() - 1));
                }
            }
        }
    }
    sources.into_iter().find(|&target| {
        let card = cards_from(state, target)[0];
        can_foundation(
            card,
            &state.foundations[card.suit as usize],
            card.suit as usize,
        ) && safe_auto(card, state)
    })
}
fn has_moves(state: &State) -> bool {
    if !state.stock.is_empty() || !state.waste.is_empty() {
        return true;
    }
    for (column, pile) in state.tableau.iter().enumerate() {
        for index in 0..pile.len() {
            if !valid_run(pile, index) {
                continue;
            }
            let run = &pile[index..];
            let last = *run.last().unwrap();
            if (0..4).any(|i| can_foundation(last, &state.foundations[i], i)) {
                return true;
            }
            for target in 0..7 {
                if target == column {
                    continue;
                }
                if can_tableau(run, &state.tableau[target])
                    && !(pile[index].rank == 13 && index == 0 && state.tableau[target].is_empty())
                {
                    return true;
                }
            }
        }
    }
    false
}
fn find_diff(old: &State, current: &State) -> Option<(Vec<Card>, Target, Target)> {
    let source = |card: Card| -> Option<Target> {
        if old.waste.len() > current.waste.len()
            && old
                .waste
                .last()
                .is_some_and(|c| c.identity() == card.identity())
        {
            return Some(Target::Waste);
        }
        for i in 0..4 {
            if old.foundations[i].len() > current.foundations[i].len()
                && old.foundations[i]
                    .last()
                    .is_some_and(|c| c.identity() == card.identity())
            {
                return Some(Target::Foundation(i));
            }
        }
        for column in 0..7 {
            if old.tableau[column].len() > current.tableau[column].len() {
                if let Some(index) = old.tableau[column]
                    .iter()
                    .position(|c| c.face && c.identity() == card.identity())
                {
                    return Some(Target::Tableau(column, index));
                }
            }
        }
        None
    };
    for i in 0..4 {
        if current.foundations[i].len() > old.foundations[i].len() {
            let cards = current.foundations[i][old.foundations[i].len()..].to_vec();
            return Some((
                cards.clone(),
                source(cards[0]).unwrap_or(Target::Foundation(i)),
                Target::Foundation(i),
            ));
        }
    }
    for column in 0..7 {
        if current.tableau[column].len() > old.tableau[column].len() {
            let cards = current.tableau[column][old.tableau[column].len()..].to_vec();
            return Some((
                cards.clone(),
                source(cards[0]).unwrap_or(Target::Tableau(column, old.tableau[column].len())),
                Target::Tableau(column, old.tableau[column].len()),
            ));
        }
    }
    if current.waste.len() > old.waste.len() && current.stock.len() < old.stock.len() {
        return Some((
            vec![*current.waste.last().unwrap()],
            Target::Stock,
            Target::Waste,
        ));
    }
    None
}
fn overlaps(pile: &[Card]) -> Vec<f32> {
    let mut values: Vec<_> = pile
        .iter()
        .take(pile.len().saturating_sub(1))
        .map(|card| if card.face { OVERLAP } else { DOWN_OVERLAP })
        .collect();
    let total: f32 = values.iter().sum();
    let available = 322.0 - CH;
    if total > available {
        for value in &mut values {
            *value = (*value * available / total * 0.5).floor() * 2.0;
            *value = value.max(8.0 * SCALE);
        }
    }
    values
}
fn card_y(pile: &[Card], index: usize) -> f32 {
    TABLEAU + overlaps(pile).iter().take(index).sum::<f32>()
}
fn position(state: &State, target: Target) -> Pos {
    match target {
        Target::Stock => Pos { x: COL_X, y: TOP },
        Target::Waste => Pos {
            x: COL_X + CW + COL_GAP,
            y: TOP,
        },
        Target::Foundation(i) => Pos {
            x: 286.0 + i as f32 * 80.0,
            y: TOP,
        },
        Target::Tableau(column, index) => Pos {
            x: COL_X + column as f32 * (CW + COL_GAP),
            y: card_y(&state.tableau[column], index),
        },
        Target::Empty(column) => Pos {
            x: COL_X + column as f32 * (CW + COL_GAP),
            y: TABLEAU,
        },
    }
}
