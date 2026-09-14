package dev.sanctum.app

import android.app.NativeActivity
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.io.FileOutputStream

class SlintActivity : NativeActivity() {
    companion object {
        init {
            System.loadLibrary("sanctum")
        }

        const val REQ_FOLDER = 1
        const val REQ_FILE = 2
        private const val MAX_COPY_BYTES = 512L * 1024L * 1024L
    }

    private external fun nativeOnPicked(requestCode: Int, path: String?)

    @Suppress("unused")
    fun pickFolder() {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).apply {
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        startActivityForResult(intent, REQ_FOLDER)
    }

    @Suppress("unused")
    fun pickFile(mime: String) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = mime.ifBlank { "*/*" }
            addFlags(
                Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
            )
        }
        startActivityForResult(intent, REQ_FILE)
    }

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (resultCode != RESULT_OK || data?.data == null) {
            nativeOnPicked(requestCode, null)
            return
        }
        val uri = data.data!!
        val flags = data.flags and
            (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
        try {
            contentResolver.takePersistableUriPermission(uri, flags)
        } catch (_: SecurityException) {
        }
        Thread {
            val copied = runCatching {
                when (requestCode) {
                    REQ_FOLDER -> copyTree(uri)
                    REQ_FILE -> copyFile(uri)
                    else -> null
                }
            }.getOrNull()
            runOnUiThread { nativeOnPicked(requestCode, copied) }
        }.start()
    }

    private fun copyTree(treeUri: Uri): String? {
        val root = DocumentFile.fromTreeUri(this, treeUri) ?: return null
        val name = safeName(root.name ?: "folder")
        val dest = uniqueDir(File(File(filesDir, "imports"), name))
        val copied = copyTreeInto(root, dest, 0)
        if (copied < 0) {
            dest.deleteRecursively()
            return null
        }
        return dest.absolutePath
    }

    private fun copyTreeInto(src: DocumentFile, dest: File, startBytes: Long): Long {
        dest.mkdirs()
        var total = startBytes
        for (child in src.listFiles()) {
            val childName = safeName(child.name ?: continue)
            if (child.isDirectory) {
                total = copyTreeInto(child, File(dest, childName), total)
                if (total < 0) return -1
            } else if (child.isFile) {
                val out = File(dest, childName)
                total = copyUriToFile(child.uri, out, total)
                if (total < 0) return -1
            }
        }
        return total
    }

    private fun copyFile(uri: Uri): String? {
        val name = safeName(queryDisplayName(uri) ?: "import.bin")
        val dest = uniqueFile(File(File(filesDir, "imports"), name))
        dest.parentFile?.mkdirs()
        val total = copyUriToFile(uri, dest, 0)
        if (total < 0) {
            dest.delete()
            return null
        }
        return dest.absolutePath
    }

    private fun copyUriToFile(uri: Uri, dest: File, startBytes: Long): Long {
        val input = contentResolver.openInputStream(uri) ?: return -1
        input.use { stream ->
            FileOutputStream(dest).use { output ->
                val buffer = ByteArray(64 * 1024)
                var total = startBytes
                while (true) {
                    val read = stream.read(buffer)
                    if (read <= 0) break
                    total += read
                    if (total > MAX_COPY_BYTES) return -1
                    output.write(buffer, 0, read)
                }
                return total
            }
        }
    }

    private fun queryDisplayName(uri: Uri): String? {
        val cursor = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        cursor?.use {
            if (it.moveToFirst()) {
                val index = it.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                if (index >= 0) return it.getString(index)
            }
        }
        return uri.lastPathSegment
    }

    private fun safeName(name: String): String {
        val cleaned = name.replace(Regex("[^A-Za-z0-9._-]"), "_").trim('_')
        return cleaned.ifBlank { "import" }
    }

    private fun uniqueDir(base: File): File {
        if (!base.exists()) return base
        var index = 2
        while (true) {
            val candidate = File(base.parentFile, "${base.name}-$index")
            if (!candidate.exists()) return candidate
            index += 1
        }
    }

    private fun uniqueFile(base: File): File {
        if (!base.exists()) return base
        val stem = base.nameWithoutExtension
        val ext = base.extension
        var index = 2
        while (true) {
            val candidate = File(
                base.parentFile,
                if (ext.isEmpty()) "$stem-$index" else "$stem-$index.$ext",
            )
            if (!candidate.exists()) return candidate
            index += 1
        }
    }
}
