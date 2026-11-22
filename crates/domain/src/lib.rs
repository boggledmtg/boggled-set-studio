//! Card and set documents. The set folder stores these objects; this crate
//! derives the fields the editor computes and checks the ones it knows.

mod json;
mod mana;
mod typeline;

use serde_json::{json, Map, Value};
use thiserror::Error;
use uuid::Uuid;

pub use json::{canonical_face, canonical_keyword, canonical_set, content_hash};
pub use mana::{color_identity, parse_mana, ManaInfo};
pub use typeline::{is_creature, parse_type_line, TypeParts};

pub const SCHEMA_VERSION: i64 = 1;
pub const M15_STYLESHEET: &str = "magic-m15-altered.mse-style";
pub const FMP_COMMIT: &str = "cec045982f40870712c0d8f3f450d30a036d87a8";
pub const FMP_REPO: &str = "MagicSetEditorPacks/Full-Magic-Pack";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DocumentError {
    #[error("the document is not a JSON object")]
    NotAnObject,
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct NewCreature {
    pub name: String,
    pub mana_cost: String,
    pub type_line: String,
    pub text: String,
    pub flavor_text: String,
    pub power: String,
    pub toughness: String,
    pub rarity: String,
    pub artist: String,
    pub number: String,
    pub design_notes: String,
}

impl Default for NewCreature {
    fn default() -> Self {
        Self {
            name: "New Creature".into(),
            mana_cost: String::new(),
            type_line: "Creature".into(),
            text: String::new(),
            flavor_text: String::new(),
            power: "1".into(),
            toughness: "1".into(),
            rarity: "common".into(),
            artist: String::new(),
            number: "1".into(),
            design_notes: String::new(),
        }
    }
}

pub fn new_set(name: &str, code: &str) -> Value {
    let code = code.trim().to_ascii_uppercase();
    json!({
        "name": name,
        "code": code,
        "releaseDate": "",
        "type": "custom",
        "baseSetSize": 0,
        "totalSetSize": 0,
        "keyruneCode": code,
        "languages": ["English"],
        "boggled": studio_object(&[
            ("designBrief", json!("")),
            ("lore", json!("")),
            ("artDirection", json!("")),
            ("themes", json!([])),
            ("mechanicalThemes", json!([])),
            ("notes", json!("")),
        ])
    })
}

pub fn new_creature(set_code: &str, fields: NewCreature) -> Value {
    let mut card = Map::new();
    card.insert("uuid".into(), json!(Uuid::new_v4().to_string()));
    card.insert("name".into(), json!(fields.name));
    card.insert("layout".into(), json!("normal"));
    card.insert("setCode".into(), json!(set_code.trim().to_ascii_uppercase()));
    card.insert("number".into(), json!(fields.number));
    card.insert("manaCost".into(), json!(fields.mana_cost));
    card.insert("type".into(), json!(fields.type_line));
    card.insert("text".into(), json!(fields.text));
    card.insert("power".into(), json!(fields.power));
    card.insert("toughness".into(), json!(fields.toughness));
    card.insert("rarity".into(), json!(normalize_rarity(&fields.rarity)));
    card.insert("artist".into(), json!(fields.artist));
    card.insert("flavorText".into(), json!(fields.flavor_text));
    card.insert("side".into(), json!("a"));
    card.insert("keywords".into(), json!([]));
    card.insert("identifiers".into(), json!({}));
    card.insert(
        "boggled".into(),
        studio_object(&[
            ("lore", json!("")),
            ("artDirection", json!("")),
            ("artPrompt", json!("")),
            ("designNotes", json!(fields.design_notes)),
            ("stylesheet", json!(M15_STYLESHEET)),
            ("styling", json!({})),
            ("mseFields", json!({})),
        ]),
    );
    let _ = derive_face(&mut card);
    Value::Object(card)
}

pub fn new_keyword(id: &str, name: &str, reminder: &str, rules: &str) -> Value {
    json!({
        "id": id,
        "name": name,
        "reminder": reminder,
        "rules": rules,
        "boggled": studio_object(&[
            ("lore", json!("")),
            ("designNotes", json!("")),
        ])
    })
}

fn studio_object(pairs: &[(&str, Value)]) -> Value {
    let mut map = Map::new();
    map.insert("schemaVersion".into(), json!(SCHEMA_VERSION));
    for (key, value) in pairs {
        map.insert((*key).to_string(), value.clone());
    }
    Value::Object(map)
}

/// A normal-layout card whose types include Creature.
pub fn is_supported_creature(card: &Map<String, Value>) -> bool {
    let layout = card.get("layout").and_then(|v| v.as_str()).unwrap_or("normal");
    if layout != "normal" {
        return false;
    }
    card.get("types")
        .and_then(|v| v.as_array())
        .map(|types| types.iter().any(|t| t.as_str() == Some("Creature")))
        .unwrap_or(false)
}

pub fn is_supported_frame(card: &Map<String, Value>) -> bool {
    let stylesheet = card.get("boggled").and_then(|s| s["stylesheet"].as_str());
    is_supported_creature(card) && matches!(stylesheet, None | Some(M15_STYLESHEET | "m15-altered"))
}

pub fn derive_face(card: &mut Map<String, Value>) -> Result<(), DocumentError> {
    let cost_text = string_field(card, "manaCost");
    let rules = string_field(card, "text");
    let info = parse_mana(&cost_text);
    card.insert("manaCost".into(), json!(info.canonical));
    card.insert("manaValue".into(), number_value(info.mana_value));
    card.insert("colors".into(), json!(info.colors.clone()));
    card.insert("colorIdentity".into(), json!(color_identity(&info, &rules)));

    let parts = parse_type_line(&string_field(card, "type"));
    card.insert("type".into(), json!(parts.line));
    card.insert("supertypes".into(), json!(parts.supertypes));
    card.insert("types".into(), json!(parts.types));
    card.insert("subtypes".into(), json!(parts.subtypes));

    if let Some(rarity) = card.get("rarity").and_then(|v| v.as_str()) {
        card.insert("rarity".into(), json!(normalize_rarity(rarity)));
    }
    if !card.contains_key("keywords") {
        card.insert("keywords".into(), json!([]));
    }
    if !card.contains_key("side") {
        card.insert("side".into(), json!("a"));
    }
    if !card.contains_key("layout") {
        card.insert("layout".into(), json!("normal"));
    }
    ensure_boggled(card);
    Ok(())
}

pub fn validate_face(card: &Map<String, Value>, set_code: &str) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for key in ["uuid", "name", "layout", "setCode", "number", "type"] {
        if string_field(card, key).is_empty() {
            warnings.push(Warning {
                path: key.into(),
                message: format!("{key} is missing"),
            });
        }
    }
    let card_code = string_field(card, "setCode");
    if !card_code.is_empty() && !set_code.is_empty() && !card_code.eq_ignore_ascii_case(set_code) {
        warnings.push(Warning {
            path: "setCode".into(),
            message: format!("{card_code} does not match the set code {set_code}"),
        });
    }
    warnings
}

pub fn validate_set(set: &Map<String, Value>) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if string_field(set, "name").is_empty() {
        warnings.push(Warning {
            path: "name".into(),
            message: "name is missing".into(),
        });
    }
    if string_field(set, "code").is_empty() {
        warnings.push(Warning {
            path: "code".into(),
            message: "code is missing".into(),
        });
    }
    warnings
}

pub fn object_map(value: Value) -> Result<Map<String, Value>, DocumentError> {
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(DocumentError::NotAnObject),
    }
}

pub fn string_field(map: &Map<String, Value>, key: &str) -> String {
    map.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn normalize_rarity(rarity: &str) -> String {
    let lower = rarity.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" => "common".into(),
        "mythic rare" | "mythic" => "mythic".into(),
        "basic land" => "basic land".into(),
        other => other.to_string(),
    }
}

fn number_value(n: f64) -> Value {
    if n.fract() == 0.0 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn ensure_boggled(card: &mut Map<String, Value>) {
    let entry = card
        .entry("boggled".to_string())
        .or_insert_with(|| json!({}));
    if let Some(map) = entry.as_object_mut() {
        map.entry("schemaVersion".to_string())
            .or_insert(json!(SCHEMA_VERSION));
        if !map.contains_key("stylesheet") {
            map.insert("stylesheet".into(), json!(M15_STYLESHEET));
        }
    }
}

pub fn next_collector_number(numbers: &[String]) -> String {
    let max = numbers
        .iter()
        .filter_map(|n| n.trim().trim_start_matches('0').parse::<u32>().ok().or_else(|| {
            n.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok()
        }))
        .max()
        .unwrap_or(0);
    (max + 1).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deriving_a_creature_fills_computed_fields_and_keeps_extra_keys() {
        let mut card = object_map(json!({
            "uuid": "id-1",
            "name": "Brass Owl",
            "layout": "normal",
            "setCode": "stm",
            "number": "12",
            "manaCost": "2U",
            "type": "Artifact Creature - Bird",
            "text": "Flying\n{T}: Add {G}.",
            "power": "1",
            "toughness": "3",
            "rarity": "Mythic Rare",
            "futureField": { "keep": true }
        }))
        .unwrap();
        derive_face(&mut card).unwrap();
        assert_eq!(card["manaCost"], json!("{2}{U}"));
        assert_eq!(card["manaValue"], json!(3));
        assert_eq!(card["colors"], json!(["U"]));
        assert_eq!(card["colorIdentity"], json!(["U", "G"]));
        assert_eq!(card["types"], json!(["Artifact", "Creature"]));
        assert_eq!(card["subtypes"], json!(["Bird"]));
        assert_eq!(card["type"], json!("Artifact Creature — Bird"));
        assert_eq!(card["rarity"], json!("mythic"));
        assert_eq!(card["futureField"]["keep"], json!(true));
        assert!(is_supported_creature(&card));
        let warnings = validate_face(&card, "STM");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn an_instant_is_stored_and_is_not_a_supported_creature() {
        let mut card = object_map(json!({
            "uuid": "id-2",
            "name": "Zap",
            "setCode": "STM",
            "number": "2",
            "manaCost": "{R}",
            "type": "Instant",
            "text": ""
        }))
        .unwrap();
        derive_face(&mut card).unwrap();
        assert!(!is_supported_creature(&card));
    }

    #[test]
    fn set_code_mismatch_warns_and_still_leaves_the_card_intact() {
        let card = object_map(json!({
            "uuid": "id",
            "name": "Owl",
            "layout": "normal",
            "setCode": "ZZZ",
            "number": "1",
            "type": "Creature"
        }))
        .unwrap();
        let warnings = validate_face(&card, "STM");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].path, "setCode");
    }

    #[test]
    fn collector_numbers_follow_the_highest_existing_number() {
        assert_eq!(next_collector_number(&[]), "1");
        assert_eq!(
            next_collector_number(&["4".into(), "12a".into(), "3".into()]),
            "13"
        );
    }

    #[test]
    fn a_new_creature_is_supported_and_canonical() {
        let card = new_creature(
            "stm",
            NewCreature {
                name: "Brass Owl".into(),
                mana_cost: "{1}{G}".into(),
                type_line: "Creature — Bird".into(),
                ..NewCreature::default()
            },
        );
        let map = card.as_object().unwrap();
        assert!(is_supported_creature(map));
        assert_eq!(map["setCode"], json!("STM"));
        assert_eq!(map["boggled"]["stylesheet"], json!(M15_STYLESHEET));
        let text = canonical_face(&card);
        assert!(text.ends_with('\n'));
        assert!(text.find("\"uuid\"").unwrap() < text.find("\"boggled\"").unwrap());
    }
}
