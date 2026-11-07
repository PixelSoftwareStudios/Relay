package dev.relay.app

/** How to ask for the battery-optimization exemption. */
enum class ExemptionStep {
	/** Already exempt, or the device has no such setting. */
	NOTHING,

	/** The system dialog "Allow Relay to run in the background?". */
	DIALOG,

	/** The battery-optimization list, because the dialog was already shown and the answer was no. */
	SETTINGS_LIST,
}

/**
 * Asks for the exemption at most once with the dialog, then only sends the user
 * to the system list when the UI asks again by an explicit tap. Nothing is
 * ever opened without a request from the UI, and a request made while the
 * previous one is still on screen is dropped, so the app cannot loop.
 */
class ExemptionPrompt(
	private val asked: () -> Boolean,
	private val markAsked: () -> Unit,
) {
	private var inFlight = false

	fun next(exempt: Boolean?): ExemptionStep {
		if (exempt == null || exempt) return ExemptionStep.NOTHING
		if (inFlight) return ExemptionStep.NOTHING
		inFlight = true
		return if (asked()) {
			ExemptionStep.SETTINGS_LIST
		} else {
			markAsked()
			ExemptionStep.DIALOG
		}
	}

	/** The app came back to the front: the previous request is over. */
	fun resumed() {
		inFlight = false
	}
}

/** How to ask for the notification permission. */
enum class NotificationStep {
	NOTHING,

	/** The system permission dialog. */
	DIALOG,

	/** Refused for good: the app's notification settings. */
	SETTINGS,
}

/**
 * [enabled] is whether Android shows this app's notifications now. On Android 13
 * and later a first or repeatable refusal gets the system dialog; a refusal for
 * good, or any switch-off on older Android, can only be undone in the settings.
 */
fun notificationStep(sdk: Int, gate: Gate, enabled: Boolean): NotificationStep = when {
	enabled -> NotificationStep.NOTHING
	gate == Gate.REQUESTING || gate == Gate.UNKNOWN -> NotificationStep.NOTHING
	sdk >= 33 && (gate == Gate.NEEDS_REQUEST || gate == Gate.DENIED) -> NotificationStep.DIALOG
	else -> NotificationStep.SETTINGS
}
