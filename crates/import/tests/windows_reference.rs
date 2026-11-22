#![cfg(windows)]

use mse::native::{render, Face};
use std::path::PathBuf;
use store::SetRoot;

#[test]
#[ignore = "requires the pinned Windows MSE installation and eleven reference exports"]
fn imported_set_matches_every_reference_pixel() {
    let runtime = std::env::var_os("BSS_MSE_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../bss-mse-reference")
        });
    let scratch = tempfile::tempdir().unwrap();
    let dest = scratch.path().join("imported");
    let report = import::import_mse_set(&runtime.join("fixtures.mse-set"), &dest).unwrap();
    assert_eq!(report.cards, 11);
    let root = SetRoot::open(&dest).unwrap();
    let faces: Vec<_> = root
        .list_faces()
        .unwrap()
        .into_iter()
        .map(|f| Face {
            card: f.value.as_object().unwrap().clone(),
            art: None,
        })
        .collect();
    let set = root.read_set().unwrap().0;
    let mut failures = Vec::new();
    for (i, face) in faces.iter().enumerate() {
        let name = face.card["name"].as_str().unwrap();
        let result = render(&runtime, &set, &faces, i).unwrap();
        let actual = image::load_from_memory(&result.png).unwrap().to_rgba8();
        let expected = image::open(runtime.join("reference-pngs").join(format!("{name}.png")))
            .unwrap()
            .to_rgba8();
        assert_eq!(actual.dimensions(), expected.dimensions());
        let changed = actual
            .pixels()
            .zip(expected.pixels())
            .filter(|(a, b)| a != b)
            .count();
        if changed > 0 {
            failures.push((name, changed));
        }
        assert!(result.layout["name"][2].as_f64().unwrap() > 0.0);
    }
    assert!(failures.is_empty(), "differing pixels: {failures:?}");
}
