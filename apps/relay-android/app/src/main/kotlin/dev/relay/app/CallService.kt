package dev.relay.app

import android.app.NotificationManager
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log
import androidx.core.app.ServiceCompat

/**
 * Foreground service of type `microphone`, alive exactly while a call is.
 * It keeps the process (and so the in-process engine) running with the screen
 * off, holds the Wi-Fi locks, puts audio in call mode and offers mute and
 * hang-up on the notification. The engine itself is not in the service: it
 * lives in the process, and the service only anchors it.
 */
class CallService : Service() {
    private lateinit var audio: AudioRouter
    private val listener: (CallInfo) -> Unit = { info ->
        if (info.active) {
            getSystemService(NotificationManager::class.java)
                .notify(CallNotification.ID, CallNotification.build(this, info))
        }
    }

    override fun onCreate() {
        super.onCreate()
        audio = AudioRouter(this)
        RelayRuntime.locks.hold(OWNER)
        audio.enter()
        RelayRuntime.addCallListener(listener)
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Must be called promptly after startForegroundService.
        val type = if (Build.VERSION.SDK_INT >= 29) ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE else 0
        ServiceCompat.startForeground(this, CallNotification.ID, CallNotification.build(this, RelayRuntime.call), type)
        val call = RelayRuntime.call
        when (intent?.action) {
            CallNotification.ACTION_MUTE -> RelayRuntime.whenReady { it.setMuted(true) }
            CallNotification.ACTION_UNMUTE -> RelayRuntime.whenReady { it.setMuted(false) }
            CallNotification.ACTION_HANGUP -> RelayRuntime.whenReady { it.hangUp() }
            CallNotification.ACTION_ANSWER -> RelayRuntime.whenReady { it.answer() }
        }
        Log.i(TAG, "call service running (action=${intent?.action}, peer active=${call.active})")
        return START_NOT_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Closing the app from recents hangs up and stops the engine.
        RelayRuntime.shutdown()
        stopSelf()
    }

    override fun onDestroy() {
        RelayRuntime.removeCallListener(listener)
        audio.exit()
        RelayRuntime.locks.drop(OWNER)
        RelayRuntime.serviceStopped()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private companion object {
        const val TAG = "relay"
        const val OWNER = "service"
    }
}
