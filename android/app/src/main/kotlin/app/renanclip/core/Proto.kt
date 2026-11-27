package app.renanclip.core

/** Mensagens da sessão (JSON dentro de quadros cifrados). Espelho de core/src/proto.rs. */
object Proto {
    const val T_JSON = 1
    const val T_CHUNK = 2 // id(16) || offset(u32 BE) || dados
    const val CHUNK = 256 * 1024
    const val MAX_TEXT = 1024 * 1024
    const val INVENTORY = 200
    /** Porta preferida do servidor (a mesma em todos os aparelhos, para achar pares pela tailnet). */
    const val DEFAULT_PORT = 47823
    const val SERVICE_TYPE = "_renanclip._tcp."
}

data class ClipMeta(
    val id: String, val ts: Long, val kind: String, val mime: String = "", val name: String = "",
    val size: Long = 0, val live: Boolean = false, val text: String? = null,
)

sealed class Msg {
    data class Inv(val items: List<Pair<String, Long>>) : Msg()
    data class Want(val ids: List<String>) : Msg()
    data class Clip(val meta: ClipMeta) : Msg()
    object Ping : Msg()
    object Pong : Msg()
    data class Bye(val revoked: Boolean) : Msg()

    fun toJson(): String = Json.write(
        when (this) {
            is Inv -> mapOf("t" to "inv", "items" to items.map { listOf(it.first, it.second) })
            is Want -> mapOf("t" to "want", "ids" to ids)
            is Clip -> LinkedHashMap<String, Any?>().apply {
                put("t", "clip"); put("id", meta.id); put("ts", meta.ts); put("kind", meta.kind); put("mime", meta.mime)
                put("name", meta.name); put("size", meta.size); put("live", meta.live)
                if (meta.text != null) put("text", meta.text)
            }
            Ping -> mapOf("t" to "ping")
            Pong -> mapOf("t" to "pong")
            is Bye -> mapOf("t" to "bye", "revoked" to revoked)
        },
    )

    companion object {
        fun parse(s: String): Msg {
            val m = Json.parseObject(s)
            return when (m["t"]) {
                "inv" -> Inv((m["items"] as? List<*> ?: throw ProtoException("inv")).map {
                    val p = it as? List<*> ?: throw ProtoException("inv")
                    (p[0] as String) to (p[1] as Number).toLong()
                })
                "want" -> Want((m["ids"] as? List<*> ?: throw ProtoException("want")).map { it as String })
                "clip" -> Clip(ClipMeta(
                    id = m["id"] as? String ?: throw ProtoException("clip"),
                    ts = (m["ts"] as? Number)?.toLong() ?: throw ProtoException("clip"),
                    kind = m["kind"] as? String ?: throw ProtoException("clip"),
                    mime = m["mime"] as? String ?: "",
                    name = m["name"] as? String ?: "",
                    size = (m["size"] as? Number)?.toLong() ?: 0,
                    live = m["live"] as? Boolean ?: false,
                    text = m["text"] as? String,
                ))
                "ping" -> Ping
                "pong" -> Pong
                "bye" -> Bye(m["revoked"] as? Boolean ?: false)
                else -> throw ProtoException("mensagem desconhecida")
            }
        }
    }
}

sealed class Payload {
    data class Text(val text: String) : Payload()
    class Image(val png: ByteArray) : Payload()
    class File(val name: String, val mime: String, val bytes: ByteArray) : Payload()
}

sealed class Event {
    object History : Event()
    object Peers : Event()
    /** Aplicar na área de transferência do sistema. */
    class Apply(val id: String, val payload: Payload) : Event()
    class PairPrompt(val pairId: Long, val peerName: String, val sas: String, val inbound: Boolean) : Event()
    class PairDone(val pairId: Long, val ok: Boolean, val peerName: String, val error: String) : Event()
}

class LocalCopy(
    val kind: String, // text | image | file
    val name: String = "", val mime: String = "", val text: String? = null, val bytes: ByteArray? = null,
    val sensitive: Boolean = false,
) {
    companion object {
        fun text(t: String) = LocalCopy("text", mime = "text/plain", text = t)
        fun image(png: ByteArray) = LocalCopy("image", mime = "image/png", bytes = png)
        fun file(name: String, mime: String, bytes: ByteArray) = LocalCopy("file", name = name, mime = mime, bytes = bytes)
    }
}

class Settings(
    val deviceName: String = "",
    val retentionDays: Int = 7,
    val paused: Boolean = false,
    val syncImages: Boolean = true,
    val syncFiles: Boolean = true,
    val maxBlobMb: Int = 10,
    val storeCapMb: Int = 500,
    val theme: String = "auto", // auto | claro | escuro
) {
    fun clamp() = Settings(deviceName, retentionDays.coerceIn(1, 3650), paused, syncImages, syncFiles, maxBlobMb.coerceIn(1, 100), storeCapMb.coerceIn(10, 10_000), theme)

    fun toJson(): String = Json.write(mapOf(
        "device_name" to deviceName, "retention_days" to retentionDays, "paused" to paused, "sync_images" to syncImages,
        "sync_files" to syncFiles, "max_blob_mb" to maxBlobMb, "store_cap_mb" to storeCapMb, "theme" to theme,
    ))

    companion object {
        fun fromJson(s: String?): Settings {
            val d = Settings()
            if (s == null) return d
            return try {
                val m = Json.parseObject(s)
                Settings(
                    deviceName = m["device_name"] as? String ?: d.deviceName,
                    retentionDays = (m["retention_days"] as? Number)?.toInt() ?: d.retentionDays,
                    paused = m["paused"] as? Boolean ?: d.paused,
                    syncImages = m["sync_images"] as? Boolean ?: d.syncImages,
                    syncFiles = m["sync_files"] as? Boolean ?: d.syncFiles,
                    maxBlobMb = (m["max_blob_mb"] as? Number)?.toInt() ?: d.maxBlobMb,
                    storeCapMb = (m["store_cap_mb"] as? Number)?.toInt() ?: d.storeCapMb,
                    theme = m["theme"] as? String ?: d.theme,
                ).clamp()
            } catch (e: Exception) { d }
        }
    }
}
