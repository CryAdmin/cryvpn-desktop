//! Запуск ядра Xray из службы VPN на Android.
//!
//! Служба VPN живёт в отдельном процессе, чтобы смахивание приложения из
//! списка недавних не гасило VPN. Ядру нужно передать открытый туннель по
//! наследству, а средства Java при запуске программы закрывают в ней все
//! чужие дескрипторы. Поэтому запуск сделан здесь, одной функцией.

use jni::objects::{JObject, JString};
use jni::sys::jint;
use jni::JNIEnv;
use std::process::{Command, Stdio};

fn строка(env: &mut JNIEnv, значение: &JString) -> String {
    env.get_string(значение).map(|с| с.into()).unwrap_or_default()
}

/// Запускает ядро и отдаёт номер его процесса; отрицательное число — отказ.
///
/// `fd` — туннель, полученный от системы. Снимаем с него запрет на
/// наследование и сообщаем номер ядру через переменную окружения. Закрыть
/// свой экземпляр после запуска — забота вызывающего: туннель должен жить
/// ровно столько, сколько живёт ядро.
#[no_mangle]
pub extern "system" fn Java_top_cryvpn_app_CryVpnService_spawnCore<'l>(
    mut env: JNIEnv<'l>,
    _служба: JObject<'l>,
    ядро: JString<'l>,
    конфиг: JString<'l>,
    базы: JString<'l>,
    журнал: JString<'l>,
    fd: jint,
) -> jint {
    let ядро = строка(&mut env, &ядро);
    let конфиг = строка(&mut env, &конфиг);
    let базы = строка(&mut env, &базы);
    let журнал = строка(&mut env, &журнал);

    if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } != 0 {
        return -1;
    }
    let Ok(вывод) = std::fs::File::create(&журнал) else { return -2 };
    let Ok(ошибки) = вывод.try_clone() else { return -2 };

    match Command::new(&ядро)
        .env("XRAY_LOCATION_ASSET", &базы)
        .env("XRAY_TUN_FD", fd.to_string())
        .args(["-c", &конфиг])
        .stdin(Stdio::null())
        .stdout(вывод)
        .stderr(ошибки)
        .spawn()
    {
        Ok(mut дитя) => {
            let номер = дитя.id() as jint;
            // Дожидаемся конца ядра в фоне, иначе после выхода оно осталось
            // бы в списке процессов «зомби», и служба считала бы его живым.
            std::thread::spawn(move || {
                let _ = дитя.wait();
            });
            номер
        }
        Err(_) => -3,
    }
}
