package dev.relay.app

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.SurfaceHolder
import android.view.WindowManager
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat
import androidx.core.view.WindowCompat
import com.google.androidgamesdk.GameActivity
import uniffi.relay_android.PickKind

/**
 * Hosts the Rust UI. `GameActivity` (not `NativeActivity`) because it gives
 * the soft keyboard a real text-input connection, is an `AppCompatActivity`
 * (permissions and document pickers use the Activity Result API) and takes
 * window insets like any other view. The Rust library is loaded from the
 * `android.app.lib_name` manifest entry; this class only forwards lifecycle,
 * insets, keys and results.
 */
class MainActivity : GameActivity() {
	private lateinit var picker: DocumentPicker
	private lateinit var scanner: QrScanner
	private lateinit var permissions: androidx.activity.result.ActivityResultLauncher<Array<String>>

	override fun onCreate(savedInstanceState: Bundle?) {
		WindowCompat.setDecorFitsSystemWindows(window, false)
		window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
		current = this
		picker = DocumentPicker(this).also { it.register() }
		scanner = QrScanner(this, RelayRuntime.scan).also { it.register() }
		permissions = registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
			result[Manifest.permission.RECORD_AUDIO]?.let { onMicrophoneResult(it) }
			result[Manifest.permission.POST_NOTIFICATIONS]?.let {
				notifications().result(it, shouldShowRequestPermissionRationale(Manifest.permission.POST_NOTIFICATIONS))
			}
		}
		super.onCreate(savedInstanceState)
		InsetsTracker(window.decorView).attach()
		readPermissions()
		requestMissingPermissions()
	}

	/**
	 * While the microphone is missing and the app is visible (a split-screen
	 * settings panel, a permission granted from a shade or adb) nothing
	 * resumes the activity, so look again every few seconds.
	 */
	private val recheck = object : Runnable {
		override fun run() {
			readPermissions()
			if (!microphone().granted) handler.postDelayed(this, RECHECK_MS)
		}
	}
	private val handler = Handler(Looper.getMainLooper())

	override fun onStart() {
		super.onStart()
		handler.postDelayed(recheck, RECHECK_MS)
		RelayRuntime.locks.hold(OWNER)
		nsd.start()
		RelayRuntime.whenReady { it.setForeground(true) }
		readPermissions()
	}

	/** Back from the system settings (or any other app): the permission may have changed. */
	override fun onResume() {
		super.onResume()
		readPermissions()
	}

	override fun onStop() {
		handler.removeCallbacks(recheck)
		releaseTalk()
		RelayRuntime.whenReady { it.setForeground(false) }
		nsd.stop()
		RelayRuntime.locks.drop(OWNER)
		super.onStop()
	}

	override fun onDestroy() {
		if (current === this) current = null
		if (isFinishing) RelayRuntime.shutdown()
		super.onDestroy()
	}

	/** Anything that could swallow the button's release lets go of the talk lease. */
	private fun releaseTalk() = RelayRuntime.engine?.cancelTalk()

	override fun onPause() {
		releaseTalk()
		super.onPause()
	}

	override fun onWindowFocusChanged(hasFocus: Boolean) {
		if (!hasFocus) releaseTalk()
		super.onWindowFocusChanged(hasFocus)
	}

	override fun surfaceDestroyed(holder: SurfaceHolder) {
		releaseTalk()
		super.surfaceDestroyed(holder)
	}

	fun pickDocument(id: ULong, kind: PickKind) = picker.launch(id, kind)

	fun scanQr(id: ULong) = scanner.start(id)

	fun askForMicrophone() {
		if (microphone().shouldRequest()) requestMissingPermissions()
	}

	private val nsd by lazy { NsdDiscovery(this) { RelayRuntime.engine } }

	private fun microphone() = RelayRuntime.microphone
	private fun notifications() = RelayRuntime.notifications

	private fun granted(permission: String) =
		ContextCompat.checkSelfPermission(this, permission) == PackageManager.PERMISSION_GRANTED

	private fun readPermissions() {
		microphone().checked(granted(Manifest.permission.RECORD_AUDIO))
		notifications().checked(Build.VERSION.SDK_INT < 33 || granted(Manifest.permission.POST_NOTIFICATIONS))
	}

	private fun requestMissingPermissions() {
		val wanted = buildList {
			if (microphone().shouldRequest()) add(Manifest.permission.RECORD_AUDIO)
			if (Build.VERSION.SDK_INT >= 33 && notifications().shouldRequest()) add(Manifest.permission.POST_NOTIFICATIONS)
		}
		if (wanted.isEmpty()) return
		if (Manifest.permission.RECORD_AUDIO in wanted) microphone().requestStarted()
		if (Manifest.permission.POST_NOTIFICATIONS in wanted) notifications().requestStarted()
		Log.i(TAG, "requesting $wanted")
		permissions.launch(wanted.toTypedArray())
	}

	private fun onMicrophoneResult(granted: Boolean) {
		microphone().result(granted, shouldShowRequestPermissionRationale(Manifest.permission.RECORD_AUDIO))
	}

	companion object {
		private const val TAG = "relay"
		private const val OWNER = "activity"
		private const val RECHECK_MS = 3000L

		@Volatile
		var current: MainActivity? = null
			private set
	}
}
