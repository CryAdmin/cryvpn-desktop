//! Обычная сеть компьютера — та, что была до туннеля.

use std::net::Ipv4Addr;

/// Адрес компьютера в обычной сети (Wi-Fi или провод).
///
/// Нужен замеру задержки: соединение, привязанное к этому адресу, Windows
/// отправляет через обычный адаптер, даже когда поднят туннель. Без
/// привязки замер уходит в наш же туннель, тот отвечает сам себе за
/// доли миллисекунды, и у всех серверов выходит ноль.
#[cfg(windows)]
pub fn обычный_адрес() -> Option<Ipv4Addr> {
    use std::os::windows::process::CommandExt;
    // Имена переменных латиницей — см. core::чужой_vpn.
    let скрипт = "\
$phys = @(Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object Status -eq 'Up' | ForEach-Object { $_.ifIndex });
$r = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -AddressFamily IPv4 -ErrorAction SilentlyContinue | Where-Object { $phys -contains $_.ifIndex } | Sort-Object { $_.RouteMetric + $_.InterfaceMetric } | Select-Object -First 1;
if ($r) { (Get-NetIPAddress -AddressFamily IPv4 -InterfaceIndex $r.ifIndex -ErrorAction SilentlyContinue | Select-Object -First 1).IPAddress }";
    let вывод = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", скрипт])
        .creation_flags(0x08000000)
        .output()
        .ok()?;
    String::from_utf8_lossy(&вывод.stdout).lines().next()?.trim().parse().ok()
}

#[cfg(not(windows))]
pub fn обычный_адрес() -> Option<Ipv4Addr> {
    None
}
