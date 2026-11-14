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
//! Linux:   шифруем AES-256-GCM, а ключ держим в связке ключей рабочего
//!          стола (GNOME Keyring, KWallet и другие с тем же интерфейсом).
//!          Связка заперта — не сохраняем. Связки в системе нет вовсе —
//!          ключ кладём файлом рядом с данными, закрытым от других
//!          пользователей: это слабее (ключ и замок лежат вместе), но
//!          иначе на таких системах входить пришлось бы при каждом запуске.

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
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn только_base64(текст: &str) -> bool {
    !текст.is_empty()
        // Потолок — от случайного мусора, а не от больших записей: список
        // серверов в зашифрованном виде занимает сотни килобайт.
        && текст.len() < 4_000_000
        && текст.bytes().all(|б| б.is_ascii_alphanumeric() || б == b'+' || б == b'/' || б == b'=')
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
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

#[cfg(not(any(windows, target_os = "macos", target_os = "android", target_os = "linux")))]
fn закрыть(_имя: &str, _открытое: &str) -> Option<String> {
    None
}

#[cfg(not(any(windows, target_os = "macos", target_os = "android", target_os = "linux")))]
fn открыть(_имя: &str, _закрытое: &str) -> Option<String> {
    None
}

// ------------------------------------------------------------------ Linux

// В связке лежит один ключ на всё приложение, а сами записи — в файлах,
// зашифрованные им. Так сделано из-за размера: список серверов занимает
// сотни килобайт, связки на такое не рассчитаны.

/// Ключ шифрования из связки ключей; при первом обращении создаётся.
/// Помним его до конца работы: каждое обращение к связке — это разговор
/// с другой программой, а записей мы читаем несколько подряд.
#[cfg(target_os = "linux")]
fn ключ() -> Option<[u8; 32]> {
    use aes_gcm::aead::{KeyInit, OsRng};
    static КЛЮЧ: std::sync::Mutex<Option<[u8; 32]>> = std::sync::Mutex::new(None);
    let mut помним = КЛЮЧ.lock().ok()?;
    if let Some(ключ) = *помним {
        return Some(ключ);
    }
    // Ключ файлом заведён раньше — значит, связки на этой машине нет,
    // и всё сохранённое зашифровано им. Держимся его и дальше.
    let ключ: [u8; 32] = if let Some(из_файла) = ключ_из_файла() {
        из_файла
    } else {
        let из_связки = keyring::Entry::new("top.cryvpn.app", "cryvpn").and_then(|запись| {
            match запись.get_password() {
                Ok(текст) => Ok(Some(текст)),
                Err(keyring::Error::NoEntry) => {
                    let новый: [u8; 32] = aes_gcm::Aes256Gcm::generate_key(OsRng).into();
                    запись.set_password(&в_base64(&новый)).map(|_| Some(в_base64(&новый)))
                }
                Err(беда) => Err(беда),
            }
        });
        match из_связки {
            Ok(Some(текст)) => crate::api::base64_в_байты(текст.trim())?.try_into().ok()?,
            // Связка есть, но заперта: новый ключ не заводим — он затёр бы
            // прежний, и всё сохранённое стало бы нечитаемым.
            _ if связка_есть() => return None,
            // Связки нет вовсе — ключ файлом.
            _ => завести_ключ_файлом()?,
        }
    };
    *помним = Some(ключ);
    Some(ключ)
}

#[cfg(target_os = "linux")]
fn файл_ключа() -> Option<std::path::PathBuf> {
    Some(crate::paths::папка_данных()?.join("key.bin"))
}

#[cfg(target_os = "linux")]
fn ключ_из_файла() -> Option<[u8; 32]> {
    std::fs::read(файл_ключа()?).ok()?.try_into().ok()
}

/// Ключ файлом, закрытым от других пользователей машины.
#[cfg(target_os = "linux")]
fn завести_ключ_файлом() -> Option<[u8; 32]> {
    use aes_gcm::aead::{KeyInit, OsRng};
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let новый: [u8; 32] = aes_gcm::Aes256Gcm::generate_key(OsRng).into();
    // Права задаём при создании, а не после: между двумя шагами файл
    // успел бы побыть открытым для всех.
    let mut файл = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(файл_ключа()?)
        .ok()?;
    файл.write_all(&новый).ok()?;
    Some(новый)
}

/// Есть ли в системе связка ключей: запущена или умеет запускаться по
/// первому обращению. Не смогли спросить — считаем, что нет: без шины
/// сообщений рабочего стола связке взяться неоткуда.
#[cfg(target_os = "linux")]
fn связка_есть() -> bool {
    let спросить = |способ: &str, довод: Option<&str>| {
        let mut команда = std::process::Command::new("gdbus");
        команда.args([
            "call", "--session",
            "--dest", "org.freedesktop.DBus",
            "--object-path", "/org/freedesktop/DBus",
            "--method", способ,
        ]);
        if let Some(довод) = довод {
            команда.arg(довод);
        }
        команда
            .stdin(std::process::Stdio::null())
            .output()
            .map(|в| String::from_utf8_lossy(&в.stdout).to_string())
            .unwrap_or_default()
    };
    спросить("org.freedesktop.DBus.NameHasOwner", Some("org.freedesktop.secrets")).contains("true")
        || спросить("org.freedesktop.DBus.ListActivatableNames", None).contains("org.freedesktop.secrets")
}

#[cfg(target_os = "linux")]
fn закрыть(имя: &str, открытое: &str) -> Option<String> {
    use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
    let шифр = aes_gcm::Aes256Gcm::new_from_slice(&ключ()?).ok()?;
    let затравка = aes_gcm::Aes256Gcm::generate_nonce(&mut OsRng);
    // Имя записи входит в проверку целостности: файл сессии, подложенный
    // на место файла подписки, не расшифруется.
    let закрыто = шифр
        .encrypt(&затравка, Payload { msg: открытое.as_bytes(), aad: имя.as_bytes() })
        .ok()?;
    let mut всё = затравка.to_vec();
    всё.extend_from_slice(&закрыто);
    Some(format!("aesgcm:v1:{}", в_base64(&всё)))
}

#[cfg(target_os = "linux")]
fn открыть(имя: &str, закрытое: &str) -> Option<String> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let тело = закрытое.strip_prefix("aesgcm:v1:")?;
    if !только_base64(тело) {
        return None;
    }
    let всё = crate::api::base64_в_байты(тело)?;
    if всё.len() < 12 + 16 {
        return None;
    }
    let (затравка, закрыто) = всё.split_at(12);
    let шифр = aes_gcm::Aes256Gcm::new_from_slice(&ключ()?).ok()?;
    let открыто = шифр
        .decrypt(aes_gcm::Nonce::from_slice(затравка), Payload { msg: закрыто, aad: имя.as_bytes() })
        .ok()?;
    String::from_utf8(открыто).ok()
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
