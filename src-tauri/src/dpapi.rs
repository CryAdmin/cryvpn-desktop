//! Шифрование средствами Windows (DPAPI) прямым обращением к системе.
//!
//! Раньше ради каждой записи и каждого чтения запускался PowerShell. Сам
//! шифр тот же, и записи, сделанные прежними версиями, читаются как есть:
//! PowerShell звал ту же функцию системы, только через посредника.

use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

/// Забираем ответ системы и возвращаем ей память.
///
/// # Safety
/// `ответ` заполнен удачным вызовом системы и ещё не освобождён.
unsafe fn забрать(ответ: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if ответ.pbData.is_null() {
        return Vec::new();
    }
    let байты = unsafe { std::slice::from_raw_parts(ответ.pbData, ответ.cbData as usize) }.to_vec();
    unsafe { LocalFree(ответ.pbData.cast()) };
    байты
}

fn вызвать(данные: &[u8], зашифровать: bool) -> Option<Vec<u8>> {
    let длина = u32::try_from(данные.len()).ok()?;
    let вход = CRYPT_INTEGER_BLOB { cbData: длина, pbData: данные.as_ptr().cast_mut() };
    let mut выход = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: вход указывает на наши данные верной длины и только читается;
    // выход система заполняет сама, память за ней возвращаем в `забрать`.
    // Окон система показывать не должна: спрашивать тут не о чем.
    let удача = unsafe {
        if зашифровать {
            CryptProtectData(
                &вход,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut выход,
            )
        } else {
            CryptUnprotectData(
                &вход,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut выход,
            )
        }
    };
    if удача == 0 {
        return None;
    }
    Some(unsafe { забрать(выход) })
}

/// Зашифровать под учётную запись человека.
pub fn закрыть(открытое: &[u8]) -> Option<Vec<u8>> {
    вызвать(открытое, true)
}

/// Расшифровать. Чужая или испорченная запись — `None`.
pub fn открыть(закрытое: &[u8]) -> Option<Vec<u8>> {
    вызвать(закрытое, false)
}
