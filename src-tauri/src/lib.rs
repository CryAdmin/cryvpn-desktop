mod api;
mod core;
mod device;
mod link;
mod paths;
mod secrets;
mod settings;

use api::{Кабинет, Ответ, Подписка};
use core::Туннель;
use serde_json::Value;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;
use tokio::sync::Mutex;

pub struct Связь(Arc<Mutex<Туннель>>);

/// Подписка, пришедшая по ссылке `cryvpn://` и ещё не забранная окном.
pub struct Ожидающая(std::sync::Mutex<Option<String>>);

/// Под этим именем подписка лежит в защищённом хранилище.
const ПОДПИСКА: &str = "sub";

/// Приняли ссылку: проверяем, запоминаем, будим окно.
fn принять_ссылку(приложение: &tauri::AppHandle, сырая: &str) {
    let Some(адрес) = link::разобрать(сырая) else {
        // Что именно прислали, в журнал не пишем: там может быть чужая
        // подписка. Достаточно знать, что ссылку отвергли.
        eprintln!("[CryVPN] ссылка отвергнута: не наш адрес или неверный вид");
        let _ = приложение.emit("ссылка-отвергнута", ());
        return;
    };
    if let Some(ждёт) = приложение.try_state::<Ожидающая>() {
        *ждёт.0.lock().unwrap() = Some(адрес);
    }
    let _ = приложение.emit("ссылка", ());
    if let Some(окно) = приложение.get_webview_window("main") {
        let _ = окно.unminimize();
        let _ = окно.set_focus();
    }
}

#[derive(serde::Serialize)]
pub struct ПоСсылке {
    адрес: String,
    /// Только что пришла по нажатию кнопки, а не поднята из хранилища.
    свежая: bool,
}

/// Окно спрашивает, есть ли подписка без входа в кабинет.
/// Свежая ссылка важнее всего остального: человек только что нажал кнопку.
#[tauri::command]
fn подписка_по_ссылке(ждёт: State<'_, Ожидающая>) -> Option<ПоСсылке> {
    if let Some(адрес) = ждёт.0.lock().unwrap().take() {
        secrets::сохранить(ПОДПИСКА, &адрес);
        return Some(ПоСсылке { адрес, свежая: true });
    }
    // Сохранённую проверяем заново: хранилище могло быть подменено.
    secrets::прочитать(ПОДПИСКА)
        .and_then(|сохранённая| link::разобрать(&format!("cryvpn://add/{сохранённая}")))
        .map(|адрес| ПоСсылке { адрес, свежая: false })
}

#[tauri::command]
fn забыть_подписку() {
    secrets::забыть(ПОДПИСКА);
}

/// Шлём код на почту.
#[tauri::command]
async fn выслать_код(кабинет: State<'_, Кабинет>, почта: String) -> Result<Ответ, String> {
    let почта = почта.trim().to_lowercase();
    if !почта.contains('@') || почта.len() < 5 {
        return Ok(Ответ { ok: false, сообщение: "Похоже, в адресе ошибка".into() });
    }
    Ok(кабинет.выслать_код(&почта).await)
}

/// Меняем код из письма на вход.
#[tauri::command]
async fn проверить_код(
    кабинет: State<'_, Кабинет>,
    почта: String,
    код: String,
) -> Result<Ответ, String> {
    let почта = почта.trim().to_lowercase();
    let код = код.trim().to_string();
    if код.is_empty() {
        return Ok(Ответ { ok: false, сообщение: "Введите код из письма".into() });
    }
    Ok(кабинет.проверить_код(&почта, &код).await)
}

/// Подписка, серверы, устройства.
#[tauri::command]
async fn профиль(кабинет: State<'_, Кабинет>) -> Result<Value, String> {
    кабинет.профиль().await
}

/// Возвращаемся в прошлую сессию, если она ещё жива.
/// Вызывается при открытии окна: человек не должен вводить код с почты
/// каждый раз, как на сайте его тоже не спрашивают.
#[tauri::command]
async fn восстановить(кабинет: State<'_, Кабинет>) -> Result<bool, String> {
    Ok(кабинет.восстановить().await)
}

/// Список серверов из подписки Ремны.
#[tauri::command]
async fn серверы(кабинет: State<'_, Кабинет>, ссылка: String) -> Result<Подписка, String> {
    if ссылка.trim().is_empty() {
        return Err("У аккаунта нет ссылки на подписку".into());
    }
    кабинет.серверы(ссылка.trim()).await
}

/// Задержка до каждого сервера, в миллисекундах; пусто — не дозвонились.
#[tauri::command]
async fn пинг(кабинет: State<'_, Кабинет>) -> Result<Vec<Option<u32>>, String> {
    Ok(кабинет.пинг().await)
}

/// Поднять туннель на выбранном сервере.
#[tauri::command]
async fn подключить(
    кабинет: State<'_, Кабинет>,
    связь: State<'_, Связь>,
    номер: usize,
) -> Result<bool, String> {
    let конфиг = {
        let подписка = кабинет.подписка.lock().await;
        подписка
            .get(номер)
            .cloned()
            .ok_or_else(|| "Сервер не найден, обновите список".to_string())?
    };

    let mut туннель = связь.0.lock().await;
    туннель.включить(&конфиг).await?;
    Ok(true)
}

#[tauri::command]
async fn отключить(связь: State<'_, Связь>) -> Result<bool, String> {
    связь.0.lock().await.выключить();
    Ok(false)
}

/// Включён ли туннель прямо сейчас. Окно спрашивает об этом при открытии:
/// приложение могли свернуть и открыть заново.
#[tauri::command]
async fn подключено(связь: State<'_, Связь>) -> Result<bool, String> {
    Ok(связь.0.lock().await.включён())
}

/// Сколько прошло через туннель. Окно спрашивает раз в секунду и само
/// считает разницу — получается скорость.
/// Открыть ссылку в обычном браузере. Нужна для объявления владельца
/// и для страницы оплаты: внутри окна приложения им не место.
#[tauri::command]
fn открыть_ссылку(приложение: tauri::AppHandle, адрес: String) -> Result<(), String> {
    if !адрес.starts_with("https://") {
        return Err("Ссылка не похожа на безопасную".into());
    }
    tauri_plugin_opener::OpenerExt::opener(&приложение)
        .open_url(адрес, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Название другого включённого VPN, если такой есть.
/// Окно спрашивает об этом, пока наш туннель поднят: появился чужой —
/// наш гасим, чтобы они не мешали друг другу.
#[tauri::command]
async fn чужой_vpn() -> Result<Option<String>, String> {
    // Проверка идёт через PowerShell и занимает до секунды — уводим её с
    // общего потока, чтобы не подвешивать остальные запросы окна.
    tokio::task::spawn_blocking(core::чужой_vpn)
        .await
        .map_err(|e| e.to_string())
}

/// Почему остановился туннель в прошлый раз — если это известно.
#[tauri::command]
fn причина_остановки() -> Option<String> {
    core::причина_остановки()
}

/// Настройки приложения.
#[tauri::command]
fn настройки() -> settings::Настройки {
    settings::прочитать()
}

#[tauri::command]
fn сохранить_настройки(новые: settings::Настройки) -> Result<(), String> {
    settings::записать(&новые)
}

#[tauri::command]
async fn счётчики(связь: State<'_, Связь>) -> Result<core::Счётчики, String> {
    Ok(связь.0.lock().await.счётчики())
}

#[tauri::command]
async fn выйти(кабинет: State<'_, Кабинет>, связь: State<'_, Связь>) -> Result<Ответ, String> {
    // Выход из профиля гасит и туннель: оставлять чужой трафик в своём
    // подключении нельзя.
    связь.0.lock().await.выключить();
    Ok(кабинет.выйти().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let кабинет = Кабинет::новый().expect("не удалось поднять http-клиент");

    tauri::Builder::default()
        // Вторая копия приложения не нужна: она передаёт первой то, с чем
        // её запустили (ссылку cryvpn://), и закрывается. Этот модуль
        // обязан стоять первым.
        .plugin(tauri_plugin_single_instance::init(|приложение, _доводы, _папка| {
            if let Some(окно) = приложение.get_webview_window("main") {
                let _ = окно.unminimize();
                let _ = окно.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(кабинет)
        .manage(Ожидающая(std::sync::Mutex::new(None)))
        .setup(|приложение| {
            // В прошлый раз приложение могли закрыть через диспетчер задач
            // прямо с поднятым туннелем. Тогда в системе остались наши
            // маршруты. Проверяем и чиним молча.
            //
            // Делаем это здесь, а не в самом начале запуска: сюда доходит
            // только первая копия приложения. Вторая, запущенная ссылкой,
            // иначе приняла бы работающий туннель первой за «остаток после
            // падения» и убила бы его.
            if core::вылечить_после_падения() {
                eprintln!("[CryVPN] сеть после прошлого сеанса приведена в порядок");
            }

            // Ссылки cryvpn://: и те, с которыми нас запустили, и те, что
            // придут, пока мы работаем.
            let ручка = приложение.handle().clone();
            приложение.deep_link().on_open_url(move |событие| {
                for адрес in событие.urls() {
                    принять_ссылку(&ручка, адрес.as_str());
                }
            });
            if let Ok(Some(адреса)) = приложение.deep_link().get_current() {
                for адрес in адреса {
                    принять_ссылку(приложение.handle(), адрес.as_str());
                }
            }

            // Ядро, драйвер и базы geoip лежат в установке приложения.
            let ресурсы = приложение
                .path()
                .resource_dir()
                .map(|п| п.join("resources"))
                .unwrap_or_else(|_| std::path::PathBuf::from("resources"));
            приложение.manage(Связь(Arc::new(Mutex::new(Туннель::новый(ресурсы)))));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            выслать_код,
            проверить_код,
            восстановить,
            профиль,
            серверы,
            пинг,
            подключить,
            отключить,
            подключено,
            счётчики,
            настройки,
            сохранить_настройки,
            открыть_ссылку,
            причина_остановки,
            чужой_vpn,
            подписка_по_ссылке,
            забыть_подписку,
            выйти
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
