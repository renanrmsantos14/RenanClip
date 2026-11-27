package app.renanclip

import android.content.ClipData
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import app.renanclip.core.LocalCopy
import java.io.ByteArrayOutputStream

/** Converte área de transferência, ACTION_SEND e URIs em [LocalCopy] (sempre fora da thread principal). */
object Intake {
    class Result(val copy: LocalCopy?, val error: String? = null)

    /** Texto, imagem (convertida para PNG) ou arquivo do item 0 do clipboard. */
    fun fromClip(ctx: Context, clip: ClipData?, capBytes: Int): Result {
        if (clip == null || clip.itemCount == 0) return Result(null, "A área de transferência está vazia.")
        if (clip.description?.extras?.getBoolean(Rc.SENSITIVE, false) == true) return Result(null, "Cópia marcada como sensível: não enviada.")
        val item = clip.getItemAt(0)
        val uri = item.uri
        if (uri != null) return fromUri(ctx, uri, clip.description?.getMimeType(0), capBytes)
        val t = item.coerceToText(ctx)?.toString().orEmpty()
        if (t.isBlank()) return Result(null, "A área de transferência está vazia.")
        return Result(LocalCopy.text(t))
    }

    fun fromUri(ctx: Context, uri: Uri, mimeHint: String?, capBytes: Int): Result {
        val cr = ctx.contentResolver
        val mime = (cr.getType(uri) ?: mimeHint ?: "application/octet-stream").lowercase()
        var name = ""
        runCatching {
            cr.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { if (it.moveToFirst()) name = it.getString(0) ?: "" }
        }
        if (name.isEmpty()) name = uri.lastPathSegment?.substringAfterLast('/') ?: "arquivo"
        val bytes = try {
            cr.openInputStream(uri)?.use { readCapped(it, capBytes) } ?: return Result(null, "Não consegui abrir o conteúdo.")
        } catch (e: Exception) { return Result(null, "Não consegui ler o conteúdo.") }
            ?: return Result(null, "Grande demais (limite de ${capBytes / 1048576} MB).")
        if (bytes.isEmpty()) return Result(null, "O conteúdo está vazio.")
        return when {
            mime == "image/png" -> Result(LocalCopy.image(bytes))
            mime.startsWith("image/") -> {
                val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)
                if (bmp == null) Result(LocalCopy.file(name, mime, bytes)) else {
                    val o = ByteArrayOutputStream()
                    bmp.compress(Bitmap.CompressFormat.PNG, 100, o)
                    bmp.recycle()
                    val png = o.toByteArray()
                    if (png.size > capBytes) Result(null, "Grande demais (limite de ${capBytes / 1048576} MB).") else Result(LocalCopy.image(png))
                }
            }
            mime.startsWith("text/") && bytes.size <= 1 shl 20 && name.substringAfterLast('.', "") in setOf("", "txt") ->
                Result(LocalCopy.text(String(bytes, Charsets.UTF_8)))
            else -> Result(LocalCopy.file(name, mime, bytes))
        }
    }

    /** Lê até `cap` bytes; null se passar do limite. */
    private fun readCapped(i: java.io.InputStream, cap: Int): ByteArray? {
        val o = ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        while (true) {
            val n = i.read(buf)
            if (n < 0) break
            if (o.size() + n > cap) return null
            o.write(buf, 0, n)
        }
        return o.toByteArray()
    }
}
