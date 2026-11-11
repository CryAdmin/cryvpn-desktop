//! Где приложение держит свои данные. У каждой системы своё место.

use std::path::PathBuf;

/// Папка данных приложения; создаётся при первом обращении.
///
/// Windows: `%LOCALAPPDATA%\CryVPN`
/// macOS:   `~/Library/Application Support/CryVPN`
pub fn папка_данных() -> Option<PathBuf> {
    let база = if cfg!(windows) {
        PathBuf::from(std::env::var("LOCALAPPDATA").ok()?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var("HOME").ok()?)
            .join("Library")
            .join("Application Support")
    } else {
        PathBuf::from(std::env::var("HOME").ok()?).join(".local").join("share")
    };
    let папка = база.join("CryVPN");
    std::fs::create_dir_all(&папка).ok()?;
    Some(папка)
}
