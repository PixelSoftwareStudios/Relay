package dev.relay.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.core.app.NotificationCompat

/** The ongoing-call notification and its actions. */
object CallNotification {
	const val ID = 1
	const val ACTION_MUTE = "dev.relay.app.MUTE"
	const val ACTION_UNMUTE = "dev.relay.app.UNMUTE"
	const val ACTION_HANGUP = "dev.relay.app.HANGUP"
	const val ACTION_ANSWER = "dev.relay.app.ANSWER"
	private const val CHANNEL = "call"

	fun ensureChannel(context: Context) {
		val manager = context.getSystemService(NotificationManager::class.java)
		if (manager.getNotificationChannel(CHANNEL) == null) {
			manager.createNotificationChannel(
				NotificationChannel(CHANNEL, "Calls", NotificationManager.IMPORTANCE_LOW).apply {
					description = "Shown while a call is up so Android keeps it running."
					setShowBadge(false)
				},
			)
		}
	}

	fun build(context: Context, info: CallInfo): Notification {
		ensureChannel(context)
		val who = info.peer.ifEmpty { "Relay call" }
		val builder = NotificationCompat.Builder(context, CHANNEL)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setContentTitle(if (info.incoming) "Incoming call" else "In a call")
			.setContentText(who)
			.setCategory(NotificationCompat.CATEGORY_CALL)
			.setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
			.setPublicVersion(
				NotificationCompat.Builder(context, CHANNEL)
					.setSmallIcon(android.R.drawable.ic_btn_speak_now)
					.setContentTitle(context.getString(R.string.call_unknown_peer))
					.build(),
			)
			.setOngoing(true)
			.setOnlyAlertOnce(true)
			.setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
			.setContentIntent(
				PendingIntent.getActivity(
					context,
					0,
					Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
					PendingIntent.FLAG_IMMUTABLE,
				),
			)
		if (info.incoming) {
			builder.addAction(0, "Answer", action(context, ACTION_ANSWER))
		} else {
			builder.addAction(
				0,
				if (info.muted) "Unmute" else "Mute",
				action(context, if (info.muted) ACTION_UNMUTE else ACTION_MUTE),
			)
		}
		builder.addAction(0, "Hang up", action(context, ACTION_HANGUP))
		return builder.build()
	}

	private fun action(context: Context, name: String): PendingIntent = PendingIntent.getService(
		context,
		name.hashCode(),
		Intent(context, CallService::class.java).setAction(name),
		PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
	)
}
