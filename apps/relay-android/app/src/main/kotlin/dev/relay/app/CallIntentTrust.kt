package dev.relay.app

import java.security.MessageDigest
import java.security.SecureRandom

/** What `MainActivity` does with an intent that claims to come from a call notification. */
enum class CallIntentAction {
	/** Not ours (no or wrong token): treat it as an ordinary launch and act on none of its extras. */
	IGNORE,

	/** Show the call screen over the lock screen and turn the screen on. */
	SHOW,

	/** As [SHOW], and answer once the microphone is allowed. */
	SHOW_AND_ANSWER,
}

/**
 * `MainActivity` is exported (it is the launcher), so any app can send it any
 * extras. The call extras are honoured only with this process's random token,
 * which Relay puts into its own immutable notification PendingIntents. A foreign
 * intent cannot know it, and it changes with every process start (notifications
 * carrying an old token are withdrawn at start).
 */
class CallIntentTrust(val token: String) {
	/** Constant-time check of the token an intent carries. */
	fun accepts(candidate: String?): Boolean {
		if (candidate == null) return false
		return MessageDigest.isEqual(token.toByteArray(), candidate.toByteArray())
	}

	fun action(fromCall: Boolean, answer: Boolean, candidate: String?): CallIntentAction = when {
		!accepts(candidate) -> CallIntentAction.IGNORE
		!fromCall -> CallIntentAction.IGNORE
		answer -> CallIntentAction.SHOW_AND_ANSWER
		else -> CallIntentAction.SHOW
	}

	companion object {
		fun random(): CallIntentTrust {
			val bytes = ByteArray(24).also { SecureRandom().nextBytes(it) }
			return CallIntentTrust(bytes.joinToString("") { "%02x".format(it) })
		}
	}
}
