package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DeviceNameTest {
	@Test
	fun theNameTheUserSeesWins() {
		assertEquals("Galaxy S25 Ultra", DeviceName.choose("Galaxy S25 Ultra", "SM-S938B"))
	}

	@Test
	fun aBlankOrMissingNameFallsBackToTheModel() {
		assertEquals("Pixel 9", DeviceName.choose(null, "Pixel 9"))
		assertEquals("Pixel 9", DeviceName.choose("", "Pixel 9"))
		assertEquals("Pixel 9", DeviceName.choose("   ", "Pixel 9"))
	}

	@Test
	fun theNameIsTrimmed() {
		assertEquals("Ana's phone", DeviceName.choose("  Ana's phone \n", null))
	}

	@Test
	fun withNothingTheEngineKeepsItsOwnDefault() {
		assertNull(DeviceName.choose(null, null))
		assertNull(DeviceName.choose(" ", ""))
	}
}
