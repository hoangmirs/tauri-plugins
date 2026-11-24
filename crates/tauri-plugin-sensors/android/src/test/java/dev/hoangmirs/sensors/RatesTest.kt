package dev.hoangmirs.sensors

import org.junit.Assert.assertEquals
import org.junit.Test

class RatesTest {
  @Test
  fun sixtyReadingsASecondArriveEvery16666Microseconds() {
    assertEquals(16_666, Rates.periodUs(60))
  }

  @Test
  fun aRateBelowOneRunsOnceASecond() {
    assertEquals(1_000_000, Rates.periodUs(0))
  }

  @Test
  fun aRateAboveSixtyRunsAtSixty() {
    assertEquals(16_666, Rates.periodUs(500))
  }
}
