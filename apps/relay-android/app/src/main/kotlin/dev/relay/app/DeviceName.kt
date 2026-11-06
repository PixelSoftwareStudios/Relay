package dev.relay.app

/**
 * The name a fresh install shows as the profile name. An Android hostname is
 * just `localhost`, so the engine is given the name the user sees for the
 * device ("Galaxy S25 Ultra") or, when that is unset, the hardware model. A
 * name the user typed in Relay always wins; the engine decides that.
 */
object DeviceName {
	/** The first of [settingsName] (`Settings.Global` `device_name`) and [model] that is not blank. */
	fun choose(settingsName: String?, model: String?): String? =
		listOf(settingsName, model).firstNotNullOfOrNull { it?.trim()?.takeIf(String::isNotEmpty) }
}
