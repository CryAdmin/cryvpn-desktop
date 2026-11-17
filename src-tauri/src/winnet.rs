//! Сведения о сети Windows прямым запросом к системе.
//!
//! Раньше то же самое узнавали через PowerShell. Каждый его запуск — это
//! старт большой программы: десятки мегабайт памяти и заметная доля
//! процессора. А проверка «не включён ли другой VPN» идёт всё время, пока
//! работает туннель. 14.11.2026 человек прислал диспетчер задач, где
//! PowerShell занимал 14% процессора. Здесь тот же вопрос задаётся системе
//! напрямую, без запуска программ.

use windows_sys::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIfEntry2, GetIpForwardTable2, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH, MIB_IF_ROW2,
    MIB_IPFORWARD_TABLE2,
};
use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
use windows_sys::Win32::Networking::WinSock::AF_INET;

/// Сетевой адаптер: то немногое, что нам о нём нужно.
struct Адаптер {
    номер: u32,
    имя: String,
    описание: String,
    вид: u32,
    работает: bool,
}

/// Строка Windows (UTF-16 с нулём на конце) → обычная строка.
///
/// # Safety
/// Указатель либо пуст, либо ведёт на строку, завершённую нулём.
unsafe fn строка(указатель: *const u16) -> String {
    if указатель.is_null() {
        return String::new();
    }
    let mut длина = 0usize;
    // Потолок — от испорченной памяти: имён адаптеров такой длины не бывает.
    while длина < 1024 && unsafe { *указатель.add(длина) } != 0 {
        длина += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(указатель, длина) })
}

fn адаптеры() -> Vec<Адаптер> {
    let флаги = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    // Система сама говорит, сколько места ей нужно; между двумя вызовами
    // список мог вырасти, поэтому пробуем несколько раз.
    let mut размер: u32 = 16 * 1024;
    for _ in 0..4 {
        // u64 — чтобы память была выровнена так, как ждёт система.
        let mut место = vec![0u64; (размер as usize).div_ceil(8)];
        let первый = место.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
        // SAFETY: место выделено нами и его размер передан верно.
        let код = unsafe {
            GetAdaptersAddresses(AF_INET as u32, флаги, std::ptr::null(), первый, &mut размер)
        };
        if код == 111 {
            continue; // ERROR_BUFFER_OVERFLOW: размер уже подсказан
        }
        if код != 0 {
            return Vec::new();
        }
        let mut итог = Vec::new();
        let mut текущий = первый.cast_const();
        while !текущий.is_null() {
            // SAFETY: система заполнила связный список внутри нашего места.
            let а = unsafe { &*текущий };
            итог.push(Адаптер {
                номер: unsafe { а.Anonymous1.Anonymous.IfIndex },
                имя: unsafe { строка(а.FriendlyName) },
                описание: unsafe { строка(а.Description) },
                вид: а.IfType,
                работает: а.OperStatus == IfOperStatusUp,
            });
            текущий = а.Next;
        }
        return итог;
    }
    Vec::new()
}

/// Номера адаптеров, через которые идёт «весь интернет»: маршрут по
/// умолчанию или пара половинок, которой его подменяют VPN-клиенты.
fn адаптеры_всего_трафика() -> Vec<u32> {
    let mut таблица: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    // SAFETY: система выделяет таблицу сама и отдаёт указатель на неё.
    if unsafe { GetIpForwardTable2(AF_INET, &mut таблица) } != 0 || таблица.is_null() {
        return Vec::new();
    }
    let mut итог = Vec::new();
    // SAFETY: в таблице NumEntries строк подряд, как обещает система.
    unsafe {
        let число = (*таблица).NumEntries as usize;
        let строки = std::slice::from_raw_parts((*таблица).Table.as_ptr(), число);
        for с in строки {
            let длина = с.DestinationPrefix.PrefixLength;
            let адрес = u32::from_be(с.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr);
            // 0.0.0.0/0, 0.0.0.0/1 и 128.0.0.0/1
            let весь = (длина == 0 && адрес == 0)
                || (длина == 1 && (адрес == 0 || адрес == 0x8000_0000));
            if весь && !итог.contains(&с.InterfaceIndex) {
                итог.push(с.InterfaceIndex);
            }
        }
        FreeMibTable(таблица.cast());
    }
    итог
}

/// Номера адаптеров, через которые Windows на самом деле отправит пакет в
/// интернет. Проверяем несколько известных адресов в разных сетях.
///
/// Одного маршрута «в весь интернет» мало. Radmin VPN держит свою карту
/// включённой, даже когда выключен сам, и прописывает через неё маршрут
/// 0.0.0.0/0 с огромной метрикой. Трафик по такому маршруту не идёт —
/// обычный интернет всегда выигрывает, — а мы принимали его за включённый
/// чужой VPN (17.11.2026, жалоба тестера). Какой маршрут выигрывает, знает
/// только система: ей и задаём вопрос.
fn адаптеры_интернета() -> Vec<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetBestRoute2, MIB_IPFORWARD_ROW2};
    use windows_sys::Win32::Networking::WinSock::SOCKADDR_INET;

    const АДРЕСА: [[u8; 4]; 3] = [[1, 1, 1, 1], [8, 8, 8, 8], [77, 88, 8, 8]];
    let mut итог = Vec::new();
    for адрес in АДРЕСА {
        // SAFETY: обе структуры — числа и объединения чисел, нули для них
        // допустимы; заполняем семейство и адрес, остальное — система.
        let mut куда: SOCKADDR_INET = unsafe { std::mem::zeroed() };
        let mut маршрут: MIB_IPFORWARD_ROW2 = unsafe { std::mem::zeroed() };
        let mut откуда: SOCKADDR_INET = unsafe { std::mem::zeroed() };
        unsafe {
            куда.Ipv4.sin_family = AF_INET;
            куда.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(адрес);
        }
        // SAFETY: указатели ведут на наши живые переменные; адаптер не
        // задаём (null и 0) — система выбирает сама, как для любого пакета.
        let код = unsafe {
            GetBestRoute2(
                std::ptr::null(),
                0,
                std::ptr::null(),
                &куда,
                0,
                &mut маршрут,
                &mut откуда,
            )
        };
        if код == 0 && !итог.contains(&маршрут.InterfaceIndex) {
            итог.push(маршрут.InterfaceIndex);
        }
    }
    итог
}

/// Настоящее ли это «железо»: сетевая карта или Wi-Fi с разъёмом или
/// антенной. Маршрут по умолчанию через такой адаптер — обычный интернет,
/// а не чужой VPN, как бы ни звалась карта.
fn железный(номер: u32) -> bool {
    // SAFETY: запись состоит из чисел и массивов, нулевая — допустимое
    // начальное значение; система заполняет её по номеру адаптера.
    let mut запись: MIB_IF_ROW2 = unsafe { std::mem::zeroed() };
    запись.InterfaceIndex = номер;
    if unsafe { GetIfEntry2(&mut запись) } != 0 {
        return false;
    }
    // Третий признак в наборе — «есть разъём» (ConnectorPresent).
    запись.InterfaceAndOperStatusFlags._bitfield & 0b100 != 0
}

/// Похож ли адаптер на туннель VPN: по виду или по тому, как он назван.
fn туннельный(а: &Адаптер) -> bool {
    const ВИД_ТУННЕЛЯ: u32 = 53; // IF_TYPE_PROP_VIRTUAL
    let описание = а.описание.to_uppercase();
    а.вид == ВИД_ТУННЕЛЯ
        || ["TAP", "TUN", "VPN", "WIREGUARD"].iter().any(|слово| описание.contains(слово))
}

/// Название чужого VPN, если он сейчас забирает весь трафик.
pub fn чужой_vpn() -> Option<String> {
    let через = адаптеры_всего_трафика();
    if через.is_empty() {
        return None;
    }
    // Маршрут «в весь интернет» есть — но выигрывает ли он? Чужой VPN тот,
    // через чью карту система реально шлёт пакеты наружу.
    let выигрывают = адаптеры_интернета();
    адаптеры()
        .into_iter()
        .find(|а| {
            через.contains(&а.номер)
                && выигрывают.contains(&а.номер)
                && а.работает
                && !а.имя.starts_with("CryVPN")
                && !а.описание.contains("CryVPN")
                && туннельный(а)
                && !железный(а.номер)
        })
        .map(|а| {
            а.описание
                .trim()
                .trim_end_matches(" Tunnel")
                .trim_end_matches(" Adapter")
                .to_string()
        })
        .filter(|имя| !имя.is_empty())
}

#[cfg(test)]
mod проверки {
    /// Настоящий запрос к системе: главное, что он не падает и не виснет.
    /// На машине сборки чужого VPN нет, но обещать этого нельзя — поэтому
    /// сам ответ не сверяем.
    #[test]
    fn запрос_к_системе_не_падает() {
        assert!(!super::адаптеры().is_empty(), "у машины нет ни одного сетевого адаптера");
        assert!(!super::адаптеры_всего_трафика().is_empty(), "у машины нет маршрута в интернет");
        assert!(!super::адаптеры_интернета().is_empty(), "система не сказала, куда пойдёт пакет");
        // На машине сборки VPN не включён, зато есть виртуальные сетевые
        // карты. Принять одну из них за чужой VPN значило бы рвать людям
        // исправное подключение — именно это здесь и ловим.
        assert_eq!(super::чужой_vpn(), None);
    }
}
