package dev.relay.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Restarts availability after boot or an app update. The service type is
 * `connectedDevice`, which Android 15 still lets a boot receiver start (the
 * `microphone`, `camera`, `dataSync`, `mediaPlayback` types are refused). If the
 * system refuses anyway the receiver leaves a notification asking to open Relay.
 * The setting is read from [AvailabilityPrefs]: nothing else is running yet.
 */
class BootReceiver : BroadcastReceiver() {
	override fun onReceive(context: Context, intent: Intent) {
		if (intent.action != Intent.ACTION_BOOT_COMPLETED && intent.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
		Log.i("relay", "boot receiver: ${intent.action}")
		// RelayApplication.onCreate already ran; this evaluates with the stored setting.
		RelayRuntime.reevaluateAvailability(fromBoot = true)
	}
}
