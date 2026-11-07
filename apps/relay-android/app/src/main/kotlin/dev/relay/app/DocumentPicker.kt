package dev.relay.app

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import uniffi.relay_android.PickKind
import uniffi.relay_android.PickOutcome
import java.io.File
import java.util.concurrent.Executors

/**
 * The system document picker (Storage Access Framework) for the profile image,
 * the ringtone and file offers. The picker returns content URIs, never paths: images are
 * read here after a size check, files are copied into the app cache and the
 * engine reads the copy.
 */
class DocumentPicker(private val activity: AppCompatActivity) {
	private var launcher: ActivityResultLauncher<Array<String>>? = null
	private var request: Pair<ULong, PickKind>? = null
	private val io = Executors.newSingleThreadExecutor()
	private val cache = File(activity.cacheDir, "picked")

	/** Must run in `onCreate`, before the activity is started. */
	fun register() {
		cache.deleteRecursively()
		launcher = activity.registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
			val (id, kind) = request ?: return@registerForActivityResult
			request = null
			if (uri == null) {
				complete(id, PickOutcome.Cancelled)
			} else {
				io.execute { complete(id, read(uri, kind)) }
			}
		}
	}

	fun launch(id: ULong, kind: PickKind) {
		if (request != null) {
			complete(id, PickOutcome.Failed("Another file picker is already open."))
			return
		}
		request = id to kind
		val types = when (kind) {
			PickKind.IMAGE -> arrayOf("image/*")
			PickKind.AUDIO -> arrayOf("audio/*", "application/ogg")
			PickKind.FILE -> arrayOf("*/*")
		}
		launcher?.launch(types)
			?: complete(id, PickOutcome.Failed("The file picker is not ready."))
	}

	private fun read(uri: Uri, kind: PickKind): PickOutcome {
		val resolver = activity.contentResolver
		val (name, size) = resolver.query(uri, null, null, null, null)?.use { c ->
			if (!c.moveToFirst()) return@use null to null
			val n = c.getColumnIndex(OpenableColumns.DISPLAY_NAME).takeIf { it >= 0 }?.let(c::getString)
			val s = c.getColumnIndex(OpenableColumns.SIZE).takeIf { it >= 0 && !c.isNull(it) }?.let(c::getLong)
			n to s
		} ?: (null to null)
		val picked = try {
			when (kind) {
				PickKind.IMAGE -> Documents.readImage(name, size) { resolver.openInputStream(uri) }
				PickKind.AUDIO -> Documents.readAudio(name, size) { resolver.openInputStream(uri) }
				PickKind.FILE -> Documents.copyFile(name, cache) { resolver.openInputStream(uri) }
			}
		} catch (e: java.io.IOException) {
			PickedDocument.Refused("Could not read that file: ${e.message}")
		}
		return when (picked) {
			is PickedDocument.Image -> PickOutcome.Image(picked.name, picked.bytes)
			is PickedDocument.Audio -> PickOutcome.Audio(picked.name, picked.bytes)
			is PickedDocument.Copied -> PickOutcome.File(picked.path)
			is PickedDocument.Refused -> PickOutcome.Failed(picked.message)
		}
	}

	private fun complete(id: ULong, outcome: PickOutcome) = RelayRuntime.whenReady { it.completePick(id, outcome) }
}
