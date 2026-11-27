package app.renanclip

import android.app.Activity
import android.app.AlertDialog
import android.os.Bundle
import android.text.InputType
import android.view.Gravity
import android.view.ViewGroup
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import app.renanclip.core.Event

/** Aparelhos pareados (renomear, endereço, revogar, status) e pareamento (código de 6 dígitos). */
class PeersActivity : Activity() {
    private lateinit var p: Pal
    private lateinit var body: LinearLayout
    private lateinit var info: TextView
    private lateinit var host: EditText
    private var dialog: AlertDialog? = null
    private val tick = object : Runnable { override fun run() { refresh(); Rc.main.postDelayed(this, 3000) } }
    private val listener: (Event) -> Unit = { ev ->
        when (ev) {
            is Event.Peers -> refresh()
            is Event.PairPrompt -> prompt(ev)
            is Event.PairDone -> {
                dialog?.dismiss()
                info.text = if (ev.ok) "Pareado com ${ev.peerName}." else "Não pareou: ${ev.error}"
                refresh()
            }
            else -> {}
        }
    }

    override fun onCreate(b: Bundle?) {
        Rc.ensure(this)
        p = Ui.applyTheme(this)
        super.onCreate(b)
        val (root, content) = Ui.screen(this, p)
        content.addView(Ui.header(this, p, "Aparelhos", true))
        val sv = ScrollView(this)
        body = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(Ui.dp(context, 16), 0, Ui.dp(context, 16), Ui.dp(context, 16)) }
        sv.addView(body)
        content.addView(sv, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        setContentView(root)
        info = Ui.label(this, "", 13f, p.sub)
        host = EditText(this)
    }

    override fun onStart() { super.onStart(); Rc.listen(listener); refresh(); Rc.main.postDelayed(tick, 3000) }
    override fun onStop() { Rc.main.removeCallbacks(tick); Rc.unlisten(listener); dialog?.dismiss(); super.onStop() }

    private fun section(title: String) = body.addView(Ui.label(this, title, 13f, p.sub, true).apply { setPadding(0, Ui.dp(context, 20), 0, Ui.dp(context, 6)) })

    private fun card(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL; background = Ui.rounded(p.surface, 12, context, p.line)
        setPadding(Ui.dp(context, 14), Ui.dp(context, 12), Ui.dp(context, 14), Ui.dp(context, 12))
    }.also { body.addView(it, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { bottomMargin = Ui.dp(this@PeersActivity, 8) }) }

    private fun refresh() {
        val e = Rc.engine
        val typed = host.text?.toString().orEmpty()
        (info.parent as? ViewGroup)?.removeView(info)
        body.removeAllViews()

        section("PAREADOS")
        val peers = e.peers()
        if (peers.isEmpty()) body.addView(Ui.label(this, "Nenhum aparelho pareado ainda.", 14f, p.sub))
        for (v in peers) {
            val c = card()
            c.addView(Ui.label(this, v.name, 16f, p.text, true))
            val st = if (v.online) "● online" else if (v.nearby) "○ na rede, conectando…" else "○ offline"
            c.addView(Ui.label(this, st, 13f, if (v.online) p.accent else p.sub))
            if (v.alias.isNotEmpty()) c.addView(Ui.label(this, "Nome original: ${v.originalName}", 12f, p.sub))
            if (v.addr.isNotEmpty()) c.addView(Ui.label(this, "Endereço: ${v.addr}", 12f, p.sub))
            if (v.lastSeen > 0) c.addView(Ui.label(this, "Visto por último: ${Rc.time(v.lastSeen)}", 12f, p.sub))
            c.addView(Ui.label(this, "Toque para renomear, definir endereço ou revogar", 11f, p.sub).apply { setPadding(0, Ui.dp(context, 6), 0, 0) })
            c.setOnClickListener { peerMenu(v.id, v.name, v.alias, v.addr) }
        }

        section("NA REDE LOCAL")
        val near = e.nearbyViews().filter { !it.paired }
        if (near.isEmpty()) body.addView(Ui.label(this, "Nenhum aparelho novo encontrado. No outro aparelho, abra Aparelhos e toque em \"Aceitar pareamento\".", 13f, p.sub))
        for (n in near) {
            val c = card()
            c.orientation = LinearLayout.HORIZONTAL; c.gravity = Gravity.CENTER_VERTICAL
            c.addView(Ui.label(this, n.name.ifEmpty { "Aparelho" }, 15f, p.text, true), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
            c.addView(Ui.button(this, p, "Parear", true) { startPair(n.addr) })
        }

        section("ADICIONAR POR NOME OU IP (TAILSCALE)")
        body.addView(Ui.label(this, "Fora de casa: nome MagicDNS (ex.: pc-renan) ou IP da tailnet (ex.: 100.101.102.103). Porta opcional; padrão 47823.", 12f, p.sub))
        host = EditText(this).apply {
            hint = "nome ou IP[:porta]"; setHintTextColor(p.sub); setTextColor(p.text); setSingleLine(); setText(typed)
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            background = Ui.rounded(p.surface, 10, context, p.line); setPadding(Ui.dp(context, 14), 0, Ui.dp(context, 14), 0)
        }
        body.addView(host, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, Ui.dp(this, 44)).apply { topMargin = Ui.dp(this@PeersActivity, 8) })
        body.addView(Ui.button(this, p, "Parear com este endereço") {
            val h = host.text.toString().trim()
            if (h.isEmpty()) host.error = "Digite o nome ou IP" else startPair(h)
        }, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = Ui.dp(this@PeersActivity, 8) })

        section("ACEITAR PAREAMENTO NESTE APARELHO")
        body.addView(Ui.button(this, p, "Aceitar pareamento por 2 minutos", true) {
            e.openPairing(120)
            info.text = "Aguardando o outro aparelho… Ele deve escolher este aparelho (ou digitar o endereço). O código de 6 dígitos aparece nos dois."
        })

        body.addView(info, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT).apply { topMargin = Ui.dp(this@PeersActivity, 12) })
    }

    private fun startPair(addr: String) {
        info.text = "Conectando a $addr…"
        Rc.engine.pairWith(addr)
    }

    private fun prompt(ev: Event.PairPrompt) {
        dialog?.dismiss()
        val sas = ev.sas.padStart(6, '0').let { it.substring(0, 3) + " " + it.substring(3) }
        val tv = Ui.label(this, sas, 40f, p.text, true).apply { gravity = Gravity.CENTER; setPadding(0, Ui.dp(context, 16), 0, Ui.dp(context, 8)); letterSpacing = 0.1f }
        val box = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL; setPadding(Ui.dp(context, 24), Ui.dp(context, 8), Ui.dp(context, 24), 0)
            addView(Ui.label(context, "Confira se o código abaixo é o mesmo em \"${ev.peerName}\". Só confirme se forem idênticos.", 14f, p.sub))
            addView(tv)
        }
        dialog = AlertDialog.Builder(this).setTitle("Parear com ${ev.peerName}").setView(box).setCancelable(false)
            .setPositiveButton("São iguais") { _, _ -> Rc.engine.pairDecide(ev.pairId, true) }
            .setNegativeButton("Recusar") { _, _ -> Rc.engine.pairDecide(ev.pairId, false) }.show()
    }

    private fun peerMenu(id: String, name: String, alias: String, addr: String) {
        AlertDialog.Builder(this).setTitle(name).setItems(arrayOf("Renomear", "Definir endereço (nome ou IP)", "Revogar pareamento")) { _, w ->
            when (w) {
                0 -> input("Nome", alias.ifEmpty { name }) { Rc.engine.renamePeer(id, it) }
                1 -> input("Nome MagicDNS ou IP[:porta]", addr) { Rc.engine.setPeerAddr(id, it) }
                2 -> AlertDialog.Builder(this).setMessage("Revogar \"$name\"? A chave é apagada nos dois lados e será preciso parear de novo.")
                    .setPositiveButton("Revogar") { _, _ -> Rc.engine.revokePeer(id) }.setNegativeButton("Cancelar", null).show()
            }
        }.show()
    }

    private fun input(title: String, initial: String, ok: (String) -> Unit) {
        val et = EditText(this).apply { setText(initial); setSingleLine(); setSelection(text.length) }
        AlertDialog.Builder(this).setTitle(title).setView(et)
            .setPositiveButton("Salvar") { _, _ -> ok(et.text.toString().trim()) }.setNegativeButton("Cancelar", null).show()
    }
}
