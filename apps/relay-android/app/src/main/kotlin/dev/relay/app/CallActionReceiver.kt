package dev.relay.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import androidx.core.content.ContextCompat

/**
 * The Accept, Decline and End call buttons of the call notifications. A tap on a
 * notification action lets the app start a microphone foreground service from
 * the background, so Accept goes through [CallService] (which answers); Decline
 * and End call only talk to the engine.
 */
class CallActionReceiver : BroadcastReceiver() {
	override fun onReceive(context: Context, intent: Intent) {
		Log.i("relay", "call action ${intent.action}")
		when (intent.action) {
			ACTION_ACCEPT -> if (RelayRuntime.microphone.granted) {
				ContextCompat.startForegroundService(
					context,
					Intent(context, CallService::class.java).setAction(CallNotification.ACTION_ANSWER),
				)
			} else {
				// A microphone service cannot start without the permission: let the app ask.
				RelayRuntime.notifier.autoAnswered(RelayRuntime.call, AutoAnswerNotification.NEEDS_MICROPHONE)
			}
			ACTION_DECLINE, ACTION_END -> RelayRuntime.whenReady { it.hangUp() }
		}
	}

	companion object {
		const val ACTION_ACCEPT = "dev.relay.app.NOTIFY_ACCEPT"
		const val ACTION_DECLINE = "dev.relay.app.NOTIFY_DECLINE"
		const val ACTION_END = "dev.relay.app.NOTIFY_END"
	}
}
