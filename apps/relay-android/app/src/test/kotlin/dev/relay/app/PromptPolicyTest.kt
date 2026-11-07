package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PromptPolicyTest {
	private var asked = false
	private val prompt = ExemptionPrompt({ asked }, { asked = true })

	@Test
	fun nothingToAskWhenExemptOrUnsupported() {
		assertEquals(ExemptionStep.NOTHING, prompt.next(true))
		assertEquals(ExemptionStep.NOTHING, prompt.next(null))
		assertFalse(asked)
	}

	@Test
	fun theFirstRequestIsTheDialogAndLaterOnesTheList() {
		assertEquals(ExemptionStep.DIALOG, prompt.next(false))
		assertTrue(asked)
		prompt.resumed()
		assertEquals(ExemptionStep.SETTINGS_LIST, prompt.next(false))
	}

	@Test
	fun aRepeatedRequestWhileOneIsOnScreenIsDropped() {
		assertEquals(ExemptionStep.DIALOG, prompt.next(false))
		assertEquals(ExemptionStep.NOTHING, prompt.next(false))
		assertEquals(ExemptionStep.NOTHING, prompt.next(false))
		prompt.resumed()
		assertEquals(ExemptionStep.SETTINGS_LIST, prompt.next(false))
	}

	@Test
	fun grantingTheExemptionEndsTheAsking() {
		prompt.next(false)
		prompt.resumed()
		assertEquals(ExemptionStep.NOTHING, prompt.next(true))
	}

	@Test
	fun notificationsOnNewAndroidAskThenSendToSettings() {
		assertEquals(NotificationStep.NOTHING, notificationStep(34, Gate.GRANTED, enabled = true))
		assertEquals(NotificationStep.DIALOG, notificationStep(34, Gate.NEEDS_REQUEST, enabled = false))
		assertEquals(NotificationStep.DIALOG, notificationStep(34, Gate.DENIED, enabled = false))
		assertEquals(NotificationStep.NOTHING, notificationStep(34, Gate.REQUESTING, enabled = false))
		assertEquals(NotificationStep.SETTINGS, notificationStep(34, Gate.BLOCKED, enabled = false))
	}

	@Test
	fun notificationsSwitchedOffOnOldAndroidOnlyHaveTheSettings() {
		assertEquals(NotificationStep.SETTINGS, notificationStep(30, Gate.GRANTED, enabled = false))
		assertEquals(NotificationStep.NOTHING, notificationStep(30, Gate.GRANTED, enabled = true))
	}

	@Test
	fun aGateNotReadYetAsksNothing() {
		assertEquals(NotificationStep.NOTHING, notificationStep(34, Gate.UNKNOWN, enabled = false))
	}
}
