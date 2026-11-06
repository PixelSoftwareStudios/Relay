package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TalkKeysTest {
    private val sent = mutableListOf<Boolean>()
    private var cancels = 0
    private var pushToTalkCall = true

    private val keys = TalkKeys(
        talk = { pressed ->
            if (pressed && !pushToTalkCall) {
                false
            } else {
                sent.add(pressed)
                true
            }
        },
        cancel = { cancels++ },
    )

    private val up = 24
    private val down = 25

    @Test
    fun aKeyHeldDuringAPushToTalkCallPressesOnceAndReleasesOnKeyUp() {
        assertTrue(keys.press(up))
        assertTrue(keys.press(up)) // auto-repeat is swallowed, not resent
        assertTrue(keys.press(up))
        assertEquals(listOf(true), sent)
        assertTrue(keys.release(up))
        assertEquals(listOf(true, false), sent)
        assertFalse(keys.holding)
    }

    @Test
    fun withoutAPushToTalkCallTheKeyChangesTheVolume() {
        pushToTalkCall = false
        assertFalse(keys.press(up))
        assertFalse(keys.release(up))
        assertEquals(emptyList<Boolean>(), sent)
    }

    @Test
    fun theLastOfTwoHeldKeysReleases() {
        keys.press(up)
        keys.press(down)
        keys.release(up)
        assertEquals(listOf(true, true), sent)
        assertTrue(keys.holding)
        keys.release(down)
        assertEquals(listOf(true, true, false), sent)
    }

    @Test
    fun aKeyUpWithoutATakenPressIsLeftToAndroid() {
        assertFalse(keys.release(up))
        assertEquals(emptyList<Boolean>(), sent)
    }

    @Test
    fun cancelAlwaysReachesTheEngineAndForgetsTheKeys() {
        keys.press(up)
        keys.cancel()
        assertEquals(1, cancels)
        assertFalse(keys.holding)
        // The key-up that never came, or comes late, must not release twice.
        assertFalse(keys.release(up))
        assertEquals(listOf(true), sent)
        keys.cancel() // nothing held, still harmless and still forwarded
        assertEquals(2, cancels)
    }
}
