package app.renanclip.core

import java.text.Normalizer

const val DAY_MS = 86_400_000L
const val PREVIEW_CHARS = 2000

class Item(
    val id: String, val kind: String, val name: String, val mime: String, val size: Long,
    /** Texto cortado em [PREVIEW_CHARS] na listagem. */
    val text: String?, val ts: Long, val pinned: Boolean, val origin: String,
)

class NewItem(val kind: String, val name: String, val mime: String, val text: String?, val origin: String)

class Peer(val id: String, val name: String, val alias: String, val addr: String, val pairedAt: Long, val lastSeen: Long) {
    fun display() = alias.ifEmpty { name }
}

enum class Upsert { Inserted, Bumped, Unchanged }

/** Estado de uma linha existente, para decidir o que um upsert faz (igual nas duas implementações). */
class Row(val ts: Long, val deleted: Boolean, val deletedAt: Long)

object StoreRules {
    fun decide(cur: Row?, ts: Long): Upsert = when {
        cur == null -> Upsert.Inserted
        cur.deleted && ts > cur.deletedAt -> Upsert.Inserted // regravado depois da lápide: ressuscita
        cur.deleted -> Upsert.Unchanged
        ts > cur.ts -> Upsert.Bumped
        else -> Upsert.Unchanged
    }

    /** Minúsculas e sem acento (pt-BR), para a busca. */
    fun fold(s: String): String =
        Normalizer.normalize(s.lowercase(), Normalizer.Form.NFD).filter { Character.getType(it) != Character.NON_SPACING_MARK.toInt() }
            .replace('ç', 'c').replace('ñ', 'n')

    fun searchKey(text: String?, name: String): String = fold((text ?: "") + " " + name)
}

/** Histórico + aparelhos pareados + configurações. Implementações: SQLite (Android) e memória (testes). */
interface ClipStore {
    fun setting(k: String): String?
    fun setSetting(k: String, v: String)

    fun upsert(id: String, n: NewItem, size: Long, ts: Long, blob: ByteArray?): Upsert
    /** max(ts, deleted_at) da linha, vivo ou lápide; null se não existe. */
    fun hasRow(id: String): Long?
    fun history(query: String, kind: String?, limit: Int): List<Item>
    fun get(id: String): Item?
    fun fullText(id: String): String?
    fun blob(id: String): ByteArray?
    fun setPinned(id: String, pinned: Boolean)
    /** Some o conteúdo; fica só a lápide para a sincronização não ressuscitar o item. */
    fun delete(id: String, now: Long)
    fun clearUnpinned(now: Long): Int
    fun purge(now: Long, days: Int, maxBytes: Long): Int
    fun inventory(limit: Int): List<Pair<String, Long>>
    fun count(): Int

    fun addPeer(id: String, name: String, key: ByteArray, addr: String, now: Long)
    fun peerKey(id: String): ByteArray?
    fun peers(): List<Peer>
    fun renamePeer(id: String, alias: String)
    /** `addr` vazio mantém o endereço guardado. */
    fun touchPeer(id: String, addr: String, now: Long)
    fun removePeer(id: String): Boolean
}

/** Implementação em memória (mesmas regras do SQLite), usada nos testes do núcleo. */
class MemoryStore : ClipStore {
    private class R(var kind: String, var name: String, var mime: String, var size: Long, var text: String?, var ts: Long,
                    var pinned: Boolean, var deleted: Boolean, var deletedAt: Long, var origin: String)
    private class P(val id: String, var name: String, var alias: String, var key: ByteArray, var addr: String, var pairedAt: Long, var lastSeen: Long)

    private val items = LinkedHashMap<String, R>()
    private val blobs = HashMap<String, ByteArray>()
    private val peers = LinkedHashMap<String, P>()
    private val settings = HashMap<String, String>()

    @Synchronized override fun setting(k: String) = settings[k]
    @Synchronized override fun setSetting(k: String, v: String) { settings[k] = v }

    @Synchronized override fun upsert(id: String, n: NewItem, size: Long, ts: Long, blob: ByteArray?): Upsert {
        val cur = items[id]
        val res = StoreRules.decide(cur?.let { Row(it.ts, it.deleted, it.deletedAt) }, ts)
        when {
            cur == null -> items[id] = R(n.kind, n.name, n.mime, size, n.text, ts, false, false, 0, n.origin)
            res == Upsert.Inserted -> { cur.kind = n.kind; cur.name = n.name; cur.mime = n.mime; cur.size = size; cur.text = n.text; cur.ts = ts; cur.origin = n.origin; cur.deleted = false; cur.deletedAt = 0 }
            res == Upsert.Bumped -> { cur.ts = ts; cur.origin = n.origin }
        }
        if (res == Upsert.Inserted && blob != null) blobs[id] = blob
        return res
    }

    @Synchronized override fun hasRow(id: String) = items[id]?.let { maxOf(it.ts, it.deletedAt) }

    private fun view(id: String, r: R) = Item(id, r.kind, r.name, r.mime, r.size, r.text?.take(PREVIEW_CHARS), r.ts, r.pinned, r.origin)

    @Synchronized override fun history(query: String, kind: String?, limit: Int): List<Item> {
        val q = StoreRules.fold(query.trim())
        return items.entries.filter { (_, r) ->
            !r.deleted && (kind.isNullOrEmpty() || r.kind == kind) && (q.isEmpty() || StoreRules.searchKey(r.text, r.name).contains(q))
        }.sortedWith(compareByDescending<Map.Entry<String, R>> { it.value.pinned }.thenByDescending { it.value.ts })
            .take(limit).map { view(it.key, it.value) }
    }

    @Synchronized override fun get(id: String) = items[id]?.takeIf { !it.deleted }?.let { view(id, it) }
    @Synchronized override fun fullText(id: String) = items[id]?.takeIf { !it.deleted }?.text
    @Synchronized override fun blob(id: String) = blobs[id]
    @Synchronized override fun setPinned(id: String, pinned: Boolean) { items[id]?.takeIf { !it.deleted }?.pinned = pinned }

    @Synchronized override fun delete(id: String, now: Long) {
        items[id]?.let { it.deleted = true; it.deletedAt = now; it.text = null; it.pinned = false; it.name = ""; it.mime = "" }
        blobs.remove(id)
    }

    @Synchronized override fun clearUnpinned(now: Long): Int {
        val ids = items.filter { !it.value.deleted && !it.value.pinned }.keys.toList()
        ids.forEach { delete(it, now) }
        return ids.size
    }

    @Synchronized override fun purge(now: Long, days: Int, maxBytes: Long): Int {
        val cutoff = now - maxOf(days, 1) * DAY_MS
        var n = 0
        val it = items.entries.iterator()
        while (it.hasNext()) {
            val (_, r) = it.next()
            if ((!r.pinned && !r.deleted && r.ts < cutoff) || (r.deleted && r.deletedAt < cutoff)) { it.remove(); n++ }
        }
        blobs.keys.retainAll { items[it]?.deleted == false }
        if (maxBytes > 0) while (blobs.values.sumOf { it.size.toLong() } > maxBytes) {
            val victim = items.entries.filter { blobs.containsKey(it.key) && !it.value.pinned }.minByOrNull { it.value.ts } ?: break
            blobs.remove(victim.key); items.remove(victim.key); n++
        }
        return n
    }

    @Synchronized override fun inventory(limit: Int) = items.filter { !it.value.deleted }.entries.sortedByDescending { it.value.ts }.take(limit).map { it.key to it.value.ts }
    @Synchronized override fun count() = items.count { !it.value.deleted }

    @Synchronized override fun addPeer(id: String, name: String, key: ByteArray, addr: String, now: Long) {
        val p = peers[id]
        if (p == null) peers[id] = P(id, name, "", key, addr, now, 0) else { p.name = name; p.key = key; p.addr = addr; p.pairedAt = now }
    }
    @Synchronized override fun peerKey(id: String) = peers[id]?.key
    @Synchronized override fun peers() = peers.values.sortedBy { it.pairedAt }.map { Peer(it.id, it.name, it.alias, it.addr, it.pairedAt, it.lastSeen) }
    @Synchronized override fun renamePeer(id: String, alias: String) { peers[id]?.alias = alias.trim() }
    @Synchronized override fun touchPeer(id: String, addr: String, now: Long) { peers[id]?.let { it.lastSeen = now; if (addr.isNotEmpty()) it.addr = addr } }
    @Synchronized override fun removePeer(id: String) = peers.remove(id) != null
}
