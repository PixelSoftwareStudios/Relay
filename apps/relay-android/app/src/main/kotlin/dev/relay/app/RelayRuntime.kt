package dev.relay.app

import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import android.os.PowerManager
import android.os.Vibrator
import androidx.core.app.NotificationManagerCompat
import android.provider.Settings
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.content.ContextCompat
import uniffi.relay_android.EngineConfig
import uniffi.relay_android.MicrophoneState
import uniffi.relay_android.RelayEngine
import uniffi.relay_android.ScanOutcome
import java.util.concurrent.Executors

/**
 * Process-wide state: the in-process engine and everything that must outlive
 * an activity (call state, locks). The engine starts when the process starts,
 * on a worker thread; anything that needs it before it is ready is queued with
 * [whenReady] and replayed in order.
 */
object RelayRuntime {
	private const val TAG = "relay"

	private val worker = Executors.newSingleThreadExecutor { Thread(it, "relay-engine-control") }
	private val main = Handler(Looper.getMainLooper())
	private val pending = mutableListOf<(RelayEngine) -> Unit>()
	private val callListeners = mutableListOf<(CallInfo) -> Unit>()
	private val policy = ServicePolicy()
	private val availabilityPolicy = AvailabilityPolicy()
	private val availabilityListeners = mutableListOf<(Availability) -> Unit>()
	private val incomingPolicy = IncomingPolicy()
	private val autoNotice = AutoAnswerNotice()

	lateinit var app: Application
		private set
	lateinit var locks: NetworkLocks
		private set

	/** The phone's call audio state (mode, focus, route), see [LingeringHold]. */
	lateinit var callAudio: LingeringHold
		private set

	lateinit var prefs: AvailabilityPrefs
		private set
	lateinit var notifier: IncomingCallNotifier
		private set
	lateinit var exemption: ExemptionPrompt
		private set

	/** What the availability service is doing; [Availability.OFF] when it is not running. */
	@Volatile
	var availability: Availability = Availability.OFF
		private set

	/** `calls.alert`, as the engine last said. */
	@Volatile
	var alert: CallAlert = CallAlert.SYSTEM
		private set

	/** A Relay screen is on show (the activity is started). */
	@Volatile
	var visible: Boolean = false
		private set

	private var onLan = false

	@Volatile
	var engine: RelayEngine? = null
		private set

	@Volatile
	var call: CallInfo = CallInfo.IDLE
		private set

	val microphone = PermissionGate { gate ->
		Log.i(TAG, "microphone permission: $gate")
		whenReady { it.setMicrophone(gate.access().toState()) }
		main.post { reevaluate() }
	}
	/** The camera permission, asked on the first tap of Scan and never at startup. */
	val camera = PermissionGate { Log.i(TAG, "camera permission: $it") }
	val scan = ScanFlow(camera) { id, result -> whenReady { it.completeScan(id, result.toOutcome()) } }
	val notifications = PermissionGate { Log.i(TAG, "notification permission: $it") }

	/** Called once from [RelayApplication.onCreate]. */
	fun init(application: Application) {
		app = application
		locks = NetworkLocks(application)
		prefs = AvailabilityPrefs(application)
		notifier = IncomingCallNotifier(application)
		exemption = ExemptionPrompt({ prefs.exemptionAsked }, { prefs.exemptionAsked = true })
		watchLan(application)
		val router = AudioRouter(application)
		callAudio = LingeringHold(
			target = object : Releasable {
				override fun acquire() = router.enter()
				override fun release() = router.exit()
			},
			scheduler = Scheduler { delay, task ->
				val run = Runnable { task() }
				main.postDelayed(run, delay)
				val cancel: () -> Unit = { main.removeCallbacks(run) }
				cancel
			},
		)
		worker.execute { startEngine() }
	}

	private fun Access.toState() = when (this) {
		Access.READY -> MicrophoneState.READY
		Access.NOT_ASKED -> MicrophoneState.NOT_ASKED
		Access.DENIED -> MicrophoneState.DENIED
		Access.BLOCKED -> MicrophoneState.BLOCKED
	}

	private fun deviceName(): String? = DeviceName.choose(
		runCatching { Settings.Global.getString(app.contentResolver, "device_name") }.getOrNull(),
		Build.MODEL,
	)

	private fun ScanResult.toOutcome() = when (this) {
		is ScanResult.Text -> ScanOutcome.Text(text)
		ScanResult.Cancelled -> ScanOutcome.Cancelled
		ScanResult.CameraDenied -> ScanOutcome.CameraDenied
		ScanResult.CameraBlocked -> ScanOutcome.CameraBlocked
		ScanResult.Unavailable -> ScanOutcome.Unavailable
		is ScanResult.Failed -> ScanOutcome.Failed(reason)
	}

	private fun startEngine() {
		try {
			val started = RelayEngine(
				EngineConfig(app.filesDir.absolutePath, deviceName()),
				AndroidBridge(app),
			)
			val queued = synchronized(pending) {
				engine = started
				pending.toList().also { pending.clear() }
			}
			// A restarted engine starts with nothing known about the permission.
			started.setMicrophone(microphone.state.access().toState())
			started.setScanner(app.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY))
			started.setAvailable(availability == Availability.ACTIVE)
			queued.forEach { it(started) }
			Log.i(TAG, "engine ready")
		} catch (e: Exception) {
			Log.e(TAG, "engine failed to start", e)
		}
	}

	/** Runs [action] now if the engine is up, else as soon as it is. */
	fun whenReady(action: (RelayEngine) -> Unit) {
		val ready = synchronized(pending) {
			engine ?: run { pending += action; null }
		}
		ready?.let(action)
	}

	fun addCallListener(listener: (CallInfo) -> Unit) = synchronized(callListeners) { callListeners += listener }

	fun removeCallListener(listener: (CallInfo) -> Unit) = synchronized(callListeners) { callListeners -= listener }

	/** The engine reported a change to the call (from a Rust thread). */
	fun onCallChanged(info: CallInfo) {
		call = info
		main.post {
			synchronized(callListeners) { callListeners.toList() }.forEach { it(info) }
			reevaluate()
			reevaluateAlerts()
		}
	}

	/** The engine reported `background.available` and `calls.alert` (from a Rust thread). */
	fun onBackgroundChanged(enabled: Boolean, alertName: String) {
		main.post {
			prefs.enabled = enabled
			alert = CallAlert.parse(alertName)
			reevaluateAvailability()
		}
	}

	/** The activity is started or stopped. */
	fun setVisible(value: Boolean) {
		visible = value
		if (value) {
			// Opening the app ends a Pause and tells the user nothing needs opening.
			prefs.paused = false
			notifier.cancelOpenRelayNeeded()
			exemption.resumed()
		}
		main.post {
			reevaluateAvailability()
			reevaluateAlerts()
			reevaluate()
		}
	}

	/** Shows or withdraws the incoming-call and answered-automatically notifications. */
	fun reevaluateAlerts() {
		val info = call
		when (val action = incomingPolicy.decide(info, visible, alert, notifier.canFullScreen())) {
			is IncomingAction.Show -> notifier.show(info, action.plan, action.fullScreen, microphone.granted)
			IncomingAction.Cancel -> notifier.cancelIncoming()
			IncomingAction.None -> Unit
		}
		if (autoNotice.changed(info)) {
			notifier.autoAnswered(info, autoAnswerNotification(microphone.granted))
		}
		if (!info.active) notifier.cancelAutoAnswered()
	}

	fun addAvailabilityListener(listener: (Availability) -> Unit) = synchronized(availabilityListeners) { availabilityListeners += listener }

	fun removeAvailabilityListener(listener: (Availability) -> Unit) = synchronized(availabilityListeners) { availabilityListeners -= listener }

	/** Starts, changes or stops the availability service. Main thread only. */
	fun reevaluateAvailability(fromBoot: Boolean = false) {
		val input = AvailabilityInput(prefs.enabled ?: false, prefs.paused, onLan)
		when (val action = availabilityPolicy.decide(input)) {
			is AvailabilityAction.Start -> {
				availability = action.state
				try {
					ContextCompat.startForegroundService(app, Intent(app, AvailabilityService::class.java))
					notifier.cancelOpenRelayNeeded()
				} catch (e: RuntimeException) {
					// Android 12+ refuses a start from the background; ask the user to open the app.
					Log.w(TAG, "availability service refused (boot=$fromBoot)", e)
					availabilityPolicy.stoppedBySystem()
					availability = Availability.OFF
					notifier.openRelayNeeded()
				}
			}
			is AvailabilityAction.Change -> {
				availability = action.state
				synchronized(availabilityListeners) { availabilityListeners.toList() }.forEach { it(action.state) }
			}
			AvailabilityAction.Stop -> {
				availability = Availability.OFF
				app.stopService(Intent(app, AvailabilityService::class.java))
			}
			AvailabilityAction.None -> Unit
		}
	}

	/** The Pause button of the availability notification. */
	fun pauseAvailability() {
		prefs.paused = true
		main.post { reevaluateAvailability() }
	}

	fun availabilityStopped() {
		availabilityPolicy.stoppedBySystem()
		availability = Availability.OFF
	}

	/** Tracks Wi-Fi and Ethernet; the availability service waits without them. */
	private fun watchLan(application: Application) {
		val connectivity = application.getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
		onLan = Lan.anyLan(
			connectivity.allNetworks.mapNotNull { connectivity.getNetworkCapabilities(it) }.map { it.transports() },
		)
		val live = mutableSetOf<Network>()
		fun changed() {
			val now = live.isNotEmpty()
			main.post {
				if (now != onLan) {
					onLan = now
					reevaluateAvailability()
				}
			}
		}
		val callback = object : ConnectivityManager.NetworkCallback() {
			override fun onAvailable(network: Network) {
				synchronized(live) { live += network }
				changed()
			}

			override fun onLost(network: Network) {
				synchronized(live) { live -= network }
				changed()
			}
		}
		for (transport in intArrayOf(NetworkCapabilities.TRANSPORT_WIFI, NetworkCapabilities.TRANSPORT_ETHERNET)) {
			runCatching {
				connectivity.registerNetworkCallback(
					NetworkRequest.Builder().addTransportType(transport).build(),
					callback,
				)
			}.onFailure { Log.w(TAG, "network callback failed", it) }
		}
	}

	private fun NetworkCapabilities.transports(): Set<Lan.Transport> = buildSet {
		if (hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) add(Lan.Transport.WIFI)
		if (hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) add(Lan.Transport.ETHERNET)
		if (hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) add(Lan.Transport.CELLULAR)
		if (hasTransport(NetworkCapabilities.TRANSPORT_VPN)) add(Lan.Transport.VPN)
	}

	/**
	 * Tells the UI what it asks about per frame: the battery exemption, whether
	 * notifications show and whether there is a vibrator. Cheap enough to call on
	 * every resume and every recheck.
	 */
	fun pushPlatformState() {
		val exempt = runCatching { batteryExempt() }.getOrDefault(true)
		val notify = NotificationManagerCompat.from(app).areNotificationsEnabled()
		val vibrate = (app.getSystemService(Context.VIBRATOR_SERVICE) as? Vibrator)?.hasVibrator() == true
		whenReady {
			it.setBatteryExempt(exempt)
			it.setNotificationsAllowed(notify)
			it.setCanVibrate(vibrate)
		}
	}

	/** Whether Android lets Relay run unrestricted in the background. */
	fun batteryExempt(): Boolean =
		(app.getSystemService(Context.POWER_SERVICE) as PowerManager).isIgnoringBatteryOptimizations(app.packageName)

	/** Starts, refreshes or stops the foreground service for the current call. */
	fun reevaluate() {
		val context: Context = app
		when (val action = policy.decide(call, microphone.granted, visible)) {
			is ServiceAction.Start -> try {
				ContextCompat.startForegroundService(context, Intent(context, CallService::class.java))
			} catch (e: RuntimeException) {
				// A microphone service cannot start from the background (an auto-answered call): ask to open the app.
				Log.w(TAG, "call service refused", e)
				policy.stoppedBySystem()
				notifier.autoAnswered(call, AutoAnswerNotification.NEEDS_MICROPHONE)
			}
			is ServiceAction.Stop -> context.stopService(Intent(context, CallService::class.java))
			ServiceAction.NeedMicrophone -> MainActivity.current?.askForMicrophone()
			is ServiceAction.Update, ServiceAction.None -> Unit
		}
	}

	fun serviceStopped() = policy.stoppedBySystem()

	/** Closing the app ends the call and takes the engine off the LAN. */
	fun shutdown() {
		if (availability != Availability.OFF) {
			// Availability keeps the engine: closing the app only ends the call.
			whenReady { it.hangUp() }
			return
		}
		main.post { app.stopService(Intent(app, CallService::class.java)) }
		worker.execute {
			val running = synchronized(pending) { engine.also { engine = null } }
			running?.stop()
			Log.i(TAG, "engine stopped")
			// A later launch in the same process starts a fresh engine.
			startEngine()
		}
	}
}
