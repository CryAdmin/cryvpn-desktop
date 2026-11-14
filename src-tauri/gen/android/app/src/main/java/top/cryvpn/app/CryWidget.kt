package top.cryvpn.app

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.view.View
import android.widget.RemoteViews
import java.io.File

/**
 * Виджет на рабочий стол: включить и выключить VPN, не открывая приложение.
 *
 * Работает так же, как плитка в шторке. Выключает сам. Включает тоже сам —
 * тем сервером, к которому подключались в прошлый раз. Если подключений
 * ещё не было или Android хочет заново спросить согласие на VPN, нажатие
 * открывает приложение с просьбой подключиться.
 *
 * Что именно сделает нажатие, решаем заранее, когда рисуем виджет: открыть
 * окно из фонового приёмника система не даёт, а из самого нажатия — даёт.
 */
class CryWidget : AppWidgetProvider() {
  companion object {
    private const val ACTION_TOGGLE = "top.cryvpn.app.WIDGET_TOGGLE"
    private const val PREFS = "cryvpn_vpn"
    private const val KEY_DIR = "dir"
    private const val GREEN = 0xFF4ADE80.toInt()
    private const val GREY = 0xCCFFFFFF.toInt()

    private fun tunnelUp(context: Context): Boolean {
      val dir = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString(KEY_DIR, null) ?: return false
      return File(dir, "tunnel.on").exists()
    }

    /** Можно ли поднять туннель молча, без окна приложения. */
    private fun canStartSilently(context: Context): Boolean = try {
      android.net.VpnService.prepare(context) == null && CryVpnService.canStartLast(context)
    } catch (e: Exception) {
      false
    }

    private fun openApp(context: Context): Intent =
      Intent(context, MainActivity::class.java)
        .setAction(CryTileService.ACTION_CONNECT)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    /** Состояние VPN изменилось — перерисовать все виджеты на столе. */
    fun refresh(context: Context, active: Boolean? = null) {
      try {
        val manager = AppWidgetManager.getInstance(context)
        val ids = manager.getAppWidgetIds(ComponentName(context, CryWidget::class.java))
        if (ids.isEmpty()) return
        val on = active ?: tunnelUp(context)
        for (id in ids) manager.updateAppWidget(id, views(context, on, manager.getAppWidgetOptions(id)))
      } catch (e: Exception) {
        // Виджет могли не поставить, а оболочка рабочего стола — не ответить.
      }
    }

    private fun views(context: Context, on: Boolean, options: Bundle?): RemoteViews {
      val views = RemoteViews(context.packageName, R.layout.cry_widget)
      views.setInt(R.id.widget_icon, "setBackgroundResource", if (on) R.drawable.widget_dot_on else R.drawable.widget_dot_off)
      views.setInt(R.id.widget_icon, "setColorFilter", if (on) GREEN else GREY)
      views.setTextViewText(R.id.widget_state, if (on) "Включён" else "Выключен")
      views.setTextColor(R.id.widget_state, if (on) GREEN else 0x99FFFFFF.toInt())
      // В одну клетку подпись не помещается — остаётся одна голова робота.
      val width = options?.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0) ?: 0
      views.setViewVisibility(R.id.widget_text, if (width in 1..99) View.GONE else View.VISIBLE)

      val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
      val click = if (on || canStartSilently(context)) {
        PendingIntent.getBroadcast(
          context, 3, Intent(context, CryWidget::class.java).setAction(ACTION_TOGGLE), flags
        )
      } else {
        PendingIntent.getActivity(context, 4, openApp(context), flags)
      }
      views.setOnClickPendingIntent(R.id.widget_root, click)
      return views
    }
  }

  override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) {
    refresh(context)
  }

  override fun onAppWidgetOptionsChanged(context: Context, manager: AppWidgetManager, id: Int, options: Bundle) {
    refresh(context)
  }

  override fun onReceive(context: Context, intent: Intent) {
    super.onReceive(context, intent)
    if (intent.action != ACTION_TOGGLE) return
    if (tunnelUp(context)) {
      // Помечаем, что это решение человека: приложение не станет пугать
      // его сообщением об обрыве.
      try {
        context.startService(
          Intent(context, CryVpnService::class.java)
            .setAction(CryVpnService.ACTION_STOP)
            .putExtra(CryVpnService.EXTRA_FROM_USER, true)
        )
        refresh(context, false)
      } catch (e: Exception) {
        refresh(context)
      }
      return
    }
    if (canStartSilently(context)) {
      try {
        val start = Intent(context, CryVpnService::class.java).setAction(CryVpnService.ACTION_START_LAST)
        if (Build.VERSION.SDK_INT >= 26) context.startForegroundService(start) else context.startService(start)
        // Служба сама перерисует виджет: включилось — зелёный, нет — серый.
        refresh(context, true)
        return
      } catch (e: Exception) {
        // Система не дала запустить службу — попробуем открыть приложение.
      }
    }
    try {
      context.startActivity(openApp(context))
    } catch (e: Exception) {
      // Не дали и этого. Перерисуем: следующее нажатие откроет приложение.
    }
    refresh(context)
  }
}
