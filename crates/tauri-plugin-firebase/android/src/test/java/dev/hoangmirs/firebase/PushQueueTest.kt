package dev.hoangmirs.firebase

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class PushQueueTest {
  private val hour = 60 * 60 * 1000L
  private val now = 100 * 24 * hour

  private fun refresh(at: Long) = PushEntry("refresh", "{\"what\":\"inbox\"}", at)

  @Test
  fun the_queue_keeps_20_and_drops_entries_older_than_a_day() {
    val old = refresh(now - 25 * hour)
    val recent = (1..21).map { refresh(now - 21 * hour + it * hour) }
    var queue = listOf(old)
    for (entry in recent) queue = PushQueue.add(queue, entry, now)

    assertEquals(20, queue.size)
    assertEquals(recent.drop(1), queue)
    assertEquals(emptyList<PushEntry>(), PushQueue.kept(queue, now + 25 * hour))
  }

  @Test
  fun clear_is_not_queued() {
    val queue = listOf(refresh(now))
    assertFalse(PushQueue.queues("clear"))
    assertEquals(queue, PushQueue.add(queue, PushEntry("clear", "{\"group\":\"all\"}", now), now))
  }
}
