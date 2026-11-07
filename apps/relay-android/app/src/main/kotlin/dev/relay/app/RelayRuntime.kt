package dev.relay.app

import android.app.Application
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
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

	lateinit var app: Application
		private set
	lateinit var locks: NetworkLocks
		private set

	/** The phone's call audio state (mode, focus, route), see [LingeringHold]. */
	lateinit var callAudio: LingeringHold
		private set

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
		}
	}

	/** Starts, refreshes or stops the foreground service for the current call. */
	fun reevaluate() {
		val context: Context = app
		when (val action = policy.decide(call, microphone.granted)) {
			is ServiceAction.Start -> ContextCompat.startForegroundService(context, Intent(context, CallService::class.java))
			is ServiceAction.Stop -> context.stopService(Intent(context, CallService::class.java))
			ServiceAction.NeedMicrophone -> MainActivity.current?.askForMicrophone()
			is ServiceAction.Update, ServiceAction.None -> Unit
		}
	}

	fun serviceStopped() = policy.stoppedBySystem()

	/** Closing the app ends the call and takes the engine off the LAN. */
	fun shutdown() {
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
