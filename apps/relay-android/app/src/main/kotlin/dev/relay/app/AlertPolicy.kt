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
 * Who makes the sound: the engine. It rings through its own audio stream
 * whenever a client is attached (the app is open or the availability service
 * runs) and the alert is `system` or `ring_and_vibrate`; that stream cannot be
 * muted per client from the shell. So the notification never rings when the
 * engine does, which would be two rings at once. The notification carries the
 * heads-up, the full-screen call screen and the vibration. When the engine is
 * not going to ring ([engineRings] false) the notification makes the sound
 * itself, which keeps the `system` profile (ringer mode, Do Not Disturb)
 * because the channel sound is left to Android.
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
	/** Whether the engine's own ring is audible for this alert (mirrors the engine's `ring_audible`). */
	fun engineRings(alert: CallAlert): Boolean =
		alert == CallAlert.SYSTEM || alert == CallAlert.RING_AND_VIBRATE

	fun plan(alert: CallAlert, engineRings: Boolean): AlertPlan {
		val sound = !engineRings && (alert == CallAlert.SYSTEM || alert == CallAlert.RING_AND_VIBRATE)
		val vibrate = alert != CallAlert.SILENT
		val base = when (alert) {
			CallAlert.SYSTEM -> "incoming_system"
			CallAlert.RING_AND_VIBRATE -> "incoming_ring_vibrate"
			CallAlert.VIBRATE -> "incoming_vibrate"
			CallAlert.SILENT -> "incoming_silent"
		}
		// A channel's sound is immutable, so a quiet variant has its own id.
		val channel = if (engineRings && (alert == CallAlert.SYSTEM || alert == CallAlert.RING_AND_VIBRATE)) "${base}_engine" else base
		return AlertPlan(channel, sound, vibrate)
	}

	fun plan(alert: CallAlert): AlertPlan = plan(alert, engineRings(alert))
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
