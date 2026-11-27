package app.renanclip

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import app.renanclip.core.ClipStore
import app.renanclip.core.Item
import app.renanclip.core.NewItem
import app.renanclip.core.PREVIEW_CHARS
import app.renanclip.core.Peer
import app.renanclip.core.Row
import app.renanclip.core.StoreRules
import app.renanclip.core.Upsert
import app.renanclip.core.DAY_MS
import java.io.File

/**
 * Histórico em SQLite; conteúdo de imagem/arquivo em `filesDir/blobs/<id>`. As regras (lápides,
 * ressurreição, busca sem acento) são as de [StoreRules], as mesmas do MemoryStore testado.
 * A coluna `search` guarda o texto já sem acento/maiúscula, então a busca é um LIKE simples.
 */
class SqliteStore(ctx: Context) : ClipStore {
    private val blobDir = File(ctx.filesDir, "blobs").also { it.mkdirs() }

    private class Helper(ctx: Context) : SQLiteOpenHelper(ctx, "renanclip.db", null, 1) {
        override fun onConfigure(db: SQLiteDatabase) { db.setForeignKeyConstraintsEnabled(false) }
        override fun onCreate(db: SQLiteDatabase) {
            db.execSQL("CREATE TABLE items(id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, mime TEXT NOT NULL, size INTEGER NOT NULL, text TEXT, search TEXT NOT NULL, ts INTEGER NOT NULL, pinned INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER NOT NULL DEFAULT 0, origin TEXT NOT NULL)")
            db.execSQL("CREATE INDEX items_ts ON items(ts DESC)")
            db.execSQL("CREATE TABLE peers(id TEXT PRIMARY KEY, name TEXT NOT NULL, alias TEXT NOT NULL DEFAULT '', key BLOB NOT NULL, addr TEXT NOT NULL DEFAULT '', paired_at INTEGER NOT NULL, last_seen INTEGER NOT NULL DEFAULT 0)")
            db.execSQL("CREATE TABLE settings(k TEXT PRIMARY KEY, v TEXT NOT NULL)")
        }
        override fun onUpgrade(db: SQLiteDatabase, o: Int, n: Int) {}
    }

    private val db: SQLiteDatabase = Helper(ctx).writableDatabase

    private fun blobFile(id: String) = File(blobDir, id.filter { it.isLetterOrDigit() })

    @Synchronized override fun setting(k: String): String? =
        db.rawQuery("SELECT v FROM settings WHERE k=?", arrayOf(k)).use { if (it.moveToFirst()) it.getString(0) else null }

    @Synchronized override fun setSetting(k: String, v: String) {
        db.execSQL("INSERT OR REPLACE INTO settings(k,v) VALUES(?,?)", arrayOf(k, v))
    }

    @Synchronized override fun upsert(id: String, n: NewItem, size: Long, ts: Long, blob: ByteArray?): Upsert {
        val cur = db.rawQuery("SELECT ts, deleted, deleted_at FROM items WHERE id=?", arrayOf(id)).use {
            if (it.moveToFirst()) Row(it.getLong(0), it.getInt(1) != 0, it.getLong(2)) else null
        }
        val res = StoreRules.decide(cur, ts)
        when {
            cur == null || res == Upsert.Inserted -> {
                val v = ContentValues().apply {
                    put("id", id); put("kind", n.kind); put("name", n.name); put("mime", n.mime); put("size", size)
                    put("text", n.text); put("search", StoreRules.searchKey(n.text, n.name)); put("ts", ts)
                    put("deleted", 0); put("deleted_at", 0); put("origin", n.origin)
                    if (cur == null) put("pinned", 0)
                }
                if (cur == null) db.insertOrThrow("items", null, v) else db.update("items", v, "id=?", arrayOf(id))
                if (blob != null) writeBlob(id, blob)
            }
            res == Upsert.Bumped -> db.execSQL("UPDATE items SET ts=?, origin=? WHERE id=?", arrayOf(ts, n.origin, id))
        }
        return res
    }

    private fun writeBlob(id: String, data: ByteArray) {
        val f = blobFile(id)
        val tmp = File(blobDir, "${f.name}.tmp")
        tmp.writeBytes(data)
        if (!tmp.renameTo(f)) { f.delete(); tmp.renameTo(f) }
    }

    @Synchronized override fun hasRow(id: String): Long? =
        db.rawQuery("SELECT MAX(ts, deleted_at) FROM items WHERE id=?", arrayOf(id)).use { if (it.moveToFirst() && !it.isNull(0)) it.getLong(0) else null }

    private fun cols() = "id, kind, name, mime, size, substr(text,1,$PREVIEW_CHARS), ts, pinned, origin"
    private fun item(c: android.database.Cursor) =
        Item(c.getString(0), c.getString(1), c.getString(2), c.getString(3), c.getLong(4), if (c.isNull(5)) null else c.getString(5), c.getLong(6), c.getInt(7) != 0, c.getString(8))

    @Synchronized override fun history(query: String, kind: String?, limit: Int): List<Item> {
        val q = StoreRules.fold(query.trim())
        val where = StringBuilder("deleted=0")
        val args = ArrayList<String>()
        if (!kind.isNullOrEmpty()) { where.append(" AND kind=?"); args.add(kind) }
        if (q.isNotEmpty()) {
            where.append(" AND search LIKE ? ESCAPE '\\'")
            args.add("%" + q.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_") + "%")
        }
        args.add(limit.toString())
        return db.rawQuery("SELECT ${cols()} FROM items WHERE $where ORDER BY pinned DESC, ts DESC LIMIT ?", args.toTypedArray()).use { c ->
            val out = ArrayList<Item>(); while (c.moveToNext()) out.add(item(c)); out
        }
    }

    @Synchronized override fun get(id: String): Item? =
        db.rawQuery("SELECT ${cols()} FROM items WHERE id=? AND deleted=0", arrayOf(id)).use { if (it.moveToFirst()) item(it) else null }

    @Synchronized override fun fullText(id: String): String? =
        db.rawQuery("SELECT text FROM items WHERE id=? AND deleted=0", arrayOf(id)).use { if (it.moveToFirst() && !it.isNull(0)) it.getString(0) else null }

    @Synchronized override fun blob(id: String): ByteArray? = blobFile(id).takeIf { it.isFile }?.readBytes()

    @Synchronized override fun setPinned(id: String, pinned: Boolean) {
        db.execSQL("UPDATE items SET pinned=? WHERE id=? AND deleted=0", arrayOf(if (pinned) 1 else 0, id))
    }

    @Synchronized override fun delete(id: String, now: Long) {
        db.execSQL("UPDATE items SET deleted=1, deleted_at=?, text=NULL, search='', pinned=0, name='', mime='' WHERE id=?", arrayOf(now, id))
        blobFile(id).delete()
    }

    @Synchronized override fun clearUnpinned(now: Long): Int {
        val ids = db.rawQuery("SELECT id FROM items WHERE deleted=0 AND pinned=0", null).use { c -> val l = ArrayList<String>(); while (c.moveToNext()) l.add(c.getString(0)); l }
        db.beginTransaction()
        try { ids.forEach { delete(it, now) }; db.setTransactionSuccessful() } finally { db.endTransaction() }
        return ids.size
    }

    @Synchronized override fun purge(now: Long, days: Int, maxBytes: Long): Int {
        val cutoff = now - maxOf(days, 1) * DAY_MS
        var n = db.delete("items", "(pinned=0 AND deleted=0 AND ts<?) OR (deleted=1 AND deleted_at<?)", arrayOf(cutoff.toString(), cutoff.toString()))
        if (maxBytes > 0) {
            while (blobBytes() > maxBytes) {
                val victim = db.rawQuery("SELECT id FROM items WHERE deleted=0 AND pinned=0 AND kind<>'text' ORDER BY ts ASC LIMIT 1", null).use { if (it.moveToFirst()) it.getString(0) else null } ?: break
                db.delete("items", "id=?", arrayOf(victim)); n++
                blobFile(victim).delete()
            }
        }
        // arquivos órfãos (linha apagada, expirada ou nunca gravada)
        val live = db.rawQuery("SELECT id FROM items WHERE deleted=0 AND kind<>'text'", null).use { c -> val s = HashSet<String>(); while (c.moveToNext()) s.add(c.getString(0)); s }
        blobDir.listFiles()?.forEach { if (it.name !in live) it.delete() }
        return n
    }

    private fun blobBytes(): Long =
        db.rawQuery("SELECT COALESCE(SUM(size),0) FROM items WHERE deleted=0 AND kind<>'text'", null).use { if (it.moveToFirst()) it.getLong(0) else 0 }

    @Synchronized override fun inventory(limit: Int): List<Pair<String, Long>> =
        db.rawQuery("SELECT id, ts FROM items WHERE deleted=0 ORDER BY ts DESC LIMIT ?", arrayOf(limit.toString())).use { c ->
            val l = ArrayList<Pair<String, Long>>(); while (c.moveToNext()) l.add(c.getString(0) to c.getLong(1)); l
        }

    @Synchronized override fun count(): Int =
        db.rawQuery("SELECT COUNT(*) FROM items WHERE deleted=0", null).use { if (it.moveToFirst()) it.getInt(0) else 0 }

    @Synchronized override fun addPeer(id: String, name: String, key: ByteArray, addr: String, now: Long) {
        val exists = db.rawQuery("SELECT 1 FROM peers WHERE id=?", arrayOf(id)).use { it.moveToFirst() }
        if (exists) db.execSQL("UPDATE peers SET name=?, key=?, addr=?, paired_at=? WHERE id=?", arrayOf(name, key, addr, now, id))
        else db.execSQL("INSERT INTO peers(id,name,alias,key,addr,paired_at,last_seen) VALUES(?,?,?,?,?,?,0)", arrayOf(id, name, "", key, addr, now))
    }

    @Synchronized override fun peerKey(id: String): ByteArray? =
        db.rawQuery("SELECT key FROM peers WHERE id=?", arrayOf(id)).use { if (it.moveToFirst()) it.getBlob(0) else null }

    @Synchronized override fun peers(): List<Peer> =
        db.rawQuery("SELECT id, name, alias, addr, paired_at, last_seen FROM peers ORDER BY paired_at", null).use { c ->
            val l = ArrayList<Peer>(); while (c.moveToNext()) l.add(Peer(c.getString(0), c.getString(1), c.getString(2), c.getString(3), c.getLong(4), c.getLong(5))); l
        }

    @Synchronized override fun renamePeer(id: String, alias: String) {
        db.execSQL("UPDATE peers SET alias=? WHERE id=?", arrayOf(alias.trim(), id))
    }

    @Synchronized override fun touchPeer(id: String, addr: String, now: Long) {
        if (now > 0) db.execSQL("UPDATE peers SET last_seen=? WHERE id=?", arrayOf(now, id))
        if (addr.isNotEmpty()) db.execSQL("UPDATE peers SET addr=? WHERE id=?", arrayOf(addr, id))
    }

    @Synchronized override fun removePeer(id: String): Boolean = db.delete("peers", "id=?", arrayOf(id)) > 0
}
