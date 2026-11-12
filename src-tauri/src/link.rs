//! Ссылки вида `cryvpn://add/https://sub.cryvps.com/<код>`.
//!
//! По такой ссылке со страницы подписки приложение получает подписку
//! человека и пускает его к серверам без почты и кода.
//!
//! Разбор намеренно строгий. Приложение запускает ядро с правами
//! администратора на той конфигурации, которую отдаст адрес из ссылки.
//! Прими мы любой адрес — и достаточно было бы подсунуть человеку ссылку
//! на чужой сервер, чтобы весь его трафик пошёл через посторонних. Поэтому
//! только наши домены, только https, только короткий код в пути.

/// Откуда разрешено брать подписку. Появится новый домен — добавлять сюда.
const НАШИ_ДОМЕНЫ: [&str; 2] = ["sub.cryvps.com", "crysubik.com"];

const НАЧАЛО: &str = "cryvpn://add/";

const НАЧАЛО_ВХОДА: &str = "cryvpn://login/";

/// Ссылка входа `cryvpn://login/<токен>`: страница подписки, открытая
/// самим человеком, выдаёт одноразовый токен, и приложение по нему входит
/// в кабинет. Сам токен ничего не открывает, пока кабинет его не признает,
/// поэтому здесь только проверяем вид: 43 знака base64url и ничего больше.
pub fn разобрать_вход(сырая: &str) -> Option<String> {
    let сырая = сырая.trim();
    if !сырая.to_ascii_lowercase().starts_with(НАЧАЛО_ВХОДА) {
        return None;
    }
    let токен = сырая[НАЧАЛО_ВХОДА.len()..].trim_end_matches('/');
    let годен = токен.len() == 43
        && токен.chars().all(|с| с.is_ascii_alphanumeric() || с == '-' || с == '_');
    годен.then(|| токен.to_string())
}

/// Возвращает адрес подписки или `None`, если ссылке нельзя доверять.
pub fn разобрать(сырая: &str) -> Option<String> {
    let сырая = сырая.trim();
    if сырая.len() > 300 || !сырая.to_ascii_lowercase().starts_with(НАЧАЛО) {
        return None;
    }
    let mut остаток = сырая[НАЧАЛО.len()..].to_string();

    // Некоторые браузеры съедают одну косую черту: `https:/sub…`.
    if let Some(хвост) = остаток.strip_prefix("https:/") {
        if !хвост.starts_with('/') {
            остаток = format!("https://{хвост}");
        }
    }
    // А Windows любит дописать косую черту в конец.
    let остаток = остаток.trim_end_matches('/');

    let адрес = reqwest::Url::parse(остаток).ok()?;
    if адрес.scheme() != "https"
        || адрес.port().is_some()
        || !адрес.username().is_empty()
        || адрес.password().is_some()
        || адрес.query().is_some()
        || адрес.fragment().is_some()
    {
        return None;
    }

    let домен = адрес.host_str()?;
    if !НАШИ_ДОМЕНЫ.contains(&домен) {
        return None;
    }

    // В пути ровно один кусок — короткий код подписки.
    let код = адрес.path().trim_matches('/');
    let годен = (6..=64).contains(&код.len())
        && код.chars().all(|с| с.is_ascii_alphanumeric() || с == '_' || с == '-');
    if !годен {
        return None;
    }

    Some(format!("https://{домен}/{код}"))
}

#[cfg(test)]
mod проверки {
    #[test]
    fn ссылка_входа() {
        let токен = "A".repeat(43);
        assert_eq!(super::разобрать_вход(&format!("cryvpn://login/{токен}")), Some(токен.clone()));
        assert_eq!(super::разобрать_вход(&format!("cryvpn://login/{токен}/")), Some(токен.clone()));
        assert_eq!(super::разобрать_вход("cryvpn://login/short"), None);
        assert_eq!(super::разобрать_вход(&format!("cryvpn://login/{}", "A".repeat(44))), None);
        assert_eq!(super::разобрать_вход(&format!("cryvpn://login/{}?x=1", "A".repeat(41))), None);
        assert_eq!(super::разобрать_вход(&format!("cryvpn://add/{токен}")), None);
    }

    use super::разобрать;

    #[test]
    fn обычная_ссылка() {
        assert_eq!(
            разобрать("cryvpn://add/https://sub.cryvps.com/AbC_123-xyz").as_deref(),
            Some("https://sub.cryvps.com/AbC_123-xyz")
        );
    }

    #[test]
    fn старый_домен_тоже_наш() {
        assert_eq!(
            разобрать("cryvpn://add/https://crysubik.com/AbCdEf12").as_deref(),
            Some("https://crysubik.com/AbCdEf12")
        );
    }

    #[test]
    fn съеденная_косая_и_хвост() {
        assert_eq!(
            разобрать("cryvpn://add/https:/sub.cryvps.com/AbCdEf12/").as_deref(),
            Some("https://sub.cryvps.com/AbCdEf12")
        );
    }

    #[test]
    fn чужой_сервер_не_пускаем() {
        for плохая in [
            "cryvpn://add/https://evil.example/AbCdEf12",
            "cryvpn://add/https://sub.cryvps.com.evil.example/AbCdEf12",
            "cryvpn://add/https://evil.example/sub.cryvps.com/AbCdEf12",
            "cryvpn://add/https://sub.cryvps.com@evil.example/AbCdEf12",
            "cryvpn://add/https://user:pw@sub.cryvps.com/AbCdEf12",
        ] {
            assert_eq!(разобрать(плохая), None, "пропустили: {плохая}");
        }
    }

    #[test]
    fn только_https_и_без_лишнего() {
        for плохая in [
            "cryvpn://add/http://sub.cryvps.com/AbCdEf12",
            "cryvpn://add/https://sub.cryvps.com:8443/AbCdEf12",
            "cryvpn://add/https://sub.cryvps.com/AbCdEf12?x=1",
            "cryvpn://add/https://sub.cryvps.com/AbCdEf12#y",
            "cryvpn://add/https://sub.cryvps.com/a/b",
            "cryvpn://add/https://sub.cryvps.com/../etc",
            "cryvpn://add/https://sub.cryvps.com/",
            "cryvpn://add/",
            "cryvpn://other/https://sub.cryvps.com/AbCdEf12",
            "https://sub.cryvps.com/AbCdEf12",
        ] {
            assert_eq!(разобрать(плохая), None, "пропустили: {плохая}");
        }
    }
}
