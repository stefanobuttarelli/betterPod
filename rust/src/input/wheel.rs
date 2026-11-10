//! The stick as an iPod click wheel (Go's `BETTERPOD_STICK_WHEEL`): while the
//! stick is pushed out, every `step` degrees of rotation is one click.
//! Clockwise moves down a list, counter-clockwise up.

/// Pushed this far out (of 1.0), the stick's angle is followed.
const ENGAGE: f32 = 0.55;
/// Back under this, the wheel lets go. Between the two the angle is too
/// noisy to follow, but the wheel stays engaged.
const RELEASE: f32 = 0.35;
/// A larger jump between two readings is a flick through the centre, not a
/// rotation.
const MAX_JUMP: f32 = 0.5;

pub(super) const DEFAULT_STEP: f32 = 15.0;

/// Degrees per click from `BETTERPOD_RUST_WHEEL_STEP` (5–90), else 15.
pub(super) fn step_from(value: Option<&str>) -> f32 {
    value
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|v| (5.0..=90.0).contains(v))
        .unwrap_or(DEFAULT_STEP)
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Wheel {
    x: f32,
    y: f32,
    engaged: bool,
    last_angle: f32,
    turned: f32,
}

impl Wheel {
    /// Follows one axis reading. Returns the clicks it completed: positive
    /// clockwise (screen y grows downwards), negative counter-clockwise.
    pub(super) fn axis(&mut self, horizontal: bool, value: i16, step: f32) -> i32 {
        let value = (f32::from(value) / 32767.0).clamp(-1.0, 1.0);
        if horizontal {
            self.x = value;
        } else {
            self.y = value;
        }
        let reach = self.x.hypot(self.y);
        if !self.engaged {
            if reach >= ENGAGE {
                self.engaged = true;
                self.last_angle = self.y.atan2(self.x);
                self.turned = 0.0;
            }
            return 0;
        }
        if reach < RELEASE {
            self.engaged = false;
            self.turned = 0.0;
            return 0;
        }
        if reach < ENGAGE {
            return 0;
        }
        let angle = self.y.atan2(self.x);
        let mut delta = angle - self.last_angle;
        if delta > std::f32::consts::PI {
            delta -= std::f32::consts::TAU;
        } else if delta < -std::f32::consts::PI {
            delta += std::f32::consts::TAU;
        }
        self.last_angle = angle;
        if delta.abs() > MAX_JUMP {
            return 0;
        }
        self.turned += delta.to_degrees();
        let clicks = (self.turned / step).trunc();
        self.turned -= clicks * step;
        clicks as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Turns a fully pushed stick from `from` to `to` degrees in small moves,
    /// as the axis events arrive, and sums the clicks.
    fn turn(wheel: &mut Wheel, from: f32, to: f32) -> i32 {
        let steps = ((to - from).abs() / 3.0).ceil() as i32;
        let mut clicks = 0;
        for i in 0..=steps {
            let a = (from + (to - from) * i as f32 / steps.max(1) as f32).to_radians();
            clicks += wheel.axis(true, (a.cos() * 32767.0) as i16, DEFAULT_STEP);
            clicks += wheel.axis(false, (a.sin() * 32767.0) as i16, DEFAULT_STEP);
        }
        clicks
    }

    #[test]
    fn clicks_follow_the_turn_each_way() {
        let mut wheel = Wheel::default();
        // 97° clockwise: six clicks, 7° kept for the next one.
        assert_eq!(turn(&mut wheel, 0.0, 97.0), 6);
        // 117° back: 110° from the kept 7°, seven clicks up.
        assert_eq!(turn(&mut wheel, 97.0, -20.0), -7);
        // Across the ±180° seam the turn stays continuous.
        let mut wheel = Wheel::default();
        assert_eq!(turn(&mut wheel, 100.0, 280.0), 12);
    }

    #[test]
    fn small_moves_add_up_and_the_centre_lets_go() {
        let mut wheel = Wheel::default();
        assert_eq!(turn(&mut wheel, 0.0, 10.0), 0);
        assert_eq!(turn(&mut wheel, 10.0, 16.0), 1);
        // Back to the centre: the partial turn is forgotten.
        assert_eq!(wheel.axis(true, 0, DEFAULT_STEP), 0);
        assert_eq!(wheel.axis(false, 0, DEFAULT_STEP), 0);
        assert_eq!(turn(&mut wheel, 30.0, 44.0), 0);
    }

    #[test]
    fn a_flick_through_the_centre_is_not_a_rotation() {
        let mut wheel = Wheel::default();
        assert_eq!(turn(&mut wheel, 0.0, 0.0), 0);
        // Right to left in one reading pair: 180° at once.
        assert_eq!(wheel.axis(true, -32767, DEFAULT_STEP), 0);
        assert_eq!(wheel.axis(false, 0, DEFAULT_STEP), 0);
        assert_eq!(turn(&mut wheel, 180.0, 180.0), 0);
    }

    #[test]
    fn the_step_is_bounded_like_go() {
        assert_eq!(step_from(None), 15.0);
        assert_eq!(step_from(Some("30")), 30.0);
        assert_eq!(step_from(Some("4")), 15.0);
        assert_eq!(step_from(Some("91")), 15.0);
        assert_eq!(step_from(Some("x")), 15.0);
    }
}
