package app.renanclip

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.widget.Toast
import app.renanclip.core.Engine
import app.renanclip.core.Event
import app.renanclip.core.Payload
import app.renanclip.core.Proto
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import java.util.concurrent.CopyOnWriteArrayList

/** Runtime do app: um Engine + um SqliteStore por processo; telas e serviço compartilham. */
object Rc {
    const val SENSITIVE = "android.content.extra.IS_SENSITIVE"

    val main = Handler(Looper.getMainLooper())
    private lateinit var app: Context
    private var store: SqliteStore? = null
    @Volatile private var eng: Engine? = null
    private val listeners = CopyOnWriteArrayList<(Event) -> Unit>()

    val engine: Engine get() = eng ?: ensure(app) // depois de "Parar", reabrir a tela religa o motor

    @Synchronized fun ensure(ctx: Context): Engine {
        app = ctx.applicationContext
        eng?.let { return it }
        val st = store ?: SqliteStore(app).also { store = it }
        val e = Engine(st, defaultName(), Proto.DEFAULT_PORT) { ev -> dispatch(ev) }
        e.start()
        eng = e
        return e
    }

    /** Para o motor (serviço encerrado). A próxima tela/serviço cria outro. */
    @Synchronized fun shutdown() {
        eng?.shutdown()
        eng = null
    }

    fun listen(l: (Event) -> Unit) { listeners.add(l) }
    fun unlisten(l: (Event) -> Unit) { listeners.remove(l) }

    private fun dispatch(ev: Event) {
        main.post {
            if (ev is Event.Apply) apply(app, ev.id, ev.payload)
            for (l in listeners) runCatching { l(ev) }
        }
    }

    private fun defaultName(): String {
        val m = Build.MODEL?.trim().orEmpty()
        return if (m.isEmpty()) "Android" else m.take(40)
    }

    // ---------- área de transferência ----------
    /** Coloca o conteúdo na área de transferência (sempre na thread principal). `sensitive` marca a cópia como sensível. */
    fun apply(ctx: Context, id: String, p: Payload, sensitive: Boolean = false) {
        val cm = ctx.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = when (p) {
            is Payload.Text -> ClipData.newPlainText("RenanClip", p.text)
            is Payload.Image -> ClipData.newUri(ctx.contentResolver, "RenanClip", shareFile(ctx, id, "imagem.png", p.png))
            is Payload.File -> ClipData(ClipDescription(p.name, arrayOf(p.mime.ifEmpty { "application/octet-stream" })), ClipData.Item(shareFile(ctx, id, p.name, p.bytes)))
        }
        if (sensitive) clip.description.extras = PersistableBundle().apply { putBoolean(SENSITIVE, true) }
        eng?.markApplied(id)
        cm.setPrimaryClip(clip)
    }

    /** Grava o conteúdo em cache/clip/<id>/<nome> e devolve o URI do FileProvider. */
    private fun shareFile(ctx: Context, id: String, name: String, data: ByteArray): android.net.Uri {
        val root = File(ctx.cacheDir, "clip")
        val dirName = id.filter { it.isLetterOrDigit() }
        val dir = File(root, dirName)
        // mantém só a última dúzia de itens no cache
        root.listFiles()?.sortedByDescending { it.lastModified() }?.drop(12)?.forEach { it.deleteRecursively() }
        dir.mkdirs()
        val safe = name.replace(Regex("[\\\\/:*?\"<>|\\u0000-\\u001f]"), "_").take(120).trim().let { if (it.isEmpty() || it.all { c -> c == '.' }) "arquivo" else it }
        val f = File(dir, safe)
        f.writeBytes(data)
        return android.net.Uri.Builder().scheme("content").authority("${ctx.packageName}.files").appendPath(dirName).appendPath(safe).build()
    }

    fun toast(ctx: Context, msg: String) {
        main.post { Toast.makeText(ctx.applicationContext, msg, Toast.LENGTH_SHORT).show() }
    }

    // ---------- formatação ----------
    private val tz = TimeZone.getTimeZone("America/Sao_Paulo")
    fun time(ts: Long): String = SimpleDateFormat("dd/MM HH:mm", Locale("pt", "BR")).also { it.timeZone = tz }.format(Date(ts))

    fun size(n: Long): String = when {
        n >= 1 shl 20 -> String.format(Locale("pt", "BR"), "%.1f MB", n / 1048576.0)
        n >= 1024 -> "${n / 1024} KB"
        else -> "$n B"
    }
}
