fn main() {
    let attributes = tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["pick_folder", "pick_file", "save_file"]),
    );
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
