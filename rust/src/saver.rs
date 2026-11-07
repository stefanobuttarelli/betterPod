//! Go 1.1 screensaver and visualizer reference implementation in Rust.
use crate::{
    art::Artwork,
    input::Action,
    library::Library,
    render::{Color, Renderer, Texture, BLACK, WHITE},
};
use std::f64::consts::TAU;
const N: usize = 1024;
const BANDS: usize = 40;
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}
#[path = "saver_clock.rs"]
mod clock;
#[path = "saver_reference.rs"]
mod reference;
#[path = "saver_viz.rs"]
mod viz;
pub use reference::{
    effect_count, effect_name, palette_count, palette_name, Saver, SaverAction, SaverStatus,
};
pub struct Analysis {
    samples: [f32; N],
    window: [f64; N],
    reversed: [usize; N],
    twiddles: [Complex; N / 2],
    spectrum: [Complex; N],
    rate: u32,
    ranges: [(usize, usize); BANDS],
    norm: f64,
    pub bands: [f64; BANDS],
    pub peaks: [f64; BANDS],
    fall: [f64; BANDS],
    pub level: f64,
    pub bass: f64,
    bass_average: f64,
    pub beat: bool,
    beat_elapsed: f64,
    pub fresh: bool,
    hue: f64,
}
impl Default for Analysis {
    fn default() -> Self {
        Self::new()
    }
}
impl Analysis {
    pub fn new() -> Self {
        let mut out = Self {
            samples: [0.0; N],
            window: [0.0; N],
            reversed: [0; N],
            twiddles: [Complex::default(); N / 2],
            spectrum: [Complex::default(); N],
            rate: 0,
            ranges: [(0, 0); BANDS],
            norm: -60.0,
            bands: [0.0; BANDS],
            peaks: [0.0; BANDS],
            fall: [0.0; BANDS],
            level: 0.0,
            bass: 0.0,
            bass_average: 0.0,
            beat: false,
            beat_elapsed: 1.0,
            fresh: false,
            hue: 0.0,
        };
        for i in 0..N {
            out.window[i] = 0.5 - 0.5 * (TAU * i as f64 / (N - 1) as f64).cos();
            out.reversed[i] = i.reverse_bits() >> (usize::BITS - N.trailing_zeros());
        }
        for i in 0..N / 2 {
            let a = -TAU * i as f64 / N as f64;
            out.twiddles[i] = Complex {
                re: a.cos(),
                im: a.sin(),
            };
        }
        out.set_rate(44100);
        out
    }
    fn set_rate(&mut self, rate: u32) {
        self.rate = if rate > 0 { rate } else { 44100 };
        let lo = 40.0_f64;
        let hi = 16000.0_f64.min(self.rate as f64 * 0.475);
        let hz = self.rate as f64 / N as f64;
        let mut previous = 1;
        for b in 0..BANDS {
            let f0 = lo * (hi / lo).powf(b as f64 / BANDS as f64);
            let f1 = lo * (hi / lo).powf((b + 1) as f64 / BANDS as f64);
            let start = previous.max((f0 / hz) as usize).min(N / 2 - 1);
            let end = ((f1 / hz) as usize + 1).max(start + 1).min(N / 2);
            self.ranges[b] = (start, end);
            previous = end.min(N / 2 - 1);
        }
    }
    pub fn samples(&mut self, input: &[f32], rate: u32) {
        if rate != self.rate {
            self.set_rate(rate);
        }
        self.samples.fill(0.0);
        let count = input.len().min(N);
        let start = input.len() - count;
        for (to, &from) in self.samples[N - count..].iter_mut().zip(&input[start..]) {
            *to = if from.is_finite() {
                from.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
    }
    pub fn update(&mut self, dt: f64) {
        let dt = if dt.is_finite() {
            if dt > 0.2 {
                1. / 30.
            } else {
                dt.max(0.)
            }
        } else {
            0.0
        };
        let k = dt * 30.0;
        let mut power = 0.0;
        for i in 0..N {
            let x = self.samples[i] as f64;
            power += x * x;
            self.spectrum[self.reversed[i]] = Complex {
                re: x * self.window[i],
                im: 0.0,
            };
        }
        self.level = (power / N as f64).sqrt();
        self.fresh = self.level >= 1e-4;
        let mut size = 2;
        while size <= N {
            let half = size / 2;
            let step = N / size;
            for i in (0..N).step_by(size) {
                for j in 0..half {
                    let w = self.twiddles[j * step];
                    let p = self.spectrum[i + j + half];
                    let u = self.spectrum[i + j];
                    let tr = w.re * p.re - w.im * p.im;
                    let ti = w.re * p.im + w.im * p.re;
                    self.spectrum[i + j] = Complex {
                        re: u.re + tr,
                        im: u.im + ti,
                    };
                    self.spectrum[i + j + half] = Complex {
                        re: u.re - tr,
                        im: u.im - ti,
                    };
                }
            }
            size <<= 1;
        }
        let mut raw = [0.0; BANDS];
        let mut top = -200.0_f64;
        for (b, out) in raw.iter_mut().enumerate() {
            let (start, end) = self.ranges[b];
            let mut max = 0.0_f64;
            for bin in &self.spectrum[start..end] {
                max = max.max(bin.re * bin.re + bin.im * bin.im);
            }
            *out = 10.0 * (max + 1e-12).log10() + 9.0 * b as f64 / BANDS as f64;
            top = top.max(*out);
        }
        if top > self.norm {
            self.norm += (top - self.norm) * 0.5;
        } else {
            self.norm = (-60.0_f64).max(self.norm - (self.norm - top) * 0.12 * dt);
        }
        for b in 0..BANDS {
            let x = if self.fresh {
                (1.0 + (raw[b] - self.norm - 2.0) / 42.0).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if x > self.bands[b] {
                self.bands[b] += (x - self.bands[b]) * (0.65 * k).min(1.0);
                self.fall[b] = 0.0;
            } else {
                self.fall[b] += 0.004 * k;
                self.bands[b] = x.max(self.bands[b] - self.fall[b] * k);
            }
            self.peaks[b] = self.bands[b].max(self.peaks[b] - 0.012 * k);
        }
        self.bass = self.bands[..BANDS / 4].iter().sum::<f64>() / (BANDS / 4) as f64;
        self.beat_elapsed += dt;
        self.beat = self.fresh
            && self.bass > 0.3
            && self.bass > self.bass_average * 1.35
            && self.beat_elapsed > 0.22;
        if self.beat {
            self.beat_elapsed = 0.0;
        }
        self.bass_average += (self.bass - self.bass_average) * (0.08 * k).min(1.0);
        self.hue = (self.hue + (0.0025 + 0.01 * self.bass) * k).fract();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fft_places_tone_in_log_frequency_band_and_decays_silence() {
        let mut a = Analysis::new();
        let rate = 48000;
        let frequency = 3000.0;
        let input: Vec<_> = (0..N)
            .map(|i| ((TAU * frequency * i as f64 / rate as f64).sin() * 0.4) as f32)
            .collect();
        a.samples(&input, rate);
        for _ in 0..20 {
            a.update(1.0 / 60.0);
        }
        let strongest = a
            .bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        let center =
            (a.ranges[strongest].0 + a.ranges[strongest].1) as f64 * 0.5 * rate as f64 / N as f64;
        assert!((center - frequency).abs() < 600.0, "{center}");
        assert!(a.level > 0.2);
        assert!(a.bands[strongest] > 0.8);
        a.samples(&[], rate);
        for _ in 0..300 {
            a.update(1.0 / 60.0);
        }
        assert!(!a.fresh);
        assert!(a.peaks.iter().all(|v| *v < 0.001));
    }
    #[test]
    fn analysis_sanitizes_invalid_samples_and_supports_high_rate() {
        let mut a = Analysis::new();
        a.samples(&[f32::NAN, f32::INFINITY, 1.0], 192000);
        a.update(0.016);
        assert!(a
            .bands
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        assert!(a.ranges.iter().all(|&(lo, hi)| lo < hi && hi <= N / 2));
    }
}
