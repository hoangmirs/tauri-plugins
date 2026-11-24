package dev.hoangmirs.sensors

object Rates {
  /** The time between readings at `hz` readings a second, held to 1…60. */
  fun periodUs(hz: Int): Int = 1_000_000 / hz.coerceIn(1, 60)
}
