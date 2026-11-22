//! Windows bridge to the pinned engine's offscreen renderer and evaluated style.
use std::fs::{self, File};
use std::io::{Read, Seek, Write};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use zip::{write::SimpleFileOptions, ZipWriter};

use crate::MseError;
use domain::string_field;

const ENGINE_SHA256: &str = "091310f9c20881d00ac941041912028470a508327bb3248cb2b64f5cc015b995";
const FIELDS: [&str; 5] = ["name", "casting_cost", "type", "text", "pt"];

pub struct Face {
    pub card: Map<String, Value>,
    pub art: Option<Vec<u8>>,
}

pub struct Preview {
    pub png: Vec<u8>,
    /// [left, top, width, height] in the 375 x 523 stylesheet coordinate space.
    pub layout: Value,
}

fn error(e: impl std::fmt::Display) -> MseError {
    MseError::Message(format!("Windows MSE: {e}"))
}

fn field(out: &mut String, level: usize, key: &str, value: &str) -> Result<(), MseError> {
    if key.is_empty()
        || key.replace('_', " ").eq_ignore_ascii_case("include file")
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b" _-.".contains(&c))
    {
        return Err(error("invalid MSE field name"));
    }
    let indent = "\t".repeat(level);
    let value = value.replace("\r\n", "\n").replace('\r', "\n");
    if value.contains('\0') {
        return Err(error("NUL in MSE field"));
    }
    out.push_str(&format!("{indent}{key}:"));
    if value.contains('\n') {
        out.push('\n');
        for line in value.split('\n') {
            out.push_str(&format!("{indent}\t{line}\n"));
        }
    } else {
        out.push_str(&format!(" {value}\n"));
    }
    Ok(())
}

fn rules_text(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('{') {
        let Some((symbol, tail)) = after.split_once('}') else {
            break;
        };
        out.push_str(before);
        if !symbol.is_empty()
            && symbol
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'/')
        {
            out.push_str(&format!("<sym>{symbol}</sym>"));
        } else {
            out.push_str(&format!("{{{symbol}}}"));
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn add_fields(out: &mut String, level: usize, values: &Value) -> Result<(), MseError> {
    if let Some(values) = values.as_object() {
        for (key, value) in values {
            if let Some(value) = value.as_str() {
                field(out, level, key, value)?;
            } else if value.is_object() {
                field(out, level, key, "")?;
                add_fields(out, level + 1, value)?;
            }
        }
    }
    Ok(())
}

fn write_set(path: &Path, set: &Value, faces: &[Face]) -> Result<(), MseError> {
    let mut zip = ZipWriter::new(File::create(path).map_err(error)?);
    let mut out =
        String::from("mse version: 2.0.2\ngame: magic\nstylesheet: m15-altered\nset info:\n");
    add_fields(&mut out, 1, &set["boggled"]["mseFields"])?;
    field(&mut out, 1, "title", set["name"].as_str().unwrap_or(""))?;
    field(&mut out, 1, "set code", set["code"].as_str().unwrap_or(""))?;
    if let Some(description) = set["boggled"]["designBrief"]
        .as_str()
        .filter(|s| !s.is_empty())
    {
        field(&mut out, 1, "description", description)?;
    }
    if set["boggled"]["styling"].is_object() {
        out.push_str("styling:\n");
        add_fields(&mut out, 1, &set["boggled"]["styling"])?;
    }
    for (index, face) in faces.iter().enumerate() {
        let card = &face.card;
        let studio = card.get("boggled").unwrap_or(&Value::Null);
        let get = |key| string_field(card, key);
        out.push_str("card:\n");
        let line = get("type");
        let (front, sub) = line
            .split_once('—')
            .or_else(|| line.split_once(" - "))
            .unwrap_or((&line, ""));
        let cost = get("manaCost").replace(['{', '}'], "");
        for (key, value) in [
            ("name", get("name")),
            ("super type", front.trim().into()),
            ("sub type", sub.trim().into()),
            ("casting cost", cost),
            ("rule text", rules_text(&get("text"))),
            (
                "flavor text",
                format!("<i-flavor>{}</i-flavor>", get("flavorText")),
            ),
            ("power", get("power")),
            ("toughness", get("toughness")),
            ("rarity", get("rarity")),
            ("illustrator", get("artist")),
        ] {
            field(&mut out, 1, key, &value)?;
        }
        let number = get("number");
        if studio["import"]["automaticNumber"].as_str() != Some(&number) {
            field(&mut out, 1, "custom card number", &number)?;
        }
        add_fields(&mut out, 1, &studio["mseFields"])?;
        if studio["styling"].as_object().is_some_and(|m| !m.is_empty()) {
            out.push_str("\thas styling: true\n\tstyling data:\n");
            add_fields(&mut out, 2, &studio["styling"])?;
        }
        if let Some(art) = &face.art {
            let name = format!("image{index}");
            field(&mut out, 1, "image", &name)?;
            zip.start_file(name, SimpleFileOptions::default())
                .map_err(error)?;
            zip.write_all(art).map_err(error)?;
        }
    }
    zip.start_file("set", SimpleFileOptions::default())
        .map_err(error)?;
    zip.write_all(out.as_bytes()).map_err(error)?;
    zip.finish().map_err(error)?;
    Ok(())
}

pub fn render(pack: &Path, set: &Value, faces: &[Face], index: usize) -> Result<Preview, MseError> {
    let face = faces
        .get(index)
        .ok_or_else(|| error("card missing from set"))?;
    if !domain::is_supported_frame(&face.card) {
        return Err(MseError::UnsupportedFrame);
    }
    crate::require_pack(pack)?;
    let exe = pack
        .join("magicseteditor.exe")
        .canonicalize()
        .map_err(error)?;
    let hash = Sha256::digest(fs::read(&exe).map_err(error)?);
    if format!("{hash:x}") != ENGINE_SHA256 {
        return Err(error(
            "unexpected engine version; run boggled pack download",
        ));
    }
    let dir = tempfile::tempdir().map_err(error)?;
    write_set(&dir.path().join("snapshot.mse-set"), set, faces)?;
    let mut commands = format!("write_image_file(set.cards[{index}],file:\"card.png\",zoom:200)\n");
    for name in FIELDS {
        for property in ["left", "top", "width", "height"] {
            commands.push_str(&format!("card_style.{name}.{property}\n"));
        }
    }
    let paragraphs = string_field(&face.card, "text").lines().count().max(1);
    commands.push_str(&format!(
        "card_style.text.layout.paragraphs[{paragraphs}].top or else -1\n:quit\n"
    ));
    let mut log = tempfile::tempfile().map_err(error)?;
    let mut child = Command::new(exe)
        .current_dir(dir.path())
        .args(["--cli", "snapshot.mse-set", "--raw"])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(log.try_clone().map_err(error)?)
        .stderr(Stdio::null())
        .spawn()
        .map_err(error)?;
    let sent = child.stdin.take().unwrap().write_all(commands.as_bytes());
    let start = Instant::now();
    loop {
        if sent.is_err() || start.elapsed() > Duration::from_secs(45) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error("renderer stopped responding"));
        }
        if let Some(status) = child.try_wait().map_err(error)? {
            if !status.success() {
                return Err(error(format!("engine exited with {status}")));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    log.rewind().map_err(error)?;
    let mut text = String::new();
    log.read_to_string(&mut text).map_err(error)?;
    let mut records = text.lines();
    let mut record = || -> Result<&str, MseError> {
        let status = records.next();
        let count = records.next();
        let value = records.next().unwrap_or("");
        if status != Some("0") || count != Some("1") {
            return Err(error(format!("invalid worker response: {value}")));
        }
        Ok(value)
    };
    record()?;
    let mut layout = Map::new();
    for name in FIELDS {
        let mut rect = [0.0; 4];
        for value in &mut rect {
            *value = record()?.parse::<f64>().map_err(error)?;
        }
        layout.insert(name.into(), json!(rect));
    }
    let flavor_top = record()?.parse::<f64>().map_err(error)?;
    layout.insert("flavorTop".into(), json!(flavor_top));
    Ok(Preview {
        png: fs::read(dir.path().join("card.png")).map_err(error)?,
        layout: Value::Object(layout),
    })
}
