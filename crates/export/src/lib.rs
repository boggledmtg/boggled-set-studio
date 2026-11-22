//! Assemble one file from a set folder.
//!
//! `mtgjson` is the only format in this version. Another format is a new arm
//! in `export_set` plus a test; the set folder stays the database either way.

use std::path::Path;

use domain::{canonical_face, canonical_set};
use serde_json::Value;
use store::SetRoot;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("unknown export format: {0}")]
    UnknownFormat(String),
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error("{0}")]
    Message(String),
}

pub fn formats() -> &'static [&'static str] {
    &["mtgjson"]
}

pub fn export_set(root: &SetRoot, format: &str, dest: &Path) -> Result<Vec<u8>, ExportError> {
    if !formats().contains(&format) {
        return Err(ExportError::UnknownFormat(format.to_string()));
    }
    let bytes = assemble_mtgjson(root)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| ExportError::Message(err.to_string()))?;
    }
    store::atomic_write(dest, &bytes)?;
    Ok(bytes)
}

pub fn assemble_mtgjson(root: &SetRoot) -> Result<Vec<u8>, ExportError> {
    let (mut set, _) = root.read_set()?;
    if let Some(object) = set.as_object_mut() {
        object.remove("boggled");
    }
    let text = canonical_set(&set);
    let mut document: Value = serde_json::from_str(&text)
        .map_err(|err| ExportError::Message(err.to_string()))?;
    let mut cards = Vec::new();
    let mut tokens = Vec::new();
    for face in root.list_faces()? {
        let exported = export_face(&face.value)?;
        if face.folder == "tokens" {
            tokens.push(exported);
        } else {
            cards.push(exported);
        }
    }
    let object = document
        .as_object_mut()
        .ok_or_else(|| ExportError::Message("set.json is not an object".into()))?;
    object.insert("cards".into(), Value::Array(cards));
    object.insert("tokens".into(), Value::Array(tokens));
    let mut bytes = serde_json::to_vec_pretty(&document)
        .map_err(|err| ExportError::Message(err.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn export_face(value: &Value) -> Result<Value, ExportError> {
    let mut copy = value.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove("boggled");
    }
    let text = canonical_face(&copy);
    serde_json::from_str(&text).map_err(|err| ExportError::Message(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{new_creature, NewCreature};
    use serde_json::json;

    #[test]
    fn mtgjson_inlines_faces_and_drops_studio_fields() {
        let dir = std::env::temp_dir().join(format!(
            "boggled-export-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let root = SetRoot::init(&dir, "Steam", "STM").unwrap();
        let mut fields = NewCreature::default();
        fields.name = "Brass Owl".into();
        fields.mana_cost = "2U".into();
        fields.type_line = "Artifact Creature - Bird".into();
        let mut card = new_creature("STM", fields);
        card.as_object_mut()
            .unwrap()
            .insert("futureField".into(), json!({"keep": true}));
        let uuid = card["uuid"].as_str().unwrap().to_string();
        root.write_face("cards", &uuid, &card, None).unwrap();

        let mut token = new_creature("STM", NewCreature {
            name: "Owl".into(),
            type_line: "Token Creature - Bird".into(),
            ..NewCreature::default()
        });
        token["types"] = json!(["Token", "Creature"]);
        let token_id = token["uuid"].as_str().unwrap().to_string();
        root.write_face("tokens", &token_id, &token, None).unwrap();

        let dest = dir.join("export").join("STM.json");
        let bytes = export_set(&root, "mtgjson", &dest).unwrap();
        let document: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(document.get("boggled").is_none());
        assert_eq!(document["code"], json!("STM"));
        assert_eq!(document["cards"][0]["name"], json!("Brass Owl"));
        assert_eq!(document["cards"][0]["futureField"]["keep"], json!(true));
        assert!(document["cards"][0].get("boggled").is_none());
        assert_eq!(document["tokens"][0]["name"], json!("Owl"));
        let unknown = export_set(&root, "pdf", &dest.join("nope")).unwrap_err();
        assert_eq!(unknown.to_string(), "unknown export format: pdf");
        assert_eq!(formats(), ["mtgjson"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
