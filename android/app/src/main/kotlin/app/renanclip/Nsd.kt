package app.renanclip

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Handler
import android.os.Looper
import app.renanclip.core.Engine
import app.renanclip.core.Nearby
import app.renanclip.core.Proto
import java.net.Inet4Address
import java.net.InetSocketAddress

/**
 * Registro e descoberta mDNS (`_renanclip._tcp.`, instância = id do aparelho, TXT id/n/v), igual ao
 * core/src/discovery.rs. Resolve um serviço por vez (o NsdManager não aceita resoluções paralelas)
 * e só usa endereços IPv4. Qualquer falha de rede só deixa o aparelho "fora da rede local".
 */
class NsdHelper(ctx: Context, private val engine: Engine) {
    private val nsd = ctx.applicationContext.getSystemService(Context.NSD_SERVICE) as NsdManager
    private val h = Handler(Looper.getMainLooper())
    private var reg: NsdManager.RegistrationListener? = null
    private var disc: NsdManager.DiscoveryListener? = null
    private val queue = ArrayDeque<NsdServiceInfo>()
    private var resolving = false
    private val idByName = HashMap<String, String>()
    private var stopped = true

    fun start() {
        stopped = false
        register()
        discover()
    }

    fun restart() { if (!stopped) { stopDiscovery(); discover() } }

    fun stop() {
        stopped = true
        stopDiscovery()
        reg?.let { runCatching { nsd.unregisterService(it) } }
        reg = null
        queue.clear(); resolving = false
    }

    private fun register() {
        val info = NsdServiceInfo().apply {
            serviceName = engine.myHex
            serviceType = Proto.SERVICE_TYPE
            port = engine.port
            setAttribute("id", engine.myHex)
            setAttribute("n", engine.settings().deviceName.take(40))
            setAttribute("v", "1")
        }
        val l = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(i: NsdServiceInfo) {}
            override fun onRegistrationFailed(i: NsdServiceInfo, code: Int) {}
            override fun onServiceUnregistered(i: NsdServiceInfo) {}
            override fun onUnregistrationFailed(i: NsdServiceInfo, code: Int) {}
        }
        reg = l
        runCatching { nsd.registerService(info, NsdManager.PROTOCOL_DNS_SD, l) }
    }

    private fun discover() {
        val l = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(t: String) {}
            override fun onDiscoveryStopped(t: String) {}
            override fun onStartDiscoveryFailed(t: String, code: Int) {}
            override fun onStopDiscoveryFailed(t: String, code: Int) {}
            override fun onServiceFound(i: NsdServiceInfo) {
                if (i.serviceName == engine.myHex) return
                h.post { queue.addLast(i); pump() }
            }
            override fun onServiceLost(i: NsdServiceInfo) {
                h.post {
                    val id = idByName.remove(i.serviceName) ?: i.serviceName
                    engine.onNearbyDown(id)
                }
            }
        }
        disc = l
        runCatching { nsd.discoverServices(Proto.SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, l) }
    }

    private fun stopDiscovery() {
        disc?.let { runCatching { nsd.stopServiceDiscovery(it) } }
        disc = null
    }

    private fun pump() {
        if (resolving || stopped) return
        val next = queue.removeFirstOrNull() ?: return
        resolving = true
        var done = false
        val finish = Runnable { if (!done) { done = true; resolving = false; pump() } }
        h.postDelayed(finish, 5000) // resolução que nunca responde não trava a fila
        @Suppress("DEPRECATION")
        runCatching {
            nsd.resolveService(next, object : NsdManager.ResolveListener {
                override fun onResolveFailed(i: NsdServiceInfo, code: Int) { h.post(finish) }
                override fun onServiceResolved(i: NsdServiceInfo) {
                    h.post {
                        val id = i.attributes["id"]?.let { String(it) } ?: i.serviceName
                        val name = i.attributes["n"]?.let { String(it) } ?: ""
                        val host = i.host
                        if (host is Inet4Address && id.length == 32) {
                            idByName[i.serviceName] = id
                            engine.onNearbyUp(Nearby(id, name, listOf(InetSocketAddress(host, i.port))))
                        }
                        finish.run()
                    }
                }
            })
        }.onFailure { h.post(finish) }
    }
}
