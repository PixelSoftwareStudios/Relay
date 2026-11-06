package dev.relay.app

/** How a QR scan request ended, before it crosses into Rust. */
sealed interface ScanResult {
	/** The raw text of the code. Rust checks it like a pasted connection string. */
	data class Text(val text: String) : ScanResult
	data object Cancelled : ScanResult
	data object CameraDenied : ScanResult
	data object CameraBlocked : ScanResult
	data object Unavailable : ScanResult
	data class Failed(val reason: String) : ScanResult
}

/** What to do next for a scan request. */
sealed interface ScanStep {
	/** Open the scanner screen. */
	data object Launch : ScanStep

	/** Show the system camera dialog first. */
	data object Ask : ScanStep

	/** Nothing to show: the request is over. */
	data class Finish(val result: ScanResult) : ScanStep
}

/**
 * One QR scan at a time, with no Android types so it runs in JVM tests. The
 * request lives here, not in the activity, so an activity recreated while the
 * scanner or the permission dialog is up still completes it.
 *
 * The camera permission is asked on the first tap, never at startup, through
 * its own [PermissionGate] (separate from the microphone's).
 */
class ScanFlow(private val camera: PermissionGate, private val complete: (ULong, ScanResult) -> Unit) {
	private var pending: ULong? = null

	val active: Boolean get() = pending != null

	/** The user tapped Scan. [granted] is the system's current answer. */
	fun begin(id: ULong, hasCamera: Boolean, granted: Boolean): ScanStep {
		if (!hasCamera) return ScanStep.Finish(ScanResult.Unavailable).also { complete(id, it.result) }
		if (pending != null) return ScanStep.Finish(ScanResult.Failed("A scan is already open.")).also { complete(id, it.result) }
		camera.checked(granted)
		val step = when {
			camera.granted -> ScanStep.Launch
			camera.shouldRequest() -> ScanStep.Ask.also { camera.requestStarted() }
			camera.state == Gate.BLOCKED -> ScanStep.Finish(ScanResult.CameraBlocked)
			else -> ScanStep.Finish(ScanResult.Failed("The camera permission dialog is already open."))
		}
		if (step is ScanStep.Finish) complete(id, step.result) else pending = id
		return step
	}

	/** The system camera dialog closed. */
	fun permissionAnswered(granted: Boolean, canAskAgain: Boolean): ScanStep {
		camera.result(granted, canAskAgain)
		return when {
			granted -> ScanStep.Launch
			camera.state == Gate.BLOCKED -> finish(ScanResult.CameraBlocked)
			else -> finish(ScanResult.CameraDenied)
		}
	}

	/** The scanner screen closed with [text] (null when cancelled or nothing was read). */
	fun scanned(text: String?, error: String? = null): ScanStep = finish(
		when {
			error != null -> ScanResult.Failed(error)
			text.isNullOrBlank() -> ScanResult.Cancelled
			else -> ScanResult.Text(text)
		},
	)

	private fun finish(result: ScanResult): ScanStep {
		pending?.let { complete(it, result) }
		pending = null
		return ScanStep.Finish(result)
	}
}
