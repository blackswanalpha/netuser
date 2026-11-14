#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod netinfo;
mod scan;
mod store;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::Manager;

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map_err(|e| e.to_string())
}

/// True for a colon-separated MAC like `aa:bb:cc:dd:ee:ff`. Guards the
/// clipboard tools against arbitrary argument strings from the frontend.
fn is_mac(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    parts.len() == 6 && parts.iter().all(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Puts text on the system clipboard (Wayland wl-copy, else X11 xclip).
fn copy_clipboard(text: &str) -> Result<(), String> {
    // Only use wl-copy under Wayland; with no display it fails and we fall back.
    // wl-copy forks its own server and exits, so `status()` reaps it.
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland
        && Command::new("wl-copy")
            .arg(text)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    {
        return Ok(());
    }
    if let Ok(mut child) = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        let wrote = child
            .stdin
            .take()
            .map(|mut stdin| stdin.write_all(text.as_bytes()))
            .transpose();
        // Always reap the child, even when the write failed.
        let status = child.wait();
        wrote.map_err(|e| e.to_string())?;
        if status.is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    Err("No clipboard tool found (install wl-clipboard or xclip)".into())
}

/// Copies the device MAC and opens the router admin page in the default browser,
/// so the user can finish blocking it there. Does not block anything itself.
/// Runs off the main thread: it shells out and waits on the clipboard tool.
#[tauri::command(async)]
fn block_helper(mac: String) -> Result<String, String> {
    if !is_mac(&mac) {
        return Err("Invalid MAC address".into());
    }
    let (_, gateway) = scan::default_iface()?;
    let gateway = gateway.ok_or("No router found on this network")?;
    let url = format!("http://{gateway}");
    copy_clipboard(&mac)?;
    let mut child = Command::new("xdg-open")
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Couldn't open the router page: {e}"))?;
    // xdg-open can stay attached to the browser; reap it off-thread so it
    // never lingers as a zombie.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(url)
}

#[tauri::command]
async fn scan(app: tauri::AppHandle) -> Result<scan::ScanResult, String> {
    let dir = config_dir(&app)?;
    // Names load inside the scan, after probing, so a rename made during the
    // ~5s probe isn't overwritten by a stale copy.
    tauri::async_runtime::spawn_blocking(move || scan::run(|| store::load(&dir)))
        .await
        .map_err(|e| e.to_string())?
}

/// WiFi signal, link rate and live throughput for the active interface.
#[tauri::command]
async fn net_stats() -> Result<netinfo::NetStats, String> {
    tauri::async_runtime::spawn_blocking(netinfo::run)
        .await
        .map_err(|e| e.to_string())?
}

/// Names a device by its MAC (empty name clears it). Returns the full name map.
#[tauri::command]
fn set_name(app: tauri::AppHandle, mac: String, name: String) -> Result<store::Names, String> {
    store::set(&config_dir(&app)?, &mac, &name)
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![scan, net_stats, set_name, block_helper])
        // Enforce "no maximize" regardless of WM support: if the window ever
        // becomes maximized (double-click titlebar, Super+Up), snap it back.
        .on_window_event(|w, e| {
            if matches!(e, tauri::WindowEvent::Resized(_)) && w.is_maximized().unwrap_or(false) {
                let _ = w.unmaximize();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running NetUser");
}

#[cfg(test)]
mod tests {
    #[test]
    fn mac_validation() {
        assert!(super::is_mac("c0:7e:40:be:97:96"));
        assert!(super::is_mac("C0:7E:40:BE:97:96"));
        assert!(!super::is_mac("--clear"));
        assert!(!super::is_mac("c0:7e:40:be:97"));
        assert!(!super::is_mac("c0:7e:40:be:97:9g"));
        assert!(!super::is_mac("c0:7e:40:be:97:961"));
    }
}
