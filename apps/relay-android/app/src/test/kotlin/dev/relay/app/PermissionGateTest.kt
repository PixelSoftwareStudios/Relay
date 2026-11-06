package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PermissionGateTest {
    private val seen = mutableListOf<Gate>()
    private val gate = PermissionGate { seen += it }

    @Test
    fun firstLaunchWithoutPermissionAsks() {
        gate.checked(granted = false)
        assertEquals(Gate.NEEDS_REQUEST, gate.state)
        assertTrue(gate.shouldRequest())
        gate.requestStarted()
        assertEquals(Gate.REQUESTING, gate.state)
        assertFalse(gate.shouldRequest())
        gate.result(granted = true, canAskAgain = true)
        assertTrue(gate.granted)
        assertEquals(listOf(Gate.NEEDS_REQUEST, Gate.REQUESTING, Gate.GRANTED), seen)
    }

    @Test
    fun aRefusalThatCanBeRepeatedAsksAgainLater() {
        gate.checked(false)
        gate.requestStarted()
        gate.result(granted = false, canAskAgain = true)
        assertEquals(Gate.DENIED, gate.state)
        assertTrue(gate.refused)
        assertTrue(gate.shouldRequest())
    }

    @Test
    fun dontAskAgainBlocksUntilSettingsGrantsIt() {
        gate.checked(false)
        gate.requestStarted()
        gate.result(granted = false, canAskAgain = false)
        assertEquals(Gate.BLOCKED, gate.state)
        assertFalse(gate.shouldRequest())
        // Resuming without the permission keeps the verdict; granting in Settings clears it.
        gate.checked(false)
        assertEquals(Gate.BLOCKED, gate.state)
        gate.checked(true)
        assertEquals(Gate.GRANTED, gate.state)
    }

    @Test
    fun revokingInSettingsWhileRunningAsksAgain() {
        gate.checked(true)
        gate.checked(false)
        assertEquals(Gate.NEEDS_REQUEST, gate.state)
    }

    @Test
    fun unchangedAnswersDoNotNotify() {
        gate.checked(true)
        gate.checked(true)
        assertEquals(listOf(Gate.GRANTED), seen)
    }
}
