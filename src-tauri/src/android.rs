//! Мост к Android: туннель от системной службы VPN, сведения об
//! устройстве и шифрование ключом из защищённого хранилища.
//!
//! Сама служба и запрос согласия написаны на Kotlin
//! (`gen/android/app/src/main/java/top/cryvpn/app`). Отсюда мы только
//! зовём их и получаем ответ.

use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Manager, Wry};

static МОСТ: OnceLock<PluginHandle<Wry>> = OnceLock::new();
static СВЕДЕНИЯ: OnceLock<Сведения> = OnceLock::new();

#[derive(Deserialize, Clone, Default)]
pub struct Сведения {
    /// Номер устройства, который Android выдаёт приложению. Переживает
    /// переустановку, пока приложение подписано тем же ключом.
    pub id: String,
    pub model: String,
    pub release: String,
    /// Папка, из которой Android разрешает запускать ядро.
    pub libs: String,
}

#[derive(Deserialize)]
struct Текст {
    text: String,
}

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("cryvpn")
        .setup(|приложение, api| {
            // Папка данных нужна сразу: в ней сессия входа и настройки.
            if let Ok(папка) = приложение.path().app_data_dir() {
                crate::paths::задать(папка);
            }
            let мост = api.register_android_plugin("top.cryvpn.app", "VpnPlugin")?;
            let _ = МОСТ.set(мост);
            Ok(())
        })
        .build()
}

fn мост() -> Result<&'static PluginHandle<Wry>, String> {
    МОСТ.get().ok_or_else(|| "мост к Android ещё не поднят".to_string())
}

pub fn сведения() -> Сведения {
    if let Some(готовые) = СВЕДЕНИЯ.get() {
        return готовые.clone();
    }
    match мост().and_then(|м| м.run_mobile_plugin::<Сведения>("info", ()).map_err(|e| e.to_string())) {
        Ok(свежие) => {
            let _ = СВЕДЕНИЯ.set(свежие.clone());
            свежие
        }
        Err(_) => Сведения::default(),
    }
}

/// Где лежит ядро: Android запускает программы только из папки библиотек.
pub fn ядро() -> PathBuf {
    PathBuf::from(сведения().libs).join("libxray.so")
}

/// Базы geoip/geosite распаковываем из APK в обычные файлы.
pub async fn подготовить_базы(куда: &Path) -> Result<(), String> {
    мост()?
        .run_mobile_plugin_async::<serde_json::Value>("assets", json!({ "dir": куда }))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Просим службу VPN поднять туннель и запустить ядро. В первый раз
/// система покажет человеку окна согласия — этот вызов их дожидается.
/// Сама служба работает в отдельном процессе; поднялась ли она, видно по
/// порту учёта ядра и файлам в папке данных (см. core.rs).
pub async fn поднять(папка: &Path, ядро: &Path, базы: &Path, сервер: &str) -> Result<(), String> {
    // Приложения, которые человек пустил мимо VPN: Android исключает их из
    // туннеля сам, по имени пакета.
    let мимо: Vec<String> =
        crate::settings::прочитать().мимо_vpn.into_iter().map(|п| п.путь).collect();
    мост()?
        .run_mobile_plugin_async::<serde_json::Value>(
            "start",
            json!({ "dir": папка, "core": ядро, "assets": базы, "bypass": мимо, "title": сервер }),
        )
        .await
        .map(|_| ())
        .map_err(|e| match e {
            tauri::plugin::mobile::PluginInvokeError::InvokeRejected(отказ) => {
                отказ.message.unwrap_or_else(|| "Android не дал поднять туннель".into())
            }
            другое => другое.to_string(),
        })
}

/// Гасим службу и ждём, пока Android примет команду.
///
/// Ждать обязательно: при переключении сервера сразу за этим идёт новый
/// запуск. Когда остановка уходила «вдогонку», она иногда приходила
/// службе уже после запуска и гасила только что поднятый туннель.
pub fn опустить() {
    if let Ok(м) = мост() {
        let _ = м.run_mobile_plugin::<serde_json::Value>("stop", ());
    }
}

/// Отдаём системе проверенный APK: она покажет окно установки поверх.
pub async fn установить(пакет: &Path) -> Result<(), String> {
    мост()?
        .run_mobile_plugin_async::<serde_json::Value>("install", json!({ "path": пакет }))
        .await
        .map(|_| ())
        .map_err(|e| match e {
            tauri::plugin::mobile::PluginInvokeError::InvokeRejected(отказ) => {
                отказ.message.unwrap_or_else(|| "Android не открыл установку".into())
            }
            другое => другое.to_string(),
        })
}

#[derive(Deserialize)]
struct Приложения {
    apps: Vec<Приложение>,
}

#[derive(Deserialize)]
struct Приложение {
    name: String,
    id: String,
}

/// Установленные приложения со значком запуска — для выбора «мимо VPN».
pub fn приложения() -> Vec<crate::apps::Программа> {
    мост()
        .ok()
        .and_then(|м| м.run_mobile_plugin::<Приложения>("apps", ()).ok())
        .map(|ответ| {
            ответ
                .apps
                .into_iter()
                .map(|п| crate::apps::Программа { имя: п.name, путь: п.id })
                .collect()
        })
        .unwrap_or_default()
}

pub fn запечатать(открытое: &str) -> Option<String> {
    мост()
        .ok()?
        .run_mobile_plugin::<Текст>("seal", json!({ "text": открытое }))
        .ok()
        .map(|т| т.text)
}

pub fn распечатать(закрытое: &str) -> Option<String> {
    мост()
        .ok()?
        .run_mobile_plugin::<Текст>("unseal", json!({ "text": закрытое }))
        .ok()
        .map(|т| т.text)
}
