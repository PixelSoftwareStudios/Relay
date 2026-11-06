package dev.relay.app

import java.io.File
import java.io.InputStream

/** Result of reading a picked document, with no Android types. */
sealed interface PickedDocument {
    data class Image(val name: String, val bytes: ByteArray) : PickedDocument
    data class Copied(val path: String) : PickedDocument
    data class Refused(val message: String) : PickedDocument
}

/** The size and name rules for documents, testable on the JVM. */
object Documents {
    /** The same limit as the UI's `AVATAR_MAX_BYTES`. */
    const val IMAGE_MAX_BYTES = 1024 * 1024

    fun safeName(raw: String?): String {
        val base = (raw ?: "file").substringAfterLast('/').substringAfterLast('\\')
        val cleaned = base.map { if (it.isLetterOrDigit() || it in "._- ") it else '_' }
            .joinToString("").trim().trimStart('.')
        return cleaned.take(100).ifEmpty { "file" }
    }

    /** Reads an image after checking [size] (when known) against the limit. */
    fun readImage(name: String?, size: Long?, open: () -> InputStream?): PickedDocument {
        if (size != null && (size <= 0 || size > IMAGE_MAX_BYTES)) {
            return PickedDocument.Refused("That image is larger than 1 MiB or empty.")
        }
        val stream = open() ?: return PickedDocument.Refused("Could not open that image.")
        stream.use {
            val bytes = it.readNBytes(IMAGE_MAX_BYTES + 1)
            if (bytes.isEmpty() || bytes.size > IMAGE_MAX_BYTES) {
                return PickedDocument.Refused("That image is larger than 1 MiB or empty.")
            }
            return PickedDocument.Image(safeName(name), bytes)
        }
    }

    /** Copies a picked file into [dir] so the engine can open it by path. */
    fun copyFile(name: String?, dir: File, open: () -> InputStream?): PickedDocument {
        val stream = open() ?: return PickedDocument.Refused("Could not open that file.")
        dir.mkdirs()
        val target = File(dir, "${System.nanoTime()}-${safeName(name)}")
        stream.use { input -> target.outputStream().use { input.copyTo(it) } }
        return PickedDocument.Copied(target.path)
    }
}
