package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LingeringHoldTest {
	private class Target : Releasable {
		var acquires = 0
		var releases = 0
		override fun acquire() { acquires++ }
		override fun release() { releases++ }
	}

	private class Clock : Scheduler {
		private var now = 0L
		private val tasks = mutableListOf<Triple<Long, () -> Unit, BooleanArray>>()

		override fun after(delayMs: Long, task: () -> Unit): () -> Unit {
			val cancelled = booleanArrayOf(false)
			tasks += Triple(now + delayMs, task, cancelled)
			return { cancelled[0] = true }
		}

		fun advance(ms: Long) {
			now += ms
			tasks.filter { it.first <= now && !it.third[0] }.forEach { it.third[0] = true; it.second() }
		}
	}

	private val target = Target()
	private val clock = Clock()
	private val hold = LingeringHold(target, clock, lingerMs = 1000)

	@Test
	fun theStateIsEnteredAtTheFirstOwnerAndOnlyOnce() {
		hold.hold("streams")
		hold.hold("service")
		assertEquals(1, target.acquires)
		assertTrue(hold.held)
	}

	@Test
	fun itIsLeftOnlyAfterTheLastOwnerAndTheLinger() {
		hold.hold("streams")
		hold.hold("service")
		hold.drop("streams")
		clock.advance(5000)
		assertTrue("the service still holds it", hold.held)
		hold.drop("service")
		clock.advance(999)
		assertEquals(0, target.releases)
		clock.advance(1)
		assertEquals(1, target.releases)
		assertFalse(hold.held)
	}

	@Test
	fun aStreamReopenedInsideTheLingerKeepsTheModeWithoutALeaveAndEnter() {
		hold.hold("streams")
		hold.drop("streams")
		clock.advance(300)
		hold.hold("streams")
		clock.advance(5000)
		assertEquals(1, target.acquires)
		assertEquals(0, target.releases)
	}

	@Test
	fun aRingEndingAndTheCallOpeningLaterThanTheLingerEntersAgain() {
		hold.hold("streams")
		hold.drop("streams")
		clock.advance(1000)
		hold.hold("streams")
		assertEquals(2, target.acquires)
		assertEquals(1, target.releases)
	}

	@Test
	fun droppingAnOwnerThatNeverHeldChangesNothing() {
		hold.drop("streams")
		clock.advance(5000)
		assertEquals(0, target.releases)
	}
}
