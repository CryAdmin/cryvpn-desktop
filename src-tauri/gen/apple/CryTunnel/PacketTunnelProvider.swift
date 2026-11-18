// Расширение VPN для iPhone: здесь работает ядро Xray.
//
// iOS запускает расширение, когда приложение просит подключиться (или
// когда VPN включают в Настройках), и держит его живым, пока VPN включён,
// даже если окно приложения закрыто. Порядок такой:
//   1. Говорим iOS, какой у туннеля адрес, маршруты и DNS: весь интернет,
//      IPv4 и IPv6, идёт в туннель.
//   2. Находим туннель, который iOS для нас открыла, — его номер файла.
//   3. Запускаем ядро и отдаём ему этот номер: дальше трафик идёт через
//      Xray прямо, без нашего участия.
// Соединения самого расширения в туннель не попадают никогда — так iOS
// устроена, поэтому ядро ходит к серверу напрямую, без кольца.

import Foundation
import LibXray
import NetworkExtension

/// Ошибка с понятным текстом: iOS покажет его приложению.
private func беда(_ текст: String) -> NSError {
  NSError(domain: "CryVPN", code: 1, userInfo: [NSLocalizedDescriptionKey: текст])
}

class PacketTunnelProvider: NEPacketTunnelProvider {
  override func startTunnel(
    options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void
  ) {
    let протокол = protocolConfiguration as? NETunnelProviderProtocol
    let настройки = протокол?.providerConfiguration ?? [:]
    // Свежая конфигурация приходит с просьбой подключиться. Если VPN
    // включили из Настроек — берём ту, что сохранили при прошлом разе.
    guard
      let конфиг = (options?["config"] as? String) ?? (настройки["config"] as? String),
      !конфиг.isEmpty
    else {
      completionHandler(беда("Откройте CryVPN и подключитесь из приложения"))
      return
    }
    var днс = ((настройки["dns"] as? String) ?? "")
      .split(separator: ",")
      .map { $0.trimmingCharacters(in: .whitespaces) }
      .filter { !$0.isEmpty }
    if днс.isEmpty { днс = ["1.1.1.1", "8.8.8.8"] }

    // Адреса туннеля — те же, что у компьютерной версии (198.19.77.0/30):
    // в живых сетях этот диапазон не встречается.
    let сеть = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "198.19.77.1")
    let в4 = NEIPv4Settings(addresses: ["198.19.77.2"], subnetMasks: ["255.255.255.252"])
    в4.includedRoutes = [NEIPv4Route.default()]
    сеть.ipv4Settings = в4
    // IPv6 тоже заворачиваем в туннель: иначе он шёл бы мимо VPN и выдавал
    // настоящий адрес. Ядро IPv6 не возит — сайты переходят на IPv4.
    let в6 = NEIPv6Settings(addresses: ["fd77:7777::2"], networkPrefixLengths: [126])
    в6.includedRoutes = [NEIPv6Route.default()]
    сеть.ipv6Settings = в6
    // Все DNS-запросы — через туннель, на выбранные в приложении адреса.
    let запросы = NEDNSSettings(servers: днс)
    запросы.matchDomains = [""]
    сеть.dnsSettings = запросы
    сеть.mtu = 1500

    setTunnelNetworkSettings(сеть) { ошибка in
      if let ошибка {
        completionHandler(ошибка)
        return
      }
      guard let номер = self.номерТуннеля() else {
        completionHandler(беда("iOS не отдала туннель. Попробуйте подключиться ещё раз"))
        return
      }
      if let причина = self.запуститьЯдро(конфиг, туннель: номер) {
        completionHandler(беда(причина))
        return
      }
      completionHandler(nil)
    }
  }

  override func stopTunnel(
    with reason: NEProviderStopReason, completionHandler: @escaping () -> Void
  ) {
    _ = позвать(["apiVersion": 3, "method": "stopXray"])
    completionHandler()
  }

  // MARK: - ядро

  /// Запустить ядро. nil — работает, иначе причина отказа.
  private func запуститьЯдро(_ конфиг: String, туннель: Int32) -> String? {
    guard
      let данные = конфиг.data(using: .utf8),
      var json = (try? JSONSerialization.jsonObject(with: данные)) as? [String: Any]
    else {
      return "Этот сервер в подписке записан неверно. Обновите подписку"
    }
    // Ядро читает настройки окружения из самой конфигурации: номер
    // туннеля и папку с базами geoip/geosite из этого расширения.
    var окружение = json["env"] as? [String: String] ?? [:]
    окружение["XRAY_TUN_FD"] = String(туннель)
    if let базы = Bundle.main.resourceURL?.appendingPathComponent("geo").path {
      окружение["XRAY_LOCATION_ASSET"] = базы
    }
    json["env"] = окружение
    guard
      let готовые = try? JSONSerialization.data(withJSONObject: json),
      let текст = String(data: готовые, encoding: .utf8)
    else {
      return "Не удалось подготовить конфигурацию"
    }
    let ответ = позвать([
      "apiVersion": 3,
      "method": "runXray",
      "payload": ["xrayJson": текст],
    ])
    if ответ["success"] as? Bool == true { return nil }
    let причина = (ответ["error"] as? String) ?? "неизвестная ошибка"
    return "Ядро не запустилось: \(причина)"
  }

  /// Вызов libXray: запрос и ответ — JSON.
  private func позвать(_ запрос: [String: Any]) -> [String: Any] {
    guard
      let данные = try? JSONSerialization.data(withJSONObject: запрос),
      let текст = String(data: данные, encoding: .utf8),
      let вход = strdup(текст)
    else { return [:] }
    defer { free(вход) }
    guard let выход = CGoInvoke(вход) else { return [:] }
    defer { CGoFree(выход) }
    let ответ = String(cString: выход)
    return ((try? JSONSerialization.jsonObject(with: Data(ответ.utf8))) as? [String: Any]) ?? [:]
  }

  // MARK: - туннель

  /// Номер файла туннеля (utun), который iOS открыла для нас.
  ///
  /// Прямого способа узнать его iOS не даёт. Перебираем открытые файлы и
  /// спрашиваем у каждого имя сетевого интерфейса: у туннеля оно «utunN».
  /// Так делают WireGuard и другие клиенты.
  private func номерТуннеля() -> Int32? {
    let SYSPROTO_CONTROL: Int32 = 2
    let UTUN_OPT_IFNAME: Int32 = 2
    var имя = [CChar](repeating: 0, count: Int(IFNAMSIZ))
    for номер: Int32 in 0...1024 {
      var длина = socklen_t(имя.count)
      if getsockopt(номер, SYSPROTO_CONTROL, UTUN_OPT_IFNAME, &имя, &длина) == 0,
        String(cString: имя).hasPrefix("utun")
      {
        return номер
      }
    }
    return nil
  }
}
