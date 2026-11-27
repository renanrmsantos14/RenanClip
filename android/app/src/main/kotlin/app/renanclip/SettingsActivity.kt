package app.renanclip

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.provider.Settings as Sys
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.View
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.RadioButton
import android.widget.RadioGroup
import android.widget.ScrollView
import android.widget.Switch
import app.renanclip.core.Settings

class SettingsActivity : Activity() {
    private lateinit var p: Pal
    private var s: Settings = Settings()

    override fun onCreate(b: Bundle?) {
        val e = Rc.ensure(this)
        p = Ui.applyTheme(this)
        super.onCreate(b)
        s = e.settings()
        val (root, content) = Ui.screen(this, p)
        content.addView(Ui.header(this, p, "Ajustes", true))
        val sv = ScrollView(this)
        val body = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(Ui.dp(context, 16), 0, Ui.dp(context, 16), Ui.dp(context, 24)) }
        sv.addView(body)
        content.addView(sv, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))

        fun title(t: String) = body.addView(Ui.label(this, t, 13f, p.sub, true).apply { setPadding(0, Ui.dp(context, 20), 0, Ui.dp(context, 6)) })
        fun note(t: String) = body.addView(Ui.label(this, t, 12f, p.sub).apply { setPadding(0, Ui.dp(context, 4), 0, 0) })
        fun field(initial: String, numeric: Boolean, on: (String) -> Unit) {
            val et = EditText(this).apply {
                setText(initial); setSingleLine(); setTextColor(p.text)
                inputType = if (numeric) InputType.TYPE_CLASS_NUMBER else InputType.TYPE_CLASS_TEXT
                background = Ui.rounded(p.surface, 10, context, p.line); setPadding(Ui.dp(context, 14), 0, Ui.dp(context, 14), 0)
                addTextChangedListener(object : TextWatcher {
                    override fun beforeTextChanged(a: CharSequence?, x: Int, y: Int, z: Int) {}
                    override fun onTextChanged(a: CharSequence?, x: Int, y: Int, z: Int) {}
                    override fun afterTextChanged(a: Editable?) { on(a.toString()) }
                })
            }
            body.addView(et, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Ui.dp(this, 44)))
        }
        fun toggle(label: String, on: Boolean, change: (Boolean) -> Unit) = body.addView(Switch(this).apply {
            text = label; isChecked = on; setTextColor(p.text); setPadding(0, Ui.dp(context, 10), 0, Ui.dp(context, 10))
            setOnCheckedChangeListener { _, v -> change(v) }
        })

        title("ESTE APARELHO")
        field(s.deviceName, false) { v -> if (v.isNotBlank()) save(Settings(v.trim(), s.retentionDays, s.paused, s.syncImages, s.syncFiles, s.maxBlobMb, s.storeCapMb, s.theme)) }
        note("Nome mostrado aos outros aparelhos (vale na próxima conexão).")

        title("HISTÓRICO")
        field(s.retentionDays.toString(), true) { v -> v.toIntOrNull()?.let { save(copy(retention = it)) } }
        note("Dias que o histórico é guardado (padrão 7). Itens fixados não expiram.")

        title("SINCRONIZAÇÃO")
        toggle("Pausar sincronização", s.paused) { save(copy(paused = it)) }
        toggle("Sincronizar imagens", s.syncImages) { save(copy(images = it)) }
        toggle("Sincronizar arquivos", s.syncFiles) { save(copy(files = it)) }
        body.addView(Ui.label(this, "Tamanho máximo de imagem/arquivo (MB)", 14f, p.text).apply { setPadding(0, Ui.dp(context, 10), 0, Ui.dp(context, 6)) })
        field(s.maxBlobMb.toString(), true) { v -> v.toIntOrNull()?.let { save(copy(maxMb = it)) } }

        title("TEMA")
        val g = RadioGroup(this)
        for ((label, v) in listOf("Seguir o sistema" to "auto", "Claro" to "claro", "Escuro" to "escuro")) {
            g.addView(RadioButton(this).apply { text = label; setTextColor(p.text); isChecked = s.theme == v; id = View.generateViewId(); tag = v })
        }
        g.setOnCheckedChangeListener { grp, id ->
            val v = grp.findViewById<RadioButton>(id)?.tag as? String ?: return@setOnCheckedChangeListener
            if (v != s.theme) { save(copy(theme = v)); recreate() }
        }
        body.addView(g)

        title("SEGUNDO PLANO")
        val prefs = getSharedPreferences("app", Context.MODE_PRIVATE)
        toggle("Iniciar com o aparelho", prefs.getBoolean("boot", true)) { prefs.edit().putBoolean("boot", it).apply() }
        body.addView(Ui.button(this, p, "Ajustar economia de bateria") { startActivity(Intent(Sys.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)) },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = Ui.dp(this@SettingsActivity, 6) })
        note("Para manter a conexão com a tela desligada, deixe o RenanClip fora da otimização de bateria.")
        body.addView(Ui.button(this, p, "Parar sincronização agora") { stopService(Intent(this, ClipService::class.java)); finish() },
            LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = Ui.dp(this@SettingsActivity, 10) })
        note("Reabre sozinha ao abrir o app.")
        setContentView(root)
    }

    private fun copy(retention: Int = s.retentionDays, paused: Boolean = s.paused, images: Boolean = s.syncImages, files: Boolean = s.syncFiles, maxMb: Int = s.maxBlobMb, theme: String = s.theme) =
        Settings(s.deviceName, retention, paused, images, files, maxMb, s.storeCapMb, theme)

    private fun save(n: Settings) { s = Rc.engine.updateSettings(n) }
}
