package dev.relay.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.ByteArrayInputStream
import java.io.File

class DocumentsTest {
	@get:Rule
	val tmp = TemporaryFolder()

	@Test
	fun namesCannotEscapeTheCacheOrHideThemselves() {
		assertEquals("passwd", Documents.safeName("../../etc/passwd"))
		assertEquals("evil.txt", Documents.safeName("..\\..\\evil.txt"))
		assertEquals("hidden", Documents.safeName(".hidden"))
		assertEquals("file", Documents.safeName(null))
		assertEquals("a_b.png", Documents.safeName("a\u0000b.png"))
	}

	@Test
	fun anImageWithinTheLimitIsRead() {
		val bytes = ByteArray(1000) { 7 }
		val picked = Documents.readImage("me.png", 1000L) { ByteArrayInputStream(bytes) }
		assertTrue(picked is PickedDocument.Image && picked.bytes.size == 1000 && picked.name == "me.png")
	}

	@Test
	fun anImageOverOneMebibyteIsRefusedBeforeReading() {
		var opened = false
		val picked = Documents.readImage("big.png", Documents.IMAGE_MAX_BYTES + 1L) {
			opened = true
			ByteArrayInputStream(ByteArray(0))
		}
		assertTrue(picked is PickedDocument.Refused)
		assertEquals(false, opened)
	}

	@Test
	fun anUnknownSizeIsStillCapped() {
		val huge = ByteArray(Documents.IMAGE_MAX_BYTES + 10)
		val picked = Documents.readImage("big.png", null) { ByteArrayInputStream(huge) }
		assertTrue(picked is PickedDocument.Refused)
	}

	@Test
	fun filesAreCopiedUnderTheCacheDirectory() {
		val dir = tmp.newFolder("picked")
		val picked = Documents.copyFile("../notes.txt", dir) { ByteArrayInputStream("hi".toByteArray()) }
		val path = (picked as PickedDocument.Copied).path
		assertEquals(dir.canonicalPath, File(path).parentFile!!.canonicalPath)
		assertEquals("hi", File(path).readText())
	}
}
