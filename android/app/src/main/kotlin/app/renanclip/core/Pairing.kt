package app.renanclip.core

import app.renanclip.core.Crypto.hex
import app.renanclip.core.Crypto.unhex
import app.renanclip.core.Crypto.unhexN
import java.io.InputStream
import java.io.OutputStream

/**
 * Pareamento com código de 6 dígitos mostrado nos DOIS aparelhos (comparação numérica com
 * compromisso). Espelho de core/src/pairing.rs; mesmos bytes no fio.
 */
class Paired(val peerId: ByteArray, val peerName: String, val key: ByteArray)

class Transcript(
    val iId: ByteArray, val rId: ByteArray, val iPub: ByteArray, val rPub: ByteArray,
    val nI: ByteArray, val nR: ByteArray,
) {
    private fun ascii(s: String) = s.toByteArray(Charsets.US_ASCII)

    fun hash(): ByteArray = Crypto.sha256(ascii("RCLP-pair"), iId, rId, iPub, rPub, nI, nR)

    /** Código de 6 dígitos exibido aos dois lados. */
    fun sas(): String {
        val h = Crypto.sha256(ascii("RCLP-sas"), hash())
        val v = ((h[0].toLong() and 255) shl 24 or ((h[1].toLong() and 255) shl 16) or ((h[2].toLong() and 255) shl 8) or (h[3].toLong() and 255)) % 1_000_000
        return "%06d".format(java.util.Locale.ROOT, v)
    }

    fun pairKey(shared: ByteArray): ByteArray = Crypto.hkdf(hash(), shared, ascii("RCLP pair key"), 32)

    fun confirmMac(key: ByteArray, role: Char): ByteArray = Crypto.hmac(key, ascii("RCLP-confirm"), byteArrayOf(role.code.toByte()), hash())

    fun confirmOk(key: ByteArray, role: Char, mac: ByteArray): Boolean =
        Crypto.hmacVerify(key, listOf(ascii("RCLP-confirm"), byteArrayOf(role.code.toByte()), hash()), mac)
}

object Pairing {
    const val MAX_PAIR_FRAME = 4096

    fun commit(iPub: ByteArray, nI: ByteArray): ByteArray = Crypto.sha256("RCLP-commit".toByteArray(), iPub, nI)

    /** Bloqueia até o usuário aceitar ou recusar: (idHex do par, nome do par, código). */
    fun interface Decide { fun decide(peerId: String, peerName: String, sas: String): Boolean }

    private fun send(o: OutputStream, m: Map<String, Any?>) = Frame.write(o, Json.write(m).toByteArray())

    private fun recv(i: InputStream): Map<String, Any?> = Json.parseObject(String(Frame.read(i, MAX_PAIR_FRAME), Charsets.UTF_8))

    private fun str(m: Map<String, Any?>, k: String): String = m[k] as? String ?: throw ProtoException("campo $k ausente")

    fun initiate(i: InputStream, o: OutputStream, myId: ByteArray, myName: String, decide: Decide): Paired {
        val eph = Ephemeral.generate()
        val iPub = eph.pub
        val nI = Crypto.rand(16)
        send(o, mapOf("t" to "p1", "id" to hex(myId), "name" to myName, "commit" to hex(commit(iPub, nI))))
        val p2 = recv(i)
        when (p2["t"]) {
            "p2" -> {}
            "p5" -> throw ProtoException("recusado pelo outro aparelho")
            else -> throw ProtoException("esperava p2")
        }
        val rId = unhexN(str(p2, "id"), 16)
        val rName = str(p2, "name")
        val rPub = unhexN(str(p2, "pb"), 65)
        val nR = unhexN(str(p2, "n"), 16)
        send(o, mapOf("t" to "p3", "pb" to hex(iPub), "n" to hex(nI)))
        val shared = eph.agree(rPub)
        val t = Transcript(myId, rId, iPub, rPub, nI, nR)
        return finish(i, o, t, shared, 'I', rId, rName, decide)
    }

    fun respond(i: InputStream, o: OutputStream, myId: ByteArray, myName: String, decide: Decide): Paired {
        val p1 = recv(i)
        if (p1["t"] != "p1") throw ProtoException("esperava p1")
        val iId = unhexN(str(p1, "id"), 16)
        val iName = str(p1, "name")
        val wantCommit = unhexN(str(p1, "commit"), 32)
        val eph = Ephemeral.generate()
        val rPub = eph.pub
        val nR = Crypto.rand(16)
        send(o, mapOf("t" to "p2", "id" to hex(myId), "name" to myName, "pb" to hex(rPub), "n" to hex(nR)))
        val p3 = recv(i)
        if (p3["t"] != "p3") throw ProtoException("esperava p3")
        val iPub = unhexN(str(p3, "pb"), 65)
        val nI = unhexN(str(p3, "n"), 16)
        if (!java.security.MessageDigest.isEqual(commit(iPub, nI), wantCommit)) throw ProtoException("compromisso não confere")
        val shared = eph.agree(iPub)
        val t = Transcript(iId, myId, iPub, rPub, nI, nR)
        return finish(i, o, t, shared, 'R', iId, iName, decide)
    }

    private fun finish(i: InputStream, o: OutputStream, t: Transcript, shared: ByteArray, role: Char, peerId: ByteArray, peerName: String, decide: Decide): Paired {
        val key = t.pairKey(shared)
        val sas = t.sas()
        if (!decide.decide(hex(peerId), peerName, sas)) {
            runCatching { send(o, mapOf("t" to "p5")) }
            throw ProtoException("pareamento recusado")
        }
        send(o, mapOf("t" to "p4", "mac" to hex(t.confirmMac(key, role))))
        val peerRole = if (role == 'I') 'R' else 'I'
        val m = recv(i)
        when (m["t"]) {
            "p4" -> {
                val mac = unhex(str(m, "mac")) ?: throw ProtoException("mac")
                if (!t.confirmOk(key, peerRole, mac)) throw ProtoException("confirmação inválida (chaves diferentes)")
            }
            "p5" -> throw ProtoException("recusado pelo outro aparelho")
            else -> throw ProtoException("esperava p4")
        }
        return Paired(peerId, peerName, key)
    }
}
