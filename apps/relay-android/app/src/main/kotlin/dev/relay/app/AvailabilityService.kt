package dev.relay.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat

/** The ongoing "Relay is available on this network" notification. */
object AvailabilityNotification {
	const val ID = 2
	const val ACTION_PAUSE = "dev.relay.app.PAUSE_AVAILABILITY"
	private const val CHANNEL = "availability"

	fun ensureChannel(context: Context) {
		val manager = context.getSystemService(NotificationManager::class.java)
		if (manager.getNotificationChannel(CHANNEL) == null) {
			manager.createNotificationChannel(
				NotificationChannel(CHANNEL, context.getString(R.string.channel_availability), NotificationManager.IMPORTANCE_LOW).apply {
					description = context.getString(R.string.channel_availability_description)
					setShowBadge(false)
				},
			)
		}
	}

	fun build(context: Context, state: Availability): Notification {
		ensureChannel(context)
		val text = if (state == Availability.ACTIVE) R.string.availability_active else R.string.availability_waiting
		return NotificationCompat.Builder(context, CHANNEL)
			.setSmallIcon(android.R.drawable.ic_btn_speak_now)
			.setContentTitle(context.getString(text))
			.setCategory(NotificationCompat.CATEGORY_SERVICE)
			.setPriority(NotificationCompat.PRIORITY_LOW)
			.setOngoing(true)
			.setOnlyAlertOnce(true)
			.setSilent(true)
			.setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
			.setContentIntent(
				PendingIntent.getActivity(
					context,
					0,
					Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
					PendingIntent.FLAG_IMMUTABLE,
				),
			)
			.addAction(
				0,
				context.getString(R.string.availability_pause),
				PendingIntent.getService(
					context,
					ACTION_PAUSE.hashCode(),
					Intent(context, AvailabilityService::class.java).setAction(ACTION_PAUSE),
					PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
				),
			)
			.build()
	}
}

/**
 * Foreground service of type `connectedDevice`, alive while `background.available`
 * is on (see [AvailabilityPolicy]). It holds no engine of its own: it keeps the
 * process alive and holds the multicast and Wi-Fi locks. The engine holds its own
 * presence while `background.available` is on, so it advertises, listens and rings with
 * no window open and no watch.
 *
 * Type choice: `connectedDevice` is the one that fits a LAN-listening peer
 * (it needs one of the network-state permissions, and `CHANGE_WIFI_MULTICAST_STATE`
 * is declared), has no time limit (unlike `dataSync` on Android 15) and may be
 * started from `BOOT_COMPLETED` (unlike `microphone`, `camera`, `mediaPlayback`
 * and `dataSync` on Android 15). The microphone type stays with [CallService].
 */
class AvailabilityService : Service() {
	private val listener: (Availability) -> Unit = { apply(it) }

	override fun onCreate() {
		super.onCreate()
		RelayRuntime.addAvailabilityListener(listener)
	}

	override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
		val state = RelayRuntime.availability.takeIf { it != Availability.OFF } ?: Availability.WAITING
		// Must be called promptly after startForegroundService.
		val type = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE else 0
		ServiceCompat.startForeground(this, AvailabilityNotification.ID, AvailabilityNotification.build(this, state), type)
		if (intent?.action == AvailabilityNotification.ACTION_PAUSE) {
			RelayRuntime.pauseAvailability()
		} else {
			apply(state)
		}
		Log.i(TAG, "availability service running ($state, action=${intent?.action})")
		return START_STICKY
	}

	private fun apply(state: Availability) {
		if (state == Availability.OFF) return
		getSystemService(NotificationManager::class.java)
			.notify(AvailabilityNotification.ID, AvailabilityNotification.build(this, state))
		if (state == Availability.ACTIVE) RelayRuntime.locks.hold(OWNER) else RelayRuntime.locks.drop(OWNER)
	}

	override fun onDestroy() {
		RelayRuntime.removeAvailabilityListener(listener)
		RelayRuntime.locks.drop(OWNER)
		RelayRuntime.availabilityStopped()
		super.onDestroy()
	}

	override fun onBind(intent: Intent?): IBinder? = null

	private companion object {
		const val TAG = "relay"
		const val OWNER = "availability"
	}
}
