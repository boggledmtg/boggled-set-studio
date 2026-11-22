//! Local HTTP API, file-change stream, and the process-wide undo stacks.

mod history;
mod routes;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use domain::FMP_COMMIT;
use notify::{RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::history::History;

pub use crate::routes::router;

#[derive(Clone, Debug)]
pub struct Change {
    pub name: String,
    pub data: String,
}

pub struct OpenSet {
    pub events: broadcast::Sender<Change>,
    /// Dropping this sender ends the watcher thread.
    _stop: Option<std::sync::mpsc::Sender<()>>,
    /// The watcher thread sends on this after it has released the directory.
    stopped: Option<std::sync::mpsc::Receiver<()>>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub recent_sets: Vec<String>,
    pub terminal_command: String,
    pub pack_path: PathBuf,
    pub auto_render: bool,
}

pub struct App {
    #[cfg(windows)]
    native_cache: Mutex<Option<([u8; 32], Arc<routes::NativePicture>)>>,
    pub token: String,
    pub support: PathBuf,
    pub opened: Mutex<HashMap<PathBuf, OpenSet>>,
    pub history: Mutex<HashMap<PathBuf, History>>,
    pub config: Mutex<Config>,
    pub terminal: terminal::Hub,
    pub sse: AtomicUsize,
    pub last: Mutex<Instant>,
    pub requests: AtomicU64,
}

impl App {
    pub fn new(support: PathBuf, token: String) -> Arc<Self> {
        let config = Config::load(&support);
        Arc::new(Self {
            #[cfg(windows)]
            native_cache: Mutex::new(None),
            token,
            support,
            opened: Mutex::new(HashMap::new()),
            history: Mutex::new(HashMap::new()),
            config: Mutex::new(config),
            terminal: terminal::Hub::new(),
            sse: AtomicUsize::new(0),
            last: Mutex::new(Instant::now()),
            requests: AtomicU64::new(0),
        })
    }

    pub fn touch(&self) {
        *self.last.lock().expect("activity") = Instant::now();
        self.requests.fetch_add(1, Ordering::Relaxed);
    }
}

impl Config {
    fn load(support: &Path) -> Self {
        let path = support.join("config.json");
        let mut config = Self::default_at(support);
        let Ok(text) = fs::read_to_string(path) else {
            return config;
        };
        let Ok(file) = serde_json::from_str::<ConfigFile>(&text) else {
            return config;
        };
        config.recent_sets = file.recent_sets;
        if !file.terminal_command.is_empty() {
            config.terminal_command = file.terminal_command;
        }
        if !file.pack_path.is_empty() {
            config.pack_path = PathBuf::from(file.pack_path);
        }
        config.auto_render = file.auto_render;
        config
    }

    fn default_at(support: &Path) -> Self {
        let (program, _) = terminal::default_shell();
        Self {
            recent_sets: Vec::new(),
            terminal_command: program,
            pack_path: support.join("packs").join(FMP_COMMIT),
            auto_render: true,
        }
    }

    fn save(&self, support: &Path) {
        let _ = fs::create_dir_all(support);
        let file = ConfigFile {
            recent_sets: self.recent_sets.clone(),
            terminal_command: self.terminal_command.clone(),
            pack_path: self.pack_path.display().to_string(),
            auto_render: self.auto_render,
        };
        if let Ok(text) = serde_json::to_string_pretty(&file) {
            let _ = fs::write(support.join("config.json"), text);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    recent_sets: Vec<String>,
    #[serde(default)]
    terminal_command: String,
    #[serde(default)]
    pack_path: String,
    #[serde(default = "yes")]
    auto_render: bool,
}

fn yes() -> bool {
    true
}

pub fn support_dir() -> PathBuf {
    if let Ok(home) = std::env::var("BOGGLED_HOME") {
        if !home.is_empty() {
            return PathBuf::from(home);
        }
    }
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("boggled");
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("boggled");
        }
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("boggled");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local").join("share").join("boggled");
    }
    PathBuf::from(".boggled")
}

pub fn normalize(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub async fn serve() -> Result<(), String> {
    let support = support_dir();
    fs::create_dir_all(&support).map_err(|err| err.to_string())?;
    let token = uuid::Uuid::new_v4().to_string();
    let port_text = std::env::var("BOGGLED_PORT").unwrap_or_else(|_| "0".into());
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port_text}"))
        .await
        .map_err(|err| err.to_string())?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    let file = json!({
        "pid": std::process::id(),
        "port": port,
        "token": token,
    });
    fs::write(
        support.join("server.json"),
        serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())?;
    println!("boggled listening on http://127.0.0.1:{port}/");
    let app = App::new(support, token);
    let idle = std::env::var("BOGGLED_IDLE_SECS")
        .ok()
        .and_then(|text| text.parse::<u64>().ok())
        .unwrap_or(15 * 60);
    if idle > 0 {
        let watched = Arc::clone(&app);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let quiet = watched.last.lock().expect("activity").elapsed().as_secs() >= idle;
                if quiet && watched.sse.load(Ordering::SeqCst) == 0 && watched.terminal.len() == 0 {
                    std::process::exit(0);
                }
            }
        });
    }
    axum::serve(listener, serve_router(app))
        .await
        .map_err(|err| err.to_string())
}

pub(crate) fn open_set(app: &App, path: &Path) -> Result<PathBuf, routes::ApiError> {
    let path = normalize(path);
    store::SetRoot::open(&path)?;
    let mut opened = app.opened.lock().expect("sets");
    if !opened.contains_key(&path) {
        let (events, _) = broadcast::channel(64);
        let (stop, stopped) = watch_set(path.clone(), events.clone());
        opened.insert(
            path.clone(),
            OpenSet {
                events,
                _stop: Some(stop),
                stopped: Some(stopped),
            },
        );
    }
    drop(opened);
    let mut config = app.config.lock().expect("config");
    let text = path.display().to_string();
    config.recent_sets.retain(|item| item != &text);
    config.recent_sets.insert(0, text);
    config.recent_sets.truncate(12);
    config.save(&app.support);
    Ok(path)
}

pub(crate) fn close_set(app: &App, path: &Path) {
    let open = app.opened.lock().expect("sets").remove(&normalize(path));
    if let Some(open) = open {
        drop(open._stop);
        if let Some(stopped) = open.stopped {
            let _ = stopped.recv_timeout(Duration::from_secs(2));
        }
    }
}

fn watch_set(
    path: PathBuf,
    events: broadcast::Sender<Change>,
) -> (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<String>>();
    let emit_to = events.clone();
    std::thread::spawn(move || {
        let mut pending = Vec::new();
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(paths) => pending.extend(paths),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if !pending.is_empty() {
                        emit(&emit_to, &mut pending);
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    if !pending.is_empty() {
                        emit(&emit_to, &mut pending);
                    }
                    break;
                }
            }
        }
    });
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let watched = path.clone();
    std::thread::spawn(move || {
        let watcher = notify::recommended_watcher(
            move |result: Result<notify::Event, notify::Error>| {
                let Ok(event) = result else {
                    let _ = tx.send(vec!["*".into()]);
                    return;
                };
                let mut rels = Vec::new();
                for changed in event.paths {
                    if let Ok(rel) = changed.strip_prefix(&watched) {
                        let text = rel.to_string_lossy().replace('\\', "/");
                        if text.ends_with(".tmp") || text.contains(".tmp") {
                            continue;
                        }
                        rels.push(text);
                    }
                }
                if !rels.is_empty() {
                    let _ = tx.send(rels);
                }
            },
        )
        .ok();
        if let Some(mut watcher) = watcher {
            let _ = watcher.watch(&path, RecursiveMode::Recursive);
            let _ = stop_rx.recv();
            drop(watcher);
        }
        let _ = done_tx.send(());
    });
    (stop_tx, done_rx)
}

fn emit(events: &broadcast::Sender<Change>, pending: &mut Vec<String>) {
    let paths = std::mem::take(pending);
    if paths.iter().any(|path| path == "*") {
        let _ = events.send(Change {
            name: "rescan".into(),
            data: "{}".into(),
        });
        return;
    }
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for path in paths {
        if let Some(kind) = kind_of(&path) {
            groups.entry(kind.into()).or_default().push(path);
        }
    }
    for (name, paths) in groups {
        let data = json!({ "paths": paths }).to_string();
        let _ = events.send(Change { name, data });
    }
}

fn kind_of(path: &str) -> Option<&'static str> {
    if path == "set.json" {
        Some("set")
    } else if path.starts_with("cards/") || path.starts_with("tokens/") {
        Some("card")
    } else if path.starts_with("keywords/") {
        Some("keyword")
    } else if path.starts_with("art/") {
        Some("art")
    } else if path.starts_with("renders/") {
        Some("render")
    } else {
        None
    }
}

pub fn cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any)
}

pub fn frontend_dist() -> Option<PathBuf> {
    let mut candidates = vec![PathBuf::from("web/dist")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("web").join("dist"));
        }
    }
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
}

pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn serve_router(app: Arc<App>) -> axum::Router {
    let mut routed = router(app);
    if let Some(dist) = frontend_dist() {
        routed = routed.fallback_service(
            ServeDir::new(dist).append_index_html_on_directories(true),
        );
    }
    routed
}
