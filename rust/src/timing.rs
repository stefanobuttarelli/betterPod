use std::path::Path;
use std::time::{Duration, Instant};

pub struct FrameClock {
    deadline: Instant,
    period: Duration,
}
impl FrameClock {
    pub fn new(hz: f64) -> Self {
        let period = Duration::from_secs_f64(1.0 / hz.clamp(1.0, 240.0));
        Self {
            deadline: Instant::now() + period,
            period,
        }
    }
    pub fn wait(&mut self, now: Instant) {
        // Never spin: the audio worker must retain CPU headroom.
        if now < self.deadline {
            std::thread::sleep(self.deadline - now);
        }
        let after = Instant::now();
        self.deadline += self.period;
        if after > self.deadline + self.period {
            self.deadline = after + self.period;
        }
    }
    pub fn reset(&mut self) {
        self.deadline = Instant::now() + self.period;
    }
}
#[derive(Clone, Copy)]
pub struct FrameSample {
    pub phase: &'static str,
    pub elapsed: f64,
    pub work_ms: f64,
    pub present_ms: f64,
    pub gap_ms: f64,
    pub playing: bool,
    pub position: f64,
    pub audio: crate::audio::AudioTelemetry,
    pub video: crate::video::VideoTelemetry,
}
pub struct Metrics {
    pub samples: Vec<FrameSample>,
}
impl Metrics {
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(120_000),
        }
    }
    pub fn push(&mut self, sample: FrameSample) {
        if self.samples.len() < self.samples.capacity() {
            self.samples.push(sample);
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), String> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut file = std::io::BufWriter::new(file);
        writeln!(
            file,
            "phase,elapsed_s,work_ms,present_ms,completion_gap_ms,playing,position_s,audio_callbacks,audio_interval_ms,audio_max_interval_ms,audio_rate,audio_chunk_frames,pcm_underruns,video_playing,video_loading,video_failed,video_position_s,video_frames,video_frame_pts_s,video_underruns,video_generation"
        )
        .map_err(|e| e.to_string())?;
        for s in &self.samples {
            writeln!(
                file,
                "{},{:.6},{:.6},{:.6},{:.6},{},{:.6},{},{:.6},{:.6},{},{},{},{},{},{},{:.6},{},{:.6},{},{}",
                s.phase,
                s.elapsed,
                s.work_ms,
                s.present_ms,
                s.gap_ms,
                s.playing,
                s.position,
                s.audio.callbacks,
                s.audio.last_interval_ms,
                s.audio.max_interval_ms,
                s.audio.rate,
                s.audio.chunk_frames,
                s.audio.underruns,s.video.playing,s.video.loading,s.video.failed,s.video.position,
                s.video.frames,s.video.frame_pts,s.video.underruns,s.video.generation
            )
            .map_err(|e| e.to_string())?;
        }
        file.flush().map_err(|e| e.to_string())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_period_is_bounded() {
        let c = FrameClock::new(60.0);
        assert!((c.period.as_secs_f64() - 1.0 / 60.0).abs() < 1e-8);
        assert_eq!(FrameClock::new(0.0).period, Duration::from_secs(1));
    }
}
