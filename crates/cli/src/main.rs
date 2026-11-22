//! `boggled` talks to the local server. `boggled serve` is that server.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "boggled", about = "Edit a Magic set folder from the terminal")]
struct Cli {
    /// Print the server's JSON instead of a short summary.
    #[arg(long, global = true)]
    json: bool,
    /// Set folder for commands that edit one set.
    #[arg(long, global = true)]
    set: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the local server in the foreground.
    Serve,
    #[command(subcommand)]
    Set(SetCommand),
    #[command(subcommand)]
    Card(CardCommand),
    /// Import an MSE .mse-set package into a new folder.
    Import {
        source: PathBuf,
        dest: PathBuf,
    },
    /// Write one assembled set file. Only mtgjson is supported.
    Export {
        #[arg(long, default_value = "mtgjson")]
        format: String,
        /// Destination file. The default is export/<code>.json inside the set.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Paint one creature, or every face in the open set.
    Render {
        uuid: Option<String>,
    },
    /// Undo the latest API edit for the open set.
    Undo,
    /// Redo the latest undone edit.
    Redo,
    /// List undo and redo labels, newest first.
    History,
    #[command(subcommand)]
    Pack(PackCommand),
}

#[derive(Subcommand)]
enum SetCommand {
    /// Create a set folder and open it.
    Init {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value = "Untitled Set")]
        name: String,
        #[arg(long, default_value = "SET")]
        code: String,
    },
    /// Report validation warnings.
    Validate,
    /// Change the set name, code, or design brief.
    Update {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        code: Option<String>,
        #[arg(long)]
        brief: Option<String>,
    },
}

#[derive(Subcommand)]
enum CardCommand {
    List,
    Get { uuid: String },
    Create {
        #[command(flatten)]
        fields: CardFields,
    },
    Update {
        uuid: String,
        #[command(flatten)]
        fields: CardFields,
    },
    Delete { uuid: String },
    SetArt { uuid: String, #[arg(long)] path: PathBuf },
}

#[derive(clap::Args, Default)]
struct CardFields {
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    cost: Option<String>,
    #[arg(long)]
    r#type: Option<String>,
    #[arg(long)]
    text: Option<String>,
    #[arg(long)]
    flavor: Option<String>,
    #[arg(long)]
    power: Option<String>,
    #[arg(long)]
    toughness: Option<String>,
    #[arg(long)]
    rarity: Option<String>,
    #[arg(long)]
    number: Option<String>,
    #[arg(long)]
    artist: Option<String>,
}

#[derive(Subcommand)]
enum PackCommand {
    /// Download the pinned Full Magic Pack into the support directory.
    Download,
}

struct Session {
    base: String,
    token: String,
    client: reqwest::Client,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Command::Serve = cli.command {
        if let Err(err) = api::serve().await {
            eprintln!("{err}");
            std::process::exit(1);
        }
        return;
    }
    let result = run(cli).await;
    if let Err(err) = result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let session = ensure_server().await?;
    let set = match &cli.set {
        Some(path) => Some(open_set(&session, path).await?),
        None => None,
    };
    let body = dispatch(&session, set.as_deref(), &cli.command).await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string()));
    } else {
        println!("{}", summarize(&cli.command, &body));
    }
    Ok(())
}

async fn dispatch(session: &Session, set: Option<&str>, command: &Command) -> Result<Value, String> {
    match command {
        Command::Serve => unreachable!("serve does not use the client"),
        Command::Set(SetCommand::Init { path, name, code }) => {
            session
                .post(
                    "/api/sets/init",
                    None,
                    None,
                    json!({ "path": path, "name": name, "code": code }),
                )
                .await
        }
        Command::Set(SetCommand::Validate) => {
            session.post("/api/validate", set, None, json!({})).await
        }
        Command::Set(SetCommand::Update { name, code, brief }) => {
            let mut patch = serde_json::Map::new();
            if let Some(name) = name {
                patch.insert("name".into(), json!(name));
            }
            if let Some(code) = code {
                patch.insert("code".into(), json!(code));
            }
            if let Some(brief) = brief {
                patch.insert("boggled".into(), json!({ "designBrief": brief }));
            }
            let revision = session.revision("/api/set", set).await?;
            session
                .put("/api/set", set, Some(&revision), Value::Object(patch))
                .await
        }
        Command::Card(CardCommand::List) => session.get("/api/cards", set).await,
        Command::Card(CardCommand::Get { uuid }) => session.get(&format!("/api/cards/{uuid}"), set).await,
        Command::Card(CardCommand::Create { fields }) => {
            session.post("/api/cards", set, None, fields.to_json()).await
        }
        Command::Card(CardCommand::Update { uuid, fields }) => {
            let path = format!("/api/cards/{uuid}");
            let revision = session.revision(&path, set).await?;
            session.put(&path, set, Some(&revision), fields.to_json()).await
        }
        Command::Card(CardCommand::Delete { uuid }) => {
            session.delete(&format!("/api/cards/{uuid}"), set).await
        }
        Command::Card(CardCommand::SetArt { uuid, path }) => {
            let card = format!("/api/cards/{uuid}");
            let revision = session.revision(&card, set).await?;
            session
                .post(
                    &format!("/api/cards/{uuid}/art"),
                    set,
                    Some(&revision),
                    json!({ "path": path }),
                )
                .await
        }
        Command::Import { source, dest } => {
            session
                .post(
                    "/api/import",
                    None,
                    None,
                    json!({ "source": source, "dest": dest }),
                )
                .await
        }
        Command::Export { format, out } => {
            let mut body = json!({ "format": format });
            if let Some(out) = out {
                body["path"] = json!(out);
            }
            session.post("/api/export", set, None, body).await
        }
        Command::Render { uuid } => {
            let path = match uuid {
                Some(uuid) => format!("/api/render/{uuid}"),
                None => "/api/render".into(),
            };
            session.post(&path, set, None, json!({})).await
        }
        Command::Undo => session.post("/api/history/undo", set, None, json!({})).await,
        Command::Redo => session.post("/api/history/redo", set, None, json!({})).await,
        Command::History => session.get("/api/history", set).await,
        Command::Pack(PackCommand::Download) => session.post("/api/pack/download", None, None, json!({})).await,
    }
}

fn summarize(command: &Command, body: &Value) -> String {
    match command {
        Command::Card(CardCommand::List) => {
            let cards = body["cards"].as_array().cloned().unwrap_or_default();
            if cards.is_empty() {
                return "No cards".into();
            }
            cards
                .iter()
                .map(|card| {
                    format!(
                        "{}  {}  {}",
                        card["number"].as_str().unwrap_or(""),
                        card["name"].as_str().unwrap_or(""),
                        card["type"].as_str().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Command::History | Command::Undo | Command::Redo => {
            let undo = labels(&body["undo"]);
            let redo = labels(&body["redo"]);
            format!("Undo:\n{undo}\nRedo:\n{redo}")
        }
        Command::Set(SetCommand::Init { .. }) | Command::Import { .. } => {
            body["path"]
                .as_str()
                .or_else(|| body["setPath"].as_str())
                .map(|path| format!("Opened {path}"))
                .unwrap_or_else(|| body.to_string())
        }
        Command::Export { .. } => body["path"]
            .as_str()
            .map(|path| format!("Exported {path}"))
            .unwrap_or_else(|| body.to_string()),
        _ => serde_json::to_string_pretty(body).unwrap_or_else(|_| body.to_string()),
    }
}

fn labels(value: &Value) -> String {
    let items = value.as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        "  (empty)".into()
    } else {
        items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|label| format!("  {label}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl CardFields {
    fn to_json(&self) -> Value {
        let mut card = serde_json::Map::new();
        let mut put = |key: &str, value: &Option<String>| {
            if let Some(value) = value {
                card.insert(key.into(), json!(value));
            }
        };
        put("name", &self.name);
        put("manaCost", &self.cost);
        put("type", &self.r#type);
        put("text", &self.text);
        put("flavorText", &self.flavor);
        put("power", &self.power);
        put("toughness", &self.toughness);
        put("rarity", &self.rarity);
        put("number", &self.number);
        put("artist", &self.artist);
        Value::Object(card)
    }
}

async fn ensure_server() -> Result<Session, String> {
    if let Some(session) = read_server() {
        if session.health().await {
            return Ok(session);
        }
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    // The server must not inherit this process's stdout pipe, or a parent
    // waiting for the CLI to exit never sees EOF.
    hide_stdio_from_children();
    std::process::Command::new(exe)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("could not start boggled serve: {err}"))?;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(session) = read_server() {
            if session.health().await {
                return Ok(session);
            }
        }
    }
    Err("boggled serve did not become ready".into())
}

fn hide_stdio_from_children() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        extern "system" {
            fn SetHandleInformation(handle: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
        }
        const HANDLE_FLAG_INHERIT: u32 = 1;
        unsafe {
            for handle in [
                std::io::stdin().as_raw_handle(),
                std::io::stdout().as_raw_handle(),
                std::io::stderr().as_raw_handle(),
            ] {
                SetHandleInformation(handle as *mut core::ffi::c_void, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn read_server() -> Option<Session> {
    let path = api::support_dir().join("server.json");
    let text = std::fs::read_to_string(path).ok()?;
    let file: Value = serde_json::from_str(&text).ok()?;
    let port = file.get("port")?.as_u64()?;
    let token = file.get("token")?.as_str()?.to_string();
    Some(Session {
        base: format!("http://127.0.0.1:{port}"),
        token,
        client: reqwest::Client::new(),
    })
}

impl Session {
    async fn health(&self) -> bool {
        self.client
            .get(format!("{}/api/health", self.base))
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
    }

    async fn get(&self, path: &str, set: Option<&str>) -> Result<Value, String> {
        self.send(reqwest::Method::GET, path, set, None, None).await
    }

    async fn post(&self, path: &str, set: Option<&str>, revision: Option<&str>, body: Value) -> Result<Value, String> {
        self.send(reqwest::Method::POST, path, set, revision, Some(body)).await
    }

    async fn put(&self, path: &str, set: Option<&str>, revision: Option<&str>, body: Value) -> Result<Value, String> {
        self.send(reqwest::Method::PUT, path, set, revision, Some(body)).await
    }

    async fn delete(&self, path: &str, set: Option<&str>) -> Result<Value, String> {
        self.send(reqwest::Method::DELETE, path, set, None, None).await
    }

    async fn revision(&self, path: &str, set: Option<&str>) -> Result<String, String> {
        let response = self.request(reqwest::Method::GET, path, set, None, None).await?;
        response
            .headers()
            .get("x-revision")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
            .ok_or_else(|| format!("{path} did not send a revision"))
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        set: Option<&str>,
        revision: Option<&str>,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let response = self.request(method, path, set, revision, body).await?;
        let status = response.status();
        let text = response.text().await.map_err(|err| err.to_string())?;
        let value = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
        if !status.is_success() {
            let message = value
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or(text.as_str());
            return Err(message.to_string());
        }
        Ok(value)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        set: Option<&str>,
        revision: Option<&str>,
        body: Option<Value>,
    ) -> Result<reqwest::Response, String> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .header("authorization", format!("Bearer {}", self.token));
        if let Some(set) = set {
            request = request.header("x-boggled-set", set);
        }
        if let Some(revision) = revision {
            request = request.header("if-match", revision);
        }
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body).map_err(|err| err.to_string())?);
        }
        request.send().await.map_err(|err| err.to_string())
    }
}

async fn open_set(session: &Session, path: &std::path::Path) -> Result<String, String> {
    let body = session
        .post("/api/sets/open", None, None, json!({ "path": path }))
        .await?;
    body.get("path")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| "open did not return a path".into())
}
