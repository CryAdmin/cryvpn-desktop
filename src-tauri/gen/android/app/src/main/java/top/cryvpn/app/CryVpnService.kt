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
    const val EXTRA_DIR = "dir"
    const val EXTRA_CORE = "core"
    const val EXTRA_ASSETS = "assets"
    const val EXTRA_BYPASS = "bypass"
    const val EXTRA_TITLE = "title"
    const val EXTRA_FROM_USER = "fromUser"
    private const val CHANNEL = "vpn"
    private const val OUTSIDE = "выключено-снаружи"

    init {
      System.loadLibrary("cryspawn")
    }
  }

  /** Запускает ядро, передавая ему туннель; отдаёт номер процесса. */
  private external fun spawnCore(core: String, config: String, assets: String, log: String, fd: Int): Int

  private var dir: File? = null
  private var corePid = 0
  private val handler = Handler(Looper.getMainLooper())
  private val watch = object : Runnable {
    override fun run() {
      if (corePid > 0 && !File("/proc/$corePid").exists()) {
        // Ядро умерло само — туннель без него бесполезен.
        corePid = 0
        shutdown()
        return
      }
      handler.postDelayed(this, 2000)
    }
  }

  private fun file(name: String): File? = dir?.let { File(it, name) }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      // Кнопка в уведомлении: помечаем, что это решение человека, а не обрыв.
      if (intent.getBooleanExtra(EXTRA_FROM_USER, false)) file("tunnel.end")?.writeText(OUTSIDE)
      // Останавливаемся «по номеру»: если следом уже пришёл новый запуск
      // (человек переключил сервер), служба остаётся жить и поднимет его.
      // Обычная остановка сняла бы службу вместе со свежим туннелем.
      handler.removeCallbacks(watch)
      killCore()
      file("tunnel.on")?.delete()
      CryTileService.refresh(this)
      if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
      stopSelf(startId)
      return START_NOT_STICKY
    }
    val folder = intent?.getStringExtra(EXTRA_DIR)
    val core = intent?.getStringExtra(EXTRA_CORE)
    val assets = intent?.getStringExtra(EXTRA_ASSETS)
    if (folder == null || core == null || assets == null) {
      stopNow()
      return START_NOT_STICKY
    }
    dir = File(folder)
    CryTileService.rememberDir(this, folder)
    killCore()
    file("tunnel.err")?.delete()
    showNotification(intent.getStringExtra(EXTRA_TITLE).orEmpty())
    try {
      val builder = Builder()
        .setSession("CryVPN")
        .setMtu(1500)
        .addAddress("198.19.77.1", 30)
        .addRoute("0.0.0.0", 0)
        // IPv6 тоже забираем в туннель: без этого он шёл бы мимо VPN.
        .addAddress("fd77:7777:7777::1", 126)
        .addRoute("::", 0)
        .addDnsServer("1.1.1.1")
        .addDnsServer("8.8.8.8")
        .addDisallowedApplication(packageName)
      // Приложения, которые человек пустил мимо VPN. Удалённое с телефона
      // приложение пропускаем: из-за него туннель не должен сорваться.
      for (pkg in intent.getStringArrayExtra(EXTRA_BYPASS) ?: arrayOf()) {
        try {
          builder.addDisallowedApplication(pkg)
        } catch (e: Exception) {
        }
      }
      if (Build.VERSION.SDK_INT >= 29) builder.setMetered(false)
      val tun = builder.establish() ?: throw IllegalStateException("Android не выдал туннель")
      val fd = tun.detachFd()
      val pid = try {
        spawnCore(
          core,
          File(folder, "current.json").path,
          assets,
          File(folder, "xray.log").path,
          fd
        )
      } finally {
        // Свой экземпляр туннеля закрываем в любом случае: при удаче он
        // дальше живёт вместе с ядром, при неудаче не должен утечь.
        try {
          ParcelFileDescriptor.adoptFd(fd).close()
        } catch (e: Exception) {
        }
      }
      if (pid <= 0) throw IllegalStateException("ядро не запустилось (код $pid)")
      corePid = pid
      file("tunnel.on")?.writeText("1")
      CryTileService.refresh(this)
      handler.removeCallbacks(watch)
      handler.postDelayed(watch, 2000)
    } catch (e: Throwable) {
      // Throwable, а не Exception: сюда попадает и сбой загрузки помощника.
      file("tunnel.err")?.writeText(e.message ?: "не удалось поднять туннель")
      shutdown()
    }
    return START_NOT_STICKY
  }

  /** Человек выключил VPN в настройках системы или включил другой VPN. */
  override fun onRevoke() {
    file("tunnel.end")?.writeText(OUTSIDE)
    shutdown()
    super.onRevoke()
  }

  override fun onDestroy() {
    handler.removeCallbacks(watch)
    killCore()
    super.onDestroy()
  }

  private fun killCore() {
    if (corePid > 0) {
      android.os.Process.killProcess(corePid)
      corePid = 0
    }
  }

  private fun shutdown() {
    handler.removeCallbacks(watch)
    killCore()
    file("tunnel.on")?.delete()
    CryTileService.refresh(this)
    stopNow()
  }

  private fun stopNow() {
    if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
    stopSelf()
  }

  private fun showNotification(server: String) {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= 26) {
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL, "Подключение VPN", NotificationManager.IMPORTANCE_LOW)
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
      .setSmallIcon(R.drawable.ic_vpn_key)
      .setContentIntent(open)
      .setOngoing(true)
      .apply {
        // Показывать сразу: иначе Android придерживает уведомление службы
        // до десяти секунд.
        if (Build.VERSION.SDK_INT >= 31) setForegroundServiceBehavior(Notification.FOREGROUND_SERVICE_IMMEDIATE)
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
