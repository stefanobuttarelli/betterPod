//! SDL input owned by the UI thread. An idle poll does not allocate.
//! The mapped game controller follows the Anbernic face-button labels.

use sdl2::controller::{Axis, Button, GameController};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::{EventPump, GameControllerSubsystem, Sdl};
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
mod media;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Select,
    SelectHeld,
    Back,
    BackHeld,
    PlayPause,
    Start,
    HoldDown,
    HoldUp,
    MediaPlayPause,
    MediaPlay,
    MediaPause,
    MediaNext,
    MediaPrevious,
    Next,
    Previous,
    VolumeUp,
    VolumeDown,
    BrightnessUp,
    BrightnessDown,
    MenuDown,
    MenuUp,
    Quit,
    Options,
    Search,
}

impl Action {
    fn repeats(self) -> bool {
        matches!(
            self,
            Self::Up
                | Self::Down
                | Self::Left
                | Self::Right
                | Self::VolumeUp
                | Self::VolumeDown
                | Self::BrightnessUp
                | Self::BrightnessDown
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Keyboard(Keycode),
    Controller(u32, Button),
    Axis(u32, Axis),
}

struct Held {
    source: Source,
    action: Action,
    since: Instant,
    last: Instant,
    long_emitted: bool,
}

pub struct Input {
    events: EventPump,
    controllers: Vec<GameController>,
    subsystem: GameControllerSubsystem,
    held: Vec<Held>,
    swap_ab: bool,
    #[cfg(target_os = "linux")]
    media: media::MediaInput,
}

impl Input {
    pub fn new(sdl: &Sdl) -> Result<Self, String> {
        let subsystem = sdl.game_controller()?;
        subsystem.set_event_state(true);
        let mut result = Self {
            events: sdl.event_pump()?,
            controllers: Vec::with_capacity(4),
            subsystem,
            held: Vec::with_capacity(24),
            swap_ab: std::env::var("BETTERPOD_RUST_SWAP_AB").map_or(true, |v| v != "0"),
            #[cfg(target_os = "linux")]
            media: media::MediaInput::new(),
        };
        for index in 0..result.subsystem.num_joysticks()? {
            result.open_controller(index);
        }
        Ok(result)
    }

    fn open_controller(&mut self, index: u32) {
        if self.subsystem.is_game_controller(index) {
            match self.subsystem.open(index) {
                Ok(controller) => {
                    eprintln!("Input: opened {}", controller.name());
                    self.controllers.push(controller);
                }
                Err(error) => eprintln!("Input: controller {index}: {error}"),
            }
        }
    }

    fn press(&mut self, source: Source, action: Action, now: Instant, out: &mut Vec<Action>) {
        if self.held.iter().any(|h| h.source == source) {
            return;
        }
        if !matches!(action, Action::Select | Action::Back) {
            out.push(action);
        }
        self.held.push(Held {
            source,
            action,
            since: now,
            last: now,
            long_emitted: false,
        });
    }

    fn release(&mut self, source: Source, now: Instant, out: &mut Vec<Action>) {
        if self
            .held
            .iter()
            .any(|h| h.source == source && h.action == Action::MenuDown)
        {
            out.push(Action::MenuUp);
        }
        if self
            .held
            .iter()
            .any(|h| h.source == source && h.action == Action::HoldDown)
        {
            out.push(Action::HoldUp);
        }
        for h in self.held.iter().filter(|h| h.source == source) {
            finish_press(h, now, out);
        }
        self.held.retain(|h| h.source != source);
    }

    fn axis(&mut self, id: u32, axis: Axis, value: i16, now: Instant, out: &mut Vec<Action>) {
        let source = Source::Axis(id, axis);
        if matches!(axis, Axis::TriggerLeft | Axis::TriggerRight) {
            if value > 16000 {
                let action = if axis == Axis::TriggerLeft {
                    Action::BrightnessDown
                } else {
                    Action::BrightnessUp
                };
                self.press(source, action, now, out);
            } else {
                self.release(source, now, out);
            }
            return;
        }
        // Keep a held direction until the stick returns near its centre.
        if self.held.iter().any(|h| h.source == source) {
            if value.unsigned_abs() < 11000 {
                self.release(source, now, out);
            }
            return;
        }
        if value.unsigned_abs() < 18000 {
            return;
        }
        let action = match axis {
            Axis::LeftX | Axis::RightX => {
                if value > 0 {
                    Action::Right
                } else {
                    Action::Left
                }
            }
            Axis::LeftY | Axis::RightY => {
                if value > 0 {
                    Action::Down
                } else {
                    Action::Up
                }
            }
            _ => return,
        };
        self.press(source, action, now, out);
    }

    pub fn poll(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        let now = Instant::now();
        while let Some(event) = self.events.poll_event() {
            match event {
                Event::Quit { .. } => out.push(Action::Quit),
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } => {
                    if let Some(action) = keyboard_action(key) {
                        self.press(Source::Keyboard(key), action, now, &mut out);
                    }
                }
                Event::KeyUp {
                    keycode: Some(key), ..
                } => self.release(Source::Keyboard(key), now, &mut out),
                Event::ControllerButtonDown { which, button, .. } => {
                    if let Some(action) = controller_action(button, self.swap_ab) {
                        self.press(Source::Controller(which, button), action, now, &mut out);
                    }
                }
                Event::ControllerButtonUp { which, button, .. } => {
                    self.release(Source::Controller(which, button), now, &mut out)
                }
                Event::ControllerAxisMotion {
                    which, axis, value, ..
                } => self.axis(which, axis, value, now, &mut out),
                Event::ControllerDeviceAdded { which, .. } => self.open_controller(which),
                Event::ControllerDeviceRemoved { which, .. } => {
                    let menu_held = self.held.iter().any(|h| {
                        matches!(h.source, Source::Controller(id, _) if id == which)
                            && h.action == Action::MenuDown
                    });
                    let hold_held = self.held.iter().any(|h| {
                        matches!(h.source, Source::Controller(id, _) if id == which)
                            && h.action == Action::HoldDown
                    });
                    self.controllers.retain(|c| c.instance_id() != which);
                    self.held.retain(|h| !matches!(h.source, Source::Controller(id, _) | Source::Axis(id, _) if id == which));
                    if menu_held {
                        out.push(Action::MenuUp);
                    }
                    if hold_held {
                        out.push(Action::HoldUp);
                    }
                }
                Event::Window {
                    win_event: sdl2::event::WindowEvent::FocusLost,
                    ..
                } => {
                    if self.held.iter().any(|h| h.action == Action::MenuDown) {
                        out.push(Action::MenuUp);
                    }
                    if self.held.iter().any(|h| h.action == Action::HoldDown) {
                        out.push(Action::HoldUp);
                    }
                    self.held.clear();
                }
                Event::MouseWheel { y, .. } => {
                    let action = if y > 0 { Action::Up } else { Action::Down };
                    for _ in 0..y.unsigned_abs().min(8) {
                        out.push(action);
                    }
                }
                _ => {}
            }
        }
        #[cfg(target_os = "linux")]
        self.media.drain(&mut out);
        repeat_held(&mut self.held, now, &mut out);
        out
    }
}

fn finish_press(held: &Held, now: Instant, out: &mut Vec<Action>) {
    if !matches!(held.action, Action::Select | Action::Back) || held.long_emitted {
        return;
    }
    let action = if now.duration_since(held.since) >= Duration::from_millis(500) {
        if held.action == Action::Select {
            Action::SelectHeld
        } else {
            Action::BackHeld
        }
    } else {
        held.action
    };
    out.push(action);
}
fn repeat_held(held: &mut [Held], now: Instant, out: &mut Vec<Action>) {
    for h in held {
        if matches!(h.action, Action::Select | Action::Back) {
            if !h.long_emitted && now.duration_since(h.since) >= Duration::from_millis(500) {
                h.long_emitted = true;
                out.push(if h.action == Action::Select {
                    Action::SelectHeld
                } else {
                    Action::BackHeld
                });
            }
            continue;
        }
        if !h.action.repeats() || now.duration_since(h.since) < Duration::from_millis(300) {
            continue;
        }
        let interval = if now.duration_since(h.since) >= Duration::from_millis(1500) {
            Duration::from_millis(40)
        } else {
            Duration::from_millis(80)
        };
        if now.duration_since(h.last) >= interval {
            // One event after a late poll, never a burst of missed repeats.
            h.last = now;
            out.push(h.action);
        }
    }
}

fn keyboard_action(key: Keycode) -> Option<Action> {
    Some(match key {
        Keycode::Up => Action::Up,
        Keycode::Down => Action::Down,
        Keycode::Left => Action::Left,
        Keycode::Right => Action::Right,
        Keycode::Return | Keycode::A => Action::Select,
        Keycode::Backspace | Keycode::B => Action::Back,
        Keycode::Space | Keycode::X | Keycode::LShift => Action::PlayPause,
        Keycode::F1 => Action::Start,
        Keycode::H | Keycode::RCtrl => Action::HoldDown,
        Keycode::Y | Keycode::LAlt => Action::Options,
        Keycode::N | Keycode::T => Action::Next,
        Keycode::P | Keycode::E => Action::Previous,
        Keycode::Equals => Action::VolumeUp,
        Keycode::Minus => Action::VolumeDown,
        Keycode::PageUp => Action::BrightnessUp,
        Keycode::PageDown => Action::BrightnessDown,
        Keycode::F | Keycode::R => Action::Search,
        Keycode::Application | Keycode::M => Action::MenuDown,
        Keycode::Escape => Action::Back,
        Keycode::Q => Action::Quit,
        _ => {
            return match key.name().as_str() {
                "AudioPlay" => Some(Action::MediaPlayPause),
                "AudioNext" => Some(Action::MediaNext),
                "AudioPrev" => Some(Action::MediaPrevious),
                _ => None,
            }
        }
    })
}

fn controller_action(button: Button, swap_ab: bool) -> Option<Action> {
    Some(match button {
        Button::DPadUp => Action::Up,
        Button::DPadDown => Action::Down,
        Button::DPadLeft => Action::Left,
        Button::DPadRight => Action::Right,
        Button::A => {
            if swap_ab {
                Action::Back
            } else {
                Action::Select
            }
        }
        Button::B => {
            if swap_ab {
                Action::Select
            } else {
                Action::Back
            }
        }
        Button::X => {
            if swap_ab {
                Action::Options
            } else {
                Action::PlayPause
            }
        }
        Button::Y => {
            if swap_ab {
                Action::PlayPause
            } else {
                Action::Options
            }
        }
        Button::Start => Action::Start,
        Button::Back => Action::HoldDown,
        Button::Guide => Action::MenuDown,
        Button::LeftShoulder => Action::Previous,
        Button::RightShoulder => Action::Next,
        Button::LeftStick | Button::RightStick => Action::Select,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_navigation_repeats_without_catch_up_bursts() {
        let start = Instant::now();
        let mut held = [Held {
            source: Source::Keyboard(Keycode::Down),
            action: Action::Down,
            since: start,
            last: start,
            long_emitted: false,
        }];
        let mut out = Vec::new();
        repeat_held(&mut held, start + Duration::from_millis(299), &mut out);
        assert!(out.is_empty());
        repeat_held(&mut held, start + Duration::from_secs(2), &mut out);
        assert_eq!(out, [Action::Down]);
        out.clear();
        repeat_held(&mut held, start + Duration::from_millis(2010), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn held_select_fires_once_and_suppresses_short_release() {
        let start = Instant::now();
        let mut held = [Held {
            source: Source::Keyboard(Keycode::Return),
            action: Action::Select,
            since: start,
            last: start,
            long_emitted: false,
        }];
        let mut out = Vec::new();
        repeat_held(&mut held, start + Duration::from_secs(10), &mut out);
        assert_eq!(out, [Action::SelectHeld]);
        out.clear();
        repeat_held(&mut held, start + Duration::from_secs(11), &mut out);
        finish_press(&held[0], start + Duration::from_secs(12), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn anbernic_labels_match_the_existing_player() {
        assert_eq!(controller_action(Button::B, true), Some(Action::Select));
        assert_eq!(controller_action(Button::A, true), Some(Action::Back));
        assert_eq!(controller_action(Button::Y, true), Some(Action::PlayPause));
        assert_eq!(controller_action(Button::X, true), Some(Action::Options));
    }
    #[test]
    fn tap_and_late_release_are_exclusive_for_both_face_buttons() {
        let start = Instant::now();
        for (action, held_action) in [
            (Action::Select, Action::SelectHeld),
            (Action::Back, Action::BackHeld),
        ] {
            let h = Held {
                source: Source::Keyboard(Keycode::Return),
                action,
                since: start,
                last: start,
                long_emitted: false,
            };
            let mut out = Vec::new();
            finish_press(&h, start + Duration::from_millis(100), &mut out);
            assert_eq!(out, [action]);
            out.clear();
            finish_press(&h, start + Duration::from_millis(500), &mut out);
            assert_eq!(out, [held_action]);
        }
    }
}
