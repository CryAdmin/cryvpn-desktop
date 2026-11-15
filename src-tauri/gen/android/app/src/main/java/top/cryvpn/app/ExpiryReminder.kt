package top.cryvpn.app

import android.app.AlarmManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject

/**
 * Уведомление «Подписка окончена» в шторке — когда приложение закрыто.
 *
 * Сервера рассылки у нас нет, поэтому напоминает сам телефон: приложение,
 * прочитав подписку, ставит будильник на час её окончания. Будильник не
 * верит себе на слово: человек мог продлить подписку в боте или на сайте,
 * не открывая приложение. Поэтому в назначенный час мы сначала спрашиваем
 * кабинет и показываем уведомление, только если он сам подтвердил, что
 * подписка кончилась. Продлили — молча переставляем будильник на новый срок. Не
 * достучались — спросим позже; наугад не уведомляем никогда.
 *
 * Чем спрашивать (адреса кабинета и код подписки), приложение передаёт
 * заранее. Лежит это зашифрованным ключом из хранилища Android, как и всё
 * остальное: код подписки — это доступ к VPN.
 */
object ExpiryReminder {
  const val ACTION_CHECK = "top.cryvpn.app.EXPIRY_CHECK"
  private const val PREFS = "cryvpn"
  private const val KEY_AT = "reminder_at"
  private const val KEY_PLAN = "reminder_plan"
  private const val KEY_TRIES = "reminder_tries"
  private const val CHANNEL = "subscription"
  private const val NOTIFICATION = 2001
  /** Кабинет молчит — спрашиваем снова через полчаса, не дольше суток. */
  private const val RETRY_MS = 30L * 60 * 1000
  private const val MAX_TRIES = 48
  /** Срок прошёл, а кабинет ещё не пересчитал подписку — спросим через пять минут. */
  private const val LAG_MS = 5L * 60 * 1000
  /** Запас после срока: кабинет должен успеть сам заметить, что он вышел. */
  private const val SLACK_MS = 60L * 1000

  private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  private fun pending(context: Context): PendingIntent =
    PendingIntent.getBroadcast(
      context, 0,
      Intent(context, ExpiryReceiver::class.java).setAction(ACTION_CHECK),
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )

  private fun arm(context: Context, atMs: Long) {
    val alarms = context.getSystemService(AlarmManager::class.java) ?: return
    // Неточный будильник: разрешения на точные не просим, а минута-другая
    // здесь ничего не решает. «WhileIdle» — чтобы сработал и на спящем телефоне.
    alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, atMs, pending(context))
  }

  /** Приложение прочитало живую подписку: срок [expireSec] (0 — бессрочная). */
  fun schedule(context: Context, expireSec: Long, plan: String) {
    if (expireSec <= 0 || plan.isBlank()) {
      cancel(context)
      return
    }
    val sealed = try {
      Vault.seal(plan.toByteArray(Charsets.UTF_8))
    } catch (e: Exception) {
      cancel(context)
      return
    }
    prefs(context).edit()
      .putLong(KEY_AT, expireSec * 1000)
      .putString(KEY_PLAN, sealed)
      .putInt(KEY_TRIES, 0)
      .apply()
    arm(context, expireSec * 1000 + SLACK_MS)
    // Подписка действует — прошлое «окончена», если висит, больше не правда.
    if (expireSec * 1000 > System.currentTimeMillis()) {
      context.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
    }
  }

  /** Вышел из аккаунта или подписка бессрочная: напоминать не о чем. */
  fun cancel(context: Context) {
    prefs(context).edit().remove(KEY_AT).remove(KEY_PLAN).remove(KEY_TRIES).apply()
    context.getSystemService(AlarmManager::class.java)?.cancel(pending(context))
    context.getSystemService(NotificationManager::class.java)?.cancel(NOTIFICATION)
  }

  /** Телефон перезагрузили или приложение обновилось — будильники при этом теряются. */
  fun rearm(context: Context) {
    val at = prefs(context).getLong(KEY_AT, 0)
    if (at <= 0) return
    arm(context, maxOf(at + SLACK_MS, System.currentTimeMillis() + 15_000))
  }

  private enum class Verdict { EXPIRED, LAGGING, UNKNOWN }

  /** Будильник сработал. Вызывается не из главного потока: ходит в сеть. */
  fun check(context: Context) {
    val store = prefs(context)
    val sealed = store.getString(KEY_PLAN, null) ?: return
    if (store.getLong(KEY_AT, 0) <= 0) return
    val plan = try {
      JSONObject(String(Vault.unseal(sealed), Charsets.UTF_8))
    } catch (e: Exception) {
      // Ключ сменился или запись испорчена — спрашивать нечем.
      cancel(context)
      return
    }
    when (val answer = ask(context, plan)) {
      Verdict.EXPIRED -> {
        store.edit().remove(KEY_AT).remove(KEY_PLAN).remove(KEY_TRIES).apply()
        notifyExpired(context)
      }
      Verdict.UNKNOWN, Verdict.LAGGING -> {
        val tries = store.getInt(KEY_TRIES, 0) + 1
        if (tries >= MAX_TRIES) {
          store.edit().remove(KEY_AT).remove(KEY_PLAN).remove(KEY_TRIES).apply()
        } else {
          store.edit().putInt(KEY_TRIES, tries).apply()
          // Частые вопросы — только первые полчаса; дальше раз в полчаса,
          // чтобы телефон не дёргал сеть зря.
          val soon = answer == Verdict.LAGGING && tries <= 6
          arm(context, System.currentTimeMillis() + if (soon) LAG_MS else RETRY_MS)
        }
      }
      is Long -> {
        // Продлили (или срок сдвинули): ждём нового срока. Ноль — бессрочная.
        if (answer <= 0L) {
          cancel(context)
        } else {
          store.edit().putLong(KEY_AT, answer * 1000).putInt(KEY_TRIES, 0).apply()
          arm(context, answer * 1000 + SLACK_MS)
        }
      }
      else -> Unit
    }
  }

  /**
   * Спрашиваем кабинет, действует ли подписка с этим кодом. Ответ:
   * Verdict.EXPIRED — кончилась; Long — действует до этого срока в секундах
   * (0 — срока нет); Verdict.LAGGING — срок прошёл, но кабинет ещё считает
   * её действующей; Verdict.UNKNOWN — не достучались или не поняли.
   *
   * Панель тут не помощник: истёкшим она отдаёт срок на месяц вперёд.
   */
  private fun ask(context: Context, plan: JSONObject): Any {
    val urls = plan.optJSONArray("urls") ?: return Verdict.UNKNOWN
    val code = plan.optString("code")
    if (code.isBlank()) return Verdict.UNKNOWN
    val body = JSONObject().put("code", code).toString().toByteArray(Charsets.UTF_8)
    for (i in 0 until urls.length()) {
      val address = urls.optString(i)
      if (!address.startsWith("https://")) continue
      var connection: HttpURLConnection? = null
      try {
        connection = (open(context, URL(address)) as HttpURLConnection).apply {
          connectTimeout = 6000
          readTimeout = 6000
          instanceFollowRedirects = false
          requestMethod = "POST"
          doOutput = true
          setRequestProperty("Content-Type", "application/json")
          setRequestProperty("User-Agent", plan.optString("agent", "CryVPN-Android"))
        }
        connection.outputStream.use { it.write(body) }
        val status = connection.responseCode
        // Прокладка не дозвонилась до кабинета — пробуем запасной адрес.
        if (status == 502 || status == 503 || status == 504) continue
        // Любой другой ответ — слово самого кабинета, оно окончательное.
        if (status != 200) return Verdict.UNKNOWN
        val text = connection.inputStream.bufferedReader(Charsets.UTF_8).use { it.readText() }
        val answer = JSONObject(text)
        // Без ясного «active» ничего не утверждаем.
        if (!answer.has("active") || answer.opt("active") !is Boolean) return Verdict.UNKNOWN
        if (!answer.getBoolean("active")) return Verdict.EXPIRED
        val end = answer.optLong("end", 0)
        // Действует, а срок уже позади — кабинет ещё не пересчитал. Спросим позже.
        if (end > 0 && end * 1000 <= System.currentTimeMillis()) return Verdict.LAGGING
        return maxOf(end, 0L)
      } catch (e: Exception) {
        // Нет связи с этим адресом — пробуем следующий.
      } finally {
        connection?.disconnect()
      }
    }
    return Verdict.UNKNOWN
  }

  /**
   * Мимо нашего же туннеля, если он поднят: узлы истёкшую подписку уже не
   * обслуживают, и запрос через VPN просто не дошёл бы до панели.
   */
  private fun open(context: Context, url: URL): java.net.URLConnection {
    val direct = try {
      val manager = context.getSystemService(ConnectivityManager::class.java)
      @Suppress("DEPRECATION")
      manager?.allNetworks?.firstOrNull { network ->
        val caps = manager.getNetworkCapabilities(network) ?: return@firstOrNull false
        caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
          caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED) &&
          !caps.hasTransport(NetworkCapabilities.TRANSPORT_VPN)
      }
    } catch (e: Exception) {
      // Список сетей не дали — идём обычной дорогой.
      null
    }
    return direct?.openConnection(url) ?: url.openConnection()
  }

  private fun notifyExpired(context: Context) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    if (Build.VERSION.SDK_INT >= 26) {
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL, "Подписка", NotificationManager.IMPORTANCE_DEFAULT)
      )
    }
    // Нажатие открывает приложение — там табличка с кнопкой «Продлить».
    val open = PendingIntent.getActivity(
      context, 2, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
    )
    val builder = if (Build.VERSION.SDK_INT >= 26) Notification.Builder(context, CHANNEL)
    else @Suppress("DEPRECATION") Notification.Builder(context)
    val notification = builder
      .setContentTitle("⏰ Подписка CryVPN окончена")
      .setContentText("💳 Нажмите, чтобы продлить — VPN заработает сразу после оплаты")
      .setSmallIcon(R.drawable.ic_robot)
      .setContentIntent(open)
      .setAutoCancel(true)
      .build()
    try {
      // Человек запретил уведомления — система просто не покажет.
      manager.notify(NOTIFICATION, notification)
    } catch (e: SecurityException) {
      // нет разрешения — молчим
    }
  }
}

/** Будильник напоминания, перезагрузка телефона, обновление приложения. */
class ExpiryReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val app = context.applicationContext
    when (intent.action) {
      ExpiryReminder.ACTION_CHECK -> {
        // В сеть из главного потока нельзя: просим у системы время и
        // уходим в отдельный поток.
        val result = goAsync()
        Thread {
          try {
            ExpiryReminder.check(app)
          } catch (e: Exception) {
            // напоминание не должно ронять приложение
          } finally {
            result.finish()
          }
        }.start()
      }
      Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED -> ExpiryReminder.rearm(app)
    }
  }
}
