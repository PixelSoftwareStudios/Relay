package dev.relay.app

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Build
import android.util.Log
import uniffi.relay_android.FoundService
import uniffi.relay_android.RelayEngine
import java.util.concurrent.Executors

/**
 * Android's own service browser as a second source of LAN peers. The engine's
 * `mdns-sd` stays the primary discovery; some devices and Wi-Fi stacks deliver
 * multicast badly, and `NsdManager` goes through the system's mDNS responder
 * which often still works. Whatever it resolves is pushed to the engine as a
 * hint: same address policy, same untrusted candidate table.
 */
class NsdDiscovery(context: Context, private val engine: () -> RelayEngine?) {
    private val nsd = context.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val resolver = Executors.newSingleThreadExecutor()
    private var listener: NsdManager.DiscoveryListener? = null

    @Synchronized
    fun start() {
        if (listener != null) return
        val discovery = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(serviceType: String) {
                Log.i(TAG, "nsd browsing $serviceType")
            }
            override fun onDiscoveryStopped(serviceType: String) {
                Log.i(TAG, "nsd stopped")
            }
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "nsd start failed: $errorCode")
                synchronized(this@NsdDiscovery) { listener = null }
            }
            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                Log.w(TAG, "nsd stop failed: $errorCode")
            }
            override fun onServiceFound(service: NsdServiceInfo) {
                resolve(service)
            }
            override fun onServiceLost(service: NsdServiceInfo) {
                engine()?.serviceLost(fullName(service.serviceName))
            }
        }
        listener = discovery
        nsd.discoverServices(SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, discovery)
    }

    @Synchronized
    fun stop() {
        val active = listener ?: return
        listener = null
        try {
            nsd.stopServiceDiscovery(active)
        } catch (e: IllegalArgumentException) {
            Log.w(TAG, "nsd stop: ${e.message}")
        }
    }

    @Suppress("DEPRECATION")
    private fun resolve(service: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= 34) {
            val callback = object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                    Log.w(TAG, "nsd resolve failed: $errorCode")
                }
                override fun onServiceUpdated(info: NsdServiceInfo) {
                    push(info, info.hostAddresses.mapNotNull { it.hostAddress })
                    // One resolution is enough: later changes arrive as new found events.
                    // A second update can arrive before the first unregister lands, and
                    // unregistering twice throws on the system's callback thread.
                    try {
                        nsd.unregisterServiceInfoCallback(this)
                    } catch (e: IllegalArgumentException) {
                        Log.d(TAG, "nsd callback already unregistered")
                    }
                }
                override fun onServiceLost() = Unit
                override fun onServiceInfoCallbackUnregistered() = Unit
            }
            nsd.registerServiceInfoCallback(service, resolver, callback)
        } else {
            nsd.resolveService(
                service,
                object : NsdManager.ResolveListener {
                    override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {
                        Log.w(TAG, "nsd resolve failed: $errorCode")
                    }
                    override fun onServiceResolved(info: NsdServiceInfo) {
                        push(info, listOfNotNull(info.host?.hostAddress))
                    }
                },
            )
        }
    }

    private fun push(info: NsdServiceInfo, addresses: List<String>) {
        val txt = info.attributes.mapValues { (_, v) -> v?.toString(Charsets.UTF_8) ?: "" }
        // Scope ids ("fe80::1%wlan0") are not accepted by the engine's parser.
        val clean = addresses.map { it.substringBefore('%') }
        Log.i(TAG, "nsd resolved ${info.serviceName} at $clean:${info.port}")
        engine()?.serviceFound(
            FoundService(fullName(info.serviceName), info.port.toUShort(), clean, txt),
        )
    }

    private fun fullName(instance: String) = "$instance.$SERVICE_TYPE.local."

    private companion object {
        const val TAG = "relay"
        const val SERVICE_TYPE = "_praxis-relay._tcp"
    }
}
