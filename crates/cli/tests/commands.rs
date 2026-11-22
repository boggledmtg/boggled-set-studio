use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct StopServer(PathBuf);

impl Drop for StopServer {
    fn drop(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.0.join("server.json")) else {
            return;
        };
        let Some(pid) = text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("\"pid\":")
                .and_then(|rest| rest.trim().trim_end_matches(',').parse::<u32>().ok())
        }) else {
            return;
        };
        let _ = if cfg!(windows) {
            Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/F"])
                .output()
        } else {
            Command::new("kill").args(["-9", &pid.to_string()]).output()
        };
    }
}

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_boggled"))
        .args(args)
        .env("BOGGLED_HOME", home)
        .env("BOGGLED_IDLE_SECS", "0")
        .output()
        .expect("run boggled")
}

#[test]
fn help_lists_serve_import_and_undo_and_skips_stats() {
    let output = Command::new(env!("CARGO_BIN_EXE_boggled"))
        .arg("--help")
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{text}");
    assert!(text.contains("serve"));
    assert!(text.contains("import"));
    assert!(text.contains("undo"));
    assert!(text.contains("redo"));
    assert!(!text.to_ascii_lowercase().contains("stats"));
}

#[test]
fn create_then_undo_removes_the_card() {
    let home = std::env::temp_dir().join(format!("boggled-cli-{}", uuid_name()));
    std::fs::create_dir_all(&home).unwrap();
    let _stop = StopServer(home.clone());
    let set = home.join("STM");
    let set_text = set.display().to_string();
    let init = run(
        &home,
        &["set", "init", "--path", &set_text, "--name", "Steam", "--code", "STM"],
    );
    assert!(
        init.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let created = run(
        &home,
        &[
            "--json",
            "--set",
            &set_text,
            "card",
            "create",
            "--name",
            "Brass Owl",
            "--type",
            "Creature",
        ],
    );
    assert!(
        created.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&created.stdout).unwrap();
    let uuid = body["uuid"].as_str().unwrap();
    let card = set.join("cards").join(format!("{uuid}.json"));
    assert!(card.is_file(), "create did not write {card:?}");

    let undone = run(&home, &["--set", &set_text, "undo"]);
    assert!(
        undone.status.success(),
        "undo failed: {}",
        String::from_utf8_lossy(&undone.stderr)
    );
    assert!(!card.exists(), "undo left the card in place");
    let message = String::from_utf8_lossy(&undone.stdout);
    assert!(message.contains("Create Brass Owl"), "{message}");
}

fn uuid_name() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}
