package dev.relay.app

/** Runs a task later; the handle cancels it. The app uses the main looper, tests a fake clock. */
fun interface Scheduler {
	fun after(delayMs: Long, task: () -> Unit): () -> Unit
}

/**
 * Holds [target] (the phone's call audio state) while any owner wants it and
 * for [lingerMs] after the last one lets go. The linger matters because the
 * engine closes a stream and opens the next one on a route change, and a ring
 * ends a moment before the call's streams open: dropping and re-entering the
 * mode in between would disconnect the new streams again.
 *
 * Owners: "streams" (the engine has an audio stream open, told by Rust before
 * the first one opens) and "service" (a call exists).
 */
class LingeringHold(
	private val target: Releasable,
	private val scheduler: Scheduler,
	private val lingerMs: Long = LINGER_MS,
) {
	private val owners = mutableSetOf<String>()
	private var active = false
	private var cancelRelease: (() -> Unit)? = null

	@Synchronized
	fun hold(owner: String) {
		owners += owner
		cancelRelease?.invoke()
		cancelRelease = null
		if (!active) {
			active = true
			target.acquire()
		}
	}

	@Synchronized
	fun drop(owner: String) {
		if (!owners.remove(owner) || owners.isNotEmpty() || !active) return
		cancelRelease?.invoke()
		cancelRelease = scheduler.after(lingerMs) { lapse() }
	}

	@Synchronized
	private fun lapse() {
		cancelRelease = null
		if (owners.isEmpty() && active) {
			active = false
			target.release()
		}
	}

	@get:Synchronized
	val held: Boolean get() = active

	companion object {
		const val LINGER_MS = 1500L
	}
}
