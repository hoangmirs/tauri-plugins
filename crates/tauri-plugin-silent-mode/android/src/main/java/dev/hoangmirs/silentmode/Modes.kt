package dev.hoangmirs.silentmode

import android.media.AudioManager

/** Android's ringer modes, by the names a webview reads. A mode a newer
 *  Android adds is read as normal: an app that cannot tell stays audible
 *  rather than going quiet for no reason the player can see. */
object Modes {
  fun name(ringerMode: Int): String =
    when (ringerMode) {
      AudioManager.RINGER_MODE_SILENT -> "silent"
      AudioManager.RINGER_MODE_VIBRATE -> "vibrate"
      else -> "normal"
    }
}
