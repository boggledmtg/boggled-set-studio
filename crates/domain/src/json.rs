//! Canonical JSON: two-space indent, trailing newline, schema key order, `boggled` last.

use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};

const FACE_KEYS: &[&str] = &[
    "uuid",
    "name",
    "layout",
    "setCode",
    "number",
    "manaCost",
    "manaValue",
    "types",
    "subtypes",
    "supertypes",
    "type",
    "text",
    "power",
    "toughness",
    "loyalty",
    "rarity",
    "artist",
    "flavorText",
    "colorIdentity",
    "colors",
    "side",
    "keywords",
    "identifiers",
    "otherFaceIds",
];

const SET_KEYS: &[&str] = &[
    "name",
    "code",
    "releaseDate",
    "type",
    "baseSetSize",
    "totalSetSize",
    "keyruneCode",
    "languages",
    "translations",
    "block",
];

const BOGGLED_KEYS: &[&str] = &[
    "schemaVersion",
    "designBrief",
    "lore",
    "artDirection",
    "artPrompt",
    "designNotes",
    "themes",
    "mechanicalThemes",
    "notes",
    "artFile",
    "renderFile",
    "stylesheet",
    "styling",
    "mseFields",
    "import",
];

const KEYWORD_KEYS: &[&str] = &["id", "name", "reminder", "rules"];

pub fn canonical_face(value: &Value) -> String {
    canonical_object(value, FACE_KEYS)
}

pub fn canonical_set(value: &Value) -> String {
    canonical_object(value, SET_KEYS)
}

pub fn canonical_keyword(value: &Value) -> String {
    canonical_object(value, KEYWORD_KEYS)
}

pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn canonical_object(value: &Value, order: &[&str]) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0, Some(order));
    out.push('\n');
    out
}

fn write_value(out: &mut String, value: &Value, indent: usize, order: Option<&[&str]>) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => out.push_str(&number_text(n)),
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(out, indent + 1);
                write_value(out, item, indent + 1, None);
                if i + 1 != items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(out, indent);
            out.push(']');
        }
        Value::Object(map) => write_object(out, map, indent, order),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>, indent: usize, order: Option<&[&str]>) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    let keys = ordered_keys(map, order);
    out.push_str("{\n");
    for (i, key) in keys.iter().enumerate() {
        push_indent(out, indent + 1);
        out.push_str(&serde_json::to_string(key).unwrap_or_else(|_| "\"\"".into()));
        out.push_str(": ");
        let child_order = if key == "boggled" { Some(BOGGLED_KEYS) } else { None };
        write_value(out, &map[key], indent + 1, child_order);
        if i + 1 != keys.len() {
            out.push(',');
        }
        out.push('\n');
    }
    push_indent(out, indent);
    out.push('}');
}

fn ordered_keys(map: &Map<String, Value>, order: Option<&[&str]>) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(order) = order {
        for key in order {
            if map.contains_key(*key) {
                keys.push((*key).to_string());
            }
        }
    }
    let mut rest: Vec<String> = map
        .keys()
        .filter(|k| *k != "boggled" && !keys.iter().any(|e| e == *k))
        .cloned()
        .collect();
    rest.sort();
    keys.extend(rest);
    if map.contains_key("boggled") {
        keys.push("boggled".to_string());
    }
    keys
}

fn number_text(n: &Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    if let Some(f) = n.as_f64() {
        if f.fract() == 0.0 && f.abs() < (i64::MAX as f64) {
            return format!("{}", f as i64);
        }
        let text = format!("{f}");
        return text;
    }
    n.to_string()
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_puts_unknown_keys_before_boggled_and_ends_with_a_newline() {
        let value = serde_json::json!({
            "boggled": { "notes": "x", "schemaVersion": 1, "extraStudio": true },
            "name": "Owl",
            "customFutureField": 1,
            "uuid": "abc"
        });
        let text = canonical_face(&value);
        assert!(text.ends_with('\n'));
        let uuid = text.find("\"uuid\"").unwrap();
        let name = text.find("\"name\"").unwrap();
        let custom = text.find("\"customFutureField\"").unwrap();
        let boggled = text.find("\"boggled\"").unwrap();
        assert!(uuid < name && name < custom && custom < boggled);
        let schema = text.find("\"schemaVersion\"").unwrap();
        let extra = text.find("\"extraStudio\"").unwrap();
        let notes = text.find("\"notes\"").unwrap();
        assert!(schema < notes && notes < extra);
    }

    #[test]
    fn hash_changes_when_bytes_change() {
        assert_ne!(content_hash(b"a"), content_hash(b"b"));
        assert_eq!(content_hash(b"a"), content_hash(b"a"));
    }
}
