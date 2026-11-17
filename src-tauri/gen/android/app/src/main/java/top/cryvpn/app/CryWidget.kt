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
open class CryWidget : AppWidgetProvider() {
  companion object {
    private const val ACTION_TOGGLE = "top.cryvpn.app.WIDGET_TOGGLE"
    private const val PREFS = "cryvpn_vpn"
    private const val KEY_DIR = "dir"
    private const val GREEN = 0xFF4ADE80.toInt()

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
        // Три вида одного виджета: высокая плитка, узкая плитка и кнопка.
        val kinds = listOf(
          CryWidget::class.java to R.layout.cry_widget,
          CryWidgetFlat::class.java to R.layout.cry_widget_flat,
          CryWidgetSmall::class.java to R.layout.cry_widget_small,
        )
        var on: Boolean? = active
        for ((kind, layout) in kinds) {
          val ids = manager.getAppWidgetIds(ComponentName(context, kind))
          if (ids.isEmpty()) continue
          val state = on ?: tunnelUp(context).also { on = it }
          val small = layout == R.layout.cry_widget_small
          for (id in ids) {
            val options = if (small) null else manager.getAppWidgetOptions(id)
            manager.updateAppWidget(id, views(context, state, options, layout))
          }
        }
      } catch (e: Exception) {
        // Виджет могли не поставить, а оболочка рабочего стола — не ответить.
      }
    }

    private fun views(context: Context, on: Boolean, options: Bundle?, layout: Int): RemoteViews {
      val small = layout == R.layout.cry_widget_small
      val views = RemoteViews(context.packageName, layout)
      views.setImageViewResource(R.id.widget_icon, if (on) R.drawable.widget_orb_on else R.drawable.widget_orb_off)
      views.setInt(R.id.widget_root, "setBackgroundResource", if (on) R.drawable.widget_bg_on else R.drawable.widget_bg)
      if (!small) decorate(context, views, on, options)
      views.setOnClickPendingIntent(R.id.widget_root, click(context, on))
      return views
    }

    /** Подпись большого виджета. */
    private fun decorate(context: Context, views: RemoteViews, on: Boolean, options: Bundle?) {
      views.setTextViewText(R.id.widget_state, if (on) "● Включён" else "○ Выключен")
      views.setTextColor(R.id.widget_state, if (on) GREEN else 0x99FFFFFF.toInt())
      // В одну клетку подпись не помещается — остаётся одна голова робота.
      val width = options?.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 0) ?: 0
      val narrow = width in 1..99
      views.setViewVisibility(R.id.widget_text, if (narrow) View.GONE else View.VISIBLE)
      // Отступы плитки 4 слева и 12 справа рассчитаны на шар с подписью.
      // Без подписи они сдвигали шар влево — тогда отступов нет, шар по центру
      val dp = context.resources.displayMetrics.density
      val start = if (narrow) 0 else (4 * dp).toInt()
      val end = if (narrow) 0 else (12 * dp).toInt()
      views.setViewPadding(R.id.widget_root, start, 0, end, 0)
    }

    /** Что сделает нажатие: переключит VPN само или откроет приложение. */
    private fun click(context: Context, on: Boolean): PendingIntent {
      val flags = PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
      return if (on || canStartSilently(context)) {
        PendingIntent.getBroadcast(
          context, 3, Intent(context, CryWidget::class.java).setAction(ACTION_TOGGLE), flags
        )
      } else {
        PendingIntent.getActivity(context, 4, openApp(context), flags)
      }
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
