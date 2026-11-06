package dev.relay.app

/** What the app knows about one runtime permission. */
enum class Gate {
	/** Not looked at yet. */
	UNKNOWN,

	/** Not granted and never asked: ask. */
	NEEDS_REQUEST,

	/** The system dialog is up. */
	REQUESTING,

	/** Granted. */
	GRANTED,

	/** Refused, but the system will show the dialog again. */
	DENIED,

	/** Refused for good: only the system settings can change it. */
	BLOCKED,
}

/** What the UI says about a permission: nothing, ask, ask again, or go to the settings. */
enum class Access {
	/** Granted, or the system dialog is on screen. */
	READY,
	NOT_ASKED,
	DENIED,
	BLOCKED,
}

/** The UI's view of a gate. */
fun Gate.access(): Access = when (this) {
	Gate.UNKNOWN, Gate.REQUESTING, Gate.GRANTED -> Access.READY
	Gate.NEEDS_REQUEST -> Access.NOT_ASKED
	Gate.DENIED -> Access.DENIED
	Gate.BLOCKED -> Access.BLOCKED
}

/**
 * The state machine for one permission, with no Android types so it runs in
 * plain JVM tests. [onChange] is called on every real transition.
 */
class PermissionGate(private val onChange: (Gate) -> Unit = {}) {
	var state: Gate = Gate.UNKNOWN
		private set

	private fun move(next: Gate) {
		if (next != state) {
			state = next
			onChange(next)
		}
	}

	/** The current system answer, read on start and on every resume. */
	fun checked(granted: Boolean) {
		when {
			granted -> move(Gate.GRANTED)
			state == Gate.UNKNOWN || state == Gate.GRANTED -> move(Gate.NEEDS_REQUEST)
			else -> Unit // keep DENIED, BLOCKED or a dialog already showing
		}
	}

	/** Whether to call `requestPermissions` now. */
	fun shouldRequest(): Boolean = state == Gate.NEEDS_REQUEST || state == Gate.DENIED

	fun requestStarted() {
		if (shouldRequest()) move(Gate.REQUESTING)
	}

	/**
	 * The dialog closed. [canAskAgain] is `shouldShowRequestPermissionRationale`
	 * after the answer: false after a refusal means "don't ask again".
	 */
	fun result(granted: Boolean, canAskAgain: Boolean) {
		move(
			when {
				granted -> Gate.GRANTED
				canAskAgain -> Gate.DENIED
				else -> Gate.BLOCKED
			},
		)
	}

	val granted: Boolean get() = state == Gate.GRANTED
	val refused: Boolean get() = state == Gate.DENIED || state == Gate.BLOCKED
}
