package app.renanclip

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import java.io.File
import java.io.FileNotFoundException

/**
 * Serve só leitura os arquivos de cache/clip/<id>/<nome> para quem colar imagem ou arquivo
 * (substitui o FileProvider do androidx para não depender de bibliotecas).
 */
class ClipFiles : ContentProvider() {
    private fun file(uri: Uri): File {
        val root = File(context!!.cacheDir, "clip").canonicalFile
        val seg = uri.pathSegments
        if (seg.size != 2) throw FileNotFoundException()
        val f = File(File(root, seg[0]), seg[1]).canonicalFile
        if (f.parentFile?.parentFile != root || !f.isFile) throw FileNotFoundException()
        return f
    }

    override fun onCreate() = true
    override fun getType(uri: Uri): String? {
        val ext = MimeTypeMap.getFileExtensionFromUrl(uri.lastPathSegment?.replace(" ", "_") ?: "")
        return MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext.lowercase()) ?: "application/octet-stream"
    }

    override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
        if (mode != "r") throw SecurityException("somente leitura")
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY)
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor {
        val f = file(uri)
        val cols = projection ?: arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE)
        val c = MatrixCursor(cols)
        c.addRow(Array<Any?>(cols.size) { i -> if (cols[i] == OpenableColumns.SIZE) f.length() else f.name })
        return c
    }

    override fun insert(uri: Uri, values: ContentValues?): Uri? = throw UnsupportedOperationException()
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?) = 0
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?) = 0
}
