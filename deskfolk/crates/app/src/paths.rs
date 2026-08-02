//! Where things live on disk.
//!
//! Bundled builds get `characters/` from the app's resource directory. A
//! `cargo tauri dev` run has no resource directory, so we fall back to the
//! workspace copy — that way the same binary works in both without an env var.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Directory holding installed character packages.
pub fn characters_dir(app: &AppHandle) -> PathBuf {
    if let Ok(res) = app.path().resource_dir() {
        let bundled = res.join("characters");
        if bundled.is_dir() {
            return bundled;
        }
    }
    workspace_characters()
}

/// The in-repo `characters/` directory, resolved from this crate's location.
fn workspace_characters() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../characters")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../characters"))
}

/// Per-companion save data (memory, mood, position).
pub fn companion_data_dir(app: &AppHandle, id: &str) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("deskfolk"))
        .join("companions")
        .join(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_characters_points_at_the_repo_copy() {
        let p = workspace_characters();
        assert!(
            p.join("yasser").join("character.json").exists(),
            "expected the Yasser package at {}",
            p.display()
        );
    }
}
