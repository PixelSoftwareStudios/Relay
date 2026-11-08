package dev.relay.app

import android.content.ClipData
import android.content.ActivityNotFoundException
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.util.Log
import androidx.core.app.NotificationManagerCompat
import uniffi.relay_android.CallSummary
import uniffi.relay_android.HostBridge
import uniffi.relay_android.HostException
import uniffi.relay_android.PickKind

/**
 * What the Rust side may ask of Android. Every method is called from a Rust
 * thread and must not throw out of the callback (the Rust side cannot handle
 * a foreign exception except through the declared error type).
 */
class AndroidBridge(private val context: Context) : HostBridge {
	private val sealer = Sealer { KeystoreKey.get() }

	override fun seal(context: String, plain: ByteArray): ByteArray = try {
		sealer.seal(context, plain)
	} catch (e: Exception) {
		throw HostException.Failed("keystore seal failed: ${e.javaClass.simpleName}")
	}

	override fun open(context: String, sealed: ByteArray): ByteArray = try {
		sealer.open(context, sealed)
	} catch (e: Exception) {
		throw HostException.Failed("keystore open failed: ${e.javaClass.simpleName}")
	}

	override fun copyText(text: String) {
		runCatching {
			val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
			clipboard.setPrimaryClip(ClipData.newPlainText("Relay", text))
		}.onFailure { Log.w(TAG, "clipboard failed", it) }
	}

	override fun openUrl(url: String) {
		runCatching {
			val uri = Uri.parse(url)
			if (uri.scheme == "https" || uri.scheme == "http") {
				context.startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
			}
		}.onFailure { Log.w(TAG, "open url failed", it) }
	}

	override fun pick(requestId: ULong, kind: PickKind) {
		val activity = MainActivity.current
		if (activity == null) {
			RelayRuntime.whenReady {
				it.completePick(requestId, uniffi.relay_android.PickOutcome.Failed("Relay is not open."))
			}
		} else {
			activity.runOnUiThread { activity.pickDocument(requestId, kind) }
		}
	}

	override fun scanQr(requestId: ULong) {
		val activity = MainActivity.current
		if (activity == null) {
			RelayRuntime.whenReady { it.completeScan(requestId, uniffi.relay_android.ScanOutcome.Failed("Relay is not open.")) }
		} else {
			activity.runOnUiThread { activity.scanQr(requestId) }
		}
	}

	override fun callChanged(summary: CallSummary) {
		RelayRuntime.onCallChanged(CallInfo(summary.active, summary.incoming, summary.muted, summary.peer, summary.autoAnswered))
	}

	override fun backgroundChanged(available: Boolean, alert: String) {
		RelayRuntime.onBackgroundChanged(available, alert)
	}

	override fun questionAnswered(id: String): Boolean = RelayRuntime.prefs.questionAnswered(id)

	override fun setQuestionAnswered(id: String) = RelayRuntime.prefs.setQuestionAnswered(id)

	override fun requestBatteryExemption() {
		runCatching {
			val step = RelayRuntime.exemption.next(RelayRuntime.batteryExempt())
			val package_ = Uri.fromParts("package", context.packageName, null)
			val intent = when (step) {
				ExemptionStep.NOTHING -> return
				ExemptionStep.DIALOG -> Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, package_)
				ExemptionStep.SETTINGS_LIST -> Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
			}
			try {
				context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
			} catch (e: ActivityNotFoundException) {
				// Some builds lack the dialog: the list is always there.
				context.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
			}
		}.onFailure { Log.w(TAG, "battery exemption request failed", it) }
	}

	override fun requestNotifications() {
		val step = notificationStep(Build.VERSION.SDK_INT, RelayRuntime.notifications.state, NotificationManagerCompat.from(context).areNotificationsEnabled())
		when (step) {
			NotificationStep.NOTHING -> Unit
			NotificationStep.DIALOG -> MainActivity.current?.let { it.runOnUiThread { it.askForNotifications() } }
			NotificationStep.SETTINGS -> runCatching {
				val intent = Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(Settings.EXTRA_APP_PACKAGE, context.packageName)
				context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
			}.onFailure { Log.w(TAG, "notification settings failed", it) }
		}
	}

	override fun audioStreamsWanted(wanted: Boolean) {
		// Returns once the call audio state is in place: the engine opens its stream next.
		if (wanted) RelayRuntime.callAudio.hold("streams") else RelayRuntime.callAudio.drop("streams")
	}

	override fun requestMicrophone() {
		MainActivity.current?.let { activity -> activity.runOnUiThread { activity.askForMicrophone() } }
	}

	override fun openAppSettings() {
		runCatching {
			val intent = Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
			context.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
		}.onFailure { Log.w(TAG, "open app settings failed", it) }
	}

	override fun quitApp() {
		MainActivity.current?.runOnUiThread { MainActivity.current?.finishAndRemoveTask() }
	}

	private companion object {
		const val TAG = "relay"
	}
}
