package dev.relay.app

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test
import java.security.GeneralSecurityException
import javax.crypto.KeyGenerator

class SealerTest {
	private val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
	private val sealer = Sealer { key }

	@Test
	fun roundTripsAndHidesThePlaintext() {
		val secret = ByteArray(64) { it.toByte() }
		val sealed = sealer.seal("identity", secret)
		assertEquals(Sealer.VERSION, sealed[0])
		assertEquals(1 + 12 + 64 + 16, sealed.size)
		assertFalse(String(sealed, Charsets.ISO_8859_1).contains(String(secret.copyOfRange(0, 16), Charsets.ISO_8859_1)))
		assertArrayEquals(secret, sealer.open("identity", sealed))
	}

	@Test
	fun everySealUsesAFreshIv() {
		val a = sealer.seal("x", byteArrayOf(1, 2, 3))
		val b = sealer.seal("x", byteArrayOf(1, 2, 3))
		assertFalse(a.contentEquals(b))
	}

	@Test
	fun aValueDoesNotOpenInAnotherSlot() {
		val sealed = sealer.seal("channel/aa/signing", byteArrayOf(9, 9))
		assertThrows(GeneralSecurityException::class.java) { sealer.open("channel/aa/password", sealed) }
	}

	@Test
	fun tamperingAndTruncationAreRejected() {
		val sealed = sealer.seal("identity", ByteArray(32))
		val flipped = sealed.copyOf().also { it[20] = (it[20].toInt() xor 1).toByte() }
		assertThrows(GeneralSecurityException::class.java) { sealer.open("identity", flipped) }
		assertThrows(GeneralSecurityException::class.java) { sealer.open("identity", sealed.copyOf(10)) }
		val wrongVersion = sealed.copyOf().also { it[0] = 9 }
		assertThrows(GeneralSecurityException::class.java) { sealer.open("identity", wrongVersion) }
	}

	@Test
	fun anotherKeyCannotOpenIt() {
		val other = Sealer { KeyGenerator.getInstance("AES").apply { init(256) }.generateKey() }
		val sealed = sealer.seal("identity", ByteArray(32))
		assertThrows(GeneralSecurityException::class.java) { other.open("identity", sealed) }
	}
}
