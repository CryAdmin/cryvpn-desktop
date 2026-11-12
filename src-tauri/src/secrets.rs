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
//! Android: шифруем ключом из защищённого хранилища устройства; ключ из
//!          него не извлекается, файл без телефона не расшифровать.

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

// Секрет в PowerShell передаём через стандартный ввод и только в base64.
//
// Раньше значение подставлялось прямо в текст команды. Это было плохо
// дважды. Открытая сессия оказывалась в командной строке процесса, которую
// видят журналы аудита и защитные программы. А файл `.dat` может подменить
// любая программа пользователя — и подставленный туда текст с кавычками
// исполнился бы как команда в нашем процессе с правами администратора.
// Теперь в команду не попадает ничего, кроме постоянного текста, а
// прочитанное из файла сначала проверяется: только знаки base64.

#[cfg(windows)]
fn закрыть(_имя: &str, открытое: &str) -> Option<String> {
    let итог = крипто(
        "$б=[Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()); \
         $з=[Security.Cryptography.ProtectedData]::Protect($б,$null,'CurrentUser'); \
         [Convert]::ToBase64String($з)",
        &в_base64(открытое.as_bytes()),
    )?;
    только_base64(&итог).then_some(итог)
}

#[cfg(windows)]
fn открыть(_имя: &str, закрытое: &str) -> Option<String> {
    if !только_base64(закрытое) {
        return None;
    }
    let итог = крипто(
        "$з=[Convert]::FromBase64String([Console]::In.ReadToEnd().Trim()); \
         $д=[Security.Cryptography.ProtectedData]::Unprotect($з,$null,'CurrentUser'); \
         [Convert]::ToBase64String($д)",
        закрытое,
    )?;
    String::from_utf8(crate::api::base64_в_байты(&итог)?).ok()
}

#[cfg(windows)]
fn крипто(что: &str, ввод: &str) -> Option<String> {
    use std::io::Write;
    use std::os::windows::process::CommandExt;
    use std::process::Stdio;
    let mut дитя = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("Add-Type -AssemblyName System.Security; {что}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(0x08000000)
        .spawn()
        .ok()?;
    дитя.stdin.take()?.write_all(ввод.as_bytes()).ok()?;
    let вывод = дитя.wait_with_output().ok()?;
    if !вывод.status.success() {
        return None;
    }
    let строка = String::from_utf8_lossy(&вывод.stdout).trim().to_string();
    (!строка.is_empty()).then_some(строка)
}

/// Только знаки base64 — ничего, что PowerShell мог бы принять за код.
#[cfg_attr(not(windows), allow(dead_code))]
fn только_base64(текст: &str) -> bool {
    !текст.is_empty()
        && текст.len() < 16_384
        && текст.bytes().all(|б| б.is_ascii_alphanumeric() || б == b'+' || б == b'/' || б == b'=')
}

#[cfg_attr(not(windows), allow(dead_code))]
fn в_base64(байты: &[u8]) -> String {
    const АЛФАВИТ: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut итог = String::with_capacity(байты.len().div_ceil(3) * 4);
    for тройка in байты.chunks(3) {
        let число = (тройка[0] as u32) << 16
            | (*тройка.get(1).unwrap_or(&0) as u32) << 8
            | *тройка.get(2).unwrap_or(&0) as u32;
        итог.push(АЛФАВИТ[(число >> 18) as usize & 63] as char);
        итог.push(АЛФАВИТ[(число >> 12) as usize & 63] as char);
        итог.push(if тройка.len() > 1 { АЛФАВИТ[(число >> 6) as usize & 63] as char } else { '=' });
        итог.push(if тройка.len() > 2 { АЛФАВИТ[число as usize & 63] as char } else { '=' });
    }
    итог
}

#[cfg(test)]
mod проверки {
    use super::*;

    #[test]
    fn base64_туда_и_обратно() {
        for текст in ["", "a", "ab", "abc", "https://sub.cryvps.com/код-с-русским", "refresh=1.2.3/+=="] {
            let закодировано = в_base64(текст.as_bytes());
            if !текст.is_empty() {
                assert!(только_base64(&закодировано), "{закодировано}");
            }
            assert_eq!(crate::api::base64_в_байты(&закодировано).unwrap(), текст.as_bytes());
        }
    }

    /// Настоящее шифрование Windows: что положили, то и достали, а
    /// подменённый файл не превращается в команду.
    #[cfg(windows)]
    #[test]
    fn dpapi_туда_и_обратно() {
        for секрет in ["refresh.token-123_ABC", "https://sub.cryvps.com/AbC123", "кириллица и 'кавычки' \"тоже\""] {
            let закрыто = закрыть("проверка", секрет).expect("не зашифровалось");
            assert!(только_base64(&закрыто));
            assert_ne!(закрыто, секрет);
            assert_eq!(открыть("проверка", &закрыто).as_deref(), Some(секрет));
        }
        assert_eq!(открыть("проверка", "x');Start-Process calc;('"), None);
        assert_eq!(открыть("проверка", "AAAA"), None);
    }

    #[test]
    fn подставленный_код_не_проходит() {
        assert!(!только_base64("x');Start-Process calc;('"));
        assert!(!только_base64("x\u{2019});Start-Process calc;(\u{2018}"));
        assert!(!только_base64("abc def"));
        assert!(!только_base64(""));
        assert!(только_base64("AAECAwQF+/8="));
    }
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

#[cfg(all(not(windows), not(target_os = "macos"), not(target_os = "android")))]
fn закрыть(_имя: &str, _открытое: &str) -> Option<String> {
    None
}

#[cfg(all(not(windows), not(target_os = "macos"), not(target_os = "android")))]
fn открыть(_имя: &str, _закрытое: &str) -> Option<String> {
    None
}

// ---------------------------------------------------------------- Android

#[cfg(target_os = "android")]
fn закрыть(_имя: &str, открытое: &str) -> Option<String> {
    crate::android::запечатать(открытое)
}

#[cfg(target_os = "android")]
fn открыть(_имя: &str, закрытое: &str) -> Option<String> {
    crate::android::распечатать(закрытое)
}
