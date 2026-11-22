//! The set folder is the database. Reads and writes are one file at a time.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use domain::{
    canonical_face, canonical_keyword, canonical_set, content_hash, new_set, string_field,
    DocumentError,
};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("{0}")]
    Message(String),
    #[error("revision conflict")]
    Conflict,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Document(#[from] DocumentError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub struct SetRoot {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct FaceRef {
    pub folder: String,
    pub uuid: String,
    pub value: Value,
    pub revision: String,
}

impl SetRoot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init(dir: &Path, name: &str, code: &str) -> Result<Self, StoreError> {
        if dir.exists() {
            return Err(StoreError::Message(format!(
                "{} already exists",
                dir.display()
            )));
        }
        fs::create_dir_all(dir)?;
        for folder in ["cards", "tokens", "keywords", "art", "renders", "symbols", "docs"] {
            fs::create_dir_all(dir.join(folder))?;
            let keep = dir.join(folder).join(".gitkeep");
            if folder != "docs" {
                fs::write(keep, b"")?;
            }
        }
        fs::write(dir.join("docs").join("design-brief.md"), design_brief())?;
        fs::write(dir.join("docs").join("schema.md"), schema_note())?;
        fs::write(dir.join("README.md"), format!("# {name}\n\nA custom Magic set.\n"))?;
        fs::write(dir.join(".gitignore"), gitignore())?;
        fs::write(dir.join(".gitattributes"), gitattributes())?;
        fs::write(dir.join("AGENTS.md"), harness())?;
        let root = Self {
            path: dir.to_path_buf(),
        };
        root.write_document("set.json", &canonical_set(&new_set(name, code)), None)?;
        Ok(root)
    }

    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        let path = dir.to_path_buf();
        if !path.join("set.json").is_file() {
            return Err(StoreError::Message(format!(
                "{} is not a set folder",
                path.display()
            )));
        }
        Ok(Self { path })
    }

    pub fn read_set(&self) -> Result<(Value, String), StoreError> {
        self.read_json("set.json")
    }

    pub fn write_set(&self, value: &Value, expected: Option<&str>) -> Result<String, StoreError> {
        self.write_document("set.json", &canonical_set(value), expected)
    }

    pub fn list_faces(&self) -> Result<Vec<FaceRef>, StoreError> {
        let mut faces = Vec::new();
        for folder in ["cards", "tokens"] {
            faces.extend(self.list_folder(folder)?);
        }
        faces.sort_by(|a, b| {
            let an = string_field(a.value.as_object().unwrap(), "number");
            let bn = string_field(b.value.as_object().unwrap(), "number");
            folder_rank(&a.folder)
                .cmp(&folder_rank(&b.folder))
                .then_with(|| number_key(&an).cmp(&number_key(&bn)))
                .then_with(|| a.uuid.cmp(&b.uuid))
        });
        Ok(faces)
    }

    pub fn read_face(&self, uuid: &str) -> Result<FaceRef, StoreError> {
        for folder in ["cards", "tokens"] {
            let rel = format!("{folder}/{uuid}.json");
            if self.path.join(&rel).is_file() {
                let (value, revision) = self.read_json(&rel)?;
                return Ok(FaceRef {
                    folder: folder.into(),
                    uuid: uuid.into(),
                    value,
                    revision,
                });
            }
        }
        Err(StoreError::NotFound)
    }

    pub fn write_face(
        &self,
        folder: &str,
        uuid: &str,
        value: &Value,
        expected: Option<&str>,
    ) -> Result<String, StoreError> {
        check_id(uuid)?;
        if folder != "cards" && folder != "tokens" {
            return Err(StoreError::Message("faces live in cards or tokens".into()));
        }
        let rel = format!("{folder}/{uuid}.json");
        self.write_document(&rel, &canonical_face(value), expected)
    }

    pub fn delete_face(&self, uuid: &str) -> Result<FaceRef, StoreError> {
        let face = self.read_face(uuid)?;
        let rel = format!("{}/{uuid}.json", face.folder);
        fs::remove_file(self.path.join(&rel))?;
        Ok(face)
    }

    pub fn list_keywords(&self) -> Result<Vec<(String, Value, String)>, StoreError> {
        let mut out = Vec::new();
        let dir = self.path.join("keywords");
        if !dir.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") {
                continue;
            }
            let id = name.trim_end_matches(".json").to_string();
            let (value, revision) = self.read_json(&format!("keywords/{name}"))?;
            out.push((id, value, revision));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub fn read_keyword(&self, id: &str) -> Result<(Value, String), StoreError> {
        check_id(id)?;
        self.read_json(&format!("keywords/{id}.json"))
    }

    pub fn write_keyword(
        &self,
        id: &str,
        value: &Value,
        expected: Option<&str>,
    ) -> Result<String, StoreError> {
        check_id(id)?;
        self.write_document(
            &format!("keywords/{id}.json"),
            &canonical_keyword(value),
            expected,
        )
    }

    pub fn delete_keyword(&self, id: &str) -> Result<(), StoreError> {
        check_id(id)?;
        let path = self.path.join(format!("keywords/{id}.json"));
        if !path.is_file() {
            return Err(StoreError::NotFound);
        }
        fs::remove_file(path)?;
        Ok(())
    }

    pub fn place_art(&self, uuid: &str, source: &Path) -> Result<String, StoreError> {
        check_id(uuid)?;
        if !source.is_file() {
            return Err(StoreError::Message(format!(
                "illustration {} was not found",
                source.display()
            )));
        }
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif") {
            return Err(StoreError::Message(
                "illustration must be a png, jpg, webp, or gif".into(),
            ));
        }
        let rel = format!("art/{uuid}.{ext}");
        let bytes = fs::read(source)?;
        self.write_bytes(&rel, &bytes)?;
        Ok(rel)
    }

    pub fn write_bytes(&self, rel: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.resolve(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        atomic_write(&path, bytes)?;
        Ok(())
    }

    pub fn read_bytes(&self, rel: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.resolve(rel)?;
        if !path.is_file() {
            return Err(StoreError::NotFound);
        }
        Ok(fs::read(path)?)
    }

    pub fn remove_rel(&self, rel: &str) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.resolve(rel)?;
        if !path.exists() {
            return Ok(None);
        }
        if path.is_dir() {
            return Err(StoreError::Message(format!("{rel} is a directory")));
        }
        let bytes = fs::read(&path)?;
        fs::remove_file(path)?;
        Ok(Some(bytes))
    }

    pub fn resolve(&self, rel: &str) -> Result<PathBuf, StoreError> {
        let rel_path = Path::new(rel);
        if rel_path.is_absolute() {
            return Err(StoreError::Message("path escapes the set".into()));
        }
        for component in rel_path.components() {
            match component {
                Component::Normal(_) | Component::CurDir => {}
                _ => return Err(StoreError::Message("path escapes the set".into())),
            }
        }
        Ok(self.path.join(rel_path))
    }

    fn list_folder(&self, folder: &str) -> Result<Vec<FaceRef>, StoreError> {
        let dir = self.path.join(folder);
        let mut faces = Vec::new();
        if !dir.exists() {
            return Ok(faces);
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".json") {
                continue;
            }
            let uuid = name.trim_end_matches(".json").to_string();
            let (value, revision) = self.read_json(&format!("{folder}/{name}"))?;
            faces.push(FaceRef {
                folder: folder.into(),
                uuid,
                value,
                revision,
            });
        }
        Ok(faces)
    }

    fn read_json(&self, rel: &str) -> Result<(Value, String), StoreError> {
        let bytes = self.read_bytes(rel)?;
        let value = serde_json::from_slice(&bytes)
            .map_err(|e| StoreError::Message(format!("{rel} is not valid JSON: {e}")))?;
        Ok((value, content_hash(&bytes)))
    }

    fn write_document(
        &self,
        rel: &str,
        text: &str,
        expected: Option<&str>,
    ) -> Result<String, StoreError> {
        let path = self.resolve(rel)?;
        if let Some(expected) = expected {
            if path.is_file() {
                let current = content_hash(&fs::read(&path)?);
                if current != expected {
                    return Err(StoreError::Conflict);
                }
            } else if !expected.is_empty() {
                return Err(StoreError::Conflict);
            }
        }
        let bytes = text.as_bytes();
        atomic_write(&path, bytes)?;
        Ok(content_hash(bytes))
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn check_id(id: &str) -> Result<(), StoreError> {
    if id.is_empty()
        || id.contains(['/', '\\', '.'])
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(StoreError::Message("identifier is not a single file name".into()));
    }
    Ok(())
}

fn folder_rank(folder: &str) -> u8 {
    if folder == "cards" {
        0
    } else {
        1
    }
}

fn number_key(number: &str) -> (u32, String) {
    let digits: String = number.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n = digits.parse().unwrap_or(u32::MAX);
    (n, number.to_string())
}

fn design_brief() -> &'static str {
    "Write the set's prompt and the decisions that follow it here.\n"
}

fn schema_note() -> &'static str {
    "Each face is one JSON file. Studio fields live under boggled. \
The card fields this editor derives are mana value, colors, color identity, \
and the type arrays. Unknown keys are kept.\n"
}

fn gitignore() -> &'static str {
    ".DS_Store\nThumbs.db\n.boggled/\nexport/\n"
}

fn gitattributes() -> &'static str {
    "art/** filter=lfs diff=lfs merge=lfs -text\nrenders/** filter=lfs diff=lfs merge=lfs -text\n"
}

fn harness() -> &'static str {
    r#"# Set harness

This file is for the coding agent designing the set. The application repository has its own harness.

1. Read `docs/design-brief.md`, `set.json`, and `docs/schema.md`.
2. Record lore, art direction, and themes on the set before creating cards.
3. Write one face per file. Keep unknown keys and the `boggled` object.
4. Put illustrations in `art/` and point `boggled.artFile` at them, preferably with `boggled card set-art`.
5. Generate illustrations with the user's own tools. This application does not generate images.
6. Run `boggled render` after a batch of writes when the editor is closed. When the server is running, saving the file renders a supported creature.
7. Run `boggled set validate` before considering a batch done.
8. Keep rendered PNGs in `renders/`.

Only a normal-layout creature on the M15 Main frame is rendered. Other faces are stored and listed.
"#
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{is_supported_creature, new_creature, NewCreature};

    fn temp_dir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "boggled-store-{}-{}",
            std::process::id(),
            uuid_ish()
        ));
        let _ = fs::remove_dir_all(&path);
        path
    }

    fn uuid_ish() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
    }

    #[test]
    fn init_writes_a_set_and_a_second_init_is_refused() {
        let dir = temp_dir();
        let root = SetRoot::init(&dir, "Brass Owl", "stm").unwrap();
        assert!(dir.join("set.json").is_file());
        assert!(dir.join("AGENTS.md").is_file());
        assert!(dir.join("cards").is_dir());
        let (set, revision) = root.read_set().unwrap();
        assert_eq!(set["code"], "STM");
        assert!(SetRoot::init(&dir, "Other", "ZZZ").is_err());
        let mut edited = set.clone();
        edited["name"] = serde_json::json!("Renamed");
        let next = root.write_set(&edited, Some(&revision)).unwrap();
        assert_ne!(next, revision);
        assert!(root.write_set(&edited, Some(&revision)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_face_round_trips_and_a_path_cannot_escape() {
        let dir = temp_dir();
        let root = SetRoot::init(&dir, "Set", "STM").unwrap();
        let card = new_creature("STM", NewCreature::default());
        let uuid = card["uuid"].as_str().unwrap().to_string();
        let revision = root.write_face("cards", &uuid, &card, None).unwrap();
        let loaded = root.read_face(&uuid).unwrap();
        assert_eq!(loaded.revision, revision);
        assert!(is_supported_creature(loaded.value.as_object().unwrap()));
        assert!(root.resolve("../outside").is_err());
        assert!(root.resolve("cards/../../secret").is_err());
        root.delete_face(&uuid).unwrap();
        assert!(root.read_face(&uuid).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn art_is_copied_into_the_set() {
        let dir = temp_dir();
        let root = SetRoot::init(&dir, "Set", "STM").unwrap();
        let src = dir.join("source.png");
        fs::write(&src, b"\x89PNG fake").unwrap();
        let rel = root
            .place_art("11111111-1111-1111-1111-111111111111", &src)
            .unwrap();
        assert_eq!(rel, "art/11111111-1111-1111-1111-111111111111.png");
        assert!(root.path.join(&rel).is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
