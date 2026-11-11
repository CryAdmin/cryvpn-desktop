//! Кто мы для панели: постоянный номер этого компьютера.
//!
//! Ремна привязывает подписку к устройствам и считает их по заголовку
//! `x-hwid`. Если номер будет новым при каждом запуске или переустановке,
//! человек за неделю съест все свои слоты и получит «лимит исчерпан» на
//! ровном месте. Поэтому берём номер, который живёт в системе: MachineGuid
//! из реестра Windows. Он ставится при установке системы и переживает
//! переустановку любых программ.
//!
//! Формат панель проверяет строго: 10–64 символа из латиницы, цифр,
//! дефиса и равно. MachineGuid — это 36 символов вида
//! `d0e8a1b2-3c4d-...`, то есть подходит как есть.

#[cfg(windows)]
pub fn номер() -> String {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const БЕЗ_ОКНА: u32 = 0x08000000; // CREATE_NO_WINDOW

    let вывод = Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .creation_flags(БЕЗ_ОКНА)
        .output();

    if let Ok(вывод) = вывод {
        let текст = String::from_utf8_lossy(&вывод.stdout);
        if let Some(строка) = текст.lines().find(|с| с.contains("MachineGuid")) {
            if let Some(значение) = строка.split_whitespace().last() {
                let очищено = пригодный(значение);
                if очищено.len() >= 10 {
                    return очищено;
                }
            }
        }
    }
    запасной()
}

/// macOS: аппаратный номер самой машины. Как и MachineGuid в Windows,
/// он переживает переустановку приложения и даже системы.
#[cfg(target_os = "macos")]
pub fn номер() -> String {
    let вывод = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output();
    if let Ok(вывод) = вывод {
        let текст = String::from_utf8_lossy(&вывод.stdout);
        if let Some(строка) = текст.lines().find(|с| с.contains("IOPlatformUUID")) {
            if let Some(значение) = строка.split('"').nth(3) {
                let очищено = пригодный(значение);
                if очищено.len() >= 10 {
                    return очищено;
                }
            }
        }
    }
    запасной()
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn номер() -> String {
    запасной()
}

/// Если реестр недоступен — собираем из имени машины и пользователя.
/// Хуже, чем MachineGuid, но постоянно в пределах одного компьютера.
fn запасной() -> String {
    let имя = имя_компьютера();
    let пользователь = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".into());

    // Свой простой хэш: тянуть ради этого целую библиотеку незачем.
    let мешанина = format!("{имя}:{пользователь}");
    let mut число: u64 = 0xcbf2_9ce4_8422_2325;
    for байт in мешанина.as_bytes() {
        число ^= *байт as u64;
        число = число.wrapping_mul(0x1000_0000_01b3);
    }
    format!("cryvpn-{число:016x}")
}

pub fn имя_компьютера() -> String {
    // У программ с окном на macOS переменной с именем машины нет —
    // спрашиваем у системы напрямую.
    #[cfg(target_os = "macos")]
    {
        if let Ok(вывод) = std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
        {
            let имя = String::from_utf8_lossy(&вывод.stdout).trim().to_string();
            if !имя.is_empty() {
                return имя;
            }
        }
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "PC".into())
}

/// Оставляем только то, что панель готова принять.
fn пригодный(значение: &str) -> String {
    значение
        .chars()
        .filter(|с| с.is_ascii_alphanumeric() || *с == '-' || *с == '=')
        .take(64)
        .collect()
}

/// Что показывать в списке устройств кабинета.
pub fn описание() -> (String, String, String) {
    #[cfg(target_os = "macos")]
    {
        let версия = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|в| String::from_utf8_lossy(&в.stdout).trim().to_string())
            .filter(|в| !в.is_empty())
            .unwrap_or_else(|| "unknown".into());
        return ("macOS".to_string(), версия, имя_компьютера());
    }
    #[cfg(not(target_os = "macos"))]
    {
        let ос = if cfg!(windows) { "Windows" } else { "Desktop" };
        let версия = std::env::var("OS").unwrap_or_else(|_| "unknown".into());
        (ос.to_string(), версия, имя_компьютера())
    }
}
