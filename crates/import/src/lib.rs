//! Read one Magic Set Editor `.mse-set` package and write a normal set folder.

use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

use domain::{
    derive_face, new_keyword, next_collector_number, string_field, M15_STYLESHEET,
    SCHEMA_VERSION,
};
use mse::{Document, Node};
use serde_json::{json, Map, Value};
use store::SetRoot;
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Store(#[from] store::StoreError),
    #[error(transparent)]
    Document(#[from] domain::DocumentError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub cards: usize,
    pub tokens: usize,
    pub keywords: usize,
    pub warnings: Vec<String>,
}

pub fn import_mse_set(source: &Path, dest: &Path) -> Result<ImportReport, ImportError> {
    if !source.is_file() {
        return Err(ImportError::Message(format!(
            "mse-set {} was not found",
            source.display()
        )));
    }
    let bytes = fs::read(source).map_err(|err| ImportError::Message(err.to_string()))?;
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|_| {
        ImportError::Message(format!("{} is not an mse-set package", source.display()))
    })?;
    let mut set_text = None;
    let mut files = HashMap::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| ImportError::Message(err.to_string()))?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        let mut body = Vec::new();
        file.read_to_end(&mut body)
            .map_err(|err| ImportError::Message(err.to_string()))?;
        if name == "set" || name.ends_with("/set") {
            set_text = Some(String::from_utf8_lossy(&body).into_owned());
        }
        if let Some(base) = name.rsplit('/').next() {
            files.entry(base.to_string()).or_insert(body);
        }
    }
    let Some(set_text) = set_text else {
        return Err(ImportError::Message(format!(
            "{} is not an mse-set package",
            source.display()
        )));
    };
    let mut doc = Document::parse(set_text.trim_start_matches('\u{feff}')).map_err(ImportError::Message)?;
    normalize_keys(&mut doc.fields);
    let info = doc.first("set info");
    let name = info
        .and_then(|node| first_scalar(node, &["title", "set name", "name"]))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "Imported Set".into());
    let code = info
        .and_then(|node| first_scalar(node, &["set code", "code"]))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "SET".into());
    let brief = info
        .and_then(|node| first_scalar(node, &["description", "notes"]))
        .unwrap_or_default();
    let default_style = doc
        .scalar("stylesheet")
        .unwrap_or(M15_STYLESHEET)
        .to_string();

    let root = SetRoot::init(dest, &name, &code)?;
    {
        let (mut set, _) = root.read_set()?;
        if let Some(studio) = set
            .get_mut("boggled")
            .and_then(|value| value.as_object_mut())
        {
            studio.insert("designBrief".into(), json!(brief));
            if let Some(info) = info {
                let mut fields = mse_map(info);
                for key in ["title", "set name", "name", "code", "set code", "description", "notes"] {
                    fields.remove(key);
                }
                if !fields.is_empty() { studio.insert("mseFields".into(), fields.into()); }
            }
            if let Some(styling) = doc.first("styling") {
                studio.insert("styling".into(), mse_map(styling).into());
            }
        }
        root.write_set(&set, None)?;
    }

    let mut warnings = Vec::new();
    let mut keywords = 0;
    for keyword in doc.all("keyword") {
        let keyword_name = first_scalar(keyword, &["keyword", "name"]).unwrap_or_default();
        if keyword_name.is_empty() {
            warnings.push("a keyword is missing a name".into());
            continue;
        }
        let id = slug(&keyword_name);
        let reminder = first_scalar(keyword, &["reminder", "reminder text"]).unwrap_or_default();
        let rules = first_scalar(keyword, &["rules", "rule text"]).unwrap_or_default();
        let value = new_keyword(&id, &keyword_name, &reminder, &rules);
        root.write_keyword(&id, &value, None)?;
        keywords += 1;
    }

    let mut card_numbers = Vec::new();
    let mut token_numbers = Vec::new();
    let mut cards = 0;
    let mut tokens = 0;
    for (index, card) in doc.all("card").into_iter().enumerate() {
        let mut face = Map::new();
        let card_name = first_scalar(card, &["name"]).unwrap_or_default();
        let label = if card_name.is_empty() {
            format!("card {}", index + 1)
        } else {
            card_name.clone()
        };
        if card_name.is_empty() {
            warnings.push(format!("{label} is missing a name"));
        }
        let type_line = type_line(card);
        if type_line.is_empty() {
            warnings.push(format!("{label} is missing a type line"));
        }
        let stylesheet = first_scalar(card, &["stylesheet"]).unwrap_or_else(|| default_style.clone());
        let stylesheet = if stylesheet == "m15-altered" {
            M15_STYLESHEET.to_owned()
        } else {
            stylesheet
        };
        let folder = if is_token(card, &stylesheet) {
            "tokens"
        } else {
            "cards"
        };
        let number = first_scalar(card, &["custom card number", "collector number", "card number", "number"]).unwrap_or_default();
        let automatic_number = number.is_empty();
        let number = if number.is_empty() {
            let existing = if folder == "tokens" {
                &token_numbers
            } else {
                &card_numbers
            };
            next_collector_number(existing)
        } else {
            number
        };
        if folder == "tokens" {
            token_numbers.push(number.clone());
        } else {
            card_numbers.push(number.clone());
        }

        face.insert("uuid".into(), json!(Uuid::new_v4().to_string()));
        face.insert("name".into(), json!(card_name));
        face.insert("layout".into(), json!("normal"));
        face.insert("setCode".into(), json!(code.to_ascii_uppercase()));
        face.insert("number".into(), json!(number));
        face.insert(
            "manaCost".into(),
            json!(first_scalar(card, &["casting cost", "mana cost"]).unwrap_or_default()),
        );
        face.insert("type".into(), json!(type_line));
        face.insert(
            "text".into(),
            json!(editable_text(first_scalar(card, &["rule text", "rules text", "text"]).unwrap_or_default())),
        );
        face.insert(
            "flavorText".into(),
            json!(editable_text(first_scalar(card, &["flavor text", "flavor"]).unwrap_or_default())),
        );
        face.insert(
            "power".into(),
            json!(first_scalar(card, &["power"]).unwrap_or_default()),
        );
        face.insert(
            "toughness".into(),
            json!(first_scalar(card, &["toughness"]).unwrap_or_default()),
        );
        face.insert(
            "rarity".into(),
            json!(first_scalar(card, &["rarity"]).unwrap_or_default()),
        );
        face.insert(
            "artist".into(),
            json!(first_scalar(card, &["illustrator", "artist", "illustrator name"]).unwrap_or_default()),
        );
        face.insert("side".into(), json!("a"));
        face.insert("keywords".into(), json!([]));
        face.insert("identifiers".into(), json!({}));

        let notes = first_scalar(card, &["notes"]).unwrap_or_default();
        let mut mse_fields = Map::new();
        if let Node::Map(fields) = card {
            for (key, node) in fields {
                if known_card_field(key) {
                    continue;
                }
                if key == "styling" || key == "styling data" {
                    continue;
                }
                if let Node::Scalar(value) = node {
                    mse_fields.insert(key.clone(), json!(value));
                }
            }
        }
        let mut styling = Map::new();
        if let Some(Node::Map(fields)) = card.map_first("styling data").or_else(|| card.map_first("styling")) {
            for (key, node) in fields {
                if let Node::Scalar(value) = node {
                    styling.insert(key.clone(), json!(value));
                }
            }
        }
        let mut studio = Map::new();
        studio.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
        studio.insert("lore".into(), json!(""));
        studio.insert("artDirection".into(), json!(""));
        studio.insert("artPrompt".into(), json!(""));
        studio.insert("designNotes".into(), json!(notes));
        studio.insert(
            "stylesheet".into(),
            json!(if stylesheet.is_empty() {
                default_style.clone()
            } else {
                stylesheet.clone()
            }),
        );
        studio.insert("styling".into(), Value::Object(styling));
        studio.insert("mseFields".into(), Value::Object(mse_fields));
        studio.insert(
            "import".into(),
            json!({
                "format": "mse-set",
                "sourceName": label,
                "automaticNumber": automatic_number.then_some(&number),
                "warnings": [],
            }),
        );

        let uuid = string_field(&face, "uuid");
        if let Some(image_name) = first_scalar(card, &["image", "card image", "illustration"]) {
            if !image_name.is_empty() {
                if let Some(bytes) = files.get(image_name.rsplit(['/', '\\']).next().unwrap_or(&image_name))
                {
                    let ext = extension_of(&image_name);
                    let rel = format!("art/{uuid}.{ext}");
                    root.write_bytes(&rel, bytes)?;
                    studio.insert("artFile".into(), json!(rel));
                } else {
                    warnings.push(format!("{label} illustration {image_name} was not in the package"));
                }
            }
        }
        face.insert("boggled".into(), Value::Object(studio));
        derive_face(&mut face)?;
        root.write_face(folder, &uuid, &Value::Object(face), None)?;
        if folder == "tokens" {
            tokens += 1;
        } else {
            cards += 1;
        }
    }

    Ok(ImportReport {
        cards,
        tokens,
        keywords,
        warnings,
    })
}

fn first_scalar(node: &Node, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = node.scalar(key) {
            return Some(value.to_string());
        }
        if let Some(Node::Script(value)) = node.map_first(key) {
            return Some(value.clone());
        }
    }
    None
}

fn normalize_keys(fields: &mut [(String, Node)]) {
    for (key, value) in fields {
        *key = key.replace('_', " ");
        if let Node::Map(children) = value {
            normalize_keys(children);
        }
    }
}

fn mse_map(node: &Node) -> Map<String, Value> {
    let Node::Map(children) = node else { return Map::new() };
    children.iter().map(|(key, value)| {
        let json = match value {
            Node::Map(_) => Value::Object(mse_map(value)),
            Node::Script(text) | Node::Scalar(text) => Value::String(text.clone()),
        };
        (key.clone(), json)
    }).collect()
}

fn editable_text(value: String) -> String {
    let value = value.strip_prefix("<i-flavor>").and_then(|s| s.strip_suffix("</i-flavor>")).unwrap_or(&value);
    value.replace("<sym>", "{").replace("</sym>", "}")
}

fn type_line(card: &Node) -> String {
    if let Some(line) = card.scalar("type") {
        if !line.trim().is_empty() {
            return line.to_string();
        }
    }
    let super_type = card.scalar("super type").unwrap_or("").trim().to_string();
    let card_type = card.scalar("card type").unwrap_or("").trim().to_string();
    let sub_type = card.scalar("sub type").unwrap_or("").trim().to_string();
    let mut front = super_type;
    if !card_type.is_empty() {
        if !front.is_empty() {
            front.push(' ');
        }
        front.push_str(&card_type);
    }
    if sub_type.is_empty() {
        front
    } else if front.is_empty() {
        sub_type
    } else {
        format!("{front} - {sub_type}")
    }
}

fn is_token(card: &Node, stylesheet: &str) -> bool {
    if stylesheet.to_ascii_lowercase().contains("token") {
        return true;
    }
    if card
        .scalar("super type")
        .unwrap_or("")
        .to_ascii_lowercase()
        .split_whitespace()
        .any(|word| word == "token")
    {
        return true;
    }
    matches!(
        card.scalar("token").unwrap_or("").to_ascii_lowercase().as_str(),
        "yes" | "true"
    )
}

fn known_card_field(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "casting cost"
            | "mana cost"
            | "type"
            | "super type"
            | "sub type"
            | "card type"
            | "rule text"
            | "rules text"
            | "text"
            | "flavor text"
            | "flavor"
            | "power"
            | "toughness"
            | "rarity"
            | "illustrator"
            | "artist"
            | "illustrator name"
            | "card number"
            | "custom card number"
            | "collector number"
            | "has styling"
            | "number"
            | "notes"
            | "stylesheet"
            | "image"
            | "card image"
            | "illustration"
            | "styling"
            | "token"
    )
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

fn extension_of(name: &str) -> String {
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
        ext
    } else {
        "png".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn package() -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "boggled-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mse = dir.join("sample.mse-set");
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
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
\tdescription: Gears and birds
keyword:
\tkeyword: Flying
\treminder: Can't be blocked except by fliers.
card:
\tname: Brass Owl
\tcasting cost: 2U
\ttype: Artifact Creature - Bird
\trule text: Flying
\tflavor text: It hoots.
\tpower: 1
\ttoughness: 3
\trarity: uncommon
\tillustrator: Ada
\timage: owl.png
\tnotes: watch the gears
\tcustom flag: kept
\tstyling:
\t\tpromo: no
card:
\tname: Helper
\tsuper type: Token
\tsub type: Bird
\tpower: 1
\ttoughness: 1
\tstylesheet: magic-token.mse-style
card:
\tname: Zap
\tcasting cost: R
\trule text: Zap something.
\tstylesheet: magic-m15-altered.mse-style
",
        )
        .unwrap();
        zip.start_file("owl.png", opts).unwrap();
        zip.write_all(&[137, 80, 78, 71, 1, 2, 3]).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        fs::write(&mse, bytes).unwrap();
        (mse, dir.join("Steam"))
    }

    #[test]
    fn an_mse_set_becomes_a_set_folder() {
        let (source, dest) = package();
        let report = import_mse_set(&source, &dest).unwrap();
        assert_eq!(report.cards, 2);
        assert_eq!(report.tokens, 1);
        assert_eq!(report.keywords, 1);
        assert!(report.warnings.iter().any(|warning| warning.contains("Zap") && warning.contains("type")));
        let root = SetRoot::open(&dest).unwrap();
        let (set, _) = root.read_set().unwrap();
        assert_eq!(set["code"], json!("STM"));
        assert_eq!(set["boggled"]["designBrief"], json!("Gears and birds"));
        let faces = root.list_faces().unwrap();
        let owl = faces
            .iter()
            .find(|face| face.value["name"] == json!("Brass Owl"))
            .unwrap();
        assert_eq!(owl.folder, "cards");
        assert_eq!(owl.value["manaCost"], json!("{2}{U}"));
        assert_eq!(owl.value["types"], json!(["Artifact", "Creature"]));
        assert_eq!(owl.value["boggled"]["designNotes"], json!("watch the gears"));
        assert_eq!(owl.value["boggled"]["mseFields"]["custom flag"], json!("kept"));
        assert_eq!(owl.value["boggled"]["styling"]["promo"], json!("no"));
        assert_eq!(owl.value["boggled"]["import"]["format"], json!("mse-set"));
        let art = owl.value["boggled"]["artFile"].as_str().unwrap();
        assert_eq!(root.read_bytes(art).unwrap(), vec![137, 80, 78, 71, 1, 2, 3]);
        assert!(faces.iter().any(|face| face.folder == "tokens" && face.value["name"] == json!("Helper")));
        let (keyword, _) = root.read_keyword("flying").unwrap();
        assert_eq!(keyword["name"], json!("Flying"));
        let again = import_mse_set(&source, &dest).unwrap_err();
        assert!(again.to_string().contains("already exists"));
        let _ = fs::remove_dir_all(dest.parent().unwrap());
    }
}
