package dev.hoangmirs.sharefile

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Base64
import androidx.activity.result.ActivityResult
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.FileProvider
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File
import java.util.concurrent.Executors

@InvokeArg
class ShareArgs {
  lateinit var data: String
  lateinit var name: String
  lateinit var mime: String
  var title: String? = null
}

/** Writes one file to the app's cache and opens the system share sheet for
 *  it. Answers `shared` when the player picks an app, `cancelled` when the
 *  sheet closes without a pick. */
@TauriPlugin
class ShareFilePlugin(private val activity: Activity) : Plugin(activity) {
  private val main = Handler(Looper.getMainLooper())
  private val worker = Executors.newSingleThreadExecutor()
  private val chosenAction = "${activity.packageName}.sharefile.CHOSEN"

  /** The open share sheet. Touched on the main thread only. */
  private var current: Sheet? = null

  private inner class Sheet(val invoke: Invoke) {
    var chosen = false
    var closed = false
    var done = false
    val late = Runnable { end() }
    val receiver =
      object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
          chosen = true
          if (closed) end()
        }
      }

    private fun close() {
      done = true
      main.removeCallbacks(late)
      runCatching { activity.unregisterReceiver(receiver) }
      if (current === this) current = null
    }

    fun end() {
      if (done) return
      close()
      invoke.resolve(JSObject().apply { put("outcome", if (chosen) "shared" else "cancelled") })
    }

    fun fail(e: Exception) {
      if (done) return
      close()
      invoke.reject(e.message ?: "the share sheet did not open", e)
    }
  }

  @Command
  fun share(invoke: Invoke) {
    val args = invoke.parseArgs(ShareArgs::class.java)
    worker.execute {
      val uri =
        try {
          write(args)
        } catch (e: Exception) {
          invoke.reject(e.message ?: "the file could not be written", e)
          return@execute
        }
      main.post { open(invoke, uri, args) }
    }
  }

  private fun write(args: ShareArgs): Uri {
    val name = args.name
    require(name.isNotEmpty() && !name.contains('/') && !name.contains('\\') && !name.contains("..")) {
      "a file name has no path separator and no '..'"
    }
    val bytes = Base64.decode(args.data, Base64.DEFAULT)
    require(bytes.isNotEmpty()) { "the file is empty" }
    // Cleared before a write and not after a share: the app the player
    // picked may still be reading the last file.
    val dir = File(activity.cacheDir, "share-file")
    dir.deleteRecursively()
    check(dir.mkdirs()) { "the share folder could not be made" }
    val file = File(dir, name)
    file.writeBytes(bytes)
    return FileProvider.getUriForFile(activity, "${activity.packageName}.sharefile", file)
  }

  private fun open(invoke: Invoke, uri: Uri, args: ShareArgs) {
    current?.end()
    val sheet = Sheet(invoke)
    try {
      val send =
        Intent(Intent.ACTION_SEND).apply {
          type = args.mime
          putExtra(Intent.EXTRA_STREAM, uri)
          // Android 10 and later show the sheet's preview from the clip.
          clipData = ClipData.newRawUri("", uri)
          addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
      // The system writes the chosen app into this intent, so it must be
      // mutable. Android 14 refuses a mutable one without an explicit target.
      var flags = PendingIntent.FLAG_UPDATE_CURRENT
      if (Build.VERSION.SDK_INT >= 31) flags = flags or PendingIntent.FLAG_MUTABLE
      val picked =
        PendingIntent.getBroadcast(
          activity,
          0,
          Intent(chosenAction).setPackage(activity.packageName),
          flags,
        )
      val filter = IntentFilter(chosenAction)
      if (Build.VERSION.SDK_INT >= 33) {
        activity.registerReceiver(sheet.receiver, filter, Context.RECEIVER_NOT_EXPORTED)
      } else {
        // No private registration exists below Android 13. Another app that
        // sends this action can only turn a cancel into "shared".
        activity.registerReceiver(sheet.receiver, filter)
      }
      current = sheet
      startActivityForResult(
        invoke,
        Intent.createChooser(send, args.title, picked.intentSender),
        "closed",
      )
    } catch (e: Exception) {
      sheet.fail(e)
    }
  }

  // Tauri finds the callback by this signature. The chooser's result code
  // does not say whether the player picked an app, so it goes unread.
  @Suppress("UNUSED_PARAMETER")
  @ActivityCallback
  fun closed(invoke: Invoke, result: ActivityResult) {
    val sheet = current
    if (sheet == null || sheet.invoke !== invoke) return
    sheet.closed = true
    // The pick's broadcast can arrive just after the chooser closes.
    if (sheet.chosen) sheet.end() else main.postDelayed(sheet.late, 500)
  }

  override fun onDestroy(activity: AppCompatActivity) {
    current?.end()
    super.onDestroy(activity)
  }
}
