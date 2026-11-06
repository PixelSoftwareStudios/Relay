package dev.relay.app

import android.content.Context
import android.net.wifi.WifiManager
import android.os.Build
import android.util.Log

/**
 * The Wi-Fi locks a LAN voice app needs.
 *
 * - `MulticastLock`: without it the Wi-Fi chip drops multicast frames to save
 *   power and mDNS discovery hears nothing.
 * - `WifiLock`: `WIFI_MODE_FULL_LOW_LATENCY` (API 29+, effective while the app
 *   is in the foreground with the screen on) turns off Wi-Fi power save, which
 *   otherwise batches packets and adds jitter to a call. Below API 29 the
 *   high-performance mode is the closest equivalent.
 *
 * Owners ("activity", "service") hold them through a [HoldSet] so neither
 * releases a lock the other still needs.
 */
class NetworkLocks(context: Context) {
    private val wifi = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager

    private val multicast = wifi.createMulticastLock("relay:mdns").apply { setReferenceCounted(false) }
    private val latency = wifi.createWifiLock(
        if (Build.VERSION.SDK_INT >= 29) WifiManager.WIFI_MODE_FULL_LOW_LATENCY else WifiManager.WIFI_MODE_FULL_HIGH_PERF,
        "relay:call",
    ).apply { setReferenceCounted(false) }

    private val multicastHolds = HoldSet(
        object : Releasable {
            override fun acquire() = log("multicast lock", multicast::acquire)
            override fun release() = log("multicast lock release", multicast::release)
        },
    )
    private val latencyHolds = HoldSet(
        object : Releasable {
            override fun acquire() = log("wifi lock", latency::acquire)
            override fun release() = log("wifi lock release", latency::release)
        },
    )

    fun hold(owner: String) {
        multicastHolds.hold(owner)
        latencyHolds.hold(owner)
    }

    fun drop(owner: String) {
        multicastHolds.drop(owner)
        latencyHolds.drop(owner)
    }

    val held: Boolean get() = multicast.isHeld && latency.isHeld

    private fun log(what: String, action: () -> Unit) {
        try {
            action()
            Log.i(TAG, "$what ok (multicast held=${multicast.isHeld}, wifi held=${latency.isHeld})")
        } catch (e: RuntimeException) {
            Log.e(TAG, "$what failed", e)
        }
    }

    private companion object {
        const val TAG = "relay"
    }
}
