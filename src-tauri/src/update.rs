//! Обновление приложения одной кнопкой.
//!
//! Источник один и зашит в код: выпуски открытого репозитория. Оттуда
//! берём описание свежей версии, сам установщик и сверяем его с описанием
//! по размеру и SHA-256. Ставим только версию новее своей: откатить
//! человека на старую через этот путь нельзя.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const ИСТОЧНИК: &str = "https://github.com/CryAdmin/cryvpn-desktop/releases/latest/download/";
const УСТАНОВЩИК: &str = "CryVPN-Setup.exe";
/// Больше этого установщик весить не может; защита от того, чтобы нам
/// не скормили бесконечный файл.
const ПОТОЛОК: u64 = 200 * 1024 * 1024;

#[derive(Deserialize)]
struct Описание {
    version: String,
    sha256: String,
    size: u64,
}

fn клиент(секунд: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("CryVPN/", env!("CARGO_PKG_VERSION")))
        .no_proxy()
        .timeout(std::time::Duration::from_secs(секунд))
        .build()
        .map_err(|e| e.to_string())
}

/// «1.2.3» → [1, 2, 3]. Что-то иное — не версия.
fn разобрать(версия: &str) -> Option<Vec<u32>> {
    let части: Option<Vec<u32>> =
        версия.trim().trim_start_matches('v').split('.').map(|ч| ч.parse().ok()).collect();
    части.filter(|ч| ч.len() == 3)
}

fn новее(та: &str, эта: &str) -> bool {
    match (разобрать(та), разобрать(эта)) {
        (Some(а), Some(б)) => а > б,
        _ => false,
    }
}

async fn описание() -> Result<Описание, String> {
    let ответ = клиент(20)?
        .get(format!("{ИСТОЧНИК}version.json"))
        .send()
        .await
        .map_err(|_| "Нет связи с сервером обновлений".to_string())?;
    if !ответ.status().is_success() {
        return Err("Сервер обновлений не ответил".into());
    }
    ответ.json().await.map_err(|_| "Описание версии не читается".to_string())
}

/// Номер версии, если вышла новее нашей.
pub async fn проверить() -> Option<String> {
    let свежая = описание().await.ok()?;
    новее(&свежая.version, env!("CARGO_PKG_VERSION")).then_some(свежая.version)
}

/// Качаем установщик и сверяем с описанием. Отдаём путь к проверенному файлу.
pub async fn скачать() -> Result<PathBuf, String> {
    let свежая = описание().await?;
    if !новее(&свежая.version, env!("CARGO_PKG_VERSION")) {
        return Err("У вас уже последняя версия".into());
    }
    let сумма = свежая.sha256.trim().to_lowercase();
    if сумма.len() != 64 || !сумма.chars().all(|с| с.is_ascii_hexdigit()) || свежая.size > ПОТОЛОК {
        return Err("Описание версии повреждено".into());
    }

    let ответ = клиент(600)?
        .get(format!("{ИСТОЧНИК}{УСТАНОВЩИК}"))
        .send()
        .await
        .map_err(|_| "Не удалось скачать обновление".to_string())?;
    if !ответ.status().is_success() || ответ.content_length().is_some_and(|д| д > ПОТОЛОК) {
        return Err("Не удалось скачать обновление".into());
    }
    let байты = ответ.bytes().await.map_err(|_| "Загрузка оборвалась".to_string())?;

    let получилось: String =
        Sha256::digest(&байты).iter().map(|б| format!("{б:02x}")).collect();
    if байты.len() as u64 != свежая.size || получилось != сумма {
        // Между двумя запросами мог выйти ещё один выпуск — тогда повтор
        // пройдёт. А подменённый файл не пройдёт никогда.
        return Err("Файл обновления не прошёл проверку, попробуйте ещё раз".into());
    }

    let папка = std::env::temp_dir().join("CryVPN-update");
    std::fs::create_dir_all(&папка).map_err(|e| e.to_string())?;
    let путь = папка.join(УСТАНОВЩИК);
    std::fs::write(&путь, &байты).map_err(|e| e.to_string())?;
    Ok(путь)
}

/// Скачанный установщик после обновления больше не нужен — убираем.
pub fn прибрать() {
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join("CryVPN-update"));
    if let Some(папка) = crate::paths::папка_данных() {
        let _ = std::fs::remove_dir_all(папка.join("update"));
    }
}

/// Качаем файл выпуска и сверяем с контрольной суммой из того же выпуска.
/// Так обновляются Android и Linux: у каждого свой файл и своя сумма рядом.
#[cfg_attr(not(any(target_os = "android", target_os = "linux")), allow(dead_code))]
async fn скачать_с_суммой(имя: &str) -> Result<PathBuf, String> {
    let свежая = описание().await?;
    if !новее(&свежая.version, env!("CARGO_PKG_VERSION")) {
        return Err("У вас уже последняя версия".into());
    }

    // Файл суммы — одна строка вида «<64 знака>  <имя файла>».
    let запись = клиент(20)?
        .get(format!("{ИСТОЧНИК}{имя}.sha256"))
        .send()
        .await
        .map_err(|_| "Нет связи с сервером обновлений".to_string())?
        .text()
        .await
        .map_err(|_| "Контрольная сумма не читается".to_string())?;
    let сумма = запись.split_whitespace().next().unwrap_or("").to_lowercase();
    if сумма.len() != 64 || !сумма.chars().all(|с| с.is_ascii_hexdigit()) {
        return Err("Контрольная сумма обновления повреждена".into());
    }

    let ответ = клиент(600)?
        .get(format!("{ИСТОЧНИК}{имя}"))
        .send()
        .await
        .map_err(|_| "Не удалось скачать обновление".to_string())?;
    if !ответ.status().is_success() || ответ.content_length().is_some_and(|д| д > ПОТОЛОК) {
        return Err("Не удалось скачать обновление".into());
    }
    let байты = ответ.bytes().await.map_err(|_| "Загрузка оборвалась".to_string())?;
    let получилось: String =
        Sha256::digest(&байты).iter().map(|б| format!("{б:02x}")).collect();
    if получилось != сумма {
        return Err("Файл обновления не прошёл проверку, попробуйте ещё раз".into());
    }

    let папка = crate::paths::папка_данных()
        .ok_or_else(|| "не найдена папка данных приложения".to_string())?
        .join("update");
    std::fs::create_dir_all(&папка).map_err(|e| e.to_string())?;
    let путь = папка.join(имя);
    std::fs::write(&путь, &байты).map_err(|e| e.to_string())?;
    Ok(путь)
}

#[cfg_attr(not(target_os = "android"), allow(dead_code))]
const ПАКЕТ_ANDROID: &str = "CryVPN-Android.apk";

/// Android: качаем свежий APK и сверяем с контрольной суммой из того же
/// выпуска. Ставит его не приложение, а система — мы только отдаём ей
/// проверенный файл.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub async fn скачать_apk() -> Result<PathBuf, String> {
    скачать_с_суммой(ПАКЕТ_ANDROID).await
}

// ------------------------------------------------------------------ Linux

/// В каком виде приложение стоит на Linux — от этого зависит, как его
/// обновлять.
#[cfg(target_os = "linux")]
pub enum Упаковка {
    /// Один файл, запускается откуда угодно. Настоящий путь к файлу он
    /// сообщает сам: работает приложение из временной точки подключения.
    AppImage(PathBuf),
    /// Пакет .deb, поставленный в систему (Ubuntu, Debian, Mint).
    Deb,
    /// Пакет .rpm, поставленный в систему (Fedora, openSUSE и родня).
    Rpm,
}

#[cfg(target_os = "linux")]
pub fn упаковка() -> Option<Упаковка> {
    if let Some(файл) = std::env::var_os("APPIMAGE").filter(|п| !п.is_empty()) {
        return Some(Упаковка::AppImage(PathBuf::from(файл)));
    }
    let мы = std::env::current_exe().ok()?;
    if !мы.starts_with("/usr") {
        return None;
    }
    // Спрашиваем у самой системы, чей это файл: на одной машине могут
    // стоять обе программы для пакетов, а поставлены мы только одной.
    let чей = |программа: &str, довод: &str| {
        std::process::Command::new(программа)
            .arg(довод)
            .arg(&мы)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|итог| итог.success())
            .unwrap_or(false)
    };
    if чей("/usr/bin/dpkg", "-S") {
        Some(Упаковка::Deb)
    } else if чей("/usr/bin/rpm", "-qf") {
        Some(Упаковка::Rpm)
    } else {
        None
    }
}

/// Поставить скачанный пакет средствами системы: она спросит пароль.
#[cfg(target_os = "linux")]
fn поставить_пакет(программа: &str, доводы: &[&str], пакет: &PathBuf) -> Result<PathBuf, String> {
    // Свой путь запоминаем до установки: после неё система отвечает
    // «/usr/bin/cryvpn (deleted)» — прежний файл заменён, а мы всё ещё он.
    // С таким путём обновлённое приложение не запустилось бы.
    let мы = std::env::current_exe().map_err(|e| e.to_string())?;
    let итог = std::process::Command::new("pkexec")
        .arg(программа)
        .args(доводы)
        .arg(пакет)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| "Не найден pkexec — обновить пакет нечем".to_string())?;
    let _ = std::fs::remove_file(пакет);
    match итог.status.code() {
        Some(0) => Ok(мы),
        Some(126) | Some(127) => Err("Обновление отменено: нужен пароль администратора".into()),
        _ => Err("Система не смогла поставить обновление".into()),
    }
}

/// Linux: скачать, проверить и поставить поверх. Отдаём путь, по которому
/// запускать обновлённое приложение.
///
/// AppImage заменяем сами: это файл человека в его папке. Пакет .deb
/// ставит система — на это нужны права, и она спросит пароль своим окном.
#[cfg(target_os = "linux")]
pub async fn обновить_linux() -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    match упаковка().ok_or("Эта сборка не обновляется сама — скачайте новую версию с GitHub")? {
        Упаковка::AppImage(файл) => {
            let свежий = скачать_с_суммой("CryVPN-Linux.AppImage").await?;
            // Сначала кладём рядом, потом переименовываем: замена одним
            // движением, и оборванная запись не оставит человека без приложения.
            let рядом = файл.with_extension("AppImage.new");
            let беда = |e: std::io::Error| {
                format!("Не удалось заменить файл приложения ({}): {e}", файл.display())
            };
            std::fs::copy(&свежий, &рядом).map_err(беда)?;
            std::fs::set_permissions(&рядом, std::fs::Permissions::from_mode(0o755)).map_err(беда)?;
            std::fs::rename(&рядом, &файл).map_err(беда)?;
            let _ = std::fs::remove_file(свежий);
            Ok(файл)
        }
        Упаковка::Deb => {
            let пакет = скачать_с_суммой("CryVPN-Linux.deb").await?;
            поставить_пакет("/usr/bin/dpkg", &["-i"], &пакет)
        }
        Упаковка::Rpm => {
            let пакет = скачать_с_суммой("CryVPN-Linux.rpm").await?;
            поставить_пакет("/usr/bin/rpm", &["-U", "--force"], &пакет)
        }
    }
}

/// Запускаем установщик молча; после установки он сам откроет приложение.
#[cfg(windows)]
pub fn поставить(установщик: &PathBuf) -> Result<(), String> {
    std::process::Command::new(установщик)
        // /S — молча, /R — открыть приложение после установки,
        // /UPDATE — это обновление: вход и настройки не трогать.
        .args(["/S", "/R", "/UPDATE"])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Установщик не запустился: {e}"))
}

#[cfg(not(windows))]
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn поставить(_установщик: &PathBuf) -> Result<(), String> {
    Err("Обновление одной кнопкой есть только в Windows".into())
}

#[cfg(test)]
mod проверки {
    use super::*;

    #[test]
    fn сравнение_версий() {
        assert!(новее("0.1.2", "0.1.1"));
        assert!(новее("0.2.0", "0.1.9"));
        assert!(новее("0.1.10", "0.1.9"));
        assert!(новее("v1.0.0", "0.9.9"));
        assert!(!новее("0.1.1", "0.1.1"));
        assert!(!новее("0.1.0", "0.1.1"));
        assert!(!новее("мусор", "0.1.1"));
        assert!(!новее("1.2", "0.1.1"));
    }
}
