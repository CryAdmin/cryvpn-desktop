//! Программы, которые человек пускает мимо VPN.
//!
//! Ядро отличает трафик по полному пути исполняемого файла, поэтому нам
//! нужен именно путь. Найти его руками умеет не каждый, так что даём два
//! способа: выбрать из уже запущенных программ с окнами или указать файл.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Программа {
    /// Что показать человеку: название из свойств файла или имя файла.
    pub имя: String,
    /// Компьютер: полный путь к исполняемому файлу — по нему ядро узнаёт
    /// трафик. Android: имя пакета — по нему приложение исключает система.
    pub путь: String,
}

/// Имя файла без папок — запасное название для списка.
pub fn имя_из_пути(путь: &str) -> String {
    путь.rsplit(['\\', '/']).next().unwrap_or(путь).to_string()
}

/// Запущенные сейчас программы, у которых есть окно.
#[cfg(windows)]
pub fn запущенные() -> Vec<Программа> {
    use std::os::windows::process::CommandExt;
    // Имена переменных латиницей — см. core::чужой_vpn. Вывод просим в
    // UTF-8 и разделяем табуляцией: в путях и названиях её не бывает.
    let скрипт = "\
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8;
Get-Process | Where-Object { $_.MainWindowTitle -and $_.Path } | Sort-Object Path -Unique | ForEach-Object {
  $d = $_.MainModule.FileVersionInfo.FileDescription; if (-not $d) { $d = $_.ProcessName }
  $d + \"`t\" + $_.Path
}";
    let Ok(вывод) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", скрипт])
        .creation_flags(0x08000000)
        .output()
    else {
        return Vec::new();
    };
    let свой = std::env::current_exe().ok().map(|п| п.display().to_string().to_lowercase());
    let mut список: Vec<Программа> = String::from_utf8_lossy(&вывод.stdout)
        .lines()
        .filter_map(|строка| {
            let (имя, путь) = строка.trim().split_once('\t')?;
            // Себя в список не кладём: приложение VPN мимо VPN — бессмыслица.
            (Some(путь.to_lowercase()) != свой).then(|| Программа {
                имя: имя.trim().to_string(),
                путь: путь.trim().to_string(),
            })
        })
        .collect();
    список.sort_by_key(|п| п.имя.to_lowercase());
    список
}

/// Android: установленные приложения, у которых есть значок запуска.
#[cfg(target_os = "android")]
pub fn запущенные() -> Vec<Программа> {
    crate::android::приложения()
}

/// Linux: программы этого человека, запущенные в его рабочем столе.
///
/// Берём только свои процессы и только те, что видят экран: служб и
/// фоновых помощников в системе сотни, и в списке «что пустить мимо VPN»
/// им делать нечего. Ядро к тому же и не смогло бы узнать трафик чужих
/// процессов — оно работает от имени человека.
#[cfg(target_os = "linux")]
pub fn запущенные() -> Vec<Программа> {
    use std::os::unix::fs::MetadataExt;
    let Some(кто) = crate::paths::папка_данных()
        .and_then(|п| std::fs::metadata(п).ok())
        .map(|м| м.uid())
    else {
        return Vec::new();
    };
    let свой = std::env::current_exe().ok();
    let mut пути = std::collections::BTreeSet::new();
    for запись in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let папка = запись.path();
        let числовая = запись.file_name().to_string_lossy().chars().all(|з| з.is_ascii_digit());
        if !числовая || std::fs::metadata(&папка).map(|м| м.uid()).ok() != Some(кто) {
            continue;
        }
        let Ok(файл) = std::fs::read_link(папка.join("exe")) else { continue };
        if Some(&файл) == свой.as_ref() {
            continue; // приложение VPN мимо VPN — бессмыслица
        }
        let путь = файл.display().to_string();
        // Служебное: помощники рабочего стола и самой системы.
        if путь.ends_with(" (deleted)")
            || путь.starts_with("/usr/libexec/")
            || путь.starts_with("/usr/lib/systemd/")
            || путь.contains("/xray")
        {
            continue;
        }
        // Видит ли процесс экран: по переменным, с которыми его запустили.
        let Ok(среда) = std::fs::read(папка.join("environ")) else { continue };
        let с_экраном = среда
            .split(|б| *б == 0)
            .any(|п| п.starts_with(b"DISPLAY=") || п.starts_with(b"WAYLAND_DISPLAY="));
        if с_экраном {
            пути.insert(путь);
        }
    }
    let mut список: Vec<Программа> =
        пути.into_iter().map(|путь| Программа { имя: имя_из_пути(&путь), путь }).collect();
    список.sort_by_key(|п| п.имя.to_lowercase());
    список
}

#[cfg(not(any(windows, target_os = "android", target_os = "linux")))]
pub fn запущенные() -> Vec<Программа> {
    Vec::new()
}

/// Пути в том виде, в каком их сравнивает ядро: с прямыми косыми.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub fn для_ядра(программы: &[Программа]) -> Vec<String> {
    программы
        .iter()
        .map(|п| п.путь.trim().replace('\\', "/"))
        .filter(|п| !п.is_empty())
        .collect()
}

#[cfg(test)]
mod проверки {
    use super::*;

    #[test]
    fn пути_для_ядра() {
        let пути = vec![
            Программа { имя: "App".into(), путь: r"C:\Program Files\App\app.exe".into() },
            Программа { имя: "пусто".into(), путь: "  ".into() },
        ];
        assert_eq!(для_ядра(&пути), vec!["C:/Program Files/App/app.exe"]);
        assert_eq!(имя_из_пути(r"C:\Program Files\App\app.exe"), "app.exe");
    }
}
