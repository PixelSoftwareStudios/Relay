package dev.relay.app

import android.media.AudioDeviceInfo

/**
 * Which output a voice call should use. A connected headset or Bluetooth
 * device wins; otherwise the loudspeaker (the earpiece needs the phone at the
 * ear, which is wrong for a test call between two devices on a desk).
 */
object AudioRoutes {
    private val PREFERENCE = listOf(
        AudioDeviceInfo.TYPE_BLE_HEADSET,
        AudioDeviceInfo.TYPE_BLUETOOTH_SCO,
        AudioDeviceInfo.TYPE_USB_HEADSET,
        AudioDeviceInfo.TYPE_WIRED_HEADSET,
        AudioDeviceInfo.TYPE_WIRED_HEADPHONES,
        AudioDeviceInfo.TYPE_BUILTIN_SPEAKER,
    )

    /** The most preferred type present in [available], or null if none is usable. */
    fun choose(available: Collection<Int>): Int? = PREFERENCE.firstOrNull { it in available }
}
