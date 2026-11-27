package app.renanclip

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Build
import android.os.Bundle
import android.text.Editable
import android.text.InputType
import android.text.TextUtils
import android.text.TextWatcher
import android.util.LruCache
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.InputMethodManager
import android.widget.BaseAdapter
import android.widget.EditText
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.TextView
import app.renanclip.core.Event
import app.renanclip.core.Item

class MainActivity : Activity() {
    private lateinit var p: Pal
    private lateinit var list: ListView
    private lateinit var empty: TextView
    private lateinit var status: TextView
    private lateinit var search: EditText
    private val tabs = ArrayList<TextView>()
    private var kind: String? = null
    private var items: List<Item> = emptyList()
    private var names: Map<String, String> = emptyMap()
    private val thumbs = object : LruCache<String, Bitmap>(24) {}
    private val listener: (Event) -> Unit = { if (it is Event.History || it is Event.Peers) refresh() }

    override fun onCreate(b: Bundle?) {
        Rc.ensure(this)
        p = Ui.applyTheme(this)
        super.onCreate(b)
        ClipService.start(this)
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED)
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
        build()
    }

    private fun build() {
        val (root, content) = Ui.screen(this, p)
        val head = Ui.header(this, p, "RenanClip", false)
        status = Ui.label(this, "", 12f, p.sub)
        head.addView(status)
        head.addView(Ui.button(this, p, "Aparelhos") { startActivity(Intent(this, PeersActivity::class.java)) }.apply { (layoutParams as? LinearLayout.LayoutParams)?.leftMargin = Ui.dp(context, 8) })
        head.addView(Ui.button(this, p, "Ajustes") { startActivity(Intent(this, SettingsActivity::class.java)) })
        content.addView(head)

        search = EditText(this).apply {
            hint = "Buscar (sem acento)"; setHintTextColor(p.sub); setTextColor(p.text); setSingleLine(); inputType = InputType.TYPE_CLASS_TEXT
            background = Ui.rounded(p.surface, 10, context, p.line); setPadding(Ui.dp(context, 14), 0, Ui.dp(context, 14), 0)
            addTextChangedListener(object : TextWatcher {
                override fun beforeTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) {}
                override fun onTextChanged(s: CharSequence?, a: Int, b: Int, c: Int) { refresh() }
                override fun afterTextChanged(s: Editable?) {}
            })
        }
        content.addView(search, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Ui.dp(this, 44)).apply { setMargins(Ui.dp(this@MainActivity, 16), 0, Ui.dp(this@MainActivity, 16), Ui.dp(this@MainActivity, 8)) })

        val row = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; setPadding(Ui.dp(context, 12), 0, Ui.dp(context, 12), Ui.dp(context, 4)) }
        for ((label, k) in listOf("Tudo" to null, "Texto" to "text", "Imagem" to "image", "Arquivo" to "file")) {
            val t = Ui.label(this, label, 13f, p.text).apply {
                gravity = Gravity.CENTER; setPadding(Ui.dp(context, 14), Ui.dp(context, 8), Ui.dp(context, 14), Ui.dp(context, 8))
                setOnClickListener { kind = k; styleTabs(); refresh() }
            }
            tabs.add(t)
            row.addView(t, LinearLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { rightMargin = Ui.dp(this@MainActivity, 6) })
        }
        content.addView(row)
        styleTabs()

        list = ListView(this).apply { divider = null; dividerHeight = 0; clipToPadding = false; setPadding(Ui.dp(context, 12), 0, Ui.dp(context, 12), Ui.dp(context, 8)) }
        empty = Ui.label(this, "", 14f, p.sub).apply { gravity = Gravity.CENTER; setPadding(Ui.dp(context, 24), Ui.dp(context, 48), Ui.dp(context, 24), 0) }
        content.addView(empty, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        content.addView(list, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        list.adapter = adapter
        list.setOnItemClickListener { _, _, i, _ -> items.getOrNull(i)?.let { copy(it, false) } }
        list.setOnItemLongClickListener { _, _, i, _ -> items.getOrNull(i)?.let { menu(it) }; true }

        val foot = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL; setPadding(Ui.dp(context, 16), 0, Ui.dp(context, 16), Ui.dp(context, 4)) }
        foot.addView(Ui.label(this, "Toque para copiar · segure para mais opções", 11f, p.sub), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
        foot.addView(Ui.label(this, "Limpar histórico", 12f, p.danger).apply {
            setPadding(Ui.dp(context, 8), Ui.dp(context, 10), Ui.dp(context, 8), Ui.dp(context, 10))
            setOnClickListener { confirmClear() }
        })
        content.addView(foot)
        setContentView(root)
    }

    private fun styleTabs() {
        val keys = listOf(null, "text", "image", "file")
        tabs.forEachIndexed { i, t ->
            val on = keys[i] == kind
            t.setTextColor(if (on) p.onAccent else p.text)
            t.background = Ui.rounded(if (on) p.accent else p.surface, 16, this, if (on) 0 else p.line)
        }
    }

    override fun onStart() { super.onStart(); Rc.listen(listener); refresh() }
    override fun onStop() { Rc.unlisten(listener); super.onStop() }

    private fun refresh() {
        val e = Rc.engine
        names = e.peers().associate { it.id to it.name }
        items = e.history(search.text.toString(), kind, 300)
        val n = e.onlineCount()
        status.text = if (n == 0) "offline" else "$n online"
        status.setTextColor(if (n == 0) p.sub else p.accent)
        empty.visibility = if (items.isEmpty()) View.VISIBLE else View.GONE
        empty.text = if (search.text.isNotEmpty() || kind != null) "Nada encontrado." else "Nada copiado ainda.\nO que você copiar nos aparelhos pareados aparece aqui."
        adapter.notifyDataSetChanged()
    }

    private fun copy(it: Item, sensitive: Boolean) {
        Thread {
            val pl = Rc.engine.payload(it.id)
            Rc.main.post {
                if (pl == null) { Rc.toast(this, "Conteúdo indisponível."); return@post }
                Rc.apply(this, it.id, pl, sensitive)
                Rc.toast(this, if (sensitive) "Copiado como sensível." else "Copiado.")
            }
        }.start()
    }

    private fun menu(it: Item) {
        val opts = arrayOf("Copiar", "Copiar como sensível (não vai para o histórico do teclado)", if (it.pinned) "Desafixar" else "Fixar", "Apagar")
        AlertDialog.Builder(this).setItems(opts) { _, w ->
            when (w) { 0 -> copy(it, false); 1 -> copy(it, true); 2 -> Rc.engine.pin(it.id, !it.pinned); 3 -> Rc.engine.delete(it.id) }
        }.show()
    }

    private fun confirmClear() {
        AlertDialog.Builder(this).setMessage("Apagar todo o histórico não fixado, neste e nos aparelhos conectados?")
            .setPositiveButton("Apagar") { _, _ -> Rc.engine.clear() }.setNegativeButton("Cancelar", null).show()
    }

    private val adapter = object : BaseAdapter() {
        override fun getCount() = items.size
        override fun getItem(i: Int) = items[i]
        override fun getItemId(i: Int) = i.toLong()
        override fun getView(i: Int, v: View?, parent: ViewGroup): View {
            val it = items[i]
            val ctx = this@MainActivity
            val card = LinearLayout(ctx).apply {
                orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL
                background = Ui.rounded(p.surface, 12, ctx, p.line); setPadding(Ui.dp(ctx, 14), Ui.dp(ctx, 12), Ui.dp(ctx, 6), Ui.dp(ctx, 12))
                layoutParams = AbsListViewLP()
            }
            val col = LinearLayout(ctx).apply { orientation = LinearLayout.VERTICAL }
            when (it.kind) {
                "image" -> {
                    val iv = ImageView(ctx).apply { scaleType = ImageView.ScaleType.CENTER_CROP; background = Ui.rounded(p.line, 8, ctx); clipToOutline = true }
                    thumb(it)?.let { bmp -> iv.setImageBitmap(bmp) }
                    col.addView(iv, LinearLayout.LayoutParams(Ui.dp(ctx, 96), Ui.dp(ctx, 72)))
                }
                "file" -> col.addView(Ui.label(ctx, it.name.ifEmpty { "arquivo" }, 15f, p.text, true).apply { maxLines = 2; ellipsize = TextUtils.TruncateAt.MIDDLE })
                else -> col.addView(Ui.label(ctx, (it.text ?: "").trim(), 15f, p.text).apply { maxLines = 3; ellipsize = TextUtils.TruncateAt.END })
            }
            val who = if (it.origin == Rc.engine.myHex) "este aparelho" else names[it.origin] ?: "outro aparelho"
            val sz = if (it.kind == "text") "" else " · ${Rc.size(it.size)}"
            col.addView(Ui.label(ctx, "${Rc.time(it.ts)} · $who$sz", 11f, p.sub).apply { setPadding(0, Ui.dp(ctx, 4), 0, 0) })
            card.addView(col, LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            card.addView(Ui.label(ctx, if (it.pinned) "★" else "☆", 20f, if (it.pinned) p.accent else p.sub).apply {
                contentDescription = if (it.pinned) "Desafixar" else "Fixar"; gravity = Gravity.CENTER
                setPadding(Ui.dp(ctx, 10), Ui.dp(ctx, 8), Ui.dp(ctx, 10), Ui.dp(ctx, 8))
                setOnClickListener { _ -> Rc.engine.pin(it.id, !it.pinned) }
            })
            card.addView(Ui.label(ctx, "✕", 16f, p.sub).apply {
                contentDescription = "Apagar"; gravity = Gravity.CENTER
                setPadding(Ui.dp(ctx, 10), Ui.dp(ctx, 8), Ui.dp(ctx, 10), Ui.dp(ctx, 8))
                setOnClickListener { _ -> Rc.engine.delete(it.id) }
            })
            // espaço entre cartões
            return LinearLayout(ctx).apply {
                setPadding(0, Ui.dp(ctx, 4), 0, Ui.dp(ctx, 4)); addView(card, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
            }
        }
    }

    private fun AbsListViewLP() = LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

    private fun thumb(it: Item): Bitmap? {
        thumbs.get(it.id)?.let { return it }
        val bytes = Rc.engine.payload(it.id).let { (it as? app.renanclip.core.Payload.Image)?.png } ?: return null
        val o = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, o)
        var s = 1
        while (o.outWidth / (s * 2) >= 192 && o.outHeight / (s * 2) >= 144) s *= 2
        val bmp = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = s }) ?: return null
        thumbs.put(it.id, bmp)
        return bmp
    }
}
