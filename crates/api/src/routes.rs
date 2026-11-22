//! HTTP routes. Handlers call the set folder, then record an undo step.

use std::convert::Infallible;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as UrlPath, State};
use axum::http::{header, HeaderMap, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use domain::{
    derive_face, is_supported_frame, new_creature, new_keyword, next_collector_number,
    object_map, string_field, validate_face, validate_set, DocumentError, NewCreature,
};
use futures_util::StreamExt;
use mse::{draw_card, DrawRequest, MseError};
use serde_json::{json, Map, Value};
use store::{FaceRef, SetRoot, StoreError};
use tokio_stream::wrappers::BroadcastStream;

use crate::history::{self, Action, ActionKind, FileSnap, History, StepError};
use crate::{close_set, cors, open_set, App, Change};

pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<StoreError> for ApiError {
    fn from(err: StoreError) -> Self {
        match err {
            StoreError::Conflict => ApiError::new(StatusCode::CONFLICT, "revision conflict"),
            StoreError::NotFound => ApiError::new(StatusCode::NOT_FOUND, "not found"),
            StoreError::Message(message) => ApiError::new(StatusCode::BAD_REQUEST, message),
            StoreError::Io(err) => ApiError::new(StatusCode::BAD_REQUEST, err.to_string()),
            StoreError::Document(err) => ApiError::new(StatusCode::BAD_REQUEST, err.to_string()),
        }
    }
}

impl From<MseError> for ApiError {
    fn from(err: MseError) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, err.to_string())
    }
}

impl From<DocumentError> for ApiError {
    fn from(err: DocumentError) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, err.to_string())
    }
}

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/sets/init", post(init_set))
        .route("/api/sets/open", post(open))
        .route("/api/sets/close", post(close))
        .route("/api/sets", get(list_sets))
        .route("/api/set", get(get_set).put(put_set))
        .route("/api/cards", get(list_cards).post(create_card))
        .route(
            "/api/cards/{uuid}",
            get(get_card).put(put_card).delete(delete_card),
        )
        .route("/api/cards/{uuid}/art", post(set_art))
        .route("/api/cards/{uuid}/picture", get(picture))
        .route("/api/keywords", get(list_keywords).post(create_keyword))
        .route(
            "/api/keywords/{id}",
            get(get_keyword).put(put_keyword).delete(delete_keyword),
        )
        .route("/api/render", post(render_all))
        .route("/api/render/{uuid}", post(render_one))
        .route("/api/validate", post(validate))
        .route("/api/import", post(import_set))
        .route("/api/export", post(export_set))
        .route("/api/export/formats", get(export_formats))
        .route("/api/history", get(get_history))
        .route("/api/history/undo", post(undo))
        .route("/api/history/redo", post(redo))
        .route("/api/pack", get(pack_status))
        .route("/api/pack/download", post(pack_download))
        .route("/events", get(events))
        .route("/terminal", get(terminal))
        .layer(axum::middleware::from_fn_with_state(app.clone(), require_auth))
        .layer(cors())
        .with_state(app)
}

pub async fn require_auth(
    State(app): State<Arc<App>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    app.touch();
    if request.method() == Method::OPTIONS || request.uri().path() == "/terminal" {
        return Ok(next.run(request).await);
    }
    let path = request.uri().path();
    if path != "/events" && !path.starts_with("/api") {
        return Ok(next.run(request).await);
    }
    let header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match header {
        Some(token) if token == app.token => Ok(next.run(request).await),
        _ => Err(ApiError::new(StatusCode::UNAUTHORIZED, "missing bearer token")),
    }
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true }))
}

async fn init_set(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    let path = required_path(&body, "path")?;
    let name = body.get("name").and_then(|value| value.as_str()).unwrap_or("Untitled Set");
    let code = body.get("code").and_then(|value| value.as_str()).unwrap_or("SET");
    SetRoot::init(&path, name, code)?;
    let path = open_set(&app, &path)?;
    Ok(Json(json!({ "path": path.display().to_string() })))
}

async fn open(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    let path = open_set(&app, &required_path(&body, "path")?)?;
    Ok(Json(json!({ "path": path.display().to_string() })))
}

async fn close(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    close_set(&app, &required_path(&body, "path")?);
    Ok(Json(json!({ "ok": true })))
}

async fn list_sets(State(app): State<Arc<App>>) -> Json<Value> {
    let sets = app
        .opened
        .lock()
        .expect("sets")
        .keys()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    Json(json!({ "sets": sets }))
}

async fn get_set(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let (value, revision) = SetRoot::open(&set)?.read_set()?;
    Ok(with_revision(value, &revision, StatusCode::OK))
}

async fn put_set(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let expected = if_match(&headers)?;
    let (current, revision) = root.read_set()?;
    if revision != expected {
        return Err(ApiError::new(StatusCode::CONFLICT, "revision conflict"));
    }
    let mut document = object_map(current)?;
    if let Some(patch) = body.as_object() {
        merge_object(&mut document, patch.clone());
    }
    let path = root.resolve("set.json")?;
    let before = vec![history::snap(&path)];
    let revision = root.write_set(&Value::Object(document.clone()), Some(&expected))?;
    record(&app, &set, "Edit set", &[path], before);
    Ok(with_revision(Value::Object(document), &revision, StatusCode::OK))
}

async fn list_cards(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let mut cards = Vec::new();
    for face in root.list_faces()? {
        cards.push(summary(&face));
    }
    Ok(Json(json!({ "cards": cards })))
}

async fn create_card(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let (set_doc, _) = root.read_set()?;
    let code = set_doc.get("code").and_then(|value| value.as_str()).unwrap_or("SET");
    let numbers = root
        .list_faces()?
        .into_iter()
        .filter(|face| face.folder == "cards")
        .map(|face| string_field(face.value.as_object().expect("face"), "number"))
        .collect::<Vec<_>>();
    let mut fields = NewCreature::default();
    fields.number = next_collector_number(&numbers);
    let mut card = object_map(new_creature(code, fields))?;
    if let Some(patch) = body.as_object() {
        merge_card(&mut card, patch.clone());
    }
    derive_face(&mut card).map_err(|err| ApiError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_supported_frame(&card) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "only a normal creature can be created",
        ));
    }
    if app.config.lock().expect("config").auto_render {
        mark_render(&mut card);
    }
    let uuid = string_field(&card, "uuid");
    let path = root.resolve(&format!("cards/{uuid}.json"))?;
    let before = vec![history::snap(&path)];
    let name = string_field(&card, "name");
    let revision = root.write_face("cards", &uuid, &Value::Object(card.clone()), None)?;
    record(&app, &set, format!("Create {name}"), &[path], before);
    paint(&app, &root, &card);
    Ok(with_revision(Value::Object(card), &revision, StatusCode::CREATED))
}

async fn get_card(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
) -> Result<Response, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let face = root.read_face(&uuid)?;
    Ok(with_revision(face.value, &face.revision, StatusCode::OK))
}

async fn put_card(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let expected = if_match(&headers)?;
    let face = root.read_face(&uuid)?;
    if face.revision != expected {
        return Err(ApiError::new(StatusCode::CONFLICT, "revision conflict"));
    }
    let mut card = object_map(face.value)?;
    if let Some(patch) = body.as_object() {
        merge_card(&mut card, patch.clone());
    }
    derive_face(&mut card).map_err(|err| ApiError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !is_supported_frame(&card) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "the field editor only saves a normal creature",
        ));
    }
    if app.config.lock().expect("config").auto_render {
        mark_render(&mut card);
    }
    let path = root.resolve(&format!("{}/{uuid}.json", face.folder))?;
    let before = vec![history::snap(&path)];
    let name = string_field(&card, "name");
    let revision = root.write_face(&face.folder, &uuid, &Value::Object(card.clone()), Some(&expected))?;
    record(&app, &set, format!("Edit {name}"), &[path], before);
    paint(&app, &root, &card);
    Ok(with_revision(Value::Object(card), &revision, StatusCode::OK))
}

async fn delete_card(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let face = root.read_face(&uuid)?;
    let path = root.resolve(&format!("{}/{uuid}.json", face.folder))?;
    let before = vec![history::snap(&path)];
    let name = face
        .value
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("card");
    root.delete_face(&uuid)?;
    let _ = root.remove_rel(&format!("renders/{uuid}.png"));
    record(&app, &set, format!("Delete {name}"), &[path], before);
    Ok(Json(json!({ "uuid": uuid })))
}

async fn set_art(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let expected = if_match(&headers)?;
    let source = body
        .get("path")
        .and_then(|value| value.as_str())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "path is required"))?;
    let bytes = fs::read(source).map_err(|_| {
        ApiError::new(StatusCode::BAD_REQUEST, format!("illustration {source} was not found"))
    })?;
    let ext = Path::new(source)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "illustration must be a png, jpg, webp, or gif",
        ));
    }
    let face = root.read_face(&uuid)?;
    if face.revision != expected {
        return Err(ApiError::new(StatusCode::CONFLICT, "revision conflict"));
    }
    let mut card = object_map(face.value)?;
    let new_rel = format!("art/{uuid}.{ext}");
    let old_rel = card
        .get("boggled")
        .and_then(|value| value.as_object())
        .and_then(|studio| studio.get("artFile"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let mut paths = vec![
        root.resolve(&format!("{}/{uuid}.json", face.folder))?,
        root.resolve(&new_rel)?,
    ];
    if let Some(old) = &old_rel {
        if old != &new_rel {
            paths.push(root.resolve(old)?);
        }
    }
    let before = history::snaps(&paths);
    root.write_bytes(&new_rel, &bytes)?;
    let studio = card
        .entry("boggled")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "boggled is not an object"))?;
    studio.insert("artFile".into(), json!(new_rel));
    if let Some(old) = old_rel {
        if old != format!("art/{uuid}.{ext}") {
            let _ = root.remove_rel(&old);
        }
    }
    let revision = root.write_face(&face.folder, &uuid, &Value::Object(card.clone()), Some(&expected))?;
    record(&app, &set, "Set illustration", &paths, before);
    paint(&app, &root, &card);
    Ok(with_revision(Value::Object(card), &revision, StatusCode::OK))
}

async fn picture(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
) -> Result<Response, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let face = root.read_face(&uuid)?;
    let card = object_map(face.value)?;
    if !is_supported_frame(&card) {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "unsupported frame"));
    }
    let rel = format!("renders/{uuid}.png");
    #[cfg(windows)]
    if let Some(preview) = native_preview(&app, &root, &card)? {
        if root.read_bytes(&rel).ok().as_deref() != Some(&preview.result.png) {
            root.write_bytes(&rel, &preview.result.png)?;
        }
        return Ok(([
            ("content-type", "image/png".to_owned()),
            ("x-boggled-layout", preview.result.layout.to_string()),
            ("x-revision", preview.face_revision.clone()),
        ], preview.result.png.clone()).into_response());
    }
    let bytes = match root.read_bytes(&rel) {
        Ok(bytes) => bytes,
        Err(_) => {
            let bytes = render_card(&app, &root, &card)?;
            root.write_bytes(&rel, &bytes)?;
            bytes
        }
    };
    Ok(([(header::CONTENT_TYPE, "image/png")], bytes).into_response())
}

async fn list_keywords(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let mut keywords = Vec::new();
    for (id, value, revision) in root.list_keywords()? {
        keywords.push(json!({ "id": id, "revision": revision, "keyword": value }));
    }
    Ok(Json(json!({ "keywords": keywords })))
}

async fn create_keyword(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let name = body.get("name").and_then(|value| value.as_str()).unwrap_or("Keyword");
    let id = body
        .get("id")
        .and_then(|value| value.as_str())
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| slug(name));
    let value = new_keyword(
        &id,
        name,
        body.get("reminder").and_then(|value| value.as_str()).unwrap_or(""),
        body.get("rules").and_then(|value| value.as_str()).unwrap_or(""),
    );
    let path = root.resolve(&format!("keywords/{id}.json"))?;
    let before = vec![history::snap(&path)];
    let revision = root.write_keyword(&id, &value, None)?;
    record(&app, &set, format!("Create keyword {name}"), &[path], before);
    Ok(with_revision(value, &revision, StatusCode::CREATED))
}

async fn get_keyword(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let (value, revision) = root.read_keyword(&id)?;
    Ok(with_revision(value, &revision, StatusCode::OK))
}

async fn put_keyword(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
    Json(body): Json<Value>,
) -> Result<Response, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let expected = if_match(&headers)?;
    let (current, revision) = root.read_keyword(&id)?;
    if revision != expected {
        return Err(ApiError::new(StatusCode::CONFLICT, "revision conflict"));
    }
    let mut keyword = object_map(current)?;
    if let Some(patch) = body.as_object() {
        for (key, value) in patch {
            if key != "id" {
                keyword.insert(key.clone(), value.clone());
            }
        }
    }
    let path = root.resolve(&format!("keywords/{id}.json"))?;
    let before = vec![history::snap(&path)];
    let name = string_field(&keyword, "name");
    let revision = root.write_keyword(&id, &Value::Object(keyword.clone()), Some(&expected))?;
    record(&app, &set, format!("Edit keyword {name}"), &[path], before);
    Ok(with_revision(Value::Object(keyword), &revision, StatusCode::OK))
}

async fn delete_keyword(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let path = root.resolve(&format!("keywords/{id}.json"))?;
    let before = vec![history::snap(&path)];
    root.delete_keyword(&id)?;
    record(&app, &set, format!("Delete keyword {id}"), &[path], before);
    Ok(Json(json!({ "id": id })))
}

async fn render_one(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    UrlPath(uuid): UrlPath<String>,
) -> Result<Json<Value>, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let face = root.read_face(&uuid)?;
    let card = object_map(face.value)?;
    let bytes = render_card(&app, &root, &card)?;
    root.write_bytes(&format!("renders/{uuid}.png"), &bytes)?;
    Ok(Json(json!({ "rendered": [uuid], "failed": [] })))
}

async fn render_all(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let mut rendered = Vec::new();
    let mut failed = Vec::new();
    for face in root.list_faces()? {
        let uuid = face.uuid.clone();
        match object_map(face.value).and_then(|card| {
            render_card(&app, &root, &card).map(|bytes| (card, bytes)).map_err(|err| {
                domain::DocumentError::Message(err.to_string())
            })
        }) {
            Ok((card, bytes)) => {
                let _ = card;
                if root.write_bytes(&format!("renders/{uuid}.png"), &bytes).is_ok() {
                    rendered.push(uuid);
                }
            }
            Err(err) => failed.push(json!({ "uuid": uuid, "error": err.to_string() })),
        }
    }
    Ok(Json(json!({ "rendered": rendered, "failed": failed })))
}

async fn validate(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let root = SetRoot::open(&opened(&app, &headers)?)?;
    let (set, _) = root.read_set()?;
    let mut warnings = Vec::new();
    if let Some(document) = set.as_object() {
        for warning in validate_set(document) {
            warnings.push(json!({ "path": format!("set.{}", warning.path), "message": warning.message }));
        }
    }
    let code = set.get("code").and_then(|value| value.as_str()).unwrap_or("");
    for face in root.list_faces()? {
        if let Some(card) = face.value.as_object() {
            for warning in validate_face(card, code) {
                warnings.push(json!({
                    "path": format!("{}/{}.{}", face.folder, face.uuid, warning.path),
                    "message": warning.message,
                }));
            }
        }
    }
    Ok(Json(json!({ "warnings": warnings })))
}

async fn import_set(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Result<Json<Value>, ApiError> {
    let source = required_path(&body, "source")?;
    let dest = required_path(&body, "dest")?;
    let report = import::import_mse_set(&source, &dest).map_err(|err| ApiError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    let dest = open_set(&app, &dest)?;
    let after = history::snapshot_tree(&dest);
    app.history.lock().expect("history").entry(dest.clone()).or_default().push(Action {
        label: "Import set".into(),
        kind: ActionKind::Tree { root: dest.clone() },
        before: Vec::new(),
        after,
    });
    Ok(Json(json!({
        "setPath": dest.display().to_string(),
        "cards": report.cards,
        "tokens": report.tokens,
        "keywords": report.keywords,
        "warnings": report.warnings,
    })))
}

async fn export_set(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let set = opened(&app, &headers)?;
    let root = SetRoot::open(&set)?;
    let format = body.get("format").and_then(|value| value.as_str()).unwrap_or("mtgjson");
    let dest = if let Some(path) = body.get("path").and_then(|value| value.as_str()) {
        PathBuf::from(path)
    } else {
        let (document, _) = root.read_set()?;
        let code = document.get("code").and_then(|value| value.as_str()).unwrap_or("set");
        set.join("export").join(format!("{code}.json"))
    };
    let before = vec![history::snap(&dest)];
    export::export_set(&root, format, &dest).map_err(|err| ApiError::new(StatusCode::BAD_REQUEST, err.to_string()))?;
    record(&app, &set, format!("Export {format}"), &[dest.clone()], before);
    Ok(Json(json!({ "path": dest.display().to_string(), "format": format })))
}

async fn export_formats() -> Json<Value> {
    Json(json!({ "formats": export::formats() }))
}

async fn get_history(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    let set = history_target(&app, &headers)?;
    Ok(Json(history_body(&app, &set)))
}

async fn undo(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    step(&app, &headers, true)
}

async fn redo(State(app): State<Arc<App>>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    step(&app, &headers, false)
}

fn step(app: &App, headers: &HeaderMap, undoing: bool) -> Result<Json<Value>, ApiError> {
    let set = history_target(app, headers)?;
    if undoing {
        let root = {
            let stacks = app.history.lock().expect("history");
            stacks.get(&set).and_then(|stack| stack.undo.last()).and_then(|action| {
                match &action.kind {
                    ActionKind::Tree { root } => Some(root.clone()),
                    ActionKind::Files => None,
                }
            })
        };
        if let Some(root) = root {
            // Drop the directory watcher before deleting the imported folder.
            close_set(app, &root);
        }
    }
    let action = {
        let mut stacks = app.history.lock().expect("history");
        let stack = stacks.entry(set.clone()).or_default();
        let result = if undoing { stack.undo() } else { stack.redo() };
        match result {
            Ok(action) => action,
            Err(StepError::Empty) => {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    if undoing { "nothing to undo" } else { "nothing to redo" },
                ))
            }
            Err(StepError::Conflict) => {
                if undoing {
                    let _ = open_set(app, &set);
                }
                return Err(ApiError::new(
                    StatusCode::CONFLICT,
                    "the files changed outside this history",
                ))
            }
            Err(StepError::Io(message)) => return Err(ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, message)),
        }
    };
    match &action.kind {
        ActionKind::Tree { root } => {
            if undoing {
                close_set(app, root);
            } else {
                let _ = open_set(app, root);
            }
        }
        ActionKind::Files => {
            let snaps = if undoing { &action.before } else { &action.after };
            if let Ok(root) = SetRoot::open(&set) {
                rerender(app, &root, snaps);
            }
        }
    }
    Ok(Json(history_body(app, &set)))
}

async fn pack_status(State(app): State<Arc<App>>) -> Json<Value> {
    let pack = app.config.lock().expect("config").pack_path.clone();
    Json(json!({
        "ready": mse::require_pack(&pack).is_ok(),
        "path": pack.display().to_string(),
        "commit": domain::FMP_COMMIT,
    }))
}

async fn pack_download(State(app): State<Arc<App>>) -> Result<Json<Value>, ApiError> {
    let pack = app.config.lock().expect("config").pack_path.clone();
    let bytes = reqwest::Client::new()
        .get(mse::pack_zip_url())
        .send()
        .await
        .map_err(|err| ApiError::new(StatusCode::BAD_GATEWAY, err.to_string()))?
        .bytes()
        .await
        .map_err(|err| ApiError::new(StatusCode::BAD_GATEWAY, err.to_string()))?;
    mse::extract_pack_zip(&bytes, &pack)?;
    Ok(Json(json!({ "ready": true, "path": pack.display().to_string() })))
}

async fn events(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let set = opened(&app, &headers)?;
    let rx = app
        .opened
        .lock()
        .expect("sets")
        .get(&set)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "set is not open"))?
        .events
        .subscribe();
    app.sse.fetch_add(1, Ordering::SeqCst);
    let guard = SseGuard(Arc::clone(&app));
    let stream = BroadcastStream::new(rx).map(move |item| {
        let _hold = &guard;
        let change = match item {
            Ok(change) => change,
            Err(_) => Change {
                name: "rescan".into(),
                data: "{}".into(),
            },
        };
        Ok(Event::default().event(change.name).data(change.data))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

struct SseGuard(Arc<App>);

impl Drop for SseGuard {
    fn drop(&mut self) {
        self.0.sse.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn terminal(State(app): State<Arc<App>>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| terminal_session(socket, app))
}

async fn terminal_session(mut socket: WebSocket, app: Arc<App>) {
    let Some(Ok(Message::Text(text))) = socket.recv().await else {
        return;
    };
    let Ok(message) = serde_json::from_str::<Value>(&text.to_string()) else {
        return;
    };
    if message.get("token").and_then(|value| value.as_str()) != Some(app.token.as_str()) {
        let _ = socket
            .send(Message::Text(r#"{"type":"error","error":"bad bearer token"}"#.into()))
            .await;
        return;
    }
    let Some(set_text) = message.get("set").and_then(|value| value.as_str()) else {
        return;
    };
    let Ok(set) = open_set(&app, Path::new(set_text)) else {
        let _ = socket
            .send(Message::Text(r#"{"type":"error","error":"set is not open"}"#.into()))
            .await;
        return;
    };
    let cols = message.get("cols").and_then(|value| value.as_u64()).unwrap_or(80) as u16;
    let rows = message.get("rows").and_then(|value| value.as_u64()).unwrap_or(24) as u16;
    let existing = message.get("session").and_then(|value| value.as_str()).map(str::to_string);
    let session = if let Some(id) = existing {
        if app.terminal.output(&id).is_err() {
            return;
        }
        id
    } else {
        let command = app.config.lock().expect("config").terminal_command.clone();
        let (program, args) = terminal::split_command(&command);
        match app.terminal.open(&set, &program, &args, cols, rows) {
            Ok(id) => id,
            Err(err) => {
                let body = json!({ "type": "error", "error": err.to_string() }).to_string();
                let _ = socket.send(Message::Text(body.into())).await;
                return;
            }
        }
    };
    let started = json!({ "type": "started", "session": session }).to_string();
    if socket.send(Message::Text(started.into())).await.is_err() {
        return;
    }
    if let Ok(buffered) = app.terminal.output(&session) {
        if !buffered.is_empty() && socket.send(Message::Binary(Bytes::from(buffered))).await.is_err() {
            return;
        }
    }
    let Ok(mut live) = app.terminal.subscribe(&session) else {
        return;
    };
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(frame)) = incoming else { break; };
                match frame {
                    Message::Binary(bytes) => { let _ = app.terminal.write(&session, &bytes); }
                    Message::Text(text) => {
                        let text = text.to_string();
                        if let Ok(control) = serde_json::from_str::<Value>(&text) {
                            match control.get("type").and_then(|value| value.as_str()) {
                                Some("resize") => {
                                    let cols = control.get("cols").and_then(|value| value.as_u64()).unwrap_or(80) as u16;
                                    let rows = control.get("rows").and_then(|value| value.as_u64()).unwrap_or(24) as u16;
                                    let _ = app.terminal.resize(&session, cols, rows);
                                }
                                Some("close") => {
                                    let _ = app.terminal.close(&session);
                                    break;
                                }
                                _ => { let _ = app.terminal.write(&session, text.as_bytes()); }
                            }
                        } else {
                            let _ = app.terminal.write(&session, text.as_bytes());
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            chunk = live.recv() => {
                let Ok(chunk) = chunk else { break; };
                if socket.send(Message::Binary(Bytes::from(chunk))).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// History survives closing a set, including an import undo that deletes the folder.
fn history_target(app: &App, headers: &HeaderMap) -> Result<PathBuf, ApiError> {
    let value = headers
        .get("x-boggled-set")
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "X-Boggled-Set is required"))?;
    let text = value
        .to_str()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "X-Boggled-Set is required"))?;
    let path = crate::normalize(Path::new(text));
    let open = app.opened.lock().expect("sets").contains_key(&path);
    let known = app.history.lock().expect("history").contains_key(&path);
    if open || known {
        Ok(path)
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "set is not open"))
    }
}

fn opened(app: &App, headers: &HeaderMap) -> Result<PathBuf, ApiError> {
    let value = headers
        .get("x-boggled-set")
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "X-Boggled-Set is required"))?;
    let text = value
        .to_str()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "X-Boggled-Set is required"))?;
    let path = crate::normalize(Path::new(text));
    if !app.opened.lock().expect("sets").contains_key(&path) {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "set is not open"));
    }
    Ok(path)
}

fn if_match(headers: &HeaderMap) -> Result<String, ApiError> {
    let value = headers.get(header::IF_MATCH).ok_or_else(|| {
        ApiError::new(StatusCode::PRECONDITION_REQUIRED, "If-Match is required")
    })?;
    let text = value
        .to_str()
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "If-Match is required"))?;
    Ok(text.trim().trim_matches('"').to_string())
}

fn required_path(body: &Value, key: &str) -> Result<PathBuf, ApiError> {
    body.get(key)
        .and_then(|value| value.as_str())
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, format!("{key} is required")))
}

fn with_revision(body: Value, revision: &str, status: StatusCode) -> Response {
    let mut response = (status, Json(body)).into_response();
    if let Ok(value) = header::HeaderValue::from_str(&format!("\"{revision}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) = header::HeaderValue::from_str(revision) {
        response
            .headers_mut()
            .insert(header::HeaderName::from_static("x-revision"), value);
    }
    response
}

fn summary(face: &FaceRef) -> Value {
    let card = face.value.as_object();
    let text = |key: &str| {
        card.and_then(|card| card.get(key))
            .cloned()
            .unwrap_or(Value::String(String::new()))
    };
    json!({
        "folder": face.folder,
        "uuid": face.uuid,
        "revision": face.revision,
        "name": text("name"),
        "manaCost": text("manaCost"),
        "manaValue": card.and_then(|card| card.get("manaValue")).cloned().unwrap_or(json!(0)),
        "type": text("type"),
        "power": text("power"),
        "toughness": text("toughness"),
        "rarity": text("rarity"),
        "number": text("number"),
        "colors": card.and_then(|card| card.get("colors")).cloned().unwrap_or(json!([])),
        "layout": text("layout"),
        "supported": card.map(is_supported_frame).unwrap_or(false),
    })
}

fn merge_card(card: &mut Map<String, Value>, patch: Map<String, Value>) {
    for (key, value) in patch {
        if key == "uuid" {
            continue;
        }
        if key == "boggled" {
            if let Some(src) = value.as_object() {
                let studio = card.entry("boggled").or_insert_with(|| json!({}));
                if let Some(dst) = studio.as_object_mut() {
                    for (child, child_value) in src {
                        dst.insert(child.clone(), child_value.clone());
                    }
                }
            }
            continue;
        }
        card.insert(key, value);
    }
}

fn merge_object(document: &mut Map<String, Value>, patch: Map<String, Value>) {
    merge_card(document, patch);
}

fn mark_render(card: &mut Map<String, Value>) {
    let uuid = string_field(card, "uuid");
    if let Some(studio) = card.get_mut("boggled").and_then(|value| value.as_object_mut()) {
        studio.insert("renderFile".into(), json!(format!("renders/{uuid}.png")));
    }
}

fn record(app: &App, set: &Path, label: impl Into<String>, paths: &[PathBuf], before: Vec<FileSnap>) {
    let after = history::snaps(paths);
    app.history.lock().expect("history").entry(set.to_path_buf()).or_default().push(Action {
        label: label.into(),
        kind: ActionKind::Files,
        before,
        after,
    });
}

fn history_body(app: &App, set: &Path) -> Value {
    let stacks = app.history.lock().expect("history");
    let (undo, redo) = stacks.get(set).map(History::labels).unwrap_or_default();
    json!({
        "undo": undo.into_iter().rev().collect::<Vec<_>>(),
        "redo": redo.into_iter().rev().collect::<Vec<_>>(),
    })
}

fn paint(app: &App, root: &SetRoot, card: &Map<String, Value>) {
    if !app.config.lock().expect("config").auto_render || !is_supported_frame(card) {
        return;
    }
    let Ok(bytes) = render_card(app, root, card) else {
        return;
    };
    let uuid = string_field(card, "uuid");
    let _ = root.write_bytes(&format!("renders/{uuid}.png"), &bytes);
}

fn rerender(app: &App, root: &SetRoot, snaps: &[FileSnap]) {
    for snap in snaps {
        let Some(uuid) = face_uuid(&snap.path) else {
            continue;
        };
        if !snap.existed {
            let _ = root.remove_rel(&format!("renders/{uuid}.png"));
            continue;
        }
        if let Ok(face) = root.read_face(&uuid) {
            if let Ok(card) = object_map(face.value) {
                paint(app, root, &card);
            }
        }
    }
}

fn face_uuid(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let uuid = name.strip_suffix(".json")?;
    let folder = path.parent()?.file_name()?.to_str()?;
    if folder == "cards" || folder == "tokens" {
        Some(uuid.to_string())
    } else {
        None
    }
}

fn render_card(app: &App, root: &SetRoot, card: &Map<String, Value>) -> Result<Vec<u8>, MseError> {
    if !is_supported_frame(card) {
        return Err(MseError::UnsupportedFrame);
    }
    #[cfg(windows)]
    if let Some(preview) = native_preview(app, root, card)? {
        return Ok(preview.result.png.clone());
    }
    let pack = app.config.lock().expect("config").pack_path.clone();
    let data = pack.join("data");
    let fonts = pack.join("Magic - Fonts");
    let art = art_bytes(root, card);
    draw_card(DrawRequest {
        card,
        art: art.as_deref(),
        style_text: None,
        data_dir: data.is_dir().then_some(data.as_path()),
        fonts_dir: fonts.is_dir().then_some(fonts.as_path()),
        allow_unlisted: false,
    })
}

#[cfg(windows)]
pub(super) struct NativePicture {
    face_revision: String,
    result: mse::native::Preview,
}

#[cfg(windows)]
fn native_preview(app: &App, root: &SetRoot, card: &Map<String, Value>) -> Result<Option<Arc<NativePicture>>, MseError> {
    use mse::native::Face;
    use sha2::{Digest, Sha256};
    // Keep the most recent image and layout together, including across a save
    // followed by a picture request. Serialize the native graphics processes.
    let pack = app.config.lock().expect("config").pack_path.clone();
    if !pack.join("data").is_dir() { return Ok(None); } // offline placeholder
    let mut last = app.native_cache.lock().map_err(|_| MseError::Message("render lock unavailable".into()))?;
    let err = |e: StoreError| MseError::Message(e.to_string());
    let (set, set_rev) = root.read_set().map_err(err)?;
    let mut hash = Sha256::new();
    hash.update(pack.to_string_lossy().as_bytes());
    hash.update(set_rev);
    let mut faces = Vec::new();
    let mut target = None;
    for entry in root.list_faces().map_err(err)? {
        let map = object_map(entry.value).map_err(|e| MseError::Message(e.to_string()))?;
        hash.update(entry.revision.as_bytes());
        // M15 uses other faces for numbering but only this face's illustration.
        let current = map.get("uuid") == card.get("uuid");
        if current {
            target = Some((faces.len(), entry.revision));
        }
        let art_path = map.get("boggled").and_then(|s| s["artFile"].as_str());
        let art = if let Some(rel) = art_path.filter(|s| current && !s.is_empty()) {
            let bytes = root.read_bytes(rel).map_err(err)?;
            hash.update(Sha256::digest(&bytes));
            Some(bytes)
        } else { None };
        faces.push(Face { card: map, art });
    }
    let (index, face_revision) = target.ok_or_else(|| MseError::Message("card was removed".into()))?;
    hash.update(index.to_le_bytes());
    let key: [u8; 32] = hash.finalize().into();
    if let Some((previous, preview)) = last.as_ref() {
        if *previous == key { return Ok(Some(preview.clone())); }
    }
    let preview = Arc::new(NativePicture {
        result: mse::native::render(&pack, &set, &faces, index)?,
        face_revision,
    });
    *last = Some((key, preview.clone()));
    Ok(Some(preview))
}

fn art_bytes(root: &SetRoot, card: &Map<String, Value>) -> Option<Vec<u8>> {
    let rel = card
        .get("boggled")
        .and_then(|value| value.as_object())
        .and_then(|studio| studio.get("artFile"))
        .and_then(|value| value.as_str())?;
    root.read_bytes(rel).ok()
}

fn slug(name: &str) -> String {
    let mut id = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            id.push(ch.to_ascii_lowercase());
        } else if !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_matches('-').to_string();
    if id.is_empty() {
        "keyword".into()
    } else {
        id
    }
}
