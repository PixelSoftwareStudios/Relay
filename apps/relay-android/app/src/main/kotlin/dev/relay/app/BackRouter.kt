package dev.relay.app

/**
 * What the system Back button or gesture does. The shared UI gets the first
 * chance (close a chat or dialog, go up one level, return to Calls); when it
 * has nothing left to close, the app goes to the background like a home
 * screen would. It never finishes the activity: the engine, and a call in
 * progress, keep running.
 */
class BackRouter(
	/** Asks the UI to go back; true when it used the action. */
	private val ui: () -> Boolean,
	private val toBackground: () -> Unit,
) {
	fun back() {
		val used = try {
			ui()
		} catch (e: RuntimeException) {
			false
		}
		if (!used) toBackground()
	}
}
