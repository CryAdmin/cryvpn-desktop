package top.cryvpn.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import java.io.File

/**
 * Системная служба VPN. Получает у Android туннель и запускает ядро Xray,
 * которое разбирает трафик.
 *
 * Служба живёт в отдельном процессе (см. манифест, `:vpn`). Окно приложения
 * тяжёлое, и на многих телефонах смахивание из списка недавних убивает его
 * процесс целиком. Пока служба жила там же, вместе с окном гас и VPN. Теперь
 * окно можно закрыть, смахнуть или выгрузить — служба и ядро остаются.
 *
 * С приложением служба разговаривает через файлы в его папке данных:
 *   tunnel.on  — туннель поднят и ядро живо;
 *   tunnel.err — почему не удалось подняться;
 *   tunnel.end — почему остановились, если выключили не из приложения.
 *
 * Собственные соединения приложения (а ядро — дочерний процесс службы) из
 * туннеля исключены: иначе трафик до сервера уходил бы обратно в туннель.
 */
class CryVpnService : VpnService() {
  companion object {
    const val ACTION_STOP = "top.cryvpn.app.STOP"
    /** Поднять туннель заново с тем же сервером — без окна приложения. */
    const val ACTION_START_LAST = "top.cryvpn.app.START_LAST"
    /** Конфигурация прошлого подключения, зашифрованная (см. Vault). */
    const val LAST_CONFIG = "last.dat"
    private const val PREFS = "cryvpn_last"

    /** Есть ли чем подняться без приложения. */
    fun canStartLast(context: android.content.Context): Boolean {
      val prefs = context.getSharedPreferences(PREFS, MODE_PRIVATE)
      val dir = prefs.getString(EXTRA_DIR, null) ?: return false
      return prefs.getString(EXTRA_ASSETS, null) != null && File(dir, LAST_CONFIG).exists()
    }
    const val EXTRA_DIR = "dir"
    const val EXTRA_CORE = "core"
    const val EXTRA_ASSETS = "assets"
    const val EXTRA_BYPASS = "bypass"
    const val EXTRA_DNS = "dns"
    const val EXTRA_TITLE = "title"
    const val EXTRA_FROM_USER = "fromUser"
    /** Защита от утечки (kill switch): при обрыве туннель не отпускаем. */
    const val EXTRA_KILL = "kill"
    /** Отметка для приложения: VPN оборвался, защита держит интернет. */
    const val BLOCK_MARK = "tunnel.block"
    /** Остановить ядро, но туннель под защитой не отпускать. */
    const val EXTRA_KEEP = "keep"
    // Имя канала уведомлений. «vpn» — прежний канал, на котором значок
    // приложения получал красную единичку: см. showNotification.
    private const val CHANNEL = "vpn-status"
    private const val OLD_CHANNEL = "vpn"
    private const val OUTSIDE = "выключено-снаружи"

    init {
      System.loadLibrary("cryspawn")
    }
  }

  /** Запускает ядро, передавая ему туннель; отдаёт номер процесса. */
  private external fun spawnCore(core: String, config: String, assets: String, log: String, fd: Int): Int

  private var dir: File? = null
  private var corePid = 0
  /**
   * Своя копия туннеля, пока включена защита от утечки. Ядро получает
   * туннель в своё распоряжение, а служба держит ещё одну ручку. Пока она
   * открыта, Android не снимает VPN, даже если ядро упало: трафик уходит в
   * туннель и там ждёт, а не идёт в интернет напрямую.
   */
  private var held: ParcelFileDescriptor? = null

  private fun release() {
    try {
      held?.close()
    } catch (e: Exception) {
    }
    held = null
    file(BLOCK_MARK)?.delete()
  }
  private val handler = Handler(Looper.getMainLooper())
  private val watch = object : Runnable {
    override fun run() {
      if (corePid > 0 && !File("/proc/$corePid").exists()) {
        corePid = 0
        if (held != null) {
          // Защита от утечки включена: туннель держим, интернет закрыт,
          // пока человек не переподключится или не снимет защиту.
          blocked()
          return
        }
        // Ядро умерло само — туннель без него бесполезен.
        shutdown()
        return
      }
      // Запоминаем, какой сервер записало приложение: при запуске с плитки
      // эту запись вернём, и окно покажет, куда подключён телефон.
      try {
        val server = file("tunnel.server")?.takeIf { it.exists() }?.readText()
        if (!server.isNullOrBlank()) {
          val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
          if (prefs.getString("server", null) != server) prefs.edit().putString("server", server).apply()
        }
      } catch (e: Exception) {
      }
      handler.postDelayed(this, 2000)
    }
  }

  private fun file(name: String): File? = dir?.let { File(it, name) }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      // Приложение меняет сервер или отключается, а защита включена: ядро
      // гасим, туннель держим. Отпустит его следующее подключение или
      // решение человека снять защиту (оно придёт отдельной командой).
      if (intent.getBooleanExtra(EXTRA_KEEP, false) && held != null) {
        handler.removeCallbacks(watch)
        killCore()
        file("tunnel.on")?.delete()
        file(BLOCK_MARK)?.writeText("1")
        CryTileService.refresh(this)
        CryWidget.refresh(this)
        return START_NOT_STICKY
      }
      // Кнопка в уведомлении: помечаем, что это решение человека, а не обрыв.
      if (intent.getBooleanExtra(EXTRA_FROM_USER, false)) file("tunnel.end")?.writeText(OUTSIDE)
      // Человек отключил VPN — защиту тоже снимаем: интернет без VPN он
      // выбрал сам.
      release()
      // Останавливаемся «по номеру»: если следом уже пришёл новый запуск
      // (человек переключил сервер), служба остаётся жить и поднимет его.
      // Обычная остановка сняла бы службу вместе со свежим туннелем.
      handler.removeCallbacks(watch)
      killCore()
      file("tunnel.on")?.delete()
      CryTileService.refresh(this)
      CryWidget.refresh(this)
      if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
      stopSelf(startId)
      return START_NOT_STICKY
    }
    // Запуск с плитки: окна приложения нет, берём всё из прошлого раза.
    val last = intent?.action == ACTION_START_LAST
    val prefs = getSharedPreferences(PREFS, MODE_PRIVATE)
    val folder = if (last) prefs.getString(EXTRA_DIR, null) else intent?.getStringExtra(EXTRA_DIR)
    // Путь к ядру не запоминаем, а спрашиваем у системы: после каждого
    // обновления приложения его папка меняется, и запомненный путь вёл бы
    // в никуда — плитка молча переставала бы включать VPN.
    val core = if (last) File(applicationInfo.nativeLibraryDir, "libxray.so").path else intent?.getStringExtra(EXTRA_CORE)
    val assets = if (last) prefs.getString(EXTRA_ASSETS, null) else intent?.getStringExtra(EXTRA_ASSETS)
    val bypass: Array<String> = if (last) {
      prefs.getString(EXTRA_BYPASS, "").orEmpty().split("\n").filter { it.isNotBlank() }.toTypedArray()
    } else {
      intent?.getStringArrayExtra(EXTRA_BYPASS) ?: arrayOf()
    }
    // DNS туннеля по выбору из настроек приложения. Пусто — запуск от
    // прежней версии или сбой передачи: берём привычные адреса.
    val dns: List<String> = (if (last) {
      prefs.getString(EXTRA_DNS, "").orEmpty().split("\n")
    } else {
      intent?.getStringArrayExtra(EXTRA_DNS)?.toList() ?: listOf()
    }).filter { it.isNotBlank() }.ifEmpty { listOf("1.1.1.1", "8.8.8.8") }
    val title = if (last) prefs.getString(EXTRA_TITLE, "").orEmpty() else intent?.getStringExtra(EXTRA_TITLE).orEmpty()
    val kill = if (last) prefs.getBoolean(EXTRA_KILL, false) else intent?.getBooleanExtra(EXTRA_KILL, false) ?: false
    if (folder == null || core == null || assets == null) {
      stopNow()
      return START_NOT_STICKY
    }
    dir = File(folder)
    CryTileService.rememberDir(this, folder)
    killCore()
    file("tunnel.err")?.delete()
    // Уведомление — первым делом: служба переднего плана обязана показать
    // его в считаные секунды после запуска.
    showNotification(title)
    val config = File(folder, "current.json")
    try {
      if (last) {
        // Конфигурацию достаём из зашифрованной копии. Открытой она лежит
        // на диске несколько секунд, пока ядро её не прочтёт.
        config.writeBytes(Vault.unseal(File(folder, LAST_CONFIG).readText()))
        file("tunnel.end")?.delete()
        prefs.getString("server", null)?.let { file("tunnel.server")?.writeText(it) }
      } else {
        // Обычный запуск из приложения: запоминаем всё нужное, чтобы в
        // следующий раз плитка включила VPN сама. Ключи сервера — только
        // зашифрованными. Не вышло запомнить — подключению это не помеха.
        try {
          File(folder, LAST_CONFIG).writeText(Vault.seal(config.readBytes()))
          prefs.edit()
            .putString(EXTRA_DIR, folder)
            .putString(EXTRA_ASSETS, assets)
            .putString(EXTRA_BYPASS, bypass.joinToString("\n"))
            .putString(EXTRA_DNS, dns.joinToString("\n"))
            .putString(EXTRA_TITLE, title)
            .putBoolean(EXTRA_KILL, kill)
            .apply()
        } catch (e: Exception) {
        }
      }
      val builder = Builder()
        .setSession("CryVPN")
        .setMtu(1500)
        .addAddress("198.19.77.1", 30)
        .addRoute("0.0.0.0", 0)
        // IPv6 тоже забираем в туннель: без этого он шёл бы мимо VPN.
        .addAddress("fd77:7777:7777::1", 126)
        .addRoute("::", 0)
        .addDisallowedApplication(packageName)
      // Адрес с ошибкой не должен сорвать туннель: пропускаем его, а если
      // не подошёл ни один — ставим привычные.
      var dnsSet = 0
      for (address in dns) {
        try {
          builder.addDnsServer(address)
          dnsSet++
        } catch (e: Exception) {
        }
      }
      if (dnsSet == 0) {
        builder.addDnsServer("1.1.1.1")
        builder.addDnsServer("8.8.8.8")
      }
      // Приложения, которые человек пустил мимо VPN. Удалённое с телефона
      // приложение пропускаем: из-за него туннель не должен сорваться.
      for (pkg in bypass) {
        try {
          builder.addDisallowedApplication(pkg)
        } catch (e: Exception) {
        }
      }
      if (Build.VERSION.SDK_INT >= 29) builder.setMetered(false)
      val tun = builder.establish() ?: throw IllegalStateException("Android не выдал туннель")
      // Новый туннель уже стоит — прежний, удерживаемый защитой, отпускаем.
      // Между ними трафик не выходит наружу ни на миг.
      release()
      val fd = tun.detachFd()
      val pid = try {
        spawnCore(
          core,
          config.path,
          assets,
          File(folder, "xray.log").path,
          fd
        )
      } finally {
        // Свой экземпляр туннеля закрываем: при удаче он дальше живёт вместе
        // с ядром, при неудаче не должен утечь. С защитой от утечки — держим
        // у себя (см. `held`), пока ядро работает или пока её не снимут.
        try {
          val own = ParcelFileDescriptor.adoptFd(fd)
          if (kill) held = own else own.close()
        } catch (e: Exception) {
        }
      }
      if (pid <= 0) throw IllegalStateException("ядро не запустилось (код $pid)")
      corePid = pid
      file("tunnel.on")?.writeText("1")
      CryTileService.refresh(this)
      CryWidget.refresh(this)
      // При запуске с плитки открытую конфигурацию убираем сами: ядро её
      // уже прочло, а приложения, которое обычно это делает, рядом нет.
      if (last) handler.postDelayed({ config.delete() }, 4000)
      handler.removeCallbacks(watch)
      handler.postDelayed(watch, 2000)
    } catch (e: Throwable) {
      // Throwable, а не Exception: сюда попадает и сбой загрузки помощника.
      file("tunnel.err")?.writeText(e.message ?: "не удалось поднять туннель")
      if (last) config.delete()
      shutdown()
    }
    return START_NOT_STICKY
  }

  /** Человек выключил VPN в настройках системы или включил другой VPN. */
  override fun onRevoke() {
    file("tunnel.end")?.writeText(OUTSIDE)
    // VPN забрали — другой VPN или человек в настройках системы. Держать
    // туннель мы больше не можем и не должны.
    release()
    shutdown()
    super.onRevoke()
  }

  override fun onDestroy() {
    handler.removeCallbacks(watch)
    killCore()
    release()
    super.onDestroy()
  }

  private fun killCore() {
    if (corePid > 0) {
      android.os.Process.killProcess(corePid)
      corePid = 0
    }
  }

  /**
   * VPN оборвался, защита держит интернет. Служба остаётся работать, в
   * шторке — объяснение и две кнопки: подключиться снова или снять защиту.
   */
  private fun blocked() {
    handler.removeCallbacks(watch)
    file("tunnel.on")?.delete()
    file(BLOCK_MARK)?.writeText("1")
    CryTileService.refresh(this)
    CryWidget.refresh(this)
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
    )
    val again = PendingIntent.getService(
      this, 2, Intent(this, CryVpnService::class.java).setAction(ACTION_START_LAST), PendingIntent.FLAG_IMMUTABLE
    )
    val stop = PendingIntent.getService(
      this, 1, Intent(this, CryVpnService::class.java).setAction(ACTION_STOP).putExtra(EXTRA_FROM_USER, true), PendingIntent.FLAG_IMMUTABLE
    )
    val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
    val notification = builder
      .setContentTitle("🛡 VPN оборвался — интернет на паузе")
      .setContentText("Защита от утечки не пускает трафик мимо VPN. Подключитесь снова или снимите защиту")
      .setSmallIcon(R.drawable.ic_robot)
      .setContentIntent(open)
      .setOngoing(true)
      .apply {
        if (Build.VERSION.SDK_INT >= 26) setBadgeIconType(Notification.BADGE_ICON_NONE)
      }
      .addAction(Notification.Action.Builder(null, "Подключить снова", again).build())
      .addAction(Notification.Action.Builder(null, "Снять защиту", stop).build())
      .build()
    getSystemService(NotificationManager::class.java)?.notify(1, notification)
  }

  private fun shutdown() {
    handler.removeCallbacks(watch)
    killCore()
    file("tunnel.on")?.delete()
    CryTileService.refresh(this)
    CryWidget.refresh(this)
    stopNow()
  }

  private fun stopNow() {
    if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
    stopSelf()
  }

  private fun showNotification(server: String) {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26) {
      // Без метки на значке приложения. Уведомление о работе VPN — это
      // состояние, а не новость: красная единичка на значке, пока VPN
      // включён, выглядит как непрочитанное сообщение. Запретить метку
      // можно только при создании канала, у готового канала система это
      // менять не даёт — поэтому канал новый, а прежний удаляем.
      manager.deleteNotificationChannel(OLD_CHANNEL)
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL, "Подключение VPN", NotificationManager.IMPORTANCE_LOW).apply {
          setShowBadge(false)
        }
      )
    }
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
    )
    val stop = PendingIntent.getService(
      this, 1, Intent(this, CryVpnService::class.java).setAction(ACTION_STOP).putExtra(EXTRA_FROM_USER, true), PendingIntent.FLAG_IMMUTABLE
    )
    val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
    val notification = builder
      .setContentTitle("CryVPN подключён")
      // Название сервера — чтобы из шторки было видно, куда подключены.
      .setContentText(if (server.isNotBlank()) server else "Нажмите, чтобы открыть приложение")
      .setSmallIcon(R.drawable.ic_robot)
      .setContentIntent(open)
      .setOngoing(true)
      .apply {
        // Показывать сразу: иначе Android придерживает уведомление службы
        // до десяти секунд.
        if (Build.VERSION.SDK_INT >= 31) setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
        // И само уведомление в счётчик на значке не идёт.
        if (Build.VERSION.SDK_INT >= 26) setBadgeIconType(Notification.BADGE_ICON_NONE)
        setNumber(0)
      }
      .addAction(Notification.Action.Builder(null, "Отключить", stop).build())
      .build()
    if (Build.VERSION.SDK_INT >= 34) {
      startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
    } else {
      startForeground(1, notification)
    }
  }
}
