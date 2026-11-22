//! Lay out and paint one M15 creature.
//!
//! Positions come from the stylesheet's scripts. A default creature follows the
//! else-branches in M15 Main (art at 29,59 316×231, text box from
//! `top_of_textbox()`). With no pack on disk the same numbers are the fallback,
//! drawn with a flat frame and a bitmap font so the editor still has a card.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use domain::{is_supported_creature, string_field, M15_STYLESHEET};
use font8x8::UnicodeFonts;
use image::{imageops, DynamicImage, Rgb, RgbImage};
use imageproc::drawing::draw_text_mut;
use serde_json::{Map, Value as Json};

use crate::pack::{Document, Node};
use crate::script::{eval_script, Arg, Value};
use crate::MseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLayout {
    pub width: u32,
    pub height: u32,
    pub name: Rect,
    pub cost: Rect,
    pub image: Rect,
    pub type_line: Rect,
    pub rules: Rect,
    pub pt: Rect,
}

pub struct DrawRequest<'a> {
    pub card: &'a Map<String, Json>,
    pub art: Option<&'a [u8]>,
    pub style_text: Option<&'a str>,
    pub data_dir: Option<&'a Path>,
    pub fonts_dir: Option<&'a Path>,
    pub allow_unlisted: bool,
}

pub fn draw_card(request: DrawRequest<'_>) -> Result<Vec<u8>, MseError> {
    let style_id = stylesheet(request.card);
    if !request.allow_unlisted && style_id != M15_STYLESHEET {
        return Err(MseError::MissingPackage(style_id));
    }
    if !is_supported_creature(request.card) {
        return Err(MseError::UnsupportedFrame);
    }
    let loaded = match request.style_text {
        Some(text) => Some(text.to_string()),
        None => match request.data_dir {
            Some(dir) => match fs::read_to_string(dir.join(&style_id).join("style")) {
                Ok(text) => Some(text),
                Err(_) if style_id == M15_STYLESHEET => None,
                Err(_) => return Err(MseError::MissingPackage(style_id)),
            },
            None => None,
        },
    };
    let layout = match &loaded {
        Some(text) => card_layout(text, request.card)?,
        None => fallback_layout(),
    };
    let mut image = RgbImage::from_pixel(layout.width.max(1), layout.height.max(1), Rgb(frame_rgb(request.card)));
    let framed = paint_frame(&mut image, request.data_dir, request.card);
    if !framed {
        let plate_w = image.width() as i32 - 28;
        let plate_h = image.height() as i32 - 28;
        fill_rect(&mut image, 14, 14, plate_w, plate_h, [255, 249, 242]);
    }
    if let Some(bytes) = request.art {
        if let Ok(art) = image::load_from_memory(bytes) {
            blit_cover(&mut image, &art, layout.image);
        }
    } else if framed {
        paint_placeholder(&mut image, request.data_dir, request.card, layout.image);
    } else {
        fill_rect(
            &mut image,
            layout.image.left,
            layout.image.top,
            layout.image.width,
            layout.image.height,
            [232, 214, 186],
        );
    }
    let power = string_field(request.card, "power");
    let toughness = string_field(request.card, "toughness");
    let pt = if power.is_empty() && toughness.is_empty() {
        String::new()
    } else {
        format!("{power}/{toughness}")
    };
    if framed && !pt.is_empty() {
        paint_pt_badge(&mut image, request.data_dir, request.card);
    }
    let fonts = load_faces(request.fonts_dir);
    let name_font = fonts
        .name
        .as_deref()
        .and_then(|bytes| FontRef::try_from_slice(bytes).ok());
    let body_font = fonts
        .body
        .as_deref()
        .and_then(|bytes| FontRef::try_from_slice(bytes).ok());
    let italic_font = fonts
        .italic
        .as_deref()
        .and_then(|bytes| FontRef::try_from_slice(bytes).ok());
    let title_font = name_font.as_ref().or(body_font.as_ref());
    let ink = [20, 16, 12];
    let cost = string_field(request.card, "manaCost");
    let mut name_rect = layout.name;
    let cost_width = mana_row_width(&cost, request.data_dir);
    if cost_width > 0 {
        name_rect.width = (name_rect.width - cost_width - 6).max(40);
    }
    paint_title(
        &mut image,
        title_font,
        &string_field(request.card, "name"),
        name_rect,
        ink,
        16.0,
        "bottom",
    );
    paint_cost(&mut image, request.data_dir, body_font.as_ref(), &cost, layout.cost);
    paint_title(
        &mut image,
        title_font,
        &string_field(request.card, "type"),
        layout.type_line,
        ink,
        13.0,
        "top",
    );
    paint_textbox(
        &mut image,
        body_font.as_ref(),
        italic_font.as_ref().or(body_font.as_ref()),
        request.data_dir,
        &string_field(request.card, "text"),
        &string_field(request.card, "flavorText"),
        inset_rules(layout.rules),
        ink,
    );
    paint_pt_text(&mut image, title_font, &pt, layout.pt, ink);
    encode_png(&image)
}

pub fn card_layout(style_text: &str, card: &Map<String, Json>) -> Result<CardLayout, MseError> {
    let doc = Document::parse(style_text).map_err(MseError::Message)?;
    let mut layout = fallback_layout();
    if let Some(width) = document_number(&doc, "card width")? {
        layout.width = width.max(1) as u32;
    }
    if let Some(height) = document_number(&doc, "card height")? {
        layout.height = height.max(1) as u32;
    }
    let Some(style) = doc.first("card style") else {
        return Ok(layout);
    };
    let vars = script_env(card);
    if let Some(node) = style.map_first("name") {
        layout.name = read_box(node, layout.name, &vars)?;
    }
    if let Some(node) = style.map_first("casting cost") {
        layout.cost = read_box(node, layout.cost, &vars)?;
    }
    if let Some(node) = style.map_first("image") {
        layout.image = read_box(node, layout.image, &vars)?;
    }
    if let Some(node) = style.map_first("type") {
        layout.type_line = read_box(node, layout.type_line, &vars)?;
    }
    if let Some(node) = style.map_first("text") {
        layout.rules = read_box(node, layout.rules, &vars)?;
    }
    Ok(layout)
}

pub fn fallback_layout() -> CardLayout {
    CardLayout {
        width: 375,
        height: 523,
        name: Rect { left: 32, top: 30, width: 250, height: 23 },
        cost: Rect { left: 250, top: 29, width: 96, height: 23 },
        image: Rect { left: 29, top: 59, width: 316, height: 231 },
        type_line: Rect { left: 32, top: 298, width: 310, height: 20 },
        rules: Rect { left: 29, top: 327, width: 314, height: 154 },
        pt: Rect { left: 293, top: 467, width: 60, height: 28 },
    }
}

pub fn average_hash(png: &[u8]) -> Result<u64, MseError> {
    let image = image::load_from_memory(png).map_err(|err| MseError::Message(err.to_string()))?;
    let gray = imageops::resize(&image.to_luma8(), 8, 8, imageops::FilterType::Triangle);
    let average = gray.pixels().map(|pixel| pixel.0[0] as u32).sum::<u32>() / 64;
    let mut bits = 0u64;
    for (index, pixel) in gray.pixels().enumerate() {
        if pixel.0[0] as u32 >= average {
            bits |= 1 << index;
        }
    }
    Ok(bits)
}

pub fn hashes_close(left: &[u8], right: &[u8], max_distance: u32) -> Result<bool, MseError> {
    let left = average_hash(left)?;
    let right = average_hash(right)?;
    Ok((left ^ right).count_ones() <= max_distance)
}

fn read_box(node: &Node, fallback: Rect, vars: &BTreeMap<String, Value>) -> Result<Rect, MseError> {
    let mut rect = fallback;
    if let Some(value) = coord(node, "left", vars)? {
        rect.left = value;
    }
    if let Some(value) = coord(node, "top", vars)? {
        rect.top = value;
    }
    if let Some(value) = coord(node, "width", vars)? {
        rect.width = value.max(0);
    }
    if let Some(value) = coord(node, "height", vars)? {
        rect.height = value.max(0);
    }
    if let Some(right) = coord(node, "right", vars)? {
        if node.map_first("width").is_some() {
            rect.left = right - rect.width;
        } else {
            rect.width = (right - rect.left).max(0);
        }
    }
    if let Some(bottom) = coord(node, "bottom", vars)? {
        rect.height = (bottom - rect.top).max(0);
    }
    Ok(rect)
}

fn coord(node: &Node, key: &str, vars: &BTreeMap<String, Value>) -> Result<Option<i32>, MseError> {
    let Some(child) = node.map_first(key) else {
        return Ok(None);
    };
    let value = eval_node(child, vars)?;
    let number = value
        .as_num()
        .ok_or_else(|| MseError::Message(format!("{key} is not a number")))?;
    Ok(Some(number.round() as i32))
}

fn document_number(doc: &Document, key: &str) -> Result<Option<i32>, MseError> {
    let Some(node) = doc.first(key) else {
        return Ok(None);
    };
    let value = eval_node(node, &BTreeMap::new())?;
    Ok(value.as_num().map(|number| number.round() as i32))
}

fn eval_node(node: &Node, vars: &BTreeMap<String, Value>) -> Result<Value, MseError> {
    let text = script_source(node).unwrap_or_default();
    eval_script(&text, vars, &mut layout_host).map_err(|err| {
        if let Some(name) = err.0.strip_prefix("missing function: ") {
            MseError::MissingFunction(name.to_string())
        } else {
            MseError::Message(err.0)
        }
    })
}

fn script_source(node: &Node) -> Option<String> {
    match node {
        Node::Scalar(text) | Node::Script(text) => Some(text.clone()),
        Node::Map(fields) => fields
            .iter()
            .find(|(key, _)| key == "script")
            .and_then(|(_, child)| script_source(child)),
    }
}

fn script_env(card: &Map<String, Json>) -> BTreeMap<String, Value> {
    let mut fields = json_map(card);
    default_str(&mut fields, "card_symbol", "none");
    for key in ["level_0", "level_1", "level_2", "transformation", "alias"] {
        default_str(&mut fields, key, "");
    }
    let styling = card
        .get("boggled")
        .and_then(|value| value.as_object())
        .and_then(|studio| studio.get("styling"))
        .map(json_value)
        .unwrap_or(Value::Map(BTreeMap::new()));
    let mut card_style = BTreeMap::new();
    let stub = Value::Map(BTreeMap::from([("content_width".into(), Value::Num(0.0))]));
    for key in ["casting_cost", "casting_cost_2", "alias", "type_2"] {
        card_style.insert(key.into(), stub.clone());
    }
    BTreeMap::from([
        ("card".into(), Value::Map(fields)),
        ("styling".into(), styling),
        (
            "set".into(),
            Value::Map(BTreeMap::from([(
                "shorten_types_for_rarity".into(),
                Value::Bool(false),
            )])),
        ),
        ("card_style".into(), Value::Map(card_style)),
    ])
}

fn default_str(fields: &mut BTreeMap<String, Value>, key: &str, value: &str) {
    if !fields.contains_key(key) {
        fields.insert(key.into(), Value::Str(value.into()));
    }
}

fn json_map(card: &Map<String, Json>) -> BTreeMap<String, Value> {
    card.iter()
        .map(|(key, value)| (key.clone(), json_value(value)))
        .collect()
}

fn json_value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Nil,
        Json::Bool(bit) => Value::Bool(*bit),
        Json::Number(number) => Value::Num(number.as_f64().unwrap_or(0.0)),
        Json::String(text) => Value::Str(text.clone()),
        Json::Array(items) => Value::List(items.iter().map(json_value).collect()),
        Json::Object(map) => Value::Map(
            map.iter()
                .map(|(key, value)| (key.clone(), json_value(value)))
                .collect(),
        ),
    }
}

fn layout_host(name: &str, args: &[Arg]) -> Result<Value, String> {
    let flag = matches!(
        name,
        "is_leveler"
            | "is_borderless"
            | "is_planeswalker"
            | "is_thbland"
            | "is_dka"
            | "is_mutate"
            | "is_puma"
            | "is_season"
            | "is_prototype"
            | "is_scrolls"
            | "is_planeshifted"
            | "is_fnm"
            | "is_notched"
            | "uses_clear_frame"
            | "uses_expanded_art"
            | "clear_tops"
            | "use_evobar"
            | "use_flash_dot"
            | "has_identity"
            | "hidden_back_rarity"
            | "dka_fonts"
            | "transform_symbol_active_left"
            | "transform_symbol_active_right"
    );
    if flag {
        return Ok(Value::Bool(false));
    }
    match name {
        "top_of_textbox" => Ok(Value::Num(327.0)),
        "bottom_of_textbox" => Ok(Value::Num(481.0)),
        "max" => Ok(Value::Num(
            args.iter()
                .filter_map(|arg| arg.value.as_num())
                .fold(f64::MIN, f64::max),
        )),
        "min" => Ok(Value::Num(
            args.iter()
                .filter_map(|arg| arg.value.as_num())
                .fold(f64::MAX, f64::min),
        )),
        "contains" => Ok(Value::Bool(false)),
        "comma_count" => Ok(Value::Num(0.0)),
        "split_text" => Ok(Value::List(vec![Value::Num(0.0); 4])),
        "rgb" | "rgba" | "card_background" | "border_mask" | "default_image"
        | "image_mask_script" | "module_identity" => Ok(Value::Str(String::new())),
        "name_font" | "name_font_italic" | "body_font" | "body_font_italic" | "type_font"
        | "type_font_italic" => Ok(Value::Str("MPlantin".into())),
        "name_font_color" | "body_font_color" | "type_font_color" | "name_font_stroke_color"
        | "body_font_stroke_color" | "type_font_stroke_color" => Ok(Value::Str("black".into())),
        "name_font_size" => Ok(Value::Num(16.0)),
        "body_font_size" | "type_font_size" => Ok(Value::Num(12.0)),
        "name_font_vertical" | "shrink_name" | "shrink_type" | "type_font_vertical"
        | "chop_right" | "rare_width" | "prototype_height" | "name_font_stroke"
        | "name_font_stroke_blur" | "body_font_stroke" | "body_font_stroke_blur"
        | "type_font_stroke" | "type_font_stroke_blur" | "rarity_user_offset_left"
        | "rarity_user_offset_top" | "rarity_user_offset_width" | "rarity_user_offset_height" => {
            Ok(Value::Num(0.0))
        }
        other => Err(format!("missing function: {other}")),
    }
}

fn stylesheet(card: &Map<String, Json>) -> String {
    card.get("boggled")
        .and_then(|value| value.as_object())
        .and_then(|studio| studio.get("stylesheet"))
        .and_then(|value| value.as_str())
        .unwrap_or(M15_STYLESHEET)
        .to_string()
}

fn frame_letter(card: &Map<String, Json>) -> char {
    let colors = card
        .get("colors")
        .and_then(|value| value.as_array())
        .map(|colors| {
            colors
                .iter()
                .filter_map(|color| color.as_str())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let artifact = card
        .get("types")
        .and_then(|value| value.as_array())
        .map(|types| types.iter().any(|kind| kind.as_str() == Some("Artifact")))
        .unwrap_or(false);
    if colors.is_empty() {
        if artifact {
            'a'
        } else {
            'c'
        }
    } else if colors.len() > 1 {
        'm'
    } else {
        match colors[0].as_str() {
            "W" => 'w',
            "U" => 'u',
            "B" => 'b',
            "R" => 'r',
            "G" => 'g',
            _ => 'c',
        }
    }
}

fn frame_rgb(card: &Map<String, Json>) -> [u8; 3] {
    match frame_letter(card) {
        'w' => [236, 230, 214],
        'u' => [48, 104, 176],
        'b' => [52, 44, 52],
        'r' => [176, 56, 44],
        'g' => [40, 118, 68],
        'm' => [196, 146, 58],
        'a' => [126, 116, 110],
        _ => [156, 156, 160],
    }
}

fn paint_frame(image: &mut RgbImage, data_dir: Option<&Path>, card: &Map<String, Json>) -> bool {
    let Some(data_dir) = data_dir else {
        return false;
    };
    let letter = frame_letter(card);
    let path = data_dir
        .join("magic-modules.mse-include")
        .join("cards")
        .join("375 m15 simple")
        .join(format!("{letter}card.jpg"));
    let Ok(frame) = image::open(path) else {
        return false;
    };
    let rgb = frame.to_rgb8();
    let scaled = imageops::resize(
        &rgb,
        image.width(),
        image.height(),
        imageops::FilterType::Triangle,
    );
    blit(image, &scaled, 0, 0);
    true
}

struct Faces {
    name: Option<Vec<u8>>,
    body: Option<Vec<u8>>,
    italic: Option<Vec<u8>>,
}

fn load_faces(dir: Option<&Path>) -> Faces {
    let Some(dir) = dir else {
        return Faces { name: None, body: None, italic: None };
    };
    let fonts = font_files(dir);
    Faces {
        name: read_font(&fonts, &["beleren-bold_p1.01.ttf", "beleren-bold"]),
        body: read_font(&fonts, &["mplantin.ttf"]),
        italic: read_font(&fonts, &["mplanti1.ttf", "mplantinit.ttf"]),
    }
}

fn font_files(dir: &Path) -> Vec<(String, std::path::PathBuf)> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name.ends_with(".ttf") || name.ends_with(".otf") {
                files.push((name, path));
            }
        }
    }
    files
}

fn read_font(fonts: &[(String, std::path::PathBuf)], names: &[&str]) -> Option<Vec<u8>> {
    for wanted in names {
        if let Some((_, path)) = fonts.iter().find(|(name, _)| name == wanted || name.starts_with(wanted)) {
            return fs::read(path).ok();
        }
    }
    None
}

fn paint_placeholder(image: &mut RgbImage, data_dir: Option<&Path>, card: &Map<String, Json>, rect: Rect) {
    let Some(data_dir) = data_dir else {
        return;
    };
    let file = match frame_letter(card) {
        'w' => "white.jpg",
        'u' => "blue.jpg",
        'b' => "black.jpg",
        'r' => "red.jpg",
        'g' => "green.jpg",
        'a' => "artifact.jpg",
        'm' => "multicolor.jpg",
        _ => "colorless.jpg",
    };
    let path = data_dir.join("magic-default-image.mse-include").join(file);
    if let Ok(art) = image::open(path) {
        blit_stretch(image, &art, rect);
    }
}

fn paint_pt_badge(image: &mut RgbImage, data_dir: Option<&Path>, card: &Map<String, Json>) {
    let Some(data_dir) = data_dir else {
        return;
    };
    let path = data_dir
        .join("magic-modules.mse-include")
        .join("pts")
        .join("375 m15")
        .join(format!("{}pt.png", frame_letter(card)));
    let Ok(badge) = image::open(path) else {
        return;
    };
    let scaled = imageops::resize(&badge.to_rgba8(), 70, 39, imageops::FilterType::Triangle);
    blit_rgba(image, &scaled, 285, 464);
}

fn mana_row_width(cost: &str, _data_dir: Option<&Path>) -> i32 {
    if cost.is_empty() {
        return 0;
    }
    let count = mana_symbols(cost).len() as i32;
    if count == 0 { 0 } else { count * SYMBOL }
}

const SYMBOL: i32 = 16;
const TEXT_SYMBOL: i32 = 14;
const BODY_SIZE: f32 = 14.0;

fn paint_title(
    image: &mut RgbImage,
    font: Option<&FontRef<'_>>,
    text: &str,
    rect: Rect,
    color: [u8; 3],
    size: f32,
    align: &str,
) {
    if text.is_empty() || rect.width <= 0 || rect.height <= 0 {
        return;
    }
    let Some(font) = font else {
        paint_line(image, None, text, rect, color, 1);
        return;
    };
    let y = match align {
        "bottom" => rect.top + rect.height - size as i32,
        "top" => rect.top + 2,
        _ => rect.top + (rect.height - size as i32).max(0) / 2,
    };
    draw_text_mut(image, Rgb(color), rect.left, y, PxScale::from(size), font, text);
}

fn paint_pt_text(image: &mut RgbImage, font: Option<&FontRef<'_>>, text: &str, rect: Rect, color: [u8; 3]) {
    if text.is_empty() {
        return;
    }
    let Some(font) = font else {
        paint_line(image, None, text, rect, color, 1);
        return;
    };
    let size = 15.0;
    let width = text_width(font, text, size);
    let x = rect.left + ((rect.width as f32 - width as f32) / 2.0).round() as i32;
    let y = rect.top + (rect.height - size as i32).max(0) / 2;
    draw_text_mut(image, Rgb(color), x, y, PxScale::from(size), font, text);
}

fn inset_rules(rect: Rect) -> Rect {
    Rect {
        left: rect.left + 6,
        top: rect.top + 4,
        width: (rect.width - 12).max(1),
        height: (rect.height - 8).max(1),
    }
}

struct Piece {
    text: String,
    symbol: bool,
    italic: bool,
}

fn paint_textbox(
    image: &mut RgbImage,
    body: Option<&FontRef<'_>>,
    italic: Option<&FontRef<'_>>,
    data_dir: Option<&Path>,
    rules: &str,
    flavor: &str,
    rect: Rect,
    color: [u8; 3],
) {
    let Some(body) = body else {
        let mut text = rules.to_string();
        if !flavor.is_empty() {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(flavor);
        }
        paint_wrapped(image, None, &text, rect, color, 1);
        return;
    };
    let italic = italic.unwrap_or(body);
    let size = BODY_SIZE;
    let mut lines = layout_text(rules, body, italic, size, rect.width, false);
    if !flavor.is_empty() {
        if !lines.is_empty() {
            lines.push(Vec::new());
        }
        lines.extend(layout_text(flavor, body, italic, size, rect.width, true));
    }
    if lines.is_empty() {
        return;
    }
    let line_h = (size * 1.36).round() as i32;
    let block = lines.len() as i32 * line_h;
    let mut y = rect.top + (rect.height - block).max(0) / 2;
    for line in &lines {
        draw_pieces(image, body, italic, data_dir, line, rect.left, y, size, color);
        y += line_h;
    }
}

fn layout_text(
    text: &str,
    body: &FontRef<'_>,
    italic: &FontRef<'_>,
    size: f32,
    max_width: i32,
    force_italic: bool,
) -> Vec<Vec<Piece>> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(Vec::new());
            continue;
        }
        let mut line = Vec::new();
        let mut used = 0;
        for mut token in tokenize(paragraph) {
            if force_italic && !token.symbol {
                token.italic = true;
            }
            let width = piece_width(body, italic, &token, size);
            let gap_font = if token.italic { italic } else { body };
            let spaced = !line.is_empty() && wants_gap(&token);
            let gap = if spaced { text_width(gap_font, " ", size) } else { 0 };
            if used > 0 && used + gap + width > max_width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            if !line.is_empty() && wants_gap(&token) {
                line.push(Piece {
                    text: " ".into(),
                    symbol: false,
                    italic: token.italic,
                });
                used += text_width(gap_font, " ", size);
            }
            used += width;
            line.push(token);
        }
        if !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

fn tokenize(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut italic = false;
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(start) = rest.find('{') {
            push_words(&mut pieces, &rest[..start], &mut italic);
            rest = &rest[start + 1..];
            if let Some(end) = rest.find('}') {
                let symbol = rest[..end].trim().to_string();
                if !symbol.is_empty() {
                    pieces.push(Piece { text: symbol, symbol: true, italic: false });
                }
                rest = &rest[end + 1..];
            } else {
                push_words(&mut pieces, rest, &mut italic);
                break;
            }
        } else {
            push_words(&mut pieces, rest, &mut italic);
            break;
        }
    }
    pieces
}

fn push_words(pieces: &mut Vec<Piece>, text: &str, italic: &mut bool) {
    for word in text.split_whitespace() {
        if word.starts_with('(') {
            *italic = true;
        }
        pieces.push(Piece {
            text: word.to_string(),
            symbol: false,
            italic: *italic,
        });
        if word.ends_with(')') {
            *italic = false;
        }
    }
}

fn wants_gap(token: &Piece) -> bool {
    if token.symbol {
        return false;
    }
    !token.text.starts_with(|ch: char| matches!(ch, '.' | ',' | ')' | ':' | ';'))
}

fn piece_width(body: &FontRef<'_>, italic: &FontRef<'_>, piece: &Piece, size: f32) -> i32 {
    if piece.symbol {
        TEXT_SYMBOL
    } else {
        let font = if piece.italic { italic } else { body };
        text_width(font, &piece.text, size)
    }
}

fn draw_pieces(
    image: &mut RgbImage,
    body: &FontRef<'_>,
    italic: &FontRef<'_>,
    data_dir: Option<&Path>,
    pieces: &[Piece],
    mut x: i32,
    y: i32,
    size: f32,
    color: [u8; 3],
) {
    for piece in pieces {
        let font = if piece.italic { italic } else { body };
        if piece.symbol {
            let pip = TEXT_SYMBOL;
            let sy = y + (size as i32 - pip) / 2;
            if !paint_symbol(image, data_dir, Some(font), &piece.text, x, sy, pip) {
                draw_text_mut(image, Rgb(color), x, y, PxScale::from(size), font, &piece.text);
            }
            x += pip;
        } else {
            draw_text_mut(image, Rgb(color), x, y, PxScale::from(size), font, &piece.text);
            x += text_width(font, &piece.text, size);
        }
    }
}

fn text_width(font: &FontRef<'_>, text: &str, size: f32) -> i32 {
    let scaled = font.as_scaled(PxScale::from(size));
    let width: f32 = text.chars().map(|ch| scaled.h_advance(scaled.glyph_id(ch))).sum();
    width.round() as i32
}

fn paint_line(
    image: &mut RgbImage,
    font: Option<&FontRef<'_>>,
    text: &str,
    rect: Rect,
    color: [u8; 3],
    scale: i32,
) {
    if text.is_empty() || rect.width <= 0 || rect.height <= 0 {
        return;
    }
    if let Some(font) = font {
        let size = (rect.height as f32).clamp(8.0, 22.0);
        draw_text_mut(
            image,
            Rgb(color),
            rect.left,
            rect.top,
            PxScale::from(size),
            font,
            text,
        );
        return;
    }
    let mut used = scale.max(1);
    let advance = 8 * used;
    if text.chars().count() as i32 * advance > rect.width && used > 1 {
        used = 1;
    }
    draw_bitmap(image, text, rect.left, rect.top, used, color);
}

fn paint_wrapped(
    image: &mut RgbImage,
    font: Option<&FontRef<'_>>,
    text: &str,
    rect: Rect,
    color: [u8; 3],
    scale: i32,
) {
    if text.is_empty() || rect.width <= 0 || rect.height <= 0 {
        return;
    }
    if let Some(font) = font {
        let size = 13.0_f32;
        let mut y = rect.top;
        for line in wrap_text(text, (rect.width as usize / 7).max(1)) {
            if y > rect.top + rect.height - size as i32 {
                break;
            }
            draw_text_mut(
                image,
                Rgb(color),
                rect.left,
                y,
                PxScale::from(size),
                font,
                &line,
            );
            y += size as i32 + 2;
        }
        return;
    }
    let advance = 8 * scale.max(1);
    let cols = (rect.width / advance).max(1) as usize;
    let mut y = rect.top;
    for line in wrap_text(text, cols) {
        if y > rect.top + rect.height - advance {
            break;
        }
        draw_bitmap(image, &line, rect.left, y, scale.max(1), color);
        y += advance + 1;
    }
}

fn paint_cost(
    image: &mut RgbImage,
    data_dir: Option<&Path>,
    font: Option<&FontRef<'_>>,
    cost: &str,
    rect: Rect,
) {
    if cost.is_empty() {
        return;
    }
    let symbols = mana_symbols(cost);
    if symbols.is_empty() {
        return;
    }
    let pip = SYMBOL;
    let right = rect.left + rect.width;
    let mut x = right - pip * symbols.len() as i32;
    let y = rect.top + (rect.height - pip).max(0) / 2;
    for symbol in symbols {
        if !paint_symbol(image, data_dir, font, &symbol, x, y, pip) {
            let color = match symbol.as_str() {
                "W" => [248, 244, 220],
                "U" => [176, 214, 236],
                "B" => [72, 64, 72],
                "R" => [232, 150, 130],
                "G" => [168, 204, 150],
                _ => [214, 206, 190],
            };
            fill_rect(image, x, y, pip - 2, pip - 2, color);
            let ink = if symbol == "B" { [255, 249, 242] } else { [40, 28, 18] };
            draw_bitmap(image, &symbol, x + 3, y + 3, 1, ink);
        }
        x += pip;
    }
}

fn paint_symbol(
    image: &mut RgbImage,
    data_dir: Option<&Path>,
    font: Option<&FontRef<'_>>,
    symbol: &str,
    x: i32,
    y: i32,
    size: i32,
) -> bool {
    let Some(data_dir) = data_dir else {
        return false;
    };
    let file = match symbol {
        "W" => "mana_w.png",
        "U" => "mana_u.png",
        "B" => "mana_b.png",
        "R" => "mana_r.png",
        "G" => "mana_g.png",
        "C" => "mana_c.png",
        "T" => "mana_t.png",
        _ if !symbol.is_empty() && symbol.chars().all(|c| c.is_ascii_digit()) => "mana_circle.png",
        _ => return false,
    };
    let path = data_dir.join("magic-mana-large.mse-symbol-font").join(file);
    let Ok(img) = image::open(path) else {
        return false;
    };
    let scaled = imageops::resize(&img.to_rgba8(), size as u32, size as u32, imageops::FilterType::Triangle);
    blit_rgba(image, &scaled, x, y);
    if symbol.chars().all(|c| c.is_ascii_digit()) {
        if let Some(font) = font {
            let digit_size = (size as f32 * 0.62).max(8.0);
            let width = text_width(font, symbol, digit_size);
            let dx = x + ((size as f32 - width as f32) / 2.0).round() as i32;
            let dy = y + ((size as f32 - digit_size) / 2.0).round() as i32 - 1;
            draw_text_mut(image, Rgb([0, 0, 0]), dx, dy, PxScale::from(digit_size), font, symbol);
        }
    }
    true
}

fn blit_stretch(dst: &mut RgbImage, src: &DynamicImage, rect: Rect) {
    if rect.width <= 0 || rect.height <= 0 {
        return;
    }
    let scaled = imageops::resize(
        &src.to_rgba8(),
        rect.width as u32,
        rect.height as u32,
        imageops::FilterType::Triangle,
    );
    blit_rgba(dst, &scaled, rect.left, rect.top);
}

fn blit_rgba(dst: &mut RgbImage, src: &image::RgbaImage, x: i32, y: i32) {
    for (sx, sy, pixel) in src.enumerate_pixels() {
        let a = pixel.0[3] as u32;
        if a == 0 {
            continue;
        }
        let dx = x + sx as i32;
        let dy = y + sy as i32;
        if dx < 0 || dy < 0 || dx as u32 >= dst.width() || dy as u32 >= dst.height() {
            continue;
        }
        if a == 255 {
            dst.put_pixel(dx as u32, dy as u32, Rgb([pixel.0[0], pixel.0[1], pixel.0[2]]));
        } else {
            let bg = dst.get_pixel(dx as u32, dy as u32).0;
            let inv = 255 - a;
            dst.put_pixel(
                dx as u32,
                dy as u32,
                Rgb([
                    ((pixel.0[0] as u32 * a + bg[0] as u32 * inv) / 255) as u8,
                    ((pixel.0[1] as u32 * a + bg[1] as u32 * inv) / 255) as u8,
                    ((pixel.0[2] as u32 * a + bg[2] as u32 * inv) / 255) as u8,
                ]),
            );
        }
    }
}

fn mana_symbols(cost: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    let mut rest = cost;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else {
            break;
        };
        symbols.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    if symbols.is_empty() {
        symbols.push(cost.to_string());
    }
    symbols
}

fn wrap_text(text: &str, cols: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let extra = if line.is_empty() { 0 } else { 1 };
            if line.chars().count() + extra + word.chars().count() > cols && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

fn draw_bitmap(image: &mut RgbImage, text: &str, x: i32, y: i32, scale: i32, color: [u8; 3]) {
    for (index, ch) in text.chars().enumerate() {
        let glyph = font8x8::BASIC_FONTS.get(ch).unwrap_or([0; 8]);
        let origin_x = x + index as i32 * 8 * scale;
        for row in 0..8 {
            for col in 0..8 {
                if glyph[row] & (1 << col) != 0 {
                    fill_rect(
                        image,
                        origin_x + col * scale,
                        y + row as i32 * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }
}

fn blit_cover(image: &mut RgbImage, art: &DynamicImage, rect: Rect) {
    if rect.width <= 0 || rect.height <= 0 {
        return;
    }
    let rgb = art.to_rgb8();
    let scale = (rect.width as f32 / rgb.width().max(1) as f32)
        .max(rect.height as f32 / rgb.height().max(1) as f32);
    let width = ((rgb.width() as f32 * scale).round() as u32).max(1);
    let height = ((rgb.height() as f32 * scale).round() as u32).max(1);
    let scaled = imageops::resize(&rgb, width, height, imageops::FilterType::Triangle);
    let crop_x = (scaled.width() as i32 - rect.width).max(0) / 2;
    let crop_y = (scaled.height() as i32 - rect.height).max(0) / 2;
    for y in 0..rect.height {
        for x in 0..rect.width {
            let sx = crop_x + x;
            let sy = crop_y + y;
            if sx >= 0 && sy >= 0 && (sx as u32) < scaled.width() && (sy as u32) < scaled.height() {
                let pixel = *scaled.get_pixel(sx as u32, sy as u32);
                put(image, rect.left + x, rect.top + y, pixel.0);
            }
        }
    }
}

fn blit(image: &mut RgbImage, src: &RgbImage, x: i32, y: i32) {
    for (sx, sy, pixel) in src.enumerate_pixels() {
        put(image, x + sx as i32, y + sy as i32, pixel.0);
    }
}

fn fill_rect(image: &mut RgbImage, x: i32, y: i32, width: i32, height: i32, color: [u8; 3]) {
    for dy in 0..height.max(0) {
        for dx in 0..width.max(0) {
            put(image, x + dx, y + dy, color);
        }
    }
}

fn put(image: &mut RgbImage, x: i32, y: i32, color: [u8; 3]) {
    if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
        image.put_pixel(x as u32, y as u32, Rgb(color));
    }
}

fn encode_png(image: &RgbImage) -> Result<Vec<u8>, MseError> {
    let mut bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    use image::ImageEncoder;
    encoder
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|err| MseError::Message(err.to_string()))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{new_creature, object_map, NewCreature};
    use serde_json::json;

    fn creature(name: &str) -> Map<String, Json> {
        let mut fields = NewCreature::default();
        fields.name = name.into();
        fields.mana_cost = "2U".into();
        fields.type_line = "Creature - Bird".into();
        fields.text = "Flying".into();
        fields.power = "1".into();
        fields.toughness = "3".into();
        object_map(new_creature("STM", fields)).unwrap()
    }

    fn paint(card: &Map<String, Json>, style: Option<&str>, allow_unlisted: bool) -> Result<Vec<u8>, MseError> {
        draw_card(DrawRequest {
            card,
            art: None,
            style_text: style,
            data_dir: None,
            fonts_dir: None,
            allow_unlisted,
        })
    }

    #[test]
    fn a_creature_render_changes_when_the_name_changes() {
        let owl = paint(&creature("Brass Owl"), None, false).unwrap();
        let zebra = paint(&creature("Zebra Monk"), None, false).unwrap();
        assert!(owl.starts_with(b"\x89PNG"));
        assert_ne!(owl, zebra);
        let image = image::load_from_memory(&owl).unwrap();
        assert_eq!((image.width(), image.height()), (375, 523));
    }

    #[test]
    fn illustration_bytes_change_the_art_box() {
        let card = creature("Brass Owl");
        let bare = paint(&card, None, false).unwrap();
        let mut art = RgbImage::from_pixel(8, 8, Rgb([220, 20, 40]));
        art.put_pixel(1, 1, Rgb([10, 10, 10]));
        let mut bytes = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        use image::ImageEncoder;
        encoder
            .write_image(art.as_raw(), 8, 8, image::ExtendedColorType::Rgb8)
            .unwrap();
        let painted = draw_card(DrawRequest {
            card: &card,
            art: Some(&bytes),
            style_text: None,
            data_dir: None,
            fonts_dir: None,
            allow_unlisted: false,
        })
        .unwrap();
        assert_ne!(bare, painted);
    }

    #[test]
    fn the_script_else_branch_places_the_text_box() {
        let style = "\
card width: 180
card height: 240
card style:
\ttext:
\t\tleft: { if card.power == \"0\" then 4 else 12 }
\t\ttop: { top_of_textbox() }
\t\twidth: { if is_leveler() then 0 else 100 }
\t\theight: 40
";
        let layout = card_layout(style, &creature("Brass Owl")).unwrap();
        assert_eq!(layout.width, 180);
        assert_eq!(layout.rules.left, 12);
        assert_eq!(layout.rules.top, 327);
        assert_eq!(layout.rules.width, 100);
        let mut zero = creature("Brass Owl");
        zero.insert("power".into(), json!("0"));
        let moved = card_layout(style, &zero).unwrap();
        assert_eq!(moved.rules.left, 4);
    }

    #[test]
    fn other_styles_fail_with_the_package_name() {
        let mut card = creature("Brass Owl");
        card.get_mut("boggled")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("stylesheet".into(), json!("magic-saga.mse-style"));
        let error = paint(&card, None, false).unwrap_err();
        assert_eq!(error.to_string(), "missing package: magic-saga.mse-style");
    }

    #[test]
    fn a_non_creature_is_an_unsupported_frame() {
        let mut card = creature("Zap");
        card.insert("type".into(), json!("Instant"));
        card.insert("types".into(), json!(["Instant"]));
        let error = paint(&card, None, false).unwrap_err();
        assert_eq!(error.to_string(), "unsupported frame");
    }

    #[test]
    fn an_unknown_script_function_is_named() {
        let style = "\
card style:
\timage:
\t\tleft: { mystery_offset() }
\t\ttop: 1
\t\twidth: 2
\t\theight: 2
";
        let error = card_layout(style, &creature("Brass Owl")).unwrap_err();
        assert_eq!(error.to_string(), "missing function: mystery_offset");
    }

    #[test]
    fn the_pinned_m15_style_lays_out_a_default_creature_when_the_file_is_present() {
        let path = Path::new(r"C:\Users\cjr\AppData\Local\Temp\m15-main.style");
        if !path.is_file() {
            return;
        }
        let text = fs::read_to_string(path).unwrap();
        let layout = card_layout(&text, &creature("Brass Owl")).unwrap();
        assert_eq!(
            layout.image,
            Rect { left: 29, top: 59, width: 316, height: 231 }
        );
        assert_eq!(layout.rules.top, 327);
        assert_eq!(layout.rules.left, 29);
        assert_eq!(layout.rules.width, 314);
    }
}
