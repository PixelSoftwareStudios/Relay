package dev.relay.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.Build
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * The phone-style notifications around a call: the incoming-call alert (heads-up
 * and full-screen call screen, Accept and Decline), the answered-automatically
 * notice (End call) and the "open Relay" notices. Decisions are made by the
 * pure policies in `AlertPolicy.kt`; this class only talks to Android.
 */
class IncomingCallNotifier(private val context: Context) {
	private val manager = NotificationManagerCompat.from(context)
	private val system = context.getSystemService(NotificationManager::class.java)
	private val power = context.getSystemService(Context.POWER_SERVICE) as PowerManager
	private var wake: PowerManager.WakeLock? = null

	fun canFullScreen(): Boolean = Build.VERSION.SDK_INT < 34 || system.canUseFullScreenIntent()

	fun show(info: CallInfo, plan: AlertPlan, fullScreen: Boolean, microphoneGranted: Boolean) {
		ensureIncomingChannel(plan)
		val who = info.peer.ifEmpty { context.getString(R.string.call_unknown_peer) }
		val open = openApp(REQUEST_OPEN, answer = false)
		val builder = NotificationCompat.Builder(context, plan.channel)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setContentTitle(context.getString(R.string.incoming_title))
			.setContentText(who)
			.setCategory(NotificationCompat.CATEGORY_CALL)
			.setPriority(NotificationCompat.PRIORITY_HIGH)
			.setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
			.setPublicVersion(publicVersion(plan.channel, context.getString(R.string.incoming_title)))
			.setOngoing(true)
			.setAutoCancel(false)
			.setContentIntent(open)
			.setOnlyAlertOnce(true)
			.addAction(0, context.getString(R.string.decline), broadcast(CallActionReceiver.ACTION_DECLINE))
			.addAction(
				0,
				context.getString(R.string.accept),
				// Without the microphone permission the service cannot start: open the app, which asks.
				if (microphoneGranted) broadcast(CallActionReceiver.ACTION_ACCEPT) else openApp(REQUEST_ACCEPT, answer = true),
			)
		if (fullScreen) builder.setFullScreenIntent(open, true)
		notify(INCOMING_ID, builder.build())
		acquireWake()
	}

	/** What the lock screen shows: no caller name. */
	private fun publicVersion(channel: String, title: String): Notification =
		NotificationCompat.Builder(context, channel)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setContentTitle(title)
			.setCategory(NotificationCompat.CATEGORY_CALL)
			.build()

	/** Withdraws call notifications left by an earlier process: their intents carry its token. */
	fun cancelStale() {
		manager.cancel(INCOMING_ID)
		manager.cancel(AUTO_ID)
	}

	fun cancelIncoming() {
		manager.cancel(INCOMING_ID)
		releaseWake()
	}

	/** "Answered automatically: name" with End call, or the request to open Relay for the microphone. */
	fun autoAnswered(info: CallInfo, kind: AutoAnswerNotification) {
		ensureChannel(CHANNEL_AUTO, R.string.channel_auto_answer, NotificationManager.IMPORTANCE_HIGH)
		val who = info.peer.ifEmpty { context.getString(R.string.call_unknown_peer) }
		val builder = NotificationCompat.Builder(context, CHANNEL_AUTO)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setCategory(NotificationCompat.CATEGORY_CALL)
			.setPriority(NotificationCompat.PRIORITY_HIGH)
			.setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
			.setPublicVersion(publicVersion(CHANNEL_AUTO, context.getString(R.string.auto_answered_public)))
			.setSilent(true)
		when (kind) {
			AutoAnswerNotification.ANSWERED -> builder
				.setContentTitle(context.getString(R.string.auto_answered, who))
				.setContentIntent(openApp(REQUEST_OPEN, answer = false))
				.addAction(0, context.getString(R.string.end_call), broadcast(CallActionReceiver.ACTION_END))
			AutoAnswerNotification.NEEDS_MICROPHONE -> builder
				.setContentTitle(context.getString(R.string.auto_answered, who))
				.setContentText(context.getString(R.string.open_for_microphone))
				.setContentIntent(openApp(REQUEST_OPEN, answer = false))
				.setFullScreenIntent(openApp(REQUEST_OPEN, answer = false), canFullScreen())
				.addAction(0, context.getString(R.string.end_call), broadcast(CallActionReceiver.ACTION_END))
		}
		notify(AUTO_ID, builder.build())
	}

	fun cancelAutoAnswered() = manager.cancel(AUTO_ID)

	/** A background service start was refused: ask the user to open Relay. */
	fun openRelayNeeded() {
		ensureChannel(CHANNEL_OPEN, R.string.channel_open, NotificationManager.IMPORTANCE_DEFAULT)
		val notification = NotificationCompat.Builder(context, CHANNEL_OPEN)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setContentTitle(context.getString(R.string.open_to_enable))
			.setContentIntent(openApp(REQUEST_OPEN, answer = false))
			.setAutoCancel(true)
			.build()
		notify(OPEN_ID, notification)
	}

	fun cancelOpenRelayNeeded() = manager.cancel(OPEN_ID)

	private fun notify(id: Int, notification: Notification) {
		try {
			// Without the notification permission this is dropped by the system; never throw out of a callback.
			manager.notify(id, notification)
		} catch (e: SecurityException) {
			Log.w(TAG, "notify refused", e)
		}
	}

	private fun ensureChannel(id: String, name: Int, importance: Int) {
		if (system.getNotificationChannel(id) != null) return
		system.createNotificationChannel(NotificationChannel(id, context.getString(name), importance).apply {
			setShowBadge(false)
			setSound(null, null)
			enableVibration(false)
		})
	}

	/**
	 * One channel per [AlertPlan.channel]. `sound` is the default ringtone on the
	 * ringtone usage so Android applies the ringer mode and Do Not Disturb.
	 */
	private fun ensureIncomingChannel(plan: AlertPlan) {
		if (system.getNotificationChannel(plan.channel) != null) return
		val channel = NotificationChannel(plan.channel, context.getString(nameFor(plan.channel)), NotificationManager.IMPORTANCE_HIGH)
		channel.setShowBadge(false)
		channel.lockscreenVisibility = Notification.VISIBILITY_PUBLIC
		if (plan.sound) {
			channel.setSound(
				RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE),
				AudioAttributes.Builder()
					.setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE)
					.setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
					.build(),
			)
		} else {
			channel.setSound(null, null)
		}
		channel.enableVibration(plan.vibrate)
		if (plan.vibrate) channel.vibrationPattern = VIBRATION
		system.createNotificationChannel(channel)
	}

	private fun nameFor(channel: String) = when {
		channel.startsWith("incoming_ring_vibrate") -> R.string.channel_incoming_ring
		channel.startsWith("incoming_vibrate") -> R.string.channel_incoming_vibrate
		channel.startsWith("incoming_silent") -> R.string.channel_incoming_silent
		else -> R.string.channel_incoming
	}

	/** Held only while a call rings with no screen of ours up, so the CPU stays awake to show it. */
	private fun acquireWake() {
		if (wake?.isHeld == true) return
		wake = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "relay:ring").apply {
			setReferenceCounted(false)
			acquire(RING_WAKE_MS)
		}
	}

	private fun releaseWake() {
		wake?.takeIf { it.isHeld }?.release()
		wake = null
	}

	private fun openApp(request: Int, answer: Boolean): PendingIntent = PendingIntent.getActivity(
		context,
		request,
		Intent(context, MainActivity::class.java)
			.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
			.putExtra(MainActivity.EXTRA_FROM_CALL, true)
			.putExtra(MainActivity.EXTRA_TOKEN, RelayRuntime.callTrust.token)
			.putExtra(MainActivity.EXTRA_ANSWER, answer),
		PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
	)

	private fun broadcast(action: String): PendingIntent = PendingIntent.getBroadcast(
		context,
		action.hashCode(),
		Intent(context, CallActionReceiver::class.java).setAction(action),
		PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
	)

	companion object {
		private const val TAG = "relay"
		const val INCOMING_ID = 3
		const val AUTO_ID = 4
		const val OPEN_ID = 5
		private const val CHANNEL_AUTO = "auto_answer"
		private const val CHANNEL_OPEN = "open_relay"
		private const val REQUEST_OPEN = 10
		private const val REQUEST_ACCEPT = 11
		private const val RING_WAKE_MS = 60_000L
		private val VIBRATION = longArrayOf(0, 600, 400, 600, 400, 600, 1000)
	}
}
