//! Настройки приложения: что помнить между запусками.
//!
//! Лежат простым файлом рядом с данными приложения. Секретов здесь нет —
//! только то, как человек хочет, чтобы программа себя вела.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Default)]
// Поля, которых нет в старом файле настроек, берутся по умолчанию. Без
// этого первое же новое поле обнуляло человеку все прежние настройки.
#[serde(default)]
pub struct Настройки {
    /// Запускать вместе с Windows. Делается заданием планировщика, а не
    /// записью в автозагрузку: приложению нужны права администратора, а
    /// обычная автозагрузка с ними показывает окно согласия при каждом
    /// входе в систему — или просто молча не запускается.
    pub с_виндой: bool,
    pub сразу_подключаться: bool,
    pub свёрнутым: bool,
    pub в_трей: bool,
    /// Последний выбранный сервер — чтобы не искать его заново.
    pub последний_сервер: Option<usize>,
    /// Программы, чей трафик идёт мимо VPN. На компьютере `путь` — это
    /// полный путь к файлу программы, на Android — имя её пакета.
    pub мимо_vpn: Vec<crate::apps::Программа>,
}

fn путь() -> Option<PathBuf> {
    Some(crate::paths::папка_данных()?.join("settings.json"))
}

pub fn прочитать() -> Настройки {
    let Some(п) = путь() else {
        return Настройки::default();
    };
    std::fs::read_to_string(п)
        .ok()
        .and_then(|т| serde_json::from_str(&т).ok())
        .unwrap_or_default()
}

pub fn записать(новые: &Настройки) -> Result<(), String> {
    // Автозапуск — это не запись в файле, а состояние системы.
    // Приводим его в соответствие здесь же, чтобы галка и реальность
    // не разъехались.
    автозапуск(новые.с_виндой)?;

    let п = путь().ok_or("не найдена папка настроек")?;
    let текст = serde_json::to_string_pretty(новые).map_err(|e| e.to_string())?;
    std::fs::write(п, текст).map_err(|e| e.to_string())
}

/// Задание планировщика с повышенными правами: только так приложение,
/// которому нужны права администратора, стартует при входе без окна
/// согласия.
#[cfg(windows)]
fn автозапуск(включить: bool) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const БЕЗ_ОКНА: u32 = 0x08000000;
    // Имя задания только латиницей и вызов без посредника cmd: с русским
    // именем командная строка Windows коверкала его кодировкой, и галка
    // в настройках кончалась ошибкой.
    const ИМЯ: &str = "CryVPNAutostart";

    let mut команда = std::process::Command::new("schtasks");
    if включить {
        let путь = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .display()
            .to_string();
        команда.args([
            "/create",
            "/tn",
            ИМЯ,
            "/tr",
            &format!("\"{путь}\""),
            "/sc",
            "onlogon",
            "/rl",
            "HIGHEST",
            "/f",
        ]);
    } else {
        команда.args(["/delete", "/tn", ИМЯ, "/f"]);
    }

    let вывод = команда
        .creation_flags(БЕЗ_ОКНА)
        .output()
        .map_err(|e| format!("не удалось изменить автозапуск: {e}"))?;

    // Удаление несуществующего задания — не ошибка: человек мог выключить
    // галку, которая и так была выключена.
    if вывод.status.success() || !включить {
        Ok(())
    } else {
        Err("Windows не дала настроить автозапуск. Запустите приложение от имени администратора".into())
    }
}

/// macOS: агент запуска в папке пользователя. Система сама поднимает
/// приложение при входе; прав администратора для этого не нужно.
#[cfg(target_os = "macos")]
fn автозапуск(включить: bool) -> Result<(), String> {
    let дом = std::env::var("HOME").map_err(|_| "не найдена домашняя папка".to_string())?;
    let папка = PathBuf::from(дом).join("Library").join("LaunchAgents");
    let файл = папка.join("top.cryvpn.app.plist");

    if !включить {
        let _ = std::fs::remove_file(файл);
        return Ok(());
    }

    let путь = std::env::current_exe().map_err(|e| e.to_string())?;
    let текст = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>top.cryvpn.app</string>
  <key>ProgramArguments</key><array><string>{}</string></array>
  <key>RunAtLoad</key><true/>
</dict>
</plist>
"#,
        путь.display()
    );
    std::fs::create_dir_all(&папка).map_err(|e| e.to_string())?;
    std::fs::write(файл, текст).map_err(|e| format!("не удалось настроить автозапуск: {e}"))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn автозапуск(_включить: bool) -> Result<(), String> {
    Ok(())
}
