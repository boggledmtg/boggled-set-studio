//! Desktop window. The set folder and the API stay in the `boggled` server.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[tauri::command]
fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose a folder")
        .pick_folder()
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn pick_file() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Choose a file")
        .pick_file()
        .map(|path| path.display().to_string())
}

#[tauri::command]
fn save_file(suggested: Option<String>) -> Option<String> {
    let mut dialog = rfd::FileDialog::new().set_title("Export set");
    if let Some(name) = suggested.as_deref() {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file().map(|path| path.display().to_string())
}

fn main() {
    let server = ensure_server().expect("boggled serve");
    let token = server.token.clone();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![pick_folder, pick_file, save_file])
        .setup(move |app| {
            let url = format!("http://127.0.0.1:{}/?shell=1", server.port)
                .parse()
                .expect("server url");
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("Boggled Set Studio")
                .inner_size(1440.0, 900.0)
                .decorations(false)
                .initialization_script(format!(
                    "window.__BOGGLED__ = {{ token: \"{token}\", url: \"\" }};"
                ))
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("boggled desktop");
}

struct RunningServer {
    port: u16,
    token: String,
}

fn ensure_server() -> Result<RunningServer, String> {
    let support = api::support_dir();
    std::fs::create_dir_all(&support).map_err(|err| err.to_string())?;
    let file = support.join("server.json");
    if let Some(server) = read_server(&file) {
        if port_open(server.port) {
            return Ok(server);
        }
    }
    let exe = boggled_exe().ok_or("boggled was not found next to the desktop app")?;
    hide_stdio_from_children();
    Command::new(exe)
        .arg("serve")
        .current_dir(serve_cwd())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("could not start boggled serve: {err}"))?;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        if let Some(server) = read_server(&file) {
            if port_open(server.port) {
                return Ok(server);
            }
        }
    }
    Err("boggled serve did not become ready".into())
}

fn read_server(path: &Path) -> Option<RunningServer> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let port = value.get("port")?.as_u64()? as u16;
    let token = value.get("token")?.as_str()?.to_string();
    if token.is_empty() {
        return None;
    }
    Some(RunningServer { port, token })
}

fn port_open(port: u16) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
}

fn boggled_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "boggled.exe" } else { "boggled" };
    let sibling = exe.parent()?.join(name);
    sibling.is_file().then_some(sibling)
}

fn serve_cwd() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..4 {
            let Some(path) = dir.clone() else { break };
            if path.join("web").join("dist").join("index.html").is_file()
                || path.join("web").join("index.html").is_file()
            {
                return path;
            }
            dir = path.parent().map(Path::to_path_buf);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn hide_stdio_from_children() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        extern "system" {
            fn SetHandleInformation(handle: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
        }
        const HANDLE_FLAG_INHERIT: u32 = 1;
        unsafe {
            for handle in [
                std::io::stdin().as_raw_handle(),
                std::io::stdout().as_raw_handle(),
                std::io::stderr().as_raw_handle(),
            ] {
                SetHandleInformation(handle as *mut core::ffi::c_void, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}
