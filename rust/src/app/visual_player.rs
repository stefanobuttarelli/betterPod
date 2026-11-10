//! Player composition ported from ij_nowplaying.go, ij_npextra.go and
//! ij_coverflow.go. Values below retain the Go 1.1 CSS coordinates/timings;
//! only the final drawing coordinates are doubled for the physical panel.
use super::*;
use betterpod_rust::motion::{Anim, Ease};
use betterpod_rust::player_geometry as geometry;
use geometry::{now_playing_pose, Pose};
use std::collections::BTreeMap;
#[cfg(test)]
mod regression_qa;

struct PoseAnim {
    from: Pose,
    to: Pose,
    progress: Anim,
    value: Pose,
}
impl PoseAnim {
    fn new(p: Pose) -> Self {
        Self {
            from: p,
            to: p,
            progress: Anim::new(1.),
            value: p,
        }
    }
    fn set(&mut self, p: Pose, duration: f64, now: f64) {
        if self.to == p {
            return;
        }
        self.sample(now);
        self.from = self.value;
        self.to = p;
        self.progress.snap(0.);
        self.progress.set(1., duration, Ease::Css, 0., now);
    }
    fn sample(&mut self, now: f64) {
        self.value = self.from.lerp(self.to, self.progress.value(now));
    }
    #[cfg(test)]
    fn snap(&mut self) {
        self.from = self.to;
        self.value = self.to;
        self.progress.snap(1.);
    }
}
struct CoverVisual {
    pose: PoseAnim,
    opacity: Anim,
    alpha: f64,
}
impl CoverVisual {
    fn new(p: Pose) -> Self {
        Self {
            pose: PoseAnim::new(p),
            opacity: Anim::new(1.),
            alpha: 1.,
        }
    }
}

pub(super) struct PlayerVisual {
    time: f64,
    phase: f64,
    phase_epoch: Option<f64>,
    previous_screen: Option<Screen>,
    flow_context: bool,
    /// The Cover Flow screen being left is Now Playing's own track card
    /// (START there): coming back to Now Playing is not a song started from
    /// Cover Flow, so the flow scene stays off.
    leaving_np_card: bool,
    covers: BTreeMap<usize, CoverVisual>,
    flow_fade: Anim,
    flow_alpha: f64,
    flow_info: Anim,
    info_alpha: f64,
    flow_np: Anim,
    flow_np_alpha: f64,
    tilt: Anim,
    tilt_value: f64,
    flip: Anim,
    flip_value: f64,
    main_x: Anim,
    volume_x: Anim,
    scrub_x: Anim,
    options_x: Anim,
    controls: [f64; 4],
    volume_until: f64,
    mode: u8,
    options_at: f64,
    option: usize,
    change: Anim,
    change_value: f64,
    bars: Anim,
    bars_live: f64,
    current: Option<Track>,
    old: Option<Track>,
    since: f64,
    direction: f64,
    last_draw: f64,
    was_paused: bool,
    saver_untilt: bool,
    saver_hide: bool,
    card_album: Option<usize>,
    card_selected: usize,
    card_scroll: f64,
    card_since: f64,
    card_move_at: Option<f64>,
    card_marquee_since: f64,
    idle_followed: bool,
}
impl PlayerVisual {
    pub(super) fn new() -> Self {
        Self {
            time: 0.,
            phase: 0.,
            phase_epoch: None,
            previous_screen: None,
            flow_context: false,
            leaving_np_card: false,
            covers: BTreeMap::new(),
            flow_fade: Anim::new(0.),
            flow_alpha: 0.,
            flow_info: Anim::new(1.),
            info_alpha: 1.,
            flow_np: Anim::new(0.),
            flow_np_alpha: 0.,
            tilt: Anim::new(1.),
            tilt_value: 1.,
            flip: Anim::new(0.),
            flip_value: 0.,
            main_x: Anim::new(0.),
            volume_x: Anim::new(1.1),
            scrub_x: Anim::new(1.1),
            options_x: Anim::new(1.1),
            controls: [0., 1.1, 1.1, 1.1],
            volume_until: -1.,
            mode: 0,
            options_at: 0.,
            option: 0,
            change: Anim::new(1.),
            change_value: 1.,
            bars: Anim::new(0.),
            bars_live: 0.,
            current: None,
            old: None,
            since: 0.,
            direction: 1.,
            last_draw: -100.,
            was_paused: false,
            saver_untilt: false,
            saver_hide: false,
            card_album: None,
            card_selected: 0,
            card_scroll: 0.,
            card_since: 0.,
            card_move_at: None,
            card_marquee_since: 0.,
            idle_followed: false,
        }
    }
}
impl App {
    /// Offer player keys before the generic START/B navigation. Transport
    /// keys remain with the audio controller; this only owns the original
    /// player's wheel, centre button and shared card navigation.
    pub(super) fn visual_player_handle(&mut self, action: Action) -> bool {
        if self.options.is_some() || self.keyboard.is_some() || self.auto_saver {
            return false;
        }
        let screen = self.screen();
        if !matches!(screen, Screen::CoverFlow | Screen::NowPlaying) {
            return false;
        }
        if action == Action::SelectHeld || action == Action::Options {
            if screen == Screen::NowPlaying {
                self.open(Screen::Queue);
            } else if let Some(selected) = self.cover_card {
                if let Some(&index) = self
                    .library
                    .albums
                    .get(self.cover_selected)
                    .and_then(|a| a.track_indices.get(selected))
                {
                    self.visual_player_song_options(index);
                }
            } else {
                self.show_album_options(self.cover_selected);
            }
            return true;
        }
        if action == Action::Start {
            if screen == Screen::NowPlaying {
                self.np_mode = 0;
                self.flip_now_playing();
                return true;
            }
            if self.cover_from_np {
                self.cover_from_np = false;
                self.player_visual.leaving_np_card = true;
                self.cover_card = None;
                self.pop_navigation_page();
                return true;
            }
            return false;
        }
        if action == Action::Back {
            if screen == Screen::NowPlaying {
                if self.np_mode == 2 {
                    self.np_mode = 0;
                    self.player_visual.volume_until = -1.;
                    return true;
                }
                // Cover Flow owns its playing view and returns to its card.
                if self.player_visual.flow_context {
                    self.pop_navigation_page();
                    self.cover_card = Some(self.player_visual.card_selected);
                    return true;
                }
                return false;
            }
            if self.cover_card.is_some() {
                self.cover_card = None;
                if self.cover_from_np {
                    self.cover_from_np = false;
                    self.player_visual.leaving_np_card = true;
                    self.pop_navigation_page();
                } else {
                    self.player_visual.flow_context = false;
                }
                return true;
            }
            return false;
        }
        if screen == Screen::CoverFlow {
            if let Some(selected) = self.cover_card {
                let count = self
                    .library
                    .albums
                    .get(self.cover_selected)
                    .map_or(0, |a| a.track_indices.len());
                match action {
                    Action::Up => self.cover_card = Some(selected.saturating_sub(1)),
                    Action::Down => {
                        self.cover_card = Some((selected + 1).min(count.saturating_sub(1)))
                    }
                    Action::Left | Action::Right => {}
                    Action::Select => {
                        let index = self
                            .library
                            .albums
                            .get(self.cover_selected)
                            .and_then(|a| a.track_indices.get(selected))
                            .copied();
                        if let Some(index) = index {
                            if self.cover_from_np {
                                // Standalone NP card: return to the existing view,
                                // rather than stack another player on its card.
                                self.cover_from_np = false;
                                self.player_visual.leaving_np_card = true;
                                self.cover_card = None;
                                self.pop_navigation_page();
                                self.song(index);
                            } else {
                                self.song(index);
                                self.player_visual.flow_context = true;
                            }
                        }
                    }
                    _ => return false,
                }
            } else {
                match action {
                    Action::Up | Action::Left => {
                        self.cover_selected = self.cover_selected.saturating_sub(1)
                    }
                    Action::Down | Action::Right => {
                        self.cover_selected = (self.cover_selected + 1)
                            .min(self.library.albums.len().saturating_sub(1))
                    }
                    Action::Select => {
                        if !self.library.albums.is_empty() {
                            self.cover_card = Some(0);
                        }
                    }
                    _ => return false,
                }
            }
            return true;
        }
        match action {
            Action::Left | Action::Right => {
                self.np_mode = (self.np_mode + if action == Action::Right { 1 } else { 2 }) % 3;
                self.player_visual.volume_until = -1.;
                self.player_visual.options_at = self.visual_time;
                true
            }
            Action::Up | Action::Down => {
                if self.np_mode == 2 {
                    if action == Action::Up {
                        self.np_option = self.np_option.saturating_sub(1);
                    } else {
                        self.np_option = (self.np_option + 1).min(1);
                    }
                    self.player_visual.options_at = self.visual_time;
                } else if self.np_mode == 1 {
                    let positive = action == Action::Up;
                    let position = self.np_seek.map_or(self.audio_snapshot.position, |s| s.0);
                    let next = position + if positive { 1. } else { -1. };
                    let duration = self
                        .audio_snapshot
                        .duration
                        .max(self.now_track.as_ref().map_or(0., |t| t.duration));
                    if next >= 0. && (duration <= 0. || next <= duration) {
                        self.np_seek = Some((next, Instant::now() + Duration::from_millis(300)));
                    }
                } else {
                    let positive = action == Action::Up;
                    let target = (self.settings.volume + if positive { 0.04 } else { -0.04 })
                        .clamp(0.01, 1.);
                    self.set_volume(target - self.settings.volume);
                    // The row itself is the Go volume feedback on NP.
                    self.toast = None;
                }
                true
            }
            Action::Select => {
                self.player_visual.volume_until = -1.;
                if self.np_mode == 2 {
                    self.player_visual.options_at = self.visual_time;
                    self.execute(if self.np_option == 0 {
                        Intent::Shuffle
                    } else {
                        Intent::Repeat
                    });
                } else {
                    self.np_mode += 1;
                    self.player_visual.options_at = self.visual_time;
                }
                true
            }
            _ => false,
        }
    }
    pub(super) fn visual_player_song_options(&mut self, index: usize) {
        let Some(track) = self.library.tracks.get(index) else {
            return;
        };
        let title = track.title.clone();
        let mut rows = vec![
            Row::new("Play Next", Intent::AddNext(index)),
            Row::new("Add to End of Queue", Intent::AddLater(index)),
            Row::new("Add to Playlist…", Intent::AddToPlaylist(index)),
            Row::new("Share", Intent::ShareTrack(index)),
        ];
        rows[0].visual.queue_mark = "queue_next".into();
        rows[1].visual.queue_mark = "queue_last".into();
        rows.push(Row::new("Cancel", Intent::CloseOptions));
        self.options = Some(Page {
            kind: ViewKind::Full,
            mini: false,
            screen: Screen::CoverFlow,
            title,
            rows: Arc::new(rows),
            selected: 0,
            scroll: 0.,
            target_scroll: 0.,
            enter: 0.,
        });
    }
    pub(super) fn visual_player_track_direction(&mut self, direction: i32) {
        self.player_visual.direction = if direction < 0 { -1. } else { 1. };
    }
    pub(super) fn visual_player_volume_changed(&mut self, now: f64) {
        if self.np_mode == 0 {
            self.player_visual.volume_until = now + 3.;
        }
    }
    pub(super) fn visual_player_can_fly(&self) -> bool {
        !self.player_visual.flow_context
    }
    pub(super) fn visual_player_begin_saver(&mut self) -> f64 {
        let p = &mut self.player_visual;
        let tilt = p.tilt.value(self.visual_time);
        p.saver_untilt = true;
        p.saver_hide = false;
        p.tilt.set(0., 0.28, Ease::InOut, 0., self.visual_time);
        tilt
    }
    pub(super) fn visual_player_saver_hide(&mut self, hide: bool) {
        self.player_visual.saver_hide = hide;
    }
    pub(super) fn visual_player_end_saver(&mut self, landed_tilted: bool) {
        let p = &mut self.player_visual;
        p.saver_untilt = false;
        p.saver_hide = false;
        if landed_tilted {
            p.tilt.snap(1.);
            p.tilt_value = 1.;
        }
    }
    pub(super) fn visual_player_tick(&mut self, now: f64, _dt: f32) {
        let actual_screen = self.screen();
        let outgoing = if matches!(actual_screen, Screen::NowPlaying | Screen::CoverFlow) {
            None
        } else {
            self.visual_menu_outgoing_screen()
                .filter(|s| matches!(s, Screen::NowPlaying | Screen::CoverFlow))
        };
        let screen = outgoing.unwrap_or(actual_screen);
        let playing = self.music_visually_playing();
        if self.last_activity.elapsed() < Duration::from_secs(10) {
            self.player_visual.idle_followed = false;
        }
        let mut flow_album_changed = false;
        // A playing flow follows the queue, including changes made in Cover Flip.
        // Idle browsing returns to that album once, without reopening its card.
        if (screen == Screen::NowPlaying
            && (self.player_visual.flow_context
                || self.player_visual.previous_screen == Some(Screen::CoverFlow))
            && (self.player_visual.current.as_ref().map(|t| &t.path)
                != self.now_track.as_ref().map(|t| &t.path)
                || self.player_visual.previous_screen != Some(Screen::NowPlaying)))
            || (screen == Screen::CoverFlow
                && self.cover_card.is_none()
                && playing
                && (!self.player_visual.idle_followed
                    || self.player_visual.current.as_ref().map(|t| &t.path)
                        != self.now_track.as_ref().map(|t| &t.path))
                && self.last_activity.elapsed() >= Duration::from_secs(10))
        {
            self.player_visual.idle_followed = true;
            if let Some(index) = self.current_index() {
                if let Some(album) = self
                    .library
                    .albums
                    .iter()
                    .position(|a| a.track_indices.contains(&index))
                {
                    flow_album_changed = self.cover_selected != album;
                    self.cover_selected = album;
                    if screen == Screen::NowPlaying {
                        self.player_visual.card_album = Some(album);
                        self.player_visual.card_selected = self.library.albums[album]
                            .track_indices
                            .iter()
                            .position(|&i| i == index)
                            .unwrap_or(0);
                        if self.cover_card.is_some() {
                            self.cover_card = Some(self.player_visual.card_selected);
                        }
                    }
                }
            }
        }
        let pv = &mut self.player_visual;
        pv.time = now;
        pv.phase = pv.phase_epoch.map_or_else(
            || {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0., |d| d.as_secs_f64() % 600.)
            },
            |epoch| (epoch + now) % 600.,
        );
        let previous = pv.previous_screen;
        if screen == Screen::NowPlaying
            && previous == Some(Screen::CoverFlow)
            && !self.cover_from_np
            && !pv.leaving_np_card
        {
            pv.flow_context = true;
        }
        if screen != Screen::CoverFlow {
            pv.leaving_np_card = false;
        }
        if !matches!(screen, Screen::NowPlaying | Screen::CoverFlow) {
            pv.flow_context = false;
        }
        let visible = matches!(screen, Screen::NowPlaying | Screen::CoverFlow);
        let was_visible =
            previous.is_some_and(|s| matches!(s, Screen::NowPlaying | Screen::CoverFlow));
        let tilt = if playing && !pv.saver_untilt { 1. } else { 0. };
        if !was_visible
            || previous.is_none()
            || (screen == Screen::NowPlaying && previous == Some(Screen::CoverFlow))
        {
            pv.tilt.snap(tilt)
        } else {
            pv.tilt.set(tilt, 0.7, Ease::InOut, 0., now)
        }
        pv.tilt_value = pv.tilt.value(now);
        let flipped = screen == Screen::CoverFlow
            && self.cover_from_np
            && !pv.flow_context
            && self.cover_card.is_some();
        pv.flip
            .set(if flipped { 1. } else { 0. }, 0.5, Ease::Css, 0., now);
        pv.flip_value = pv.flip.value(now);
        let flow_visible =
            visible && (pv.flow_context || screen == Screen::CoverFlow && !self.cover_from_np);
        if flow_visible && outgoing.is_none() {
            pv.flow_fade.set(1., 0.3, Ease::Framer, 0., now)
        } else {
            pv.flow_fade.set(0., 0.3, Ease::InOut, 0., now)
        }
        pv.flow_alpha = pv.flow_fade.value(now);
        let flow_playing = pv.flow_context && screen == Screen::NowPlaying;
        pv.flow_np.set(
            if flow_playing { 1. } else { 0. },
            if flow_playing { 0.25 } else { 0.16 },
            if flow_playing {
                Ease::InOut
            } else {
                Ease::OutCubic
            },
            if flow_playing { 0.25 } else { 0. },
            now,
        );
        pv.flow_np_alpha = pv.flow_np.value(now);
        let selected = self.cover_card.is_some() || pv.flow_context;
        pv.flow_info
            .set(if selected { 0. } else { 1. }, 0.3, Ease::InOut, 0., now);
        pv.info_alpha = pv.flow_info.value(now);
        if flow_visible {
            let active = self.cover_selected;
            let first = active.saturating_sub(15);
            let end = (active + 16).min(self.library.albums.len());
            pv.covers.retain(|i, _| *i >= first && *i < end);
            for i in first..end {
                let (target, duration) = if i != active {
                    (geometry::side_pose(i as isize - active as isize), 0.25)
                } else if flow_playing {
                    let mut p = now_playing_pose(pv.tilt.target());
                    p.angle -= 360.;
                    (p, 0.5)
                } else if selected {
                    (Pose::BACK, 0.5)
                } else {
                    (Pose::FRONT, 0.3)
                };
                let c = pv
                    .covers
                    .entry(i)
                    .or_insert_with(|| CoverVisual::new(target));
                // Entry owns one flip. Steady pause/resume uses only the tilt
                // tween; a second pose tween would jump when it hands back.
                if !(i == active
                    && flow_playing
                    && previous == Some(Screen::NowPlaying)
                    && !flow_album_changed)
                {
                    c.pose.set(target, duration, now);
                }
                c.pose.sample(now);
                // Compose tilt with the entry flip, including pause/resume
                // during that flip. There is no discontinuous handoff.
                if i == active && flow_playing {
                    let mut p = now_playing_pose(pv.tilt_value);
                    p.angle -= 360.;
                    c.pose.to = p;
                    c.pose.sample(now);
                }
                let hidden = i != active && (flow_playing || self.cover_from_np);
                c.opacity
                    .set(if hidden { 0. } else { 1. }, 0.35, Ease::Css, 0., now);
                c.alpha = c.opacity.value(now);
            }
        }
        if pv.current.as_ref().map(|t| (&t.path, &t.cover))
            != self.now_track.as_ref().map(|t| (&t.path, &t.cover))
        {
            if pv.current.is_some() && now - pv.last_draw < 0.2 {
                pv.old = pv.current.take();
                pv.change.snap(0.);
                pv.change.set(1., 0.42, Ease::Linear, 0., now);
            } else {
                pv.old = None;
                pv.change.snap(1.);
            }
            pv.current = self.now_track.clone();
            pv.since = now;
        }
        pv.change_value = pv.change.value(now);
        if pv.was_paused && playing {
            pv.since = now;
        }
        pv.was_paused = !playing;
        pv.bars
            .set(if playing { 1. } else { 0. }, 0.45, Ease::InOut, 0., now);
        pv.bars_live = pv.bars.value(now);
        if pv.mode != self.np_mode || pv.option != self.np_option {
            pv.options_at = now;
            pv.mode = self.np_mode;
            pv.option = self.np_option;
        }
        if self.np_mode == 2 && now - pv.options_at > 8. {
            self.np_mode = 0;
            pv.mode = 0;
        }
        let scrub = self.np_mode == 1;
        let opts = self.np_mode == 2;
        let volume = now < pv.volume_until;
        let targets = control_targets(scrub, opts, volume);
        for (anim, target) in [
            (&mut pv.main_x, targets[0]),
            (&mut pv.volume_x, targets[1]),
            (&mut pv.scrub_x, targets[2]),
            (&mut pv.options_x, targets[3]),
        ] {
            anim.set(target, 0.3, Ease::Css, 0., now);
        }
        pv.controls = [
            pv.main_x.value(now),
            pv.volume_x.value(now),
            pv.scrub_x.value(now),
            pv.options_x.value(now),
        ];
        let card = if self.cover_card.is_some() {
            Some(self.cover_selected)
        } else {
            None
        };
        if card.is_some() && card != pv.card_album {
            pv.card_album = card;
            pv.card_since = now;
            pv.card_scroll = 0.;
            pv.card_marquee_since = now;
            pv.card_selected = self.cover_card.unwrap_or(0);
        }
        if let Some(selected) = self.cover_card {
            if selected != pv.card_selected {
                pv.card_selected = selected;
                pv.card_move_at = Some(now);
                pv.card_marquee_since = now;
            }
        }
        let list_h = geometry::backside_height() - geometry::info_height() - 2.;
        if pv.card_selected as f64 * 25. < pv.card_scroll {
            pv.card_scroll = pv.card_selected as f64 * 25.;
        }
        if (pv.card_selected + 1) as f64 * 25. > pv.card_scroll + list_h {
            pv.card_scroll = (pv.card_selected + 1) as f64 * 25. - list_h;
        }
        pv.card_scroll = pv.card_scroll.max(0.);
        pv.previous_screen = Some(screen);
    }
    /// Deterministic endpoint for the local Go/Rust reference capture harness.
    #[cfg(test)]
    pub(super) fn visual_player_set_phase_epoch(&mut self, epoch: f64) {
        self.player_visual.phase_epoch = Some(epoch);
        self.player_visual.phase = (epoch + self.visual_time) % 600.;
    }
    #[cfg(test)]
    pub(super) fn visual_player_snap_state(&mut self, now: f64) {
        self.visual_player_tick(now, 0.);
        let p = &mut self.player_visual;
        for a in [
            &mut p.flow_fade,
            &mut p.flow_info,
            &mut p.flow_np,
            &mut p.tilt,
            &mut p.flip,
            &mut p.main_x,
            &mut p.volume_x,
            &mut p.scrub_x,
            &mut p.options_x,
            &mut p.change,
            &mut p.bars,
        ] {
            let target = a.target();
            a.snap(target);
        }
        for c in p.covers.values_mut() {
            c.pose.snap();
            let target = c.opacity.target();
            c.opacity.snap(target);
            c.alpha = target;
        }
        p.flow_alpha = p.flow_fade.target();
        p.info_alpha = p.flow_info.target();
        p.flow_np_alpha = p.flow_np.target();
        p.tilt_value = p.tilt.target();
        p.flip_value = p.flip.target();
        p.change_value = p.change.target();
        p.bars_live = p.bars.target();
        p.controls = [
            p.main_x.target(),
            p.volume_x.target(),
            p.scrub_x.target(),
            p.options_x.target(),
        ];
    }
    pub(super) fn visual_player_draw_coverflow(&mut self, r: &mut Renderer) {
        if self.cover_from_np && !self.player_visual.flow_context {
            self.visual_player_draw_now_playing(r);
            return;
        }
        self.visual_player_draw_flow(r);
    }
    fn visual_player_draw_flow(&mut self, r: &mut Renderer) {
        let dark = self.dark();
        let theme = PlayerTheme::new(dark);
        let pv = &self.player_visual;
        r.push_transform(0., 0., 1., 1., pv.flow_alpha as f32);
        r.push_clip(0., 40., 640., 440.);
        let active = self.cover_selected;
        if let Some(album) = self.library.albums.get(active) {
            if pv.info_alpha > 0.002 {
                let (th, ta) = crate::render::line_box(32.);
                let (ah, aa) = crate::render::line_box(26.);
                let y = 40. + 408. - th - ah;
                center_text(
                    r,
                    &album.name,
                    0.,
                    y + ta,
                    640.,
                    32.,
                    700,
                    opacity(theme.text, pv.info_alpha),
                    576.,
                );
                center_text(
                    r,
                    &album.artist,
                    0.,
                    y + th + aa,
                    640.,
                    26.,
                    600,
                    opacity(theme.sub, pv.info_alpha),
                    576.,
                );
            }
        }
        // Keep the three-request budget, but load the visible center first.
        let first = active.saturating_sub(15);
        let end = (active + 16).min(self.library.albums.len());
        let mut asked = 0;
        let prioritized = std::iter::once(active).chain((1..=15).flat_map(|d| {
            [
                active.checked_sub(d),
                active.checked_add(d).filter(|i| *i < end),
            ]
            .into_iter()
            .flatten()
        }));
        for i in prioritized.filter(|i| *i >= first && *i < end) {
            if asked >= 3 {
                break;
            }
            if pv.covers.get(&i).is_some_and(|c| c.alpha > 0.001) {
                let path = self.library.albums[i].cover.as_deref();
                if let Some(path) = path {
                    if self.art.missing(Some(path)) {
                        self.art.get(Some(path));
                        asked += 1;
                    }
                }
            }
        }
        for distance in (0..=15).rev() {
            for sign in [-1isize, 1] {
                if distance == 0 && sign == 1 {
                    continue;
                }
                let index = active as isize + distance * sign;
                if index < 0 {
                    continue;
                }
                let i = index as usize;
                let Some(c) = pv.covers.get(&i) else { continue };
                if c.alpha <= 0.001 {
                    continue;
                }
                let Some(album) = self.library.albums.get(i) else {
                    continue;
                };
                let p = c.pose.value;
                let on_back = i == active && pv.card_album == Some(i) && !geometry::front(p);
                if on_back {
                    draw_card(
                        r,
                        p,
                        i,
                        pv.card_selected,
                        pv,
                        &self.library,
                        &self.views_album_tracks,
                        &self.audio_snapshot,
                        dark,
                        c.alpha,
                    );
                } else {
                    let tex = self.art.peek(album.cover.as_deref());
                    let selected = i == active && (self.cover_card.is_some() || pv.flow_context);
                    draw_cover(
                        r,
                        p,
                        tex,
                        theme.back,
                        c.alpha,
                        if selected {
                            0.2 * pv.flow_np_alpha
                        } else {
                            0.2
                        },
                    );
                }
            }
        }
        if pv.flow_context && pv.flow_np_alpha > 0.002 {
            draw_np_body(
                r,
                pv,
                &self.visual_assets,
                &self.settings,
                &self.audio_snapshot,
                self.np_seek.as_ref().map(|s| s.0),
                dark,
                pv.flow_np_alpha,
            );
            self.player_visual.last_draw = self.player_visual.time;
        }
        r.pop_clip();
        r.pop_transform();
    }
    pub(super) fn visual_player_draw_now_playing(&mut self, r: &mut Renderer) {
        if self.player_visual.flow_context {
            self.visual_player_draw_flow(r);
            return;
        }
        let dark = self.dark();
        let theme = PlayerTheme::new(dark);
        let pv = &self.player_visual;
        r.push_transform(0., 0., 1., 1., 1.);
        r.push_clip(0., 40., 640., 440.);
        if let Some(track) = pv.current.as_ref() {
            let mut base = now_playing_pose(pv.tilt_value);
            base.angle -= 360.;
            let p = base.lerp(Pose::BACK, pv.flip_value);
            if pv.saver_hide {
                // Its cover is the full-strength flying cover above the saver.
            } else if pv.card_album.is_some() && !geometry::front(p) {
                draw_card(
                    r,
                    p,
                    pv.card_album.unwrap(),
                    pv.card_selected,
                    pv,
                    &self.library,
                    &self.views_album_tracks,
                    &self.audio_snapshot,
                    dark,
                    1.,
                );
            } else {
                let texture = self.art.get(track.cover.as_deref());
                if let Some(old) = pv
                    .old
                    .as_ref()
                    .filter(|old| old.cover != track.cover && pv.change_value < 1.)
                {
                    let old_texture = self.art.get(old.cover.as_deref());
                    draw_cover(
                        r,
                        p,
                        old_texture,
                        theme.back,
                        1.,
                        0.2 * (1. - pv.flip_value),
                    );
                    draw_cover(
                        r,
                        p,
                        texture,
                        theme.back,
                        pv.change_value,
                        0.2 * (1. - pv.flip_value),
                    );
                } else {
                    draw_cover(r, p, texture, theme.back, 1., 0.2 * (1. - pv.flip_value));
                }
            }
            let alpha = (1. - 2. * pv.flip_value).max(0.);
            if alpha > 0.002 {
                draw_np_body(
                    r,
                    pv,
                    &self.visual_assets,
                    &self.settings,
                    &self.audio_snapshot,
                    self.np_seek.as_ref().map(|s| s.0),
                    dark,
                    alpha,
                );
            }
        }
        self.player_visual.last_draw = self.player_visual.time;
        r.pop_clip();
        r.pop_transform();
    }
}

#[derive(Clone, Copy)]
struct PlayerTheme {
    bg: Color,
    text: Color,
    sub: Color,
    np_sub: Color,
    stripe: Color,
    border: Color,
    back: Color,
}
impl PlayerTheme {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: rgb(22, 23, 25),
                text: rgb(238, 239, 241),
                sub: rgb(150, 153, 158),
                np_sub: rgb(155, 158, 163),
                stripe: rgb(30, 31, 34),
                border: rgb(60, 62, 66),
                back: rgb(46, 48, 52),
            }
        } else {
            Self {
                bg: WHITE,
                text: FG,
                sub: rgb(100, 100, 100),
                np_sub: rgb(99, 101, 103),
                stripe: rgb(242, 243, 245),
                border: rgb(211, 211, 211),
                back: rgb(233, 234, 236),
            }
        }
    }
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255., g as f32 / 255., b as f32 / 255., 1.]
}
fn control_targets(scrub: bool, options: bool, volume: bool) -> [f64; 4] {
    [
        if scrub || options || volume { -1.1 } else { 0. },
        if volume && !scrub && !options {
            0.
        } else {
            1.1
        },
        if scrub {
            0.
        } else if options {
            -1.1
        } else {
            1.1
        },
        if options { 0. } else { 1.1 },
    ]
}
fn opacity(mut c: Color, a: f64) -> Color {
    c[3] *= a as f32;
    c
}
fn draw_cover(
    r: &mut Renderer,
    p: Pose,
    texture: Texture,
    back: Color,
    alpha: f64,
    reflection: f64,
) {
    if geometry::front(p) {
        r.projected_image(
            texture,
            geometry::quad(p, 0., 0., 128., 128.),
            [0., 0., 1., 1.],
            [opacity(WHITE, alpha); 4],
        );
        if reflection > 0.001 {
            r.projected_image(
                texture,
                geometry::quad(p, 0., 128., 128., 166.4),
                [0., 1., 1., 0.7],
                [
                    opacity(WHITE, alpha * reflection),
                    opacity(WHITE, alpha * reflection),
                    opacity(WHITE, 0.),
                    opacity(WHITE, 0.),
                ],
            );
        }
    } else {
        let h = geometry::plane(geometry::matrix(p));
        let transform = [
            [h[0] as f32, h[1] as f32, (2. * h[2]) as f32],
            [
                (h[3] + 20. * h[6]) as f32,
                (h[4] + 20. * h[7]) as f32,
                (2. * h[5] + 40. * h[8]) as f32,
            ],
            [(h[6] / 2.) as f32, (h[7] / 2.) as f32, h[8] as f32],
        ];
        r.push_projective(transform, alpha as f32);
        r.rect(0., 0., 256., 256., back);
        r.pop_transform();
    }
}
fn ellipsis(r: &mut Renderer, text: &str, size: f32, weight: u16, width: f32) -> String {
    if r.text_width_weighted(text, size, weight) <= width {
        return text.to_owned();
    }
    let ew = r.text_width_weighted("…", size, weight);
    if ew > width {
        return String::new();
    }
    let mut result = String::new();
    for ch in text.chars() {
        result.push(ch);
        if r.text_width_weighted(&result, size, weight) + ew > width {
            result.pop();
            break;
        }
    }
    result.push('…');
    result
}
fn center_text(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    width: f32,
    size: f32,
    weight: u16,
    color: Color,
    max: f32,
) {
    let text = ellipsis(r, text, size, weight, max);
    let tw = r.text_width_weighted(&text, size, weight);
    r.text_baseline(
        &text,
        x + (width - tw) * 0.5,
        baseline,
        size,
        weight,
        color,
        max,
    );
}
fn marquee_cycle(r: &mut Renderer, text: &str, size: f32, weight: u16, width: f32) -> f64 {
    let tw = r.text_width_weighted(text, size, weight);
    if tw <= width {
        0.
    } else {
        ((tw + 80.) / 64.) as f64 + 1.5
    }
}
fn draw_marquee(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    weight: u16,
    width: f32,
    color: Color,
    since: f64,
    now: f64,
    cycle: f64,
    live: bool,
) {
    let tw = r.text_width_weighted(text, size, weight);
    if tw <= width || !live {
        let text = ellipsis(r, text, size, weight, width);
        r.text_baseline(&text, x, baseline, size, weight, color, width);
        return;
    }
    let own = marquee_cycle(r, text, size, weight, width);
    let cycle = cycle.max(own);
    let t = (now - since).max(0.) % cycle;
    let offset = if t <= 1.5 {
        0.
    } else {
        ((t - 1.5) * 64.).min((tw + 80.) as f64)
    } as f32;
    r.push_clip(x, baseline - size * 1.3, width, size * 1.7);
    r.text_baseline(text, x - offset, baseline, size, weight, color, tw + 2.);
    r.text_baseline(
        text,
        x - offset + tw + 80.,
        baseline,
        size,
        weight,
        color,
        tw + 2.,
    );
    r.pop_clip();
}
fn draw_np_body(
    r: &mut Renderer,
    pv: &PlayerVisual,
    assets: &VisualAssets,
    settings: &Settings,
    snapshot: &AudioSnapshot,
    seek: Option<f64>,
    dark: bool,
    alpha: f64,
) {
    let Some(track) = pv.current.as_ref() else {
        return;
    };
    let theme = PlayerTheme::new(dark);
    r.push_transform(0., 0., 1., 1., alpha as f32);
    if pv.change_value < 1. {
        if let Some(old) = pv.old.as_ref() {
            let out = Ease::InOut.sample((pv.change_value / 0.45).min(1.));
            let enter = Ease::InOut.sample(((pv.change_value - 0.45) / 0.55).max(0.));
            draw_info(
                r,
                old,
                pv,
                -44. * pv.direction * out,
                1. - out,
                false,
                theme,
            );
            draw_info(
                r,
                track,
                pv,
                44. * pv.direction * (1. - enter),
                enter,
                false,
                theme,
            );
        }
    } else {
        draw_info(
            r,
            track,
            pv,
            0.,
            1.,
            snapshot.playing && !snapshot.paused,
            theme,
        );
    }
    let y = 398.;
    let width = 608.;
    let [main, volume, scrub, options] = pv.controls.map(|v| v as f32 * width);
    if volume < width {
        let x = volume + 16.;
        sprite(r, assets, "volume_mute_bar", dark, x + 24., y, 1.);
        progress(
            r,
            assets,
            x + 100.,
            y,
            408.,
            settings.volume as f64,
            false,
            dark,
        );
        sprite(r, assets, "volume_full_bar", dark, x + 524., y, 1.);
    }
    if main > -width {
        let x = main + 16.;
        draw_time(r, &playback_time(snapshot.position), x + 24., y, theme.text);
        let remaining = if snapshot.duration > 0. {
            format!(
                "-{}",
                playback_time((snapshot.duration - snapshot.position).max(0.))
            )
        } else {
            "--:--".into()
        };
        draw_time(r, &remaining, x + 504., y, theme.text);
        progress(
            r,
            assets,
            x + 120.,
            y,
            368.,
            if snapshot.duration > 0. {
                snapshot.position / snapshot.duration
            } else {
                0.
            },
            false,
            dark,
        );
    }
    if scrub < width && scrub > -width {
        let x = scrub + 16.;
        let current = seek.unwrap_or(snapshot.position);
        draw_time(r, &playback_time(current), x + 24., y, theme.text);
        draw_time(
            r,
            &format!("-{}", playback_time(snapshot.duration - current)),
            x + 504.,
            y,
            theme.text,
        );
        let pct = if snapshot.duration > 0. {
            (current / snapshot.duration * 100.).round() / 100.
        } else {
            0.
        };
        progress(r, assets, x + 116., y, 376., pct, true, dark);
    }
    if options < width {
        draw_options(r, pv, assets, settings, options + 16., y, dark);
    }
    r.pop_transform();
}
fn draw_info(
    r: &mut Renderer,
    track: &Track,
    pv: &PlayerVisual,
    dx: f64,
    alpha: f64,
    live: bool,
    theme: PlayerTheme,
) {
    if alpha <= 0.002 {
        return;
    }
    let mut lines = Vec::with_capacity(3);
    for (text, size, color) in [
        (&track.title, 29.44, theme.text),
        (&track.artist, 24., theme.np_sub),
        (&track.album, 24., theme.np_sub),
    ] {
        if !text.is_empty() {
            lines.push((text.as_str(), size, color, crate::render::line_box(size)));
        }
    }
    let h = lines.iter().map(|l| l.3 .0).sum::<f32>() + if track.lossless { 38. } else { 0. };
    let mut y = 40. + (308. - h) * 0.5;
    let cycle = lines
        .iter()
        .map(|l| marquee_cycle(r, l.0, l.1, 600, 288.))
        .fold(0., f64::max);
    r.push_clip(288., 40., 384., 308.);
    for (text, size, color, (lh, asc)) in lines {
        draw_marquee(
            r,
            text,
            336. + dx as f32,
            y + asc,
            size,
            600,
            288.,
            opacity(color, alpha),
            pv.since,
            pv.time,
            cycle,
            live,
        );
        y += lh;
    }
    if track.lossless {
        draw_lossless(
            r,
            336. + dx as f32,
            y + 14.,
            pv.phase,
            pv.bars_live,
            opacity(theme.np_sub, alpha),
        );
    }
    r.pop_clip();
}
fn draw_lossless(r: &mut Renderer, x: f32, y: f32, now: f64, live: f64, color: Color) {
    let base = [0.42, 0.72, 1., 0.72, 0.42];
    let frequencies = [[2.3, 5.1], [3.1, 1.7], [1.9, 4.3], [2.7, 6.1], [3.6, 2.2]];
    let phases = [[0., 1.3], [2.1, 0.4], [4., 2.8], [1.2, 5.], [3.3, 0.9]];
    let mut bx = x;
    for i in 0..5 {
        let m = 0.5
            + 0.32 * (now * frequencies[i][0] + phases[i][0]).sin()
            + 0.18 * (now * frequencies[i][1] + phases[i][1]).sin();
        let mv = 0.5 * base[i] + 0.5 * (0.25 + 0.75 * m);
        let height = (24. * (base[i] * (1. - live) + mv * live)).max(4.4) as f32;
        r.rounded_rect(bx, y + 12. - height * 0.5, 4.4, height, 2.2, color);
        bx += 7.8;
    }
    let mut tx = bx + 7.;
    for ch in "LOSSLESS".chars() {
        let text = ch.to_string();
        r.text_baseline(&text, tx, y + 12. + 18. * 0.727 * 0.5, 18., 700, color, 30.);
        tx += r.text_width_weighted(&text, 18., 700) + 1.8;
    }
}
fn playback_time(seconds: f64) -> String {
    let seconds = (seconds.max(0.) as u64) % 3600;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
fn draw_time(r: &mut Renderer, text: &str, x: f32, y: f32, color: Color) {
    let (_, asc) = crate::render::line_box(24.);
    r.text_baseline(text, x, y + 1. + asc, 24., 700, color, 100.);
}
fn sprite(
    r: &mut Renderer,
    assets: &VisualAssets,
    name: &str,
    dark: bool,
    x: f32,
    y: f32,
    alpha: f64,
) {
    let texture = if dark {
        assets
            .get(&format!("{name}__dark"))
            .or_else(|| assets.get(name))
    } else {
        assets.get(name)
    };
    if let Some(t) = texture {
        r.image_uv(
            t,
            [x, y, t.width as f32, t.height as f32],
            [0., 0., 1., 1.],
            alpha as f32,
        );
    }
}
pub(super) fn progress(
    r: &mut Renderer,
    assets: &VisualAssets,
    x: f32,
    y: f32,
    width: f32,
    fraction: f64,
    scrub: bool,
    dark: bool,
) {
    let fraction = fraction.clamp(0., 1.) as f32;
    let track = if dark {
        assets
            .get("gloss_track__dark")
            .or_else(|| assets.get("gloss_track"))
    } else {
        assets.get("gloss_track")
    };
    let mut parts = Vec::with_capacity(3);
    if let Some(t) = track {
        parts.push((
            t,
            x,
            width.min(t.width as f32),
            [0., 0., width.min(t.width as f32) / t.width as f32, 1.],
        ));
    }
    if scrub {
        if let Some(t) = assets.get("scrubber") {
            parts.push((
                t,
                x + width * fraction - 16.,
                t.width as f32,
                [0., 0., 1., 1.],
            ));
        }
    } else if let Some(t) = assets.get("gloss_blue") {
        // Whole pixels, as Go draws it: a bar that grows by a fraction of a
        // pixel every frame looks the same but forces a full redraw.
        let w = (width * fraction).round().min(t.width as f32);
        if w > 0. {
            parts.push((t, x, w, [0., 0., w / t.width as f32, 1.]));
        }
    }
    for &(t, px, w, uv) in &parts {
        r.image_uv(t, [px, y, w, t.height as f32], uv, 1.);
    }
    // The original reads 32 rows of the completed bar and mirrors them;
    // drawing identical one-pixel UV slices preserves its half-pixel fade.
    for row in 0..24 {
        let alpha = (1. - (row as f32 + 0.5) / 24.) * 0.35;
        for &(t, px, w, uv) in &parts {
            let source = 31 - row;
            r.image_uv(
                t,
                [px, y + 32. + row as f32, w, 1.],
                [uv[0], source as f32 / 32., uv[2], (source + 1) as f32 / 32.],
                alpha,
            );
        }
    }
}
fn draw_options(
    r: &mut Renderer,
    pv: &PlayerVisual,
    assets: &VisualAssets,
    settings: &Settings,
    x: f32,
    y: f32,
    dark: bool,
) {
    let theme = PlayerTheme::new(dark);
    let (_, asc) = crate::render::line_box(24.);
    for i in 0..2 {
        let bx = x + 24. + i as f32 * 288.;
        let selected = i == pv.option;
        let (icon, label, value) = if i == 0 {
            (
                "hdr_shuffle",
                "Shuffle",
                if !settings.shuffle {
                    "Off"
                } else if settings.shuffle_mode == "albums" {
                    "Albums"
                } else {
                    "Songs"
                },
            )
        } else {
            (
                if settings.repeat == "one" {
                    "hdr_repeat_one"
                } else {
                    "hdr_repeat"
                },
                "Repeat",
                match settings.repeat.as_str() {
                    "one" => "One",
                    "all" => "All",
                    _ => "Off",
                },
            )
        };
        let top = y - 10.;
        let h = 52.;
        let stops = if selected {
            vec![(0., rgb(60, 184, 255)), (1., rgb(52, 122, 181))]
        } else if dark {
            vec![
                (0., rgb(85, 88, 95)),
                (0.4844, rgb(52, 55, 61)),
                (1., rgb(60, 63, 69)),
            ]
        } else {
            vec![
                (0., WHITE),
                (0.4844, rgb(203, 204, 206)),
                (1., rgb(223, 223, 223)),
            ]
        };
        rounded_gradient(r, bx, top, 272., h, 12., &stops);
        if !selected {
            // Go's 1 px outline on the unselected button (ij_npextra.go:
            // sheetBorder at 35 %), 2 px on the doubled panel
            let border = if dark {
                rgb(0x15, 0x17, 0x1a)
            } else {
                rgb(0x3e, 0x42, 0x49)
            };
            r.stroke_rounded(bx, top, 272., h, 12., 2., opacity(border, 0.35));
        }
        let texture = if selected {
            assets.get(&format!("{icon}__white"))
        } else if dark {
            assets.get(&format!("{icon}__dark"))
        } else {
            assets.get(icon)
        }
        .or_else(|| assets.get(icon));
        let iw = texture.map_or(32., |t| t.width as f32);
        if let Some(t) = texture {
            r.image_uv(
                t,
                [
                    bx + 18.,
                    top + h * 0.5 - t.height as f32 * 0.5,
                    t.width as f32,
                    t.height as f32,
                ],
                [0., 0., 1., 1.],
                if value == "Off" { 0.45 } else { 1. },
            );
        }
        let tx = bx + 18. + iw + 12.;
        let baseline = top + h * 0.5 + asc * 0.5 - 1.;
        let tc = if selected { WHITE } else { theme.text };
        let vc = if selected {
            opacity(WHITE, 0.8)
        } else {
            theme.sub
        };
        r.text_baseline(label, tx, baseline, 24., 700, tc, 200.);
        let lw = r.text_width_weighted(label, 24., 700);
        let vw = r.text_width_weighted(value, 24., 500);
        let vx = (tx + lw + 12.).max(bx + 272. - 18. - vw);
        r.text_baseline(value, vx, baseline, 24., 500, vc, (bx + 254. - vx).max(0.));
    }
}
fn rounded_gradient(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    stops: &[(f32, Color)],
) {
    r.rounded_gradient(x, y, w, h, radius, stops);
}
fn gradient_color(stops: &[(f32, Color)], p: f32) -> Color {
    for pair in stops.windows(2) {
        if p <= pair[1].0 {
            let k = ((p - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0., 1.);
            return std::array::from_fn(|i| pair[0].1[i] + (pair[1].1[i] - pair[0].1[i]) * k);
        }
    }
    stops.last().unwrap().1
}
fn draw_card(
    r: &mut Renderer,
    p: Pose,
    album_index: usize,
    selected: usize,
    pv: &PlayerVisual,
    library: &Library,
    _rows: &[Arc<Vec<Row>>],
    snapshot: &AudioSnapshot,
    dark: bool,
    alpha: f64,
) {
    let Some(album) = library.albums.get(album_index) else {
        return;
    };
    let theme = PlayerTheme::new(dark);
    let width = 512.;
    let height = geometry::backside_height() as f32 * 2.;
    let info = geometry::info_height() as f32 * 2.;
    r.push_projective(geometry::card_homography(p), alpha as f32);
    r.rect(0., 0., width, height, theme.bg);
    r.gradient(
        2.,
        2.,
        508.,
        info - 2.,
        rgb(101, 133, 173),
        rgb(120, 154, 179),
    );
    r.rect(2., info, 508., 2., rgb(109, 135, 163));
    let (title_h, title_asc) = crate::render::line_box(32.);
    let (_, artist_asc) = crate::render::line_box(28.);
    let cycle = marquee_cycle(r, &album.name, 32., 700, 476.).max(marquee_cycle(
        r,
        &album.artist,
        28.,
        700,
        476.,
    ));
    draw_marquee(
        r,
        &album.name,
        18.,
        10. + title_asc,
        32.,
        700,
        476.,
        WHITE,
        pv.card_since,
        pv.time,
        cycle,
        true,
    );
    draw_marquee(
        r,
        &album.artist,
        18.,
        10. + title_h + artist_asc,
        28.,
        700,
        476.,
        WHITE,
        pv.card_since,
        pv.time,
        cycle,
        true,
    );
    let list_y = info + 2.;
    let list_h = height - list_y - 2.;
    let scrollable = album.track_indices.len() as f32 * 50. > list_h;
    r.push_transform(2., list_y, 1., 1., 1.);
    r.push_clip(0., 0., 508., list_h);
    let (lh, asc) = crate::render::line_box(28.);
    let first = (pv.card_scroll / 25.).floor().max(0.) as usize;
    for n in first..album.track_indices.len() {
        let y = n as f32 * 50. - pv.card_scroll as f32 * 2.;
        if y > list_h {
            break;
        }
        let Some(track) = library.tracks.get(album.track_indices[n]) else {
            continue;
        };
        let chosen = n == selected;
        let color = if chosen { WHITE } else { theme.text };
        if chosen {
            r.gradient(0., y, 508., 50., rgb(60, 184, 255), rgb(52, 122, 181));
        } else if n % 2 == 1 {
            r.rect(0., y, 508., 50., theme.stripe);
        }
        let duration = if track.duration > 0. {
            track_length(track.duration)
        } else {
            String::new()
        };
        let dw = if duration.is_empty() {
            0.
        } else {
            r.text_width_weighted(&duration, 28., 600) + 20.
        };
        let current = snapshot.path.as_ref() == Some(&track.path);
        let tx = 8. + if current { 28. } else { 0. };
        let right = 508. - if scrollable { 18. } else { 0. } - dw;
        let text_width = (right - tx - if dw > 0. { 0. } else { 32. } - 16.).max(0.);
        let ty = y + (50. - (16. + lh)) * 0.5;
        let baseline = ty + 8. + asc;
        draw_marquee(
            r,
            &track.title,
            tx + 8.,
            baseline,
            28.,
            700,
            text_width,
            color,
            pv.card_marquee_since,
            pv.time,
            0.,
            chosen,
        );
        if current {
            let cy = baseline - 28. * 0.727 * 0.5;
            let x = 18.;
            if snapshot.paused {
                r.rect(x, cy - 9., 5.2, 18., color);
                r.rect(x + 10., cy - 9., 5.2, 18., color);
            } else {
                r.triangle([[x, cy - 9.], [x, cy + 9.], [x + 15., cy]], color);
            }
        }
        if !duration.is_empty() {
            let tw = r.text_width_weighted(&duration, 28., 600);
            r.text_baseline(
                &duration,
                right + dw - 16. - tw,
                baseline,
                28.,
                600,
                color,
                tw + 1.,
            );
        }
    }
    if scrollable {
        let a = pv.card_move_at.map_or(0., |at| {
            let d = pv.time - at;
            if d <= 0.9 {
                1.
            } else if d >= 1.25 {
                0.
            } else {
                1. - Ease::Css.sample((d - 0.9) / 0.35)
            }
        });
        if a > 0. {
            draw_scrollbar(
                r,
                490.,
                0.,
                18.,
                list_h,
                pv.card_scroll as f32 * 2.,
                album.track_indices.len() as f32 * 50.,
                a,
                dark,
            );
        }
    }
    r.pop_clip();
    r.pop_transform();
    for rect in [
        [0., 0., width, 2.],
        [0., height - 2., width, 2.],
        [0., 0., 2., height],
        [width - 2., 0., 2., height],
    ] {
        r.rect(rect[0], rect[1], rect[2], rect[3], theme.border);
    }
    r.pop_transform();
}
fn track_length(seconds: f64) -> String {
    let seconds = (seconds.max(0.) + 0.5) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
fn draw_scrollbar(
    r: &mut Renderer,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scroll: f32,
    total: f32,
    alpha: f64,
    dark: bool,
) {
    let bw = 12.;
    let x = crate::render::scrollbar_track_x(x, w);
    let y0 = y + 6.;
    let y1 = y + h - 6.;
    let border = if dark {
        rgb(69, 72, 78)
    } else {
        rgb(180, 180, 180)
    };
    let track = if dark {
        [rgb(38, 40, 44), rgb(48, 50, 55)]
    } else {
        [rgb(228, 228, 228), rgb(247, 247, 247)]
    };
    rounded_gradient(
        r,
        x,
        y0,
        bw,
        y1 - y0,
        6.,
        &[
            (0., opacity(track[0], alpha)),
            (1., opacity(track[1], alpha)),
        ],
    );
    // Original stroke is one physical pixel, inside the rounded track.
    r.rounded_rect(x, y0, bw, y1 - y0, 6., opacity(border, alpha));
    rounded_gradient(
        r,
        x + 1.,
        y0 + 1.,
        bw - 2.,
        y1 - y0 - 2.,
        5.,
        &[
            (0., opacity(track[0], alpha)),
            (1., opacity(track[1], alpha)),
        ],
    );
    let th = ((y1 - y0) * h / total).max(28.);
    let fraction = if total > h {
        (scroll / (total - h)).clamp(0., 1.)
    } else {
        0.
    };
    let ty = y0 + (y1 - y0 - th) * fraction;
    let colors = if dark {
        [rgb(163, 166, 172), rgb(134, 137, 144), rgb(114, 117, 123)]
    } else {
        [rgb(168, 168, 168), rgb(138, 138, 138), rgb(118, 118, 118)]
    };
    let tx = x + 1.;
    let tw = 10.;
    let radius = 5.;
    for col in 0..10 {
        let xx = col as f32;
        let dx = (xx + 0.5 - radius).abs();
        let inset = radius - (radius * radius - dx * dx).max(0.).sqrt();
        r.rect(
            tx + xx,
            ty + inset,
            1.,
            th - 2. * inset,
            opacity(
                gradient_color(
                    &[(0., colors[0]), (0.5, colors[1]), (1., colors[2])],
                    (xx + 0.5) / tw,
                ),
                alpha,
            ),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_control_rows_leave_on_the_original_sides() {
        assert_eq!(control_targets(false, false, false), [0., 1.1, 1.1, 1.1]);
        assert_eq!(control_targets(false, false, true), [-1.1, 0., 1.1, 1.1]);
        assert_eq!(control_targets(true, false, true), [-1.1, 1.1, 0., 1.1]);
        assert_eq!(control_targets(false, true, false), [-1.1, 1.1, -1.1, 0.]);
    }
    #[test]
    fn interrupted_cover_pose_starts_at_the_visible_position() {
        let mut pose = PoseAnim::new(Pose::FRONT);
        pose.set(Pose::BACK, 0.5, 1.);
        pose.sample(1.2);
        let visible = pose.value;
        pose.set(Pose::FRONT, 0.3, 1.2);
        assert_eq!(pose.from, visible);
        pose.sample(1.5);
        assert_eq!(pose.value, Pose::FRONT);
    }
    #[test]
    fn source_time_formats_keep_distinct_list_and_playback_rules() {
        assert_eq!(track_length(59.6), "1:00");
        assert_eq!(track_length(3600.), "1:00:00");
        assert_eq!(playback_time(3599.9), "59:59");
        assert_eq!(playback_time(3600.), "00:00");
        assert_eq!(playback_time(-2.), "00:00");
    }
}
