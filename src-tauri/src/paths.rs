//! Где приложение держит свои данные. У каждой системы своё место.

use std::path::PathBuf;

/// Папка данных приложения; создаётся при первом обращении.
///
/// Windows: `%LOCALAPPDATA%\CryVPN`
/// macOS:   `~/Library/Application Support/CryVPN`
pub fn папка_данных() -> Option<PathBuf> {
    // Android: место выдаёт система, своё у каждого приложения, и другим
    // программам туда хода нет.
    #[cfg(target_os = "android")]
    {
        let папка = ANDROID.get()?.join("CryVPN");
        std::fs::create_dir_all(&папка).ok()?;
        return Some(папка);
    }
    #[cfg(not(target_os = "android"))]
    папка_настольная()
}

#[cfg(target_os = "android")]
static ANDROID: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Android сообщает папку приложения при запуске.
#[cfg(target_os = "android")]
pub fn задать(папка: PathBuf) {
    let _ = ANDROID.set(папка);
}

#[cfg(not(target_os = "android"))]
fn папка_настольная() -> Option<PathBuf> {
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
