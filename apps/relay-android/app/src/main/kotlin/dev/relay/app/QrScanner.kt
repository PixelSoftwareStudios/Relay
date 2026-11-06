package dev.relay.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat

/**
 * Connects [ScanFlow] to the activity: the camera permission dialog and the
 * scanner screen. The two launchers are registered in `onCreate`, so a
 * recreated activity gets the answer of a dialog or scanner that was open.
 */
class QrScanner(private val activity: AppCompatActivity, private val flow: ScanFlow) {
	private var permission: ActivityResultLauncher<String>? = null
	private var scanner: ActivityResultLauncher<Intent>? = null

	/** Must run in `onCreate`, before the activity is started. */
	fun register() {
		permission = activity.registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
			val canAskAgain = activity.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)
			run(flow.permissionAnswered(granted, canAskAgain))
		}
		scanner = activity.registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
			val data = result.data
			flow.scanned(data?.getStringExtra(ScanActivity.EXTRA_TEXT), data?.getStringExtra(ScanActivity.EXTRA_ERROR))
		}
	}

	fun start(id: ULong) {
		val hasCamera = activity.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY)
		val granted = ContextCompat.checkSelfPermission(activity, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
		run(flow.begin(id, hasCamera, granted))
	}

	private fun run(step: ScanStep) {
		when (step) {
			ScanStep.Launch -> scanner?.launch(Intent(activity, ScanActivity::class.java))
			ScanStep.Ask -> permission?.launch(Manifest.permission.CAMERA)
			is ScanStep.Finish -> Unit
		}
	}
}
