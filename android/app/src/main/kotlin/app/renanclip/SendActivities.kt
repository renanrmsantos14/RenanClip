package app.renanclip

import android.app.Activity
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import app.renanclip.core.LocalCopy

/** Envia um conteúdo (já lido) e avisa o resultado; reconecta se o serviço acabou de subir. */
internal object Sender {
    fun send(ctx: Context, copies: List<LocalCopy>, errors: List<String>) {
        val e = Rc.ensure(ctx)
        ClipService.start(ctx)
        var sent = 0
        val ids = ArrayList<String>()
        for (c in copies) { val id = e.localCopy(c); if (id != null) { sent++; ids.add(id) } }
        if (sent > 0 && e.onlineCount() == 0 && e.peers().isNotEmpty()) {
            // processo recém-iniciado: espera a conexão (até 4 s) e reenvia ao vivo
            val end = System.currentTimeMillis() + 4000
            while (e.onlineCount() == 0 && System.currentTimeMillis() < end) Thread.sleep(150)
            if (e.onlineCount() > 0) ids.forEach { e.touch(it) }
        }
        val n = e.onlineCount()
        val msg = when {
            sent > 0 && n > 0 -> "Enviado para $n aparelho${if (n > 1) "s" else ""}."
            sent > 0 -> "Salvo no histórico; vai quando houver aparelho conectado."
            errors.isNotEmpty() -> errors.first()
            else -> "Nada para enviar (sincronização pausada ou conteúdo ignorado)."
        }
        Rc.toast(ctx, msg)
    }
}

/** Activity transparente do tile: lê o clipboard em primeiro plano (Android 10+ só permite assim). */
class SendClipboardActivity : Activity() {
    private var done = false

    override fun onCreate(b: Bundle?) { super.onCreate(b); Rc.ensure(this) }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (!hasFocus || done) return
        done = true
        val ctx = applicationContext
        val clip = (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager).primaryClip
        Thread {
            val cap = Rc.engine.settings().maxBlobMb * 1024 * 1024
            val r = Intake.fromClip(ctx, clip, cap)
            Sender.send(ctx, listOfNotNull(r.copy), listOfNotNull(r.error))
        }.start()
        finish()
    }
}

/** Alvo do "Compartilhar" do sistema: texto, imagens e arquivos pequenos. */
class ShareActivity : Activity() {
    override fun onCreate(b: Bundle?) {
        super.onCreate(b)
        Rc.ensure(this)
        val ctx = applicationContext
        val i = intent
        val uris = ArrayList<Uri>()
        @Suppress("DEPRECATION")
        when (i?.action) {
            Intent.ACTION_SEND -> i.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.add(it) }
            Intent.ACTION_SEND_MULTIPLE -> i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.addAll(it) }
        }
        val text = i?.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()
        val mime = i?.type
        Thread {
            val cap = Rc.engine.settings().maxBlobMb * 1024 * 1024
            val copies = ArrayList<LocalCopy>(); val errors = ArrayList<String>()
            if (!text.isNullOrBlank() && uris.isEmpty()) copies.add(LocalCopy.text(text))
            for (u in uris) { val r = Intake.fromUri(ctx, u, mime, cap); r.copy?.let { copies.add(it) }; r.error?.let { errors.add(it) } }
            if (copies.isEmpty() && errors.isEmpty()) errors.add("Nada para enviar.")
            Sender.send(ctx, copies, errors)
        }.start()
        finish()
    }
}

/** Botão "Enviar área de transferência" nas configurações rápidas. */
class SendTileService : TileService() {
    override fun onStartListening() {
        qsTile?.let { it.state = Tile.STATE_INACTIVE; it.label = "Enviar área de transferência"; it.updateTile() }
    }

    @Suppress("DEPRECATION")
    override fun onClick() {
        val i = Intent(this, SendClipboardActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
        if (android.os.Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(android.app.PendingIntent.getActivity(this, 0, i, android.app.PendingIntent.FLAG_IMMUTABLE))
        } else startActivityAndCollapse(i)
    }
}
