package dev.hoangmirs.silentmode

import android.media.AudioManager
import org.junit.Assert.assertEquals
import org.junit.Test

class ModesTest {
  @Test
  fun eachRingerModeHasTheNameAWebviewReads() {
    assertEquals("silent", Modes.name(AudioManager.RINGER_MODE_SILENT))
    assertEquals("vibrate", Modes.name(AudioManager.RINGER_MODE_VIBRATE))
    assertEquals("normal", Modes.name(AudioManager.RINGER_MODE_NORMAL))
  }

  @Test
  fun aModeANewerAndroidAddsStaysAudible() {
    assertEquals("normal", Modes.name(42))
  }
}
