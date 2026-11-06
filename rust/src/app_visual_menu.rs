//! Go 1.1 menu composition, in physical 640 × 480 coordinates.
//! The original Go geometry and timings are the specification. All artwork
//! lookups are RAM-only; immutable sprites are uploaded before the UI starts.
use super::*;

const MINI_HEIGHT: f32 = 116.0;
const HEADER_HEIGHT: f32 = 40.0;
const SCROLLBAR_WIDTH: f32 = 18.0;
const MAX_LAYOUTS: usize = 12;
const MAX_LAYOUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewKind {
    #[default]
    Auto,
    None,
    Music,
    Apps,
    Settings,
    NowPlaying,
    Icon {
        asset: String,
        title: String,
        subtitle: String,
    },
    Image {
        asset: String,
        title: String,
        subtitle: String,
        size: u16,
    },
    Weather,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RowVisual {
    pub thumbnail: bool,
    pub value: String,
    pub checked: bool,
    pub icon: String,
    pub header: bool,
    pub no_arrow: bool,
    pub duration: String,
    pub queue_mark: String,
    pub progress: f32,
    pub preview: PreviewKind,
    pub search_field: bool,
    pub placeholder: String,
    pub typing: bool,
}

/// Set by a background platform watcher. Drawing never polls hardware.
#[derive(Clone, Copy, Debug)]
pub(super) struct HeaderStatus {
    pub battery: Option<u8>,
    pub charging: bool,
    pub external_power: bool,
    pub bluetooth: bool,
    pub headphones: bool,
    /// -1 is disconnected; otherwise 0..=5, as Go's wifiLevel.
    pub wifi: i8,
}
impl Default for HeaderStatus {
    fn default() -> Self {
        Self {
            battery: None,
            charging: false,
            external_power: false,
            bluetooth: false,
            headphones: false,
            wifi: -1,
        }
    }
}

use betterpod_rust::motion::{Anim, Ease as Curve};

#[derive(Clone, Copy)]
struct Tween {
    anim: Anim,
    target: f32,
}
impl Tween {
    fn new(value: f32) -> Self {
        Self {
            anim: Anim::new(value as f64),
            target: value,
        }
    }
    fn value(&self, now: f64) -> f32 {
        let mut anim = self.anim;
        anim.value(now) as f32
    }
    fn to(&mut self, target: f32, duration: f64, curve: Curve, delay: f64, now: f64) {
        self.anim.set(target as f64, duration, curve, delay, now);
        self.target = target;
    }
    fn snap(&mut self, value: f32) {
        self.anim.snap(value as f64);
        self.target = value;
    }
}
pub(super) fn go_ease_css(t: f32) -> f32 {
    Curve::Css.sample(t as f64) as f32
}
pub(super) fn go_ease_in_out(t: f32) -> f32 {
    Curve::InOut.sample(t as f64) as f32
}

/// Go lineBox, returned in physical pixels (height, ascent).
pub(super) fn line_box(size: f32) -> (f32, f32) {
    let css = size * 0.5;
    (
        (css * 0.96875).round() * 2.0 + (css * 0.24121094).round() * 2.0,
        (size * 0.96875).round() - if size == 28.0 { 1.0 } else { 0.0 },
    )
}
pub(super) fn row_height(row: &Row) -> f32 {
    if row.visual.search_field {
        return 92.0;
    }
    if row.visual.header {
        return 44.0;
    }
    if row.visual.progress > 0.0 {
        return 136.0;
    }
    let mut height = 16.0 + line_box(28.0).0;
    if !row.detail.is_empty() {
        height += line_box(24.0).0;
    }
    if row.visual.thumbnail || row.cover.is_some() || !row.visual.icon.is_empty() {
        height = height.max(96.0);
    }
    height
}
pub(super) fn row_tops(rows: &[Row]) -> Vec<f32> {
    let mut tops = Vec::with_capacity(rows.len() + 1);
    tops.push(0.0);
    for row in rows {
        tops.push(tops.last().copied().unwrap_or(0.0) + row_height(row));
    }
    tops
}

struct Layout {
    rows: std::sync::Weak<Vec<Row>>,
    tops: Vec<f32>,
}
struct Navigation {
    previous: Page,
    forward: bool,
    started: f64,
    cross_panel: bool,
    previous_id: u64,
}
#[derive(Clone)]
struct ViewMotion {
    id: u64,
    depth: usize,
    page: Page,
    enter: Anim,
    hide: Anim,
    exiting: bool,
}
fn anim_value(anim: &Anim, now: f64) -> f32 {
    let mut copy = *anim;
    copy.value(now) as f32
}
struct CoverCycle {
    order: Vec<usize>,
    starts: HashMap<usize, f64>,
    last_cycle: f64,
    transition: Option<f64>,
    generation: u64,
    seeded: bool,
}
impl Default for CoverCycle {
    fn default() -> Self {
        Self {
            order: Vec::new(),
            starts: HashMap::new(),
            last_cycle: 0.0,
            transition: None,
            generation: u64::MAX,
            seeded: false,
        }
    }
}

pub(super) struct MenuVisual {
    pub status: HeaderStatus,
    pub volume_osd: Anim,
    pub volume_until: f64,
    pub brightness_osd: Anim,
    pub brightness_until: f64,
    pub brightness_level: i32,
    navigation: Option<Navigation>,
    views: Vec<ViewMotion>,
    active_view: Option<u64>,
    next_view: u64,
    left_panel: Anim,
    full_panel: Anim,
    preview_hide: Anim,
    layouts: Vec<Layout>,
    selection: Option<(Screen, usize)>,
    selection_since: f64,
    preview_pending: PreviewKind,
    preview_pending_since: f64,
    preview: PreviewKind,
    weather_kind: usize,
    music_preview: Tween,
    np_pan_start: f64,
    np_pan_shown: f64,
    cycle: CoverCycle,
    mini_show: [Tween; 2],
    mini_pause: Tween,
    mini_change: Tween,
    mini_track: Option<Track>,
    mini_old: Option<Track>,
    mini_since: f64,
    mini_paused_since: Option<f64>,
    mini_direction: f32,
    last_queue_index: usize,
    last_drawn: f64,
    ticker_track: Option<Track>,
    ticker_next: Option<Track>,
    ticker_started: Option<f64>,
    ticker_last_path: Option<PathBuf>,
    ticker_last_playing: bool,
    ask_on: bool,
    ask: Anim,
    ask_selected: usize,
    ask_until: f64,
    ask_reset: Option<f64>,
}
impl MenuVisual {
    pub fn new() -> Self {
        Self {
            status: HeaderStatus::default(),
            volume_osd: Anim::new(0.0),
            volume_until: f64::NEG_INFINITY,
            brightness_osd: Anim::new(0.0),
            brightness_until: f64::NEG_INFINITY,
            brightness_level: 100,
            navigation: None,
            views: Vec::new(),
            active_view: None,
            next_view: 1,
            left_panel: Anim::new(0.0),
            full_panel: Anim::new(1.0),
            preview_hide: Anim::new(0.0),
            layouts: Vec::new(),
            selection: None,
            selection_since: 0.0,
            preview_pending: PreviewKind::Music,
            preview_pending_since: 0.0,
            preview: PreviewKind::Music,
            weather_kind: 2,
            music_preview: Tween::new(0.0),
            np_pan_start: f64::NEG_INFINITY,
            np_pan_shown: f64::NEG_INFINITY,
            cycle: CoverCycle::default(),
            mini_show: [Tween::new(0.0), Tween::new(0.0)],
            mini_pause: Tween::new(0.0),
            mini_change: Tween::new(1.0),
            mini_track: None,
            mini_old: None,
            mini_since: 0.0,
            mini_paused_since: None,
            mini_direction: 1.0,
            last_queue_index: 0,
            last_drawn: f64::NEG_INFINITY,
            ticker_track: None,
            ticker_next: None,
            ticker_started: None,
            ticker_last_path: None,
            ticker_last_playing: false,
            ask_on: false,
            ask: Anim::new(0.0),
            ask_selected: 0,
            ask_until: f64::NEG_INFINITY,
            ask_reset: None,
        }
    }
    pub fn tick(&mut self, now: f64) {
        let active = self.active_view;
        let outgoing = self.navigation.as_ref().map(|nav| nav.previous_id);
        self.views.retain(|view| {
            Some(view.id) == active
                || Some(view.id) == outgoing
                || !view.exiting
                || view.enter.running(now)
        });
        if self.ask_on && now >= self.ask_until {
            self.ask_on = false;
            self.ask.set(0.0, 0.320, Curve::Linear, 0.0, now);
        }
        if self.ask_reset.is_some_and(|at| now >= at) {
            self.ask_reset = None;
            self.ask.snap(0.0);
        }
        if now > self.volume_until && self.volume_osd.target() == 1.0 {
            self.volume_osd.set(0.0, 0.3, Curve::Css, 0.0, now);
        }
        if now > self.brightness_until && self.brightness_osd.target() == 1.0 {
            self.brightness_osd.set(0.0, 0.3, Curve::Css, 0.0, now);
        }
        if self
            .navigation
            .as_ref()
            .is_some_and(|n| now - n.started >= if n.cross_panel { 0.4 } else { 0.3 })
        {
            self.navigation = None;
        }
    }
    pub fn seed_preview_order(&mut self, order: Vec<usize>, started: f64) {
        self.cycle.order = order;
        self.cycle.starts.clear();
        if let Some(&first) = self.cycle.order.first() {
            self.cycle.starts.insert(first, started);
        }
        self.cycle.last_cycle = started;
        self.cycle.transition = None;
        self.cycle.seeded = true;
    }
    fn layout_index(&mut self, rows: &Arc<Vec<Row>>) -> usize {
        // Height caching must not own old search results or retired libraries.
        // Weak identity also changes when Arc::make_mut modifies shared rows.
        self.layouts.retain(|layout| layout.rows.strong_count() > 0);
        let key = Arc::downgrade(rows);
        if let Some(index) = self
            .layouts
            .iter()
            .position(|layout| std::sync::Weak::ptr_eq(&layout.rows, &key))
        {
            return index;
        }
        let tops = row_tops(rows);
        let bytes = tops.capacity() * std::mem::size_of::<f32>();
        let mut cached_bytes: usize = self
            .layouts
            .iter()
            .map(|layout| layout.tops.capacity() * std::mem::size_of::<f32>())
            .sum();
        while !self.layouts.is_empty()
            && (self.layouts.len() >= MAX_LAYOUTS
                || cached_bytes.saturating_add(bytes) > MAX_LAYOUT_BYTES)
        {
            let retired = self.layouts.remove(0);
            cached_bytes -= retired.tops.capacity() * std::mem::size_of::<f32>();
        }
        self.layouts.push(Layout { rows: key, tops });
        self.layouts.len() - 1
    }
}

#[derive(Clone, Copy)]
struct MenuTheme {
    bg: Color,
    text: Color,
    sub: Color,
    header_top: Color,
    header_bottom: Color,
    header_border: Color,
    header_text: Color,
    preview_top: Color,
    preview_bottom: Color,
    stripe: Color,
}
fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}
fn alpha(mut color: Color, opacity: f32) -> Color {
    color[3] *= opacity;
    color
}
fn theme(dark: bool) -> MenuTheme {
    if dark {
        MenuTheme {
            bg: rgb(22, 23, 25),
            text: rgb(238, 239, 241),
            sub: rgb(150, 153, 158),
            header_top: rgb(78, 81, 86),
            header_bottom: rgb(36, 38, 42),
            header_border: rgb(10, 11, 12),
            header_text: rgb(240, 241, 243),
            preview_top: rgb(61, 65, 74),
            preview_bottom: rgb(23, 25, 29),
            stripe: rgb(30, 31, 34),
        }
    } else {
        MenuTheme {
            bg: rgb(255, 255, 255),
            text: rgb(0, 0, 0),
            sub: rgb(100, 100, 100),
            header_top: rgb(254, 255, 255),
            header_bottom: rgb(177, 182, 185),
            header_border: rgb(121, 149, 163),
            header_text: rgb(0, 0, 0),
            preview_top: rgb(177, 181, 192),
            preview_bottom: rgb(104, 110, 122),
            stripe: rgb(242, 243, 245),
        }
    }
}

impl MenuVisual {
    fn navigate(
        &mut self,
        previous: Page,
        current: Page,
        current_depth: usize,
        forward: bool,
        now: f64,
    ) {
        let previous_depth = if forward {
            current_depth.saturating_sub(1)
        } else {
            current_depth + 1
        };
        let previous_id = if let Some(id) = self.active_view.filter(|id| {
            self.views.iter().any(|view| {
                view.id == *id
                    && view.page.screen == previous.screen
                    && view.depth == previous_depth
            })
        }) {
            if let Some(view) = self.views.iter_mut().find(|view| view.id == id) {
                view.page = previous.clone();
            }
            id
        } else {
            let id = self.next_view;
            self.next_view += 1;
            self.views.push(ViewMotion {
                id,
                depth: previous_depth,
                page: previous.clone(),
                enter: Anim::new(0.0),
                hide: Anim::new(0.0),
                exiting: false,
            });
            id
        };
        let cross = previous.kind != current.kind;
        let current_id = if !forward {
            self.views
                .iter()
                .rev()
                .find(|view| {
                    !view.exiting
                        && view.depth == current_depth
                        && view.page.screen == current.screen
                })
                .map(|view| view.id)
        } else {
            None
        };
        let current_id = if let Some(id) = current_id {
            if let Some(view) = self.views.iter_mut().find(|view| view.id == id) {
                view.page = current.clone();
            }
            id
        } else {
            let id = self.next_view;
            self.next_view += 1;
            self.views.push(ViewMotion {
                id,
                depth: current_depth,
                page: current.clone(),
                enter: Anim::new(if forward && !cross { 1.0 } else { 0.0 }),
                hide: Anim::new(if !forward && !cross { -1.0 } else { 0.0 }),
                exiting: false,
            });
            id
        };
        for view in &mut self.views {
            if view.id == previous_id {
                if forward && !cross {
                    view.hide.set(-1.0, 0.300, Curve::Css, 0.0, now);
                } else if !forward {
                    view.exiting = true;
                    if !cross {
                        view.enter.set(1.0, 0.300, Curve::InOut, 0.0, now);
                    }
                }
            }
            if view.id == current_id {
                if forward && !cross {
                    view.enter.set(0.0, 0.300, Curve::InOut, 0.0, now);
                } else if !forward && !cross {
                    view.hide.set(0.0, 0.300, Curve::Css, 0.0, now);
                }
            }
        }
        if current.kind == ViewKind::CoverFlow {
            self.left_panel.set(-1.0, 0.350, Curve::Css, 0.050, now);
            self.preview_hide.set(1.0, 0.350, Curve::Css, 0.0, now);
        } else {
            if previous.kind == ViewKind::CoverFlow {
                self.preview_hide.set(0.0, 0.350, Curve::Css, 0.0, now);
            }
            if cross {
                if current.split() {
                    self.left_panel.set(0.0, 0.350, Curve::Css, 0.0, now);
                    self.full_panel.set(1.0, 0.350, Curve::Css, 0.0, now);
                } else {
                    self.left_panel.set(
                        -1.0,
                        0.350,
                        Curve::Css,
                        if previous.split() { 0.050 } else { 0.0 },
                        now,
                    );
                    self.full_panel.set(0.0, 0.350, Curve::Css, 0.0, now);
                }
            }
        }
        self.active_view = Some(current_id);
        self.navigation = Some(Navigation {
            previous,
            forward,
            started: now,
            cross_panel: cross,
            previous_id,
        });
    }
}

impl App {
    pub(super) fn visual_menu_active_page(&self) -> Option<Page> {
        self.visual_menu
            .active_view
            .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
            .map(|view| view.page.clone())
    }
    pub(super) fn visual_menu_remote_state(&self) -> serde_json::Value {
        serde_json::json!({"active":self.visual_menu.active_view,"left":anim_value(&self.visual_menu.left_panel,self.visual_time),"full":anim_value(&self.visual_menu.full_panel,self.visual_time),
        "views":self.visual_menu.views.iter().map(|v|serde_json::json!({"id":v.id,"screen":format!("{:?}",v.page.screen),"kind":format!("{:?}",v.page.kind),"depth":v.depth,"enter":anim_value(&v.enter,self.visual_time),"hide":anim_value(&v.hide,self.visual_time),"exiting":v.exiting})).collect::<Vec<_>>()})
    }
    /// False asks the caller to use the source's fallback action sheet.
    pub(super) fn visual_menu_ask_clear_now_playing(&mut self) -> bool {
        if !self
            .pages
            .last()
            .is_some_and(|page| page.split() && self.visual_menu_mini_wanted(page))
        {
            return false;
        }
        self.visual_menu.ask_on = true;
        self.visual_menu.ask_selected = 0;
        self.visual_menu.ask_until = self.visual_time + 5.0;
        self.visual_menu
            .ask
            .set(1.0, 0.320, Curve::Linear, 0.0, self.visual_time);
        true
    }
    pub(super) fn visual_menu_confirm_handle(&mut self, action: Action) -> bool {
        if !self.visual_menu.ask_on {
            return false;
        }
        match action {
            Action::Left | Action::Right => {
                self.visual_menu.ask_selected = 1 - self.visual_menu.ask_selected;
                self.visual_menu.ask_until = self.visual_time + 5.0;
            }
            Action::Select | Action::Back | Action::BackHeld => {
                let clear = action == Action::Select && self.visual_menu.ask_selected == 1;
                self.visual_menu.ask_on = false;
                if clear {
                    self.execute(Intent::ClearNowPlaying);
                    self.visual_menu.ask_reset = Some(self.visual_time + 0.600);
                } else {
                    self.visual_menu
                        .ask
                        .set(0.0, 0.320, Curve::Linear, 0.0, self.visual_time);
                }
            }
            _ => {}
        }
        true
    }
    pub(super) fn visual_menu_volume_changed(&mut self, now: f64) {
        self.visual_menu
            .volume_osd
            .set(1.0, 0.220, Curve::Css, 0.0, now);
        self.visual_menu.volume_until = now + 1.5;
        self.visual_menu.brightness_until = now;
    }
    pub(super) fn visual_menu_brightness_changed(&mut self, level: i32, now: f64) {
        self.visual_menu.brightness_level = level.clamp(0, 100);
        self.visual_menu
            .brightness_osd
            .set(1.0, 0.220, Curve::Css, 0.0, now);
        self.visual_menu.brightness_until = now + 1.5;
        self.visual_menu.volume_until = now;
    }
    #[cfg(test)]
    pub(super) fn visual_menu_qa_mini_state(&self) -> (bool, f64) {
        let Some(page) = self.pages.last() else {
            return (false, 0.0);
        };
        let kind = if page.split() { 0 } else { 1 };
        (
            self.visual_menu_mini_wanted(page),
            self.visual_menu.mini_show[kind].value(self.visual_time) as f64,
        )
    }

    /// ij_about.go: fixed wordmark above the independently scrolling credits.
    pub(super) fn visual_menu_draw_about(&mut self, r: &mut Renderer, _fg: Color, _muted: Color) {
        let th = theme(self.dark());
        r.rect(0.0, 40.0, 640.0, 440.0, th.bg);
        let better_width = r.text_width_weighted("better", 40.0, 900);
        let pod_width = r.text_width_weighted("Pod", 40.0, 900);
        let row_width = 96.0 + 24.0 + better_width + pod_width;
        let left = (640.0 - row_width) * 0.5;
        if let Some(logo) = self.visual_assets.get("betterpod_logo_96") {
            r.image(logo, left, 68.0, 96.0, 96.0, 1.0);
        }
        let baseline = 68.0 + (96.0 - line_box(40.0).0) * 0.5 + line_box(40.0).1;
        r.text_baseline(
            "better",
            left + 120.0,
            baseline,
            40.0,
            900,
            th.text,
            better_width,
        );
        r.text_baseline(
            "Pod",
            left + 120.0 + better_width,
            baseline,
            40.0,
            900,
            if self.dark() {
                rgb(88, 186, 255)
            } else {
                rgb(47, 134, 208)
            },
            pod_width,
        );
        let description = "The mid-2000s music player, reborn on retro handhelds";
        let desc_width = r.text_width_weighted(description, 22.0, 500);
        r.text_baseline(
            description,
            (640.0 - desc_width) * 0.5,
            180.0 + line_box(22.0).1,
            22.0,
            500,
            th.sub,
            624.0,
        );
        let Some(page) = self.pages.last() else {
            return;
        };
        let tops = row_tops(&page.rows);
        let total = tops.last().copied().unwrap_or(0.0);
        let height = 248.0;
        let scroll = page.scroll.clamp(0.0, (total - height).max(0.0));
        let scrollable = total > height + 1.0;
        let width = if scrollable { 622.0 } else { 640.0 };
        r.push_clip(0.0, 232.0, 640.0, height);
        for (i, row) in page.rows.iter().enumerate() {
            let y = 232.0 + tops[i] - scroll;
            if y + row_height(row) < 232.0 {
                continue;
            }
            if y >= 480.0 {
                break;
            }
            draw_row(
                r,
                &mut self.art,
                &self.visual_assets,
                row,
                &row.visual.queue_mark,
                0.0,
                y,
                width,
                row_height(row),
                i == page.selected,
                false,
                false,
                i,
                None,
                th,
                self.visual_menu.selection_since,
                self.visual_time,
                self.audio_snapshot.playing,
            );
        }
        if scrollable {
            draw_scrollbar(
                r,
                622.0,
                232.0,
                18.0,
                height,
                scroll,
                total,
                self.dark(),
                1.0,
            );
        }
        r.pop_clip();
    }

    /// Call after mutating the navigation stack. Previous rows are Arc snapshots.
    pub(super) fn visual_menu_navigation(&mut self, previous: Option<Page>, forward: bool) {
        if let (Some(previous), Some(current)) = (previous, self.pages.last().cloned()) {
            if matches!(previous.screen, Screen::NowPlaying | Screen::CoverFlow)
                && matches!(current.screen, Screen::NowPlaying | Screen::CoverFlow)
            {
                // These are two states of the same animated player. Its cover
                // owns the flip, while the outer panels still need to follow
                // the current page kind and the retained navigation depth.
                // In particular, Cover Flow leaves the full panel offscreen.
                self.visual_menu_snap_state(self.visual_time);
                return;
            }
            self.visual_menu.navigate(
                previous,
                current,
                self.pages.len().saturating_sub(1),
                forward,
                self.visual_time,
            );
        }
    }
    /// Extra's internal stack uses a zero-based depth, offset above the app page.
    pub(super) fn visual_menu_external_navigation(
        &mut self,
        previous: Page,
        current: Page,
        external_depth: usize,
        forward: bool,
        now: f64,
    ) {
        self.visual_time = now;
        let depth = self.pages.len().saturating_sub(1) + external_depth;
        self.visual_menu
            .navigate(previous, current, depth, forward, now);
    }

    /// Reconcile panel endpoints for fixed captures and the internally animated player.
    pub(super) fn visual_menu_snap_state(&mut self, now: f64) {
        self.visual_time = now;
        self.visual_menu_sync(now);
        self.visual_menu.navigation = None;
        self.visual_menu.views.clear();
        for (depth, page) in self.pages.iter().enumerate() {
            let covered = self
                .pages
                .iter()
                .skip(depth + 1)
                .any(|later| later.kind == page.kind);
            let id = self.visual_menu.next_view;
            self.visual_menu.next_view += 1;
            self.visual_menu.views.push(ViewMotion {
                id,
                depth,
                page: page.clone(),
                enter: Anim::new(0.0),
                hide: Anim::new(if covered { -1.0 } else { 0.0 }),
                exiting: false,
            });
            self.visual_menu.active_view = Some(id);
        }
        let split = self.pages.last().is_some_and(Page::split);
        let flow = self
            .pages
            .last()
            .is_some_and(|page| page.kind == ViewKind::CoverFlow);
        self.visual_menu
            .left_panel
            .snap(if split { 0.0 } else { -1.0 });
        self.visual_menu
            .full_panel
            .snap(if split || flow { 1.0 } else { 0.0 });
        self.visual_menu
            .preview_hide
            .snap(if flow { 1.0 } else { 0.0 });
        self.visual_menu.preview = self.visual_menu.preview_pending.clone();
        let music = if self.visual_menu.preview == PreviewKind::Music {
            0.0
        } else {
            1.0
        };
        self.visual_menu.music_preview.snap(music);
        self.visual_menu.mini_change.snap(1.0);
        self.visual_menu
            .mini_pause
            .snap(if self.audio_snapshot.playing {
                0.0
            } else {
                1.0
            });
        if let Some(page) = self.pages.last() {
            let want = self.visual_menu_mini_wanted(page) as u8 as f32;
            self.visual_menu.mini_show[if page.split() { 0 } else { 1 }].snap(want);
        }
    }

    pub(super) fn visual_menu_tick(&mut self, now: f64) {
        self.visual_time = now;
        self.visual_menu_sync(now);
    }

    fn visual_menu_sync(&mut self, now: f64) {
        self.visual_menu.tick(now);
        let active_page = self.projected_page().or_else(|| self.pages.last().cloned());
        if let Some(page) = active_page.as_ref() {
            // Shared custom controllers can switch Split/Full without adding
            // an App stack entry. Keep header/panel metadata consistent.
            if let Some(last) = self.pages.last_mut() {
                last.kind = page.kind;
                last.mini = page.mini;
                last.title = page.title.clone();
            }
            if let Some(view) = self
                .visual_menu
                .active_view
                .and_then(|id| self.visual_menu.views.iter_mut().find(|view| view.id == id))
            {
                view.page = page.clone();
            } else {
                let id = self.visual_menu.next_view;
                self.visual_menu.next_view += 1;
                self.visual_menu.views.push(ViewMotion {
                    id,
                    depth: self.pages.len() - 1,
                    page: page.clone(),
                    enter: Anim::new(0.0),
                    hide: Anim::new(0.0),
                    exiting: false,
                });
                self.visual_menu.active_view = Some(id);
            }
        }
        self.visual_menu_follow_ticker(now);
        let selected = active_page.as_ref().map(|p| (p.screen, p.selected));
        if self.visual_menu.selection != selected {
            self.visual_menu.selection = selected;
            self.visual_menu.selection_since = now;
        }
        let preview = active_page
            .as_ref()
            .filter(|p| p.split())
            .and_then(|p| p.rows.get(p.selected))
            .map_or_else(
                || self.visual_menu.preview_pending.clone(),
                |row| match &row.visual.preview {
                    PreviewKind::Auto => match &row.intent {
                        Intent::Open(Screen::Settings) => PreviewKind::Settings,
                        Intent::Open(Screen::Apps) => PreviewKind::Apps,
                        Intent::Open(Screen::NowPlaying) => PreviewKind::NowPlaying,
                        _ => PreviewKind::Music,
                    },
                    other => other.clone(),
                },
            );
        if self.visual_menu.preview_pending != preview {
            if preview == PreviewKind::Weather {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 7;
                self.rng ^= self.rng << 17;
                // Uniformly choose one of the other ten source weather kinds.
                let other = self.rng as usize % 10;
                self.visual_menu.weather_kind = if other >= self.visual_menu.weather_kind {
                    other + 1
                } else {
                    other
                };
            }
            self.visual_menu.preview_pending = preview;
            self.visual_menu.preview_pending_since = now;
        }
        if now - self.visual_menu.preview_pending_since >= 0.75
            && self.visual_menu.preview != self.visual_menu.preview_pending
        {
            self.visual_menu.preview = self.visual_menu.preview_pending.clone();
            let target = if self.visual_menu.preview == PreviewKind::Music {
                0.0
            } else {
                1.0
            };
            self.visual_menu
                .music_preview
                .to(target, 0.4, Curve::InOut, 0.0, now);
        }
        let paused = !self.audio_snapshot.playing;
        if paused {
            self.visual_menu.mini_paused_since.get_or_insert(now);
        } else {
            self.visual_menu.mini_paused_since = None;
        }
        let pause = if paused { 1.0 } else { 0.0 };
        if self.visual_menu.mini_pause.target != pause {
            self.visual_menu
                .mini_pause
                .to(pause, 0.3, Curve::InOut, 0.0, now);
            if !paused {
                self.visual_menu.mini_since = now;
            }
        }
        if self.now_track.is_some()
            && self.visual_menu.mini_track.as_ref().map(|t| &t.path)
                != self.now_track.as_ref().map(|t| &t.path)
        {
            self.visual_menu.mini_old = self.visual_menu.mini_track.take();
            self.visual_menu.mini_track = self.now_track.clone();
            self.visual_menu.mini_since = now;
            self.visual_menu.mini_direction =
                if self.queue_index < self.visual_menu.last_queue_index {
                    -1.0
                } else {
                    1.0
                };
            self.visual_menu.last_queue_index = self.queue_index;
            if self.visual_menu.mini_old.is_some() && now - self.visual_menu.last_drawn < 0.1 {
                self.visual_menu.mini_change.snap(0.0);
                self.visual_menu
                    .mini_change
                    .to(1.0, 0.38, Curve::Linear, 0.0, now);
            } else {
                self.visual_menu.mini_change.snap(1.0);
                self.visual_menu.mini_pause.snap(pause);
            }
        }
    }

    fn visual_menu_mini_wanted(&self, page: &Page) -> bool {
        self.now_track.is_some()
            && !self
                .visual_menu
                .mini_paused_since
                .is_some_and(|since| self.visual_time - since >= 600.0)
            && page.mini
    }
    pub(super) fn visual_menu_content_height(&self, page: &Page) -> f32 {
        440.0
            - if self.visual_menu_mini_wanted(page) {
                MINI_HEIGHT
            } else {
                0.0
            }
    }
    /// Uses the same immutable prefix heights as drawing. The caller skips
    /// unchanged(row Arc, selected index, viewport) tuples, so frame ticks do
    /// not walk a large library and selection changes are constant-time.
    pub(super) fn visual_menu_reveal_current_selection(&mut self) {
        let Some(page) = self.pages.last() else {
            return;
        };
        if page.rows.is_empty() {
            if let Some(page) = self.pages.last_mut() {
                page.selected = 0;
                page.scroll = 0.0;
                page.target_scroll = 0.0;
            }
            return;
        }
        let available = self.visual_menu_content_height(page);
        let rows = Arc::clone(&page.rows);
        let mut selected = page.selected.min(rows.len() - 1);
        if rows[selected].visual.header {
            selected = (selected + 1).min(rows.len() - 1);
        }
        let index = self.visual_menu.layout_index(&rows);
        let tops = &self.visual_menu.layouts[index].tops;
        let top = tops[selected];
        let row_height = tops[selected + 1] - top;
        let total = tops.last().copied().unwrap_or(0.0);
        let page = self.pages.last_mut().unwrap();
        page.selected = selected;
        if top < page.target_scroll {
            page.target_scroll = top;
        } else if top + row_height > page.target_scroll + available {
            page.target_scroll = top + row_height - available;
        }
        page.target_scroll = page.target_scroll.clamp(0.0, (total - available).max(0.0));
        page.scroll = page.target_scroll;
    }

    fn visual_menu_ticker_allowed(&self) -> bool {
        if self.screen() == Screen::Quiz {
            return false;
        }
        if self.screen() == Screen::CoverFlow {
            return !self.cover_from_np;
        }
        let Some(page) = self.pages.last() else {
            return false;
        };
        !matches!(page.screen, Screen::NowPlaying | Screen::Video)
            && !self.visual_menu_mini_wanted(page)
            && !(page.split() && self.visual_menu.preview == PreviewKind::NowPlaying)
    }
    fn visual_menu_follow_ticker(&mut self, now: f64) {
        let path = self.now_track.as_ref().map(|t| t.path.clone());
        let changed =
            path != self.visual_menu.ticker_last_path || !self.visual_menu.ticker_last_playing;
        if self.audio_snapshot.playing
            && changed
            && self.visual_menu_ticker_allowed()
            && !self.held
            && !self.screen_off
        {
            if let Some(track) = self.now_track.clone() {
                let elapsed = self
                    .visual_menu
                    .ticker_started
                    .map_or(f64::INFINITY, |start| now - start);
                if elapsed >= 4.26 {
                    self.visual_menu.ticker_started = Some(now);
                } else if elapsed >= 0.38 && elapsed < 3.88 {
                    self.visual_menu.ticker_started = Some(now - 0.38);
                } else if elapsed >= 3.88 {
                    self.visual_menu.ticker_next = Some(track.clone());
                }
                if elapsed < 3.88 || elapsed >= 4.26 {
                    self.visual_menu.ticker_track = Some(track);
                }
            }
        }
        self.visual_menu.ticker_last_path = path;
        self.visual_menu.ticker_last_playing = self.audio_snapshot.playing;
        if self
            .visual_menu
            .ticker_started
            .is_some_and(|start| now - start >= 4.26)
        {
            self.visual_menu.ticker_started = None;
            if let Some(next) = self.visual_menu.ticker_next.take() {
                self.visual_menu.ticker_track = Some(next);
                self.visual_menu.ticker_started = Some(now);
            }
        }
    }

    fn visual_menu_panel_x(&self, page: &Page) -> f32 {
        match page.kind {
            ViewKind::Split => anim_value(&self.visual_menu.left_panel, self.visual_time) * 320.0,
            ViewKind::Full => anim_value(&self.visual_menu.full_panel, self.visual_time) * 640.0,
            ViewKind::CoverFlow => 0.0,
        }
    }
    fn visual_menu_view_offset(&self, view: &ViewMotion) -> f32 {
        self.visual_menu_panel_x(&view.page)
            + (anim_value(&view.enter, self.visual_time) + anim_value(&view.hide, self.visual_time))
                * if view.page.split() { 320.0 } else { 640.0 }
    }
    /// Source enter/hide values keep their own clocks through an interruption.
    pub(super) fn visual_menu_active_offset(&self) -> f32 {
        self.visual_menu
            .active_view
            .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
            .map_or(0.0, |view| self.visual_menu_view_offset(view))
    }
    pub(super) fn visual_menu_outgoing_pose(&self) -> Option<(Page, f32, bool)> {
        let nav = self.visual_menu.navigation.as_ref()?;
        let view = self
            .visual_menu
            .views
            .iter()
            .find(|view| view.id == nav.previous_id)?;
        Some((
            view.page.clone(),
            self.visual_menu_view_offset(view),
            nav.cross_panel,
        ))
    }
    fn visual_menu_draw_body_layer(&mut self, r: &mut Renderer, view: &ViewMotion) {
        let page = &view.page;
        let width = if page.split() { 320.0 } else { 640.0 };
        let panel = self.visual_menu_panel_x(page);
        let body = (anim_value(&view.enter, self.visual_time)
            + anim_value(&view.hide, self.visual_time))
            * width;
        if panel + width <= 0.0 || panel >= 640.0 || body >= width || body <= -width {
            return;
        }
        r.push_clip(panel, 40.0, width, 440.0);
        r.push_transform(panel + body, 0.0, 1.0, 1.0, 1.0);
        r.rect(0.0, 40.0, width, 440.0, theme(self.dark()).bg);
        let bottom = if self.visual_menu_mini_wanted(page) {
            364.0
        } else {
            480.0
        };
        self.visual_menu_rows(
            r,
            page,
            0.0,
            width,
            bottom,
            Some(view.id) == self.visual_menu.active_view,
            false,
        );
        r.pop_transform();
        r.pop_clip();
    }
    fn visual_menu_panel_chrome(&mut self, r: &mut Renderer, kind: ViewKind) {
        let top = self
            .visual_menu
            .active_view
            .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
            .filter(|view| view.page.kind == kind)
            .map(|view| view.page.clone())
            .or_else(|| {
                self.pages
                    .iter()
                    .rev()
                    .find(|page| page.kind == kind)
                    .cloned()
            });
        let fallback = self
            .visual_menu
            .views
            .iter()
            .rev()
            .find(|view| view.page.kind == kind)
            .map(|view| view.page.clone());
        let Some(page) = top.clone().or(fallback) else {
            return;
        };
        let panel = self.visual_menu_panel_x(&page);
        let width = if page.split() { 320.0 } else { 640.0 };
        if panel >= 640.0 || panel + width <= 0.0 {
            return;
        }
        let title = self
            .visual_menu
            .active_view
            .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
            .map_or(page.title.clone(), |current| current.page.title.clone());
        r.push_transform(panel, 0.0, 1.0, 1.0, 1.0);
        self.visual_menu_draw_header(
            r,
            &title,
            0.0,
            width,
            self.visual_menu
                .active_view
                .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
                .is_some_and(|view| view.page.kind == kind),
        );
        let wanted = top
            .as_ref()
            .is_some_and(|page| self.visual_menu_mini_wanted(page));
        self.visual_menu_mini_with_wanted(r, &page, 0.0, wanted);
        r.pop_transform();
        if kind == ViewKind::Split {
            let opacity = (panel / 320.0 + 1.0).clamp(0.0, 1.0);
            r.push_transform(0.0, 0.0, 1.0, 1.0, opacity);
            self.visual_menu_shadow(r, panel + 320.0, page.screen == Screen::Settings);
            r.pop_transform();
        }
    }
    pub(super) fn visual_menu_back_transition(&self) -> bool {
        self.visual_menu
            .navigation
            .as_ref()
            .is_some_and(|nav| !nav.forward)
    }
    fn visual_menu_preview_visible(&self) -> bool {
        self.visual_menu.left_panel.running(self.visual_time)
            || anim_value(&self.visual_menu.left_panel, self.visual_time) > -0.999
            || self.visual_menu.full_panel.running(self.visual_time)
    }
    fn visual_menu_outgoing_rows(&mut self, r: &mut Renderer) {
        let current = self.visual_menu.active_view;
        let mut views: Vec<_> = self
            .visual_menu
            .views
            .iter()
            .filter(|view| {
                Some(view.id) != current
                    && (is_menu(view.page.screen)
                        || (view.page.screen == Screen::Extra && view.page.split()))
            })
            .cloned()
            .collect();
        views.sort_by_key(|view| (view.depth, view.id));
        for view in views {
            self.visual_menu_draw_body_layer(r, &view);
        }
    }
    pub(super) fn visual_menu_draw_outgoing(&mut self, r: &mut Renderer) {
        if self.visual_menu_preview_visible() {
            self.visual_menu_preview(r);
        }
        self.visual_menu_outgoing_rows(r);
        self.visual_menu_panel_chrome(r, ViewKind::Split);
        if self.pages.last().is_some_and(Page::split) {
            self.visual_menu_panel_chrome(r, ViewKind::Full);
        }
    }
    pub(super) fn visual_menu_draw(
        &mut self,
        r: &mut Renderer,
        elapsed: f64,
        fg: Color,
        muted: Color,
    ) {
        self.visual_time = elapsed;
        self.visual_menu_sync(elapsed);
        let active_page = if self.screen() == Screen::Extra {
            self.extras
                .visual_page()
                .or_else(|| self.pages.last().cloned())
        } else {
            self.pages.last().cloned()
        };
        let Some(page) = active_page else {
            return;
        };
        if self.visual_menu_preview_visible() {
            self.visual_menu_preview(r);
        }
        let view = self
            .visual_menu
            .active_view
            .and_then(|id| self.visual_menu.views.iter().find(|view| view.id == id))
            .cloned();
        let back = self.visual_menu_back_transition();
        if back {
            if let Some(view) = &view {
                self.visual_menu_draw_body_layer(r, view);
            }
        }
        self.visual_menu_outgoing_rows(r);
        if !page.split() {
            // Go draws each panel's block after that panel's view buffers.
            // Retained Music rows paint their entire440px white cache, so
            // painting this before the rows erased the outgoing mini-player.
            // It must also remain below the incoming full-width panel.
            self.visual_menu_panel_chrome(r, ViewKind::Split);
        }
        if !back {
            self.visual_draw_outgoing_custom(r, elapsed, fg, muted);
        }
        if !back {
            if let Some(view) = &view {
                self.visual_menu_draw_body_layer(r, view);
            }
        }
        if back {
            self.visual_draw_outgoing_custom(r, elapsed, fg, muted);
        }
        self.visual_menu_panel_chrome(r, page.kind);
        if page.split() {
            self.visual_menu_panel_chrome(r, ViewKind::Full);
        }
    }

    pub(super) fn visual_menu_page(
        &mut self,
        r: &mut Renderer,
        page: &Page,
        offset: f32,
        whole_panel: bool,
        outgoing: bool,
    ) {
        let width = if page.split() { 320.0 } else { 640.0 };
        if whole_panel {
            r.push_transform(offset, 0.0, 1.0, 1.0, 1.0);
            self.visual_menu_draw_header(r, &page.title, 0.0, width, !outgoing);
        }
        let body_offset = if whole_panel { 0.0 } else { offset };
        r.push_clip(0.0, 40.0, width, 440.0);
        r.push_transform(body_offset, 0.0, 1.0, 1.0, 1.0);
        let wants_mini = self.visual_menu_mini_wanted(page);
        let bottom = if wants_mini {
            480.0 - MINI_HEIGHT
        } else {
            480.0
        };
        r.rect(0.0, 40.0, width, 440.0, theme(self.dark()).bg);
        self.visual_menu_rows(r, page, 0.0, width, bottom, !outgoing, false);
        r.pop_transform();
        r.pop_clip();
        if whole_panel {
            self.visual_menu_mini(r, page, 0.0);
            r.pop_transform();
        }
        if page.split() && whole_panel {
            let shadow_x = offset + width;
            self.visual_menu_shadow(r, shadow_x, page.screen == Screen::Settings);
        }
    }

    fn visual_menu_rows(
        &mut self,
        r: &mut Renderer,
        page: &Page,
        x: f32,
        width: f32,
        bottom: f32,
        active: bool,
        column: bool,
    ) {
        let th = theme(self.dark());
        let height = bottom - HEADER_HEIGHT;
        r.push_clip(x, HEADER_HEIGHT, width, height);
        r.rect(x, HEADER_HEIGHT, width, height, th.bg);
        let layout_index = self.visual_menu.layout_index(&page.rows);
        let tops = &self.visual_menu.layouts[layout_index].tops;
        let total = tops.last().copied().unwrap_or(0.0);
        let scrollable = total > height + 1.0;
        let track_list = false; // Only CoverFlowBack uses Go's TrackList layout.
        let content_width = width
            - if scrollable && !track_list {
                SCROLLBAR_WIDTH
            } else {
                0.0
            };
        let scroll = page.scroll.max(0.0).min((total - height).max(0.0));
        let first = tops.partition_point(|&top| top < scroll).saturating_sub(1);
        let selected_since = self.visual_menu.selection_since;
        let current = self.current_index();
        for i in first..page.rows.len() {
            let y = HEADER_HEIGHT + tops[i] - scroll;
            if y >= bottom {
                break;
            }
            let row = &page.rows[i];
            let row_height = tops[i + 1] - tops[i];
            let selected = i == page.selected && (!column || self.preview_focus);
            let inactive = page.screen == Screen::Settings && self.preview_focus && !column;
            let marked_path = match row.intent {
                Intent::Song(index) => self.library.tracks.get(index).map(|track| &track.path),
                Intent::QueueSong(index) => self.queue.get(index),
                _ => None,
            };
            let queue_mark = match marked_path {
                Some(path) => self.queued_marks.get(path).map_or("", String::as_str),
                None => row.visual.queue_mark.as_str(),
            };
            draw_row(
                r,
                &mut self.art,
                &self.visual_assets,
                row,
                queue_mark,
                x,
                y,
                content_width,
                row_height,
                selected,
                inactive,
                track_list,
                i,
                current,
                th,
                selected_since,
                self.visual_time,
                self.audio_snapshot.playing,
            );
        }
        if page.rows.is_empty() {
            let message = if self.scanning {
                "Reading your music..."
            } else {
                match page.screen {
                    Screen::Artists => "No artists",
                    Screen::Albums => "No albums",
                    Screen::Songs | Screen::Search => "No songs to show",
                    Screen::Album(_) => "No saved songs",
                    _ => "Nothing to see here",
                }
            };
            let size = 32.0;
            let tw = r.text_width_weighted(message, size, 500).min(width - 32.0);
            r.text_baseline(
                message,
                x + (width - tw) * 0.5,
                HEADER_HEIGHT + height * 0.5 - line_box(size).0 * 0.5 + line_box(size).1,
                size,
                500,
                th.sub,
                width - 32.0,
            );
        }
        if scrollable && !track_list {
            draw_scrollbar(
                r,
                x + width - SCROLLBAR_WIDTH,
                HEADER_HEIGHT,
                SCROLLBAR_WIDTH,
                height,
                scroll,
                total,
                self.dark(),
                1.0,
            );
        }
        r.pop_clip();
        let _ = active; // Retained outgoing pages keep their cursor, as Go's cached views do.
    }

    pub(super) fn visual_menu_draw_header(
        &mut self,
        r: &mut Renderer,
        title: &str,
        x: f32,
        width: f32,
        active: bool,
    ) {
        let th = theme(self.dark());
        r.push_clip(x, 0.0, width, HEADER_HEIGHT);
        r.gradient(x, 0.0, width, 38.0, th.header_top, th.header_bottom);
        r.rect(x, 38.0, width, 2.0, th.header_border);
        let status = self.visual_menu.status;
        let battery_x = x + width - 12.0 - 52.0;
        if let Some(shell) = self.visual_assets.get(if self.dark() {
            "battery_shell__dark"
        } else {
            "battery_shell"
        }) {
            let tint = WHITE;
            r.image_tinted(
                shell,
                [battery_x, 7.0, 52.0, 24.0],
                [0.0, 0.0, 1.0, 1.0],
                tint,
            );
        }
        // Go intentionally shows an unknown battery as full, and clamps its
        // minimum visible fill to15% so the warning remains legible.
        let pct = status.battery.unwrap_or(100).max(15);
        let fill_name = if pct <= 20 && !status.charging {
            "battery_fill_warn"
        } else {
            "battery_fill_ok"
        };
        if let Some(fill) = self.visual_assets.get(fill_name) {
            let amount = (pct as f32 / 85.0).min(1.0);
            let w = (44.0 * amount).round();
            r.image_uv(
                fill,
                [battery_x + 2.0, 9.0, w, 20.0],
                [0.0, 0.0, w / 44.0, 1.0],
                1.0,
            );
        }
        if status.charging {
            if let Some(icon) = self.visual_assets.get(if pct >= 100 {
                "battery_plug"
            } else {
                "battery_bolt"
            }) {
                r.image(icon, battery_x + 2.0, 9.0, 44.0, 20.0, 1.0);
            }
        }
        let mut right = battery_x - 10.0;
        let icon_color = self
            .visual_assets
            .tint(if self.dark() {
                "bluetooth__dark"
            } else {
                "bluetooth"
            })
            .unwrap_or(th.header_text);
        if status.bluetooth || status.headphones {
            draw_headset(
                r,
                right + 3.0 - 30.0,
                7.0,
                status.wifi,
                status.bluetooth,
                icon_color,
            );
            right -= 34.0;
        } else if status.wifi >= 0 {
            draw_wifi(
                r,
                right + 3.0 - 36.0,
                7.0,
                ((status.wifi as i32 * 3 + 4) / 5) as usize,
                icon_color,
            );
            right -= 40.0;
        }
        if self.now_track.is_some() && self.screen() != Screen::Quiz {
            if let Some(icon) = self.visual_assets.get(if self.audio_snapshot.playing {
                "play"
            } else {
                "pause"
            }) {
                let name = if self.audio_snapshot.playing {
                    "play"
                } else {
                    "pause"
                };
                right = put_icon(
                    r,
                    icon,
                    right,
                    7.0,
                    WHITE,
                    self.visual_assets.ink(name).unwrap_or((0.0, 0.0)),
                );
            }
        }
        let repeat = match self.settings.repeat.as_str() {
            "one" => Some(if self.dark() {
                "hdr_repeat_one__dark"
            } else {
                "hdr_repeat_one"
            }),
            "all" => Some(if self.dark() {
                "hdr_repeat__dark"
            } else {
                "hdr_repeat"
            }),
            _ => None,
        };
        if let Some(icon) = repeat.and_then(|name| self.visual_assets.get(name)) {
            let name = repeat.unwrap();
            right = put_icon(
                r,
                icon,
                right,
                6.0,
                WHITE,
                self.visual_assets.ink(name).unwrap_or((0.0, 0.0)),
            );
        }
        if self.settings.shuffle {
            let name = if self.dark() {
                "hdr_shuffle__dark"
            } else {
                "hdr_shuffle"
            };
            if let Some(icon) = self.visual_assets.get(name) {
                right = put_icon(
                    r,
                    icon,
                    right,
                    6.0,
                    WHITE,
                    self.visual_assets.ink(name).unwrap_or((0.0, 0.0)),
                );
            }
        }
        if self.held {
            draw_lock(r, right - 16.0, 9.0, icon_color);
            right -= 26.0;
        }
        let text_x = x + 12.0;
        let mut available = (right - text_x).max(0.0);
        if self.held && (x != 0.0 || width >= 640.0 || self.screen() == Screen::NowPlaying)
            || width >= 640.0
                && matches!(self.screen(), Screen::Game | Screen::Puzzle | Screen::Quiz)
                && self.settings.game_clock.iter().any(|name| name == title)
        {
            let clock_width = r.text_width_weighted(&self.clock, 26.0, 700);
            let clock_x = x + 320.0 - clock_width * 0.5;
            available = available.min(clock_x - 16.0 - text_x);
            r.text_baseline(
                &self.clock,
                clock_x,
                3.0 + line_box(26.0).1,
                26.0,
                700,
                th.header_text,
                clock_width,
            );
        }
        self.visual_menu_header_title(r, title, text_x, available, active, th);
        r.pop_clip();
    }

    fn visual_menu_header_title(
        &mut self,
        r: &mut Renderer,
        title: &str,
        x: f32,
        width: f32,
        active: bool,
        th: MenuTheme,
    ) {
        let now = self.visual_time;
        let baseline = 3.0 + line_box(26.0).1;
        let title = ellipsize(r, title, 26.0, 700, width);
        if self.held && now < self.visual_menu.brightness_until {
            draw_header_bar(
                r,
                x,
                width,
                0.0,
                self.visual_menu.brightness_level as f32 / 100.0,
                true,
                false,
                th.header_text,
            );
            return;
        }
        let brightness = self.visual_menu.brightness_osd.value(now) as f32;
        let volume = self.visual_menu.volume_osd.value(now) as f32;
        let osd = if brightness > 0.001 {
            Some((brightness, true))
        } else if volume > 0.001 && active && self.screen() != Screen::NowPlaying {
            Some((volume, false))
        } else {
            None
        };
        if let Some((amount, brightness)) = osd {
            let up = -40.0 * go_ease_in_out(amount);
            r.text_baseline(&title, x, baseline + up, 26.0, 700, th.header_text, width);
            let level = if brightness {
                self.visual_menu.brightness_level as f32 / 100.0
            } else {
                self.settings.volume
            };
            let headphones =
                self.visual_menu.status.headphones || self.visual_menu.status.bluetooth;
            draw_header_bar(
                r,
                x,
                width,
                40.0 + up,
                level,
                brightness,
                headphones,
                th.header_text,
            );
            return;
        }
        if active && self.visual_menu_ticker_allowed() {
            if let (Some(start), Some(track)) = (
                self.visual_menu.ticker_started,
                self.visual_menu.ticker_track.as_ref(),
            ) {
                let elapsed = now - start;
                let (title_offset, song_offset) = if elapsed < 0.38 {
                    let p = go_ease_in_out((elapsed / 0.38) as f32);
                    (-40.0 * p, 40.0 * (1.0 - p))
                } else if elapsed < 3.88 {
                    (-40.0, 0.0)
                } else {
                    let p = go_ease_in_out(((elapsed - 3.88) / 0.38) as f32);
                    (40.0 * (1.0 - p), -40.0 * p)
                };
                if title_offset > -40.0 && title_offset < 40.0 {
                    r.text_baseline(
                        &title,
                        x,
                        baseline + title_offset,
                        26.0,
                        700,
                        th.header_text,
                        width,
                    );
                }
                if song_offset > -40.0 && song_offset < 40.0 {
                    let texture = self.art.get(track.cover.as_deref());
                    r.image(texture, x, 6.0 + song_offset, 28.0, 28.0, 1.0);
                    let name = if track.title.is_empty() {
                        "Unknown Song"
                    } else {
                        &track.title
                    };
                    let room = (width - 56.0).max(0.0);
                    let name = ellipsize(r, name, 26.0, 700, room);
                    r.text_baseline(
                        &name,
                        x + 40.0,
                        baseline + song_offset,
                        26.0,
                        700,
                        th.header_text,
                        room,
                    );
                }
                return;
            }
        }
        r.text_baseline(&title, x, baseline, 26.0, 700, th.header_text, width);
    }

    fn visual_menu_shadow(&self, r: &mut Renderer, x: f32, column: bool) {
        // Gaussian edge profile: sigma=24 physical px, sampled at pixel centres.
        let (count, step, scale) = if column {
            (36, 2.0, 0.35)
        } else {
            (72, 1.0, 1.0)
        };
        for i in 0..count {
            let distance = i as f32 * step + 0.5;
            let opacity = 0.5 * erfc(distance / (std::f32::consts::SQRT_2 * 24.0)) * scale;
            r.rect(x + i as f32, 0.0, 1.0, 480.0, [0.0, 0.0, 0.0, opacity]);
        }
    }

    fn visual_menu_preview(&mut self, r: &mut Renderer) {
        let hide = anim_value(&self.visual_menu.preview_hide, self.visual_time);
        if hide >= 0.999 && !self.visual_menu.preview_hide.running(self.visual_time) {
            return;
        }
        r.push_transform(320.0 * hide, 0.0, 1.0, 1.0, 1.0 - hide);
        self.visual_menu_preview_content(r);
        r.pop_transform();
    }
    fn visual_menu_preview_content(&mut self, r: &mut Renderer) {
        let th = theme(self.dark());
        r.push_clip(320.0, 0.0, 320.0, 480.0);
        r.rect(320.0, 0.0, 320.0, 480.0, th.bg);
        let settings = self
            .pages
            .last()
            .is_some_and(|p| p.screen == Screen::Settings);
        if settings {
            r.pop_clip();
            self.visual_menu_column(r);
            return;
        }
        let kind = self.visual_menu.preview.clone();
        match kind {
            PreviewKind::Apps => self.visual_menu_icon_preview(r, "apps", "Apps", ""),
            PreviewKind::Settings => {
                self.visual_menu_icon_preview(r, "settings_tile", "Settings", "")
            }
            PreviewKind::Icon {
                asset,
                title,
                subtitle,
            } => self.visual_menu_icon_preview(r, &asset, &title, &subtitle),
            PreviewKind::Image {
                asset,
                title,
                subtitle,
                size,
            } => {
                self.visual_menu_image_preview(r, Some(&asset), &title, &subtitle, size as f32);
            }
            PreviewKind::Weather => {
                let y = self.visual_menu_image_preview(r, None, "Weather", "", 256.0);
                let (code, day) = [
                    (0, true),
                    (0, false),
                    (2, true),
                    (2, false),
                    (3, true),
                    (45, true),
                    (51, true),
                    (61, true),
                    (71, true),
                    (56, true),
                    (95, true),
                ][self.visual_menu.weather_kind.min(10)];
                betterpod_rust::weather::draw_reference_icon(
                    r,
                    code,
                    day,
                    352.0,
                    y,
                    256.0,
                    if self.settings.effects == "light" {
                        -1.0
                    } else {
                        (self.visual_time / 3.0).rem_euclid(1.0)
                    },
                );
            }
            PreviewKind::NowPlaying => {
                if let Some(track) = self.now_track.as_ref() {
                    let texture = self.art.get(track.cover.as_deref());
                    let now = self.visual_time;
                    if now - self.visual_menu.np_pan_shown > 0.5 {
                        self.visual_menu.np_pan_start = now;
                    }
                    self.visual_menu.np_pan_shown = now;
                    let phase = (now - self.visual_menu.np_pan_start).rem_euclid(16.0);
                    let light = self.settings.effects == "light";
                    if light {
                        r.image(texture, 320.0, 0.0, 480.0, 480.0, 1.0);
                    } else if phase < 14.0 {
                        let shift = -160.0 * go_ease_in_out((phase / 14.0) as f32);
                        r.image(texture, 320.0 + shift, 0.0, 480.0, 480.0, 1.0);
                    } else {
                        r.image(texture, 160.0, 0.0, 480.0, 480.0, 1.0);
                        r.image(
                            texture,
                            320.0,
                            0.0,
                            480.0,
                            480.0,
                            go_ease_css(((phase - 14.0) / 2.0) as f32),
                        );
                    }
                }
            }
            _ => {}
        }
        let m = self.visual_menu.music_preview.value(self.visual_time);
        if self.visual_menu.preview == PreviewKind::Music || m < 1.0 {
            let tx = 320.0 * 1.3 * m;
            r.push_transform(tx, 0.0, 1.0, 1.0, 1.0 - m);
            self.visual_menu_ken_burns(r);
            r.pop_transform();
        }
        r.pop_clip();
    }

    fn visual_menu_icon_preview(&self, r: &mut Renderer, asset: &str, title: &str, subtitle: &str) {
        let th = theme(self.dark());
        r.gradient(320.0, 0.0, 320.0, 480.0, th.preview_top, th.preview_bottom);
        let title_height = line_box(32.0).0;
        let subtitle_height = if subtitle.is_empty() {
            0.0
        } else {
            line_box(24.0).0
        };
        let total = 192.0 + 32.0 + title_height + subtitle_height;
        let y = (480.0 - total) * 0.5;
        if let Some(texture) = self.visual_assets.get(asset) {
            r.image(texture, 384.0, y + 16.0, 192.0, 192.0, 1.0);
        }
        let tw = r.text_width_weighted(title, 32.0, 500);
        r.text_baseline(
            title,
            320.0 + (320.0 - tw) * 0.5,
            y + 224.0 + line_box(32.0).1,
            32.0,
            500,
            WHITE,
            304.0,
        );
        if !subtitle.is_empty() {
            let tw = r.text_width_weighted(subtitle, 24.0, 400);
            r.text_baseline(
                subtitle,
                320.0 + (320.0 - tw) * 0.5,
                y + 224.0 + title_height + line_box(24.0).1,
                24.0,
                400,
                WHITE,
                304.0,
            );
        }
    }

    /// Original drawImgPreview: unlike Apps/Settings there is no extra icon
    /// inset and its label is separated by twelve CSS pixels.
    fn visual_menu_image_preview(
        &self,
        r: &mut Renderer,
        asset: Option<&str>,
        title: &str,
        subtitle: &str,
        size: f32,
    ) -> f32 {
        let th = theme(self.dark());
        r.gradient(320.0, 0.0, 320.0, 480.0, th.preview_top, th.preview_bottom);
        let total = size
            + 24.0
            + line_box(32.0).0
            + if subtitle.is_empty() {
                0.0
            } else {
                line_box(24.0).0
            };
        let y = (480.0 - total) / 2.0;
        if let Some(texture) = asset.and_then(|name| self.visual_assets.get(name)) {
            r.image(texture, 320.0 + (320.0 - size) / 2.0, y, size, size, 1.0);
        }
        let width = r.text_width_weighted(title, 32.0, 500);
        r.text_baseline(
            title,
            320.0 + (320.0 - width) / 2.0,
            y + size + 24.0 + line_box(32.0).1,
            32.0,
            500,
            WHITE,
            304.0,
        );
        if !subtitle.is_empty() {
            let width = r.text_width_weighted(subtitle, 24.0, 500);
            r.text_baseline(
                subtitle,
                320.0 + (320.0 - width) / 2.0,
                y + size + 24.0 + line_box(32.0).0 + line_box(24.0).1,
                24.0,
                500,
                [1.0, 1.0, 1.0, 0.75],
                304.0,
            );
        }
        y
    }

    fn visual_menu_ken_burns(&mut self, r: &mut Renderer) {
        let now = self.visual_time;
        if self.visual_menu.cycle.generation != self.library_generation
            && !self.visual_menu.cycle.seeded
        {
            let mut order: Vec<usize> = self
                .library
                .albums
                .iter()
                .enumerate()
                .filter_map(|(i, a)| a.cover.as_ref().map(|_| i))
                .collect();
            if order.is_empty() {
                order.extend(0..self.library.albums.len());
            }
            // Same Fisher-Yates order selection as Go; random seed belongs to App.
            for i in (1..order.len()).rev() {
                self.rng ^= self.rng << 13;
                self.rng ^= self.rng >> 7;
                self.rng ^= self.rng << 17;
                order.swap(i, self.rng as usize % (i + 1));
            }
            self.visual_menu.cycle.order = order;
            self.visual_menu.cycle.starts.clear();
            if let Some(&first) = self.visual_menu.cycle.order.first() {
                self.visual_menu.cycle.starts.insert(first, now);
            }
            self.visual_menu.cycle.last_cycle = now;
            self.visual_menu.cycle.transition = None;
            self.visual_menu.cycle.generation = self.library_generation;
        }
        let fade = if self.settings.effects == "light" {
            0.6
        } else {
            2.0
        };
        let cycle = &mut self.visual_menu.cycle;
        if cycle.order.is_empty() {
            return;
        }
        if now - cycle.last_cycle >= 5.0 && cycle.transition.is_none() {
            cycle.last_cycle += 5.0;
            cycle.transition = Some(now);
            if let Some(&next) = cycle.order.get(1) {
                cycle.starts.entry(next).or_insert(now);
            }
        }
        if cycle.transition.is_some_and(|start| now - start >= fade) {
            let first = cycle.order.remove(0);
            cycle.order.push(first);
            cycle.starts.remove(&first);
            cycle.transition = None;
        }
        let top = cycle.order[0];
        let under = cycle.order.get(1).copied();
        let top_opacity = cycle.transition.map_or(1.0, |start| {
            1.0 - go_ease_css(((now - start) / fade) as f32)
        });
        let top_start = cycle.starts.get(&top).copied().unwrap_or(now);
        let under_start = under
            .and_then(|i| cycle.starts.get(&i).copied())
            .unwrap_or(now);
        let light = self.settings.effects == "light";
        let mut draw = |index: usize, started: f64, opacity: f32| {
            let cover = self
                .library
                .albums
                .get(index)
                .and_then(|a| a.cover.as_deref());
            let texture = self.art.get(cover);
            let pan = if light {
                0.0
            } else {
                48.0 * go_ease_css(((now - started) / 20.0) as f32)
            };
            r.image(texture, 160.0 + pan, 0.0, 480.0, 480.0, opacity);
        };
        if let Some(under) = under {
            draw(under, under_start, 1.0);
        }
        draw(top, top_start, top_opacity);
    }

    fn visual_menu_column(&mut self, r: &mut Renderer) {
        let th = theme(self.dark());
        let title = self
            .pages
            .last()
            .and_then(|p| p.rows.get(p.selected))
            .map(|row| row.label.clone())
            .unwrap_or_else(|| "Settings".into());
        r.push_clip(320.0, 0.0, 320.0, 480.0);
        r.gradient(320.0, 0.0, 320.0, 38.0, th.header_top, th.header_bottom);
        r.rect(320.0, 38.0, 320.0, 2.0, th.header_border);
        let label = ellipsize(r, &title, 26.0, 700, 300.0);
        r.text_baseline(
            &label,
            332.0,
            3.0 + line_box(26.0).1,
            26.0,
            700,
            th.header_text,
            300.0,
        );
        r.rect(320.0, 40.0, 320.0, 440.0, th.bg);
        let mut scroll = 0.0;
        let mut top = 0.0;
        for (index, row) in self.preview_rows.iter().enumerate() {
            let h = row_height(row);
            if self.preview_focus && index == self.preview_selected && top + h > 440.0 {
                scroll = top + h - 440.0;
            }
            top += h;
        }
        let mut y = 40.0 - scroll;
        for (i, row) in self.preview_rows.iter().enumerate() {
            let h = row_height(row);
            if y + h >= 40.0 && y < 480.0 {
                draw_row(
                    r,
                    &mut self.art,
                    &self.visual_assets,
                    row,
                    &row.visual.queue_mark,
                    320.0,
                    y,
                    320.0,
                    h,
                    self.preview_focus && i == self.preview_selected,
                    false,
                    false,
                    i,
                    None,
                    th,
                    self.visual_menu.selection_since,
                    self.visual_time,
                    self.audio_snapshot.playing,
                );
            }
            y += h;
        }
        r.pop_clip();
    }

    fn visual_menu_mini(&mut self, r: &mut Renderer, page: &Page, x: f32) {
        let wanted = self.visual_menu_mini_wanted(page);
        self.visual_menu_mini_with_wanted(r, page, x, wanted);
    }
    fn visual_menu_mini_with_wanted(
        &mut self,
        r: &mut Renderer,
        page: &Page,
        x: f32,
        wanted: bool,
    ) {
        let now = self.visual_time;
        let kind = if page.split() { 0 } else { 1 };
        self.visual_menu.mini_show[kind].to(
            if wanted { 1.0 } else { 0.0 },
            0.3,
            Curve::Css,
            0.0,
            now,
        );
        let shown = self.visual_menu.mini_show[kind].value(now);
        let Some(track) = self.visual_menu.mini_track.clone() else {
            return;
        };
        if shown <= 0.001 {
            return;
        }
        self.visual_menu.last_drawn = now;
        let width = if page.split() { 320.0 } else { 640.0 };
        let y = 364.0 + (116.0 * (1.0 - shown)).round();
        r.push_clip(x, y, width, 480.0 - y);
        if let Some(woven) = self.visual_assets.get("woven") {
            for dx in (0..width as usize).step_by(256) {
                let w = (width - dx as f32).min(256.0);
                r.image_uv(
                    woven,
                    [x + dx as f32, y, w, 116.0],
                    [0.0, 0.0, w / 256.0, 116.0 / 256.0],
                    1.0,
                );
            }
        } else {
            r.rect(x, y, width, 116.0, rgb(44, 48, 60));
        }
        r.rect(x, y, width, 2.0, rgb(8, 9, 12));
        r.rect(x, y + 2.0, width, 1.0, [1.0, 1.0, 1.0, 0.12]);
        let confirm = go_ease_in_out(self.visual_menu.ask.value(now) as f32);
        if confirm > 0.001 {
            if confirm < 0.999 {
                self.visual_menu_mini_content(
                    r,
                    &track,
                    x,
                    y - 116.0 * confirm,
                    width,
                    0.0,
                    1.0 - confirm,
                    true,
                    false,
                );
            }
            draw_mini_confirm(
                r,
                x,
                y + 116.0 * (1.0 - confirm),
                width,
                self.visual_menu.ask_selected,
            );
            r.pop_clip();
            return;
        }
        let phase = self.visual_menu.mini_change.value(now);
        let old = self.visual_menu.mini_old.clone();
        if phase < 1.0 && old.is_some() {
            let old = old.unwrap();
            let out = go_ease_in_out((phase / 0.45).min(1.0));
            let incoming = go_ease_in_out(((phase - 0.45) / 0.55).max(0.0));
            let shift = 52.0 * self.visual_menu.mini_direction;
            let same = old.album == track.album && old.album_artist == track.album_artist;
            if same {
                self.visual_menu_mini_cover(r, &track, x, y, 0.0, 1.0);
            }
            self.visual_menu_mini_content(
                r,
                &old,
                x,
                y,
                width,
                -shift * out,
                1.0 - out,
                !same,
                false,
            );
            self.visual_menu_mini_content(
                r,
                &track,
                x,
                y,
                width,
                shift * (1.0 - incoming),
                incoming,
                !same,
                false,
            );
        } else {
            self.visual_menu_mini_content(r, &track, x, y, width, 0.0, 1.0, true, true);
        }
        r.pop_clip();
    }

    fn visual_menu_mini_cover(
        &mut self,
        r: &mut Renderer,
        track: &Track,
        x: f32,
        y: f32,
        dx: f32,
        opacity: f32,
    ) {
        if opacity <= 0.002 {
            return;
        }
        let paused = self.visual_menu.mini_pause.value(self.visual_time);
        let size = (80.0 * (1.0 - 0.14 * paused)).round();
        let ax = (x + dx + 14.0 + (80.0 - size) * 0.5).round();
        let ay = (y + (116.0 - size) * 0.5 + 1.0).round();
        r.rect(
            ax - 2.0,
            ay - 2.0,
            size + 4.0,
            size + 4.0,
            [1.0, 1.0, 1.0, 0.85 * opacity],
        );
        let texture = self.art.get(track.cover.as_deref());
        r.image(texture, ax, ay, size, size, opacity);
        if paused > 0.01 {
            r.rect(ax, ay, size, size, [0.0, 0.0, 0.0, 0.42 * paused * opacity]);
            for bx in [ax + size * 0.5 - 10.0, ax + size * 0.5 + 3.0] {
                r.rounded_rect(
                    bx,
                    ay + size * 0.5 - 13.0,
                    7.0,
                    26.0,
                    2.4,
                    [1.0, 1.0, 1.0, paused * opacity],
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visual_menu_mini_content(
        &mut self,
        r: &mut Renderer,
        track: &Track,
        x: f32,
        y: f32,
        width: f32,
        dx: f32,
        opacity: f32,
        with_art: bool,
        live: bool,
    ) {
        if with_art {
            self.visual_menu_mini_cover(r, track, x, y, dx, opacity);
        }
        if opacity <= 0.002 {
            return;
        }
        let left = x + 102.0;
        r.push_clip(left, y, (x + width - left).max(0.0), 480.0 - y);
        let text_x = x + dx + 108.0;
        let text_width = width - 124.0;
        let lines = [
            (&track.title, 24.0, 700, 1.0),
            (&track.artist, 22.0, 600, 0.86),
            (&track.album, 22.0, 500, 0.86),
        ];
        let total: f32 = lines.iter().map(|(_, size, _, _)| line_box(*size).0).sum();
        let mut baseline_y = y + (116.0 - total) * 0.5 + 1.0;
        let mut cycle: f64 = 0.0;
        for (text, size, weight, _) in lines {
            let text_length = r.text_width_weighted(text, size, weight);
            if text_length > text_width {
                cycle = cycle.max((text_length as f64 + 80.0) / 64.0 + 1.5);
            }
        }
        for (text, size, weight, opacity_scale) in lines {
            let (height, ascent) = line_box(size);
            let baseline = baseline_y + ascent;
            let scrolling = live && self.visual_menu.mini_pause.value(self.visual_time) < 0.5;
            let white = [1.0, 1.0, 1.0, opacity * opacity_scale];
            let shadow = [0.0, 0.0, 0.0, 0.35 * opacity];
            if scrolling && r.text_width_weighted(text, size, weight) > text_width {
                marquee(
                    r,
                    text,
                    text_x,
                    baseline + 1.0,
                    size,
                    weight,
                    shadow,
                    text_width,
                    self.visual_menu.mini_since,
                    self.visual_time,
                    cycle,
                );
                marquee(
                    r,
                    text,
                    text_x,
                    baseline,
                    size,
                    weight,
                    white,
                    text_width,
                    self.visual_menu.mini_since,
                    self.visual_time,
                    cycle,
                );
            } else {
                let text = ellipsize(r, text, size, weight, text_width);
                r.text_baseline(
                    &text,
                    text_x,
                    baseline + 1.0,
                    size,
                    weight,
                    shadow,
                    text_width,
                );
                r.text_baseline(&text, text_x, baseline, size, weight, white, text_width);
            }
            baseline_y += height;
        }
        r.pop_clip();
    }
}

pub(super) fn is_menu(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::Home
            | Screen::Music
            | Screen::Albums
            | Screen::Artists
            | Screen::Artist(_)
            | Screen::Album(_)
            | Screen::Songs
            | Screen::Queue
            | Screen::Settings
            | Screen::Apps
            | Screen::Search
            | Screen::SearchResults(_)
            | Screen::Puzzles
            | Screen::GameClocks
            | Screen::GameHistory
            | Screen::Library
            | Screen::Screensaver
            | Screen::SearchHub
            | Screen::YouTubeSettings
    )
}

fn draw_mini_confirm(r: &mut Renderer, x: f32, y: f32, width: f32, selected: usize) {
    use super::app_feedback::rounded_gradient;
    let heading = "Clear Now Playing?";
    let tw = r.text_width_weighted(heading, 24.0, 700);
    let left = x + (width - tw) * 0.5;
    let baseline = y + 14.0 + line_box(24.0).1;
    r.text_baseline(
        heading,
        left,
        baseline + 1.0,
        24.0,
        700,
        [0.0, 0.0, 0.0, 0.35],
        tw + 1.0,
    );
    r.text_baseline(heading, left, baseline, 24.0, 700, WHITE, tw + 1.0);
    let button_width = (width - 40.0 - 12.0) * 0.5;
    let button_y = y + 56.0;
    for (i, label) in ["Keep", "Clear"].iter().enumerate() {
        let x0 = x + 20.0 + i as f32 * (button_width + 12.0);
        let (top, bottom, color) = if selected == i && i == 1 {
            (rgb(255, 107, 94), rgb(212, 42, 30), WHITE)
        } else if selected == i {
            (WHITE, rgb(214, 218, 224), rgb(30, 32, 36))
        } else {
            (
                [1.0, 1.0, 1.0, 0.18],
                [1.0, 1.0, 1.0, 0.08],
                if i == 1 { rgb(255, 138, 128) } else { WHITE },
            )
        };
        rounded_gradient(
            r,
            x0,
            button_y,
            button_width,
            44.0,
            22.0,
            &[(0.0, top), (1.0, bottom)],
        );
        let tw = r.text_width_weighted(label, 22.0, 700);
        r.text_baseline(
            label,
            x0 + (button_width - tw) * 0.5,
            button_y + 22.0 + 22.0 * 0.727 * 0.5,
            22.0,
            700,
            color,
            button_width,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    r: &mut Renderer,
    art: &mut Artwork,
    assets: &VisualAssets,
    row: &Row,
    queue_mark: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    selected: bool,
    inactive: bool,
    track_list: bool,
    index: usize,
    current: Option<usize>,
    th: MenuTheme,
    since: f64,
    now: f64,
    playing: bool,
) {
    if row.visual.search_field {
        draw_search_field(r, row, x, y, width, selected, th.bg[0] < 0.5);
        return;
    }
    if row.visual.header {
        r.gradient(x, y, width, height, th.header_top, th.header_bottom);
        r.rect(x, y, width, 2.0, th.header_border);
        r.rect(x, y + height - 2.0, width, 2.0, th.header_border);
        r.text_baseline(
            &row.label,
            x + 24.0,
            y + (height - line_box(24.0).1) * 0.5 + line_box(24.0).1 - 2.0,
            24.0,
            700,
            th.header_text,
            width - 48.0,
        );
        return;
    }
    if track_list && index % 2 == 1 && !selected {
        r.rect(x, y, width, height, th.stripe);
    }
    if selected {
        r.gradient(
            x,
            y,
            width,
            height,
            if inactive {
                rgb(168, 176, 186)
            } else {
                rgb(60, 184, 255)
            },
            if inactive {
                rgb(122, 131, 142)
            } else {
                rgb(52, 122, 181)
            },
        );
    }
    let color = if selected { WHITE } else { th.text };
    let sub = if selected { WHITE } else { th.sub };
    if row.visual.progress > 0.0 {
        let title = ellipsize(r, &row.label, 28.0, 700, width - 48.0);
        let detail = ellipsize(r, &row.detail, 24.0, 400, width - 48.0);
        r.text_baseline(
            &title,
            x + 24.0,
            y + 16.0 + line_box(28.0).1,
            28.0,
            700,
            color,
            width - 48.0,
        );
        r.text_baseline(
            &detail,
            x + 24.0,
            y + 54.0 + line_box(24.0).1,
            24.0,
            400,
            sub,
            width - 48.0,
        );
        if let Some(track) = assets.get("gloss_track") {
            r.image_uv(
                track,
                [x + 24.0, y + 92.0, width - 48.0, 32.0],
                [0.0, 0.0, (width - 48.0) / 800.0, 1.0],
                1.0,
            );
        }
        if let Some(fill) = assets.get("gloss_blue") {
            let w = (width - 48.0) * row.visual.progress.clamp(0.0, 1.0);
            r.image_uv(
                fill,
                [x + 24.0, y + 92.0, w, 32.0],
                [0.0, 0.0, w / 800.0, 1.0],
                1.0,
            );
        }
        return;
    }
    let mut text_left = if track_list { 8.0 } else { 0.0 };
    if row.visual.thumbnail || !row.visual.icon.is_empty() || row.cover.is_some() {
        let texture = if row.visual.icon.is_empty() {
            Some(art.get(row.cover.as_deref()))
        } else {
            assets.get(&row.visual.icon)
        };
        if let Some(texture) = texture {
            r.image(texture, x, y + (height - 96.0) * 0.5, 96.0, 96.0, 1.0);
        }
        text_left = 104.0;
    }
    let arrow = selected && row.arrow && !row.visual.no_arrow;
    let checked = row.visual.checked;
    let mut right = width - if arrow { 48.0 } else { 0.0 } - if checked { 44.0 } else { 0.0 };
    let label_width = r.text_width_weighted(&row.label, 28.0, 700);
    let mut value_width = 0.0;
    if !row.visual.value.is_empty() {
        value_width = r.text_width_weighted(&row.visual.value, 28.0, 400) + 24.0;
        let room = right - text_left - label_width - 32.0;
        if value_width > room {
            value_width = room;
            if value_width < 80.0 {
                value_width = 0.0;
            }
        }
        right -= value_width;
    }
    let track = match row.intent {
        Intent::Song(index) | Intent::AddNext(index) | Intent::AddLater(index) => Some(index),
        _ => None,
    };
    let mark = track_list && track.is_some() && track == current;
    let mut duration_width = 0.0;
    if track_list && !row.visual.duration.is_empty() {
        duration_width = r.text_width_weighted(&row.visual.duration, 28.0, 600) + 20.0;
        right -= duration_width;
    }
    if mark {
        text_left += 28.0;
    }
    let speaker = !track_list && !selected && track.is_some() && track == current;
    if speaker {
        right -= 48.0;
    }
    let queue_left = text_left;
    if !queue_mark.is_empty() {
        text_left += 56.0;
    }
    let pad_right = if arrow || !row.visual.value.is_empty() || checked || duration_width > 0.0 {
        0.0
    } else {
        32.0
    };
    let text_width = (right - text_left - pad_right - 16.0).max(0.0);
    let (label_height, label_ascent) = line_box(28.0);
    let block = 16.0
        + label_height
        + if row.detail.is_empty() {
            0.0
        } else {
            line_box(24.0).0
        };
    let text_y = y + (height - block) * 0.5;
    let baseline = text_y + 8.0 + label_ascent;
    if !queue_mark.is_empty() {
        let qx = x + queue_left + 12.0;
        let qy = text_y + 8.0 + label_ascent * 0.62;
        super::app_feedback::queue_icon(r, qx, qy, queue_mark == "queue_next", color);
    }
    if selected && label_width > text_width {
        // The outer list clip is restored immediately below.
        r.push_clip(x + text_left + 8.0, y, text_width, height);
        marquee(
            r,
            &row.label,
            x + text_left + 8.0,
            baseline,
            28.0,
            700,
            color,
            text_width,
            since,
            now,
            0.0,
        );
        r.pop_clip();
    } else {
        let label = ellipsize(r, &row.label, 28.0, 700, text_width);
        r.text_baseline(
            &label,
            x + text_left + 8.0,
            baseline,
            28.0,
            700,
            color,
            text_width,
        );
    }
    if !row.detail.is_empty() {
        let detail = if row.visual.typing {
            betterpod_rust::inline_keys::caret_fit(r, &row.detail, 24.0, 400, text_width)
        } else {
            betterpod_rust::inline_keys::ellipsize(r, &row.detail, 24.0, 400, text_width)
        };
        betterpod_rust::inline_keys::draw(
            r,
            &detail,
            x + text_left + 8.0,
            text_y + label_height + 8.0 + line_box(24.0).1,
            24.0,
            400,
            sub,
            text_width,
        );
    }
    let mut rx = width - if arrow { 48.0 } else { 32.0 };
    if checked {
        rx -= 28.0;
        if let Some(check) = assets.get(if selected || th.bg[0] < 0.5 {
            "check_white"
        } else {
            "check"
        }) {
            r.image(check, x + rx, y + (height - 28.0) * 0.5, 28.0, 28.0, 1.0);
        }
        rx -= 16.0;
    }
    if value_width > 0.0 {
        let value = ellipsize(r, &row.visual.value, 28.0, 400, value_width - 24.0);
        let tw = r.text_width_weighted(&value, 28.0, 400);
        r.text_baseline(
            &value,
            x + rx - tw,
            baseline,
            28.0,
            400,
            sub,
            value_width - 24.0,
        );
    }
    if duration_width > 0.0 {
        let tw = r.text_width_weighted(&row.visual.duration, 28.0, 600);
        r.text_baseline(
            &row.visual.duration,
            x + width - 16.0 - tw,
            baseline,
            28.0,
            600,
            color,
            duration_width,
        );
    }
    if mark {
        let mx = x + text_left - 28.0 + 10.0;
        let cy = baseline - 10.0;
        if playing {
            r.triangle([[mx, cy - 9.0], [mx, cy + 9.0], [mx + 15.2, cy]], color);
        } else {
            r.rect(mx, cy - 9.0, 5.2, 18.0, color);
            r.rect(mx + 10.0, cy - 9.0, 5.2, 18.0, color);
        }
    }
    if arrow {
        if let Some(arrow) = assets.get("arrow_right") {
            r.image(
                arrow,
                x + width - 48.0,
                y + (height - 48.0) * 0.5,
                48.0,
                48.0,
                1.0,
            );
        }
    }
    if speaker {
        if let Some(speaker) = assets.get("volume_full_16") {
            r.image(
                speaker,
                x + width - 40.0,
                y + (height - 32.0) * 0.5,
                32.0,
                32.0,
                1.0,
            );
        }
    }
}

fn draw_search_field(
    r: &mut Renderer,
    row: &Row,
    x: f32,
    y: f32,
    width: f32,
    selected: bool,
    dark: bool,
) {
    use super::app_feedback::{rounded_gradient, stroke_rounded};
    let (top, bottom, line, field, edge, text, dim) = if dark {
        (
            rgb(51, 54, 60),
            rgb(38, 40, 45),
            rgb(20, 21, 24),
            rgb(27, 28, 32),
            rgb(74, 78, 86),
            rgb(236, 237, 240),
            rgb(120, 124, 130),
        )
    } else {
        (
            rgb(221, 226, 232),
            rgb(195, 202, 211),
            rgb(165, 173, 184),
            WHITE,
            rgb(154, 163, 174),
            rgb(20, 22, 26),
            rgb(150, 155, 162),
        )
    };
    r.gradient(x, y, width, 92.0, top, bottom);
    r.rect(x, y + 91.0, width, 1.0, line);
    let left = x + 16.0;
    let right = x + width - 16.0;
    let cy = y + 46.0;
    if selected {
        rounded_gradient(
            r,
            left - 5.0,
            y + 11.0,
            right - left + 10.0,
            70.0,
            35.0,
            &[
                (0.0, [90.0 / 255.0, 170.0 / 255.0, 1.0, 0.9]),
                (1.0, [40.0 / 255.0, 120.0 / 255.0, 220.0 / 255.0, 0.9]),
            ],
        );
    }
    r.rounded_rect(left, y + 16.0, right - left, 60.0, 30.0, field);
    rounded_gradient(
        r,
        left,
        y + 16.0,
        right - left,
        10.0,
        30.0,
        &[(0.0, [0.0, 0.0, 0.0, 0.10]), (1.0, [0.0, 0.0, 0.0, 0.0])],
    );
    stroke_rounded(r, left, y + 16.0, right - left, 60.0, 30.0, 1.5, edge);
    super::app_keyboard::draw_magnifier(r, left + 20.0, cy, 26.0, dim);
    let mut text_x = left + 60.0;
    let mut text_right = right - 24.0;
    if !row.visual.value.is_empty() {
        let tw = r.text_width_weighted(&row.visual.value, 24.0, 500);
        r.text_baseline(
            &row.visual.value,
            text_right - tw,
            cy + 24.0 * 0.727 * 0.5,
            24.0,
            500,
            dim,
            tw + 1.0,
        );
        text_right -= tw + 20.0;
    }
    if row.label.is_empty() || row.label == "|" {
        if row.label == "|" {
            let tw = r.text_width_weighted("|", 28.0, 500);
            r.text_baseline(
                "|",
                text_x,
                cy + 28.0 * 0.727 * 0.5,
                28.0,
                500,
                text,
                tw + 1.0,
            );
            text_x += tw + 4.0;
        }
        let placeholder = ellipsize(r, &row.visual.placeholder, 28.0, 500, text_right - text_x);
        r.text_baseline(
            &placeholder,
            text_x,
            cy + 28.0 * 0.727 * 0.5,
            28.0,
            500,
            dim,
            text_right - text_x,
        );
        return;
    }
    let mut label = row.label.as_str();
    while r.text_width_weighted(label, 28.0, 500) > text_right - text_x && label.chars().count() > 1
    {
        label = &label[label.chars().next().unwrap().len_utf8()..];
        let shortened = format!("…{label}");
        if r.text_width_weighted(&shortened, 28.0, 500) <= text_right - text_x {
            r.text_baseline(
                &shortened,
                text_x,
                cy + 28.0 * 0.727 * 0.5,
                28.0,
                500,
                text,
                text_right - text_x,
            );
            return;
        }
    }
    r.text_baseline(
        label,
        text_x,
        cy + 28.0 * 0.727 * 0.5,
        28.0,
        500,
        text,
        text_right - text_x,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_header_bar(
    r: &mut Renderer,
    x: f32,
    width: f32,
    off: f32,
    level: f32,
    brightness: bool,
    headphones: bool,
    color: Color,
) {
    use super::app_feedback::rounded_gradient;
    if brightness {
        let (cx, cy) = (x + 14.0, 20.0 + off);
        r.circle(cx, cy, 5.2, color);
        for i in 0..8 {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            r.circle(cx + 9.2 * a.cos(), cy + 9.2 * a.sin(), 1.5, color);
        }
    } else {
        draw_volume_icon(r, x, 6.0 + off, headphones, color);
    }
    let bar_x = x + 42.0;
    let bar_width = (width - 54.0).min(300.0);
    if bar_width < 60.0 {
        return;
    }
    rounded_gradient(
        r,
        bar_x,
        15.0 + off,
        bar_width,
        10.0,
        5.0,
        &[(0.0, [0.0, 0.0, 0.0, 0.22]), (1.0, [0.0, 0.0, 0.0, 0.12])],
    );
    if brightness || level > 0.001 {
        let filled = (bar_width * level.clamp(0.0, 1.0)).max(10.0);
        rounded_gradient(
            r,
            bar_x,
            15.0 + off,
            filled,
            10.0,
            5.0,
            &[(0.0, rgb(60, 184, 255)), (1.0, rgb(52, 122, 181))],
        );
    }
}
pub(super) fn draw_volume_icon(r: &mut Renderer, x: f32, y: f32, headphones: bool, color: Color) {
    if headphones {
        arc(r, x + 14.0, y + 16.0, 10.4, 180.0, 360.0, 3.2, color);
        r.rounded_rect(x + 2.2, y + 14.8, 6.4, 10.8, 2.4, color);
        r.rounded_rect(x + 19.4, y + 14.8, 6.4, 10.8, 2.4, color);
    } else {
        r.rect(x + 2.4, y + 10.0, 4.8, 8.0, color);
        r.triangle(
            [
                [x + 7.2, y + 10.0],
                [x + 14.0, y + 4.4],
                [x + 14.0, y + 23.6],
            ],
            color,
        );
        r.triangle(
            [
                [x + 7.2, y + 10.0],
                [x + 14.0, y + 23.6],
                [x + 7.2, y + 18.0],
            ],
            color,
        );
        arc(r, x + 15.0, y + 14.0, 5.2, -50.0, 50.0, 2.6, color);
        arc(r, x + 15.0, y + 14.0, 10.0, -50.0, 50.0, 2.6, color);
    }
}
#[allow(clippy::too_many_arguments)]
fn arc(
    r: &mut Renderer,
    x: f32,
    y: f32,
    radius: f32,
    start: f32,
    end: f32,
    line: f32,
    color: Color,
) {
    let points: Vec<_> = (0..=32)
        .map(|i| {
            let a = (start + (end - start) * i as f32 / 32.0).to_radians();
            [x + radius * a.cos(), y + radius * a.sin()]
        })
        .collect();
    aa_polyline(r, &points, line, color);
}

pub(super) fn ellipsize(
    r: &mut Renderer,
    text: &str,
    size: f32,
    weight: u16,
    max_width: f32,
) -> String {
    if max_width <= 0.0 {
        return String::new();
    }
    if r.text_width_weighted(text, size, weight) <= max_width {
        return text.to_owned();
    }
    let ellipsis = "…";
    let ew = r.text_width_weighted(ellipsis, size, weight);
    if ew > max_width {
        return String::new();
    }
    let mut ends: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    ends.push(text.len());
    let (mut lo, mut hi) = (0, ends.len() - 1);
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if r.text_width_weighted(&text[..ends[mid]], size, weight) + ew <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    while lo > 0 && text[..ends[lo]].ends_with(' ') {
        lo -= 1;
    }
    let mut out = String::with_capacity(ends[lo] + ellipsis.len());
    out.push_str(&text[..ends[lo]]);
    out.push_str(ellipsis);
    out
}

fn caret_fit(r: &mut Renderer, text: &str, size: f32, weight: u16, width: f32) -> String {
    if r.text_width_weighted(text, size, weight) <= width {
        return text.to_owned();
    }
    let Some(caret) = text.rfind('|') else {
        return ellipsize(r, text, size, weight, width);
    };
    if r.text_width_weighted(&format!("{}…", &text[..caret + 1]), size, weight) <= width {
        return ellipsize(r, text, size, weight, width);
    }
    let tail = if caret + 1 < text.len() { "…" } else { "" };
    let mut from = 0;
    while from < caret
        && r.text_width_weighted(
            &format!("…{}{}", &text[from..caret + 1], tail),
            size,
            weight,
        ) > width
    {
        from += text[from..].chars().next().unwrap().len_utf8();
    }
    ellipsize(r, &format!("…{}", &text[from..]), size, weight, width)
}

#[allow(clippy::too_many_arguments)]
fn marquee(
    r: &mut Renderer,
    text: &str,
    x: f32,
    baseline: f32,
    size: f32,
    weight: u16,
    color: Color,
    width: f32,
    since: f64,
    now: f64,
    shared_cycle: f64,
) {
    let length = r.text_width_weighted(text, size, weight);
    if length <= width {
        r.text_baseline(text, x, baseline, size, weight, color, width);
        return;
    }
    let loop_width = length + 80.0;
    let own = loop_width as f64 / 64.0 + 1.5;
    let cycle = shared_cycle.max(own);
    let t = (now - since).max(0.0) % cycle;
    let offset = if t <= 1.5 {
        0.0
    } else {
        ((t - 1.5) * 64.0).min(loop_width as f64) as f32
    };
    r.text_baseline(
        text,
        x - offset,
        baseline,
        size,
        weight,
        color,
        length + 1.0,
    );
    r.text_baseline(
        text,
        x - offset + loop_width,
        baseline,
        size,
        weight,
        color,
        length + 1.0,
    );
}

fn draw_scrollbar(
    r: &mut Renderer,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    scroll: f32,
    total: f32,
    dark: bool,
    opacity: f32,
) {
    let x0 = x + (width - 12.0) * 0.5 - 2.0;
    let y0 = y + 6.0;
    let y1 = y + height - 6.0;
    let (track, border, thumb) = if dark {
        (
            [rgb(38, 40, 44), rgb(48, 50, 55)],
            rgb(69, 72, 78),
            [rgb(163, 166, 172), rgb(134, 137, 144), rgb(114, 117, 123)],
        )
    } else {
        (
            [rgb(228, 228, 228), rgb(247, 247, 247)],
            rgb(180, 180, 180),
            [rgb(168, 168, 168), rgb(138, 138, 138), rgb(118, 118, 118)],
        )
    };
    r.rounded_rect(x0, y0, 12.0, y1 - y0, 6.0, alpha(border, opacity));
    r.rounded_rect(
        x0 + 1.0,
        y0 + 1.0,
        10.0,
        y1 - y0 - 2.0,
        5.0,
        alpha(track[0], opacity),
    );
    let h = ((y1 - y0) * height / total).max(28.0);
    let fraction = if total > height {
        (scroll / (total - height)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let ty = y0 + (y1 - y0 - h) * fraction;
    // Ten one-pixel strips reproduce the original horizontal glossy gradient;
    // rounded endcaps are composited from the same three-stop colors.
    r.rounded_rect(x0 + 1.0, ty, 10.0, h, 5.0, alpha(thumb[1], opacity));
    for i in 1..9 {
        let t = i as f32 / 9.0;
        let (a, b, u) = if t < 0.5 {
            (thumb[0], thumb[1], t * 2.0)
        } else {
            (thumb[1], thumb[2], (t - 0.5) * 2.0)
        };
        let color = std::array::from_fn(|j| a[j] + (b[j] - a[j]) * u);
        r.rect(
            x0 + 1.0 + i as f32,
            ty + 5.0,
            1.0,
            (h - 10.0).max(0.0),
            alpha(color, opacity),
        );
    }
}

fn put_icon(
    r: &mut Renderer,
    texture: Texture,
    right: f32,
    y: f32,
    tint: Color,
    ink: (f32, f32),
) -> f32 {
    let width = texture.width as f32;
    r.image_tinted(
        texture,
        [right + ink.1 - width, y, width, texture.height as f32],
        [0.0, 0.0, 1.0, 1.0],
        tint,
    );
    right + ink.1 - width + ink.0 - 10.0
}
fn draw_wifi(r: &mut Renderer, x: f32, y: f32, arcs: usize, color: Color) {
    r.rounded_rect(x + 15.2, y + 17.5, 5.6, 5.6, 2.8, color);
    for (i, radius) in [7.7, 13.2, 18.7].iter().copied().enumerate() {
        let color = if i < arcs { color } else { alpha(color, 0.3) };
        let points: Vec<_> = (0..=24)
            .map(|sample| {
                let angle = (-135.0 + 90.0 * sample as f32 / 24.0).to_radians();
                [
                    x + 18.0 + radius * angle.cos(),
                    y + 20.3 + radius * angle.sin(),
                ]
            })
            .collect();
        aa_polyline(r, &points, 3.2, color);
    }
}
fn draw_headset(r: &mut Renderer, x: f32, y: f32, wifi: i8, bluetooth: bool, color: Color) {
    let cy = if wifi >= 0 { 9.5 } else { 11.8 };
    let (cx, s) = (15.0, 19.0);
    let w = s * 1.05;
    let points: Vec<_> = (0..=24)
        .map(|sample| {
            let angle = (180.0 + 180.0 * sample as f32 / 24.0).to_radians();
            [
                x + cx + (w * 0.5 - s * 0.07) * angle.cos(),
                y + cy + s * 0.025 + (s * 0.525 - s * 0.07) * angle.sin(),
            ]
        })
        .collect();
    aa_polyline(r, &points, s * 0.14, color);
    let cw = s * 0.30;
    let ch = s * 0.46;
    let yb = y + cy + s * 0.05;
    for cup_x in [x + cx - w * 0.5 - cw * 0.15, x + cx + w * 0.5 - cw * 0.85] {
        r.rounded_rect(cup_x, yb - ch * 0.1, cw, ch, cw * 0.35, color);
    }
    if bluetooth {
        let bw = s * 0.16;
        let bh = s * 0.56;
        let by = y + cy + s * 0.1;
        let pts = [
            (cx - bw, by - bh * 0.27),
            (cx + bw, by + bh * 0.25),
            (cx, by + bh * 0.5),
            (cx, by - bh * 0.5),
            (cx + bw, by - bh * 0.25),
            (cx - bw, by + bh * 0.27),
        ];
        aa_polyline(r, &pts.map(|p| [x + p.0, p.1]), s * 0.085, color);
    }
    if wifi >= 0 {
        for i in 0..5 {
            r.rounded_rect(
                x + cx + (i as f32 - 2.0) * 5.2 - 1.6,
                y + 21.4 - 1.6,
                3.2,
                3.2,
                1.6,
                if i < wifi as usize {
                    color
                } else {
                    alpha(color, 0.25)
                },
            );
        }
    }
}
fn draw_lock(r: &mut Renderer, x: f32, y: f32, color: Color) {
    r.stroke_rounded(x + 4.5, y + 2.5, 7.0, 10.2, 3.5, 2.6, color);
    r.rounded_rect(x, y + 9.6, 16.0, 12.4, 2.8, color);
}

/// Native GPU stroke: a continuous mesh avoids the stair-step edges of butt
/// line segments. Coverage fringes and round caps retain Go's original paths.
pub(super) fn aa_polyline(r: &mut Renderer, points: &[[f32; 2]], width: f32, color: Color) {
    if points.len() < 2 || width <= 0.0 {
        return;
    }
    let mut rings = Vec::with_capacity(points.len());
    for (i, p) in points.iter().enumerate() {
        let before = points[i.saturating_sub(1)];
        let after = points[(i + 1).min(points.len() - 1)];
        let (dx, dy) = (after[0] - before[0], after[1] - before[1]);
        let length = dx.hypot(dy).max(1e-6);
        let (nx, ny) = (-dy / length, dx / length);
        let next = if i + 1 < points.len() {
            points[i + 1]
        } else {
            *p
        };
        let previous = if i + 1 < points.len() {
            *p
        } else {
            points[i - 1]
        };
        let (sx, sy) = (next[0] - previous[0], next[1] - previous[1]);
        let segment = sx.hypot(sy).max(1e-6);
        let miter = (nx * (-sy / segment) + ny * (sx / segment))
            .abs()
            .max(0.5)
            .recip();
        let inside = (width * 0.5 - 0.5).max(0.0) * miter;
        let outside = (width * 0.5 + 0.5) * miter;
        rings.push([
            [p[0] + nx * inside, p[1] + ny * inside],
            [p[0] - nx * inside, p[1] - ny * inside],
            [p[0] + nx * outside, p[1] + ny * outside],
            [p[0] - nx * outside, p[1] - ny * outside],
        ]);
    }
    let mut clear = color;
    clear[3] = 0.0;
    for pair in rings.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        r.triangle([a[0], b[0], b[1]], color);
        r.triangle([a[0], b[1], a[1]], color);
        for side in 0..2 {
            r.colored_triangle([a[side], b[side], b[side + 2]], [color, color, clear]);
            r.colored_triangle([a[side], b[side + 2], a[side + 2]], [color, clear, clear]);
        }
    }
    for (index, other, start) in [(0, 1, true), (points.len() - 1, points.len() - 2, false)] {
        let p = points[index];
        let q = points[other];
        let direction = if start {
            (q[1] - p[1]).atan2(q[0] - p[0])
        } else {
            (p[1] - q[1]).atan2(p[0] - q[0])
        };
        let begin = direction
            + if start {
                std::f32::consts::FRAC_PI_2
            } else {
                -std::f32::consts::FRAC_PI_2
            };
        for step in 0..12 {
            let a = begin + step as f32 * std::f32::consts::PI / 12.0;
            let b = begin + (step + 1) as f32 * std::f32::consts::PI / 12.0;
            let at = |angle: f32, radius: f32| {
                [p[0] + angle.cos() * radius, p[1] + angle.sin() * radius]
            };
            let ia = at(a, (width * 0.5 - 0.5).max(0.0));
            let ib = at(b, (width * 0.5 - 0.5).max(0.0));
            let oa = at(a, width * 0.5 + 0.5);
            let ob = at(b, width * 0.5 + 0.5);
            r.triangle([p, ia, ib], color);
            r.colored_triangle([ia, ib, ob], [color, color, clear]);
            r.colored_triangle([ia, ob, oa], [color, clear, clear]);
        }
    }
}
// Abramowitz-Stegun erfc approximation; absolute error < 1.5e-7.
fn erfc(x: f32) -> f32 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let erf = 1.0
        - (((((1.0614054 * t - 1.4531521) * t + 1.4214138) * t - 0.28449672) * t + 0.2548296) * t)
            * (-z * z).exp();
    if x >= 0.0 {
        1.0 - erf
    } else {
        1.0 + erf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn page(screen: Screen, kind: ViewKind) -> Page {
        Page {
            screen,
            kind,
            mini: false,
            title: format!("{screen:?}"),
            rows: Arc::new(vec![Row::new("Row", Intent::Noop)]),
            selected: 0,
            scroll: 0.0,
            target_scroll: 0.0,
            enter: 0.0,
        }
    }
    fn body(view: &ViewMotion, now: f64) -> f32 {
        anim_value(&view.enter, now) + anim_value(&view.hide, now)
    }
    #[test]
    fn immutable_prefix_heights_are_reused_for_selection_and_drawing() {
        let plain = Row::new("Plain", Intent::Noop);
        let mut subtitle = Row::new("Subtitle", Intent::Noop);
        subtitle.detail = "Artist".into();
        let mut cover = Row::new("Cover", Intent::Noop);
        cover.cover = Some(PathBuf::from("/synthetic/cover.png"));
        let mut header = Row::new("Header", Intent::Noop);
        header.visual.header = true;
        let rows = Arc::new(vec![plain, subtitle, cover, header]);
        let mut visual = MenuVisual::new();
        let first = visual.layout_index(&rows);
        assert_eq!(
            visual.layouts[first].tops,
            vec![0.0, 50.0, 130.0, 226.0, 270.0]
        );
        let pointer = visual.layouts[first].tops.as_ptr();
        for _ in 0..100 {
            assert_eq!(visual.layout_index(&rows), first);
            assert_eq!(visual.layouts[first].tops.as_ptr(), pointer);
        }
        let mut changed = Arc::clone(&rows);
        Arc::make_mut(&mut changed)[1].detail.clear();
        let next = visual.layout_index(&changed);
        assert_ne!(next, first);
        assert_eq!(visual.layouts[next].tops[2], 100.0);
        assert_eq!(visual.layouts.len(), 2);
    }
    #[test]
    fn expired_search_rows_are_released_and_mutation_rebuilds_heights() {
        let mut visual = MenuVisual::new();
        let old_results = Arc::new(vec![Row::new("Old search result", Intent::Noop)]);
        let old = Arc::downgrade(&old_results);
        visual.layout_index(&old_results);
        assert_eq!(Arc::strong_count(&old_results), 1);
        drop(old_results);
        assert!(
            old.upgrade().is_none(),
            "height cache retained old result rows"
        );

        let mut results = Arc::new(vec![Row::new("New search result", Intent::Noop)]);
        let index = visual.layout_index(&results);
        assert_eq!(visual.layouts.len(), 1);
        assert_eq!(visual.layouts[index].tops, vec![0.0, 50.0]);
        let before_mutation = Arc::downgrade(&results);
        // Only weak cache references remain. make_mut must invalidate that
        // identity even though no second strong Arc forced a row clone.
        Arc::make_mut(&mut results)[0].detail = "A second line".into();
        assert!(before_mutation.upgrade().is_none());
        let index = visual.layout_index(&results);
        assert_eq!(visual.layouts.len(), 1);
        assert_eq!(visual.layouts[index].tops, vec![0.0, 80.0]);
    }
    #[test]
    fn live_prefix_heights_share_a_byte_budget() {
        let mut visual = MenuVisual::new();
        let rows = Arc::new(vec![Row::new("Retained page", Intent::Noop)]);
        // Simulate a fully used metric budget without allocating a large
        // synthetic library just to check cache eviction.
        visual.layouts.push(Layout {
            rows: Arc::downgrade(&rows),
            tops: vec![0.0; MAX_LAYOUT_BYTES / std::mem::size_of::<f32>()],
        });
        let current = Arc::new(vec![Row::new("Current page", Intent::Noop)]);
        let index = visual.layout_index(&current);
        assert_eq!(visual.layouts.len(), 1);
        assert_eq!(visual.layouts[index].tops, vec![0.0, 50.0]);
        assert_eq!(Arc::strong_count(&rows), 1);
    }
    #[test]
    fn interrupted_nested_navigation_keeps_both_source_clocks() {
        let home = page(Screen::Home, ViewKind::Split);
        let music = page(Screen::Music, ViewKind::Split);
        let apps = page(Screen::Apps, ViewKind::Split);
        let mut visual = MenuVisual::new();
        visual.navigate(home, music.clone(), 1, true, 0.0);
        let music_id = visual.active_view.unwrap();
        let before = body(
            visual.views.iter().find(|v| v.id == music_id).unwrap(),
            0.150,
        );
        visual.navigate(music, apps, 2, true, 0.150);
        let retained = visual.views.iter().find(|v| v.id == music_id).unwrap();
        assert!((body(retained, 0.150) - before).abs() < 1e-6);
        // Its incoming InOut clock ends at300ms while the independent outgoing
        // CSS clock is only halfway through; neither is restarted at a full panel.
        assert!((body(retained, 0.300) + Curve::Css.sample(0.5) as f32).abs() < 1e-6);
        assert_eq!(visual.views.len(), 3);
    }
    #[test]
    fn backing_out_mid_entry_retargets_without_jumping_parent_or_child() {
        let home = page(Screen::Home, ViewKind::Split);
        let music = page(Screen::Music, ViewKind::Split);
        let mut visual = MenuVisual::new();
        visual.navigate(home.clone(), music.clone(), 1, true, 0.0);
        let before: Vec<_> = visual
            .views
            .iter()
            .map(|v| (v.id, body(v, 0.120)))
            .collect();
        visual.navigate(music, home, 0, false, 0.120);
        for (id, position) in before {
            assert!(
                (body(visual.views.iter().find(|v| v.id == id).unwrap(), 0.120) - position).abs()
                    < 1e-6
            );
        }
    }
    #[test]
    fn coverflow_preview_and_full_panel_reversals_sample_the_existing_pose() {
        let home = page(Screen::Home, ViewKind::Split);
        let flow = page(Screen::CoverFlow, ViewKind::CoverFlow);
        let mut visual = MenuVisual::new();
        visual.navigate(home.clone(), flow.clone(), 1, true, 0.0);
        let preview = anim_value(&visual.preview_hide, 0.175);
        let left = anim_value(&visual.left_panel, 0.175);
        visual.navigate(flow, home, 0, false, 0.175);
        assert!((anim_value(&visual.preview_hide, 0.175) - preview).abs() < 1e-6);
        assert!((anim_value(&visual.left_panel, 0.175) - left).abs() < 1e-6);
        assert_eq!(anim_value(&visual.preview_hide, 0.6), 0.0);
    }
    #[test]
    fn go_css_curve_and_line_metrics_are_not_rust_defaults() {
        assert!((go_ease_css(0.5) - 0.8024034).abs() < 0.00001);
        assert!((go_ease_in_out(0.5) - 0.5).abs() < 0.00001);
        assert_eq!(line_box(28.0), (34.0, 26.0));
        assert_eq!(line_box(24.0), (30.0, 23.0));
        assert_eq!(line_box(26.0), (32.0, 25.0));
    }
    #[test]
    fn timed_transition_retargets_without_a_jump() {
        let mut tween = Tween::new(0.0);
        tween.to(1.0, 0.3, Curve::Css, 0.0, 1.0);
        let halfway = tween.value(1.15);
        tween.to(0.0, 0.3, Curve::Css, 0.0, 1.15);
        assert!((tween.value(1.15) - halfway).abs() < 0.00001);
        assert_eq!(tween.value(2.0), 0.0);
    }
    #[test]
    fn menu_rows_use_original_variable_geometry() {
        let mut plain = Row::new("Plain", Intent::CloseOptions);
        assert_eq!(row_height(&plain), 50.0);
        plain.detail = "Subtitle".into();
        assert_eq!(row_height(&plain), 80.0);
        plain.cover = Some(PathBuf::from("cover"));
        assert_eq!(row_height(&plain), 96.0);
        plain.visual.progress = 0.2;
        assert_eq!(row_height(&plain), 136.0);
        let rows = [Row::new("First", Intent::CloseOptions), plain];
        assert_eq!(row_tops(&rows), vec![0.0, 50.0, 186.0]);
    }
}

impl App {
    pub(super) fn visual_menu_outgoing_page(&self) -> Option<(Page, f32, bool)> {
        self.visual_menu_outgoing_pose()
    }
    pub(super) fn visual_menu_outgoing_screen(&self) -> Option<Screen> {
        self.visual_menu_outgoing_page()
            .map(|(page, _, _)| page.screen)
    }
}
