//! Хранилище секретов: сессия входа и ссылка на подписку.
//!
//! Обе вещи открывают доступ к аккаунту человека, поэтому открытым текстом
//! на диск не ложатся никогда. Не удалось защитить — не сохраняем вовсе:
//! «чтобы работало» здесь не довод.
//!
//! Windows: шифруем DPAPI под учётную запись, храним в файле `<имя>.dat`.
//!          Файл, скопированный на другую машину или открытый другим
//!          пользователем, не расшифруется.
//! macOS:   кладём в Связку ключей; в файле только пометка, где искать.

fn файл(имя: &str) -> Option<std::path::PathBuf> {
    Some(crate::paths::папка_данных()?.join(format!("{имя}.dat")))
}

pub fn сохранить(имя: &str, значение: &str) -> bool {
    let Some(путь) = файл(имя) else { return false };
    match закрыть(имя, значение) {
        Some(закрыто) => std::fs::write(путь, закрыто).is_ok(),
        None => {
            let _ = std::fs::remove_file(путь);
            false
        }
    }
}

pub fn прочитать(имя: &str) -> Option<String> {
    let запись = std::fs::read_to_string(файл(имя)?).ok()?;
    let запись = запись.trim();
    if запись.is_empty() {
        return None;
    }
    открыть(имя, запись).filter(|з| !з.is_empty())
}

pub fn забыть(имя: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("security")
            .args(["delete-generic-password", "-a", "cryvpn", "-s", &служба(имя)])
            .output();
    }
    if let Some(путь) = файл(имя) {
        let _ = std::fs::remove_file(путь);
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn закрыть(_имя: &str, открытое: &str) -> Option<String> {
    крипто(&format!(
        "$д=[Text.Encoding]::UTF8.GetBytes('{}'); \
         $з=[Security.Cryptography.ProtectedData]::Protect($д,$null,'CurrentUser'); \
         [Convert]::ToBase64String($з)",
        открытое.replace('\'', "''")
    ))
}

#[cfg(windows)]
fn открыть(_имя: &str, закрытое: &str) -> Option<String> {
    крипто(&format!(
        "$з=[Convert]::FromBase64String('{}'); \
         $д=[Security.Cryptography.ProtectedData]::Unprotect($з,$null,'CurrentUser'); \
         [Text.Encoding]::UTF8.GetString($д)",
        закрытое.replace('\'', "''")
    ))
}

#[cfg(windows)]
fn крипто(что: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let вывод = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Add-Type -AssemblyName System.Security; {что}"),
        ])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    if !вывод.status.success() {
        return None;
    }
    let строка = String::from_utf8_lossy(&вывод.stdout).trim().to_string();
    (!строка.is_empty()).then_some(строка)
}

// ------------------------------------------------------------------ macOS

#[cfg(target_os = "macos")]
fn служба(имя: &str) -> String {
    format!("top.cryvpn.app.{имя}")
}

#[cfg(target_os = "macos")]
fn закрыть(имя: &str, открытое: &str) -> Option<String> {
    let вывод = std::process::Command::new("security")
        .args(["add-generic-password", "-U", "-a", "cryvpn", "-s", &служба(имя), "-w", открытое])
        .output()
        .ok()?;
    вывод.status.success().then(|| "keychain:v1".to_string())
}

#[cfg(target_os = "macos")]
fn открыть(имя: &str, закрытое: &str) -> Option<String> {
    if !закрытое.starts_with("keychain:") {
        return None;
    }
    let вывод = std::process::Command::new("security")
        .args(["find-generic-password", "-a", "cryvpn", "-s", &служба(имя), "-w"])
        .output()
        .ok()?;
    if !вывод.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&вывод.stdout).trim().to_string())
}

// ----------------------------------------------------------------- прочее

#[cfg(all(not(windows), not(target_os = "macos")))]
fn закрыть(_имя: &str, _открытое: &str) -> Option<String> {
    None
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn открыть(_имя: &str, _закрытое: &str) -> Option<String> {
    None
}
