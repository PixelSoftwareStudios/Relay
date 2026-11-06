package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Test

class BackRouterTest {
	private var backgrounded = 0

	private fun router(ui: () -> Boolean) = BackRouter(ui) { backgrounded++ }

	@Test
	fun theUiUsingBackKeepsTheAppOpen() {
		router { true }.back()
		assertEquals(0, backgrounded)
	}

	@Test
	fun nothingLeftToCloseMovesTheTaskToTheBackground() {
		router { false }.back()
		assertEquals(1, backgrounded)
	}

	@Test
	fun eachPressIsDecidedOnItsOwn() {
		val answers = ArrayDeque(listOf(true, true, false, false))
		val router = router { answers.removeFirst() }
		repeat(4) { router.back() }
		assertEquals(2, backgrounded)
	}

	@Test
	fun anEngineThatIsNotThereYetStillLeaves() {
		router { error("engine not started") }.back()
		assertEquals(1, backgrounded)
	}
}
