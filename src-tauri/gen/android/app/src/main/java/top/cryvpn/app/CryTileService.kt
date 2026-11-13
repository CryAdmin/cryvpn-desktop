package top.cryvpn.app

import android.app.PendingIntent
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import java.io.File

/**
 * Плитка в шторке: включить и выключить VPN, не открывая приложение.
 *
 * Выключает плитка сама — службе VPN для этого ничего не нужно. Включить
 * сама не может: конфигурация сервера на диске не хранится, её собирает
 * приложение из подписки. Поэтому нажатие на выключенную плитку открывает
 * приложение с просьбой подключиться, и оно подключается к последнему
 * серверу без лишних нажатий.
 *
 * Живёт в том же процессе, что и служба VPN: ради плитки тяжёлое окно
 * приложения поднимать незачем.
 */
class CryTileService : TileService() {
  companion object {
    const val ACTION_CONNECT = "top.cryvpn.app.CONNECT"
    private const val PREFS = "cryvpn_vpn"
    private const val KEY_DIR = "dir"

    /** Служба VPN запоминает папку данных: по ней видно, поднят ли туннель. */
    fun rememberDir(context: Context, dir: String) {
      context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString(KEY_DIR, dir).apply()
    }

    /** Состояние VPN изменилось — пусть система перерисует плитку. */
    fun refresh(context: Context) {
      try {
        requestListeningState(context, ComponentName(context, CryTileService::class.java))
      } catch (e: Exception) {
        // Плитку могли не добавить в шторку — тогда и обновлять нечего.
      }
    }
  }

  private fun tunnelUp(): Boolean {
    val dir = getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_DIR, null) ?: return false
    return File(dir, "tunnel.on").exists()
  }

  private fun show(active: Boolean) {
    val tile = qsTile ?: return
    tile.state = if (active) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
    tile.label = "CryVPN"
    if (Build.VERSION.SDK_INT >= 29) tile.subtitle = if (active) "Включён" else "Выключен"
    tile.updateTile()
  }

  override fun onStartListening() {
    super.onStartListening()
    show(tunnelUp())
  }

  override fun onClick() {
    super.onClick()
    if (tunnelUp()) {
      // Помечаем, что это решение человека: приложение не станет пугать
      // его сообщением об обрыве.
      startService(
        Intent(this, CryVpnService::class.java)
          .setAction(CryVpnService.ACTION_STOP)
          .putExtra(CryVpnService.EXTRA_FROM_USER, true)
      )
      show(false)
      return
    }
    val open = Intent(this, MainActivity::class.java)
      .setAction(ACTION_CONNECT)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    val launch = Runnable {
      if (Build.VERSION.SDK_INT >= 34) {
        startActivityAndCollapse(
          PendingIntent.getActivity(this, 2, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        )
      } else {
        @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
        startActivityAndCollapse(open)
      }
    }
    // На заблокированном экране сначала просим разблокировать.
    if (isLocked) unlockAndRun(launch) else launch.run()
  }
}
