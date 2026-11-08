package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CallIntentTrustTest {
	private val trust = CallIntentTrust("secret-token")

	@Test
	fun onlyTheRightTokenIsAccepted() {
		assertTrue(trust.accepts("secret-token"))
		assertFalse(trust.accepts(null))
		assertFalse(trust.accepts(""))
		assertFalse(trust.accepts("secret-token "))
		assertFalse(trust.accepts("SECRET-TOKEN"))
	}

	@Test
	fun aForeignIntentWithTheExtrasIsIgnored() {
		assertEquals(CallIntentAction.IGNORE, trust.action(fromCall = true, answer = true, candidate = null))
		assertEquals(CallIntentAction.IGNORE, trust.action(true, true, "guess"))
		assertEquals(CallIntentAction.IGNORE, trust.action(true, false, "guess"))
	}

	@Test
	fun theTokenWithoutTheCallFlagIsNotACallIntent() {
		assertEquals(CallIntentAction.IGNORE, trust.action(fromCall = false, answer = true, candidate = "secret-token"))
	}

	@Test
	fun ourNotificationsShowOrAnswer() {
		assertEquals(CallIntentAction.SHOW, trust.action(true, false, "secret-token"))
		assertEquals(CallIntentAction.SHOW_AND_ANSWER, trust.action(true, true, "secret-token"))
	}

	@Test
	fun everyProcessGetsItsOwnLongToken() {
		val a = CallIntentTrust.random()
		val b = CallIntentTrust.random()
		assertNotEquals(a.token, b.token)
		assertEquals(48, a.token.length)
	}
}
