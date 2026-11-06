package dev.relay.app

import android.view.View
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import uniffi.relay_android.InsetsPx

/**
 * Reports the system bars, display cutout and soft keyboard to the engine's UI
 * in physical pixels. The Rust side converts to points with the surface scale.
 */
class InsetsTracker(private val view: View) {
	fun attach() {
		ViewCompat.setOnApplyWindowInsetsListener(view) { _, insets ->
			val bars = insets.getInsets(
				WindowInsetsCompat.Type.systemBars() or WindowInsetsCompat.Type.displayCutout(),
			)
			val keyboard = if (insets.isVisible(WindowInsetsCompat.Type.ime())) {
				insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
			} else {
				0
			}
			val px = InsetsPx(
				top = bars.top.toFloat(),
				bottom = bars.bottom.toFloat(),
				left = bars.left.toFloat(),
				right = bars.right.toFloat(),
				keyboard = keyboard.toFloat(),
			)
			RelayRuntime.whenReady { it.setInsets(px) }
			insets
		}
		ViewCompat.requestApplyInsets(view)
	}
}
