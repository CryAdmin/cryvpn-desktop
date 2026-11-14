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

/// Android: номер, который система выдаёт приложению. Он один на
/// устройство и ключ подписи приложения, переустановку переживает.
#[cfg(target_os = "android")]
pub fn номер() -> String {
    let очищено = пригодный(&crate::android::сведения().id);
    if очищено.len() >= 10 {
        return format!("android-{очищено}").chars().take(64).collect();
    }
    запасной()
}

/// Linux: номер машины из `/etc/machine-id`. Сам номер наружу не отдаём —
/// система просит этого не делать, он общий для всех программ. Отдаём
/// его отпечаток, свой у нашего приложения.
#[cfg(target_os = "linux")]
pub fn номер() -> String {
    use sha2::{Digest, Sha256};
    let машина = std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .unwrap_or_default();
    let машина = машина.trim();
    if машина.len() >= 10 {
        let отпечаток = Sha256::digest(format!("top.cryvpn.app:{машина}").as_bytes());
        let текст: String = отпечаток.iter().take(16).map(|б| format!("{б:02x}")).collect();
        return format!("linux-{текст}");
    }
    запасной()
}

#[cfg(not(any(windows, target_os = "macos", target_os = "android", target_os = "linux")))]
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
    #[cfg(target_os = "linux")]
    {
        if let Ok(имя) = std::fs::read_to_string("/etc/hostname") {
            let имя = имя.trim().to_string();
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

/// Приводим текст к латинице для заголовков запроса.
///
/// Заголовки HTTP — это латиница. Имя компьютера с русскими буквами
/// («MacBook Air — Андрей») панель читала побайтно и записывала
/// кракозябрами. Поэтому кириллицу пишем латиницей, типографские тире и
/// кавычки заменяем простыми, а всё прочее нелатинское отбрасываем.
pub fn латиницей(текст: &str) -> String {
    let mut итог = String::with_capacity(текст.len());
    for знак in текст.chars() {
        let замена: &str = match знак {
            'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' | 'э' => "e",
            'ё' => "yo", 'ж' => "zh", 'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k",
            'л' => "l", 'м' => "m", 'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r",
            'с' => "s", 'т' => "t", 'у' => "u", 'ф' => "f", 'х' => "kh", 'ц' => "ts",
            'ч' => "ch", 'ш' => "sh", 'щ' => "sch", 'ъ' | 'ь' => "", 'ы' => "y",
            'ю' => "yu", 'я' => "ya",
            'А' => "A", 'Б' => "B", 'В' => "V", 'Г' => "G", 'Д' => "D", 'Е' | 'Э' => "E",
            'Ё' => "Yo", 'Ж' => "Zh", 'З' => "Z", 'И' => "I", 'Й' => "Y", 'К' => "K",
            'Л' => "L", 'М' => "M", 'Н' => "N", 'О' => "O", 'П' => "P", 'Р' => "R",
            'С' => "S", 'Т' => "T", 'У' => "U", 'Ф' => "F", 'Х' => "Kh", 'Ц' => "Ts",
            'Ч' => "Ch", 'Ш' => "Sh", 'Щ' => "Sch", 'Ъ' | 'Ь' => "", 'Ы' => "Y",
            'Ю' => "Yu", 'Я' => "Ya",
            '—' | '–' | '‑' => "-",
            '«' | '»' | '“' | '”' | '„' => "\"",
            '’' | '‘' => "'",
            '\u{a0}' => " ",
            прочий if прочий.is_ascii_graphic() || прочий == ' ' => {
                итог.push(прочий);
                continue;
            }
            _ => continue,
        };
        итог.push_str(замена);
    }
    let итог = итог.split_whitespace().collect::<Vec<_>>().join(" ");
    if итог.is_empty() { "unknown".to_string() } else { итог }
}

/// Что показывать в списке устройств кабинета. Всё латиницей: эти строки
/// уходят в заголовках запроса.
pub fn описание() -> (String, String, String) {
    let (ос, версия, модель) = описание_как_есть();
    (латиницей(&ос), латиницей(&версия), латиницей(&модель))
}

fn описание_как_есть() -> (String, String, String) {
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
    #[cfg(target_os = "android")]
    {
        let с = crate::android::сведения();
        return ("Android".to_string(), с.release, с.model);
    }
    #[cfg(target_os = "linux")]
    {
        // «Ubuntu 24.04.1 LTS», «Fedora Linux 41» — как система зовёт себя сама.
        let версия = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|текст| {
                текст.lines().find_map(|строка| {
                    строка.strip_prefix("PRETTY_NAME=").map(|з| з.trim().trim_matches('"').to_string())
                })
            })
            .filter(|в| !в.is_empty())
            .unwrap_or_else(|| "unknown".into());
        return ("Linux".to_string(), версия, имя_компьютера());
    }
    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "linux")))]
    {
        let ос = if cfg!(windows) { "Windows" } else { "Desktop" };
        let версия = std::env::var("OS").unwrap_or_else(|_| "unknown".into());
        (ос.to_string(), версия, имя_компьютера())
    }
}

#[cfg(test)]
mod проверки {
    use super::латиницей;

    #[test]
    fn имя_устройства_латиницей() {
        assert_eq!(латиницей("MacBook Air\u{a0}— Андрей"), "MacBook Air - Andrey");
        assert_eq!(латиницей("ПК Жени"), "PK Zheni");
        assert_eq!(латиницей("vivo I2407"), "vivo I2407");
        assert_eq!(латиницей("华为"), "unknown");
        assert!(латиницей("Ноутбук «Щука» №1").is_ascii());
    }
}
