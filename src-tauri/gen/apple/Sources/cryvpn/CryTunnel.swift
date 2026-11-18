// Управление VPN на iPhone со стороны приложения.
//
// Сам VPN — это расширение CryTunnel (папка CryTunnel рядом): iOS держит
// его работающим, даже когда окно закрыто, и в нём крутится ядро Xray.
// Отсюда мы только просим iOS включить или выключить его и следим за
// состоянием. Функции зовёт код на Rust (src-tauri/src/ios.rs) как
// обычные функции на C.

import Foundation
import NetworkExtension

private let расширение = "top.cryvpn.app.tunnel"

/// Всё общее состояние — под одним замком: зовут нас из разных потоков.
private final class Состояние {
  static let общее = Состояние()
  private let замок = NSLock()
  private var менеджер: NETunnelProviderManager?
  private var статус: Int32 = 0
  private var загружаем = false
  private var наблюдаем = false

  func взятьМенеджер() -> NETunnelProviderManager? {
    замок.lock(); defer { замок.unlock() }
    return менеджер
  }

  func запомнить(_ новый: NETunnelProviderManager?) {
    замок.lock()
    менеджер = новый
    замок.unlock()
    обновитьСтатус()
  }

  func статусСейчас() -> Int32 {
    замок.lock(); defer { замок.unlock() }
    return статус
  }

  /// Перечитать статус у запомненного менеджера.
  func обновитьСтатус() {
    замок.lock()
    let м = менеджер
    замок.unlock()
    let новый: Int32
    switch м?.connection.status {
    case .connected?: новый = 2
    case .connecting?, .reasserting?: новый = 1
    default: новый = 0
    }
    замок.lock()
    статус = новый
    замок.unlock()
  }

  /// Подписаться на перемены статуса — один раз за жизнь приложения.
  func наблюдать() {
    замок.lock()
    if наблюдаем { замок.unlock(); return }
    наблюдаем = true
    замок.unlock()
    NotificationCenter.default.addObserver(
      forName: .NEVPNStatusDidChange, object: nil, queue: nil
    ) { _ in
      Состояние.общее.обновитьСтатус()
    }
  }

  /// Загрузить наш менеджер в фоне, не дожидаясь ответа. Годится и для
  /// главного потока: ответ iOS придёт туда же, когда он освободится.
  func загрузитьВФоне() {
    замок.lock()
    if загружаем || менеджер != nil { замок.unlock(); return }
    загружаем = true
    замок.unlock()
    наблюдать()
    NETunnelProviderManager.loadAllFromPreferences { список, _ in
      self.замок.lock()
      self.загружаем = false
      self.замок.unlock()
      self.запомнить(список?.first(where: наш))
    }
  }
}

private func наш(_ м: NETunnelProviderManager) -> Bool {
  (м.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == расширение
}

/// Строка для Rust: выделяем strdup, освобождает cryvpn_ios_free.
private func вRust(_ текст: String) -> UnsafeMutablePointer<CChar>? {
  strdup(текст)
}

/// Дождаться ответа iOS. Только не из главного потока: ответ придёт туда.
private func дождаться<T>(_ дело: (@escaping (T) -> Void) -> Void) -> T {
  let сигнал = DispatchSemaphore(value: 0)
  var итог: T!
  дело { ответ in
    итог = ответ
    сигнал.signal()
  }
  сигнал.wait()
  return итог
}

/// Понятные слова вместо кодов ошибок iOS.
private func понятно(_ ошибка: Error) -> String {
  let ns = ошибка as NSError
  if ns.domain == NEVPNErrorDomain, let код = NEVPNError.Code(rawValue: ns.code) {
    switch код {
    case .configurationReadWriteFailed:
      return "🔒 Без разрешения на VPN подключиться нельзя. Нажмите «Подключить» и выберите «Разрешить»"
    case .configurationInvalid, .configurationDisabled:
      return "⚙️ iOS не приняла настройки VPN. Попробуйте подключиться ещё раз"
    case .connectionFailed:
      return "VPN не поднялся. Попробуйте ещё раз или другой сервер"
    default:
      break
    }
  }
  return ошибка.localizedDescription
}

@_cdecl("cryvpn_ios_free")
public func cryvpnIosFree(_ текст: UnsafeMutablePointer<CChar>?) {
  free(текст)
}

/// Включить VPN и дождаться, пока он заработает. nil — получилось,
/// иначе строка с причиной.
@_cdecl("cryvpn_ios_start")
public func cryvpnIosStart(
  _ конфигC: UnsafePointer<CChar>,
  _ серверC: UnsafePointer<CChar>,
  _ днсC: UnsafePointer<CChar>,
  _ защита: Bool
) -> UnsafeMutablePointer<CChar>? {
  let конфиг = String(cString: конфигC)
  let сервер = String(cString: серверC)
  let днс = String(cString: днсC)
  let состояние = Состояние.общее
  состояние.наблюдать()

  // Наш менеджер, если уже есть, иначе новый.
  let (список, ошибкаЧтения): ([NETunnelProviderManager]?, Error?) = дождаться { готово in
    NETunnelProviderManager.loadAllFromPreferences { список, ошибка in готово((список, ошибка)) }
  }
  if let ошибкаЧтения { return вRust(понятно(ошибкаЧтения)) }
  let менеджер = список?.first(where: наш) ?? NETunnelProviderManager()

  let протокол = NETunnelProviderProtocol()
  протокол.providerBundleIdentifier = расширение
  // В Настройках → VPN это видно как «сервер».
  протокол.serverAddress = сервер.isEmpty ? "CryVPN" : сервер
  // Конфигурация лежит и здесь: если VPN включат из Настроек, без окна,
  // расширение возьмёт её отсюда.
  протокол.providerConfiguration = ["config": конфиг, "dns": днс]
  if #available(iOS 14.0, *) {
    // Защита от утечки: при обрыве iOS не выпустит трафик мимо VPN.
    протокол.includeAllNetworks = защита
    протокол.excludeLocalNetworks = true
  }
  менеджер.protocolConfiguration = протокол
  менеджер.localizedDescription = "CryVPN"
  менеджер.isEnabled = true

  // В первый раз здесь iOS спросит: «Разрешить добавить конфигурацию VPN?»
  let ошибкаЗаписи: Error? = дождаться { готово in
    менеджер.saveToPreferences { ошибка in готово(ошибка) }
  }
  if let ошибкаЗаписи { return вRust(понятно(ошибкаЗаписи)) }
  // После записи iOS требует перечитать настройки, иначе первый запуск
  // молча не срабатывает.
  let ошибкаПеречитки: Error? = дождаться { готово in
    менеджер.loadFromPreferences { ошибка in готово(ошибка) }
  }
  if let ошибкаПеречитки { return вRust(понятно(ошибкаПеречитки)) }
  состояние.запомнить(менеджер)

  do {
    try менеджер.connection.startVPNTunnel(options: ["config": конфиг as NSString])
  } catch {
    return вRust(понятно(error))
  }

  // Ждём итога: расширение говорит «готово», только когда ядро запустилось.
  let начало = Date()
  let срок = начало.addingTimeInterval(45)
  var былоПодключение = false
  while Date() < срок {
    let статус = менеджер.connection.status
    switch статус {
    case .connected:
      состояние.обновитьСтатус()
      return nil
    case .connecting, .reasserting:
      былоПодключение = true
    case .disconnected, .invalid:
      // Сразу после просьбы статус ещё «выключен». Если он так и не стал
      // «подключается» за три секунды — расширение упало, не начав.
      if былоПодключение || Date().timeIntervalSince(начало) > 3 {
        return вRust(причинаОбрыва(менеджер) ?? "VPN не поднялся. Попробуйте ещё раз или другой сервер")
      }
    default:
      break
    }
    Thread.sleep(forTimeInterval: 0.2)
  }
  менеджер.connection.stopVPNTunnel()
  return вRust("VPN не поднялся за 45 секунд. Попробуйте ещё раз или другой сервер")
}

/// Почему расширение не поднялось: текст, который оно само отдало iOS.
private func причинаОбрыва(_ менеджер: NETunnelProviderManager) -> String? {
  guard #available(iOS 16.0, *) else { return nil }
  let ошибка: Error? = дождаться { готово in
    менеджер.connection.fetchLastDisconnectError { ошибка in готово(ошибка) }
  }
  guard let ошибка else { return nil }
  let текст = (ошибка as NSError).localizedDescription
  return текст.isEmpty ? nil : текст
}

@_cdecl("cryvpn_ios_stop")
public func cryvpnIosStop() {
  let состояние = Состояние.общее
  if let менеджер = состояние.взятьМенеджер() {
    менеджер.connection.stopVPNTunnel()
    return
  }
  // Менеджер ещё не загружен (например, сразу после запуска приложения) —
  // загружаем и гасим в фоне.
  NETunnelProviderManager.loadAllFromPreferences { список, _ in
    let наш = список?.first(where: наш)
    наш?.connection.stopVPNTunnel()
    состояние.запомнить(наш)
  }
}

/// 0 — выключен, 1 — подключается, 2 — включён. Не ждёт iOS.
@_cdecl("cryvpn_ios_status")
public func cryvpnIosStatus() -> Int32 {
  let состояние = Состояние.общее
  состояние.загрузитьВФоне()
  состояние.обновитьСтатус()
  return состояние.статусСейчас()
}

/// Модель телефона («iPhone16,1») и версия iOS — для кабинета.
@_cdecl("cryvpn_ios_info")
public func cryvpnIosInfo() -> UnsafeMutablePointer<CChar>? {
  var сведения = utsname()
  uname(&сведения)
  let модель = withUnsafePointer(to: &сведения.machine) {
    $0.withMemoryRebound(to: CChar.self, capacity: 256) { String(cString: $0) }
  }
  let в = ProcessInfo.processInfo.operatingSystemVersion
  let версия = "\(в.majorVersion).\(в.minorVersion).\(в.patchVersion)"
  let данные = ["model": модель, "release": версия]
  guard let json = try? JSONSerialization.data(withJSONObject: данные),
        let текст = String(data: json, encoding: .utf8)
  else { return nil }
  return вRust(текст)
}
