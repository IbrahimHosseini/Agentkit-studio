#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
const MAX_ATTACHMENT: u64 = 16 * 1024 * 1024;
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
#[derive(Default)]
struct StartupState(std::sync::atomic::AtomicU8);
impl StartupState {
    fn pending(&self) -> bool { self.0.load(std::sync::atomic::Ordering::SeqCst) == 0 }
    fn finish(&self, state: u8) -> bool {
        self.0.compare_exchange(0, state, std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst).is_ok()
    }
}
fn startup_trace(stage: &str) {
    if std::env::var_os("AGENTKIT_STARTUP_DIAGNOSTICS").is_some() {
        eprintln!("studio-startup {}ms {stage}", STARTED.get_or_init(std::time::Instant::now).elapsed().as_millis());
    }
}
#[tauri::command]
fn frontend_ready(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.label() != "main" { return Err("Unknown startup window".into()); }
    let handle = window.clone();
    window.run_on_main_thread(move || {
        let app = handle.app_handle();
        if !app.state::<StartupState>().pending() { return; }
        startup_trace("frontend-mounted");
        if handle.show().is_err() {
            startup_failure(app.clone());
            return;
        }
        app.state::<StartupState>().finish(1);
        let _ = handle.set_focus();
        startup_trace("window-visible");
    }).map_err(|e| e.to_string())
}
fn startup_failure(app: tauri::AppHandle) {
    if !app.state::<StartupState>().finish(2) { return; }
    startup_trace("startup-failed");
    let exit = app.clone();
    app.dialog()
        .message("The interface could not finish loading. Close and reopen Agentkit Studio. If this continues, reinstall the application. Your conversations are kept.")
        .title("Agentkit Studio could not start")
        .kind(tauri_plugin_dialog::MessageDialogKind::Error)
        .show(move |_| exit.exit(1));
}

#[tauri::command]
async fn engine_request(method: String, params: Value) -> Result<Value, String> {
    agentkit_core::client::request(&method, params)
        .await
        .map_err(|e| e.to_string())
}
fn image_mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}
fn describe(path: &str) -> Result<Value, String> {
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    let meta = fs::metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_ATTACHMENT {
        return Err("Choose a regular file up to 16 MiB".into());
    }
    let mime = image_mime(&path);
    let bytes = if mime.starts_with("image/") && meta.len() <= 2 * 1024 * 1024 {
        Some(fs::read(&path).map_err(|e| e.to_string())?)
    } else {
        None
    };
    Ok(
        json!({"path":path.to_string_lossy(),"name":path.file_name().unwrap_or_default().to_string_lossy(),"size":meta.len(),"mime":mime,"kind":if mime.starts_with("image/"){"image"}else{"file"},"preview_bytes":bytes}),
    )
}
#[tauri::command]
fn attachment_info(paths: Vec<String>) -> Result<Vec<Value>, String> {
    if paths.len() > 12 {
        return Err("Attach at most 12 files".into());
    }
    paths.iter().map(|p| describe(p)).collect()
}
#[tauri::command]
fn stage_clipboard_image(
    app: tauri::AppHandle,
    bytes: Vec<u8>,
    mime: String,
) -> Result<Value, String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ATTACHMENT {
        return Err("Image must be between 1 byte and 16 MiB".into());
    }
    let (ext, valid) = match mime.as_str() {
        "image/png" => ("png", bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
        "image/jpeg" => ("jpg", bytes.starts_with(b"\xff\xd8\xff")),
        "image/gif" => (
            "gif",
            bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        ),
        "image/webp" => (
            "webp",
            bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        ),
        _ => return Err("Supported images: PNG, JPEG, GIF, WebP".into()),
    };
    if !valid {
        return Err("Image content does not match its format".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("attachments");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let used: u64 = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    if used + bytes.len() as u64 > 64 * 1024 * 1024 {
        return Err("Pasted-image storage reached 64 MiB. Attach an existing file instead.".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = dir.join(format!("paste-{}-{}.{}", std::process::id(), stamp, ext));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes).map_err(|e| e.to_string())?;
    describe(&path.to_string_lossy())
}
/// Delete only an app-owned staged image that the daemon confirms is not history.
#[tauri::command]
async fn remove_staged_attachment(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?.join("attachments");
    let directory = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let candidate = fs::canonicalize(&path).map_err(|e| e.to_string())?;
    if candidate.parent() != Some(directory.as_path())
        || !candidate.file_name().and_then(|n| n.to_str()).is_some_and(|name| name.starts_with("paste-"))
        || !candidate.is_file()
    {
        return Err("Only Studio-staged clipboard images can be removed".into());
    }
    let reference = agentkit_core::client::request("studio/attachment/referenced", json!({"path":candidate.to_string_lossy()}))
        .await.map_err(|e| e.to_string())?;
    if reference.get("referenced").and_then(Value::as_bool) != Some(false) {
        return Err("This image is retained by conversation history".into());
    }
    fs::remove_file(candidate).map_err(|e| e.to_string())
}
fn main() {
    STARTED.get_or_init(std::time::Instant::now);
    startup_trace("process-started");
    tauri::Builder::default()
        .manage(StartupState::default())
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|_, payload| {
            startup_trace(match payload.event() {
                tauri::webview::PageLoadEvent::Started => "page-started",
                tauri::webview::PageLoadEvent::Finished => "page-finished",
            });
        })
        .setup(|app| {
            startup_trace("setup");
            // This watchdog does not depend on JavaScript or the local engine.
            // Resolve its race with frontend_ready on the native main thread.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(20));
                if handle.state::<StartupState>().pending() {
                    let failed = handle.clone();
                    let _ = handle.run_on_main_thread(move || startup_failure(failed));
                }
            });
            if std::env::var_os("AGENTKIT_ENGINE_BIN").is_none() {
                let binary = if cfg!(windows) {
                    "agentkit.exe"
                } else {
                    "agentkit"
                };
                let engine = app.path().resource_dir()?.join("binaries").join(binary);
                if engine.is_file() {
                    std::env::set_var("AGENTKIT_ENGINE_BIN", engine);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            frontend_ready,
            engine_request,
            attachment_info,
            stage_clipboard_image,
            remove_staged_attachment
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Agentkit Studio");
}

#[cfg(test)]
mod startup_tests {
    use super::*;
    #[test]
    fn ready_cannot_be_replaced_by_timeout_or_duplicate_ready() {
        let state = StartupState::default();
        assert!(state.pending());
        assert!(state.finish(1));
        assert!(!state.pending());
        assert!(!state.finish(2));
        assert!(!state.finish(1));
    }
    #[test]
    fn timeout_prevents_late_frontend_from_opening_an_empty_window() {
        let state = StartupState::default();
        assert!(state.finish(2));
        assert!(!state.pending());
        assert!(!state.finish(1));
        assert!(!state.finish(2));
    }
}
