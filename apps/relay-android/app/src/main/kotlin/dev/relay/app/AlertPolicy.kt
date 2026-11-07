package dev.relay.app

/** `calls.alert`. */
enum class CallAlert {
	SYSTEM,
	RING_AND_VIBRATE,
	VIBRATE,
	SILENT;

	companion object {
		/** The engine's wire name; anything unknown is the system profile. */
		fun parse(name: String): CallAlert = when (name) {
			"ring_and_vibrate" -> RING_AND_VIBRATE
			"vibrate" -> VIBRATE
			"silent" -> SILENT
			else -> SYSTEM
		}
	}
}

/**
 * How the incoming-call notification alerts.
 *
 * Who makes the sound is decided per alert:
 * - `system`: the shell. The notification channel plays the default ringtone and
 *   vibrates, and Android applies the ringer mode and Do Not Disturb. The engine
 *   is told to stay quiet (`SetClientQuiet`), so the phone's silent mode is
 *   respected and nothing rings twice.
 * - `ring_and_vibrate`: the engine rings (forced, whatever the ringer mode); the
 *   notification adds the heads-up, the call screen and vibration, no sound.
 * - `vibrate`: the engine stays silent by its own rule; the notification vibrates.
 * - `silent`: nothing sounds or vibrates; the notification is visual only.
 *
 * With a Relay screen open the engine rings through the UI's own (never quiet)
 * client and no notification is posted.
 */
data class AlertPlan(
	/** Notification channel id; one per distinct behaviour because a channel's sound and vibration are fixed once created. */
	val channel: String,
	/** The channel plays the default ringtone (Android applies ringer mode and Do Not Disturb). */
	val sound: Boolean,
	/** The channel vibrates. */
	val vibrate: Boolean,
)

object AlertPolicy {
	/** Whether the shell alerts the call itself, so the engine must stay quiet (`SetClientQuiet(true)`). */
	fun shellAlerts(alert: CallAlert): Boolean = alert == CallAlert.SYSTEM

	/** Whether the engine's own ring is audible for this alert in the background. */
	fun engineRings(alert: CallAlert): Boolean = alert == CallAlert.RING_AND_VIBRATE

	fun plan(alert: CallAlert): AlertPlan {
		val sound = shellAlerts(alert)
		val vibrate = alert != CallAlert.SILENT
		val channel = when (alert) {
			CallAlert.SYSTEM -> "incoming_system"
			// The engine makes the sound: a channel of its own with none, as channels cannot change.
			CallAlert.RING_AND_VIBRATE -> "incoming_ring_vibrate_engine"
			CallAlert.VIBRATE -> "incoming_vibrate"
			CallAlert.SILENT -> "incoming_silent"
		}
		return AlertPlan(channel, sound, vibrate)
	}
}

/** What to do about the incoming-call notification for the current state. */
sealed interface IncomingAction {
	/** Show it; [fullScreen] puts the call screen over the lock screen. */
	data class Show(val plan: AlertPlan, val fullScreen: Boolean) : IncomingAction

	data object Cancel : IncomingAction

	data object None : IncomingAction
}

/**
 * Posts the incoming-call notification while a call rings and no Relay screen
 * is visible (a visible app shows the call itself and the engine rings).
 */
class IncomingPolicy {
	private var shown = false

	fun decide(
		info: CallInfo,
		visible: Boolean,
		alert: CallAlert,
		canFullScreen: Boolean,
	): IncomingAction {
		val wanted = info.active && info.incoming && !visible
		if (!wanted) {
			val was = shown
			shown = false
			return if (was) IncomingAction.Cancel else IncomingAction.None
		}
		if (shown) return IncomingAction.None
		shown = true
		return IncomingAction.Show(AlertPolicy.plan(alert), canFullScreen)
	}

	/** The notification was dismissed or answered elsewhere. */
	fun cleared() {
		shown = false
	}
}

/** Notices a call becoming auto-answered, once per call. */
class AutoAnswerNotice {
	private var announced = false

	/** True exactly when the notification should be posted now. */
	fun changed(info: CallInfo): Boolean {
		if (!info.active) {
			announced = false
			return false
		}
		if (!info.autoAnswered) return false
		if (announced) return false
		announced = true
		return true
	}
}

/** What the answered-automatically notification asks of the user. */
enum class AutoAnswerNotification {
	/** Heads-up "Answered automatically: name" with End call. */
	ANSWERED,

	/** The microphone permission is missing: ask to open the app. */
	NEEDS_MICROPHONE,
}

fun autoAnswerNotification(microphoneGranted: Boolean): AutoAnswerNotification =
	if (microphoneGranted) AutoAnswerNotification.ANSWERED else AutoAnswerNotification.NEEDS_MICROPHONE

/** Whether a partial wake lock is wanted: only while a call rings with no screen of ours up. */
fun wakeForRinging(info: CallInfo, visible: Boolean): Boolean = info.active && info.incoming && !visible
