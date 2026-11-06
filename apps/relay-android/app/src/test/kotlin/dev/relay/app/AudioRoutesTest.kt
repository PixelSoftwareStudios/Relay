package dev.relay.app

import android.media.AudioDeviceInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AudioRoutesTest {
	@Test
	fun aHeadsetBeatsTheSpeakerAndBluetoothBeatsWired() {
		val all = listOf(
			AudioDeviceInfo.TYPE_BUILTIN_EARPIECE,
			AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
			AudioDeviceInfo.TYPE_WIRED_HEADSET,
			AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
		)
		assertEquals(AudioDeviceInfo.TYPE_BLUETOOTH_SCO, AudioRoutes.choose(all))
		assertEquals(AudioDeviceInfo.TYPE_WIRED_HEADSET, AudioRoutes.choose(all - AudioDeviceInfo.TYPE_BLUETOOTH_SCO))
	}

	@Test
	fun withNothingElseTheLoudspeakerIsUsedNotTheEarpiece() {
		val onlyBuiltIn = listOf(AudioDeviceInfo.TYPE_BUILTIN_EARPIECE, AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
		assertEquals(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER, AudioRoutes.choose(onlyBuiltIn))
		assertNull(AudioRoutes.choose(listOf(AudioDeviceInfo.TYPE_BUILTIN_EARPIECE)))
	}
}
