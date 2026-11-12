#[cfg(target_os = "android")]
mod android;
mod api;
mod apps;
mod core;
mod device;
mod link;
mod net;
mod paths;
mod secrets;
mod settings;
mod update;

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

/// Токен входа из ссылки `cryvpn://login/…`, ещё не забранный окном.
pub struct ОжидающийВход(std::sync::Mutex<Option<String>>);

/// Под этим именем подписка лежит в защищённом хранилище.
const ПОДПИСКА: &str = "sub";

/// Есть ли значок возле часов. Прятать окно по крестику можно только
/// когда он есть: иначе человек остался бы без способа открыть приложение.
#[cfg(desktop)]
static ЗНАЧОК_ЕСТЬ: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Достаём окно на передний план — в том числе спрятанное к часам.
fn показать_окно(приложение: &tauri::AppHandle) {
    if let Some(окно) = приложение.get_webview_window("main") {
        #[cfg(desktop)]
        {
            let _ = окно.show();
            let _ = окно.unminimize();
        }
        let _ = окно.set_focus();
    }
}

/// Гасим туннель перед выходом приложения и ждём конца уборки.
#[cfg(desktop)]
fn погасить_туннель(приложение: &tauri::AppHandle) {
    if let Some(связь) = приложение.try_state::<Связь>() {
        let был = tauri::async_runtime::block_on(async {
            let mut туннель = связь.0.lock().await;
            let был = туннель.включён();
            туннель.выключить_до_конца();
            был
        });
        core::забыть_сервер();
        if был {
            core::записать_событие("выключен: приложение закрывается");
        }
    }
}

/// Значок возле часов: по нему приложение открывают и из него же выходят.
/// Не получилось создать — не страшно, просто крестик будет закрывать
/// приложение, как раньше.
#[cfg(desktop)]
fn завести_значок(приложение: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let открыть = MenuItem::with_id(приложение, "open", "Открыть CryVPN", true, None::<&str>)?;
    let выход = MenuItem::with_id(приложение, "quit", "Выйти и отключить VPN", true, None::<&str>)?;
    let меню = Menu::with_items(приложение, &[&открыть, &выход])?;

    let mut значок = TrayIconBuilder::with_id("main")
        .tooltip("CryVPN")
        .menu(&меню)
        .show_menu_on_left_click(false)
        .on_menu_event(|приложение, событие| match событие.id.as_ref() {
            "open" => показать_окно(приложение),
            "quit" => {
                // Выход — это и отключение: оставлять туннель без приложения нельзя.
                погасить_туннель(приложение);
                приложение.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|значок, событие| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = событие
            {
                показать_окно(значок.app_handle());
            }
        });
    if let Some(картинка) = приложение.default_window_icon() {
        значок = значок.icon(картинка.clone());
    }
    значок.build(приложение)?;
    ЗНАЧОК_ЕСТЬ.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Приняли ссылку: проверяем, запоминаем, будим окно.
fn принять_ссылку(приложение: &tauri::AppHandle, сырая: &str) {
    // Ссылка входа: токен отдаём окну, оно само спросит кабинет.
    if let Some(токен) = link::разобрать_вход(сырая) {
        if let Some(ждёт) = приложение.try_state::<ОжидающийВход>() {
            *ждёт.0.lock().unwrap() = Some(токен);
        }
        let _ = приложение.emit("ссылка", ());
        показать_окно(приложение);
        return;
    }
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
    показать_окно(приложение);
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

/// Окно забирает токен входа, пришедший по ссылке. Отдаётся один раз.
#[tauri::command]
fn вход_по_ссылке(ждёт: State<'_, ОжидающийВход>) -> Option<String> {
    ждёт.0.lock().unwrap().take()
}

#[tauri::command]
fn забыть_подписку() {
    api::забыть_память();
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

/// Вход через Telegram: ссылка на бота.
#[tauri::command]
async fn телеграм_начать(кабинет: State<'_, Кабинет>) -> Result<api::НачалоТелеграма, String> {
    Ok(кабинет.телеграм_начать().await)
}

/// Вход через Telegram: нажал ли человек «Войти» в боте.
#[tauri::command]
async fn телеграм_опрос(
    кабинет: State<'_, Кабинет>,
    токен: String,
) -> Result<api::ОпросТелеграма, String> {
    Ok(кабинет.телеграм_опрос(токен.trim()).await)
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
async fn восстановить(кабинет: State<'_, Кабинет>) -> Result<&'static str, String> {
    Ok(match кабинет.вернуться().await {
        api::Возврат::Вошли => "вошли",
        api::Возврат::Нет => "нет",
        api::Возврат::НетСвязи => "нет_связи",
    })
}

/// Профиль с прошлого удачного раза — когда вход жив, а до кабинета не
/// достучаться. Без сохранённой сессии не отдаём ничего.
#[tauri::command]
fn профиль_из_памяти() -> Option<Value> {
    api::профиль_из_памяти()
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
async fn пинг(
    кабинет: State<'_, Кабинет>,
    связь: State<'_, Связь>,
) -> Result<Vec<Option<u32>>, String> {
    // При поднятом туннеле меряем мимо него, с обычного адреса компьютера.
    // Без туннеля привязка не нужна: путь и так прямой.
    let мимо = if связь.0.lock().await.включён() {
        tokio::task::spawn_blocking(net::обычный_адрес).await.ok().flatten()
    } else {
        None
    };
    Ok(кабинет.пинг(мимо).await)
}

/// Номер новой версии, если она вышла.
#[tauri::command]
async fn обновление() -> Result<Option<String>, String> {
    // Windows обновляется сама, одной кнопкой. Android о новой версии
    // только сообщает и отдаёт человеку свежий APK — ставит его система.
    // На Mac приложение не распространяется, проверять нечего.
    if !cfg!(any(windows, target_os = "android")) {
        return Ok(None);
    }
    Ok(update::проверить().await)
}

/// Скачать новую версию, проверить и поставить поверх. Приложение при
/// этом закрывается, установщик откроет его снова уже обновлённым.
#[tauri::command]
async fn обновиться(
    приложение: tauri::AppHandle,
    связь: State<'_, Связь>,
) -> Result<(), String> {
    // Android: приложение только скачивает и проверяет APK, а ставит его
    // система — она покажет человеку своё окно установки. Туннель не
    // трогаем: передумает — VPN останется включён.
    #[cfg(target_os = "android")]
    {
        let _ = (&приложение, &связь);
        let пакет = update::скачать_apk().await?;
        return android::установить(&пакет).await;
    }
    #[cfg(not(target_os = "android"))]
    {
        let установщик = update::скачать().await?;
        // Туннель гасим только когда файл уже скачан и проверен: если
        // загрузка не удалась, человек остаётся с работающим VPN.
        связь.0.lock().await.выключить_до_конца();
        update::поставить(&установщик)?;
        приложение.exit(0);
        Ok(())
    }
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
    // На компьютере подключение обязано уложиться в разумный срок: вечное
    // «Подключаемся…» без объяснения хуже честной ошибки. На Android срок
    // не ставим — там человек может долго читать системные вопросы, а
    // сама служба ограничена отдельно.
    #[cfg(not(target_os = "android"))]
    {
        let срок = std::time::Duration::from_secs(45);
        match tokio::time::timeout(срок, туннель.включить(&конфиг)).await {
            Ok(итог) => итог?,
            Err(_) => {
                туннель.выключить();
                return Err(
                    "Подключение не уложилось в 45 секунд. Попробуйте ещё раз или другой сервер"
                        .into(),
                );
            }
        }
    }
    #[cfg(target_os = "android")]
    туннель.включить(&конфиг).await?;

    core::запомнить_сервер(
        номер,
        конфиг.get("remarks").and_then(|и| и.as_str()).unwrap_or(""),
    );
    core::записать_событие(&format!("включён: сервер №{номер}"));
    Ok(true)
}

/// `причина` — кто и почему отключает: кнопка, чужой VPN, смена аккаунта.
/// Уходит в дневник туннеля.
#[tauri::command]
async fn отключить(связь: State<'_, Связь>, причина: Option<String>) -> Result<bool, String> {
    связь.0.lock().await.выключить();
    core::забыть_сервер();
    core::записать_событие(&format!(
        "выключен: {}",
        причина.as_deref().unwrap_or("причина не названа")
    ));
    Ok(false)
}

/// Окно заметило, что туннель пропал сам, — записываем и это.
#[tauri::command]
fn отметить_обрыв(причина: Option<String>) {
    core::записать_событие(&format!(
        "оборвался сам: {}",
        причина.as_deref().unwrap_or("причина неизвестна")
    ));
}

/// Сколько секунд туннель уже поднят. Ноль — если он выключен.
#[tauri::command]
async fn идёт_секунд(связь: State<'_, Связь>) -> Result<u64, String> {
    if !связь.0.lock().await.включён() {
        return Ok(0);
    }
    Ok(core::идёт_секунд().unwrap_or(0))
}

/// Через какой сервер идёт трафик прямо сейчас: номер в списке, если
/// туннель поднят и под этим номером всё ещё тот же сервер.
#[tauri::command]
async fn текущий_сервер(
    кабинет: State<'_, Кабинет>,
    связь: State<'_, Связь>,
) -> Result<Option<usize>, String> {
    if !связь.0.lock().await.включён() {
        return Ok(None);
    }
    let Some((номер, имя)) = core::текущий_сервер() else {
        return Ok(None);
    };
    let подписка = кабинет.подписка.lock().await;
    let тот_же = подписка
        .get(номер)
        .and_then(|к| к.get("remarks"))
        .and_then(|и| и.as_str())
        .is_some_and(|и| и == имя);
    Ok(тот_же.then_some(номер))
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

/// Запущенные программы с окнами — чтобы выбрать, кого пустить мимо VPN.
#[tauri::command]
async fn запущенные_программы() -> Result<Vec<apps::Программа>, String> {
    tokio::task::spawn_blocking(apps::запущенные).await.map_err(|e| e.to_string())
}

/// Окно выбора файла программы. Отдаёт путь или ничего, если передумали.
#[cfg(desktop)]
#[tauri::command]
async fn выбрать_программу(приложение: tauri::AppHandle) -> Result<Option<apps::Программа>, String> {
    use tauri_plugin_dialog::DialogExt;
    let выбор = tokio::task::spawn_blocking(move || {
        приложение
            .dialog()
            .file()
            .set_title("Программа, которая пойдёт мимо VPN")
            .add_filter("Программы", &["exe"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(выбор.and_then(|файл| файл.into_path().ok()).map(|путь| {
        let путь = путь.display().to_string();
        apps::Программа { имя: apps::имя_из_пути(&путь), путь }
    }))
}

#[cfg(not(desktop))]
#[tauri::command]
async fn выбрать_программу() -> Result<Option<apps::Программа>, String> {
    Ok(None)
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
    core::забыть_сервер();
    Ok(кабинет.выйти().await)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let кабинет = Кабинет::новый().expect("не удалось поднять http-клиент");

    let сборщик = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let сборщик = сборщик.plugin(android::init());
    #[cfg(desktop)]
    let сборщик = сборщик
        // Вторая копия приложения не нужна: она передаёт первой то, с чем
        // её запустили (ссылку cryvpn://), и закрывается. Этот модуль
        // обязан стоять первым.
        .plugin(tauri_plugin_single_instance::init(|приложение, _доводы, _папка| {
            показать_окно(приложение);
        }))
        .plugin(tauri_plugin_dialog::init());
    сборщик
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(кабинет)
        .manage(Ожидающая(std::sync::Mutex::new(None)))
        .manage(ОжидающийВход(std::sync::Mutex::new(None)))
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

            update::прибрать();

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

            #[cfg(desktop)]
            if let Err(беда) = завести_значок(приложение.handle()) {
                eprintln!("[CryVPN] значок возле часов не создан: {беда}");
            }

            // Ядро, драйвер и базы geoip лежат в установке приложения.
            #[cfg(target_os = "android")]
            let ресурсы = paths::папка_данных()
                .map(|п| п.join("assets"))
                .unwrap_or_else(|| std::path::PathBuf::from("assets"));
            #[cfg(not(target_os = "android"))]
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
            телеграм_начать,
            телеграм_опрос,
            восстановить,
            профиль,
            серверы,
            пинг,
            подключить,
            отключить,
            отметить_обрыв,
            подключено,
            текущий_сервер,
            идёт_секунд,
            профиль_из_памяти,
            счётчики,
            запущенные_программы,
            выбрать_программу,
            настройки,
            сохранить_настройки,
            открыть_ссылку,
            причина_остановки,
            чужой_vpn,
            подписка_по_ссылке,
            забыть_подписку,
            вход_по_ссылке,
            обновление,
            обновиться,
            выйти
        ])
        // Крестик не закрывает приложение, а прячет окно к часам: VPN при
        // этом продолжает работать. Так ведут себя все VPN-клиенты, и люди
        // этого ждут — иначе «закрыл окно» значило «остался без защиты».
        .on_window_event(|_окно, _событие| {
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = _событие {
                let прятать = ЗНАЧОК_ЕСТЬ.load(std::sync::atomic::Ordering::Relaxed)
                    && settings::прочитать().в_трее;
                if прятать {
                    api.prevent_close();
                    let _ = _окно.hide();
                } else {
                    // Окно закрывают насовсем. Ядро — отдельный процесс и
                    // само вместе с приложением не уйдёт: без этого после
                    // выхода оставались бы поднятый туннель и ни одного
                    // окна, чтобы его выключить.
                    погасить_туннель(_окно.app_handle());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|_приложение, _событие| {
            // Последний рубеж: какой бы дорогой приложение ни выходило,
            // туннель на компьютере без него остаться не должен. На Android
            // наоборот — служба VPN намеренно переживает окно.
            #[cfg(desktop)]
            if let tauri::RunEvent::Exit = _событие {
                погасить_туннель(_приложение);
            }
        });
}
