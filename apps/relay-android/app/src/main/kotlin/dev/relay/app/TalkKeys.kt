package dev.relay.app

/**
 * Volume keys as hold-to-talk. The engine decides whether a key press is a
 * talk key (a push-to-talk call is up); this class only remembers which keys
 * it took, so the matching key-up always reaches the engine and a vanished
 * key (pause, focus or surface loss) is always released.
 *
 * [talk] forwards a press or release to the engine and returns whether the
 * engine used the key. [cancel] tells the engine to release whatever is held.
 */
class TalkKeys(
    private val talk: (pressed: Boolean) -> Boolean,
    private val cancel: () -> Unit,
) {
    private val down = mutableSetOf<Int>()

    /** A key went down; true when the key must not change the volume. */
    @Synchronized
    fun press(keyCode: Int): Boolean {
        if (keyCode in down) return true // auto-repeat of a key already taken
        if (!talk(true)) return false
        down.add(keyCode)
        return true
    }

    /** A key went up; true when it was a talk key. The last key up releases. */
    @Synchronized
    fun release(keyCode: Int): Boolean {
        if (!down.remove(keyCode)) return false
        if (down.isEmpty()) talk(false)
        return true
    }

    /** The activity can no longer see key-up events. Always tells the engine. */
    @Synchronized
    fun cancel() {
        down.clear()
        cancel.invoke()
    }

    @get:Synchronized
    val holding: Boolean get() = down.isNotEmpty()
}
