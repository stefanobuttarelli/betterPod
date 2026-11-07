//! Foreground editor reads and curation writes retain their pressed source.
use super::*;

#[derive(Clone, Copy)]
enum Completion {
    ReadEditor,
    ServiceWrite,
}
#[derive(Clone, Copy)]
enum Source {
    List,
    AppSheet,
    CurationSheet,
}
pub(super) struct PendingAlbumRead {
    token: u64,
    screen: Screen,
    depth: usize,
    generation: u64,
    rows: Arc<Vec<Row>>,
    selected: usize,
    source: Source,
    completion: Completion,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let samples = vec![0u8; 160];
        let mut bytes = b"RIFF".to_vec();
        bytes.extend((36u32 + samples.len() as u32).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(8000u32.to_le_bytes());
        bytes.extend(16000u32.to_le_bytes());
        bytes.extend(2u16.to_le_bytes());
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend((samples.len() as u32).to_le_bytes());
        bytes.extend(samples);
        std::fs::write(path, bytes).unwrap();
    }
    fn select_sheet(app: &mut App, label: &str) {
        let target = app
            .options
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .position(|row| row.label == label)
            .unwrap();
        while app.options.as_ref().unwrap().selected < target {
            app.handle(Action::Down);
        }
        while app.options.as_ref().unwrap().selected > target {
            app.handle(Action::Up);
        }
        app.handle(Action::Select);
    }
    fn pump(app: &mut App, r: &Renderer, until: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !until(app) && Instant::now() < deadline {
            app.tick(1.0 / 60.0, r);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(
            until(app),
            "The local initial tag-read route did not settle"
        );
    }
    fn capture(app: &mut App, r: &mut Renderer, out: &std::path::Path, name: &str) {
        app.draw(r, app.visual_time);
        r.finish();
        r.screenshot(&out.join(format!("{name}.png"))).unwrap();
    }

    #[test]
    #[ignore = "requires local SDL/GLES fixtures; run with the existing audio-qa image and no network"]
    fn initial_album_info_input_retains_source_and_cancels_before_late_results() {
        let assets =
            PathBuf::from(std::env::var_os("BETTERPOD_RUST_ASSETS_DIR").expect("local assets"));
        let out = PathBuf::from(
            std::env::var_os("BETTERPOD_RUST_QA_OUTPUT").expect("local scratch output"),
        )
        .join(format!(
            "album-read-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let music = out.join("music");
        let first = music.join("Fixture Artist/Fixture Album/01 First.wav");
        wav(&first);
        wav(&music.join("Fixture Artist/Fixture Album/02 Second.wav"));
        wav(&music.join("Fixture Artist/Other Album/01 Third.wav"));
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let _audio = sdl.audio().unwrap();
        video
            .gl_attr()
            .set_context_profile(sdl2::video::GLProfile::GLES);
        video.gl_attr().set_context_version(2, 0);
        let window = video
            .window("Initial Album Info local check", 640, 480)
            .opengl()
            .build()
            .unwrap();
        let _context = window.gl_create_context().unwrap();
        let mut r = Renderer::new(&video, &assets.join("ui_font.ttf"), 640, 480).unwrap();
        let mut app = App::new(music, out.join("data"), assets, None, &r).unwrap();
        pump(&mut app, &r, |app| !app.scanning);
        assert_eq!(app.library.tracks.len(), 3);
        app.open(Screen::Albums);
        app.handle(Action::Options);
        assert!(app.options.is_some());
        for _ in 0..30 {
            app.tick(1.0 / 60.0, &r);
            app.draw(&mut r, app.visual_time);
            r.finish();
        }
        let rows = Arc::clone(&app.pages.last().unwrap().rows);
        let source_selected = app.pages.last().unwrap().selected;
        let source_scroll = app.pages.last().unwrap().scroll;
        let depth = app.pages.len();
        select_sheet(&mut app, "Edit");
        let sheet = app
            .options
            .clone()
            .expect("Edit must retain the pressed sheet while waiting");
        assert!(app.pending_album_read.is_some());
        assert_eq!(
            app.screen(),
            Screen::Albums,
            "No placeholder editor page may open before the read finishes"
        );
        assert_eq!(app.pages.len(), depth);
        assert_eq!(app.album_read_busy_row(&sheet), Some(sheet.selected));
        capture(&mut app, &mut r, &out, "01-edit-pressed-busy");
        for action in [
            Action::Down,
            Action::Up,
            Action::Left,
            Action::Right,
            Action::Select,
            Action::Options,
        ] {
            app.handle(action);
        }
        assert_eq!(app.options.as_ref().unwrap().selected, sheet.selected);
        assert!(Arc::ptr_eq(
            &app.options.as_ref().unwrap().rows,
            &sheet.rows
        ));
        app.handle(Action::Back);
        assert!(app.pending_album_read.is_none());
        assert!(!app.extras.initial_album_read_pending());
        assert_eq!(app.screen(), Screen::Albums);
        assert!(Arc::ptr_eq(&app.pages.last().unwrap().rows, &rows));
        assert!(Arc::ptr_eq(
            &app.options.as_ref().unwrap().rows,
            &sheet.rows
        ));
        assert_eq!(app.options.as_ref().unwrap().selected, sheet.selected);
        assert_eq!(app.pages.last().unwrap().selected, source_selected);
        assert_eq!(app.pages.last().unwrap().scroll, source_scroll);
        for _ in 0..35 {
            app.tick(1.0 / 60.0, &r);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            app.screen(),
            Screen::Albums,
            "A cancelled read's late completion must not navigate"
        );
        capture(&mut app, &mut r, &out, "02-edit-cancelled-same-sheet");
        app.handle(Action::Back);
        assert!(
            app.options.is_none(),
            "The next Back closes the sheet normally"
        );
        app.handle(Action::Options);
        select_sheet(&mut app, "Edit");
        pump(&mut app, &r, |app| app.screen() == Screen::Extra);
        assert!(app.pending_album_read.is_none());
        assert!(app.options.is_none());
        assert!(!app.extras.initial_album_read_pending());
        assert!(!app
            .extras
            .visual_page()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.label.contains("Loading album information")));
        for _ in 0..30 {
            app.tick(1.0 / 60.0, &r);
            app.draw(&mut r, app.visual_time);
            r.finish();
        }
        capture(&mut app, &mut r, &out, "03-editor-ready");
        app.handle(Action::Back);
        pump(&mut app, &r, |app| app.screen() == Screen::Albums);
        assert_eq!(app.pages.len(), depth);
        assert!(Arc::ptr_eq(&app.pages.last().unwrap().rows, &rows));
        app.open(Screen::Songs);
        let source = app.pages.last().unwrap().clone();
        app.execute(Intent::SongInfo(0));
        assert_eq!(app.screen(), Screen::Songs);
        assert!(app.album_read_row_busy(&source, source.selected));
        app.handle(Action::Back);
        assert_eq!(app.screen(), Screen::Songs);
        assert!(Arc::ptr_eq(&app.pages.last().unwrap().rows, &source.rows));
        assert_eq!(app.pages.last().unwrap().selected, source.selected);
        let original_audio = std::fs::read(&first).unwrap();
        app.execute(Intent::SeparateSong(0));
        assert_eq!(
            app.screen(),
            Screen::Songs,
            "A separate job must not open an Extra placeholder"
        );
        assert!(app.pending_album_read.is_some());
        assert!(
            !app.handle_pending_album_read(Action::Start),
            "Start remains the audio play/pause control"
        );
        assert!(!app.handle_pending_album_read(Action::PlayPause));
        app.handle(Action::Back);
        assert!(app.pending_album_read.is_none());
        assert_eq!(app.screen(), Screen::Songs);
        assert!(Arc::ptr_eq(&app.pages.last().unwrap().rows, &source.rows));
        for _ in 0..35 {
            app.tick(1.0 / 60.0, &r);
            std::thread::sleep(Duration::from_millis(2));
        }
        pump(&mut app, &r, |app| !app.scanning);
        app.execute(Intent::MergeSong(0));
        pump(&mut app, &r, |app| {
            app.screen() == Screen::Curation
                && app.pages.last().is_some_and(|page| page.rows.len() > 1)
        });
        app.handle(Action::Down);
        app.handle(Action::Select);
        assert!(app.curation.has_options());
        for _ in 0..30 {
            app.tick(1.0 / 60.0, &r);
            app.draw(&mut r, app.visual_time);
            r.finish();
        }
        let merge_sheet = app.curation.visual_options().unwrap();
        app.handle(Action::Select);
        assert_eq!(app.screen(), Screen::Curation);
        assert!(app.pending_album_read.is_some());
        assert_eq!(
            app.album_read_busy_row(&merge_sheet),
            Some(merge_sheet.selected)
        );
        assert!(Arc::ptr_eq(
            &app.curation.visual_options().unwrap().rows,
            &merge_sheet.rows
        ));
        capture(&mut app, &mut r, &out, "04-merge-confirmation-busy");
        app.handle(Action::Back);
        assert!(app.pending_album_read.is_none());
        assert!(app.curation.has_options());
        assert!(Arc::ptr_eq(
            &app.curation.visual_options().unwrap().rows,
            &merge_sheet.rows
        ));
        for _ in 0..35 {
            app.tick(1.0 / 60.0, &r);
            std::thread::sleep(Duration::from_millis(2));
        }
        pump(&mut app, &r, |app| !app.scanning);
        assert_eq!(
            app.screen(),
            Screen::Curation,
            "Cancelled merge completion cannot navigate"
        );
        capture(&mut app, &mut r, &out, "05-merge-cancelled-same-sheet");
        app.handle(Action::Select);
        assert!(
            app.pending_album_read.is_some(),
            "The retained confirmation must remain usable for retry"
        );
        pump(&mut app, &r, |app| {
            app.pending_album_read.is_none() && app.screen() != Screen::Curation
        });
        assert_ne!(app.screen(), Screen::Extra);
        assert!(!app.curation.has_options());
        pump(&mut app, &r, |app| !app.scanning);
        assert_eq!(
            std::fs::read(&first).unwrap(),
            original_audio,
            "Logical merge and cancel retain the original audio bytes"
        );
        let track = *app.by_path.get(&first).unwrap();
        let screen = app.screen();
        app.execute(Intent::SeparateSong(track));
        assert_eq!(app.screen(), screen);
        pump(&mut app, &r, |app| app.pending_album_read.is_none());
        assert_eq!(
            app.screen(),
            screen,
            "Completed separation stays in its source section"
        );
        assert_ne!(app.screen(), Screen::Extra);
        assert_eq!(std::fs::read(&first).unwrap(), original_audio);
        app.shutdown(&r).unwrap();
    }
}

impl PendingAlbumRead {
    fn source(&self, page: &Page) -> bool {
        self.screen == page.screen && Arc::ptr_eq(&self.rows, &page.rows)
    }
}

impl App {
    pub(super) fn album_read_busy_row(&self, page: &Page) -> Option<usize> {
        self.pending_album_read
            .as_ref()
            .filter(|pending| pending.source(page))
            .map(|pending| pending.selected)
    }
    pub(super) fn album_read_row_busy(&self, page: &Page, row: usize) -> bool {
        self.album_read_busy_row(page) == Some(row)
    }
    pub(super) fn begin_album_read(
        &mut self,
        paths: Vec<PathBuf>,
        title: String,
        edit: betterpod_rust::metadata::TagEdit,
    ) {
        if !self.prepare_source_service(Completion::ReadEditor) {
            return;
        }
        self.extras.edit_album(paths, title, edit);
        if let Some(token) = self.extras.initial_album_read_token() {
            self.pending_album_read.as_mut().unwrap().token = token;
        } else {
            // Submission errors already prepare a readable editor error page.
            self.finish_pending_album_read();
        }
    }
    pub(super) fn begin_service_write(&mut self, task: betterpod_rust::online::Task) {
        if !self.prepare_source_service(Completion::ServiceWrite) {
            return;
        }
        self.extras.curation_task(task);
        if let Some(token) = self.extras.service_foreground_token() {
            self.pending_album_read.as_mut().unwrap().token = token;
        } else {
            self.finish_pending_album_read();
        }
    }
    fn prepare_source_service(&mut self, completion: Completion) -> bool {
        if self.pending_album_read.is_some() {
            return false;
        }
        let (source, kind) = if let Some(page) = self.options.clone() {
            (Some(page), Source::AppSheet)
        } else if self.screen() == Screen::Curation && self.curation.has_options() {
            (self.curation.visual_options(), Source::CurationSheet)
        } else {
            (self.navigation_page(), Source::List)
        };
        let Some(source) = source else {
            return false;
        };
        self.pending_album_read = Some(PendingAlbumRead {
            token: 0,
            screen: self.screen(),
            depth: self.pages.len(),
            generation: self.library_generation,
            rows: source.rows,
            selected: source.selected,
            source: kind,
            completion,
        });
        true
    }
    pub(super) fn cancel_pending_album_read(&mut self) {
        let Some(pending) = self.pending_album_read.take() else {
            return;
        };
        match pending.completion {
            Completion::ReadEditor
                if self.extras.initial_album_read_token() == Some(pending.token) =>
            {
                self.extras.cancel_initial_album_read()
            }
            Completion::ServiceWrite => self.extras.cancel_service_foreground(pending.token),
            _ => {}
        }
    }
    fn finish_pending_album_read(&mut self) {
        let Some(pending) = self.pending_album_read.take() else {
            return;
        };
        match pending.completion {
            Completion::ReadEditor => {
                self.options = None;
                self.open(Screen::Extra);
            }
            Completion::ServiceWrite => {
                self.extras.dismiss_service_source_view();
                if self.extras.service_write_result(pending.token) == Some(true) {
                    match pending.source {
                        Source::AppSheet => self.options = None,
                        Source::CurationSheet => self.curation.dismiss_options(),
                        Source::List => {}
                    }
                }
            }
        }
    }
    pub(super) fn poll_pending_album_read(&mut self) {
        let Some(pending) = self.pending_album_read.as_ref() else {
            return;
        };
        let (screen, depth, generation, kind, completion, token, source_rows) = (
            pending.screen,
            pending.depth,
            pending.generation,
            pending.source,
            pending.completion,
            pending.token,
            Arc::clone(&pending.rows),
        );
        let source = match kind {
            Source::AppSheet => self.options.clone(),
            Source::CurationSheet => self.curation.visual_options(),
            Source::List => self.navigation_page(),
        };
        let owns_source = self.screen() == screen
            && self.pages.len() == depth
            && self.library_generation == generation
            && source
                .is_some_and(|page| page.screen == screen && Arc::ptr_eq(&page.rows, &source_rows));
        if !owns_source {
            self.cancel_pending_album_read();
        } else {
            let current = match completion {
                Completion::ReadEditor => self.extras.initial_album_read_token(),
                Completion::ServiceWrite => self.extras.service_foreground_token(),
            };
            if current.is_none() {
                self.finish_pending_album_read();
            } else if current != Some(token) {
                // A follow-up read owns its own worker and source marker.
                self.pending_album_read = None;
            }
        }
    }
    pub(super) fn handle_pending_album_read(&mut self, action: Action) -> bool {
        if self.pending_album_read.is_none() {
            return false;
        }
        match action {
            Action::Back | Action::BackHeld => {
                self.cancel_pending_album_read();
                true
            }
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::Select
            | Action::SelectHeld
            | Action::Options
            | Action::Search => true,
            _ => false,
        }
    }
}
