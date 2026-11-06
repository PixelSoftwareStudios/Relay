package dev.relay.app

/** Something that is held and released, like a Wi-Fi lock. */
interface Releasable {
	fun acquire()
	fun release()
}

/**
 * Holds [lock] while at least one owner wants it. The activity and the call
 * service both need the Wi-Fi locks; neither may release them under the other.
 */
class HoldSet(private val lock: Releasable) {
	private val owners = mutableSetOf<String>()

	@Synchronized
	fun hold(owner: String) {
		if (owners.add(owner) && owners.size == 1) lock.acquire()
	}

	@Synchronized
	fun drop(owner: String) {
		if (owners.remove(owner) && owners.isEmpty()) lock.release()
	}

	@get:Synchronized
	val held: Boolean get() = owners.isNotEmpty()
}
