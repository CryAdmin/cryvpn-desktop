//! Адреса серверов из подписки узнаём сами, до подключения.
//!
//! Зачем. Пока VPN работает, DNS на Windows ходит только через туннель
//! (заслон в killswitch.rs). А ядру, чтобы открыть новое соединение с
//! сервером, нужен адрес этого сервера. Если ядро спросит его у Windows,
//! вопрос уйдёт в туннель, который держит то же ядро, — получится кольцо:
//! чтобы узнать адрес сервера, нужен сервер. Раньше кольцо разрывал роутер,
//! которому Windows задавала тот же вопрос параллельно, — это и была утечка.
//!
//! Поэтому адрес узнаём заранее, пока туннеля ещё нет, и вписываем его
//! прямо в конфигурацию ядра. Ядро берёт его оттуда и Windows не
//! спрашивает. Имя сервера при этом остаётся прежним — шифрование и
//! проверка сертификата идут по имени, как и раньше (проверено на Xray
//! 26.11.9: соединение уходит на вписанный адрес, в TLS — прежнее имя).

use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::time::Duration;

use serde_json::{json, Value};

/// Открытые DNS, которые спрашиваем своим вопросом, если Windows не
/// ответила. Так бывает при защите от утечки: VPN оборвался, интернет
/// закрыт всем, кроме самого приложения, — а для переподключения нужен
/// адрес сервера.
const ОТКРЫТЫЕ_DNS: [Ipv4Addr; 3] = [
    Ipv4Addr::new(77, 88, 8, 8),
    Ipv4Addr::new(8, 8, 8, 8),
    Ipv4Addr::new(1, 1, 1, 1),
];

/// Где в выходе записан адрес сервера: `settings.vnext[]`, `settings.servers[]`
/// или прямо `settings.address` (новая запись VLESS).
fn записи_серверов(выход: &mut Value) -> Vec<&mut Value> {
    let Some(настройки) = выход.get_mut("settings") else {
        return Vec::new();
    };
    if настройки.get("address").is_some() {
        return vec![настройки];
    }
    let mut записи = Vec::new();
    if let Some(настройки) = настройки.as_object_mut() {
        for (ключ, список) in настройки.iter_mut() {
            if ключ == "vnext" || ключ == "servers" {
                if let Some(список) = список.as_array_mut() {
                    записи.extend(список.iter_mut());
                }
            }
        }
    }
    записи
}

/// Имя сервера, если записан не адрес, а имя.
fn имя_сервера(запись: &Value) -> Option<String> {
    let адрес = запись.get("address")?.as_str()?.trim();
    if адрес.is_empty() || адрес.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    Some(адрес.to_ascii_lowercase())
}

/// Имена серверов во всех выходах конфигурации.
pub fn имена(конфиг: &Value) -> Vec<String> {
    let mut копия = конфиг.clone();
    let mut имена: Vec<String> = Vec::new();
    for выход in копия.get_mut("outbounds").and_then(|в| в.as_array_mut()).into_iter().flatten() {
        for запись in записи_серверов(выход) {
            if let Some(имя) = имя_сервера(запись) {
                if !имена.contains(&имя) {
                    имена.push(имя);
                }
            }
        }
    }
    имена
}

/// Вписать узнанные адреса: таблица имён ядра и указание выходам брать
/// адрес из неё, а не у Windows.
pub fn вписать(конфиг: &mut Value, адреса: &BTreeMap<String, Vec<Ipv4Addr>>) {
    if адреса.is_empty() {
        return;
    }
    if let Some(выходы) = конфиг.get_mut("outbounds").and_then(|в| в.as_array_mut()) {
        for выход in выходы {
            let есть_наш = записи_серверов(выход)
                .into_iter()
                .any(|з| имя_сервера(з).is_some_and(|и| адреса.contains_key(&и)));
            if !есть_наш {
                continue;
            }
            if !выход.get("streamSettings").is_some_and(|с| с.is_object()) {
                выход["streamSettings"] = json!({});
            }
            let поток = &mut выход["streamSettings"];
            if !поток.get("sockopt").is_some_and(|с| с.is_object()) {
                поток["sockopt"] = json!({});
            }
            let своё = поток["sockopt"].get("domainStrategy").and_then(|с| с.as_str()).unwrap_or("AsIs");
            // Своё правило подписки, если оно уже велит спрашивать ядро, не
            // трогаем. «Как есть» означает «спросить Windows» — его и меняем.
            if своё.eq_ignore_ascii_case("AsIs") {
                поток["sockopt"]["domainStrategy"] = json!("UseIPv4");
            }
        }
    }
    if !конфиг.get("dns").is_some_and(|д| д.is_object()) {
        конфиг["dns"] = json!({});
    }
    if !конфиг["dns"].get("hosts").is_some_and(|таблица| таблица.is_object()) {
        конфиг["dns"]["hosts"] = json!({});
    }
    for (имя, список) in адреса {
        let ключ = format!("full:{имя}");
        // Свою запись подписки не перебиваем.
        if конфиг["dns"]["hosts"].get(&ключ).is_none() && конфиг["dns"]["hosts"].get(имя).is_none() {
            конфиг["dns"]["hosts"][ключ] = json!(список.iter().map(|а| а.to_string()).collect::<Vec<_>>());
        }
    }
}

/// Узнать IPv4-адреса имени. Сначала обычным путём через Windows, вместе
/// с ним — своим вопросом к открытым DNS: какой ответ придёт первым, тот и
/// берём. Не ответил никто за 4 секунды — пусто, ядро спросит само.
pub async fn узнать(имя: &str) -> Vec<Ipv4Addr> {
    use tokio::task::JoinSet;
    let mut задачи: JoinSet<Vec<Ipv4Addr>> = JoinSet::new();
    let своё = имя.to_string();
    задачи.spawn(async move {
        match tokio::net::lookup_host((своё.as_str(), 443)).await {
            Ok(адреса) => адреса
                .filter_map(|а| match а.ip() {
                    std::net::IpAddr::V4(в4) => Some(в4),
                    _ => None,
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    });
    for сервер in ОТКРЫТЫЕ_DNS {
        let своё = имя.to_string();
        задачи.spawn(async move { спросить(сервер, &своё).await.unwrap_or_default() });
    }
    let срок = tokio::time::sleep(Duration::from_secs(4));
    tokio::pin!(срок);
    loop {
        tokio::select! {
            _ = &mut срок => return Vec::new(),
            готово = задачи.join_next() => match готово {
                None => return Vec::new(),
                Some(Ok(адреса)) if !адреса.is_empty() => {
                    задачи.abort_all();
                    return адреса;
                }
                Some(_) => {}
            }
        }
    }
}

/// Вопрос «какой IPv4 у имени» прямо к DNS-серверу, без службы Windows.
pub(crate) async fn спросить(сервер: Ipv4Addr, имя: &str) -> Option<Vec<Ipv4Addr>> {
    let сокет = tokio::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    сокет.connect((сервер, 53)).await.ok()?;
    let номер = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .subsec_nanos()
        ^ u32::from(сервер)) as u16;
    сокет.send(&вопрос(номер, имя)?).await.ok()?;
    let mut ответ = [0u8; 1500];
    let длина = tokio::time::timeout(Duration::from_secs(3), сокет.recv(&mut ответ)).await.ok()?.ok()?;
    разобрать(&ответ[..длина], номер)
}

/// Вопрос DNS: заголовок, имя по частям, тип A, класс IN.
fn вопрос(номер: u16, имя: &str) -> Option<Vec<u8>> {
    let mut п = Vec::with_capacity(64);
    п.extend_from_slice(&номер.to_be_bytes());
    п.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // рекурсия, один вопрос
    for часть in имя.trim_end_matches('.').split('.') {
        if часть.is_empty() || часть.len() > 63 || !часть.is_ascii() {
            return None;
        }
        п.push(часть.len() as u8);
        п.extend_from_slice(часть.as_bytes());
    }
    п.push(0);
    п.extend_from_slice(&[0, 1, 0, 1]);
    Some(п)
}

/// Пропустить имя в ответе: по частям или ссылкой на уже названное.
fn пропустить_имя(д: &[u8], mut и: usize) -> Option<usize> {
    loop {
        let длина = *д.get(и)? as usize;
        if длина == 0 {
            return Some(и + 1);
        }
        if длина & 0xC0 == 0xC0 {
            return Some(и + 2);
        }
        и += 1 + длина;
    }
}

/// Достать из ответа адреса IPv4. Чужой или битый ответ — ничего.
fn разобрать(д: &[u8], номер: u16) -> Option<Vec<Ipv4Addr>> {
    if д.len() < 12 || u16::from_be_bytes([д[0], д[1]]) != номер {
        return None;
    }
    // Это ответ, и без ошибки.
    if д[2] & 0x80 == 0 || д[3] & 0x0F != 0 {
        return None;
    }
    let вопросов = u16::from_be_bytes([д[4], д[5]]) as usize;
    let ответов = u16::from_be_bytes([д[6], д[7]]) as usize;
    let mut и = 12;
    for _ in 0..вопросов {
        и = пропустить_имя(д, и)? + 4;
    }
    let mut адреса = Vec::new();
    for _ in 0..ответов {
        и = пропустить_имя(д, и)?;
        let шапка = д.get(и..и + 10)?;
        let тип = u16::from_be_bytes([шапка[0], шапка[1]]);
        let длина = u16::from_be_bytes([шапка[8], шапка[9]]) as usize;
        и += 10;
        let данные = д.get(и..и + длина)?;
        if тип == 1 && длина == 4 {
            адреса.push(Ipv4Addr::new(данные[0], данные[1], данные[2], данные[3]));
        }
        и += длина;
    }
    (!адреса.is_empty()).then_some(адреса)
}

#[cfg(test)]
mod проверки {
    use super::*;

    #[test]
    fn имена_только_доменные_и_без_повторов() {
        let конфиг = json!({"outbounds": [
            {"protocol": "vless", "settings": {"vnext": [{"address": "Node.Example.com", "port": 443}]}},
            {"protocol": "vless", "settings": {"vnext": [{"address": "1.2.3.4", "port": 443}]}},
            {"protocol": "trojan", "settings": {"servers": [{"address": "node.example.com", "port": 443}]}},
            {"protocol": "vless", "settings": {"address": "flat.example.net", "port": 443}},
            {"protocol": "freedom", "settings": {}},
            {"protocol": "blackhole"}
        ]});
        assert_eq!(имена(&конфиг), vec!["node.example.com", "flat.example.net"]);
    }

    #[test]
    fn вписать_ставит_таблицу_и_правило_выходу() {
        let mut конфиг = json!({"outbounds": [
            {"protocol": "vless", "settings": {"vnext": [{"address": "node.example.com", "port": 443}]},
             "streamSettings": {"network": "tcp", "security": "reality"}},
            {"protocol": "freedom", "tag": "direct"},
            {"protocol": "vless", "settings": {"vnext": [{"address": "other.example.com", "port": 443}]},
             "streamSettings": {"sockopt": {"domainStrategy": "UseIPv6v4"}}}
        ], "dns": {"hosts": {"full:other.example.com": "9.9.9.9"}}});
        let mut адреса = BTreeMap::new();
        адреса.insert("node.example.com".to_string(), vec![Ipv4Addr::new(5, 6, 7, 8)]);
        адреса.insert("other.example.com".to_string(), vec![Ipv4Addr::new(1, 1, 1, 1)]);
        вписать(&mut конфиг, &адреса);
        assert_eq!(конфиг["outbounds"][0]["streamSettings"]["sockopt"]["domainStrategy"], "UseIPv4");
        assert_eq!(конфиг["outbounds"][0]["streamSettings"]["security"], "reality");
        assert!(конфиг["outbounds"][1].get("streamSettings").is_none());
        // Своё правило и своя запись подписки остаются.
        assert_eq!(конфиг["outbounds"][2]["streamSettings"]["sockopt"]["domainStrategy"], "UseIPv6v4");
        assert_eq!(конфиг["dns"]["hosts"]["full:other.example.com"], "9.9.9.9");
        assert_eq!(конфиг["dns"]["hosts"]["full:node.example.com"], json!(["5.6.7.8"]));
    }

    #[test]
    fn пустые_адреса_ничего_не_меняют() {
        let исходный = json!({"outbounds": [{"protocol": "vless", "settings": {"vnext": [{"address": "a.b", "port": 1}]}}]});
        let mut конфиг = исходный.clone();
        вписать(&mut конфиг, &BTreeMap::new());
        assert_eq!(конфиг, исходный);
    }

    #[test]
    fn вопрос_и_разбор_ответа() {
        let в = вопрос(0x1234, "a.bc").unwrap();
        assert_eq!(&в[..2], &[0x12, 0x34]);
        assert_eq!(&в[12..], &[1, b'a', 2, b'b', b'c', 0, 0, 1, 0, 1]);
        // Ответ: тот же вопрос, CNAME ссылкой на имя и две записи A.
        let mut ответ = пример_ответа();
        ответ[0] = 0x12;
        ответ[1] = 0x34;
        assert_eq!(
            разобрать(&ответ, 0x1234).unwrap(),
            vec![Ipv4Addr::new(10, 0, 0, 1), Ipv4Addr::new(10, 0, 0, 2)]
        );
        // Чужой номер — не наш ответ.
        assert!(разобрать(&ответ, 0x9999).is_none());
        // Обрезанный на любом месте — не падаем.
        for длина in 0..ответ.len() {
            let _ = разобрать(&ответ[..длина], 0x1234);
        }
    }

    fn пример_ответа() -> Vec<u8> {
        let mut ответ = vec![0, 0, 0x81, 0x80, 0, 1, 0, 3, 0, 0, 0, 0];
        ответ.extend_from_slice(&[1, b'a', 2, b'b', b'c', 0, 0, 1, 0, 1]);
        // CNAME (тип 5) — пропускается.
        ответ.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 2, 0xC0, 12]);
        ответ.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 1]);
        ответ.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 0, 0, 2]);
        ответ
    }

    /// Живая сеть: своим вопросом узнаём адрес известного имени.
    /// `cargo test --lib -- --ignored узнаём_адрес_сами`
    #[test]
    #[ignore]
    fn узнаём_адрес_сами() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let адреса = rt.block_on(спросить(Ipv4Addr::new(8, 8, 8, 8), "one.one.one.one")).unwrap();
        assert!(адреса.contains(&Ipv4Addr::new(1, 1, 1, 1)), "{адреса:?}");
    }
}
