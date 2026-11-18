//! Мост к iPhone: включить и выключить VPN, узнать его состояние.
//!
//! VPN на iPhone живёт в расширении (`gen/apple/CryTunnel`): iOS держит его
//! работающим, даже когда окно приложения закрыто, и в нём крутится ядро
//! Xray. Просить iOS включить или выключить расширение умеет только код на
//! Swift (`gen/apple/Sources/cryvpn/CryTunnel.swift`). Отсюда мы зовём его
//! функции напрямую, как функции на C.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

unsafe extern "C" {
    fn cryvpn_ios_start(
        config: *const c_char,
        title: *const c_char,
        dns: *const c_char,
        kill: bool,
    ) -> *mut c_char;
    fn cryvpn_ios_stop();
    fn cryvpn_ios_status() -> i32;
    fn cryvpn_ios_info() -> *mut c_char;
    fn cryvpn_ios_free(text: *mut c_char);
}

/// Забрать строку у Swift и освободить её память там же, где выделяли.
fn забрать(указатель: *mut c_char) -> Option<String> {
    if указатель.is_null() {
        return None;
    }
    // SAFETY: Swift отдаёт строку, завершённую нулём, выделенную strdup.
    let текст = unsafe { CStr::from_ptr(указатель) }.to_string_lossy().into_owned();
    unsafe { cryvpn_ios_free(указатель) };
    Some(текст)
}

/// Поднять VPN. Ждёт, пока iOS спросит у человека разрешение (в первый
/// раз), включит расширение и ядро в нём заработает. Блокирует поток —
/// звать только не из главного: ответы iOS приходят именно в главный.
pub fn поднять(конфиг: &str, сервер: &str, днс: &[String], защита: bool) -> Result<(), String> {
    let конфиг = CString::new(конфиг).map_err(|_| "конфигурация повреждена".to_string())?;
    let сервер = CString::new(сервер.replace('\0', "")).unwrap_or_default();
    let днс = CString::new(днс.join(",")).unwrap_or_default();
    // SAFETY: все три строки живут до конца вызова.
    let ответ = unsafe { cryvpn_ios_start(конфиг.as_ptr(), сервер.as_ptr(), днс.as_ptr(), защита) };
    match забрать(ответ) {
        None => Ok(()),
        Some(беда) => Err(беда),
    }
}

/// Выключить VPN. Не ждёт: iOS гасит расширение сама.
pub fn опустить() {
    // SAFETY: функция без аргументов, безопасна из любого потока.
    unsafe { cryvpn_ios_stop() }
}

/// 0 — выключен, 1 — подключается, 2 — включён.
///
/// Ответ берётся из запомненного состояния и не ждёт iOS, поэтому годится
/// и для главного потока.
pub fn состояние() -> i32 {
    // SAFETY: функция без аргументов, безопасна из любого потока.
    unsafe { cryvpn_ios_status() }
}

/// Модель телефона и версия iOS — для списка устройств в кабинете.
pub fn сведения() -> (String, String) {
    // SAFETY: функция без аргументов; строку забираем через `забрать`.
    let текст = забрать(unsafe { cryvpn_ios_info() }).unwrap_or_default();
    let данные: serde_json::Value = serde_json::from_str(&текст).unwrap_or_default();
    let поле = |имя: &str| {
        данные.get(имя).and_then(|з| з.as_str()).filter(|з| !з.is_empty()).unwrap_or("unknown").to_string()
    };
    (поле("model"), поле("release"))
}
