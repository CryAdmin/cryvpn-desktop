package top.cryvpn.app

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.view.View
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    // Тёмный фон под строкой состояния — значит, значки на ней светлые.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT)
    )
    super.onCreate(savedInstanceState)
    noteTileRequest(intent)
    // Окно рисуется под строкой состояния и под жестовой полосой. Фон там
    // наш, тёмный, а само содержимое отодвигаем, чтобы его не перекрывало.
    val root = findViewById<View>(android.R.id.content)
    root.setBackgroundColor(Color.parseColor("#0a0c10"))
    ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
      val bars = insets.getInsets(
        WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout() or WindowInsetsCompat.Type.ime()
      )
      view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
      WindowInsetsCompat.CONSUMED
    }
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    noteTileRequest(intent)
  }

  /** Нажали плитку в шторке: запоминаем просьбу, приложение её заберёт. */
  private fun noteTileRequest(intent: Intent?) {
    if (intent?.action == CryTileService.ACTION_CONNECT) {
      VpnPlugin.tileRequestAt = System.currentTimeMillis()
      // Просьба одноразовая: поворот экрана не должен повторить её.
      intent.action = Intent.ACTION_MAIN
    }
  }

  /** Ответ на вопрос об уведомлениях — передаём тому, кто его ждёт. */
  override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
    super.onRequestPermissionsResult(requestCode, permissions, grantResults)
    if (requestCode == VpnPlugin.NOTIFICATIONS_REQUEST) {
      val next = VpnPlugin.afterNotificationAnswer
      VpnPlugin.afterNotificationAnswer = null
      next?.invoke()
    }
  }
}
