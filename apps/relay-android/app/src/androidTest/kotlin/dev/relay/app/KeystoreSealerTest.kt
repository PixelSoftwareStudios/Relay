package dev.relay.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.After
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import org.junit.runner.RunWith
import java.security.GeneralSecurityException

/** Runs on a device or emulator: the real Android Keystore. */
@RunWith(AndroidJUnit4::class)
class KeystoreSealerTest {
    private val alias = "relay-test-key"
    private val sealer = Sealer { KeystoreKey.get(alias) }

    @After
    fun cleanUp() = KeystoreKey.delete(alias)

    @Test
    fun theKeystoreKeyRoundTripsAndIsNotExportable() {
        val secret = ByteArray(64) { (it * 3).toByte() }
        val sealed = sealer.seal("identity", secret)
        assertArrayEquals(secret, sealer.open("identity", sealed))
        // Non-exportable: the key object has no encoded form.
        assertEquals(null, KeystoreKey.get(alias).encoded)
    }

    @Test
    fun theSameKeyIsFoundAgainAfterItWasCreated() {
        val sealed = sealer.seal("channel/x/signing", byteArrayOf(1, 2, 3))
        val again = Sealer { KeystoreKey.get(alias) }
        assertArrayEquals(byteArrayOf(1, 2, 3), again.open("channel/x/signing", sealed))
    }

    @Test
    fun slotBindingHoldsOnTheRealKeystore() {
        val sealed = sealer.seal("channel/x/signing", byteArrayOf(1))
        assertThrows(GeneralSecurityException::class.java) { sealer.open("channel/x/password", sealed) }
    }

    @Test
    fun aDeletedKeyCannotOpenOldValues() {
        val sealed = sealer.seal("identity", ByteArray(8))
        KeystoreKey.delete(alias)
        assertThrows(GeneralSecurityException::class.java) { sealer.open("identity", sealed) }
    }
}
