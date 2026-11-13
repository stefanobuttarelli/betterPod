//! The original separate split/full panels and retained custom views.
use super::*;
impl App {
    pub(super) fn visual_draw_outgoing_custom(
        &mut self,
        r: &mut Renderer,
        elapsed: f64,
        fg: Color,
        muted: Color,
    ) {
        let Some((page, offset, whole_panel)) = self.visual_menu_outgoing_page() else {
            return;
        };
        if visual_menu::is_menu(page.screen) || (page.screen == Screen::Extra && page.split()) {
            return;
        }
        let (bg, _, _) = self.colors();
        r.push_clip(
            0.0,
            if whole_panel { 0.0 } else { 40.0 },
            640.0,
            if whole_panel { 480.0 } else { 440.0 },
        );
        r.push_transform(
            if page.screen == Screen::CoverFlow {
                0.0
            } else {
                offset
            },
            0.0,
            1.0,
            1.0,
            1.0,
        );
        if page.screen != Screen::CoverFlow {
            r.rect(0.0, 40.0, 640.0, 440.0, bg);
        }
        let drew_previous = (page.screen == Screen::Extra
            && self.extras.draw_previous(
                &page,
                r,
                self.dark(),
                elapsed,
                &mut self.art,
                &self.visual_assets,
            ))
            || (page.screen == Screen::Curation
                && self.curation.draw_previous(
                    &page,
                    r,
                    self.dark(),
                    &self.visual_assets,
                    elapsed,
                ));
        if !drew_previous {
            if matches!(page.screen, Screen::Extra | Screen::Curation) && !page.rows.is_empty() {
                self.visual_menu_page(r, &page, 0.0, false, true);
            } else {
                self.visual_draw_screen(page.screen, r, elapsed, fg, muted);
            }
        }
        if whole_panel && !matches!(page.screen, Screen::Visualizer | Screen::Video) {
            self.visual_menu_draw_header(r, &page.title, 0.0, 640.0, false);
        }
        r.pop_transform();
        r.pop_clip();
    }
    pub(super) fn visual_draw_screen(
        &mut self,
        screen: Screen,
        renderer: &mut Renderer,
        elapsed: f64,
        fg: Color,
        muted: Color,
    ) {
        match screen {
            Screen::WifiLibrary => self.wifi_library_draw(renderer, elapsed),
            Screen::CoverFlow => self.visual_player_draw_coverflow(renderer),
            Screen::NowPlaying => self.visual_player_draw_now_playing(renderer),
            Screen::Visualizer => {
                let current = self.current_index();
                if let Err(error) =
                    self.saver
                        .draw(renderer, &mut self.art, &self.library, current, &self.clock)
                {
                    eprintln!("Visualizer: {error}");
                }
            }
            Screen::Files => {
                let dark = self.dark();
                let page = self.files.visual_page();
                self.visual_menu_page(renderer, &page, 0.0, false, false);
                let _ = dark;
            }
            Screen::Maintenance => {
                let dark = self.dark();
                if self.maintenance.as_ref().is_some_and(|m| m.is_custom()) {
                    if let Some(m) = self.maintenance.as_mut() {
                        m.draw_custom(renderer, dark, &self.visual_assets, &mut self.art);
                    }
                } else if let Some(page) = self.maintenance.as_mut().and_then(|m| m.visual_page()) {
                    self.visual_menu_page(renderer, &page, 0.0, false, false);
                }
            }
            Screen::Curation => {
                if self.curation.is_custom() {
                    self.curation
                        .draw_custom(renderer, self.dark(), &self.visual_assets, elapsed);
                } else if let Some(page) = self.curation.visual_page() {
                    self.visual_menu_page(renderer, &page, 0.0, false, false);
                }
            }
            Screen::Quiz => {
                let dark = self.dark();
                if let Some(quiz) = self.quiz.as_mut().or(self.retired_quiz.as_mut()) {
                    quiz.draw(
                        renderer,
                        &self.library,
                        &self.audio_snapshot,
                        dark,
                        &mut self.art,
                        elapsed,
                    );
                }
            }
            Screen::Game => {
                let dark = self.dark();
                if let Some(game) = self.game.as_mut().or(self.retired_game.as_mut()) {
                    game.set_time(elapsed);
                    game.set_clock_shown(
                        self.settings
                            .game_clock
                            .iter()
                            .any(|name| name == game.title()),
                    );
                    game.draw(renderer, dark);
                }
            }
            Screen::Video => self.draw_video(renderer),
            Screen::Puzzle => {
                let dark = self.dark();
                if let Some(puzzle) = self.puzzle.as_ref() {
                    puzzle.draw(renderer, dark);
                }
            }
            Screen::About => self.visual_menu_draw_about(renderer, fg, muted),
            Screen::Extra => {
                let dark = self.dark();
                if !self.extras.has_custom_visual() {
                    if let Some(page) = self.extras.visual_page() {
                        if page.split() {
                            self.visual_menu_draw(renderer, elapsed, fg, muted);
                        } else {
                            self.visual_menu_page(renderer, &page, 0.0, false, false);
                        }
                    }
                    self.extras
                        .draw_overlay(renderer, &self.visual_assets, dark, elapsed);
                } else {
                    self.extras
                        .draw(renderer, dark, elapsed, &mut self.art, &self.visual_assets);
                }
            }
            _ => self.visual_menu_draw(renderer, elapsed, fg, muted),
        }
    }
}
