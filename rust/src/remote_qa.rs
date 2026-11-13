//! Explicit local-only SSH diagnostics. Disabled unless QA_DIR is provided.
//! Socket/file work and PNG compression belong to the worker, not the UI.
use crate::{app::App, input::Action, render::Renderer};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
struct Request {
    action: Option<Action>,
    frames: u32,
    capture: Option<String>,
    reply: mpsc::SyncSender<(Value, Option<image::RgbaImage>)>,
}
pub struct RemoteQa {
    rx: mpsc::Receiver<Request>,
    pending: Option<Request>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl RemoteQa {
    pub fn from_env() -> Result<Option<Self>, String> {
        let Some(root) = std::env::var_os("BETTERPOD_RUST_QA_DIR").map(PathBuf::from) else {
            return Ok(None);
        };
        if !root.is_absolute() {
            return Err("QA_DIR must be absolute".into());
        }
        std::fs::create_dir_all(root.join("captures")).map_err(|e| e.to_string())?;
        let path = root.join("control.sock");
        if path.exists() {
            return Err("QA socket already exists".into());
        }
        let listener = UnixListener::bind(&path).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::sync_channel(2);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::Builder::new()
            .name("remote-qa".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    let (mut stream, _) = match listener.accept() {
                        Ok(s) => s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                            continue;
                        }
                        Err(_) => break,
                    };
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
                    let result = (|| -> Result<Value, String> {
                        let mut bytes = Vec::new();
                        let mut b = [0u8; 1024];
                        while !bytes.contains(&b'\n') {
                            let n = stream.read(&mut b).map_err(|e| e.to_string())?;
                            if n == 0 {
                                return Err("Incomplete request".into());
                            }
                            bytes.extend_from_slice(&b[..n]);
                            if bytes.len() > 16384 {
                                return Err("Request too large".into());
                            }
                        }
                        let command: Value = serde_json::from_slice(
                            bytes.split(|c| *c == b'\n').next().unwrap_or_default(),
                        )
                        .map_err(|e| e.to_string())?;
                        let capture = command["capture"].as_str().map(str::to_owned);
                        if capture.as_ref().is_some_and(|s| {
                            s.is_empty()
                                || s.len() > 100
                                || !s
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                        }) {
                            return Err("Invalid capture name".into());
                        }
                        let action = command["action"].as_str().map(parse_action).transpose()?;
                        let (reply, answer) = mpsc::sync_channel(1);
                        tx.try_send(Request {
                            action,
                            frames: command["frames"].as_u64().unwrap_or(6).min(120) as u32,
                            capture: capture.clone(),
                            reply,
                        })
                        .map_err(|e| e.to_string())?;
                        let (mut state, image) = answer
                            .recv_timeout(Duration::from_secs(30))
                            .map_err(|e| e.to_string())?;
                        if let (Some(name), Some(image)) = (capture, image) {
                            let path = root.join("captures").join(format!("{name}.png"));
                            let white = image
                                .pixels()
                                .filter(|p| p[0] > 250 && p[1] > 250 && p[2] > 250)
                                .count();
                            state["white_fraction"] =
                                json!(white as f64 / (image.width() * image.height()) as f64);
                            image.save(&path).map_err(|e| e.to_string())?;
                            state["capture"] = json!(path);
                        }
                        Ok(state)
                    })();
                    let answer = match result {
                        Ok(state) => json!({"ok":true,"state":state}),
                        Err(e) => json!({"ok":false,"error":e}),
                    };
                    let _ = writeln!(stream, "{answer}");
                }
                let _ = std::fs::remove_file(path);
            })
            .map_err(|e| e.to_string())?;
        Ok(Some(Self {
            rx,
            pending: None,
            stop,
            worker: Some(worker),
        }))
    }
    pub fn before_frame(&mut self, app: &mut App) {
        if self.pending.is_none() {
            if let Ok(request) = self.rx.try_recv() {
                if let Some(action) = request.action {
                    app.handle(action)
                }
                self.pending = Some(request)
            }
        }
    }
    pub fn after_frame(&mut self, app: &mut App, r: &Renderer) {
        let Some(request) = self.pending.as_mut() else {
            return;
        };
        if request.frames > 0 {
            request.frames -= 1;
            return;
        }
        let request = self.pending.take().unwrap();
        let mut state = app.remote_qa_state();
        state["renderer"] = r.diagnostics();
        let image = request.capture.map(|_| r.capture_rgba());
        let _ = request.reply.try_send((state, image));
    }
}
impl Drop for RemoteQa {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.pending.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}
fn parse_action(s: &str) -> Result<Action, String> {
    Ok(match s {
        "up" => Action::Up,
        "down" => Action::Down,
        "left" => Action::Left,
        "right" => Action::Right,
        "a" => Action::Select,
        "b" => Action::Back,
        "a_held" => Action::SelectHeld,
        "b_held" => Action::BackHeld,
        "x" => Action::PlayPause,
        "start" => Action::Start,
        "y" => Action::Options,
        "l" => Action::Previous,
        "r" => Action::Next,
        "hold_down" => Action::HoldDown,
        "hold_up" => Action::HoldUp,
        "menu_down" => Action::MenuDown,
        "menu_up" => Action::MenuUp,
        "search" => Action::Search,
        "quit" => Action::Quit,
        "vol_up" => Action::VolumeUp,
        "vol_down" => Action::VolumeDown,
        "bright_up" => Action::BrightnessUp,
        "bright_down" => Action::BrightnessDown,
        _ => return Err("Unknown QA action".into()),
    })
}
