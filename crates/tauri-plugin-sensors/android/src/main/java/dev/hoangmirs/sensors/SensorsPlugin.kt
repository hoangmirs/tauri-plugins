package dev.hoangmirs.sensors

import android.app.Activity
import android.content.Context
import android.content.pm.ActivityInfo
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class StartArgs {
  var kind: String = ""
  var hz: Int = 30
}

@InvokeArg
class StopArgs {
  var kind: String = ""
}

private const val STANDARD_GRAVITY = 9.80665
private val KINDS = setOf("motion", "heading")

/** Android reports acceleration as the force that holds the phone up, so a
 *  phone lying face up reads +1 g on z. CoreMotion reports the pull itself,
 *  -1 g on z. Both kinds of reading take CoreMotion's signs, so a reading
 *  means the same on both systems. */
private fun inG(v: FloatArray, into: DoubleArray) {
  for (i in 0..2) into[i] = -v[i] / STANDARD_GRAVITY
}

/** Sensors run only while a kind is started and the app is in front. Every
 *  field is read and changed on the main thread: commands move there, and
 *  sensor events arrive there. */
@TauriPlugin
class SensorsPlugin(private val activity: Activity) : Plugin(activity) {
  private val sensors = activity.getSystemService(Context.SENSOR_SERVICE) as SensorManager
  private val gravity: Sensor? = sensors.getDefaultSensor(Sensor.TYPE_GRAVITY)
  private val linear: Sensor? = sensors.getDefaultSensor(Sensor.TYPE_LINEAR_ACCELERATION)
  private val accelerometer: Sensor? = sensors.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
  private val gyroscope: Sensor? = sensors.getDefaultSensor(Sensor.TYPE_GYROSCOPE)
  private val rotation: Sensor? = sensors.getDefaultSensor(Sensor.TYPE_ROTATION_VECTOR)

  private val hasMotion = (gravity != null || accelerometer != null) && gyroscope != null
  private val hasHeading = rotation != null

  private val periods = mutableMapOf<String, Int>()
  private var inFront = true
  private var orientationBeforeLock: Int? = null

  private val motion = MotionListener()
  private val heading = HeadingListener()

  @Command
  fun available(invoke: Invoke) {
    val kinds = JSArray()
    if (hasMotion) kinds.put("motion")
    if (hasHeading) kinds.put("heading")
    invoke.resolve(JSObject().apply { put("kinds", kinds) })
  }

  @Command
  fun start(invoke: Invoke) {
    val args = invoke.parseArgs(StartArgs::class.java)
    if (args.kind !in KINDS) {
      invoke.reject("no sensor kind ${args.kind}")
      return
    }
    activity.runOnUiThread {
      periods[args.kind] = Rates.periodUs(args.hz)
      listen()
      invoke.resolve()
    }
  }

  @Command
  fun stop(invoke: Invoke) {
    val args = invoke.parseArgs(StopArgs::class.java)
    activity.runOnUiThread {
      if (periods.remove(args.kind) != null) listen()
      invoke.resolve()
    }
  }

  @Command
  fun lockRotation(invoke: Invoke) {
    activity.runOnUiThread {
      if (orientationBeforeLock == null) orientationBeforeLock = activity.requestedOrientation
      activity.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_LOCKED
      invoke.resolve()
    }
  }

  @Command
  fun unlockRotation(invoke: Invoke) {
    activity.runOnUiThread {
      orientationBeforeLock?.let { activity.requestedOrientation = it }
      orientationBeforeLock = null
      invoke.resolve()
    }
  }

  override fun onPause() {
    inFront = false
    listen()
  }

  override fun onResume() {
    inFront = true
    listen()
  }

  // The newer overload takes an AppCompatActivity, a type tauri-android keeps
  // to itself, so this plugin would need a library of its own to name it.
  @Suppress("OVERRIDE_DEPRECATION")
  override fun onDestroy() {
    periods.clear()
    sensors.unregisterListener(motion)
    sensors.unregisterListener(heading)
  }

  private fun listen() {
    sensors.unregisterListener(motion)
    sensors.unregisterListener(heading)
    if (!inFront) return
    periods["motion"]?.takeIf { hasMotion }?.let { us ->
      motion.reset(us)
      for (sensor in motion.sensors()) sensors.registerListener(motion, sensor, us)
    }
    periods["heading"]?.let { us ->
      heading.reset(us)
      rotation?.let { sensors.registerListener(heading, it, us) }
    }
  }

  /** Android paces readings only roughly, so each listener skips one that
   *  comes well before its period. */
  private abstract class Paced : SensorEventListener {
    private var periodNs = 0L
    private var lastNs = Long.MIN_VALUE

    fun reset(periodUs: Int) {
      periodNs = periodUs * 1_000L
      lastNs = Long.MIN_VALUE
    }

    fun due(timestampNs: Long): Boolean {
      if (lastNs != Long.MIN_VALUE && timestampNs - lastNs < periodNs * 9 / 10) return false
      lastNs = timestampNs
      return true
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
  }

  /** Gravity and linear acceleration come from Android's own fusion when the
   *  phone has both. Without them, a low-pass filter splits the
   *  accelerometer into the two. */
  private inner class MotionListener : Paced() {
    private val fused = gravity != null && linear != null
    private val g = DoubleArray(3)
    private val r = DoubleArray(3)
    private val a = DoubleArray(3)
    private val smoothed = FloatArray(3)
    private val raw = FloatArray(3)
    private var primed = false

    fun sensors(): List<Sensor> =
      if (fused) listOfNotNull(gravity, linear, gyroscope) else listOfNotNull(accelerometer, gyroscope)

    override fun onSensorChanged(event: SensorEvent) {
      val v = event.values
      when (event.sensor.type) {
        Sensor.TYPE_GYROSCOPE -> for (i in 0..2) r[i] = v[i].toDouble()
        Sensor.TYPE_LINEAR_ACCELERATION -> inG(v, a)
        Sensor.TYPE_GRAVITY -> {
          inG(v, g)
          send(event.timestamp)
        }
        Sensor.TYPE_ACCELEROMETER -> {
          for (i in 0..2) {
            smoothed[i] = if (primed) 0.8f * smoothed[i] + 0.2f * v[i] else v[i]
            raw[i] = v[i] - smoothed[i]
          }
          primed = true
          inG(smoothed, g)
          inG(raw, a)
          send(event.timestamp)
        }
      }
    }

    private fun send(timestampNs: Long) {
      if (!due(timestampNs)) return
      trigger(
        "reading",
        JSObject().apply {
          put("kind", "motion")
          put("g", JSArray(g.toList()))
          put("r", JSArray(r.toList()))
          put("a", JSArray(a.toList()))
          put("at", timestampNs / 1_000_000.0)
        },
      )
    }
  }

  private inner class HeadingListener : Paced() {
    private val matrix = FloatArray(9)
    private val angles = FloatArray(3)

    override fun onSensorChanged(event: SensorEvent) {
      if (!due(event.timestamp)) return
      // Some phones send five values, and older Androids throw on more than four.
      val v = if (event.values.size > 4) event.values.copyOf(4) else event.values
      SensorManager.getRotationMatrixFromVector(matrix, v)
      SensorManager.getOrientation(matrix, angles)
      val deg = (Math.toDegrees(angles[0].toDouble()) + 360.0) % 360.0
      trigger(
        "reading",
        JSObject().apply {
          put("kind", "heading")
          put("deg", deg)
          put("at", event.timestamp / 1_000_000.0)
        },
      )
    }
  }
}
