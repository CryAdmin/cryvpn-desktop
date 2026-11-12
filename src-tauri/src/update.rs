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

const ПАКЕТ_ANDROID: &str = "CryVPN-Android.apk";

/// Android: качаем свежий APK и сверяем с контрольной суммой из того же
/// выпуска. Ставит его не приложение, а система — мы только отдаём ей
/// проверенный файл.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub async fn скачать_apk() -> Result<PathBuf, String> {
    let свежая = описание().await?;
    if !новее(&свежая.version, env!("CARGO_PKG_VERSION")) {
        return Err("У вас уже последняя версия".into());
    }

    // Файл суммы — одна строка вида «<64 знака>  CryVPN-Android.apk».
    let запись = клиент(20)?
        .get(format!("{ИСТОЧНИК}{ПАКЕТ_ANDROID}.sha256"))
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
        .get(format!("{ИСТОЧНИК}{ПАКЕТ_ANDROID}"))
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
    let путь = папка.join(ПАКЕТ_ANDROID);
    std::fs::write(&путь, &байты).map_err(|e| e.to_string())?;
    Ok(путь)
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
