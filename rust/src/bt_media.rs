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
        if let Some(c) = service.connection.lock().ok().and_then(|c| c.clone()) {
            break c;
        }
        if started.elapsed() > Duration::from_secs(3) {
            return None;
        }
        thread::sleep(Duration::from_millis(100));
    };
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
            .ok()?;
    }
    let deadline = Instant::now() + timeout;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(_)) => return Some(Ok(())),
            Ok(Err(error)) => {
                eprintln!("Bluetooth pairing with {address}: {error}");
                return Some(Err(pairing_error(&error.to_string())));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Some(Err("Pairing stopped".into()))
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancelled() || Instant::now() >= deadline {
            let _ = connection.call_method(
                Some("org.bluez"),
                device.as_str(),
                Some("org.bluez.Device1"),
                "CancelPairing",
                &(),
            );
            return Some(Err(if cancelled() {
                "Cancelled".into()
            } else {
                "No answer from the device. Put it in pairing mode and try again".into()
            }));
        }
    }
}

fn device_path(address: &str) -> String {
    format!("{ADAPTER_PATH}/dev_{}", address.replace(':', "_"))
}

/// Plain words for BlueZ's pairing errors; the full one goes to the log.
///
/// "Rejected" is the other device refusing the console before any code
/// (Pairing Not Allowed, security or address refused): a car not in its
/// add-a-phone screen, or one that wants a phone. "Canceled" is the other
/// device dropping the link or out of room. A code refused with B, or on
/// the car's screen, is "AuthenticationFailed".
fn pairing_error(error: &str) -> String {
    if error.contains("AlreadyExists") {
        return "Already paired".into();
    }
    if error.contains("AuthenticationRejected") {
        return "The device refused to pair. Open its pairing screen and try again".into();
    }
    if error.contains("AuthenticationCanceled") {
        return "The device stopped the pairing. Try again".into();
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
        let (tx, rx) = async_channel::bounded(1);
        if let Ok(mut pending) = self.pending.lock() {
            // A newer question replaces an older one; the old one is refused.
            if let Some(old) = pending.replace(tx) {
                let _ = old.try_send(Answer::No);
            }
        }
        if self.prompts.try_send(prompt).is_err() {
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
    fn release(&self) {}

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

impl Worker {
    fn connect(&self, now: &NowPlaying) -> zbus::Result<Connection> {
        let agent = Agent {
            prompts: self.prompts.clone(),
            pending: self.pending.clone(),
        };
        let player = Player {
            now: now.clone(),
            actions: self.actions.clone(),
        };
        connection::Builder::system()?
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
        loop {
            let wait = next_check.saturating_duration_since(Instant::now());
            let command = match self.rx.recv_timeout(wait) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            };
            if connection.is_none() && (command.is_none() || Instant::now() >= next_check) {
                match self.connect(&now) {
                    Ok(c) => {
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
                    agent_default = on;
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
                        .and_then(|_| Self::register_player(c, &now));
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
        assert!(pairing_error("org.bluez.Error.AuthenticationCanceled").contains("stopped"));
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
    }
    #[interface(name = "org.bluez.AgentManager1")]
    impl FakeBluez {
        fn register_agent(&self, path: OwnedObjectPath, capability: String) {
            self.calls.lock().unwrap().push(format!("agent {} {capability}", path.as_str()));
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
            eprintln!("dbus-daemon missing: skipped");
            return;
        };
        while !socket.exists() {
            thread::sleep(Duration::from_millis(20));
        }
        let address = format!("unix:path={}", socket.display());
        // SAFETY: only this test reads the variable, before any bus use.
        unsafe { std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", &address) };
        let fake = FakeBluez::default();
        let calls = fake.calls.clone();
        let _bluez = connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.bluez")
            .unwrap()
            .serve_at("/org/bluez", fake)
            .unwrap()
            .serve_at(ADAPTER_PATH, FakeMedia(calls.clone()))
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
        thread::spawn(move || worker.run());
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
