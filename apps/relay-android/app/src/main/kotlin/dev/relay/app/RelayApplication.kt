package dev.relay.app

import android.app.Application

class RelayApplication : Application() {
	override fun onCreate() {
		super.onCreate()
		RelayRuntime.init(this)
	}
}
