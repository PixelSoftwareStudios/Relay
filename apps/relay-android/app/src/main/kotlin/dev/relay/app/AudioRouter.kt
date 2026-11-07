package dev.relay.app

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioDeviceCallback
import android.media.AudioManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
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
	private var current: Int? = null
	private val devices = object : AudioDeviceCallback() {
		override fun onAudioDevicesAdded(added: Array<out AudioDeviceInfo>) = reroute()
		override fun onAudioDevicesRemoved(removed: Array<out AudioDeviceInfo>) = reroute()
	}

	/**
	 * Mode and route are in place when this returns: the engine opens its
	 * stream right after, and either change would disconnect it. Focus and the
	 * headset callback are not needed for that, so they follow on the main
	 * thread instead of delaying the first sound.
	 */
	@Synchronized
	fun enter() {
		if (active) return
		active = true
		val started = SystemClock.elapsedRealtime()
		previousMode = audio.mode
		audio.mode = AudioManager.MODE_IN_COMMUNICATION
		route()
		Log.i(TAG, "audio: mode=${audio.mode} route set in ${SystemClock.elapsedRealtime() - started} ms")
		Handler(Looper.getMainLooper()).post { finishEnter() }
	}

	@Synchronized
	private fun finishEnter() {
		if (!active) return
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
		// A headset plugged in or taken out during the call moves it. The
		// engine reopens its streams on whatever route the system settles on.
		audio.registerAudioDeviceCallback(devices, Handler(Looper.getMainLooper()))
		Log.i(TAG, "audio: focus=$granted")
	}

	@Synchronized
	fun exit() {
		if (!active) return
		active = false
		runCatching { audio.unregisterAudioDeviceCallback(devices) }
		current = null
		if (Build.VERSION.SDK_INT >= 31) audio.clearCommunicationDevice() else audio.isSpeakerphoneOn = false
		focus?.let { audio.abandonAudioFocusRequest(it) }
		focus = null
		audio.mode = previousMode
		Log.i(TAG, "audio: restored mode=${audio.mode}")
	}

	@Synchronized
	private fun reroute() {
		if (active) route()
	}

	private fun route() {
		if (Build.VERSION.SDK_INT >= 31) {
			val devices: List<AudioDeviceInfo> = audio.availableCommunicationDevices
			val type = AudioRoutes.switchTo(current, devices.map { it.type }) ?: return
			val device = devices.firstOrNull { it.type == type } ?: return
			current = type
			Log.i(TAG, "audio route: type=${device.type} set=${audio.setCommunicationDevice(device)}")
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
