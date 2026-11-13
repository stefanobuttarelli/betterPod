//! The car (or any Bluetooth receiver) side of BlueZ, over D-Bus:
//!
//! - a pairing agent that can show and confirm a code. Knulli's own agent
//!   says the console has no screen or buttons ("NoInputNoOutput"), and the
//!   app paired through `bluetoothctl` with nobody to answer a code. Speakers
//!   pair without a code; a car usually shows one and waits for both sides.
//!   Ours answers "KeyboardDisplay", asks on screen, and is the default
//!   agent only while the Bluetooth page is open, so a car can also start
//!   the pairing. BlueZ restores the previous default agent when ours steps
//!   down.
//! - the console's class: a phone's, which many cars require before they
//!   pair (`bt_class`).
//! - a media player registered with BlueZ (org.bluez.Media1.RegisterPlayer),
//!   so the car shows the title, artist, album, length and position and its
//!   buttons control the app. The cover's file goes out as `mpris:artUrl`;
//!   BlueZ does not send covers to a car yet, and will read it from there.
//!
//! One thread owns the connection. Nothing here runs on the UI thread except
//! queue operations.
use crate::input::Action;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};
use zbus::{
    blocking::{connection, Connection},
    interface,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value},
};

const AGENT_PATH: &str = "/org/betterpod/agent";
const PLAYER_PATH: &str = "/org/betterpod/player";
const ADAPTER_PATH: &str = "/org/bluez/hci0";
/// "KeyboardDisplay": a code can be shown, confirmed (A/B) or typed.
const CAPABILITY: &str = "KeyboardDisplay";

/// What the car shows as playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub track: u32,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
    pub stopped: bool,
    pub cover: Option<PathBuf>,
}

/// A question or note from a pairing, shown by the Bluetooth page.
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    /// Both screens show the same six digits; A pairs.
    Confirm { device: String, passkey: u32 },
    /// Pair with a device that asked without a code.
    Authorize { device: String },
    /// Type the PIN (old devices) or passkey the other device shows.
    Enter { device: String, digits_only: bool },
    /// Type this code on the other device.
    Show { device: String, code: String },
    /// The other side gave up; close the question.
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Yes,
    No,
    Text(String),
}

enum Command {
    Publish(NowPlaying),
    Agent(bool),
}

struct Service {
    commands: mpsc::SyncSender<Command>,
    prompts: Mutex<mpsc::Receiver<Prompt>>,
    actions: Mutex<mpsc::Receiver<Action>>,
    pending: Arc<Mutex<Option<async_channel::Sender<Answer>>>>,
    connection: Arc<Mutex<Option<Connection>>>,
}

static SERVICE: OnceLock<Option<Service>> = OnceLock::new();

fn service() -> Option<&'static Service> {
    SERVICE
        .get_or_init(|| {
            if std::env::var("BETTERPOD_RUST_BT_MEDIA").is_ok_and(|v| v == "0") {
                return None;
            }
            let (commands, rx) = mpsc::sync_channel(32);
            let (prompt_tx, prompts) = mpsc::sync_channel(8);
            let (action_tx, actions) = mpsc::sync_channel(32);
            let pending = Arc::new(Mutex::new(None));
            let connection = Arc::new(Mutex::new(None));
            let worker = Worker {
                rx,
                prompts: prompt_tx,
                actions: action_tx,
                pending: pending.clone(),
                shared: connection.clone(),
                phone_class: crate::bt_class::enabled(),
            };
            thread::Builder::new()
                .name("bt-media".into())
                .spawn(move || worker.run())
                .ok()?;
            Some(Service {
                commands,
                prompts: Mutex::new(prompts),
                actions: Mutex::new(actions),
                pending,
                connection,
            })
        })
        .as_ref()
}

/// Starts the service (once). Cheap to call again.
pub fn start() {
    let _ = service();
}

/// Sends what is playing; call only when it changed.
pub fn publish(now: NowPlaying) {
    if let Some(service) = service() {
        let _ = service.commands.try_send(Command::Publish(now));
    }
}

/// The agent takes pairings from other devices while the page is open.
pub fn set_default_agent(on: bool) {
    if let Some(service) = service() {
        let _ = service.commands.try_send(Command::Agent(on));
    }
}

/// Buttons pressed on the car, as the app's own media actions.
pub fn drain_actions(out: &mut Vec<Action>) {
    let Some(service) = SERVICE.get().and_then(Option::as_ref) else {
        return;
    };
    if let Ok(rx) = service.actions.lock() {
        while let Ok(action) = rx.try_recv() {
            out.push(action);
        }
    }
}

pub fn poll_prompt() -> Option<Prompt> {
    let service = SERVICE.get().and_then(Option::as_ref)?;
    service.prompts.lock().ok()?.try_recv().ok()
}

/// The answer to the open question; ignored when none is waiting.
pub fn answer(answer: Answer) {
    let Some(service) = SERVICE.get().and_then(Option::as_ref) else {
        return;
    };
    if let Some(tx) = service.pending.lock().ok().and_then(|mut p| p.take()) {
        eprintln!(
            "Bluetooth agent: console answered {}",
            match &answer {
                Answer::Yes => "yes",
                Answer::No => "no",
                Answer::Text(_) => "with a code",
            }
        );
        let _ = tx.try_send(answer);
    }
}

/// Pairs with our agent, so a code can be confirmed on screen. Returns
/// `None` without a system bus (the caller falls back to bluetoothctl).
/// `cancelled` is polled; on true the pairing is cancelled.
pub fn pair(
    address: &str,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Option<Result<(), String>> {
    let service = service()?;
    // The worker connects in the background; give it a moment on first use.
    let started = Instant::now();
    let connection = loop {
        if cancelled() {
            return Some(Err("Cancelled".into()));
        }
        if let Some(c) = service.connection.lock().ok().and_then(|c| c.clone()) {
            break c;
        }
        if started.elapsed() > Duration::from_secs(3) {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    };
    Some(pair_on_connection(&connection, address, timeout, cancelled))
}

fn pair_on_connection(
    connection: &Connection,
    address: &str,
    timeout: Duration,
    cancelled: impl Fn() -> bool,
) -> Result<(), String> {
    if cancelled() {
        return Err("Cancelled".into());
    }
    // A bus connection can be shared before the worker's first registration,
    // or BlueZ may have restarted. Pair must use our interactive agent, not
    // silently fall back to Knulli's NoInputNoOutput agent.
    if let Err(error) = Worker::register_agent(connection, false) {
        eprintln!("Bluetooth pairing with {address}: agent registration failed: {error}");
        return Err("Bluetooth pairing is not ready. Try again".into());
    }
    if cancelled() {
        return Err("Cancelled".into());
    }
    eprintln!("Bluetooth pairing with {address}: started with {CAPABILITY} agent");
    let started = Instant::now();
    let device = device_path(address);
    let (tx, rx) = mpsc::channel();
    {
        let connection = connection.clone();
        let device = device.clone();
        thread::Builder::new()
            .name("bt-pair".into())
            .spawn(move || {
                let _ = tx.send(connection.call_method(
                    Some("org.bluez"),
                    device.as_str(),
                    Some("org.bluez.Device1"),
                    "Pair",
                    &(),
                ));
            })
            .map_err(|error| format!("Could not start Bluetooth pairing: {error}"))?;
    }
    let deadline = Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(_)) => {
                eprintln!("Bluetooth pairing with {address}: completed after {:.1}s", started.elapsed().as_secs_f32());
                return Ok(());
            }
            Ok(Err(error)) => {
                eprintln!("Bluetooth pairing with {address}: failed after {:.1}s: {error}", started.elapsed().as_secs_f32());
                return Err(pairing_error(&error.to_string()));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Pairing stopped".into())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let was_cancelled = cancelled();
        if was_cancelled || Instant::now() >= deadline {
            eprintln!(
                "Bluetooth pairing with {address}: cancelling locally after {:.1}s ({})",
                started.elapsed().as_secs_f32(),
                if was_cancelled { "console cancelled" } else { "pairing deadline" }
            );
            let _ = connection.call_method(
                Some("org.bluez"),
                device.as_str(),
                Some("org.bluez.Device1"),
                "CancelPairing",
                &(),
            );
            return Err(if was_cancelled {
                "Cancelled".into()
            } else {
                "No answer from the device. Put it in pairing mode and try again".into()
            });
        }
    }
}

fn device_path(address: &str) -> String {
    format!("{ADAPTER_PATH}/dev_{}", address.replace(':', "_"))
}

/// Plain words for BlueZ's pairing errors; the full one goes to the log.
///
/// BlueZ maps cancellation, disconnection and insufficient resources to
/// AuthenticationCanceled. The error alone does not identify which side
/// ended pairing, or prove a missing phone profile.
fn pairing_error(error: &str) -> String {
    if error.contains("AlreadyExists") {
        return "Already paired".into();
    }
    if error.contains("AuthenticationRejected") {
        return "The device refused to pair. Open its pairing screen and try again".into();
    }
    if error.contains("AuthenticationCanceled") {
        return "Pairing was cancelled or the connection was lost. Try again".into();
    }
    if error.contains("AuthenticationFailed") {
        return "Pairing failed: the codes did not match or the device refused. Try again".into();
    }
    if error.contains("AuthenticationTimeout") || error.contains("ConnectionAttemptFailed") {
        return "No answer from the device. Put it in pairing mode and try again".into();
    }
    if error.contains("UnknownObject") || error.contains("DoesNotExist") {
        return "The device is no longer in range. Search again".into();
    }
    let short = error.rsplit(": ").next().unwrap_or(error);
    format!("Pairing failed: {short}")
}

// ---------------------------------------------------------------- agent

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.bluez.Error")]
enum BluezError {
    #[zbus(error)]
    ZBus(zbus::Error),
    Rejected(String),
}

struct Agent {
    prompts: mpsc::SyncSender<Prompt>,
    pending: Arc<Mutex<Option<async_channel::Sender<Answer>>>>,
}

impl Agent {
    async fn ask(&self, prompt: Prompt) -> Answer {
        // Log the stage, never PINs or passkeys.
        let (kind, device) = match &prompt {
            Prompt::Confirm { device, .. } => ("confirmation requested", device.as_str()),
            Prompt::Authorize { device } => ("pairing approval requested", device.as_str()),
            Prompt::Enter { device, .. } => ("code entry requested", device.as_str()),
            _ => ("prompt requested", "unknown device"),
        };
        eprintln!("Bluetooth agent: {kind} for {device}");
        let (tx, rx) = async_channel::bounded(1);
        if let Ok(mut pending) = self.pending.lock() {
            // A newer question replaces an older one; the old one is refused.
            if let Some(old) = pending.replace(tx) {
                eprintln!("Bluetooth agent: replaced an unanswered request");
                let _ = old.try_send(Answer::No);
            }
        }
        if self.prompts.try_send(prompt).is_err() {
            eprintln!("Bluetooth agent: prompt queue unavailable; rejecting request");
            return Answer::No;
        }
        rx.recv().await.unwrap_or(Answer::No)
    }
}

async fn device_name(connection: &zbus::Connection, device: &ObjectPath<'_>) -> String {
    let fallback = device
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .trim_start_matches("dev_")
        .replace('_', ":");
    let Ok(proxy) =
        zbus::Proxy::new(connection, "org.bluez", device.to_owned(), "org.bluez.Device1").await
    else {
        return fallback;
    };
    proxy
        .get_property::<String>("Alias")
        .await
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(fallback)
}

fn rejected() -> BluezError {
    BluezError::Rejected("Rejected on the console".into())
}

#[interface(name = "org.bluez.Agent1")]
impl Agent {
    fn release(&self) {
        eprintln!("Bluetooth agent: released by BlueZ");
    }

    async fn request_pin_code(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<String, BluezError> {
        let device = device_name(connection, &device).await;
        match self.ask(Prompt::Enter { device, digits_only: false }).await {
            Answer::Text(pin) if (1..=16).contains(&pin.len()) => Ok(pin),
            _ => Err(rejected()),
        }
    }

    async fn display_pin_code(
        &self,
        device: OwnedObjectPath,
        pincode: String,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        let device = device_name(connection, &device).await;
        let _ = self.prompts.try_send(Prompt::Show { device, code: pincode });
    }

    async fn request_passkey(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<u32, BluezError> {
        let device = device_name(connection, &device).await;
        match self.ask(Prompt::Enter { device, digits_only: true }).await {
            Answer::Text(code) => code
                .parse::<u32>()
                .ok()
                .filter(|code| *code <= 999_999)
                .ok_or_else(rejected),
            _ => Err(rejected()),
        }
    }

    async fn display_passkey(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        _entered: u16,
        #[zbus(connection)] connection: &zbus::Connection,
    ) {
        let device = device_name(connection, &device).await;
        let _ = self.prompts.try_send(Prompt::Show {
            device,
            code: format!("{passkey:06}"),
        });
    }

    async fn request_confirmation(
        &self,
        device: OwnedObjectPath,
        passkey: u32,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<(), BluezError> {
        let device = device_name(connection, &device).await;
        match self.ask(Prompt::Confirm { device, passkey }).await {
            Answer::Yes => Ok(()),
            _ => Err(rejected()),
        }
    }

    async fn request_authorization(
        &self,
        device: OwnedObjectPath,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<(), BluezError> {
        let device = device_name(connection, &device).await;
        match self.ask(Prompt::Authorize { device }).await {
            Answer::Yes => Ok(()),
            _ => Err(rejected()),
        }
    }

    /// A paired device opening a profile (audio, remote control): allowed,
    /// as Knulli's agent does.
    fn authorize_service(&self, _device: OwnedObjectPath, _uuid: String) {}

    fn cancel(&self) {
        eprintln!("Bluetooth agent: BlueZ cancelled the pending prompt");
        if let Some(tx) = self.pending.lock().ok().and_then(|mut p| p.take()) {
            let _ = tx.try_send(Answer::No);
        }
        let _ = self.prompts.try_send(Prompt::Cancelled);
    }
}

// ---------------------------------------------------------------- player

struct Player {
    now: NowPlaying,
    actions: mpsc::SyncSender<Action>,
}

impl Player {
    fn send(&self, action: Action) {
        let _ = self.actions.try_send(action);
    }
}

fn micros(seconds: f64) -> i64 {
    (seconds.max(0.0) * 1_000_000.0) as i64
}

fn owned(value: Value<'_>) -> Option<OwnedValue> {
    OwnedValue::try_from(value).ok()
}

/// MPRIS metadata, as BlueZ reads it for the car's display.
fn metadata(now: &NowPlaying) -> HashMap<String, OwnedValue> {
    let mut map = HashMap::new();
    let mut put = |key: &str, value: Value<'_>| {
        if let Some(value) = owned(value) {
            map.insert(key.to_string(), value);
        }
    };
    put(
        "mpris:trackid",
        Value::from(ObjectPath::from_static_str_unchecked(
            "/org/betterpod/track",
        )),
    );
    if !now.title.is_empty() {
        put("xesam:title", Value::from(now.title.as_str()));
    }
    if !now.artist.is_empty() {
        put("xesam:artist", Value::from(vec![now.artist.as_str()]));
    }
    if !now.album.is_empty() {
        put("xesam:album", Value::from(now.album.as_str()));
    }
    if now.track > 0 {
        put("xesam:trackNumber", Value::from(now.track as i32));
    }
    if now.duration > 0.0 {
        put("mpris:length", Value::from(micros(now.duration)));
    }
    if let Some(cover) = &now.cover {
        put(
            "mpris:artUrl",
            Value::from(format!("file://{}", cover.display())),
        );
    }
    map
}

fn status(now: &NowPlaying) -> &'static str {
    if now.stopped {
        "Stopped"
    } else if now.playing {
        "Playing"
    } else {
        "Paused"
    }
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&self) {
        self.send(Action::MediaPlay);
    }
    fn pause(&self) {
        self.send(Action::MediaPause);
    }
    fn play_pause(&self) {
        self.send(Action::MediaPlayPause);
    }
    fn stop(&self) {
        self.send(Action::MediaPause);
    }
    fn next(&self) {
        self.send(Action::MediaNext);
    }
    fn previous(&self) {
        self.send(Action::MediaPrevious);
    }
    fn seek(&self, _offset: i64) {}

    #[zbus(signal)]
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> String {
        status(&self.now).into()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        metadata(&self.now)
    }
    #[zbus(property)]
    fn position(&self) -> i64 {
        micros(self.now.position)
    }
    #[zbus(property)]
    fn loop_status(&self) -> String {
        "None".into()
    }
    #[zbus(property)]
    fn set_loop_status(&mut self, _value: String) {}
    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn set_shuffle(&mut self, _value: bool) {}
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------- worker

struct Worker {
    rx: mpsc::Receiver<Command>,
    prompts: mpsc::SyncSender<Prompt>,
    actions: mpsc::SyncSender<Action>,
    pending: Arc<Mutex<Option<async_channel::Sender<Answer>>>>,
    shared: Arc<Mutex<Option<Connection>>>,
    /// Shows the console to cars as a phone (`bt_class`).
    phone_class: bool,
}

/// BlueZ's answer when something is registered already: fine.
fn already(result: zbus::Result<zbus::Message>) -> zbus::Result<()> {
    match result {
        Ok(_) => Ok(()),
        Err(zbus::Error::MethodError(name, _, _)) if name.as_str().ends_with("AlreadyExists") => {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Knulli keeps the system bus in /run without a /var/run/dbus alias.
/// zbus's built-in Unix default is /var/run/dbus/system_bus_socket. Respect
/// every explicit override, including an invalid one, before trying /run.
fn system_bus_address(overridden: bool, socket: &Path) -> Option<String> {
    use std::os::unix::fs::FileTypeExt;
    if !overridden && socket.metadata().is_ok_and(|meta| meta.file_type().is_socket()) {
        Some(format!("unix:path={}", socket.display()))
    } else {
        None
    }
}

impl Worker {
    fn connect(&self, now: &NowPlaying, address: Option<&str>) -> zbus::Result<Connection> {
        let agent = Agent {
            prompts: self.prompts.clone(),
            pending: self.pending.clone(),
        };
        let player = Player {
            now: now.clone(),
            actions: self.actions.clone(),
        };
        let builder = match address {
            Some(address) => connection::Builder::address(address)?,
            None => connection::Builder::system()?,
        };
        builder
            .serve_at(AGENT_PATH, agent)?
            .serve_at(PLAYER_PATH, player)?
            .build()
    }

    fn call(
        connection: &Connection,
        path: &str,
        interface: &str,
        method: &str,
        body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
    ) -> zbus::Result<zbus::Message> {
        connection.call_method(Some("org.bluez"), path, Some(interface), method, body)
    }

    fn register_agent(connection: &Connection, default: bool) -> zbus::Result<()> {
        let path = ObjectPath::from_static_str_unchecked(AGENT_PATH);
        already(Self::call(
            connection,
            "/org/bluez",
            "org.bluez.AgentManager1",
            "RegisterAgent",
            &(&path, CAPABILITY),
        ))?;
        if default {
            Self::call(
                connection,
                "/org/bluez",
                "org.bluez.AgentManager1",
                "RequestDefaultAgent",
                &(&path,),
            )?;
        }
        Ok(())
    }

    /// Steps down as default: BlueZ then returns to Knulli's agent. Ours stays
    /// registered for the pairings the app starts.
    fn release_default(connection: &Connection) -> zbus::Result<()> {
        let path = ObjectPath::from_static_str_unchecked(AGENT_PATH);
        let _ = Self::call(
            connection,
            "/org/bluez",
            "org.bluez.AgentManager1",
            "UnregisterAgent",
            &(&path,),
        );
        Self::register_agent(connection, false)
    }

    fn register_player(connection: &Connection, now: &NowPlaying) -> zbus::Result<()> {
        let path = ObjectPath::from_static_str_unchecked(PLAYER_PATH);
        let mut properties: HashMap<&str, Value<'_>> = HashMap::new();
        properties.insert("PlaybackStatus", Value::from(status(now)));
        properties.insert("LoopStatus", Value::from("None"));
        properties.insert("Shuffle", Value::from(false));
        properties.insert("Position", Value::from(micros(now.position)));
        let meta: HashMap<String, Value<'_>> = metadata(now)
            .into_iter()
            .map(|(k, v)| (k, Value::from(v)))
            .collect();
        properties.insert("Metadata", Value::from(meta));
        already(Self::call(
            connection,
            ADAPTER_PATH,
            "org.bluez.Media1",
            "RegisterPlayer",
            &(&path, properties),
        ))
    }

    fn emit(connection: &Connection, old: &NowPlaying, now: &NowPlaying) -> zbus::Result<()> {
        let iface = connection
            .object_server()
            .interface::<_, Player>(PLAYER_PATH)?;
        iface.get_mut().now = now.clone();
        let player = iface.get();
        let emitter = iface.signal_emitter();
        zbus::block_on(async {
            if status(old) != status(now) {
                player.playback_status_changed(emitter).await?;
            }
            if metadata(old) != metadata(now) {
                player.metadata_changed(emitter).await?;
            }
            player.position_changed(emitter).await?;
            Player::seeked(emitter, micros(now.position)).await
        })
    }

    /// Cars pair only with phones: the console says it is one (see
    /// `bt_class`). Logs each change, and each new error once.
    fn present_as_phone(logged: &mut Option<String>) {
        use crate::bt_class::{ensure_phone, Outcome};
        match ensure_phone(0) {
            Ok(Outcome::Changed { from, to }) => {
                eprintln!(
                    "Bluetooth: the console now shows itself as a phone to cars \
                     (class 0x{from:06x} -> 0x{to:06x})"
                );
                *logged = None;
            }
            Ok(Outcome::Unchanged) => *logged = None,
            Err(error) => {
                if logged.as_ref() != Some(&error) {
                    eprintln!("Bluetooth: cannot set the phone class ({error})");
                    *logged = Some(error);
                }
            }
        }
    }

    fn run(self) {
        self.run_with_bus(|| {
            system_bus_address(
                std::env::var_os("DBUS_SYSTEM_BUS_ADDRESS").is_some(),
                Path::new("/run/dbus/system_bus_socket"),
            )
        });
    }

    fn run_with_bus(self, resolve_address: impl Fn() -> Option<String>) {
        let mut now = NowPlaying {
            stopped: true,
            ..NowPlaying::default()
        };
        let mut agent_default = false;
        let mut connection: Option<Connection> = None;
        let mut next_check = Instant::now();
        let mut logged: Option<String> = None;
        let phone_class = self.phone_class;
        let mut class_logged: Option<String> = None;
        let mut registration_logged = false;
        loop {
            let wait = next_check.saturating_duration_since(Instant::now());
            let command = match self.rx.recv_timeout(wait) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            // The page's intent survives an unavailable bus and the retry
            // interval; the UI sends this only when the page opens or closes.
            if let Some(Command::Agent(on)) = &command {
                agent_default = *on;
            }
            if connection.is_none() && (command.is_none() || Instant::now() >= next_check) {
                // The socket may appear after startup. Resolve it again on
                // every attempt instead of retaining a missing /run socket.
                let address = resolve_address();
                match self.connect(&now, address.as_deref()) {
                    Ok(c) => {
                        eprintln!(
                            "Bluetooth media: connected to system bus ({})",
                            address.as_deref().unwrap_or("environment/default address")
                        );
                        if let Ok(mut shared) = self.shared.lock() {
                            *shared = Some(c.clone());
                        }
                        connection = Some(c);
                    }
                    Err(error) => {
                        let text = error.to_string();
                        if logged.as_ref() != Some(&text) {
                            eprintln!("Bluetooth media: no system bus ({text})");
                            logged = Some(text);
                        }
                        next_check = Instant::now() + Duration::from_secs(30);
                        if let Some(Command::Publish(n)) = command {
                            now = n;
                        }
                        continue;
                    }
                }
            }
            let Some(c) = connection.as_ref() else {
                continue;
            };
            match command {
                Some(Command::Publish(n)) => {
                    if let Err(error) = Self::emit(c, &now, &n) {
                        eprintln!("Bluetooth media: {error}");
                    }
                    now = n;
                }
                Some(Command::Agent(on)) => {
                    let result = if on {
                        Self::register_agent(c, true)
                    } else {
                        Self::release_default(c)
                    };
                    if let Err(error) = result {
                        eprintln!("Bluetooth agent: {error}");
                    }
                }
                None => {
                    // bluetoothd restarts and adapters come back: register
                    // again; "already registered" is the normal answer.
                    let result = Self::register_agent(c, agent_default)
                        .and_then(|_| {
                            if !registration_logged {
                                eprintln!("Bluetooth agent: registered {CAPABILITY} agent");
                                registration_logged = true;
                            }
                            Self::register_player(c, &now)
                        });
                    match result {
                        Ok(()) => logged = None,
                        Err(error) => {
                            let text = error.to_string();
                            if logged.as_ref() != Some(&text) {
                                eprintln!("Bluetooth media: {text}");
                                logged = Some(text);
                            }
                        }
                    }
                    if phone_class {
                        Self::present_as_phone(&mut class_logged);
                    }
                    next_check = Instant::now() + Duration::from_secs(10);
                }
            }
        }
    }
}

/// The cover file next to a song, for `mpris:artUrl`.
pub fn cover_beside(song: &Path) -> Option<PathBuf> {
    let folder = song.parent()?;
    ["cover.jpg", "folder.jpg", "front.jpg", "cover.png", "folder.png", "Cover.jpg", "Folder.jpg"]
        .iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_bus_selection_rechecks_the_socket_and_preserves_overrides() {
        let dir = std::env::temp_dir().join(format!("bp-bus-address-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let resolve = || system_bus_address(false, &socket);
        assert_eq!(resolve(), None, "the bus has not started yet");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        assert_eq!(
            resolve(),
            Some(format!("unix:path={}", socket.display())),
            "a later connection attempt must discover the new socket"
        );
        // Presence, not nonempty content, defines an explicit environment
        // override. Even an invalid override belongs to zbus to diagnose.
        assert_eq!(system_bus_address(true, &socket), None);
        let regular = dir.join("regular");
        std::fs::write(&regular, "not a socket").unwrap();
        assert_eq!(system_bus_address(false, &regular), None);
        assert_eq!(system_bus_address(false, &dir.join("missing")), None);
        drop(listener);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn metadata_carries_what_the_car_shows() {
        let now = NowPlaying {
            title: "Tokyo-3".into(),
            artist: "Shiro Sagisu".into(),
            album: "Evangelion".into(),
            track: 5,
            duration: 200.5,
            position: 10.0,
            playing: true,
            stopped: false,
            cover: Some("/music/a/cover.jpg".into()),
        };
        let map = metadata(&now);
        assert_eq!(
            String::try_from(map["xesam:title"].clone()).unwrap(),
            "Tokyo-3"
        );
        assert_eq!(
            Vec::<String>::try_from(map["xesam:artist"].clone()).unwrap(),
            vec!["Shiro Sagisu".to_string()]
        );
        assert_eq!(i64::try_from(map["mpris:length"].clone()).unwrap(), 200_500_000);
        assert_eq!(i32::try_from(map["xesam:trackNumber"].clone()).unwrap(), 5);
        assert_eq!(
            String::try_from(map["mpris:artUrl"].clone()).unwrap(),
            "file:///music/a/cover.jpg"
        );
        assert_eq!(status(&now), "Playing");
        let empty = metadata(&NowPlaying::default());
        assert!(!empty.contains_key("xesam:title") && !empty.contains_key("mpris:length"));
    }

    #[test]
    fn pairing_errors_read_plainly() {
        assert!(pairing_error("org.bluez.Error.AuthenticationFailed: x").contains("codes"));
        assert!(pairing_error("org.bluez.Error.AuthenticationTimeout").contains("No answer"));
        assert!(pairing_error("org.bluez.Error.AuthenticationRejected").contains("device refused"));
        assert!(pairing_error("org.bluez.Error.AuthenticationCanceled").contains("cancelled or"));
        assert_eq!(device_path("AA:BB:CC:DD:EE:FF"), "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF");
    }

    #[test]
    fn a_question_waits_for_its_answer_and_cancel_refuses_it() {
        let (prompts, rx) = mpsc::sync_channel(4);
        let pending = Arc::new(Mutex::new(None));
        let agent = Agent {
            prompts,
            pending: pending.clone(),
        };
        let asked = thread::spawn(move || {
            zbus::block_on(agent.ask(Prompt::Confirm {
                device: "Car".into(),
                passkey: 123456,
            }))
        });
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            Prompt::Confirm { device: "Car".into(), passkey: 123456 }
        );
        while pending.lock().unwrap().is_none() {
            thread::sleep(Duration::from_millis(5));
        }
        pending.lock().unwrap().take().unwrap().try_send(Answer::Yes).unwrap();
        assert_eq!(asked.join().unwrap(), Answer::Yes);
    }

    /// A stand-in for bluetoothd on a private bus: records registrations.
    #[derive(Default, Clone)]
    struct FakeBluez {
        calls: Arc<Mutex<Vec<String>>>,
        reject_registration: Arc<std::sync::atomic::AtomicBool>,
    }
    #[interface(name = "org.bluez.AgentManager1")]
    impl FakeBluez {
        fn register_agent(&self, path: OwnedObjectPath, capability: String) -> Result<(), BluezError> {
            self.calls.lock().unwrap().push(format!("agent {} {capability}", path.as_str()));
            if self.reject_registration.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(BluezError::Rejected("Agent unavailable".into()));
            }
            Ok(())
        }
        fn request_default_agent(&self, _path: OwnedObjectPath) {
            self.calls.lock().unwrap().push("default".into());
        }
        fn unregister_agent(&self, _path: OwnedObjectPath) {
            self.calls.lock().unwrap().push("unregister".into());
        }
    }
    struct FakeMedia(Arc<Mutex<Vec<String>>>);
    #[interface(name = "org.bluez.Media1")]
    impl FakeMedia {
        fn register_player(&self, path: OwnedObjectPath, properties: HashMap<String, OwnedValue>) {
            let status = String::try_from(properties["PlaybackStatus"].clone()).unwrap();
            self.0.lock().unwrap().push(format!("player {} {status}", path.as_str()));
        }
    }
    struct FakeDevice {
        calls: Arc<Mutex<Vec<String>>>,
        hold_pair: Arc<std::sync::atomic::AtomicBool>,
        cancel_tx: async_channel::Sender<()>,
        cancel_rx: async_channel::Receiver<()>,
        finished: mpsc::Sender<()>,
    }
    #[interface(name = "org.bluez.Device1")]
    impl FakeDevice {
        async fn pair(&self) {
            self.calls.lock().unwrap().push("pair".into());
            if self.hold_pair.load(std::sync::atomic::Ordering::SeqCst) {
                let _ = self.cancel_rx.recv().await;
            }
            let _ = self.finished.send(());
        }
        fn cancel_pairing(&self) {
            self.calls.lock().unwrap().push("cancel-pairing".into());
            let _ = self.cancel_tx.try_send(());
        }
    }

    /// The whole path over a real D-Bus daemon: registration with BlueZ,
    /// a car's code confirmed on screen, a car's button, the car's display.
    #[test]
    fn talks_to_bluez_over_a_private_bus() {
        let dir = std::env::temp_dir().join(format!("bp-bus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let config = dir.join("bus.conf");
        std::fs::write(&config, format!(
            "<busconfig><type>session</type><listen>unix:path={}</listen><auth>EXTERNAL</auth>\
             <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
             <allow eavesdrop=\"true\"/><allow own=\"*\"/></policy></busconfig>",
            socket.display()
        )).unwrap();
        let Ok(mut daemon) = std::process::Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .spawn()
        else {
            assert!(!cfg!(target_os = "linux"), "Linux tests require dbus-daemon");
            eprintln!("dbus-daemon missing: skipped");
            return;
        };
        while !socket.exists() {
            thread::sleep(Duration::from_millis(20));
        }
        let address = format!("unix:path={}", socket.display());
        // This is the same selection as Knulli's /run socket, without
        // modifying /run or a process-wide D-Bus environment variable.
        assert_eq!(system_bus_address(false, &socket), Some(address.clone()));
        assert_eq!(system_bus_address(true, &socket), None, "explicit overrides win");
        assert_eq!(system_bus_address(false, &config), None, "a regular file is not a bus");
        assert_eq!(system_bus_address(false, &dir.join("missing")), None);
        let fake = FakeBluez::default();
        let calls = fake.calls.clone();
        let reject_registration = fake.reject_registration.clone();
        let hold_pair = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (cancel_tx, cancel_rx) = async_channel::bounded(1);
        let (finished, finished_pairs) = mpsc::channel();
        let _bluez = connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.bluez")
            .unwrap()
            .serve_at("/org/bluez", fake)
            .unwrap()
            .serve_at(ADAPTER_PATH, FakeMedia(calls.clone()))
            .unwrap()
            .serve_at(device_path("AA:BB:CC:DD:EE:FF"), FakeDevice {
                calls: calls.clone(),
                hold_pair: hold_pair.clone(),
                cancel_tx,
                cancel_rx,
                finished,
            })
            .unwrap()
            .build()
            .unwrap();

        let (commands, rx) = mpsc::sync_channel(8);
        let (prompt_tx, prompts) = mpsc::sync_channel(8);
        let (action_tx, actions) = mpsc::sync_channel(8);
        let pending = Arc::new(Mutex::new(None));
        let shared = Arc::new(Mutex::new(None));
        let worker = Worker {
            rx,
            prompts: prompt_tx,
            actions: action_tx,
            pending: pending.clone(),
            shared: shared.clone(),
            phone_class: false,
        };
        let worker_socket = socket.clone();
        thread::spawn(move || {
            worker.run_with_bus(|| system_bus_address(false, &worker_socket))
        });
        let wait = |what: &str| {
            let until = Instant::now() + Duration::from_secs(5);
            while !calls.lock().unwrap().iter().any(|c| c.starts_with(what)) {
                assert!(Instant::now() < until, "no {what}: {:?}", calls.lock().unwrap());
                thread::sleep(Duration::from_millis(20));
            }
        };
        wait("agent /org/betterpod/agent KeyboardDisplay");
        wait("player /org/betterpod/player Stopped");
        commands.send(Command::Agent(true)).unwrap();
        wait("default");

        // bluetoothd asks our agent to confirm the car's code
        let ours = shared.lock().unwrap().clone().unwrap();
        // A cancelled request never reaches BlueZ. A live request registers
        // the interactive agent on its connection before starting Pair.
        calls.lock().unwrap().clear();
        assert_eq!(
            pair_on_connection(&ours, "AA:BB:CC:DD:EE:FF", Duration::from_secs(2), || true),
            Err("Cancelled".into())
        );
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(
            pair_on_connection(&ours, "AA:BB:CC:DD:EE:FF", Duration::from_secs(2), || false),
            Ok(())
        );
        assert_eq!(*calls.lock().unwrap(), [
            "agent /org/betterpod/agent KeyboardDisplay", "pair"
        ]);
        finished_pairs.recv_timeout(Duration::from_secs(2)).unwrap();

        // A registration failure must not silently pair with the system's
        // NoInputNoOutput agent, nor may cancellation during registration.
        calls.lock().unwrap().clear();
        reject_registration.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(pair_on_connection(&ours, "AA:BB:CC:DD:EE:FF", Duration::from_secs(2), || false)
            .unwrap_err().contains("not ready"));
        assert_eq!(*calls.lock().unwrap(), ["agent /org/betterpod/agent KeyboardDisplay"]);
        reject_registration.store(false, std::sync::atomic::Ordering::SeqCst);
        calls.lock().unwrap().clear();
        let checked = std::cell::Cell::new(false);
        assert_eq!(
            pair_on_connection(&ours, "AA:BB:CC:DD:EE:FF", Duration::from_secs(2), || checked.replace(true)),
            Err("Cancelled".into())
        );
        assert_eq!(*calls.lock().unwrap(), ["agent /org/betterpod/agent KeyboardDisplay"]);

        // Once Pair is pending, cancel its actual D-Bus operation exactly
        // once, for both a console cancellation and the pairing deadline.
        hold_pair.store(true, std::sync::atomic::Ordering::SeqCst);
        for user_cancel in [true, false] {
            calls.lock().unwrap().clear();
            let checks = std::cell::Cell::new(0);
            let error = pair_on_connection(&ours, "AA:BB:CC:DD:EE:FF", Duration::ZERO, || {
                let count = checks.get();
                checks.set(count + 1);
                user_cancel && count >= 2
            }).unwrap_err();
            assert_eq!(error, if user_cancel { "Cancelled" } else {
                "No answer from the device. Put it in pairing mode and try again"
            });
            assert_eq!(*calls.lock().unwrap(), [
                "agent /org/betterpod/agent KeyboardDisplay", "pair", "cancel-pairing"
            ]);
            finished_pairs.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        hold_pair.store(false, std::sync::atomic::Ordering::SeqCst);
        let name = ours.unique_name().unwrap().to_string();
        let car = connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        let asking = {
            let car = car.clone();
            let name = name.clone();
            thread::spawn(move || {
                car.call_method(
                    Some(name.as_str()),
                    AGENT_PATH,
                    Some("org.bluez.Agent1"),
                    "RequestConfirmation",
                    &(ObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(), 123456u32),
                )
            })
        };
        let prompt = prompts.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(prompt, Prompt::Confirm { device: "AA:BB:CC:DD:EE:FF".into(), passkey: 123456 });
        pending.lock().unwrap().take().unwrap().try_send(Answer::Yes).unwrap();
        assert!(asking.join().unwrap().is_ok());

        // a refused code is an org.bluez.Error.Rejected
        let refusing = {
            let car = car.clone();
            let name = name.clone();
            thread::spawn(move || {
                car.call_method(
                    Some(name.as_str()),
                    AGENT_PATH,
                    Some("org.bluez.Agent1"),
                    "RequestConfirmation",
                    &(ObjectPath::try_from("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF").unwrap(), 1u32),
                )
            })
        };
        prompts.recv_timeout(Duration::from_secs(5)).unwrap();
        pending.lock().unwrap().take().unwrap().try_send(Answer::No).unwrap();
        let error = refusing.join().unwrap().unwrap_err().to_string();
        assert!(error.contains("org.bluez.Error.Rejected"), "{error}");

        // the car's play button
        car.call_method(Some(name.as_str()), PLAYER_PATH, Some("org.mpris.MediaPlayer2.Player"), "Next", &())
            .unwrap();
        assert_eq!(actions.recv_timeout(Duration::from_secs(5)).unwrap(), Action::MediaNext);

        // what the car reads after a song starts
        commands
            .send(Command::Publish(NowPlaying {
                title: "Tokyo-3".into(),
                playing: true,
                duration: 120.0,
                ..NowPlaying::default()
            }))
            .unwrap();
        let player = zbus::blocking::Proxy::new(&car, name.as_str(), PLAYER_PATH, "org.mpris.MediaPlayer2.Player").unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let status: String = player.get_property("PlaybackStatus").unwrap();
            if status == "Playing" {
                break;
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(20));
        }
        let meta: HashMap<String, OwnedValue> = player.get_property("Metadata").unwrap();
        assert_eq!(String::try_from(meta["xesam:title"].clone()).unwrap(), "Tokyo-3");

        commands.send(Command::Agent(false)).unwrap();
        wait("unregister");
        drop(commands);
        let _ = daemon.kill();
        let _ = std::fs::remove_dir_all(dir);
    }
}
