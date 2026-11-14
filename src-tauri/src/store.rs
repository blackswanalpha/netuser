//! Persists user-chosen device names, keyed by MAC address, as a JSON file
//! in the app config directory. Names survive IP changes and restarts.

use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub type Names = HashMap<String, String>;

/// Serializes load-modify-save so concurrent renames can't drop each other.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

fn file(dir: &Path) -> PathBuf {
    dir.join("names.json")
}

/// Names for display. A missing or unreadable file shows no names.
pub fn load(dir: &Path) -> Names {
    read(dir).unwrap_or_default()
}

/// Strict read: a missing file is empty, a corrupt one is an error, so `set`
/// never overwrites every saved name with a single entry.
fn read(dir: &Path) -> Result<Names, String> {
    match fs::read_to_string(file(dir)) {
        Ok(s) => serde_json::from_str(&s)
            .map_err(|e| format!("{} is corrupt: {e}", file(dir).display())),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Names::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes to a temp file then renames it over the target, so readers (the
/// scan thread) never see a half-written file and a crash can't truncate it.
fn save(dir: &Path, names: &Names) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(names).map_err(|e| e.to_string())?;
    let tmp = dir.join("names.json.tmp");
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(json.as_bytes()).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&tmp, file(dir)).map_err(|e| e.to_string())
}

/// Sets a trimmed name for a MAC, or clears it when the name is empty.
pub fn set(dir: &Path, mac: &str, name: &str) -> Result<Names, String> {
    let mac = mac.trim().to_lowercase();
    if mac.is_empty() {
        return Err("Missing MAC address".into());
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut names = read(dir)?;
    let name = name.trim();
    if name.is_empty() {
        names.remove(&mac);
    } else {
        names.insert(mac, name.chars().take(40).collect());
    }
    save(dir, &names)?;
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("netuser-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn set_and_clear_roundtrip() {
        let d = tmpdir("roundtrip");
        set(&d, "AA:BB:CC:DD:EE:FF", "  Phone ").unwrap();
        assert_eq!(load(&d).get("aa:bb:cc:dd:ee:ff").map(String::as_str), Some("Phone"));
        set(&d, "aa:bb:cc:dd:ee:ff", "").unwrap();
        assert!(load(&d).is_empty());
        assert!(!d.join("names.json.tmp").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_file_is_not_overwritten() {
        let d = tmpdir("corrupt");
        fs::create_dir_all(&d).unwrap();
        fs::write(file(&d), "{not json").unwrap();
        assert!(set(&d, "aa:bb:cc:dd:ee:ff", "Phone").is_err());
        assert_eq!(fs::read_to_string(file(&d)).unwrap(), "{not json");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn concurrent_sets_keep_every_name() {
        let d = tmpdir("concurrent");
        std::thread::scope(|s| {
            for i in 0..16 {
                let d = &d;
                s.spawn(move || set(d, &format!("aa:bb:cc:dd:ee:{i:02x}"), "x").unwrap());
            }
        });
        assert_eq!(load(&d).len(), 16);
        let _ = fs::remove_dir_all(&d);
    }
}
