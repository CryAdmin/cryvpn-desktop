package top.cryvpn.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import java.io.File

/**
 * Системная служба VPN. Её дело — получить у Android туннель и отдать его
 * номер ядру; сам трафик разбирает ядро Xray, запущенное рядом.
 *
 * Собственные соединения приложения (а ядро — его дочерний процесс) из
 * туннеля исключены: иначе трафик до сервера уходил бы обратно в туннель.
 */
class CryVpnService : VpnService() {
  companion object {
    const val ACTION_STOP = "top.cryvpn.app.STOP"
    const val EXTRA_MARKER = "marker"
    private const val CHANNEL = "vpn"

    /** Кто ждёт номер туннеля: (номер, ошибка). */
    @Volatile var waiter: ((Int?, String?) -> Unit)? = null
  }

  private var marker: File? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    if (intent?.action == ACTION_STOP) {
      shutdown()
      return START_NOT_STICKY
    }
    intent?.getStringExtra(EXTRA_MARKER)?.let { marker = File(it) }
    showNotification()
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
      if (Build.VERSION.SDK_INT >= 29) builder.setMetered(false)
      val tun = builder.establish() ?: throw IllegalStateException("Android не выдал туннель")
      // Номер уходит ядру; с этого момента туннель живёт, пока жив его процесс.
      deliver(tun.detachFd(), null)
    } catch (e: Exception) {
      deliver(null, e.message ?: "не удалось поднять туннель")
      shutdown()
    }
    return START_NOT_STICKY
  }

  private fun deliver(fd: Int?, error: String?) {
    val w = waiter
    waiter = null
    w?.invoke(fd, error)
  }

  /** Человек выключил VPN в настройках системы или включил другой VPN. */
  override fun onRevoke() {
    shutdown()
    super.onRevoke()
  }

  override fun onDestroy() {
    marker?.delete()
    super.onDestroy()
  }

  /** Отметку стираем: по её пропаже приложение гасит ядро. */
  private fun shutdown() {
    marker?.delete()
    if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE) else @Suppress("DEPRECATION") stopForeground(true)
    stopSelf()
  }

  private fun showNotification() {
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
      this, 1, Intent(this, CryVpnService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE
    )
    val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(this, CHANNEL) else @Suppress("DEPRECATION") Notification.Builder(this)
    val notification = builder
      .setContentTitle("CryVPN подключён")
      .setContentText("Нажмите, чтобы открыть приложение")
      .setSmallIcon(R.drawable.ic_vpn_key)
      .setContentIntent(open)
      .setOngoing(true)
      .addAction(Notification.Action.Builder(null, "Отключить", stop).build())
      .build()
    if (Build.VERSION.SDK_INT >= 34) {
      startForeground(1, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
    } else {
      startForeground(1, notification)
    }
  }
}
