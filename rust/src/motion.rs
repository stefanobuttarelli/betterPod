//! The Go 1.1 transition clock and CSS timing functions, in seconds.
#[derive(Clone, Copy, Debug)]
pub enum Ease {
    Css,
    InOut,
    Framer,
    Linear,
    OutCubic,
}
impl Ease {
    pub fn sample(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Self::Linear => x,
            Self::OutCubic => 1.0 - (1.0 - x).powi(3),
            Self::Css => bezier(x, 0.25, 0.1, 0.25, 1.0),
            Self::InOut => bezier(x, 0.42, 0.0, 0.58, 1.0),
            Self::Framer => bezier(x, 0.25, 0.1, 0.35, 1.0),
        }
    }
}
fn bezier(x: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    if x <= 0.0 || x >= 1.0 {
        return x;
    }
    let b = |t: f64, p1: f64, p2: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    let db = |t: f64, p1: f64, p2: f64| {
        let u = 1.0 - t;
        3.0 * u * u * p1 + 6.0 * u * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
    };
    let mut t = x;
    for _ in 0..8 {
        let d = db(t, x1, x2);
        if d.abs() < 1e-6 {
            break;
        }
        t = (t - (b(t, x1, x2) - x) / d).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..20 {
        if (b(t, x1, x2) - x).abs() <= 1e-5 {
            break;
        }
        if b(t, x1, x2) < x {
            lo = t
        } else {
            hi = t
        }
        t = (lo + hi) * 0.5;
    }
    b(t, y1, y2)
}
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    pub from: f64,
    pub to: f64,
    pub start: f64,
    pub duration: f64,
    pub ease: Ease,
}
impl Tween {
    pub fn value(&self, now: f64) -> (f64, bool) {
        if self.duration <= 0.0 {
            return (self.to, true);
        }
        let p = (now - self.start) / self.duration;
        if p <= 0.0 {
            return (self.from, false);
        }
        if p >= 1.0 {
            return (self.to, true);
        }
        (
            self.from + (self.to - self.from) * self.ease.sample(p),
            false,
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    tween: Option<Tween>,
    resting: f64,
}
impl Anim {
    pub fn new(value: f64) -> Self {
        Self {
            tween: None,
            resting: value,
        }
    }
    pub fn value(&mut self, now: f64) -> f64 {
        if let Some(tween) = self.tween {
            let (value, done) = tween.value(now);
            self.resting = value;
            if done {
                self.tween = None;
            }
            value
        } else {
            self.resting
        }
    }
    pub fn snap(&mut self, value: f64) {
        self.resting = value;
        self.tween = None;
    }
    pub fn target(&self) -> f64 {
        self.tween.map_or(self.resting, |t| t.to)
    }
    pub fn running(&self, now: f64) -> bool {
        self.tween.is_some_and(|t| !t.value(now).1)
    }
    pub fn set(&mut self, target: f64, duration: f64, ease: Ease, delay: f64, now: f64) {
        let current = self.value(now);
        if self.target() == target {
            return;
        }
        self.tween = Some(Tween {
            from: current,
            to: target,
            start: now + delay,
            duration,
            ease,
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delayed_retarget_samples_current_pose() {
        let mut a = Anim::new(0.0);
        a.set(1.0, 0.3, Ease::InOut, 0.05, 0.0);
        assert_eq!(a.value(0.025), 0.0);
        assert!((a.value(0.2) - 0.5).abs() < 1e-5);
        a.set(0.0, 0.3, Ease::Css, 0.0, 0.2);
        assert!((a.value(0.2) - 0.5).abs() < 1e-5);
        assert_eq!(a.value(0.5), 0.0);
        assert!(!a.running(0.5));
    }
    #[test]
    fn unchanged_target_does_not_restart() {
        let mut a = Anim::new(0.0);
        a.set(1.0, 0.3, Ease::Css, 0.0, 0.0);
        a.set(1.0, 0.3, Ease::Css, 0.0, 0.2);
        assert_eq!(a.value(0.3), 1.0);
    }
    #[test]
    fn reference_curves_reach_exact_endpoints() {
        for e in [
            Ease::Css,
            Ease::InOut,
            Ease::Framer,
            Ease::Linear,
            Ease::OutCubic,
        ] {
            assert_eq!(e.sample(0.0), 0.0);
            assert_eq!(e.sample(1.0), 1.0);
        }
        assert!((Ease::Css.sample(0.5) - 0.8024).abs() < 0.0001);
    }
}
