//! The original Find Missing Covers ready/job/results view, ij_covers.go.
use super::*;
use betterpod_rust::{
    curation::CoverSearchProgress,
    online::{Event, JobHandle, Output},
    render::{line_box, Color},
    visual_assets::VisualAssets,
};
pub(super) enum Response {
    Start(Vec<CoverQuery>),
    Back,
}
pub(super) struct Covers {
    albums: Option<Vec<CoverQuery>>,
    state: CoverSearchProgress,
    handle: Option<JobHandle>,
    started: bool,
    shown: f64,
    last: Option<f64>,
}
impl Covers {
    #[cfg(test)]
    pub fn visual_fixture(albums: Vec<CoverQuery>, key: &str) -> Self {
        let mut view = Self::new(albums);
        let n = view.state.total;
        if !key.ends_with("ready") {
            view.started = true;
            view.state.index = 1.min(n.saturating_sub(1));
            view.state.found = usize::from(n > 1);
            if let Some(album) = view.albums.as_ref().and_then(|a| a.get(view.state.index)) {
                view.state.current = album.album.clone();
                view.state.artist = album.artist.clone();
            }
            if key.ends_with("done") {
                view.state.finished = true;
                view.state.index = n;
                view.state.current.clear();
                view.state.artist.clear();
                view.state.found = usize::from(n > 0);
                view.state.missed = view
                    .albums
                    .as_ref()
                    .unwrap()
                    .iter()
                    .skip(view.state.found)
                    .map(|a| a.album.clone())
                    .collect();
                view.state.failed = view.state.missed.len();
            }
            view.shown = view.target();
        }
        view
    }
    pub fn new(albums: Vec<CoverQuery>) -> Self {
        Self {
            state: CoverSearchProgress {
                total: albums.len(),
                ..Default::default()
            },
            albums: Some(albums),
            handle: None,
            started: false,
            shown: 0.,
            last: None,
        }
    }
    pub fn running(&self) -> bool {
        self.started && !self.state.finished
    }
    pub fn started(&mut self, handle: JobHandle) {
        self.started = true;
        self.handle = Some(handle);
    }
    pub fn failed(&mut self, message: String) {
        self.started = true;
        self.state.finished = true;
        self.state.errors += 1;
        self.state.error = message;
    }
    pub fn cancel(&self) {
        if let Some(handle) = &self.handle {
            handle.cancel();
        }
    }
    pub fn event(&mut self, event: Event) -> bool {
        let Some(handle) = &self.handle else {
            return false;
        };
        match event {
            Event::Progress { id, status, .. } if id == handle.id => {
                if let Some(state) = CoverSearchProgress::from_status(&status) {
                    self.state = state;
                }
            }
            Event::Finished { id, result } if id == handle.id => {
                match result {
                    Ok(Output::CurationApplied { matched, total }) => {
                        self.state.found = matched;
                        self.state.total = total;
                    }
                    Err(message) => {
                        if handle.is_cancelled() {
                            self.state.cancelled = true;
                        } else {
                            self.state.errors += 1;
                            self.state.error = message;
                        }
                    }
                    _ => {}
                }
                self.state.finished = true;
                self.state.current.clear();
                self.state.artist.clear();
                self.state.index = self.state.total;
                return self.state.found > 0;
            }
            _ => {}
        }
        false
    }
    pub fn handle(&mut self, action: Action) -> Option<Response> {
        if matches!(action, Action::Back | Action::MenuDown) {
            return Some(Response::Back);
        }
        if action != Action::Select {
            return None;
        }
        if !self.started && self.state.total > 0 {
            self.started = true;
            return self.albums.take().map(Response::Start);
        }
        if self.running() {
            self.cancel();
            None
        } else {
            Some(Response::Back)
        }
    }
    fn target(&self) -> f64 {
        if self.state.finished {
            1.
        } else if self.started && self.state.total > 0 {
            self.state.index as f64 / self.state.total as f64
        } else {
            0.
        }
    }
    pub fn draw(&mut self, r: &mut Renderer, assets: &VisualAssets, dark: bool, now: f64) {
        let dt = self.last.map_or(0.5, |last| (now - last).max(0.).min(0.5));
        self.last = Some(now);
        let target = self.target();
        if target < self.shown {
            self.shown = target
        } else {
            self.shown += (target - self.shown) * (dt * 8.).min(1.);
        }
        let bg = if dark { rgb(22, 23, 25) } else { [1.; 4] };
        let text = if dark {
            rgb(238, 239, 241)
        } else {
            rgb(0, 0, 0)
        };
        let sub = if dark {
            rgb(150, 153, 158)
        } else {
            rgb(100, 100, 100)
        };
        r.rect(0., 40., 640., 440., bg);
        let (title, line1, line2, button) = self.labels();
        center(r, &title, 100. + line_box(32.).1, 32., 700, text);
        if !line1.is_empty() {
            center(r, &line1, 144. + line_box(26.).1, 26., 500, sub);
        }
        if !line2.is_empty() {
            center(r, &line2, 184. + line_box(24.).1, 24., 500, sub);
        }
        super::super::app_visual_player::progress(
            r, assets, 80., 272., 480., self.shown, false, dark,
        );
        if self.started {
            center(
                r,
                &format!("{}%", (self.shown * 100. + 0.5) as u32),
                332. + line_box(24.).1,
                24.,
                700,
                sub,
            );
        }
        let primary = button != "Cancel";
        let gradient = if primary {
            vec![(0., rgb(60, 184, 255)), (1., rgb(52, 122, 181))]
        } else if dark {
            vec![
                (0., rgb(85, 88, 95)),
                (0.4844, rgb(52, 55, 61)),
                (1., rgb(60, 63, 69)),
            ]
        } else {
            vec![
                (0., [1.; 4]),
                (0.4844, rgb(203, 204, 206)),
                (1., rgb(223, 223, 223)),
            ]
        };
        r.rounded_gradient(80., 408., 480., 56., 18., &gradient);
        r.stroke_rounded(
            80.,
            408.,
            480.,
            56.,
            18.,
            2.,
            if primary {
                rgb(58, 120, 173)
            } else if dark {
                rgb(21, 23, 26)
            } else {
                rgb(62, 66, 73)
            },
        );
        center(
            r,
            button,
            436. + 28. * 0.727 * 0.5,
            28.,
            700,
            if primary { [1.; 4] } else { text },
        );
    }
    fn labels(&self) -> (String, String, String, &'static str) {
        let s = &self.state;
        let mut title = "Find Missing Covers".into();
        let (mut line1, mut line2, mut button) = (String::new(), String::new(), "Find Covers");
        if !self.started {
            line1 = format!(
                "{} {} without a cover",
                s.total,
                if s.total == 1 { "album" } else { "albums" }
            );
            line2 = "From MusicBrainz, iTunes, Deezer, Wikipedia · needs Wi-Fi".into();
            if s.total == 0 {
                line1 = "Every album has a cover".into();
                button = "Done";
            }
        } else if s.finished {
            if s.total == 0 {
                line1 = "Every album has a cover".into();
            } else {
                title = format!("Found {} of {}", s.found, s.total);
                line1 = format!("{} not found", s.missed.len());
                if s.cancelled {
                    line1 = format!("Stopped · {line1}");
                }
                if s.errors > 0 && s.missed.is_empty() {
                    line1 = format!("{} could not be looked up", s.errors);
                    line2 = s.error.clone();
                } else if s.errors > 0 {
                    line1 = format!(
                        "{} not found · {} could not be looked up ({})",
                        s.missed.len(),
                        s.errors,
                        s.error
                    );
                    line2 = format!("Not found: {}", s.missed.join(", "));
                } else if !s.missed.is_empty() {
                    line2 = format!("Not found: {}", s.missed.join(", "));
                }
            }
            button = "Done";
        } else {
            if !s.current.is_empty() {
                title = s.current.clone();
                line1 = s.artist.clone();
            }
            line2 = format!(
                "Album {} of {} · {} found · {} not found",
                (s.index + 1).min(s.total),
                s.total,
                s.found,
                s.failed
            );
            button = "Cancel";
        }
        (title, line1, line2, button)
    }
}
impl Drop for Covers {
    fn drop(&mut self) {
        if self.running() {
            self.cancel();
        }
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn center(r: &mut Renderer, text: &str, baseline: f32, size: f32, weight: u16, color: Color) {
    let chars = text.chars().take(4096).collect::<Vec<_>>();
    let max = 560.;
    let mut label = text.chars().take(4096).collect::<String>();
    if r.text_width_weighted(&label, size, weight) > max {
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            let candidate = format!("{}…", chars[..mid].iter().collect::<String>());
            if r.text_width_weighted(&candidate, size, weight) <= max {
                lo = mid
            } else {
                hi = mid - 1
            }
        }
        label = format!("{}…", chars[..lo].iter().collect::<String>().trim_end());
    }
    let width = r.text_width_weighted(&label, size, weight);
    r.text_baseline(
        &label,
        (640. - width) * 0.5,
        baseline,
        size,
        weight,
        color,
        max,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    fn queries() -> Vec<CoverQuery> {
        vec![
            CoverQuery {
                paths: vec!["/music/one.flac".into()],
                album: "One".into(),
                artist: "Artist".into(),
                ..Default::default()
            },
            CoverQuery {
                paths: vec!["/music/two.flac".into()],
                album: "Two".into(),
                artist: "Artist".into(),
                ..Default::default()
            },
        ]
    }
    #[test]
    fn ready_view_needs_explicit_start_and_back_does_not_cancel() {
        let mut v = Covers::new(queries());
        assert_eq!(v.labels().3, "Find Covers");
        assert!(matches!(v.handle(Action::Select),Some(Response::Start(albums))if albums.len()==2));
        assert!(v.running());
        assert!(matches!(v.handle(Action::Back), Some(Response::Back)));
        assert!(v.running());
    }
    #[test]
    fn original_running_and_partial_results_text_keep_not_found_separate_from_lookup_errors() {
        let mut v = Covers::new(queries());
        v.started = true;
        v.state.current = "One".into();
        v.state.artist = "Artist".into();
        assert_eq!(
            v.labels(),
            (
                "One".into(),
                "Artist".into(),
                "Album 1 of 2 · 0 found · 0 not found".into(),
                "Cancel"
            )
        );
        v.state.finished = true;
        v.state.found = 1;
        v.state.missed = vec!["Two".into()];
        assert_eq!(
            v.labels(),
            (
                "Found 1 of 2".into(),
                "1 not found".into(),
                "Not found: Two".into(),
                "Done"
            )
        );
        v.state.cancelled = true;
        assert_eq!(v.labels().1, "Stopped · 1 not found");
    }
    #[test]
    fn background_back_keeps_real_job_handle_alive_and_cancel_button_cancels_it() {
        let root = std::env::temp_dir().join(format!(
            "betterpod-cover-view-cancel-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("music")).unwrap();
        let mut services =
            betterpod_rust::online::Services::new_with_music(root.join("data"), root.join("music"))
                .unwrap();
        // An empty worker task cannot make any network request. Its real
        // cancellation token still verifies the view's controller binding.
        let handle = services
            .submit(Task::FindMissingCovers { albums: Vec::new() })
            .unwrap();
        let mut v = Covers::new(queries());
        v.started(handle.clone());
        assert!(matches!(v.handle(Action::Back), Some(Response::Back)));
        assert!(!handle.is_cancelled());
        assert!(v.handle(Action::Select).is_none());
        assert!(handle.is_cancelled());
        v.event(Event::Finished {
            id: handle.id,
            result: Err("Cancelled".into()),
        });
        assert_eq!(v.labels().1, "Stopped · 0 not found");
        assert_eq!(v.labels().3, "Done");
        services.shutdown();
        std::fs::remove_dir_all(root).unwrap();
    }
}
