package dev.relay.app

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.os.Build
import android.util.Log

/**
 * Puts the phone in a voice-call audio state for the length of a call:
 * `MODE_IN_COMMUNICATION` (so the platform applies echo cancellation and call
 * volume), transient audio focus, and a sensible output route. The engine's
 * AAudio streams use the voice-communication input preset and usage on top.
 */
class AudioRouter(context: Context) {
	private val audio = context.applicationContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
	private var focus: AudioFocusRequest? = null
	private var previousMode = AudioManager.MODE_NORMAL
	private var active = false

	fun enter() {
		if (active) return
		active = true
		previousMode = audio.mode
		audio.mode = AudioManager.MODE_IN_COMMUNICATION
		val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT)
			.setAudioAttributes(
				AudioAttributes.Builder()
					.setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
					.setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
					.build(),
			)
			.setOnAudioFocusChangeListener { Log.i(TAG, "audio focus change $it") }
			.build()
		focus = request
		val granted = audio.requestAudioFocus(request)
		route()
		Log.i(TAG, "audio: mode=${audio.mode} focus=$granted")
	}

	fun exit() {
		if (!active) return
		active = false
		if (Build.VERSION.SDK_INT >= 31) audio.clearCommunicationDevice() else audio.isSpeakerphoneOn = false
		focus?.let { audio.abandonAudioFocusRequest(it) }
		focus = null
		audio.mode = previousMode
		Log.i(TAG, "audio: restored mode=${audio.mode}")
	}

	private fun route() {
		if (Build.VERSION.SDK_INT >= 31) {
			val devices: List<AudioDeviceInfo> = audio.availableCommunicationDevices
			val type = AudioRoutes.choose(devices.map { it.type })
			val device = devices.firstOrNull { it.type == type }
			if (device != null) {
				Log.i(TAG, "audio route: type=${device.type} set=${audio.setCommunicationDevice(device)}")
			}
		} else {
			val outputs = audio.getDevices(AudioManager.GET_DEVICES_OUTPUTS).map { it.type }
			val type = AudioRoutes.choose(outputs)
			audio.isSpeakerphoneOn = type == null || type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER
		}
	}

	private companion object {
		const val TAG = "relay"
	}
}
