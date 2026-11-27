package app.renanclip.core

import app.renanclip.core.Crypto.hex
import app.renanclip.core.Crypto.unhexN
import java.io.IOException
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

/** Um aparelho visto pela descoberta (NSD/mDNS) na rede local. */
class Nearby(val id: String, val name: String, val addrs: List<InetSocketAddress>)

class PeerView(val id: String, val name: String, val originalName: String, val alias: String, val online: Boolean, val nearby: Boolean, val addr: String, val lastSeen: Long)
class NearbyView(val id: String, val name: String, val addr: String, val paired: Boolean)

fun nowMs() = System.currentTimeMillis()

/**
 * Motor: servidor TCP, pareamento, sessões cifradas e sincronização. Sem Android: a descoberta
 * (NSD) entra por [onNearbyUp]/[onNearbyDown] e os eventos saem por [events].
 * Espelha core/src/engine.rs (inclusive a rota tailnet: o endereço guardado do par pode ser nome
 * MagicDNS ou IP 100.x e é tentado depois da rede local).
 */
class Engine(
    private val store: ClipStore,
    private val deviceNameDefault: String,
    private val preferredPort: Int = Proto.DEFAULT_PORT,
    private val events: (Event) -> Unit,
) {
    private sealed class Out {
        class M(val m: Msg) : Out()
        class Chunk(val id: ByteArray, val off: Int, val data: ByteArray) : Out()
        object Close : Out()
    }

    private class Conn(val cid: Long, val isClient: Boolean, val q: LinkedBlockingQueue<Out>, val sock: Socket)
    private class Asm(val meta: ClipMeta, val buf: java.io.ByteArrayOutputStream)

    val myHex: String
    private val myId: ByteArray
    private val server: ServerSocket
    val port: Int
    @Volatile private var settings: Settings
    private val conns = HashMap<String, Conn>()
    private val nearby = ConcurrentHashMap<String, Nearby>()
    private val dialing = ConcurrentHashMap.newKeySet<String>()
    private val backoff = ConcurrentHashMap<String, LongArray>() // peer -> [falhas, próxima tentativa]
    @Volatile private var pairingUntil = 0L
    private val waiters = ConcurrentHashMap<Long, ArrayBlockingQueue<Boolean>>()
    private val nextId = AtomicLong(1)
    @Volatile private var lastApplied: Pair<String, Long>? = null
    private val stop = AtomicBoolean(false)

    init {
        val h = store.setting("device_id")
        myHex = if (h != null && h.length == 32) h else hex(Crypto.rand(16)).also { store.setSetting("device_id", it) }
        myId = unhexN(myHex, 16)
        var s = Settings.fromJson(store.setting("settings"))
        if (s.deviceName.isEmpty()) s = Settings(deviceName = deviceNameDefault, retentionDays = s.retentionDays, paused = s.paused, syncImages = s.syncImages, syncFiles = s.syncFiles, maxBlobMb = s.maxBlobMb, storeCapMb = s.storeCapMb, theme = s.theme)
        settings = s
        store.setSetting("settings", s.toJson())
        server = try {
            ServerSocket().apply { reuseAddress = true; bind(InetSocketAddress(preferredPort)) }
        } catch (e: IOException) {
            ServerSocket().apply { reuseAddress = true; bind(InetSocketAddress(0)) } // ocupada: porta automática
        }
        port = server.localPort
    }

    fun start() {
        purge()
        thread("accept") {
            while (!stop.get()) {
                val s = try { server.accept() } catch (e: IOException) { break }
                thread("inbound") { handleInbound(s) }
            }
        }
        thread("maint") {
            var tick = 0L
            while (!stop.get()) {
                try { Thread.sleep(5000) } catch (e: InterruptedException) { break }
                tick++
                store.peers().forEach { dialAsync(it.id, false) }
                if (tick % 720 == 0L) purge()
            }
        }
    }

    fun shutdown() {
        stop.set(true)
        runCatching { server.close() }
        val all = synchronized(conns) { conns.values.toList().also { conns.clear() } }
        for (c in all) { c.q.offer(Out.Close); runCatching { c.sock.close() } }
    }

    private fun thread(name: String, r: () -> Unit) = Thread({ try { r() } catch (_: Throwable) {} }, "rc-$name").also { it.isDaemon = true; it.start() }

    val myId16: ByteArray get() = myId.copyOf()

    // ---------- descoberta ----------
    fun onNearbyUp(n: Nearby) {
        if (n.id == myHex || n.id.length != 32) return
        nearby[n.id] = n
        events(Event.Peers)
        if (store.peerKey(n.id) != null) dialAsync(n.id, true)
    }

    fun onNearbyDown(id: String) {
        nearby.remove(id)
        events(Event.Peers)
    }

    // ---------- configurações ----------
    fun settings(): Settings = settings

    fun updateSettings(s: Settings): Settings {
        val c = s.clamp()
        store.setSetting("settings", c.toJson())
        settings = c
        purge()
        events(Event.History)
        return c
    }

    // ---------- histórico ----------
    /** Registra uma cópia local e a envia aos aparelhos conectados. null se ignorada. */
    fun localCopy(c: LocalCopy): String? {
        val s = settings
        if (s.paused || c.sensitive) return null
        val cap = s.maxBlobMb.coerceAtLeast(1) * 1024 * 1024
        val bytes: ByteArray
        val text: String?
        val blob: ByteArray?
        when (c.kind) {
            "text" -> {
                val t = c.text ?: return null
                val b = t.toByteArray()
                if (t.isBlank() || b.size > Proto.MAX_TEXT) return null
                bytes = b; text = t; blob = null
            }
            "image" -> {
                if (!s.syncImages) return null
                val b = c.bytes ?: return null
                if (b.isEmpty() || b.size > cap) return null
                bytes = b; text = null; blob = b
            }
            "file" -> {
                if (!s.syncFiles) return null
                val b = c.bytes ?: return null
                if (b.isEmpty() || b.size > cap || c.name.isEmpty()) return null
                bytes = b; text = null; blob = b
            }
            else -> return null
        }
        val name = if (c.kind == "file") c.name else ""
        val id = Crypto.contentId(c.kind, name, bytes)
        if (recentlyApplied(id)) return null
        val r = store.upsert(id, NewItem(c.kind, name, c.mime, text, myHex), bytes.size.toLong(), nowMs(), blob)
        if (r == Upsert.Unchanged) return null
        events(Event.History)
        purge()
        broadcastItem(id, true)
        return id
    }

    fun history(query: String, kind: String?, limit: Int) = store.history(query, kind, limit)

    fun payload(id: String): Payload? {
        val it0 = store.get(id) ?: return null
        return when (it0.kind) {
            "text" -> store.fullText(id)?.let { Payload.Text(it) }
            "image" -> store.blob(id)?.let { Payload.Image(it) }
            "file" -> store.blob(id)?.let { Payload.File(it0.name, it0.mime, it) }
            else -> null
        }
    }

    /** Chamar depois de colocar o item na área de transferência por ação do usuário. */
    fun markApplied(id: String) { lastApplied = id to System.nanoTime() }

    /** Reenvia um item do histórico (ao escolher um item antigo, ele sobe ao topo). */
    fun touch(id: String) {
        val it = store.get(id) ?: return
        store.upsert(id, NewItem(it.kind, it.name, it.mime, null, myHex), it.size, nowMs(), null)
        events(Event.History)
        broadcastItem(id, true)
    }

    fun pin(id: String, pinned: Boolean) { store.setPinned(id, pinned); events(Event.History) }
    fun delete(id: String) { store.delete(id, nowMs()); events(Event.History) }
    fun clear(): Int { val n = store.clearUnpinned(nowMs()); events(Event.History); return n }

    // ---------- aparelhos ----------
    fun peers(): List<PeerView> {
        val on = synchronized(conns) { conns.keys.toSet() }
        return store.peers().map { PeerView(it.id, it.display(), it.name, it.alias, it.id in on, nearby.containsKey(it.id), it.addr, it.lastSeen) }
    }

    fun nearbyViews(): List<NearbyView> {
        val paired = store.peers().map { it.id }.toSet()
        return nearby.values.map { NearbyView(it.id, it.name, it.addrs.firstOrNull()?.let { a -> "${a.address.hostAddress}:${a.port}" } ?: "", it.id in paired) }.sortedBy { it.name }
    }

    fun renamePeer(id: String, alias: String) { store.renamePeer(id, alias); events(Event.Peers) }

    /** Revoga o aparelho: apaga a chave e, se estiver online, avisa o outro lado. */
    fun revokePeer(id: String) {
        val c = synchronized(conns) { conns.remove(id) }
        if (c != null) { c.q.offer(Out.M(Msg.Bye(true))); c.q.offer(Out.Close) }
        store.removePeer(id)
        events(Event.Peers)
    }

    fun onlineCount() = synchronized(conns) { conns.size }

    // ---------- pareamento ----------
    /** Aceita pareamentos de entrada por `secs` segundos. */
    fun openPairing(secs: Int) { pairingUntil = nowMs() + secs.coerceAtLeast(0) * 1000L }

    fun pairDecide(pairId: Long, accept: Boolean) { waiters.remove(pairId)?.offer(accept) }

    /** Resolve "nome", "nome:porta", "IP" ou "IP:porta" (MagicDNS da tailnet, hosts da LAN). Porta padrão: 47823. */
    fun resolve(host: String): InetSocketAddress = Resolver.resolve(host)

    /** Inicia o pareamento com o aparelho em `host` (nome ou IP, porta opcional). Devolve o id do pareamento. */
    fun pairWith(host: String): Long {
        val pid = nextId.getAndIncrement()
        thread("pair-out") {
            val r = try {
                val addr = resolve(host)
                val s = Socket()
                s.connect(addr, 4000)
                s.soTimeout = 135_000
                s.tcpNoDelay = true
                Frame.writePreamble(s.getOutputStream(), Frame.MODE_PAIR)
                val p = Pairing.initiate(s.getInputStream(), s.getOutputStream(), myId, settings.deviceName) { _, pn, sas -> askUser(pid, pn, sas, false) }
                s.close()
                Result.success(p to host.trim())
            } catch (e: Exception) { Result.failure(e) }
            finishPair(pid, r)
        }
        return pid
    }

    /** Guarda o endereço (nome MagicDNS ou IP, porta opcional) de um aparelho já pareado. */
    fun setPeerAddr(id: String, addr: String) {
        store.touchPeer(id, addr.trim(), 0)
        events(Event.Peers)
        dialAsync(id, true)
    }

    private fun askUser(pid: Long, peerName: String, sas: String, inbound: Boolean): Boolean {
        val q = ArrayBlockingQueue<Boolean>(1) // capacidade 1: decidir dentro do callback do evento não trava
        waiters[pid] = q
        events(Event.PairPrompt(pid, peerName, sas, inbound))
        val ok = q.poll(120, TimeUnit.SECONDS) ?: false
        waiters.remove(pid)
        return ok
    }

    private fun finishPair(pid: Long, r: Result<Pair<Paired, String>>) {
        r.onSuccess { (p, addr) ->
            val id = hex(p.peerId)
            store.addPeer(id, p.peerName, p.key, addr, nowMs())
            events(Event.PairDone(pid, true, p.peerName, ""))
            events(Event.Peers)
            dialAsync(id, true)
        }.onFailure { e ->
            events(Event.PairDone(pid, false, "", e.message ?: e.toString()))
        }
    }

    // ---------- entrada ----------
    private fun handleInbound(s: Socket) {
        try {
            s.tcpNoDelay = true
            s.soTimeout = 10_000
            val i = s.getInputStream(); val o = s.getOutputStream()
            when (Frame.readPreamble(i)) {
                Frame.MODE_PAIR -> {
                    if (nowMs() > pairingUntil) { s.close(); return } // ninguém abriu o pareamento
                    s.soTimeout = 135_000
                    val pid = nextId.getAndIncrement()
                    val r = try {
                        val p = Pairing.respond(i, o, myId, settings.deviceName) { _, pn, sas -> askUser(pid, pn, sas, true) }
                        Result.success(p to "")
                    } catch (e: Exception) { Result.failure(e) }
                    finishPair(pid, r)
                    runCatching { s.close() }
                }
                Frame.MODE_SESSION -> {
                    val hello = Json.parseObject(String(Frame.read(i, 512), Charsets.UTF_8))
                    if (hello["t"] != "s1") { s.close(); return }
                    val id = hello["id"] as? String ?: run { s.close(); return }
                    val key = store.peerKey(id) ?: run { s.close(); return }
                    val nc = unhexN(hello["n"] as? String ?: "", 16)
                    val ns = Crypto.rand(16)
                    Frame.write(o, Json.write(mapOf("t" to "s2", "id" to myHex, "n" to hex(ns))).toByteArray())
                    val (c2s, s2c) = Crypto.sessionKeys(key, nc, ns)
                    runSession(s, id, Seal(s2c, Dir.ServerToClient), Seal(c2s, Dir.ClientToServer), false)
                }
                else -> s.close()
            }
        } catch (e: Exception) {
            runCatching { s.close() }
        }
    }

    // ---------- saída ----------
    private fun dialAsync(peer: String, force: Boolean) {
        if (stop.get()) return
        synchronized(conns) { if (conns.containsKey(peer)) return }
        val bo = backoff[peer]
        if (!force && bo != null && nowMs() < bo[1]) return
        if (!dialing.add(peer)) return
        thread("dial") {
            try {
                val ok = dial(peer)
                if (ok) backoff.remove(peer) else {
                    val f = ((bo?.get(0) ?: 0L) + 1).coerceAtMost(12)
                    backoff[peer] = longArrayOf(f, nowMs() + minOf(5_000L * f, 60_000L))
                }
            } finally { dialing.remove(peer) }
        }
    }

    /** 1º rede local (NSD), 2º o endereço guardado do par (último bom / nome MagicDNS / IP da tailnet). Offline não é erro. */
    private fun dial(peer: String): Boolean {
        val key = store.peerKey(peer) ?: return false
        val addrs = ArrayList<InetSocketAddress>()
        nearby[peer]?.addrs?.let { addrs.addAll(it) }
        val stored = store.peers().firstOrNull { it.id == peer }?.addr.orEmpty()
        if (stored.isNotBlank()) {
            try { val a = resolve(stored); if (a !in addrs) addrs.add(a) } catch (_: Exception) { /* sem rede/tailscale: segue offline */ }
        }
        for (a in addrs) {
            try { if (dialOne(peer, key, a)) return true } catch (_: Exception) { }
        }
        return false
    }

    private fun dialOne(peer: String, key: ByteArray, addr: InetSocketAddress): Boolean {
        val s = Socket()
        try {
            s.connect(addr, 3000)
            s.tcpNoDelay = true
            s.soTimeout = 6000
            val i = s.getInputStream(); val o = s.getOutputStream()
            Frame.writePreamble(o, Frame.MODE_SESSION)
            val nc = Crypto.rand(16)
            Frame.write(o, Json.write(mapOf("t" to "s1", "id" to myHex, "n" to hex(nc))).toByteArray())
            val s2 = Json.parseObject(String(Frame.read(i, 512), Charsets.UTF_8))
            if (s2["t"] != "s2") throw ProtoException("esperava s2")
            if (s2["id"] != peer) throw ProtoException("aparelho diferente do esperado") // o id em S2 confirma quem atendeu
            val ns = unhexN(s2["n"] as? String ?: "", 16)
            val (c2s, s2c) = Crypto.sessionKeys(key, nc, ns)
            runSession(s, peer, Seal(c2s, Dir.ClientToServer), Seal(s2c, Dir.ServerToClient), true)
            return true
        } catch (e: Exception) {
            runCatching { s.close() }
            throw e
        }
    }

    // ---------- sessão ----------
    private fun runSession(sock: Socket, peer: String, txSeal: Seal, rxSeal: Seal, isClient: Boolean) {
        val rd = sock.getInputStream(); val wr = sock.getOutputStream()
        sock.soTimeout = 45_000
        val cid = nextId.getAndIncrement()
        val q = LinkedBlockingQueue<Out>()
        // Só registra a conexão depois do 1º quadro decifrado com a chave do par: quem só conhece o id
        // (público no mDNS) não consegue ocupar o lugar do par nem aparecer como "online".
        var registered = false
        fun register(): Boolean {
            synchronized(conns) {
                val old = conns[peer]
                if (old != null) {
                    // Duas conexões simultâneas: fica a aberta pelo aparelho de menor id (regra igual nos dois lados).
                    val newClient = if (isClient) myHex else peer
                    val oldClient = if (old.isClient) myHex else peer
                    if (newClient > oldClient) return false
                    old.q.offer(Out.Close); runCatching { old.sock.close() }
                }
                conns[peer] = Conn(cid, isClient, q, sock)
            }
            store.touchPeer(peer, "", nowMs()) // não sobrescreve o endereço guardado (pode ser nome MagicDNS)
            backoff.remove(peer)
            events(Event.Peers)
            return true
        }

        val writer = Thread({
            try {
                while (true) {
                    val out = q.poll(15, TimeUnit.SECONDS) ?: Out.M(Msg.Ping)
                    val plain = when (out) {
                        Out.Close -> break
                        is Out.M -> byteArrayOf(Proto.T_JSON.toByte()) + out.m.toJson().toByteArray()
                        is Out.Chunk -> {
                            val b = java.nio.ByteBuffer.allocate(21 + out.data.size)
                            b.put(Proto.T_CHUNK.toByte()); b.put(out.id); b.putInt(out.off); b.put(out.data)
                            b.array()
                        }
                    }
                    Frame.write(wr, txSeal.seal(plain))
                }
            } catch (_: Exception) { }
            runCatching { sock.close() }
        }, "rc-conn-write").also { it.isDaemon = true; it.start() }

        q.offer(Out.M(Msg.Inv(store.inventory(Proto.INVENTORY))))
        val asm = HashMap<String, Asm>()
        try {
            while (true) {
                val plain = rxSeal.open(Frame.read(rd, Frame.MAX_FRAME + 64))
                if (!registered) { if (!register()) break; registered = true }
                if (plain.isEmpty()) break
                val keep = when (plain[0].toInt()) {
                    Proto.T_JSON -> onMsg(peer, q, asm, Msg.parse(String(plain, 1, plain.size - 1, Charsets.UTF_8)))
                    Proto.T_CHUNK -> if (plain.size > 21) onChunk(peer, asm, plain) else false
                    else -> false
                }
                if (!keep) break
            }
        } catch (_: Exception) {
        } finally {
            q.offer(Out.Close)
            runCatching { writer.join(2000) }
            runCatching { sock.close() }
            val gone = synchronized(conns) { if (conns[peer]?.cid == cid) { conns.remove(peer); true } else false }
            if (gone) events(Event.Peers)
        }
    }

    private fun onMsg(peer: String, q: LinkedBlockingQueue<Out>, asm: HashMap<String, Asm>, m: Msg): Boolean {
        when (m) {
            Msg.Ping -> q.offer(Out.M(Msg.Pong))
            Msg.Pong -> {}
            is Msg.Bye -> { if (m.revoked) { store.removePeer(peer); events(Event.Peers) }; return false }
            is Msg.Inv -> {
                val want = m.items.take(Proto.INVENTORY * 2).filter { (id, ts) -> id.length == 32 && (store.hasRow(id)?.let { ts > it } ?: true) }.map { it.first }
                if (want.isNotEmpty()) q.offer(Out.M(Msg.Want(want)))
            }
            is Msg.Want -> m.ids.take(Proto.INVENTORY * 2).forEach { sendItem(q, it, false) }
            is Msg.Clip -> return onClip(peer, asm, m.meta)
        }
        return true
    }

    private fun onClip(peer: String, asm: HashMap<String, Asm>, meta: ClipMeta): Boolean {
        val s = settings
        val cap = s.maxBlobMb.coerceAtLeast(1) * 1024L * 1024L
        if (meta.id.length != 32) return false
        when (meta.kind) {
            "text" -> {
                val t = meta.text ?: return true
                val b = t.toByteArray()
                if (b.size > Proto.MAX_TEXT || Crypto.contentId("text", "", b) != meta.id) return true // id não bate: descarta sem derrubar
                stored(meta, store.upsert(meta.id, NewItem("text", "", "text/plain", t, peer), b.size.toLong(), meta.ts, null), Payload.Text(t), s)
            }
            "image", "file" -> {
                val okKind = if (meta.kind == "image") s.syncImages else s.syncFiles
                val needed = store.hasRow(meta.id)?.let { meta.ts > it } ?: true
                if (!okKind || !needed || meta.size <= 0 || meta.size > cap) return true
                if (asm.size < 4) asm[meta.id] = Asm(meta, java.io.ByteArrayOutputStream(meta.size.toInt()))
            }
        }
        return true
    }

    private fun onChunk(peer: String, asm: HashMap<String, Asm>, plain: ByteArray): Boolean {
        val id = hex(plain.copyOfRange(1, 17))
        val off = java.nio.ByteBuffer.wrap(plain, 17, 4).int.toLong() and 0xffffffffL
        val a = asm[id] ?: return true
        if (off != a.buf.size().toLong() || a.buf.size() + (plain.size - 21) > a.meta.size) { asm.remove(id); return true }
        a.buf.write(plain, 21, plain.size - 21)
        if (a.buf.size() < a.meta.size) return true
        asm.remove(id)
        val meta = a.meta
        val data = a.buf.toByteArray()
        if (Crypto.contentId(meta.kind, meta.name, data) != meta.id) return true // corrompido ou forjado
        val payload = if (meta.kind == "image") Payload.Image(data) else Payload.File(meta.name, meta.mime, data)
        val r = store.upsert(meta.id, NewItem(meta.kind, meta.name, meta.mime, null, peer), data.size.toLong(), meta.ts, data)
        stored(meta, r, payload, settings)
        return true
    }

    private fun stored(meta: ClipMeta, r: Upsert, payload: Payload, s: Settings) {
        if (r == Upsert.Unchanged) return
        events(Event.History)
        purge()
        if (meta.live && !s.paused) {
            lastApplied = meta.id to System.nanoTime()
            events(Event.Apply(meta.id, payload))
        }
    }

    private fun sendItem(q: LinkedBlockingQueue<Out>, id: String, live: Boolean) {
        val it = store.get(id) ?: return
        val text = if (it.kind == "text") store.fullText(id) else null
        if (it.kind == "text" && text == null) return
        val meta = ClipMeta(id, it.ts, it.kind, it.mime, it.name, it.size, live, text)
        if (it.kind != "text") {
            val b = store.blob(id) ?: return
            val raw = Crypto.unhex(id) ?: return
            q.offer(Out.M(Msg.Clip(meta.copy(size = b.size.toLong()))))
            var off = 0
            while (off < b.size) {
                val n = minOf(Proto.CHUNK, b.size - off)
                q.offer(Out.Chunk(raw, off, b.copyOfRange(off, off + n)))
                off += n
            }
        } else q.offer(Out.M(Msg.Clip(meta)))
    }

    private fun broadcastItem(id: String, live: Boolean) {
        val qs = synchronized(conns) { conns.values.map { it.q } }
        qs.forEach { sendItem(it, id, live) }
    }

    private fun recentlyApplied(id: String): Boolean = lastApplied?.let { (a, t) -> a == id && System.nanoTime() - t < 4_000_000_000L } ?: false

    private fun purge() {
        val s = settings
        val n = store.purge(nowMs(), s.retentionDays, s.storeCapMb * 1024L * 1024L)
        if (n > 0) events(Event.History)
    }
}

object Resolver {
    /** "nome", "nome:porta", "IP", "IP:porta", "[v6]" ou "[v6]:porta". Porta padrão 47823. Prefere IPv4. */
    fun resolve(host: String): InetSocketAddress {
        val h = host.trim()
        if (h.isEmpty() || h.length > 255) throw ProtoException("endereço vazio")
        var name = h
        var port = Proto.DEFAULT_PORT
        if (h.startsWith("[")) {
            val e = h.indexOf(']')
            if (e < 0) throw ProtoException("endereço inválido")
            name = h.substring(1, e)
            h.substring(e + 1).removePrefix(":").takeIf { it.isNotEmpty() }?.let { port = it.toIntOrNull() ?: throw ProtoException("porta inválida") }
        } else if (h.count { it == ':' } == 1) {
            name = h.substringBefore(':')
            port = h.substringAfter(':').toIntOrNull() ?: throw ProtoException("porta inválida")
        }
        if (port !in 1..65535 || name.isBlank()) throw ProtoException("endereço inválido")
        val all = try { InetAddress.getAllByName(name) } catch (e: Exception) { throw ProtoException("não encontrei \"$name\"") }
        val a = all.firstOrNull { it is Inet4Address } ?: all.firstOrNull() ?: throw ProtoException("não encontrei \"$name\"")
        return InetSocketAddress(a, port)
    }
}
