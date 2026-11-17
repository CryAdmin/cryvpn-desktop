//! Защита от утечки (kill switch) на Windows.
//!
//! Включённая в настройках, она ставит фильтр в сетевой экран самой
//! Windows (WFP): наружу можно только через туннель CryVPN. Если VPN
//! оборвался — ядро упало, сервер пропал, сеть моргнула, — программы не
//! уходят в интернет напрямую с настоящим адресом человека, а просто
//! ждут, пока VPN поднимется снова.
//!
//! Что пропускаем всегда:
//! - саму машину (127.0.0.1 и прочее «внутри»);
//! - ядро и само приложение — иначе VPN не смог бы переподключиться;
//! - программы из списка «мимо VPN»: человек сам решил, что им туннель не
//!   нужен, — они работают и при оборванном VPN;
//! - домашнюю сеть (роутер, принтер, DHCP) и частную сеть Tailscale.
//!
//! Всё остальное, что идёт не через туннель, — запрещено. В том числе DNS
//! роутеру: роутер — домашняя сеть, но вопрос он передаёт провайдеру.
//!
//! Фильтр живёт ровно столько, сколько приложение: он поставлен в «сеансе»,
//! который Windows сама убирает, когда процесс приложения закрывается или
//! падает. Человек никогда не останется без интернета из-за забытого
//! правила: закрыл CryVPN — интернет вернулся.
//!
//! Здесь же — заслон от утечек. Он стоит всегда, пока работает VPN, и от
//! настройки не зависит. 17.11.2026 тест на живой машине показал: при
//! включённом VPN часть DNS-запросов Windows отправляла мимо туннеля —
//! роутеру, а от него провайдеру. Windows спрашивает DNS во всех сетях
//! сразу и берёт первый ответ. Ещё IPv6: туннель CryVPN на Windows его не
//! несёт, и сайты могли видеть настоящий IPv6-адрес (через WebRTC тоже).
//! Заслон закрывает ровно это: DNS — только через туннель, IPv6 наружу —
//! никак. Остальной интернет он не трогает.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::NetworkManagement::WindowsFilteringPlatform::*;

/// Адрес туннеля CryVPN (см. core.rs, `gateway`). Всё, что уходит с него, —
/// уже внутри VPN.
const ТУННЕЛЬ: (u32, u32) = (0xC6_13_4D_00, 0xFF_FF_FF_FC); // 198.19.77.0/30

/// Сети, куда можно и без VPN: своя домашняя сеть, частная сеть
/// Tailscale, служебные адреса для DHCP и обнаружения устройств.
const СВОИ_СЕТИ_V4: [(u32, u32); 7] = [
    (0x0A_00_00_00, 0xFF_00_00_00), // 10.0.0.0/8
    (0xAC_10_00_00, 0xFF_F0_00_00), // 172.16.0.0/12
    (0xC0_A8_00_00, 0xFF_FF_00_00), // 192.168.0.0/16
    (0xA9_FE_00_00, 0xFF_FF_00_00), // 169.254.0.0/16
    (0x64_40_00_00, 0xFF_C0_00_00), // 100.64.0.0/10 — Tailscale
    (0xE0_00_00_00, 0xF0_00_00_00), // 224.0.0.0/4 — многоадресные
    (0xFF_FF_FF_FF, 0xFF_FF_FF_FF), // 255.255.255.255 — DHCP
];

/// То же для IPv6: местные адреса, частные (в том числе Tailscale) и
/// многоадресные. Остальной IPv6 при защите закрыт: туннель CryVPN на
/// Windows его не несёт, и мимо VPN он ушёл бы с настоящим адресом.
const СВОИ_СЕТИ_V6: [([u8; 16], u8); 3] = [
    ([0xFE, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 10), // fe80::/10
    ([0xFC, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 7),     // fc00::/7
    ([0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 8),     // ff00::/8
];

/// Частная сеть Tailscale: её DNS (100.100.100.100) отвечает за имена своих
/// машин, и закрывать его незачем — наружу он не ходит.
const TAILSCALE_V4: (u32, u32) = (0x64_40_00_00, 0xFF_C0_00_00); // 100.64.0.0/10
const TAILSCALE_V6: ([u8; 16], u8) = ([0xFD, 0x7A, 0x11, 0x5C, 0xA1, 0xE0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 48); // fd7a:115c:a1e0::/48

/// Порт DNS.
const ПОРТ_DNS: u16 = 53;

/// Свой подслой фильтров — чтобы наши правила решали между собой по весу и
/// не путались с чужими. У защиты и заслона подслои разные: они стоят в
/// разных сеансах и живут независимо. Запрет в любом из подслоёв
/// перевешивает разрешение в другом — так Windows сводит подслои.
const ПОДСЛОЙ: GUID = GUID::from_u128(0x7c1d_5b2e_6a4f_4e8d_9b3a_c0f1_e2d3_a4b5);
const ПОДСЛОЙ_ЗАСЛОНА: GUID = GUID::from_u128(0x3e8a_91c4_57b2_4d0f_a6e1_b9c2_d4f5_0718);

/// Какой набор правил ставим.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Вид {
    /// Защита от утечки: наружу — только через туннель.
    Защита,
    /// Заслон: DNS — только через туннель, IPv6 наружу закрыт.
    Заслон,
}

struct Сеанс(HANDLE);

// Ручка сеанса — просто число; Windows разрешает пользоваться ею из любого
// потока.
unsafe impl Send for Сеанс {}

impl Drop for Сеанс {
    fn drop(&mut self) {
        // Закрыли сеанс — Windows сама снимает все его фильтры.
        // SAFETY: ручка получена от FwpmEngineOpen0 и закрывается один раз.
        unsafe { FwpmEngineClose0(self.0) };
    }
}

static ДЕЙСТВУЕТ: Mutex<Option<Сеанс>> = Mutex::new(None);

/// Держит ли защита интернет закрытым прямо сейчас (стоит ли фильтр).
pub fn держит() -> bool {
    ДЕЙСТВУЕТ.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).is_some()
}

/// Снять защиту: интернет снова открыт и без VPN.
pub fn выключить() {
    if ДЕЙСТВУЕТ.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).take().is_some() {
        crate::core::записать_событие("защита от утечки снята");
    }
}

/// Поставить защиту (или обновить её: поменялся список «мимо VPN»).
/// `свои_программы` — ядро, само приложение и программы «мимо VPN».
pub fn включить(свои_программы: &[PathBuf]) -> Result<(), String> {
    // Новый фильтр ставим раньше, чем снимаем прежний: между ними не
    // должно быть ни мгновения без защиты.
    let новый = поставить(свои_программы, Вид::Защита)?;
    let прежний = ДЕЙСТВУЕТ.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).replace(новый);
    if прежний.is_none() {
        crate::core::записать_событие("защита от утечки включена");
    }
    drop(прежний);
    Ok(())
}

/// Заслон и номер процесса ядра, ради которого он поставлен.
static ЗАСЛОН: Mutex<Option<(Сеанс, u32)>> = Mutex::new(None);

/// Поставить заслон от утечек DNS и IPv6 на время работы ядра `ядро_pid`.
/// Когда этот процесс завершится — сам ли, по кнопке ли, — заслон снимается:
/// поток ниже ждёт его конца средствами Windows и процессор не тратит.
pub fn заслон_поставить(свои_программы: &[PathBuf], ядро_pid: u32) -> Result<(), String> {
    let новый = поставить(свои_программы, Вид::Заслон)?;
    let прежний = ЗАСЛОН.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).replace((новый, ядро_pid));
    drop(прежний);
    std::thread::spawn(move || {
        дождаться_процесса(ядро_pid);
        заслон_снять_за(ядро_pid);
    });
    Ok(())
}

/// Снять заслон: VPN выключен, DNS и IPv6 снова идут как обычно.
pub fn заслон_снять() {
    drop(ЗАСЛОН.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).take());
}

/// Стоит ли заслон прямо сейчас.
#[cfg(test)]
fn заслон_стоит() -> bool {
    ЗАСЛОН.lock().unwrap_or_else(|отравлен| отравлен.into_inner()).is_some()
}

/// Снять заслон, только если он ещё принадлежит этому ядру. Пока ждали,
/// могли переподключиться — заслон нового ядра трогать нельзя.
fn заслон_снять_за(ядро_pid: u32) {
    let mut заслон = ЗАСЛОН.lock().unwrap_or_else(|отравлен| отравлен.into_inner());
    if заслон.as_ref().is_some_and(|(_, чей)| *чей == ядро_pid) {
        drop(заслон.take());
    }
}

/// Ждать конца процесса. Поток спит в ядре Windows и просыпается ровно
/// один раз — когда процесс завершился.
fn дождаться_процесса(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE};
    // SAFETY: обычные вызовы с числами; ручку закрываем ниже.
    let ручка = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if ручка.is_null() {
        // Процесса уже нет.
        return;
    }
    // SAFETY: ручка открыта выше и закрывается один раз.
    unsafe {
        WaitForSingleObject(ручка, INFINITE);
        CloseHandle(ручка);
    }
}

fn ошибка(что: &str, код: u32) -> String {
    format!("Не удалось включить защиту от утечки ({что}, код 0x{код:08X}). Выключите её в настройках или запустите приложение от имени администратора")
}

fn широкая(текст: &str) -> Vec<u16> {
    текст.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Значение условия держит указатели на данные — они должны жить, пока
/// фильтр добавляется. Складываем их сюда.
#[derive(Default)]
struct Хранилище {
    v4: Vec<Box<FWP_V4_ADDR_AND_MASK>>,
    v6: Vec<Box<FWP_V6_ADDR_AND_MASK>>,
}

fn поставить(свои_программы: &[PathBuf], вид: Вид) -> Result<Сеанс, String> {
    let имя = широкая("CryVPN");
    // SAFETY: обнуление — допустимое начальное состояние описания сеанса.
    let mut описание: FWPM_SESSION0 = unsafe { std::mem::zeroed() };
    описание.flags = FWPM_SESSION_FLAG_DYNAMIC;
    описание.displayData.name = имя.as_ptr() as *mut u16;
    let mut ручка: HANDLE = std::ptr::null_mut();
    // SAFETY: все указатели — на живые переменные этой функции.
    let код = unsafe {
        FwpmEngineOpen0(
            std::ptr::null(),
            windows_sys::Win32::System::Rpc::RPC_C_AUTHN_WINNT,
            std::ptr::null(),
            &описание,
            &mut ручка,
        )
    };
    if код != 0 {
        return Err(ошибка("сетевой экран Windows не ответил", код));
    }
    let сеанс = Сеанс(ручка);

    // Всё одной сделкой: либо защита стоит целиком, либо её нет совсем.
    // SAFETY: ручка открыта выше.
    let код = unsafe { FwpmTransactionBegin0(ручка, 0) };
    if код != 0 {
        return Err(ошибка("сделка не началась", код));
    }
    if let Err(беда) = добавить_всё(ручка, свои_программы, вид) {
        // SAFETY: сделка открыта выше.
        unsafe { FwpmTransactionAbort0(ручка) };
        return Err(беда);
    }
    // SAFETY: сделка открыта выше.
    let код = unsafe { FwpmTransactionCommit0(ручка) };
    if код != 0 {
        return Err(ошибка("сделка не прошла", код));
    }
    Ok(сеанс)
}

/// Код программы для фильтра. Удалённую программу пропускаем молча: из-за
/// неё защита не должна отказываться работать.
struct Программа(*mut FWP_BYTE_BLOB);

impl Drop for Программа {
    fn drop(&mut self) {
        // SAFETY: память выделила сама система в FwpmGetAppIdFromFileName0.
        unsafe { FwpmFreeMemory0(&mut self.0 as *mut *mut FWP_BYTE_BLOB as *mut *mut core::ffi::c_void) };
    }
}

fn код_программы(путь: &Path) -> Option<Программа> {
    if !путь.exists() {
        return None;
    }
    let путь = широкая(&путь.to_string_lossy());
    let mut код: *mut FWP_BYTE_BLOB = std::ptr::null_mut();
    // SAFETY: строка заканчивается нулём, указатель на результат — наш.
    let итог = unsafe { FwpmGetAppIdFromFileName0(путь.as_ptr(), &mut код) };
    (итог == 0 && !код.is_null()).then_some(Программа(код))
}

fn добавить_всё(ручка: HANDLE, свои_программы: &[PathBuf], вид: Вид) -> Result<(), String> {
    let имя = широкая("CryVPN");
    // SAFETY: обнуление — допустимое начальное состояние подслоя.
    let mut подслой: FWPM_SUBLAYER0 = unsafe { std::mem::zeroed() };
    подслой.subLayerKey = подслой_для(вид);
    подслой.displayData.name = имя.as_ptr() as *mut u16;
    подслой.weight = 0xFFFF;
    // SAFETY: описание подслоя живо до конца вызова.
    let код = unsafe { FwpmSubLayerAdd0(ручка, &подслой, std::ptr::null_mut()) };
    if код != 0 {
        return Err(ошибка("подслой", код));
    }

    let программы: Vec<Программа> = свои_программы.iter().filter_map(|п| код_программы(п)).collect();

    // Внутри подслоя решает самый тяжёлый подошедший фильтр.
    for (слой, v6) in [
        (FWPM_LAYER_ALE_AUTH_CONNECT_V4, false),
        (FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, false),
        (FWPM_LAYER_ALE_AUTH_CONNECT_V6, true),
        (FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6, true),
    ] {
        let mut хранилище = Хранилище::default();

        // 15: сама машина — всегда.
        let петля = условие_флаги(FWP_CONDITION_FLAG_IS_LOOPBACK);
        фильтр(ручка, слой, вид, 15, FWP_ACTION_PERMIT, &[петля])?;

        // 14: ядро, приложение и программы «мимо VPN».
        for программа in &программы {
            let условие = условие_программа(программа.0);
            фильтр(ручка, слой, вид, 14, FWP_ACTION_PERMIT, &[условие])?;
        }

        // 13: всё, что идёт через туннель CryVPN.
        if !v6 {
            let условие = условие_v4(FWPM_CONDITION_IP_LOCAL_ADDRESS, ТУННЕЛЬ, &mut хранилище);
            фильтр(ручка, слой, вид, 13, FWP_ACTION_PERMIT, &[условие])?;
        }

        // 12: частная сеть Tailscale, в том числе её DNS.
        let tailscale = if v6 {
            условие_v6(FWPM_CONDITION_IP_REMOTE_ADDRESS, TAILSCALE_V6.0, TAILSCALE_V6.1, &mut хранилище)
        } else {
            условие_v4(FWPM_CONDITION_IP_REMOTE_ADDRESS, TAILSCALE_V4, &mut хранилище)
        };
        фильтр(ручка, слой, вид, 12, FWP_ACTION_PERMIT, &[tailscale])?;

        // 11: DNS мимо туннеля — нельзя, даже роутеру: он передаст вопрос
        // провайдеру, и тот увидит, какие сайты открывает человек.
        фильтр(ручка, слой, вид, 11, FWP_ACTION_BLOCK, &[условие_порт(ПОРТ_DNS)])?;

        // Заслон на этом всё: остальной IPv4 он не трогает. Защита и заслон
        // для IPv6 закрывают и остальное.
        if вид == Вид::Заслон && !v6 {
            continue;
        }

        // 10: домашняя сеть (роутер, принтер, DHCP). Несколько условий об
        // одном и том же поле Windows складывает через «или».
        let свои: Vec<FWPM_FILTER_CONDITION0> = if v6 {
            СВОИ_СЕТИ_V6
                .iter()
                .map(|&(адрес, длина)| условие_v6(FWPM_CONDITION_IP_REMOTE_ADDRESS, адрес, длина, &mut хранилище))
                .collect()
        } else {
            СВОИ_СЕТИ_V4
                .iter()
                .map(|&сеть| условие_v4(FWPM_CONDITION_IP_REMOTE_ADDRESS, сеть, &mut хранилище))
                .collect()
        };
        фильтр(ручка, слой, вид, 10, FWP_ACTION_PERMIT, &свои)?;

        // 0: остальное — нельзя.
        фильтр(ручка, слой, вид, 0, FWP_ACTION_BLOCK, &[])?;
    }
    Ok(())
}

fn подслой_для(вид: Вид) -> GUID {
    match вид {
        Вид::Защита => ПОДСЛОЙ,
        Вид::Заслон => ПОДСЛОЙ_ЗАСЛОНА,
    }
}

fn условие_порт(порт: u16) -> FWPM_FILTER_CONDITION0 {
    // SAFETY: обнуление — допустимое начальное состояние условия.
    let mut усл: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
    усл.fieldKey = FWPM_CONDITION_IP_REMOTE_PORT;
    усл.matchType = FWP_MATCH_EQUAL;
    усл.conditionValue.r#type = FWP_UINT16;
    усл.conditionValue.Anonymous.uint16 = порт;
    усл
}

fn условие_флаги(флаги: u32) -> FWPM_FILTER_CONDITION0 {
    // SAFETY: обнуление — допустимое начальное состояние условия.
    let mut усл: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
    усл.fieldKey = FWPM_CONDITION_FLAGS;
    усл.matchType = FWP_MATCH_FLAGS_ALL_SET;
    усл.conditionValue.r#type = FWP_UINT32;
    усл.conditionValue.Anonymous.uint32 = флаги;
    усл
}

fn условие_программа(код: *mut FWP_BYTE_BLOB) -> FWPM_FILTER_CONDITION0 {
    // SAFETY: обнуление — допустимое начальное состояние условия.
    let mut усл: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
    усл.fieldKey = FWPM_CONDITION_ALE_APP_ID;
    усл.matchType = FWP_MATCH_EQUAL;
    усл.conditionValue.r#type = FWP_BYTE_BLOB_TYPE;
    усл.conditionValue.Anonymous.byteBlob = код;
    усл
}

fn условие_v4(поле: GUID, (адрес, маска): (u32, u32), х: &mut Хранилище) -> FWPM_FILTER_CONDITION0 {
    х.v4.push(Box::new(FWP_V4_ADDR_AND_MASK { addr: адрес, mask: маска }));
    // SAFETY: обнуление — допустимое начальное состояние условия.
    let mut усл: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
    усл.fieldKey = поле;
    усл.matchType = FWP_MATCH_EQUAL;
    усл.conditionValue.r#type = FWP_V4_ADDR_MASK;
    усл.conditionValue.Anonymous.v4AddrMask = &mut **х.v4.last_mut().expect("только что положили");
    усл
}

fn условие_v6(поле: GUID, адрес: [u8; 16], длина: u8, х: &mut Хранилище) -> FWPM_FILTER_CONDITION0 {
    х.v6.push(Box::new(FWP_V6_ADDR_AND_MASK { addr: адрес, prefixLength: длина }));
    // SAFETY: обнуление — допустимое начальное состояние условия.
    let mut усл: FWPM_FILTER_CONDITION0 = unsafe { std::mem::zeroed() };
    усл.fieldKey = поле;
    усл.matchType = FWP_MATCH_EQUAL;
    усл.conditionValue.r#type = FWP_V6_ADDR_MASK;
    усл.conditionValue.Anonymous.v6AddrMask = &mut **х.v6.last_mut().expect("только что положили");
    усл
}

fn фильтр(
    ручка: HANDLE,
    слой: GUID,
    вид: Вид,
    вес: u8,
    действие: u32,
    условия: &[FWPM_FILTER_CONDITION0],
) -> Result<(), String> {
    let имя = широкая(match вид {
        Вид::Защита => "CryVPN — защита от утечки",
        Вид::Заслон => "CryVPN — заслон от утечек DNS и IPv6",
    });
    let mut условия = условия.to_vec();
    // SAFETY: обнуление — допустимое начальное состояние фильтра.
    let mut ф: FWPM_FILTER0 = unsafe { std::mem::zeroed() };
    ф.displayData.name = имя.as_ptr() as *mut u16;
    ф.layerKey = слой;
    ф.subLayerKey = подслой_для(вид);
    ф.weight.r#type = FWP_UINT8;
    ф.weight.Anonymous.uint8 = вес;
    ф.numFilterConditions = условия.len() as u32;
    ф.filterCondition = if условия.is_empty() { std::ptr::null_mut() } else { условия.as_mut_ptr() };
    ф.action.r#type = действие;
    let mut номер = 0u64;
    // SAFETY: фильтр и всё, на что он указывает, живы до конца вызова.
    let код = unsafe { FwpmFilterAdd0(ручка, &ф, std::ptr::null_mut(), &mut номер) };
    if код != 0 {
        return Err(ошибка("фильтр", код));
    }
    Ok(())
}

#[cfg(test)]
mod проверки {
    use super::*;

    /// На живой Windows с правами администратора: защита закрывает прямой
    /// выход в интернет и открывает его обратно, когда снята.
    /// `cargo test --lib -- --ignored защита_закрывает_и_открывает`
    #[test]
    #[ignore]
    fn защита_закрывает_и_открывает() {
        let достучаться = || {
            std::net::TcpStream::connect_timeout(
                &"1.1.1.1:443".parse().unwrap(),
                std::time::Duration::from_secs(4),
            )
            .is_ok()
        };
        assert!(достучаться(), "без защиты интернета нет — проверять нечего");
        включить(&[]).expect("защита не встала");
        assert!(держит());
        assert!(!достучаться(), "защита стоит, а интернет открыт");
        // Сама машина доступна и под защитой.
        let слушатель = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(std::net::TcpStream::connect(слушатель.local_addr().unwrap()).is_ok());
        выключить();
        assert!(!держит());
        assert!(достучаться(), "защиту сняли, а интернет не вернулся");
    }

    /// Как в жизни: VPN поднят — через туннель всё работает; ядро упало —
    /// интернет закрыт; защиту сняли — открыт. Нужна живая Windows с
    /// правами администратора и ядро Xray с драйвером рядом (путь — в
    /// переменной `XRAY`). Вместо настоящего сервера — выход «напрямую»
    /// изнутри самого ядра: для защиты разницы нет.
    #[test]
    #[ignore]
    fn обрыв_vpn_под_защитой() {
        let достучаться = || {
            std::net::TcpStream::connect_timeout(
                &"1.1.1.1:443".parse().unwrap(),
                std::time::Duration::from_secs(4),
            )
            .is_ok()
        };
        let ядро = PathBuf::from(std::env::var("XRAY").expect("укажите XRAY"));
        let конфиг = std::env::temp_dir().join("cryvpn-killswitch.json");
        std::fs::write(
            &конфиг,
            r#"{"log":{"loglevel":"warning"},"inbounds":[{"tag":"tun","protocol":"tun","settings":{"name":"CryVPN","mtu":1500,"gateway":["198.19.77.1/30"],"dns":["1.1.1.1"],"autoSystemRoutingTable":["0.0.0.0/0"],"autoOutboundsInterface":"auto"}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#,
        )
        .unwrap();

        включить(&[ядро.clone()]).expect("защита не встала");
        let mut дитя = std::process::Command::new(&ядро).arg("run").arg("-c").arg(&конфиг).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(8));
        assert!(достучаться(), "VPN поднят, защита стоит — а интернета нет");

        let _ = дитя.kill();
        let _ = дитя.wait();
        std::thread::sleep(std::time::Duration::from_secs(4));
        assert!(!достучаться(), "ядро упало, а интернет пошёл напрямую — защита не держит");

        выключить();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(достучаться(), "защиту сняли, а интернет не вернулся");
    }

    /// Свой вопрос к открытому DNS от этого процесса — так спрашивают
    /// программы со своим DNS (браузеры тоже так умеют).
    fn dns_отвечает() -> bool {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(crate::serverdns::спросить(std::net::Ipv4Addr::new(8, 8, 8, 8), "one.one.one.one"))
            .is_some()
    }

    fn интернет_есть() -> bool {
        std::net::TcpStream::connect_timeout(&"1.1.1.1:443".parse().unwrap(), std::time::Duration::from_secs(4))
            .is_ok()
    }

    /// Заслон без туннеля: DNS мимо туннеля закрыт, остальной интернет
    /// открыт. Процесс, ради которого поставлен заслон, завершился — заслон
    /// снят сам. Нужна живая Windows с правами администратора.
    /// `cargo test --lib -- --ignored заслон_закрывает_dns_мимо_туннеля`
    #[test]
    #[ignore]
    fn заслон_закрывает_dns_мимо_туннеля() {
        assert!(dns_отвечает(), "без заслона DNS не отвечает — проверять нечего");
        // Вместо ядра — любой долгий процесс: заслону важен только его конец.
        let mut дитя = std::process::Command::new("ping").args(["-n", "60", "127.0.0.1"]).spawn().unwrap();
        заслон_поставить(&[], дитя.id()).expect("заслон не встал");
        assert!(заслон_стоит());
        assert!(!dns_отвечает(), "заслон стоит, а DNS ушёл мимо туннеля");
        assert!(интернет_есть(), "заслон закрыл не только DNS, но и обычный интернет");
        // Сама машина доступна.
        let слушатель = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        assert!(std::net::TcpStream::connect(слушатель.local_addr().unwrap()).is_ok());

        let _ = дитя.kill();
        let _ = дитя.wait();
        for _ in 0..30 {
            if !заслон_стоит() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!заслон_стоит(), "процесс завершился, а заслон остался");
        assert!(dns_отвечает(), "заслон снят, а DNS не вернулся");
    }

    /// Как в жизни: ядро подняло туннель, заслон стоит — DNS через туннель
    /// работает, и у программ, и у самой Windows. Нужна живая Windows с
    /// правами администратора и ядро Xray (путь — в переменной `XRAY`).
    #[test]
    #[ignore]
    fn заслон_пропускает_dns_через_туннель() {
        let ядро = PathBuf::from(std::env::var("XRAY").expect("укажите XRAY"));
        let конфиг = std::env::temp_dir().join("cryvpn-zaslon.json");
        std::fs::write(
            &конфиг,
            r#"{"log":{"loglevel":"warning"},"inbounds":[{"tag":"tun","protocol":"tun","settings":{"name":"CryVPN","mtu":1500,"gateway":["198.19.77.1/30"],"dns":["1.1.1.1"],"autoSystemRoutingTable":["0.0.0.0/0"],"autoOutboundsInterface":"auto"}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#,
        )
        .unwrap();
        let mut дитя = std::process::Command::new(&ядро).arg("run").arg("-c").arg(&конфиг).spawn().unwrap();
        заслон_поставить(&[ядро.clone()], дитя.id()).expect("заслон не встал");
        std::thread::sleep(std::time::Duration::from_secs(8));

        assert!(интернет_есть(), "туннель поднят, а интернета нет");
        assert!(dns_отвечает(), "DNS через туннель не прошёл");
        use std::net::ToSocketAddrs;
        assert!(
            ("example.com", 443).to_socket_addrs().is_ok_and(|mut а| а.next().is_some()),
            "Windows не нашла имя через туннель"
        );

        let _ = дитя.kill();
        let _ = дитя.wait();
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!(!заслон_стоит(), "ядро завершилось, а заслон остался");
    }

    /// Защита от утечки теперь закрывает и DNS мимо туннеля — в том числе
    /// роутеру: он «домашняя сеть», но передаёт вопрос провайдеру.
    #[test]
    #[ignore]
    fn защита_закрывает_dns() {
        assert!(dns_отвечает(), "без защиты DNS не отвечает — проверять нечего");
        включить(&[]).expect("защита не встала");
        assert!(!dns_отвечает(), "защита стоит, а DNS ушёл мимо туннеля");
        выключить();
        assert!(dns_отвечает(), "защиту сняли, а DNS не вернулся");
    }

    #[test]
    fn сети_записаны_верно() {
        let адрес = |п1: u8, п2: u8, п3: u8, п4: u8| u32::from_be_bytes([п1, п2, п3, п4]);
        assert_eq!(ТУННЕЛЬ.0, адрес(198, 19, 77, 0));
        assert_eq!(СВОИ_СЕТИ_V4[0].0, адрес(10, 0, 0, 0));
        assert_eq!(СВОИ_СЕТИ_V4[1].1, адрес(255, 240, 0, 0));
        assert_eq!(СВОИ_СЕТИ_V4[2].0, адрес(192, 168, 0, 0));
        assert_eq!(СВОИ_СЕТИ_V4[4], (адрес(100, 64, 0, 0), адрес(255, 192, 0, 0)));
        assert_eq!(TAILSCALE_V4, СВОИ_СЕТИ_V4[4]);
        assert_eq!(
            std::net::Ipv6Addr::from(TAILSCALE_V6.0),
            "fd7a:115c:a1e0::".parse::<std::net::Ipv6Addr>().unwrap()
        );
    }
}
