package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Test

class ServicePolicyTest {
	private val policy = ServicePolicy()
	private val ringing = CallInfo(active = true, incoming = true, muted = false, peer = "Laptop")
	private val connected = ringing.copy(incoming = false)

	@Test
	fun idleDoesNothing() {
		assertEquals(ServiceAction.None, policy.decide(CallInfo.IDLE, microphoneGranted = true))
	}

	@Test
	fun aCallStartsTheServiceOnceThenOnlyUpdatesOnChange() {
		assertEquals(ServiceAction.Start(ringing), policy.decide(ringing, true))
		assertEquals(ServiceAction.None, policy.decide(ringing, true))
		assertEquals(ServiceAction.Update(connected), policy.decide(connected, true))
		assertEquals(ServiceAction.Update(connected.copy(muted = true)), policy.decide(connected.copy(muted = true), true))
	}

	@Test
	fun theEndOfTheCallStopsTheService() {
		policy.decide(connected, true)
		assertEquals(ServiceAction.Stop, policy.decide(CallInfo.IDLE, true))
		assertEquals(ServiceAction.None, policy.decide(CallInfo.IDLE, true))
	}

	@Test
	fun withoutTheMicrophoneThePermissionIsRequestedNotTheService() {
		assertEquals(ServiceAction.NeedMicrophone, policy.decide(connected, microphoneGranted = false))
		// Granted later: the same call now starts the service.
		assertEquals(ServiceAction.Start(connected), policy.decide(connected, microphoneGranted = true))
	}

	@Test
	fun aServiceKilledBySystemRestartsOnTheNextEvaluation() {
		policy.decide(connected, true)
		policy.stoppedBySystem()
		assertEquals(ServiceAction.Start(connected), policy.decide(connected, true))
	}

	@Test
	fun aRingingCallWithNoScreenDoesNotStartTheMicrophoneService() {
		assertEquals(ServiceAction.None, policy.decide(ringing, microphoneGranted = true, visible = false))
		// Answered from the notification: the call is no longer ringing and the service starts.
		assertEquals(ServiceAction.Start(connected), policy.decide(connected, microphoneGranted = true, visible = false))
	}

	@Test
	fun anAutoAnsweredCallInTheBackgroundStartsTheService() {
		val auto = connected.copy(autoAnswered = true)
		assertEquals(ServiceAction.Start(auto), policy.decide(auto, microphoneGranted = true, visible = false))
	}
}
