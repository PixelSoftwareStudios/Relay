package dev.relay.app

import com.google.zxing.BarcodeFormat
import com.google.zxing.BinaryBitmap
import com.google.zxing.DecodeHintType
import com.google.zxing.InvertedLuminanceSource
import com.google.zxing.LuminanceSource
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.ReaderException
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader

/**
 * Offline QR decoding with ZXing's pure-Java core: no Google components, no
 * network. Only QR codes are tried, light on dark first and then dark on light
 * (an inverted code, as some screens show it).
 */
class QrDecoder {
	private val reader = QRCodeReader()
	private val hints = mapOf(
		DecodeHintType.POSSIBLE_FORMATS to listOf(BarcodeFormat.QR_CODE),
		DecodeHintType.TRY_HARDER to true,
	)

	/**
	 * Decodes the luminance plane of a camera frame: [data] holds [rowStride] bytes
	 * per row, of which the first [width] are pixels. Returns the text, or null when
	 * there is no readable code. Rotation does not matter to a QR reader.
	 */
	fun decode(data: ByteArray, rowStride: Int, width: Int, height: Int): String? =
		decode(PlanarYUVLuminanceSource(data, rowStride, height, 0, 0, width, height, false))

	fun decode(source: LuminanceSource): String? =
		attempt(source) ?: attempt(InvertedLuminanceSource(source))

	private fun attempt(source: LuminanceSource): String? = try {
		reader.decode(BinaryBitmap(HybridBinarizer(source)), hints).text
	} catch (e: ReaderException) {
		null
	} finally {
		reader.reset()
	}
}

/** Lets through at most one frame per [intervalMs]; the rest are skipped without decoding. */
class FrameThrottle(private val intervalMs: Long) {
	private var last = Long.MIN_VALUE

	fun admit(nowMs: Long): Boolean {
		if (last != Long.MIN_VALUE && nowMs - last < intervalMs) return false
		last = nowMs
		return true
	}
}
