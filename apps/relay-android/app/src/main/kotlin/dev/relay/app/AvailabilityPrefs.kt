package dev.relay.app

import android.content.Context

/**
 * What the shell must know before the engine has said anything (after a
 * reboot, the receiver runs before any state arrives): a copy of
 * `background.available`, the call alert, and the user's Pause. The engine's
 * setting stays the truth; the copy is rewritten whenever it changes.
 */
class AvailabilityPrefs(context: Context) {
	private val prefs = context.applicationContext.getSharedPreferences("availability", Context.MODE_PRIVATE)

	/** `background.available` as last seen; null before the first snapshot. */
	var enabled: Boolean?
		get() = if (prefs.contains(ENABLED)) prefs.getBoolean(ENABLED, true) else null
		set(value) {
			prefs.edit().apply { if (value == null) remove(ENABLED) else putBoolean(ENABLED, value) }.apply()
		}

	/** Pause pressed in the notification; cleared when the app is opened. */
	var paused: Boolean
		get() = prefs.getBoolean(PAUSED, false)
		set(value) = prefs.edit().putBoolean(PAUSED, value).apply()

	/** The battery-optimization dialog has been shown once. */
	var exemptionAsked: Boolean
		get() = prefs.getBoolean(EXEMPTION_ASKED, false)
		set(value) = prefs.edit().putBoolean(EXEMPTION_ASKED, value).apply()

	/** A one-time question (by id) was answered; false until [setQuestionAnswered]. */
	fun questionAnswered(id: String): Boolean = prefs.getBoolean("question_$id", false)

	fun setQuestionAnswered(id: String) = prefs.edit().putBoolean("question_$id", true).apply()

	private companion object {
		const val ENABLED = "enabled"
		const val PAUSED = "paused"
		const val EXEMPTION_ASKED = "exemption_asked"
	}
}
