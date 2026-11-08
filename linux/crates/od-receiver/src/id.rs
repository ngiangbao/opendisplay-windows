//! Stable per-install identity (§2.1, §6.1): the same UUID in the Bonjour TXT
//! record and in `hello.id`, persisted under `$XDG_STATE_HOME/opendisplay/id`.

use std::path::PathBuf;

use anyhow::{Context, Result};

fn state_dir() -> PathBuf {
    // Windows: HOME and XDG_* are normally unset, so use the per-user app data
    // folder (the old /tmp fallback pointed at a root-level 	mp that
    // ordinary users cannot create).
    #[cfg(windows)]
    if let Some(d) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(d).join("OpenDisplay");
    }
    if let Some(p) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(p).join("opendisplay");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join(".local/state/opendisplay")
}

/// Load the install id, creating and persisting one on first run.
pub fn load_or_create() -> Result<String> {
    let dir = state_dir();
    let path = dir.join("id");
    if let Ok(s) = std::fs::read_to_string(&path) {
        let s = s.trim();
        if uuid::Uuid::parse_str(s).is_ok() {
            return Ok(s.to_uppercase());
        }
    }
    let id = uuid::Uuid::new_v4().to_string().to_uppercase();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, format!("{id}\n"))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(id)
}
