package dev.hoangmirs.firebase

import app.tauri.plugin.JSObject
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class PushService : FirebaseMessagingService() {
  // With no live plugin, the next `push_token` call reads the new token.
  override fun onNewToken(token: String) {
    FirebasePlugin.live()?.emit("pushToken", JSObject().put("token", token))
  }

  override fun onMessageReceived(message: RemoteMessage) {
    val shown = message.notification
    if (shown != null) {
      arrived(shown, message.data)
      return
    }
    val action = message.data["action"] ?: return
    Push.receive(applicationContext, action, message.data["data"])
  }

  // The library calls this only while the app is open, and then it shows
  // nothing. The app decides what to show, from the same shape the iOS
  // notification plugin gives: `title`, `body` and `extra`.
  private fun arrived(shown: RemoteMessage.Notification, data: Map<String, String>) {
    val extra = JSObject()
    for (key in listOf("link", "kind", "id", "group")) data[key]?.let { extra.put(key, it) }
    FirebasePlugin.live()?.emit(
      "pushArrived",
      JSObject().put("title", shown.title ?: "").put("body", shown.body ?: "").put("extra", extra),
    )
  }
}
