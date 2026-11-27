package app.renanclip

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** Reinicia o serviço depois do boot ou de uma atualização do app (pode ser desligado nos ajustes). */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(ctx: Context, i: Intent) {
        if (i.action != Intent.ACTION_BOOT_COMPLETED && i.action != Intent.ACTION_MY_PACKAGE_REPLACED) return
        if (ctx.getSharedPreferences("app", Context.MODE_PRIVATE).getBoolean("boot", true)) ClipService.start(ctx)
    }
}
