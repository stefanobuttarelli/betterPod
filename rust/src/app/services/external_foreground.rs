//! User-awaited Weather and Bluetooth work owns its source row, never a passive refresh.
use super::*;
use betterpod_rust::weather::{WorkerHandle, WorkerReply};

pub(super) enum ExternalKind {
    WeatherSearch,
    WeatherAdd(Place),
    Forecast { index: usize, open: bool },
    Bluetooth,
}
pub(super) struct ExternalPending {
    handle: WorkerHandle,
    owner: u64,
    row: usize,
    generation: u64,
    kind: ExternalKind,
    error: Option<String>,
}

impl Extras {
    pub(super) fn start_external_weather(&mut self, command: WeatherCommand, kind: ExternalKind) {
        self.cancel_external_foreground();
        let prepared = self.prepare_foreground_row();
        let Some((owner, row)) = prepared else {
            return;
        };
        if let Some(handle) = self.weather.start(command) {
            self.external_pending = Some(ExternalPending {
                handle,
                owner,
                row,
                generation: self.generation,
                kind,
                error: None,
            });
        } else {
            self.clear_prepared_foreground_row(prepared);
            self.weather_loading = false;
            self.forecast_loading = false;
            self.effects
                .push(Effect::Notice("Weather is busy. Try again.".into()));
        }
    }
    pub(super) fn start_external_bluetooth(&mut self, request: BtRequest) {
        self.cancel_external_foreground();
        let prepared = self.prepare_foreground_row();
        let Some((owner, row)) = prepared else {
            return;
        };
        if let Some(handle) = self.bluetooth.start(request) {
            self.external_pending = Some(ExternalPending {
                handle,
                owner,
                row,
                generation: self.generation,
                kind: ExternalKind::Bluetooth,
                error: None,
            });
        } else {
            self.clear_prepared_foreground_row(prepared);
            self.effects
                .push(Effect::Notice("Bluetooth is busy. Try again.".into()));
        }
    }
    pub(super) fn external_busy_row(&self) -> Option<(u64, usize)> {
        self.external_pending
            .as_ref()
            .filter(|pending| !pending.handle.is_cancelled())
            .map(|pending| (pending.owner, pending.row))
    }
    fn finish_external(&mut self) -> Option<ExternalKind> {
        let pending = self.external_pending.take()?;
        self.clear_prepared_foreground_row(Some((pending.owner, pending.row)));
        self.status.clear();
        Some(pending.kind)
    }
    pub(super) fn cancel_external_foreground(&mut self) {
        let Some(pending) = self.external_pending.take() else {
            return;
        };
        if matches!(pending.kind, ExternalKind::Bluetooth)
            && self.bluetooth_prompt_owned_by(pending.owner)
        {
            self.close_bluetooth_prompt();
            betterpod_rust::bt_media::answer(betterpod_rust::bt_media::Answer::No);
        }
        pending.handle.cancel();
        self.clear_prepared_foreground_row(Some((pending.owner, pending.row)));
        self.weather_loading = false;
        self.forecast_loading = false;
        self.pending_weather = None;
    }
    /// A pairing question belongs to the pending device row even when its
    /// sheet is above that row. Other sheets and keyboards stay locked.
    fn bluetooth_prompt_owned_by(&self, owner: u64) -> bool {
        let Some(page) = self.pages.last() else {
            return false;
        };
        if page.key == owner && matches!(self.keyboard, Some((_, Typing::BtCode(_)))) {
            return true;
        }
        self.bt_prompt == Some(page.key)
            && page.role == PageRole::Sheet
            && self
                .pages
                .iter()
                .rev()
                .nth(1)
                .is_some_and(|source| source.key == owner)
    }
    pub(super) fn close_bluetooth_prompt(&mut self) {
        if let Some(key) = self.bt_prompt.take() {
            if self.pages.last().is_some_and(|page| page.key == key) {
                self.pages.pop();
            }
        }
        if matches!(self.keyboard, Some((_, Typing::BtCode(_)))) {
            if let Some((mut keyboard, _)) = self.keyboard.take() {
                keyboard.hide(self.visual_time);
                self.closing_keyboard = Some(keyboard);
            }
        }
    }
    pub(super) fn handle_external_foreground(&mut self, action: Action) -> bool {
        let Some(pending) = self.external_pending.as_ref() else {
            return false;
        };
        if matches!(pending.kind, ExternalKind::Bluetooth)
            && pending.generation == self.generation
            && self.bluetooth_prompt_owned_by(pending.owner)
        {
            // The operation cannot finish without the user's answer. Keep
            // normal sheet selection and PIN editing available, including
            // the keyboard's B/backspace and hold-B/close controls.
            let typing = matches!(self.keyboard, Some((_, Typing::BtCode(_))));
            let cancel_row = self.pages.last().is_some_and(|page| {
                page.rows.get(page.selected).is_some_and(|row| matches!(row.choice, Choice::Dismiss))
                    && page.rows.iter().any(|row| {
                        matches!(&row.choice, Choice::ConfirmAction(choice) if matches!(**choice, Choice::BtAnswer(_)))
                    })
            });
            if action == Action::BackHeld
                || (!typing && (action == Action::Back || action == Action::Select && cancel_row))
            {
                self.cancel_external_foreground();
                return true;
            }
            return false;
        }
        match action {
            Action::Back | Action::BackHeld => {
                self.cancel_external_foreground();
                true
            }
            Action::Up
            | Action::Down
            | Action::Left
            | Action::Right
            | Action::Select
            | Action::SelectHeld
            | Action::Options
            | Action::Search
            | Action::MenuDown
            | Action::MenuUp => true,
            _ => false,
        }
    }
    fn owns_external(&self, id: Option<u64>, weather: bool) -> bool {
        self.external_pending.as_ref().is_some_and(|pending| {
            Some(pending.handle.id) == id
                && !pending.handle.is_cancelled()
                && pending.generation == self.generation
                && matches!(pending.kind, ExternalKind::Bluetooth) != weather
                && (self
                    .pages
                    .last()
                    .is_some_and(|page| page.key == pending.owner)
                    || !weather && self.bluetooth_prompt_owned_by(pending.owner))
        })
    }
    fn refresh_weather_source(&mut self) {
        if self
            .pages
            .last()
            .is_some_and(|page| page.role == PageRole::WeatherHome)
        {
            self.replace("Weather", self.weather_rows());
            self.pages.last_mut().unwrap().role = PageRole::WeatherHome;
        } else if self
            .pages
            .last()
            .is_some_and(|page| page.role == PageRole::WeatherSettings)
        {
            self.replace("Weather Settings", self.weather_settings_rows());
            self.pages.last_mut().unwrap().role = PageRole::WeatherSettings;
        }
    }
    pub(super) fn external_weather_reply(&mut self, reply: WorkerReply<WeatherEvent>) {
        let owned = self.owns_external(reply.id, true);
        if let WeatherEvent::State(state) = &reply.value {
            self.weather_state = state.clone()
        }
        if reply.id.is_some() && !owned {
            if self
                .external_pending
                .as_ref()
                .is_some_and(|pending| Some(pending.handle.id) == reply.id)
            {
                self.cancel_external_foreground();
            }
            return;
        }
        if !owned {
            if self.external_pending.is_none() && self.foreground.is_none() {
                if matches!(reply.value, WeatherEvent::State(_)) {
                    self.refresh_weather_source();
                } else if let WeatherEvent::Error { context, message } = reply.value {
                    if !context.starts_with("forecast:") && context != "search" {
                        self.effects
                            .push(Effect::Notice(format!("{context}: {message}")));
                    }
                }
            }
            return;
        }
        match reply.value {
            WeatherEvent::SearchResults { query, places } => {
                if query != self.weather_query {
                    self.cancel_external_foreground();
                    return;
                }
                self.finish_external();
                self.weather_loading = false;
                self.weather_error.clear();
                self.weather_results = Some(places);
                self.refresh_weather_source();
                if let Some(page) = self.pages.last_mut() {
                    page.selected = if page.rows.len() > 1 { 1 } else { 0 };
                }
            }
            WeatherEvent::Error { context, message } => {
                if context.starts_with("forecast:")
                    && self
                        .external_pending
                        .as_ref()
                        .is_some_and(|pending| matches!(pending.kind, ExternalKind::WeatherAdd(_)))
                {
                    self.external_pending.as_mut().unwrap().error = Some(message);
                    return; // Add may still commit its saved city; wait for its owned final snapshot.
                }
                let kind = self.finish_external();
                self.weather_loading = false;
                self.forecast_loading = false;
                if context == "search" {
                    self.weather_error = message;
                    self.refresh_weather_source();
                } else if matches!(kind, Some(ExternalKind::Forecast { open: false, .. })) {
                    self.forecast_error = message;
                } else {
                    self.effects.push(Effect::Notice(message));
                    self.refresh_weather_source();
                }
            }
            WeatherEvent::State(_) if reply.finished => {
                let error = self
                    .external_pending
                    .as_ref()
                    .and_then(|pending| pending.error.clone());
                let kind = self.finish_external();
                self.forecast_loading = false;
                match kind {
                    Some(ExternalKind::WeatherAdd(place)) => {
                        self.weather_query.clear();
                        self.weather_results = None;
                        self.refresh_weather_source();
                        if let Some(error) = error {
                            self.effects.push(Effect::Notice(error));
                        } else if let Some(index) = self.weather_state.places.iter().position(|p| {
                            (p.lat - place.lat).abs() < 0.0001 && (p.lon - place.lon).abs() < 0.0001
                        }) {
                            self.open_external_forecast(index);
                        }
                    }
                    Some(ExternalKind::Forecast { index, open: true }) => {
                        self.open_external_forecast(index)
                    }
                    _ => self.refresh_weather_source(),
                }
            }
            _ => {}
        }
    }
    fn open_external_forecast(&mut self, index: usize) {
        if index >= self.weather_state.places.len() {
            return;
        }
        self.forecast = Some(index);
        self.forecast_day = 0;
        self.forecast_error.clear();
        self.forecast_loading = false;
        self.push("Weather", Vec::new());
        self.forecast_depth = self.pages.len();
    }
    pub(super) fn external_bluetooth_reply(&mut self, reply: WorkerReply<Result<BtState, String>>) {
        let owned = self.owns_external(reply.id, false);
        if let Ok(state) = &reply.value {
            if state.header_only {
                self.bt_state.audio_connected = state.audio_connected;
                return;
            }
            self.bt_state = state.clone();
        }
        if reply.id.is_some() && !owned {
            if self
                .external_pending
                .as_ref()
                .is_some_and(|pending| Some(pending.handle.id) == reply.id)
            {
                self.cancel_external_foreground();
            }
            return;
        }
        if owned {
            self.close_bluetooth_prompt();
            self.finish_external();
        } else if self.external_pending.is_some() || self.foreground.is_some() {
            return;
        }
        match reply.value {
            Ok(_) => {
                self.bt_error.clear();
                if self
                    .bt_scanning_until
                    .is_some_and(|until| self.visual_time >= until)
                {
                    self.bt_scanning_until = None;
                }
            }
            Err(error) => {
                self.bt_error = error.clone();
                self.bt_scanning_until = None;
                if self
                    .pages
                    .last()
                    .is_some_and(|page| page.title == "Bluetooth")
                {
                    self.effects.push(Effect::Notice(error));
                }
            }
        }
        if self
            .pages
            .last()
            .is_some_and(|page| page.title == "Bluetooth")
        {
            self.replace("Bluetooth", self.bluetooth_rows());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        extras: Extras,
    }
    impl Fixture {
        fn new(title: &str) -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let root = std::env::temp_dir().join(format!(
                "bp-external-foreground-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(root.join("music")).unwrap();
            let mut extras = Extras::new(root.join("music"), root.join("data")).unwrap();
            extras.push(
                title,
                vec![
                    Item::new("Pressed item", Choice::Nothing),
                    Item::new("Other item", Choice::Nothing),
                ],
            );
            if title == "Weather" {
                extras.pages.last_mut().unwrap().role = PageRole::WeatherHome;
            }
            Self { root, extras }
        }
        fn claim(&mut self, kind: ExternalKind) -> WorkerHandle {
            let handle = self
                .extras
                .weather
                .start(WeatherCommand::Search("x".into()))
                .unwrap(); // Local validation only; no network.
            let (owner, row) = self.extras.prepare_foreground_row().unwrap();
            self.extras.external_pending = Some(ExternalPending {
                handle: handle.clone(),
                owner,
                row,
                generation: self.extras.generation,
                kind,
                error: None,
            });
            handle
        }
        fn confirm_pairing(&mut self) {
            self.extras.push_confirmation(
                "Code 123456 · Same on “Car”?",
                "",
                "Pair",
                Choice::BtAnswer(true),
            );
            self.extras.bt_prompt = self.extras.pages.last().map(|page| page.key);
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.extras.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn external_weather_keeps_source_focus_locks_navigation_and_back_rejects_stale_results() {
        let mut fixture = Fixture::new("Weather");
        fixture.extras.weather_query = "Rome".into();
        let handle = fixture.claim(ExternalKind::WeatherSearch);
        let owner = fixture.extras.pages.last().unwrap().key;
        assert!(fixture.extras.visual_page().unwrap().rows[0].visual.busy);
        for action in [Action::Down, Action::Select, Action::Options, Action::Right] {
            fixture.extras.handle(action);
        }
        assert_eq!(fixture.extras.pages.last().unwrap().selected, 0);
        fixture
            .extras
            .external_weather_reply(WorkerReply::background(WeatherEvent::State(
                WeatherSnapshot::default(),
            )));
        assert!(
            fixture.extras.external_pending.is_some(),
            "Startup/passive state cannot complete a pressed action"
        );
        fixture.extras.handle(Action::Back);
        assert!(handle.is_cancelled());
        assert!(fixture.extras.external_pending.is_none());
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
        assert!(!fixture.extras.pages.last().unwrap().rows[0].visual.busy);
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &handle,
            WeatherEvent::SearchResults {
                query: "Rome".into(),
                places: vec![Place {
                    name: "Old result".into(),
                    ..Place::default()
                }],
            },
            true,
        ));
        assert!(fixture.extras.weather_results.is_none());
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
    }
    #[test]
    fn stale_weather_add_or_forecast_error_cannot_navigate_or_complete_a_new_action() {
        let mut fixture = Fixture::new("Weather");
        let place = Place {
            name: "Rome".into(),
            lat: 41.9,
            lon: 12.5,
            ..Place::default()
        };
        let old = fixture.claim(ExternalKind::WeatherAdd(place.clone()));
        fixture.extras.cancel_external_foreground();
        fixture
            .extras
            .push("YouTube", vec![Item::new("Search", Choice::Nothing)]);
        let owner = fixture.extras.pages.last().unwrap().key;
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &old,
            WeatherEvent::State(WeatherSnapshot {
                fahrenheit: false,
                places: vec![place],
            }),
            true,
        ));
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
        assert!(fixture.extras.forecast.is_none());
        let current = fixture.claim(ExternalKind::Forecast {
            index: 1,
            open: false,
        });
        fixture.extras.forecast_loading = true;
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &old,
            WeatherEvent::Error {
                context: "forecast:0".into(),
                message: "Old forecast failed".into(),
            },
            true,
        ));
        assert!(fixture.extras.forecast_loading && fixture.extras.forecast_error.is_empty());
        assert_eq!(
            fixture.extras.external_pending.as_ref().unwrap().handle.id,
            current.id
        );
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &current,
            WeatherEvent::Error {
                context: "forecast:1".into(),
                message: "Current forecast failed".into(),
            },
            true,
        ));
        assert!(!fixture.extras.forecast_loading && fixture.extras.external_pending.is_none());
        assert_eq!(fixture.extras.forecast_error, "Current forecast failed");
    }
    #[test]
    fn passive_bluetooth_snapshot_cannot_clear_pending_action_or_close_a_new_page() {
        let mut fixture = Fixture::new("Bluetooth");
        let handle = fixture.claim(ExternalKind::Bluetooth);
        let owner = fixture.extras.pages.last().unwrap().key;
        fixture
            .extras
            .external_bluetooth_reply(WorkerReply::background(Ok(BtState {
                powered: true,
                ..BtState::default()
            })));
        assert!(fixture.extras.external_pending.is_some());
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
        assert!(fixture.extras.pages.last().unwrap().rows[0].visual.busy);
        fixture.extras.cancel_external_foreground();
        fixture.extras.push("Weather", Vec::new());
        let current = fixture.extras.pages.last().unwrap().key;
        fixture.extras.external_bluetooth_reply(WorkerReply::owned(
            &handle,
            Err("Old connect failed".into()),
            true,
        ));
        assert_eq!(fixture.extras.pages.last().unwrap().key, current);
        assert!(fixture.extras.bt_error.is_empty());
    }
    #[test]
    fn bluetooth_pairing_confirmation_accepts_a_while_connect_is_pending() {
        let mut fixture = Fixture::new("Bluetooth");
        let handle = fixture.claim(ExternalKind::Bluetooth);
        let owner = fixture.extras.pages.last().unwrap().key;
        fixture.confirm_pairing();
        fixture.extras.handle(Action::Select);
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
        assert!(
            fixture.extras.bt_prompt.is_none(),
            "A must reach the pairing answer"
        );
        assert!(!handle.is_cancelled(), "Confirming must keep Connect alive");
        assert!(fixture.extras.external_pending.is_some());
        fixture.extras.handle(Action::Down);
        assert_eq!(fixture.extras.pages.last().unwrap().selected, 0);
    }
    #[test]
    fn bluetooth_pairing_keyboard_accepts_input_while_connect_is_pending() {
        let mut fixture = Fixture::new("Bluetooth");
        let handle = fixture.claim(ExternalKind::Bluetooth);
        fixture.extras.keyboard = Some((Keyboard::new("123"), Typing::BtCode(true)));
        fixture.extras.handle(Action::Back);
        assert_eq!(fixture.extras.keyboard.as_ref().unwrap().0.query, "12");
        assert!(
            !handle.is_cancelled(),
            "B keeps its keyboard backspace meaning"
        );
        fixture.extras.handle(Action::Select);
        assert_ne!(fixture.extras.keyboard.as_ref().unwrap().0.query, "12");
        fixture.extras.handle(Action::Start);
        assert!(fixture.extras.keyboard.is_none(), "START submits the code");
        assert!(fixture.extras.external_pending.is_some());
        assert!(!handle.is_cancelled());
    }
    #[test]
    fn bluetooth_pairing_back_and_cancel_return_to_the_owned_row() {
        for actions in [vec![Action::Back], vec![Action::Down, Action::Select]] {
            let mut fixture = Fixture::new("Bluetooth");
            let handle = fixture.claim(ExternalKind::Bluetooth);
            let owner = fixture.extras.pages.last().unwrap().key;
            fixture.confirm_pairing();
            for action in actions {
                fixture.extras.handle(action);
            }
            assert!(handle.is_cancelled());
            assert!(fixture.extras.external_pending.is_none());
            assert!(fixture.extras.bt_prompt.is_none());
            assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
            assert_eq!(fixture.extras.pages.last().unwrap().selected, 0);
            assert!(!fixture.extras.pages.last().unwrap().rows[0].visual.busy);
        }
        let mut fixture = Fixture::new("Bluetooth");
        let handle = fixture.claim(ExternalKind::Bluetooth);
        fixture.extras.keyboard = Some((Keyboard::new("123"), Typing::BtCode(true)));
        fixture.extras.handle(Action::BackHeld);
        assert!(handle.is_cancelled());
        assert!(fixture.extras.keyboard.is_none());
        assert!(fixture.extras.external_pending.is_none());
    }
    #[test]
    fn bluetooth_pairing_failure_closes_its_prompt_and_keeps_a_retry_path() {
        for keyboard in [false, true] {
            let mut fixture = Fixture::new("Bluetooth");
            let handle = fixture.claim(ExternalKind::Bluetooth);
            let owner = fixture.extras.pages.last().unwrap().key;
            if keyboard {
                fixture.extras.keyboard = Some((Keyboard::new("123"), Typing::BtCode(true)));
            } else {
                fixture.confirm_pairing();
            }
            fixture.extras.external_bluetooth_reply(WorkerReply::owned(
                &handle,
                Err("Pairing interrupted".into()),
                true,
            ));
            assert!(fixture.extras.external_pending.is_none());
            assert!(fixture.extras.bt_prompt.is_none() && fixture.extras.keyboard.is_none());
            assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
            assert_eq!(fixture.extras.bt_error, "Pairing interrupted");
            assert!(fixture.extras.effects.iter().any(
                |effect| matches!(effect, Effect::Notice(error) if error == "Pairing interrupted")
            ));
            assert!(fixture
                .extras
                .pages
                .last()
                .unwrap()
                .rows
                .iter()
                .all(|row| !row.visual.busy));
        }
    }
    #[test]
    fn bluetooth_pairing_old_reply_cannot_close_a_new_question() {
        let mut fixture = Fixture::new("Bluetooth");
        let old = fixture.claim(ExternalKind::Bluetooth);
        fixture.confirm_pairing();
        fixture.extras.handle(Action::Back);
        let current = fixture.claim(ExternalKind::Bluetooth);
        fixture.confirm_pairing();
        let question = fixture.extras.bt_prompt;
        fixture.extras.external_bluetooth_reply(WorkerReply::owned(
            &old,
            Err("Old pairing failed".into()),
            true,
        ));
        assert_eq!(fixture.extras.bt_prompt, question);
        assert_eq!(fixture.extras.pages.last().map(|page| page.key), question);
        assert!(!current.is_cancelled());
        assert!(fixture.extras.bt_error.is_empty());
        assert_eq!(
            fixture.extras.external_pending.as_ref().unwrap().handle.id,
            current.id
        );
    }
    #[test]
    fn bluetooth_pairing_does_not_unlock_unrelated_sheets_or_keyboards() {
        let mut fixture = Fixture::new("Bluetooth");
        let handle = fixture.claim(ExternalKind::Bluetooth);
        fixture.extras.keyboard = Some((Keyboard::new("Search"), Typing::Combined));
        fixture.extras.handle(Action::Select);
        assert_eq!(fixture.extras.keyboard.as_ref().unwrap().0.query, "Search");
        fixture.extras.keyboard = None;
        fixture.confirm_pairing();
        let pairing_question = fixture.extras.bt_prompt;
        fixture.extras.push_confirmation(
            "Code 123456 · Same on “Car”?",
            "",
            "Unrelated action",
            Choice::Nothing,
        );
        let unrelated = fixture.extras.pages.last().unwrap().key;
        fixture.extras.handle(Action::Select);
        assert_eq!(fixture.extras.pages.last().unwrap().key, unrelated);
        assert_eq!(fixture.extras.bt_prompt, pairing_question);
        assert!(!handle.is_cancelled());

        let mut weather = Fixture::new("Weather");
        let handle = weather.claim(ExternalKind::WeatherSearch);
        weather.confirm_pairing();
        let question = weather.extras.bt_prompt;
        weather.extras.handle(Action::Select);
        assert_eq!(weather.extras.pages.last().map(|page| page.key), question);
        assert!(!handle.is_cancelled());
    }
    #[test]
    fn weather_add_forecast_failure_waits_for_committed_city_without_stale_navigation() {
        let mut fixture = Fixture::new("Weather");
        let place = Place {
            name: "Rome".into(),
            lat: 41.9,
            lon: 12.5,
            ..Place::default()
        };
        fixture.extras.weather_query = "Rome".into();
        fixture.extras.weather_results = Some(vec![place.clone()]);
        let handle = fixture.claim(ExternalKind::WeatherAdd(place.clone()));
        let owner = fixture.extras.pages.last().unwrap().key;
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &handle,
            WeatherEvent::Error {
                context: "forecast:0".into(),
                message: "Forecast unavailable".into(),
            },
            false,
        ));
        assert!(
            fixture.extras.external_pending.is_some(),
            "The forecast error precedes the Add commit snapshot"
        );
        fixture.extras.external_weather_reply(WorkerReply::owned(
            &handle,
            WeatherEvent::State(WeatherSnapshot {
                fahrenheit: false,
                places: vec![place],
            }),
            true,
        ));
        assert!(
            fixture.extras.external_pending.is_none()
                && fixture.extras.weather_results.is_none()
                && fixture.extras.weather_query.is_empty()
        );
        assert_eq!(fixture.extras.pages.last().unwrap().key, owner);
        assert!(fixture.extras.forecast.is_none());
        assert!(fixture
            .extras
            .pages
            .last()
            .unwrap()
            .rows
            .iter()
            .any(|row| row.title == "Rome"));
        assert!(fixture.extras.effects.iter().any(
            |effect| matches!(effect, Effect::Notice(message) if message == "Forecast unavailable")
        ));
    }
}
