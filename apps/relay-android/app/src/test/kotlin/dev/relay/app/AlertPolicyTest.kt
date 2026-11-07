package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AlertPolicyTest {
	private val ringing = CallInfo(active = true, incoming = true, muted = false, peer = "Laptop")

	@Test
	fun theEngineRingsForSystemAndRingAndVibrateOnly() {
		assertTrue(AlertPolicy.engineRings(CallAlert.SYSTEM))
		assertTrue(AlertPolicy.engineRings(CallAlert.RING_AND_VIBRATE))
		assertFalse(AlertPolicy.engineRings(CallAlert.VIBRATE))
		assertFalse(AlertPolicy.engineRings(CallAlert.SILENT))
	}

	@Test
	fun theNotificationNeverRingsWhenTheEngineDoes() {
		for (alert in CallAlert.entries) {
			val plan = AlertPolicy.plan(alert)
			assertFalse("$alert would ring twice", plan.sound)
		}
	}

	@Test
	fun withoutTheEngineRingTheSystemProfileIsLeftToAndroid() {
		val system = AlertPolicy.plan(CallAlert.SYSTEM, engineRings = false)
		assertTrue(system.sound)
		assertTrue(system.vibrate)
		assertEquals("incoming_system", system.channel)
		assertTrue(AlertPolicy.plan(CallAlert.RING_AND_VIBRATE, engineRings = false).sound)
	}

	@Test
	fun vibrateIsVibrationOnlyAndSilentIsNothing() {
		val vibrate = AlertPolicy.plan(CallAlert.VIBRATE)
		assertTrue(vibrate.vibrate)
		assertFalse(vibrate.sound)
		val silent = AlertPolicy.plan(CallAlert.SILENT)
		assertFalse(silent.vibrate)
		assertFalse(silent.sound)
	}

	@Test
	fun everyDistinctBehaviourHasItsOwnChannelBecauseChannelsAreImmutable() {
		val plans = CallAlert.entries.flatMap { listOf(AlertPolicy.plan(it, true), AlertPolicy.plan(it, false)) }
		for (a in plans) for (b in plans) {
			if (a.channel == b.channel) {
				assertEquals(a.sound, b.sound)
				assertEquals(a.vibrate, b.vibrate)
			}
		}
		assertNotEquals(AlertPolicy.plan(CallAlert.SYSTEM, true).channel, AlertPolicy.plan(CallAlert.SYSTEM, false).channel)
		assertEquals(
			setOf("incoming_system", "incoming_system_engine", "incoming_ring_vibrate", "incoming_ring_vibrate_engine", "incoming_vibrate", "incoming_silent"),
			plans.map { it.channel }.toSet(),
		)
	}

	@Test
	fun parsingTheWireNames() {
		assertEquals(CallAlert.SYSTEM, CallAlert.parse("system"))
		assertEquals(CallAlert.RING_AND_VIBRATE, CallAlert.parse("ring_and_vibrate"))
		assertEquals(CallAlert.VIBRATE, CallAlert.parse("vibrate"))
		assertEquals(CallAlert.SILENT, CallAlert.parse("silent"))
		assertEquals(CallAlert.SYSTEM, CallAlert.parse("something else"))
	}

	@Test
	fun aRingingCallWithNoScreenPostsOnceAndCancelsWhenItStops() {
		val policy = IncomingPolicy()
		val show = policy.decide(ringing, visible = false, alert = CallAlert.SYSTEM, canFullScreen = true)
		assertEquals(IncomingAction.Show(AlertPolicy.plan(CallAlert.SYSTEM), fullScreen = true), show)
		assertEquals(IncomingAction.None, policy.decide(ringing, false, CallAlert.SYSTEM, true))
		assertEquals(IncomingAction.Cancel, policy.decide(ringing.copy(incoming = false), false, CallAlert.SYSTEM, true))
		assertEquals(IncomingAction.None, policy.decide(CallInfo.IDLE, false, CallAlert.SYSTEM, true))
	}

	@Test
	fun aVisibleAppShowsTheCallItselfSoNoNotification() {
		val policy = IncomingPolicy()
		assertEquals(IncomingAction.None, policy.decide(ringing, visible = true, alert = CallAlert.SYSTEM, canFullScreen = true))
	}

	@Test
	fun openingTheAppWhileRingingWithdrawsTheNotification() {
		val policy = IncomingPolicy()
		policy.decide(ringing, false, CallAlert.SILENT, false)
		assertEquals(IncomingAction.Cancel, policy.decide(ringing, true, CallAlert.SILENT, false))
	}

	@Test
	fun withoutTheFullScreenGrantItIsOnlyAHeadsUp() {
		val show = IncomingPolicy().decide(ringing, false, CallAlert.VIBRATE, canFullScreen = false)
		assertEquals(IncomingAction.Show(AlertPolicy.plan(CallAlert.VIBRATE), fullScreen = false), show)
	}

	@Test
	fun theWakeLockIsOnlyForARingingCallWithNoScreen() {
		assertTrue(wakeForRinging(ringing, visible = false))
		assertFalse(wakeForRinging(ringing, visible = true))
		assertFalse(wakeForRinging(ringing.copy(incoming = false), visible = false))
		assertFalse(wakeForRinging(CallInfo.IDLE, visible = false))
	}

	@Test
	fun anAutoAnsweredCallIsAnnouncedOncePerCall() {
		val notice = AutoAnswerNotice()
		val auto = ringing.copy(incoming = false, autoAnswered = true)
		assertFalse(notice.changed(ringing))
		assertTrue(notice.changed(auto))
		assertFalse(notice.changed(auto))
		assertFalse(notice.changed(auto.copy(muted = true)))
		assertFalse(notice.changed(CallInfo.IDLE))
		assertTrue("the next call is announced again", notice.changed(auto))
	}

	@Test
	fun aHandAnsweredCallIsNotAnnounced() {
		val notice = AutoAnswerNotice()
		assertFalse(notice.changed(ringing.copy(incoming = false)))
	}

	@Test
	fun withoutTheMicrophoneTheNoticeAsksToOpenTheApp() {
		assertEquals(AutoAnswerNotification.ANSWERED, autoAnswerNotification(true))
		assertEquals(AutoAnswerNotification.NEEDS_MICROPHONE, autoAnswerNotification(false))
	}
}
