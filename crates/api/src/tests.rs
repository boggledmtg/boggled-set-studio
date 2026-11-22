//! HTTP coverage for the edits a person can make, and for undoing each one.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use store::SetRoot;
use tower::ServiceExt;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{router, App};

struct Api {
    app: Arc<App>,
    home: PathBuf,
}

struct Reply {
    status: StatusCode,
    revision: Option<String>,
    body: Value,
}

impl Api {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!("boggled-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&home).unwrap();
        Self {
            app: App::new(home.clone(), "test-token".into()),
            home,
        }
    }

    async fn send(
        &self,
        method: &str,
        uri: &str,
        set: Option<&Path>,
        if_match: Option<&str>,
        body: Option<Value>,
        auth: bool,
    ) -> Reply {
        let mut builder = Request::builder().method(method).uri(uri);
        if auth {
            builder = builder.header(header::AUTHORIZATION, "Bearer test-token");
        }
        if let Some(set) = set {
            builder = builder.header("x-boggled-set", set.display().to_string());
        }
        if let Some(revision) = if_match {
            builder = builder.header(header::IF_MATCH, revision);
        }
        let request = match body {
            Some(body) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        };
        let response = router(self.app.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let revision = response
            .headers()
            .get("x-revision")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Reply {
            status,
            revision,
            body,
        }
    }

    async fn init(&self, name: &str, code: &str) -> PathBuf {
        let path = self.home.join(code);
        let reply = self
            .send(
                "POST",
                "/api/sets/init",
                None,
                None,
                Some(json!({ "path": path, "name": name, "code": code })),
                true,
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        PathBuf::from(reply.body["path"].as_str().unwrap())
    }

    async fn create(&self, set: &Path, name: &str) -> (String, String) {
        let reply = self
            .send(
                "POST",
                "/api/cards",
                Some(set),
                None,
                Some(json!({
                    "name": name,
                    "manaCost": "{2}{U}",
                    "type": "Artifact Creature — Bird",
                    "power": "1",
                    "toughness": "3",
                })),
                true,
            )
            .await;
        assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
        (
            reply.body["uuid"].as_str().unwrap().to_string(),
            reply.revision.expect("revision"),
        )
    }

    async fn history(&self, set: &Path) -> (Vec<String>, Vec<String>) {
        let reply = self
            .send("GET", "/api/history", Some(set), None, None, true)
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        (strings(&reply.body["undo"]), strings(&reply.body["redo"]))
    }

    async fn undo(&self, set: &Path) -> Reply {
        self.send("POST", "/api/history/undo", Some(set), None, None, true)
            .await
    }

    async fn redo(&self, set: &Path) -> Reply {
        self.send("POST", "/api/history/redo", Some(set), None, None, true)
            .await
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect()
}

fn card_file(set: &Path, uuid: &str) -> PathBuf {
    set.join("cards").join(format!("{uuid}.json"))
}

#[tokio::test]
async fn a_missing_token_is_rejected() {
    let api = Api::new();
    let reply = api
        .send("GET", "/api/health", None, None, None, false)
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    let ok = api.send("GET", "/api/health", None, None, None, true).await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[tokio::test]
async fn set_init_does_not_start_history() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    assert_eq!(api.history(&set).await, (vec![], vec![]));
    let reply = api.undo(&set).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn creating_a_card_undoes_and_redoes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, _) = api.create(&set, "Brass Owl").await;
    assert!(card_file(&set, &uuid).is_file());
    assert!(set.join("renders").join(format!("{uuid}.png")).is_file());
    assert_eq!(
        api.history(&set).await.0,
        vec!["Create Brass Owl".to_string()]
    );

    let undone = api.undo(&set).await;
    assert_eq!(undone.status, StatusCode::OK, "{}", undone.body);
    assert!(!card_file(&set, &uuid).is_file());
    assert!(!set.join("renders").join(format!("{uuid}.png")).is_file());
    assert_eq!(undone.body["redo"][0], json!("Create Brass Owl"));

    let redone = api.redo(&set).await;
    assert_eq!(redone.status, StatusCode::OK, "{}", redone.body);
    let card: Value =
        serde_json::from_slice(&std::fs::read(card_file(&set, &uuid)).unwrap()).unwrap();
    assert_eq!(card["name"], json!("Brass Owl"));
    assert_eq!(card["manaCost"], json!("{2}{U}"));
    assert!(set.join("renders").join(format!("{uuid}.png")).is_file());
}

#[tokio::test]
async fn editing_a_card_undoes_and_redoes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, revision) = api.create(&set, "Brass Owl").await;
    let reply = api
        .send(
            "PUT",
            &format!("/api/cards/{uuid}"),
            Some(&set),
            Some(&revision),
            Some(json!({ "name": "Copper Owl", "text": "Flying" })),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["name"], json!("Copper Owl"));
    assert_eq!(reply.body["colors"], json!(["U"]));

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    let current = api
        .send("GET", &format!("/api/cards/{uuid}"), Some(&set), None, None, true)
        .await;
    assert_eq!(current.body["name"], json!("Brass Owl"));

    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    let current = api
        .send("GET", &format!("/api/cards/{uuid}"), Some(&set), None, None, true)
        .await;
    assert_eq!(current.body["name"], json!("Copper Owl"));
    assert_eq!(current.body["text"], json!("Flying"));
}

#[tokio::test]
async fn a_stale_revision_is_rejected() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, _) = api.create(&set, "Brass Owl").await;
    let missing = api
        .send(
            "PUT",
            &format!("/api/cards/{uuid}"),
            Some(&set),
            None,
            Some(json!({ "name": "Nope" })),
            true,
        )
        .await;
    assert_eq!(missing.status, StatusCode::PRECONDITION_REQUIRED);
    let stale = api
        .send(
            "PUT",
            &format!("/api/cards/{uuid}"),
            Some(&set),
            Some("not-the-revision"),
            Some(json!({ "name": "Nope" })),
            true,
        )
        .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    assert_eq!(api.history(&set).await.0, vec!["Create Brass Owl".to_string()]);
}

#[tokio::test]
async fn deleting_a_card_undoes_and_redoes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, _) = api.create(&set, "Brass Owl").await;
    let reply = api
        .send(
            "DELETE",
            &format!("/api/cards/{uuid}"),
            Some(&set),
            None,
            None,
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert!(!card_file(&set, &uuid).is_file());

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(card_file(&set, &uuid).is_file());
    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    assert!(!card_file(&set, &uuid).is_file());
    let (undo, redo) = api.history(&set).await;
    assert_eq!(undo[0], "Delete Brass Owl");
    assert!(redo.is_empty());
}

#[tokio::test]
async fn setting_art_undoes_and_redoes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, revision) = api.create(&set, "Brass Owl").await;
    let picture = api.home.join("owl.png");
    std::fs::write(&picture, [137, 80, 78, 71, 1, 2, 3]).unwrap();
    let reply = api
        .send(
            "POST",
            &format!("/api/cards/{uuid}/art"),
            Some(&set),
            Some(&revision),
            Some(json!({ "path": picture })),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let art = set.join("art").join(format!("{uuid}.png"));
    assert_eq!(std::fs::read(&art).unwrap(), vec![137, 80, 78, 71, 1, 2, 3]);

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(!art.exists());
    let current = api
        .send("GET", &format!("/api/cards/{uuid}"), Some(&set), None, None, true)
        .await;
    assert!(current.body["boggled"].get("artFile").is_none());

    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    assert_eq!(std::fs::read(&art).unwrap(), vec![137, 80, 78, 71, 1, 2, 3]);
    assert_eq!(api.history(&set).await.0[0], "Set illustration");
}

#[tokio::test]
async fn editing_set_info_undoes_and_redoes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let current = api.send("GET", "/api/set", Some(&set), None, None, true).await;
    let reply = api
        .send(
            "PUT",
            "/api/set",
            Some(&set),
            current.revision.as_deref(),
            Some(json!({
                "name": "Steam Age",
                "boggled": { "designBrief": "Gears and birds" }
            })),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["name"], json!("Steam Age"));
    assert_eq!(reply.body["boggled"]["designBrief"], json!("Gears and birds"));
    assert_eq!(reply.body["code"], json!("STM"));

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    let restored = api.send("GET", "/api/set", Some(&set), None, None, true).await;
    assert_eq!(restored.body["name"], json!("Steam"));
    assert_eq!(restored.body["boggled"]["designBrief"], json!(""));

    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    let again = api.send("GET", "/api/set", Some(&set), None, None, true).await;
    assert_eq!(again.body["boggled"]["designBrief"], json!("Gears and birds"));
    assert_eq!(api.history(&set).await.0[0], "Edit set");
}

#[tokio::test]
async fn exporting_creates_a_file_that_undo_removes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    api.create(&set, "Brass Owl").await;
    let reply = api
        .send(
            "POST",
            "/api/export",
            Some(&set),
            None,
            Some(json!({})),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let dest = set.join("export").join("STM.json");
    assert!(dest.is_file());
    assert!(std::fs::read_to_string(&dest).unwrap().contains("Brass Owl"));
    assert!(!std::fs::read_to_string(&dest).unwrap().contains("boggled"));

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(!dest.exists());
    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    assert!(std::fs::read_to_string(&dest).unwrap().contains("Brass Owl"));
    assert_eq!(api.history(&set).await.0[0], "Export mtgjson");
}

#[tokio::test]
async fn exporting_over_an_existing_file_restores_those_bytes() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let dest = set.join("export").join("STM.json");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, b"previous-export").unwrap();
    let reply = api
        .send("POST", "/api/export", Some(&set), None, Some(json!({})), true)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_ne!(std::fs::read(&dest).unwrap(), b"previous-export");

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert_eq!(std::fs::read(&dest).unwrap(), b"previous-export");
    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    assert!(std::fs::read_to_string(&dest).unwrap().contains("\"code\": \"STM\""));
}

#[tokio::test]
async fn keyword_writes_undo_and_redo() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let created = api
        .send(
            "POST",
            "/api/keywords",
            Some(&set),
            None,
            Some(json!({ "name": "Flying", "reminder": "Flies." })),
            true,
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let path = set.join("keywords").join("flying.json");
    assert!(path.is_file());

    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(!path.exists());
    assert_eq!(api.redo(&set).await.status, StatusCode::OK);
    assert!(path.is_file());

    let edited = api
        .send(
            "PUT",
            "/api/keywords/flying",
            Some(&set),
            created.revision.as_deref(),
            Some(json!({ "reminder": "Can't be blocked except by fliers." })),
            true,
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    let current = api
        .send("GET", "/api/keywords/flying", Some(&set), None, None, true)
        .await;
    assert_eq!(current.body["reminder"], json!("Flies."));
    assert_eq!(api.redo(&set).await.status, StatusCode::OK);

    let removed = api
        .send(
            "DELETE",
            "/api/keywords/flying",
            Some(&set),
            None,
            None,
            true,
        )
        .await;
    assert_eq!(removed.status, StatusCode::OK, "{}", removed.body);
    assert!(!path.exists());
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(path.is_file());
    let (undo, redo) = api.history(&set).await;
    assert_eq!(
        undo,
        vec![
            "Edit keyword Flying".to_string(),
            "Create keyword Flying".to_string(),
        ]
    );
    assert_eq!(redo, vec!["Delete keyword flying".to_string()]);
}

#[tokio::test]
async fn importing_a_set_undoes_by_deleting_the_folder() {
    let api = Api::new();
    let source = mse_package(&api.home);
    let dest = api.home.join("Imported");
    let reply = api
        .send(
            "POST",
            "/api/import",
            None,
            None,
            Some(json!({ "source": source, "dest": dest })),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let set = PathBuf::from(reply.body["setPath"].as_str().unwrap());
    assert!(set.join("set.json").is_file());
    assert_eq!(api.history(&set).await.0, vec!["Import set".to_string()]);

    let undone = api.undo(&set).await;
    assert_eq!(undone.status, StatusCode::OK, "{}", undone.body);
    assert!(!set.exists());
    assert_eq!(undone.body["redo"][0], json!("Import set"));

    let redone = api.redo(&set).await;
    assert_eq!(redone.status, StatusCode::OK, "{}", redone.body);
    assert!(set.join("set.json").is_file());
    let root = SetRoot::open(&set).unwrap();
    assert!(root
        .list_faces()
        .unwrap()
        .iter()
        .any(|face| face.value["name"] == json!("Brass Owl")));
}

#[tokio::test]
async fn a_changed_import_refuses_undo_and_keeps_the_step() {
    let api = Api::new();
    let source = mse_package(&api.home);
    let reply = api
        .send(
            "POST",
            "/api/import",
            None,
            None,
            Some(json!({ "source": source, "dest": api.home.join("Imported") })),
            true,
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let set = PathBuf::from(reply.body["setPath"].as_str().unwrap());
    let current = api.send("GET", "/api/set", Some(&set), None, None, true).await;
    let edited = api
        .send(
            "PUT",
            "/api/set",
            Some(&set),
            current.revision.as_deref(),
            Some(json!({ "name": "Touched" })),
            true,
        )
        .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert_eq!(api.history(&set).await.1, vec!["Edit set".to_string()]);

    std::fs::write(set.join("extra.txt"), b"notes").unwrap();
    let refused = api.undo(&set).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert!(set.join("set.json").is_file());
    assert!(set.join("extra.txt").is_file());
    let (undo, redo) = api.history(&set).await;
    assert_eq!(undo, vec!["Import set".to_string()]);
    assert!(redo.is_empty());

    std::fs::remove_file(set.join("extra.txt")).unwrap();
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(!set.exists());
}

#[tokio::test]
async fn an_external_edit_blocks_undo_until_the_bytes_match() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, revision) = api.create(&set, "Brass Owl").await;
    let renamed = api
        .send(
            "PUT",
            &format!("/api/cards/{uuid}"),
            Some(&set),
            Some(&revision),
            Some(json!({ "name": "Copper Owl" })),
            true,
        )
        .await;
    assert_eq!(renamed.status, StatusCode::OK, "{}", renamed.body);
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    let (undo, redo) = api.history(&set).await;
    assert_eq!(undo, vec!["Create Brass Owl".to_string()]);
    assert_eq!(redo, vec!["Edit Copper Owl".to_string()]);

    let path = card_file(&set, &uuid);
    let saved = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"tampered").unwrap();
    let refused = api.undo(&set).await;
    assert_eq!(refused.status, StatusCode::CONFLICT, "{}", refused.body);
    assert_eq!(std::fs::read(&path).unwrap(), b"tampered");
    let (undo, redo) = api.history(&set).await;
    assert_eq!(undo, vec!["Create Brass Owl".to_string()]);
    assert!(redo.is_empty());

    let again = api.undo(&set).await;
    assert_eq!(again.status, StatusCode::CONFLICT);

    std::fs::write(&path, &saved).unwrap();
    assert_eq!(api.undo(&set).await.status, StatusCode::OK);
    assert!(!path.exists());
}

#[tokio::test]
async fn a_direct_disk_edit_is_not_a_history_step() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, _) = api.create(&set, "Brass Owl").await;
    let before = api.history(&set).await;
    std::fs::write(set.join("scratch.txt"), b"agent note").unwrap();
    let root = SetRoot::open(&set).unwrap();
    root.write_face(
        "cards",
        "instant01",
        &json!({
            "uuid": "instant01",
            "name": "Zap",
            "layout": "normal",
            "setCode": "STM",
            "type": "Instant",
            "manaCost": "{R}"
        }),
        None,
    )
    .unwrap();
    assert_eq!(api.history(&set).await, before);
    assert!(card_file(&set, &uuid).is_file());
    assert!(card_file(&set, "instant01").is_file());
}

#[tokio::test]
async fn rendering_paints_a_creature_and_reports_an_unsupported_card() {
    let api = Api::new();
    let set = api.init("Steam", "STM").await;
    let (uuid, _) = api.create(&set, "Brass Owl").await;
    let root = SetRoot::open(&set).unwrap();
    root.write_face(
        "cards",
        "instant01",
        &json!({
            "uuid": "instant01",
            "name": "Zap",
            "layout": "normal",
            "setCode": "STM",
            "type": "Instant",
            "manaCost": "{R}"
        }),
        None,
    )
    .unwrap();
    let before = api.history(&set).await;
    let reply = api
        .send("POST", "/api/render", Some(&set), None, None, true)
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let rendered = strings(&reply.body["rendered"]);
    assert!(rendered.contains(&uuid));
    let failed = reply.body["failed"].as_array().unwrap();
    assert!(failed.iter().any(|item| {
        item["uuid"] == json!("instant01") && item["error"].as_str().unwrap().contains("unsupported")
    }));
    let png = std::fs::read(set.join("renders").join(format!("{uuid}.png"))).unwrap();
    assert_eq!(&png[..4], &[137, 80, 78, 71]);
    assert_eq!(api.history(&set).await, before);

    let rejected = api
        .send(
            "POST",
            "/api/cards",
            Some(&set),
            None,
            Some(json!({ "name": "Shock", "type": "Instant" })),
            true,
        )
        .await;
    assert_eq!(rejected.status, StatusCode::BAD_REQUEST, "{}", rejected.body);
    assert_eq!(api.history(&set).await, before);
}

fn mse_package(dir: &Path) -> PathBuf {
    let path = dir.join("sample.mse-set");
    let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    zip.start_file("set", opts).unwrap();
    zip.write_all(
        b"\
mse version: 2.0.2
game: magic
stylesheet: magic-m15-altered.mse-style
set info:
\ttitle: Steam Test
\tset code: stm
keyword:
\tkeyword: Flying
\treminder: Flies.
card:
\tname: Brass Owl
\tcasting cost: 2U
\ttype: Creature - Bird
\tpower: 1
\ttoughness: 1
",
    )
    .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    std::fs::write(&path, bytes).unwrap();
    path
}
