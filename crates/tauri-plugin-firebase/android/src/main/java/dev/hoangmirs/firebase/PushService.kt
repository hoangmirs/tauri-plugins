package dev.hoangmirs.firebase

import app.tauri.plugin.JSObject
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage

class PushService : FirebaseMessagingService() {
  // With no live plugin, the next `push_token` call reads the new token.
  override fun onNewToken(token: String) {
    FirebasePlugin.live()?.emit("pushToken", JSObject().put("token", token))
  }

  // A message with a notification part is the library's to show, and it shows
  // nothing while the app is open.
  override fun onMessageReceived(message: RemoteMessage) {
    if (message.notification != null) return
    val action = message.data["action"] ?: return
    Push.receive(applicationContext, action, message.data["data"])
  }
}
