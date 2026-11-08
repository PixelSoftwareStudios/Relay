package dev.relay.app

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import androidx.camera.core.Camera
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Reads one QR code with the camera (CameraX preview plus ZXing's pure-Java
 * decoder on the luminance plane: no Google components, no network). It returns the raw text and
 * nothing else; the Rust UI decides whether it is a Relay address. The camera
 * is bound to this activity's lifecycle, so a recreated screen simply rebinds.
 */
class ScanActivity : AppCompatActivity() {
	private lateinit var preview: PreviewView
	private lateinit var torch: Button
	private lateinit var executor: ExecutorService
	private val decoder = QrDecoder()
	private val throttle = FrameThrottle(FRAME_INTERVAL_MS)
	private var camera: Camera? = null
	private var torchOn = false
	private var done = false

	override fun onCreate(savedInstanceState: Bundle?) {
		super.onCreate(savedInstanceState)
		executor = Executors.newSingleThreadExecutor()
		preview = PreviewView(this).apply { implementationMode = PreviewView.ImplementationMode.COMPATIBLE }
		setContentView(layout())
		setResult(RESULT_CANCELED)
		startCamera()
	}

	private fun layout(): View {
		val hint = TextView(this).apply {
			setText(R.string.scan_hint)
			setTextColor(Color.WHITE)
			textSize = 16f
			gravity = Gravity.CENTER
			setBackgroundColor(0x99000000.toInt())
			setPadding(dp(16), dp(12), dp(16), dp(12))
		}
		torch = Button(this).apply {
			setText(R.string.scan_torch)
			visibility = View.GONE
			setOnClickListener { toggleTorch() }
		}
		val cancel = Button(this).apply {
			setText(R.string.scan_cancel)
			setOnClickListener { finish() }
		}
		val bar = LinearLayout(this).apply {
			orientation = LinearLayout.VERTICAL
			setPadding(dp(16), dp(8), dp(16), dp(16))
			addView(torch)
			addView(cancel)
		}
		return FrameLayout(this).apply {
			setBackgroundColor(Color.BLACK)
			addView(preview, FrameLayout.LayoutParams(-1, -1))
			addView(hint, FrameLayout.LayoutParams(-1, -2, Gravity.TOP))
			addView(bar, FrameLayout.LayoutParams(-1, -2, Gravity.BOTTOM))
			// Edge-to-edge: keep the hint and the buttons clear of the system bars.
			ViewCompat.setOnApplyWindowInsetsListener(this) { _, insets ->
				val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
				hint.translationY = bars.top.toFloat()
				bar.translationY = -bars.bottom.toFloat()
				insets
			}
		}
	}

	private fun dp(value: Int) = (value * resources.displayMetrics.density).toInt()

	private fun startCamera() {
		val future = ProcessCameraProvider.getInstance(this)
		future.addListener({
			try {
				val provider = future.get()
				val view = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
				val analysis = ImageAnalysis.Builder()
					.setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
					.build()
				analysis.setAnalyzer(executor) { frame ->
					try {
						if (!done && throttle.admit(SystemClock.elapsedRealtime())) {
							val plane = frame.planes[0]
							val buffer = plane.buffer
							val data = ByteArray(buffer.remaining()).also { buffer.get(it) }
							decoder.decode(data, plane.rowStride, frame.width, frame.height)?.let(::found)
						}
					} catch (e: RuntimeException) {
						Log.w(TAG, "frame skipped", e)
					} finally {
						frame.close()
					}
				}
				provider.unbindAll()
				val bound = provider.bindToLifecycle(this, CameraSelector.DEFAULT_BACK_CAMERA, view, analysis)
				camera = bound
				if (bound.cameraInfo.hasFlashUnit()) torch.visibility = View.VISIBLE
			} catch (e: Exception) {
				Log.w(TAG, "camera failed", e)
				fail(e.javaClass.simpleName)
			}
		}, ContextCompat.getMainExecutor(this))
	}

	private fun toggleTorch() {
		torchOn = !torchOn
		camera?.cameraControl?.enableTorch(torchOn)
	}

	private fun found(text: String) = runOnUiThread {
		if (done) return@runOnUiThread
		done = true
		setResult(RESULT_OK, Intent().putExtra(EXTRA_TEXT, text))
		finish()
	}

	private fun fail(reason: String) {
		if (done) return
		done = true
		setResult(RESULT_OK, Intent().putExtra(EXTRA_ERROR, reason))
		finish()
	}

	override fun onDestroy() {
		executor.shutdown()
		super.onDestroy()
	}

	companion object {
		private const val TAG = "relay"
		/** Frames are decoded at about 7 per second; the preview itself runs at full rate. */
		private const val FRAME_INTERVAL_MS = 150L
		const val EXTRA_TEXT = "dev.relay.app.scan.text"
		const val EXTRA_ERROR = "dev.relay.app.scan.error"
	}
}
