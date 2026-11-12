package top.cryvpn.app

import android.app.Activity
import android.content.Intent
import android.net.VpnService
import android.os.Build
import android.provider.Settings
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

@InvokeArg
class StartArgs {
  var dir: String = ""
  var core: String = ""
  var assets: String = ""
  var bypass: Array<String> = arrayOf()
  var title: String = ""
}

@InvokeArg
class AssetsArgs {
  var dir: String = ""
}

@InvokeArg
class InstallArgs {
  var path: String = ""
}

@InvokeArg
class TextArgs {
  var text: String = ""
}

/** Мост между приложением и Android: туннель, сведения об устройстве, шифрование. */
@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
  private var pendingDir = ""
  private var pendingCore = ""
  private var pendingAssets = ""
  private var pendingBypass: Array<String> = arrayOf()
  private var pendingTitle = ""

  /**
   * С Android 13 уведомления показываются только с согласия человека. Без
   * него служба VPN работает, но в шторке её не видно — и непонятно,
   * включён ли VPN, пока приложение свёрнуто.
   *
   * Ответа обязательно дожидаемся: уведомление службы, отправленное до
   * согласия, система выбрасывает насовсем и после согласия не возвращает.
   * Откажет — туннель всё равно поднимется, просто без уведомления.
   */
  private fun needNotificationConsent(): Boolean {
    if (Build.VERSION.SDK_INT < 33) return false
    val permission = android.Manifest.permission.POST_NOTIFICATIONS
    if (activity.checkSelfPermission(permission) == android.content.pm.PackageManager.PERMISSION_GRANTED) return false
    // Человек уже отказал насовсем — система окно не покажет и не ответит.
    return !askedBefore() || activity.shouldShowRequestPermissionRationale(permission)
  }

  private fun askedBefore(): Boolean {
    val prefs = activity.getSharedPreferences("cryvpn", android.content.Context.MODE_PRIVATE)
    val asked = prefs.getBoolean("asked_notifications", false)
    prefs.edit().putBoolean("asked_notifications", true).apply()
    return asked
  }

  companion object {
    const val NOTIFICATIONS_REQUEST = 7001

    /** Что сделать, когда человек ответил на вопрос об уведомлениях. */
    @Volatile var afterNotificationAnswer: (() -> Unit)? = null
  }

  /** Установленные приложения со значком запуска, кроме нас самих. */
  @Command
  fun apps(invoke: Invoke) {
    Thread {
      val pm = activity.packageManager
      val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
      val list = app.tauri.plugin.JSArray()
      pm.queryIntentActivities(launcher, 0)
        .map { it.activityInfo.packageName to it.loadLabel(pm).toString() }
        .filter { it.first != activity.packageName }
        .distinctBy { it.first }
        .sortedBy { it.second.lowercase() }
        .forEach { (id, name) ->
          val item = JSObject()
          item.put("id", id)
          item.put("name", name)
          list.put(item)
        }
      val out = JSObject()
      out.put("apps", list)
      invoke.resolve(out)
    }.start()
  }

  /** Сведения об устройстве и о том, где лежит ядро. */
  @Command
  fun info(invoke: Invoke) {
    val out = JSObject()
    out.put("id", Settings.Secure.getString(activity.contentResolver, Settings.Secure.ANDROID_ID) ?: "")
    out.put("model", (Build.MANUFACTURER + " " + Build.MODEL).trim())
    out.put("release", Build.VERSION.RELEASE ?: "")
    out.put("libs", activity.applicationInfo.nativeLibraryDir)
    // Скачанный в прошлый раз APK уже не нужен: либо он установлен, либо
    // от установки отказались.
    File(activity.cacheDir, "update").deleteRecursively()
    invoke.resolve(out)
  }

  /**
   * Установка обновления. Файл уже скачан и сверен с контрольной суммой;
   * наше дело — отдать его системному установщику. Ставит и спрашивает
   * согласие сама система, обойти это нельзя и не нужно.
   */
  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(InstallArgs::class.java)
    Thread {
      try {
        val source = File(args.path)
        val dir = File(activity.cacheDir, "update")
        dir.mkdirs()
        val apk = File(dir, "CryVPN-Android.apk")
        source.copyTo(apk, overwrite = true)
        source.delete()
        val uri = androidx.core.content.FileProvider.getUriForFile(
          activity, activity.packageName + ".fileprovider", apk
        )
        val intent = Intent(Intent.ACTION_VIEW)
          .setDataAndType(uri, "application/vnd.android.package-archive")
          .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        activity.runOnUiThread {
          try {
            activity.startActivity(intent)
            invoke.resolve(JSObject())
          } catch (e: Exception) {
            invoke.reject("Android не открыл установку: ${e.message}")
          }
        }
      } catch (e: Exception) {
        invoke.reject("не удалось подготовить обновление: ${e.message}")
      }
    }.start()
  }

  /** Базы geoip/geosite лежат внутри APK; ядру нужны обычные файлы. */
  @Command
  fun assets(invoke: Invoke) {
    val args = invoke.parseArgs(AssetsArgs::class.java)
    Thread {
      try {
        val dir = File(args.dir)
        dir.mkdirs()
        // Распаковываем один раз на каждую установленную версию: метка —
        // время последнего обновления приложения.
        val version = activity.packageManager.getPackageInfo(activity.packageName, 0).lastUpdateTime.toString()
        val stamp = File(dir, ".stamp")
        val names = arrayOf("geoip.dat", "geosite.dat")
        val ready = stamp.exists() && stamp.readText() == version && names.all { File(dir, it).exists() }
        if (!ready) {
          stamp.delete()
          for (name in names) {
            activity.assets.open(name).use { input ->
              File(dir, name).outputStream().use { input.copyTo(it) }
            }
          }
          stamp.writeText(version)
        }
        invoke.resolve(JSObject())
      } catch (e: Exception) {
        invoke.reject("не удалось подготовить базы адресов: ${e.message}")
      }
    }.start()
  }

  /** Поднять туннель. В первый раз Android сам спросит у человека согласие. */
  @Command
  fun start(invoke: Invoke) {
    val args = invoke.parseArgs(StartArgs::class.java)
    pendingDir = args.dir
    pendingCore = args.core
    pendingAssets = args.assets
    pendingBypass = args.bypass
    pendingTitle = args.title
    if (needNotificationConsent()) {
      afterNotificationAnswer = { activity.runOnUiThread { askVpnConsent(invoke) } }
      activity.requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), NOTIFICATIONS_REQUEST)
    } else {
      askVpnConsent(invoke)
    }
  }

  private fun askVpnConsent(invoke: Invoke) {
    val consent = VpnService.prepare(activity)
    if (consent != null) {
      startActivityForResult(invoke, consent, "onConsent")
    } else {
      launch(invoke)
    }
  }

  @ActivityCallback
  fun onConsent(invoke: Invoke, result: ActivityResult) {
    if (result.resultCode == Activity.RESULT_OK) {
      launch(invoke)
    } else {
      invoke.reject("Без разрешения на VPN подключиться нельзя")
    }
  }

  /**
   * Запускаем службу и сразу отвечаем приложению. Служба живёт в другом
   * процессе и о своих делах сообщает файлами (см. CryVpnService); дождаться
   * туннеля — забота приложения, у него на это есть срок.
   */
  private fun launch(invoke: Invoke) {
    try {
      val intent = Intent(activity, CryVpnService::class.java)
        .putExtra(CryVpnService.EXTRA_DIR, pendingDir)
        .putExtra(CryVpnService.EXTRA_CORE, pendingCore)
        .putExtra(CryVpnService.EXTRA_ASSETS, pendingAssets)
        .putExtra(CryVpnService.EXTRA_BYPASS, pendingBypass)
        .putExtra(CryVpnService.EXTRA_TITLE, pendingTitle)
      if (Build.VERSION.SDK_INT >= 26) activity.startForegroundService(intent) else activity.startService(intent)
      invoke.resolve(JSObject())
    } catch (e: Exception) {
      invoke.reject("Android не дал запустить службу VPN: ${e.message}")
    }
  }

  @Command
  fun stop(invoke: Invoke) {
    activity.startService(
      Intent(activity, CryVpnService::class.java).setAction(CryVpnService.ACTION_STOP)
    )
    invoke.resolve(JSObject())
  }

  // --- шифрование ключом из защищённого хранилища Android ---------------
  // Ключ не покидает хранилище устройства: файл, вытащенный из телефона,
  // без него не расшифровать.

  private fun key(): SecretKey {
    val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
    (store.getKey("cryvpn", null) as? SecretKey)?.let { return it }
    val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
    generator.init(
      KeyGenParameterSpec.Builder("cryvpn", KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
        .setKeySize(256)
        .build()
    )
    return generator.generateKey()
  }

  @Command
  fun seal(invoke: Invoke) {
    val args = invoke.parseArgs(TextArgs::class.java)
    try {
      val cipher = Cipher.getInstance("AES/GCM/NoPadding")
      cipher.init(Cipher.ENCRYPT_MODE, key())
      val sealed = cipher.iv + cipher.doFinal(args.text.toByteArray(Charsets.UTF_8))
      val out = JSObject()
      out.put("text", Base64.encodeToString(sealed, Base64.NO_WRAP))
      invoke.resolve(out)
    } catch (e: Exception) {
      invoke.reject("шифрование не удалось")
    }
  }

  @Command
  fun unseal(invoke: Invoke) {
    val args = invoke.parseArgs(TextArgs::class.java)
    try {
      val raw = Base64.decode(args.text, Base64.NO_WRAP)
      val cipher = Cipher.getInstance("AES/GCM/NoPadding")
      cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw, 0, 12))
      val out = JSObject()
      out.put("text", String(cipher.doFinal(raw, 12, raw.size - 12), Charsets.UTF_8))
      invoke.resolve(out)
    } catch (e: Exception) {
      invoke.reject("расшифровка не удалась")
    }
  }
}
