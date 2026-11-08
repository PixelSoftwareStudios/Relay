package dev.relay.app

import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class QrDecoderTest {
	private val decoder = QrDecoder()
	private val text = "relay://192.168.1.20:7777/abc?fp=0123456789"

	/** Luminance bytes of a code with a quiet zone, 4 pixels per module and [padding] spare bytes per row. */
	private fun image(content: String, inverted: Boolean, padding: Int): Triple<ByteArray, Int, Int> {
		val matrix = QRCodeWriter().encode(content, BarcodeFormat.QR_CODE, 0, 0, mapOf(EncodeHintType.MARGIN to 4))
		val scale = 4
		val w = matrix.width * scale
		val h = matrix.height * scale
		val stride = w + padding
		val data = ByteArray(stride * h) { 128.toByte() }
		for (y in 0 until h) for (x in 0 until w) {
			val dark = matrix.get(x / scale, y / scale)
			val black = dark != inverted
			data[y * stride + x] = (if (black) 0 else 255).toByte()
		}
		return Triple(data, w, h)
	}

	@Test
	fun readsADarkOnLightCode() {
		val (data, w, h) = image(text, inverted = false, padding = 0)
		assertEquals(text, decoder.decode(data, w, w, h))
	}

	@Test
	fun readsAnInvertedCode() {
		val (data, w, h) = image(text, inverted = true, padding = 0)
		assertEquals(text, decoder.decode(data, w, w, h))
	}

	@Test
	fun readsFramesWhoseRowsArePaddedPastTheWidth() {
		val (data, w, h) = image(text, false, 24)
		assertEquals(text, decoder.decode(data, w + 24, w, h))
	}

	@Test
	fun noCodeIsNull() {
		val data = ByteArray(200 * 200) { 200.toByte() }
		assertNull(decoder.decode(data, 200, 200, 200))
	}

	@Test
	fun theThrottleAdmitsOneFramePerInterval() {
		val throttle = FrameThrottle(150)
		assertTrue(throttle.admit(0))
		assertFalse(throttle.admit(100))
		assertTrue(throttle.admit(150))
		assertFalse(throttle.admit(200))
		assertTrue(throttle.admit(10_000))
	}
}
