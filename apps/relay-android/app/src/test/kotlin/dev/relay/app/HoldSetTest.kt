package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class HoldSetTest {
    private class FakeLock : Releasable {
        var acquires = 0
        var releases = 0
        override fun acquire() { acquires++ }
        override fun release() { releases++ }
    }

    private val lock = FakeLock()
    private val holds = HoldSet(lock)

    @Test
    fun theLockIsAcquiredOnceForTwoOwnersAndReleasedWithTheLast() {
        holds.hold("activity")
        holds.hold("service")
        assertEquals(1, lock.acquires)
        holds.drop("activity")
        assertTrue(holds.held)
        assertEquals(0, lock.releases)
        holds.drop("service")
        assertFalse(holds.held)
        assertEquals(1, lock.releases)
    }

    @Test
    fun repeatedHoldsAndDropsAreHarmless() {
        holds.hold("activity")
        holds.hold("activity")
        holds.drop("activity")
        holds.drop("activity")
        assertEquals(1, lock.acquires)
        assertEquals(1, lock.releases)
    }
}
