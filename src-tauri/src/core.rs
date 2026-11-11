//! Туннель: ядро Xray в режиме TUN.
//!
//! Весь трафик системы уходит в туннельный адаптер, как у настоящего
//! VPN-клиента. Системный прокси не трогаем вовсе — он покрывает только
//! браузеры и оставляет мимо себя игры, торренты и часть программ.
//!
//! Почему из игр не выкидывает: правила маршрутизации приходят из подписки
//! Ремны и уже разделяют трафик — российские домены и адреса идут напрямую,
//! остальное в туннель. Мы эти правила не трогаем, только добавляем вход.
//!
//! Нужны права администратора: без них адаптер не создать и маршруты не
//! переписать. Поэтому приложение запрашивает их при запуске один раз, а не
//! дёргает окно согласия при каждом подключении.

use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Child, Command};

/// Версии держим в коде: обновление ядра должно быть нашим решением,
/// а не сюрпризом в день, когда у поставщика что-то поменялось.

/// Порт, на котором ядро отдаёт счётчики трафика. Берём непопулярный,
/// чтобы не столкнуться с чужой программой.
const ПОРТ_УЧЁТА: u16 = 10853;

#[cfg(windows)]
const ИМЯ_ЯДРА: &str = "xray.exe";
#[cfg(not(windows))]
const ИМЯ_ЯДРА: &str = "xray";

/// Как зовётся туннельный адаптер. macOS принимает только имена вида
/// `utunN`; номер берём большой, чтобы не занять тот, что нужен системе
/// или другому VPN.
#[cfg(target_os = "macos")]
const ИМЯ_ТУННЕЛЯ: &str = "utun123";
#[cfg(not(target_os = "macos"))]
const ИМЯ_ТУННЕЛЯ: &str = "CryVPN";

/// Чем поднимаем туннель. У разных движков разное поведение в сетях
/// провайдеров: где-то лучше один, где-то другой. Поэтому выбор оставляем
/// человеку, а не зашиваем намертво.
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Движок {
    /// Встроенный TUN самого ядра Xray. Работает с версии 26.1.23,
    /// сам правит таблицу маршрутов.
    XrayTun,
}

impl Default for Движок {
    fn default() -> Self {
        Движок::XrayTun
    }
}

pub struct Туннель {
    процесс: Option<Child>,
    /// На macOS ядро запускает не приложение, а оболочка с правами root:
    /// обычной программе там туннель не поднять. Дочернего процесса у нас
    /// тогда нет, и жив ли туннель, узнаём по его порту учёта.
    внешний: bool,
    /// Уборка после прошлого отключения, идущая в фоне.
    уборка: Option<std::thread::JoinHandle<()>>,
    pub движок: Движок,
    /// Где лежат ядро, драйвер и базы geoip. Кладём их в установщик:
    /// качать 16 МБ при первом подключении — это сорок секунд молчания,
    /// а у части людей GitHub ещё и недоступен.
    pub ресурсы: PathBuf,
}

#[derive(serde::Serialize, Default, Clone, Copy)]
pub struct Счётчики {
    pub отдано: u64,
    pub принято: u64,
}

impl Туннель {
    pub fn новый(ресурсы: PathBuf) -> Self {
        Self { процесс: None, внешний: false, уборка: None, движок: Движок::default(), ресурсы }
    }

    pub fn включён(&mut self) -> bool {
        if self.внешний {
            return порт_жив(ПОРТ_УЧЁТА);
        }
        match &mut self.процесс {
            None => false,
            Some(дитя) => matches!(дитя.try_wait(), Ok(None)),
        }
    }

    pub async fn включить(&mut self, конфиг: &Value) -> Result<(), String> {
        self.выключить();
        // Уборка прошлого сеанса снимает маршруты адаптера CryVPN. Если
        // не дождаться её конца, она снесла бы маршруты уже нового туннеля.
        self.дождаться_уборки();

        let ядро = self.ресурсы.join(ИМЯ_ЯДРА);
        if !ядро.exists() {
            return Err("В установке нет ядра — переустановите приложение".into());
        }

        // Чужой VPN мы не выключаем: это чужая программа, и снимать её
        // туннель без спроса значило бы ломать человеку систему. Говорим
        // прямо, что мешает и что сделать.
        if let Some(чужой) = чужой_vpn() {
            return Err(format!(
                "Сейчас включён другой VPN — {чужой}. Отключите его и нажмите ещё раз: два VPN одновременно мешают друг другу"
            ));
        }

        let готовый = собрать_конфиг(конфиг);
        let путь_конфига = папка()?.join("current.json");
        std::fs::write(
            &путь_конфига,
            serde_json::to_vec_pretty(&готовый).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("не удалось записать конфигурацию: {e}"))?;

        let журнал = папка()?.join("xray.log");
        // Журнал прошлого сеанса не затираем, а откладываем рядом: когда
        // туннель оборвался и человек тут же переподключился, причина
        // обрыва была только там. Посещённых адресов в журнале нет —
        // их запись выключена, остаются предупреждения и ошибки ядра.
        let _ = std::fs::rename(&журнал, папка()?.join("xray.prev.log"));
        // Отметка на диске ставится до запуска. В Windows по ней после
        // падения чинится сеть. На macOS она ещё и держит ядро живым:
        // оболочка с правами снимает его, как только отметка исчезнет.
        отметка_поставить();

        #[cfg(target_os = "macos")]
        {
            if let Err(беда) = запустить_с_правами(&ядро, &путь_конфига, &журнал, &self.ресурсы) {
                отметка_снять();
                let _ = std::fs::remove_file(&путь_конфига);
                return Err(беда);
            }
            self.внешний = true;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let дитя = запустить(&ядро, &путь_конфига, &журнал, &self.ресурсы)?;
            self.процесс = Some(дитя);
        }

        // Ждём, пока ядро поднимет учёт: это признак, что оно живо и
        // дошло до работы, а не упало на разборе конфигурации.
        if !дождаться_порта(ПОРТ_УЧЁТА, 40).await {
            let причина = последние_строки(&журнал).unwrap_or_default();
            self.выключить();
            return Err(if причина.is_empty() {
                "Ядро не запустилось. Проверьте права администратора".into()
            } else {
                format!("Ядро не запустилось: {причина}")
            });
        }

        // Ядро прочитало конфигурацию при старте и больше к файлу не
        // вернётся. А в файле — ключи подписки этого человека открытым
        // текстом. Стираем сразу: на диске им делать нечего.
        let _ = std::fs::remove_file(&путь_конфига);
        Ok(())
    }

    /// Отключение должно быть мгновенным: человек нажал и сразу видит итог.
    ///
    /// Ядро снимаем сразу. Вежливая просьба закрыться до него не доходит —
    /// у него нет окна, — а ждали мы её по три секунды на каждое нажатие.
    /// Адаптер Windows убирает вместе с процессом, маршруты уходят с ним;
    /// это проверено на живой машине трижды. Уборка остатков остаётся как
    /// страховка, но идёт в фоне и кнопку не держит.
    pub fn выключить(&mut self) {
        let был_запущен = self.процесс.is_some() || self.внешний;
        if let Some(mut дитя) = self.процесс.take() {
            let _ = дитя.kill();
            let _ = дитя.wait();
        }
        отметка_снять();
        if self.внешний {
            // Отметка снята — оболочка с правами сама гасит ядро. Ждём,
            // пока оно действительно уйдёт, чтобы следующее подключение
            // не столкнулось с ещё живым прежним.
            for _ in 0..16 {
                if !порт_жив(ПОРТ_УЧЁТА) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            self.внешний = false;
        }
        // Конфигурацию с ключами стираем сразу. Журнал ядра откладываем
        // как «прошлый»: в нём нет ни ключей, ни посещённых адресов, зато
        // по нему видно, почему оборвался туннель.
        if let Ok(п) = папка() {
            let _ = std::fs::remove_file(п.join("current.json"));
            if был_запущен {
                let _ = std::fs::rename(п.join("xray.log"), п.join("xray.prev.log"));
            }
        }
        if был_запущен {
            self.дождаться_уборки();
            self.уборка = Some(std::thread::spawn(прибрать_за_собой));
        }
    }

    fn дождаться_уборки(&mut self) {
        if let Some(ручка) = self.уборка.take() {
            let _ = ручка.join();
        }
    }

    /// Сколько прошло через туннель за всё подключение.
    pub fn счётчики(&self) -> Счётчики {
        let ядро = self.ресурсы.join(ИМЯ_ЯДРА);
        let Ok(вывод) = запрос_учёта(&ядро) else {
            return Счётчики::default();
        };
        let Ok(данные) = serde_json::from_str::<Value>(&вывод) else {
            return Счётчики::default();
        };

        let mut итог = Счётчики::default();
        for запись in данные.get("stat").and_then(|с| с.as_array()).into_iter().flatten() {
            let имя = запись.get("name").and_then(|и| и.as_str()).unwrap_or("");
            // Ядро отдаёт значение числом, а старые версии — строкой.
            // Принимаем оба вида: из-за ожидания одной только строки
            // скорость в окне показывала ноль при живом трафике.
            let сколько = запись
                .get("value")
                .and_then(|з| {
                    з.as_u64()
                        .or_else(|| з.as_str().and_then(|т| т.parse::<u64>().ok()))
                })
                .unwrap_or(0);
            if имя.contains("uplink") {
                итог.отдано += сколько;
            } else if имя.contains("downlink") {
                итог.принято += сколько;
            }
        }
        итог
    }
}

impl Drop for Туннель {
    /// Приложение закрыли — туннель гасим. Иначе адаптер останется в
    /// системе, маршруты будут вести в никуда, и человек потеряет интернет.
    fn drop(&mut self) {
        self.выключить();
        self.дождаться_уборки();
    }
}

/// Собираем конфигурацию для ядра: от подписки берём всё, кроме входов.
///
/// Входы в подписке рассчитаны на прокси (socks и http на localhost).
/// Нам нужен туннель, поэтому ставим свой вход, а правила, выходы и DNS
/// оставляем как есть — в них и заключается вся «умная» маршрутизация.
fn собрать_конфиг(исходный: &Value) -> Value {
    let mut конфиг = исходный.clone();

    конфиг["inbounds"] = json!([
        {
            "tag": "tun",
            "protocol": "tun",
            "settings": {
                "name": ИМЯ_ТУННЕЛЯ,
                "desc": "CryVPN",
                "mtu": 1500,
                // Адрес из диапазона 198.18.0.0/15: он отведён под стенды и
                // в живых сетях не встречается. Но самое начало, 198.18.0.1,
                // берут себе другие VPN-клиенты (проверено на живой машине),
                // а 172.19.x — Docker и WSL. Поэтому своя подсеть в глубине
                // диапазона: спорить за адрес нам не с кем.
                "gateway": ["198.19.77.1/30"],
                "dns": ["1.1.1.1", "8.8.8.8"],
                "userLevel": 0,
                // Ядро само пропишет маршруты и само исключит из туннеля
                // собственные соединения — без этого трафик до сервера
                // уходил бы обратно в туннель и всё вставало бы кольцом.
                "autoSystemRoutingTable": ["0.0.0.0/0"],
                "autoOutboundsInterface": "auto"
            },
            "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
        },
        {
            "tag": "учёт",
            "protocol": "dokodemo-door",
            "listen": "127.0.0.1",
            "port": ПОРТ_УЧЁТА,
            "settings": { "address": "127.0.0.1" }
        }
    ]);

    // Журнал посещений выключаем: по умолчанию ядро пишет в файл каждый
    // адрес, куда ходил человек. Список его сайтов на диске нам не нужен и
    // ему не нужен — оставляем только предупреждения и ошибки самого ядра.
    // Подробный журнал включается только файлом-меткой `debug.on` в папке
    // данных — для разбора неполадки на конкретной машине. В нём видны
    // адреса, куда ходит человек, поэтому по умолчанию он выключен и
    // включать его можно только с ведома владельца компьютера.
    let подробно = папка().map(|п| п.join("debug.on").exists()).unwrap_or(false);
    конфиг["log"] = json!({
        "access": "none",
        "loglevel": if подробно { "debug" } else { "warning" },
        "dnsLog": false
    });

    конфиг["stats"] = json!({});
    конфиг["api"] = json!({ "tag": "api", "services": ["StatsService"] });
    конфиг["policy"] = json!({
        "system": { "statsOutboundUplink": true, "statsOutboundDownlink": true }
    });

    // Запросы к учёту не должны уходить в туннель.
    if let Some(правила) = конфиг
        .get_mut("routing")
        .and_then(|р| р.get_mut("rules"))
        .and_then(|р| р.as_array_mut())
    {
        правила.insert(
            0,
            json!({ "type": "field", "inboundTag": ["учёт"], "outboundTag": "api" }),
        );
    }

    конфиг
}

fn папка() -> Result<PathBuf, String> {
    crate::paths::папка_данных().ok_or_else(|| "не найдена папка данных приложения".to_string())
}

#[cfg(windows)]
fn запустить(
    ядро: &PathBuf,
    конфиг: &PathBuf,
    журнал: &PathBuf,
    ресурсы: &PathBuf,
) -> Result<Child, String> {
    use std::os::windows::process::CommandExt;
    let файл = std::fs::File::create(журнал).map_err(|e| e.to_string())?;
    let ошибки = файл.try_clone().map_err(|e| e.to_string())?;
    Command::new(ядро)
        // Без этого ядро не найдёт geoip.dat и geosite.dat, и правила
        // «российское идёт напрямую» молча перестанут работать.
        .env("XRAY_LOCATION_ASSET", ресурсы)
        .args(["-c", &конфиг.display().to_string()])
        .stdout(файл)
        .stderr(ошибки)
        .creation_flags(0x08000000) // без чёрного окна консоли
        .spawn()
        .map_err(|e| format!("не удалось запустить ядро: {e}"))
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn запустить(
    ядро: &PathBuf,
    конфиг: &PathBuf,
    _журнал: &PathBuf,
    ресурсы: &PathBuf,
) -> Result<Child, String> {
    Command::new(ядро)
        .env("XRAY_LOCATION_ASSET", ресурсы)
        .args(["-c", &конфиг.display().to_string()])
        .spawn()
        .map_err(|e| format!("не удалось запустить ядро: {e}"))
}

/// macOS: ядро запускает оболочка с правами администратора.
///
/// Туннельный адаптер и таблицу маршрутов там может трогать только root.
/// Система сама показывает окно с паролем; мы пароль не видим и не храним.
///
/// Оболочка не просто запускает ядро, а сторожит его: стоит исчезнуть
/// отметке на диске или самому приложению — ядро снимается. Поэтому
/// отключение не требует второго пароля, а упавшее приложение не
/// оставляет после себя туннель, который некому выключить.
#[cfg(target_os = "macos")]
fn запустить_с_правами(
    ядро: &PathBuf,
    конфиг: &PathBuf,
    журнал: &PathBuf,
    ресурсы: &PathBuf,
) -> Result<(), String> {
    let данные = папка()?;
    let отметка = данные.join("tunnel.on");
    let сценарий = данные.join("run.sh");

    // Одинарная кавычка в пути сломала бы сценарий; в путях macOS её
    // практически не бывает, но лучше отказаться, чем запустить кривое.
    for путь in [ядро, конфиг, журнал, ресурсы, &отметка] {
        if путь.display().to_string().contains('\'') {
            return Err("В пути к приложению есть кавычка — туннель не поднять".into());
        }
    }

    // Оболочка сторожит ядро и ведёт запись о том, чем кончился сеанс.
    //
    // Проверено на живой машине: на macOS ядро раз в ~22 минуты само
    // завершает работу с кодом 0 — не падает и не убито, а выходит.
    // Причина пока не найдена. Поэтому оболочка поднимает его заново сама:
    // для человека это секундная заминка вместо оборванного VPN и нового
    // ввода пароля. Каждая такая остановка пишется в `tunnel.end`.
    //
    // Конфигурация для перезапуска нужна оболочке всё время, а в ней ключи
    // подписки. Поэтому она сразу забирает её в файл, доступный только
    // root, и стирает исходную из папки пользователя.
    //
    // Если ядро падает трижды подряд сразу после старта — это не та
    // болезнь, а сломанная конфигурация; тогда сдаёмся и говорим об этом,
    // а не крутим перезапуск вечно.
    let итог = данные.join("tunnel.end");
    let текст = format!(
        "#!/bin/sh\n\
export XRAY_LOCATION_ASSET='{ресурсы}'\n\
CFG=/var/run/cryvpn-{мы}.json\n\
umask 077\n\
cp '{конфиг}' \"$CFG\" && rm -f '{конфиг}'\n\
note() {{ echo \"$(date '+%Y-%m-%d %H:%M:%S') $1 | маршрут: $(route -n get default 2>/dev/null | awk '/interface/{{print $2}}') | utun: $(ifconfig {туннель} >/dev/null 2>&1 && echo есть || echo нет)\" >> '{итог}'; }}\n\
WHY=''\n\
FAST=0\n\
N=0\n\
: > '{журнал}'\n\
while [ -z \"$WHY\" ]; do\n\
  S=$(date +%s)\n\
  '{ядро}' -c \"$CFG\" >> '{журнал}' 2>&1 &\n\
  X=$!\n\
  while :; do\n\
    kill -0 {мы} 2>/dev/null || {{ WHY='приложение закрыто'; break; }}\n\
    [ -f '{отметка}' ] || {{ WHY='отметка снята: отключение по кнопке'; break; }}\n\
    if ! kill -0 $X 2>/dev/null; then\n\
      wait $X; CODE=$?\n\
      LIVED=$(( $(date +%s) - S ))\n\
      N=$((N+1))\n\
      if [ $LIVED -lt 15 ]; then FAST=$((FAST+1)); else FAST=0; fi\n\
      if [ $FAST -ge 3 ]; then WHY=\"ядро не держится: три остановки подряд сразу после старта, код $CODE\"; break; fi\n\
      note \"ядро остановилось само, код $CODE, прожило $LIVED с — перезапуск №$N\"\n\
      break\n\
    fi\n\
    sleep 0.3\n\
  done\n\
done\n\
note \"$WHY\"\n\
kill $X 2>/dev/null\n\
sleep 0.5\n\
kill -9 $X 2>/dev/null\n\
rm -f \"$CFG\" '{конфиг}' '{отметка}'\n",
        ресурсы = ресурсы.display(),
        ядро = ядро.display(),
        конфиг = конфиг.display(),
        журнал = журнал.display(),
        отметка = отметка.display(),
        итог = итог.display(),
        туннель = ИМЯ_ТУННЕЛЯ,
        мы = std::process::id(),
    );
    std::fs::write(&сценарий, текст).map_err(|e| format!("не удалось подготовить запуск: {e}"))?;

    let яблочный = format!(
        "do shell script \"/bin/sh '{}' > /dev/null 2>&1 &\" with prompt \"CryVPN поднимает туннель\" with administrator privileges",
        сценарий.display()
    );
    let вывод = Command::new("osascript")
        .args(["-e", &яблочный])
        .output()
        .map_err(|e| format!("не удалось запросить права: {e}"))?;

    if вывод.status.success() {
        Ok(())
    } else {
        let причина = String::from_utf8_lossy(&вывод.stderr);
        Err(if причина.contains("-128") || причина.contains("User canceled") {
            "Подключение отменено: нужен пароль администратора".into()
        } else {
            format!("Система не дала права на туннель: {}", причина.trim())
        })
    }
}

#[cfg(windows)]
fn запрос_учёта(ядро: &PathBuf) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let вывод = Command::new(ядро)
        .args([
            "api",
            "statsquery",
            &format!("--server=127.0.0.1:{ПОРТ_УЧЁТА}"),
        ])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&вывод.stdout).to_string())
}

#[cfg(not(windows))]
fn запрос_учёта(ядро: &PathBuf) -> Result<String, String> {
    let вывод = Command::new(ядро)
        .args([
            "api",
            "statsquery",
            &format!("--server=127.0.0.1:{ПОРТ_УЧЁТА}"),
        ])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&вывод.stdout).to_string())
}

/// Включён ли сейчас другой VPN, который гонит через себя весь интернет.
///
/// Два таких туннеля одновременно делят маршрут по умолчанию, и в итоге не
/// работает ни один: трафик уходит то в один, то в другой. Поэтому наш
/// туннель не поднимается поверх чужого и гаснет, если чужой включили
/// позже.
///
/// Признак: маршрут по умолчанию (целиком или двумя половинами) ведёт
/// через туннельный адаптер, и этот адаптер не наш. Туннели без такого
/// маршрута — например Tailscale без выходного узла — интернет через себя
/// не ведут, им мы не мешаем и они нам тоже. Возвращает название чужого
/// адаптера, как его показывает Windows.
#[cfg(windows)]
pub fn чужой_vpn() -> Option<String> {
    use std::os::windows::process::CommandExt;
    // Имена переменных в сценарии — латиницей: с русскими PowerShell
    // спотыкался о кодировку командной строки.
    let скрипт = "\
$phys = @(Get-NetAdapter -Physical -ErrorAction SilentlyContinue | ForEach-Object { $_.ifIndex });
$routes = Get-NetRoute -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $_.DestinationPrefix -in '0.0.0.0/0','0.0.0.0/1','128.0.0.0/1' };
foreach ($r in $routes) {
  if ($phys -contains $r.ifIndex) { continue }
  $a = Get-NetAdapter -InterfaceIndex $r.ifIndex -ErrorAction SilentlyContinue;
  if (-not $a -or $a.Status -ne 'Up' -or $a.Name -like 'CryVPN*') { continue }
  if ($a.InterfaceType -eq 53 -or $a.InterfaceDescription -match 'TAP|TUN|VPN|WireGuard') { $a.InterfaceDescription; break }
}";
    let вывод = Command::new("powershell")
        .args(["-NoProfile", "-Command", скрипт])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    let имя = String::from_utf8_lossy(&вывод.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches(" Tunnel")
        .trim_end_matches(" Adapter")
        .to_string();
    (!имя.is_empty()).then_some(имя)
}

#[cfg(not(windows))]
pub fn чужой_vpn() -> Option<String> {
    None
}

/// Чем кончился последний сеанс туннеля, по записи сторожевой оболочки.
pub fn причина_остановки() -> Option<String> {
    let текст = std::fs::read_to_string(папка().ok()?.join("tunnel.end")).ok()?;
    текст.lines().last().map(|с| с.trim().to_string()).filter(|с| !с.is_empty())
}

/// Отвечает ли ядро на своём порту учёта прямо сейчас.
fn порт_жив(порт: u16) -> bool {
    let адрес = std::net::SocketAddr::from(([127, 0, 0, 1], порт));
    std::net::TcpStream::connect_timeout(&адрес, std::time::Duration::from_millis(200)).is_ok()
}

// --- чтобы после нас сеть осталась целой -------------------------------

fn отметка() -> Option<PathBuf> {
    папка().ok().map(|п| п.join("tunnel.on"))
}

fn отметка_поставить() {
    if let Some(п) = отметка() {
        let _ = std::fs::write(п, "1");
    }
}

fn отметка_снять() {
    if let Some(п) = отметка() {
        let _ = std::fs::remove_file(п);
    }
}

/// Приложение в прошлый раз закрылось с поднятым туннелем — значит в
/// системе могли остаться наши маршруты. Вызывается при запуске.
pub fn вылечить_после_падения() -> bool {
    // Следы прошлого сеанса стираем при каждом запуске, упало приложение
    // или нет: в конфигурации лежат ключи подписки, в журнале — работа
    // ядра. Пережить перезапуск они не должны ни в каком случае.
    if let Ok(п) = папка() {
        let _ = std::fs::remove_file(п.join("current.json"));
    }

    let Some(п) = отметка() else { return false };
    if !п.exists() {
        return false;
    }
    прибить_чужие_ядра();
    прибрать_за_собой();
    отметка_снять();
    true
}

/// Ядро, оставшееся от прошлого сеанса, держит адаптер и маршруты.
///
/// Убиваем только своё — то, что запущено из нашей установки. Имя
/// `xray.exe` носят ядра и других VPN-клиентов; снять их все разом значило
/// бы молча оборвать человеку чужую программу.
#[cfg(windows)]
fn прибить_чужие_ядра() {
    use std::os::windows::process::CommandExt;
    let Ok(мы) = std::env::current_exe() else { return };
    let Some(папка_установки) = мы.parent() else { return };
    let скрипт = format!(
        "Get-Process xray -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -like '{}*' }} | Stop-Process -Force",
        папка_установки.display().to_string().replace('\'', "''")
    );
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &скрипт])
        .creation_flags(0x08000000)
        .output();
}

#[cfg(not(windows))]
fn прибить_чужие_ядра() {}

/// Снимаем маршруты нашего адаптера и чистим кэш DNS.
///
/// Делаем это даже когда ядро закрылось само: лишняя уборка безвредна,
/// а недоубранный маршрут оставляет человека без интернета — и он идёт
/// не в поддержку, а удалять приложение.
#[cfg(windows)]
fn прибрать_за_собой() {
    use std::os::windows::process::CommandExt;
    let скрипт = "\
$ад = Get-NetAdapter -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'CryVPN*' -or $_.InterfaceDescription -like '*CryVPN*' };
foreach ($а in $ад) {
  Get-NetRoute -InterfaceIndex $а.ifIndex -ErrorAction SilentlyContinue | Remove-NetRoute -Confirm:$false -ErrorAction SilentlyContinue
}
ipconfig /flushdns | Out-Null";

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", скрипт])
        .creation_flags(0x08000000)
        .output();
}

#[cfg(not(windows))]
fn прибрать_за_собой() {}

async fn дождаться_порта(порт: u16, попыток: u32) -> bool {
    for _ in 0..попыток {
        if tokio::net::TcpStream::connect(("127.0.0.1", порт)).await.is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    false
}

/// Последние строки журнала ядра — чтобы сказать человеку причину, а не
/// общее «не удалось подключиться».
fn последние_строки(журнал: &PathBuf) -> Option<String> {
    let текст = std::fs::read_to_string(журнал).ok()?;
    let хвост: Vec<&str> = текст.lines().rev().take(3).collect();
    let собрано = хвост.into_iter().rev().collect::<Vec<_>>().join("; ");
    (!собрано.trim().is_empty()).then_some(собрано)
}
