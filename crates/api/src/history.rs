//! In-memory undo for one process. Each open set has its own stack.
//!
//! A step is the files an API write changed. Undo applies the previous bytes
//! only when every file still matches the recorded result. A direct disk edit
//! leaves the step in place and clears redo.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct FileSnap {
    pub path: PathBuf,
    pub existed: bool,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub enum ActionKind {
    Files,
    /// Import (or any write that created a whole folder). Undo deletes it only
    /// when the tree still matches the files this step wrote.
    Tree { root: PathBuf },
}

#[derive(Clone, Debug)]
pub struct Action {
    pub label: String,
    pub kind: ActionKind,
    pub before: Vec<FileSnap>,
    pub after: Vec<FileSnap>,
}

#[derive(Default)]
pub struct History {
    pub undo: Vec<Action>,
    pub redo: Vec<Action>,
}

pub enum StepError {
    Empty,
    Conflict,
    Io(String),
}

pub fn snap(path: &Path) -> FileSnap {
    match fs::read(path) {
        Ok(bytes) => FileSnap {
            path: path.to_path_buf(),
            existed: true,
            bytes,
        },
        Err(_) => FileSnap {
            path: path.to_path_buf(),
            existed: false,
            bytes: Vec::new(),
        },
    }
}

pub fn snaps(paths: &[PathBuf]) -> Vec<FileSnap> {
    paths.iter().map(|path| snap(path)).collect()
}

pub fn snapshot_tree(root: &Path) -> Vec<FileSnap> {
    let mut files = Vec::new();
    walk(root, &mut files);
    files.sort();
    files.into_iter().map(|path| snap(&path)).collect()
}

impl History {
    pub fn labels(&self) -> (Vec<String>, Vec<String>) {
        (
            self.undo.iter().map(|action| action.label.clone()).collect(),
            self.redo.iter().map(|action| action.label.clone()).collect(),
        )
    }

    pub fn push(&mut self, action: Action) {
        self.undo.push(action);
        self.redo.clear();
    }

    pub fn undo(&mut self) -> Result<Action, StepError> {
        let Some(action) = self.undo.last() else {
            return Err(StepError::Empty);
        };
        if !matches_after(action) {
            self.redo.clear();
            return Err(StepError::Conflict);
        }
        apply(&action.before, &action.kind, false)?;
        let action = self.undo.pop().expect("undo step");
        self.redo.push(action.clone());
        Ok(action)
    }

    pub fn redo(&mut self) -> Result<Action, StepError> {
        let Some(action) = self.redo.last() else {
            return Err(StepError::Empty);
        };
        if !matches_before(action) {
            return Err(StepError::Conflict);
        }
        apply(&action.after, &action.kind, true)?;
        let action = self.redo.pop().expect("redo step");
        self.undo.push(action.clone());
        Ok(action)
    }
}

fn matches_after(action: &Action) -> bool {
    match &action.kind {
        ActionKind::Files => action.after.iter().all(snap_matches),
        ActionKind::Tree { root } => tree_matches(root, &action.after),
    }
}

fn matches_before(action: &Action) -> bool {
    match &action.kind {
        ActionKind::Files => action.before.iter().all(snap_matches),
        ActionKind::Tree { root } => !root.exists(),
    }
}

fn snap_matches(snap: &FileSnap) -> bool {
    match fs::read(&snap.path) {
        Ok(bytes) => snap.existed && bytes == snap.bytes,
        Err(_) => !snap.existed && !snap.path.exists(),
    }
}

fn tree_matches(root: &Path, after: &[FileSnap]) -> bool {
    if !root.is_dir() {
        return false;
    }
    let mut expected = BTreeSet::new();
    for snap in after {
        if !snap_matches(snap) {
            return false;
        }
        let Ok(rel) = snap.path.strip_prefix(root) else {
            return false;
        };
        expected.insert(rel.to_string_lossy().replace('\\', "/"));
    }
    let mut actual = Vec::new();
    walk(root, &mut actual);
    if actual.len() != expected.len() {
        return false;
    }
    actual.into_iter().all(|path| {
        path.strip_prefix(root)
            .map(|rel| expected.contains(&rel.to_string_lossy().replace('\\', "/")))
            .unwrap_or(false)
    })
}

fn apply(snaps: &[FileSnap], kind: &ActionKind, forward: bool) -> Result<(), StepError> {
    if let ActionKind::Tree { root } = kind {
        if !forward {
            if root.exists() {
                fs::remove_dir_all(root).map_err(|err| StepError::Io(err.to_string()))?;
            }
            return Ok(());
        }
    }
    for snap in snaps {
        if snap.existed {
            if let Some(parent) = snap.path.parent() {
                fs::create_dir_all(parent).map_err(|err| StepError::Io(err.to_string()))?;
            }
            store::atomic_write(&snap.path, &snap.bytes)
                .map_err(|err| StepError::Io(err.to_string()))?;
        } else if snap.path.is_file() {
            fs::remove_file(&snap.path).map_err(|err| StepError::Io(err.to_string()))?;
        }
    }
    Ok(())
}

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}
