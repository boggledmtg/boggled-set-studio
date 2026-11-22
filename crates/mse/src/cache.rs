//! Install a Full Magic Pack zip into the local cache.
//!
//! Install the data and fonts from the pinned archive. On Windows also install
//! the executable used by the native bridge.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};

use domain::{FMP_COMMIT, FMP_REPO, M15_STYLESHEET};
use zip::ZipArchive;

use crate::MseError;

const SKIP_FILES: &[&str] = &[
    "magicseteditor.exe",
    "mse.exe",
    "magicseteditor.com",
    "mse.com",
];

pub fn pack_zip_url() -> String {
    format!("https://codeload.github.com/{FMP_REPO}/zip/{FMP_COMMIT}")
}

pub fn style_path(pack_root: &Path, stylesheet: &str) -> PathBuf {
    pack_root.join("data").join(stylesheet).join("style")
}

pub fn require_pack(pack_root: &Path) -> Result<PathBuf, MseError> {
    let path = style_path(pack_root, M15_STYLESHEET);
    if path.is_file() && (!cfg!(windows) || pack_root.join("magicseteditor.exe").is_file()) {
        Ok(path)
    } else {
        Err(MseError::Message(
            "Full Magic Pack is not downloaded. Run boggled pack download".into(),
        ))
    }
}

pub fn extract_pack_zip(bytes: &[u8], dest: &Path) -> Result<(), MseError> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor).map_err(|err| MseError::Message(err.to_string()))?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|err| MseError::Message(err.to_string()))?;
        names.push(file.name().replace('\\', "/"));
    }
    let strip = common_root(&names);
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| MseError::Message(err.to_string()))?;
        let raw = file.name().replace('\\', "/");
        if raw.ends_with('/') {
            continue;
        }
        let relative = strip_root(&raw, strip);
        if relative.is_empty() || !wanted(&relative) {
            continue;
        }
        let path = safe_join(dest, &relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| MseError::Message(err.to_string()))?;
        }
        let mut out = fs::File::create(&path).map_err(|err| MseError::Message(err.to_string()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|err| MseError::Message(err.to_string()))?;
        out.write_all(&buf)
            .map_err(|err| MseError::Message(err.to_string()))?;
    }
    Ok(())
}

fn common_root(names: &[String]) -> &str {
    let mut files = names.iter().filter(|name| !name.ends_with('/'));
    let Some(first) = files.next() else {
        return "";
    };
    let Some((root, _)) = first.split_once('/') else {
        return "";
    };
    if root.is_empty() {
        return "";
    }
    if files.all(|name| name.starts_with(root) && name[root.len()..].starts_with('/')) {
        root
    } else {
        ""
    }
}

fn strip_root<'a>(name: &'a str, root: &str) -> &'a str {
    if root.is_empty() {
        name
    } else {
        name.trim_start_matches(root).trim_start_matches('/')
    }
}

fn wanted(relative: &str) -> bool {
    if cfg!(windows) && relative == "magicseteditor.exe" {
        return true;
    }
    let file_name = relative.rsplit('/').next().unwrap_or(relative);
    if SKIP_FILES.iter().any(|skip| file_name.eq_ignore_ascii_case(skip)) {
        return false;
    }
    if file_name.to_ascii_lowercase().ends_with(".odt") {
        return false;
    }
    relative.starts_with("data/") || relative.starts_with("Magic - Fonts/")
}

fn safe_join(dest: &Path, relative: &str) -> Result<PathBuf, MseError> {
    let mut out = dest.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err(MseError::Message("pack zip escapes the cache".into()));
        }
        out.push(part);
    }
    if out.components().any(|component| component == Component::ParentDir) {
        return Err(MseError::Message("pack zip escapes the cache".into()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn sample_zip() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(cursor);
        let opts = SimpleFileOptions::default();
        zip.start_file(
            "Full-Magic-Pack/data/magic-m15-altered.mse-style/style",
            opts,
        )
        .unwrap();
        zip.write_all(b"card width: 375\n").unwrap();
        zip.start_file("Full-Magic-Pack/magicseteditor.exe", opts)
            .unwrap();
        zip.write_all(b"binary").unwrap();
        zip.start_file("Full-Magic-Pack/Magic - Fonts/MPlantin.txt", opts)
            .unwrap();
        zip.write_all(b"font").unwrap();
        zip.start_file("Full-Magic-Pack/docs/tutorial.odt", opts)
            .unwrap();
        zip.write_all(b"odt").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_pack_zip_installs_the_platform_runtime() {
        let dir = std::env::temp_dir().join(format!(
            "boggled-pack-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        extract_pack_zip(&sample_zip(), &dir).unwrap();
        let style = style_path(&dir, M15_STYLESHEET);
        assert_eq!(fs::read_to_string(&style).unwrap(), "card width: 375\n");
        assert_eq!(
            fs::read_to_string(dir.join("Magic - Fonts").join("MPlantin.txt")).unwrap(),
            "font"
        );
        assert_eq!(dir.join("magicseteditor.exe").exists(), cfg!(windows));
        assert!(!dir.join("docs").exists());
        let error = require_pack(&dir.join("missing")).unwrap_err();
        assert!(error.to_string().contains("boggled pack download"));
        let _ = fs::remove_dir_all(&dir);
    }
}
