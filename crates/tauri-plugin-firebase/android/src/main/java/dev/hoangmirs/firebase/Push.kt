package dev.hoangmirs.firebase

import android.app.NotificationManager
import android.content.Context
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener

/** One silent push: its action, its data as the JSON text it came with, and
 *  when it came, in milliseconds since 1970. */
data class PushEntry(val action: String, val data: String?, val at: Long)

object PushQueue {
  const val MAX = 20
  const val MAX_AGE_MILLIS = 24 * 60 * 60 * 1000L
  const val CLEAR = "clear"

  fun queues(action: String) = action != CLEAR

  fun kept(entries: List<PushEntry>, now: Long) = entries.filter { now - it.at < MAX_AGE_MILLIS }.takeLast(MAX)

  fun add(entries: List<PushEntry>, entry: PushEntry, now: Long) =
    kept(if (queues(entry.action)) entries + entry else entries, now)
}

/** The queue in the app's own preferences, so an app the system stopped still
 *  finds its pushes at the next start. */
object Push {
  private const val PREFS = "dev.hoangmirs.firebase.push"
  private const val KEY = "queue"
  private val lock = Any()

  /** Handles a data-only message. `clear` removes the delivered notifications
   *  of its group, and every other action joins the queue. */
  fun receive(context: Context, action: String, data: String?) {
    if (!PushQueue.queues(action)) {
      (parse(data) as? JSONObject)?.optString("group")?.takeIf { it.isNotEmpty() }?.let { clear(context, it) }
      return
    }
    val entry = PushEntry(action, data, System.currentTimeMillis())
    synchronized(lock) { write(context, PushQueue.add(read(context), entry, entry.at)) }
    FirebasePlugin.live()?.emit("pushData", json(entry))
  }

  /** Answers the queue, oldest first, and empties it. */
  fun take(context: Context): JSArray {
    val entries = synchronized(lock) {
      val kept = PushQueue.kept(read(context), System.currentTimeMillis())
      write(context, emptyList())
      kept
    }
    val out = JSArray()
    entries.forEach { out.put(json(it)) }
    return out
  }

  private fun clear(context: Context, group: String) {
    val manager = context.getSystemService(NotificationManager::class.java) ?: return
    manager.activeNotifications.filter { it.tag == group }.forEach { manager.cancel(it.tag, it.id) }
  }

  private fun json(entry: PushEntry): JSObject =
    JSObject().put("action", entry.action).put("data", parse(entry.data)).put("at", entry.at)

  private fun parse(text: String?): Any = try {
    text?.let { JSONTokener(it).nextValue() } ?: JSONObject.NULL
  } catch (e: Exception) {
    JSONObject.NULL
  }

  private fun prefs(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  private fun read(context: Context): List<PushEntry> = try {
    val array = JSONArray(prefs(context).getString(KEY, "[]"))
    (0 until array.length()).map { i ->
      val o = array.getJSONObject(i)
      PushEntry(o.getString("action"), if (o.isNull("data")) null else o.getString("data"), o.getLong("at"))
    }
  } catch (e: Exception) {
    emptyList()
  }

  private fun write(context: Context, entries: List<PushEntry>) {
    val array = JSONArray()
    entries.forEach { array.put(JSONObject().put("action", it.action).put("data", it.data ?: JSONObject.NULL).put("at", it.at)) }
    prefs(context).edit().putString(KEY, array.toString()).commit()
  }
}
