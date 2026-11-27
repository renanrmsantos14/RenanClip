package app.renanclip

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.IBinder
import app.renanclip.core.Event

/** Serviço em primeiro plano: mantém o servidor TCP, a descoberta NSD e a reconexão aos pareados. */
class ClipService : Service() {
    private var nsd: NsdHelper? = null
    private var net: ConnectivityManager.NetworkCallback? = null
    private val listener: (Event) -> Unit = { if (it is Event.Peers) notifyState() }

    override fun onBind(i: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, "Sincronização", NotificationManager.IMPORTANCE_MIN).apply {
            description = "Mantém o RenanClip conectado aos seus aparelhos"; setShowBadge(false)
        })
        // startForeground primeiro: o prazo é de poucos segundos e abrir o banco/porta pode demorar
        if (!startFg(build(0))) { stopSelf(); return }
        val e = Rc.ensure(this)
        getSystemService(NotificationManager::class.java).notify(ID, build(e.onlineCount()))
        nsd = NsdHelper(this, e).also { it.start() }
        val cm = getSystemService(ConnectivityManager::class.java)
        net = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(n: Network) { Rc.main.post { nsd?.restart() } }
        }.also { runCatching { cm.registerDefaultNetworkCallback(it) } }
        Rc.listen(listener)
    }

    override fun onStartCommand(i: Intent?, flags: Int, startId: Int): Int {
        if (i?.action == ACTION_STOP) { stopSelf(); return START_NOT_STICKY }
        return START_STICKY
    }

    override fun onDestroy() {
        Rc.unlisten(listener)
        net?.let { runCatching { getSystemService(ConnectivityManager::class.java).unregisterNetworkCallback(it) } }
        nsd?.stop()
        Rc.shutdown()
        running = false
        super.onDestroy()
    }

    private fun startFg(n: Notification): Boolean = try {
        if (Build.VERSION.SDK_INT >= 29) startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        else startForeground(ID, n)
        running = true
        true
    } catch (_: Exception) { false } // Android 12+: reinício em segundo plano pode ser negado

    private fun notifyState() {
        runCatching { getSystemService(NotificationManager::class.java).notify(ID, build(Rc.engine.onlineCount())) }
    }

    private fun build(online: Int): Notification {
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        val stop = PendingIntent.getService(this, 1, Intent(this, ClipService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE)
        val text = when (online) { 0 -> "Sem aparelhos conectados"; 1 -> "1 aparelho conectado"; else -> "$online aparelhos conectados" }
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_notif).setContentTitle("RenanClip").setContentText(text)
            .setContentIntent(open).setOngoing(true).setShowWhen(false)
            .setCategory(Notification.CATEGORY_SERVICE).setVisibility(Notification.VISIBILITY_SECRET)
            .addAction(Notification.Action.Builder(null, "Parar", stop).build())
            .build()
    }

    companion object {
        const val CHANNEL = "sync"
        const val ID = 1
        const val ACTION_STOP = "app.renanclip.STOP"
        @Volatile var running = false

        fun start(ctx: Context) {
            runCatching { ctx.startForegroundService(Intent(ctx, ClipService::class.java)) }
        }
    }
}
