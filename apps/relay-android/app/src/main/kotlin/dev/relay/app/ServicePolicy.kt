package dev.relay.app

/** What the call looks like to the foreground service. */
data class CallInfo(
	val active: Boolean,
	val incoming: Boolean,
	val muted: Boolean,
	val peer: String,
) {
	companion object {
		val IDLE = CallInfo(active = false, incoming = false, muted = false, peer = "")
	}
}

sealed interface ServiceAction {
	/** Start the foreground service (microphone type) with this info. */
	data class Start(val info: CallInfo) : ServiceAction

	/** Refresh the running notification. */
	data class Update(val info: CallInfo) : ServiceAction

	data object Stop : ServiceAction

	/** A call needs the microphone but the permission is missing: ask first. */
	data object NeedMicrophone : ServiceAction

	data object None : ServiceAction
}

/**
 * When the foreground service runs: exactly while a call exists and the
 * microphone permission is held. Android 14 throws if a microphone service
 * starts without the permission, so the policy never asks for that.
 */
class ServicePolicy {
	private var running = false
	private var shown: CallInfo? = null

	fun decide(info: CallInfo, microphoneGranted: Boolean): ServiceAction {
		if (!info.active) {
			val was = running
			running = false
			shown = null
			return if (was) ServiceAction.Stop else ServiceAction.None
		}
		if (!running) {
			if (!microphoneGranted) return ServiceAction.NeedMicrophone
			running = true
			shown = info
			return ServiceAction.Start(info)
		}
		if (shown == info) return ServiceAction.None
		shown = info
		return ServiceAction.Update(info)
	}

	/** The system stopped the service (task removed, killed). */
	fun stoppedBySystem() {
		running = false
		shown = null
	}
}
