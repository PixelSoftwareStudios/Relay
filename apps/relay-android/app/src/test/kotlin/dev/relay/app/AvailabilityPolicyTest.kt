package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AvailabilityPolicyTest {
	private val policy = AvailabilityPolicy()
	private val lan = AvailabilityInput(enabled = true, paused = false, onLan = true)

	@Test
	fun theTargetFollowsTheSettingPauseAndTheNetwork() {
		assertEquals(Availability.ACTIVE, lan.target())
		assertEquals(Availability.WAITING, lan.copy(onLan = false).target())
		assertEquals(Availability.OFF, lan.copy(enabled = false).target())
		assertEquals(Availability.OFF, lan.copy(paused = true).target())
		assertEquals(Availability.OFF, lan.copy(enabled = false, onLan = false).target())
	}

	@Test
	fun offDoesNothingUntilTheSettingTurnsOn() {
		assertEquals(AvailabilityAction.None, policy.decide(lan.copy(enabled = false)))
		assertEquals(AvailabilityAction.Start(Availability.ACTIVE), policy.decide(lan))
		assertEquals(AvailabilityAction.None, policy.decide(lan))
	}

	@Test
	fun losingTheNetworkWaitsAndTheNetworkReturningActivatesWithoutRestarting() {
		policy.decide(lan)
		assertEquals(AvailabilityAction.Change(Availability.WAITING), policy.decide(lan.copy(onLan = false)))
		assertEquals(AvailabilityAction.None, policy.decide(lan.copy(onLan = false)))
		assertEquals(AvailabilityAction.Change(Availability.ACTIVE), policy.decide(lan))
	}

	@Test
	fun startingWithoutANetworkStartsWaiting() {
		assertEquals(AvailabilityAction.Start(Availability.WAITING), policy.decide(lan.copy(onLan = false)))
	}

	@Test
	fun switchingTheSettingOffStopsTheServiceOnce() {
		policy.decide(lan)
		assertEquals(AvailabilityAction.Stop, policy.decide(lan.copy(enabled = false)))
		assertEquals(AvailabilityAction.None, policy.decide(lan.copy(enabled = false)))
	}

	@Test
	fun pauseStopsAndOpeningTheAppResumes() {
		policy.decide(lan)
		assertEquals(AvailabilityAction.Stop, policy.decide(lan.copy(paused = true)))
		assertEquals(AvailabilityAction.Start(Availability.ACTIVE), policy.decide(lan))
	}

	@Test
	fun aServiceTheSystemStoppedIsStartedAgainOnTheNextEvaluation() {
		policy.decide(lan)
		policy.stoppedBySystem()
		assertEquals(AvailabilityAction.Start(Availability.ACTIVE), policy.decide(lan))
	}

	@Test
	fun onlyWifiAndEthernetAreALan() {
		assertTrue(Lan.isLan(setOf(Lan.Transport.WIFI)))
		assertTrue(Lan.isLan(setOf(Lan.Transport.ETHERNET, Lan.Transport.VPN)))
		assertTrue(Lan.isLan(setOf(Lan.Transport.WIFI, Lan.Transport.VPN)))
		assertFalse(Lan.isLan(setOf(Lan.Transport.CELLULAR)))
		assertFalse(Lan.isLan(setOf(Lan.Transport.VPN)))
		assertFalse(Lan.isLan(emptySet()))
		assertTrue(Lan.anyLan(listOf(setOf(Lan.Transport.CELLULAR), setOf(Lan.Transport.WIFI))))
		assertFalse(Lan.anyLan(emptyList()))
	}
}
