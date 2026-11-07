package dev.relay.app

/** What the availability service is doing. */
enum class Availability {
	/** Not running: the setting is off or the user paused it. */
	OFF,

	/** Running, but there is no Wi-Fi or Ethernet: no locks, the engine stays off the LAN. */
	WAITING,

	/** Running on a LAN: locks held, a watch open, so the engine advertises, listens and rings. */
	ACTIVE,
}

/** What decides the state. */
data class AvailabilityInput(
	/** `background.available`. */
	val enabled: Boolean,
	/** The user pressed Pause in the notification; cleared when the app is opened. */
	val paused: Boolean,
	/** The device has Wi-Fi or Ethernet. */
	val onLan: Boolean,
) {
	fun target(): Availability = when {
		!enabled || paused -> Availability.OFF
		!onLan -> Availability.WAITING
		else -> Availability.ACTIVE
	}
}

sealed interface AvailabilityAction {
	/** Start the foreground service in this state. */
	data class Start(val state: Availability) : AvailabilityAction

	/** The service runs; change its state (notification, locks, watch). */
	data class Change(val state: Availability) : AvailabilityAction

	data object Stop : AvailabilityAction

	data object None : AvailabilityAction
}

/**
 * The lifecycle of the availability service.
 *
 * Losing the LAN does not stop the service, it moves to [Availability.WAITING].
 * Android 12 and later refuse to start a foreground service from the
 * background, and a network callback in a cached process is the background;
 * only a process that already has a foreground service may start one. So the
 * service keeps the process alive and waits, with no locks and no watch, and
 * [Availability.ACTIVE] returns when the network does.
 */
class AvailabilityPolicy {
	private var running = false
	private var shown = Availability.OFF

	fun decide(input: AvailabilityInput): AvailabilityAction {
		val target = input.target()
		if (target == Availability.OFF) {
			val was = running
			running = false
			shown = Availability.OFF
			return if (was) AvailabilityAction.Stop else AvailabilityAction.None
		}
		if (!running) {
			running = true
			shown = target
			return AvailabilityAction.Start(target)
		}
		if (shown == target) return AvailabilityAction.None
		shown = target
		return AvailabilityAction.Change(target)
	}

	/** The system stopped the service, or it could not be started. */
	fun stoppedBySystem() {
		running = false
		shown = Availability.OFF
	}
}

/** Which networks count as "the LAN": the ones a peer on the same network can be reached over. */
object Lan {
	enum class Transport { WIFI, ETHERNET, CELLULAR, VPN, BLUETOOTH, OTHER }

	/** A VPN alone is not a LAN; Wi-Fi or Ethernet under it is. */
	fun isLan(transports: Set<Transport>): Boolean =
		Transport.WIFI in transports || Transport.ETHERNET in transports

	/** Any of the device's networks is a LAN. */
	fun anyLan(networks: Collection<Set<Transport>>): Boolean = networks.any(::isLan)
}
