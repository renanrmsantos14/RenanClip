package app.renanclip.core

import java.math.BigInteger
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.PrivateKey
import java.security.SecureRandom
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPrivateKeySpec
import java.security.spec.ECPublicKeySpec
import javax.crypto.Cipher
import javax.crypto.KeyAgreement
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

open class RcException(msg: String) : Exception(msg)
class CryptoException(msg: String) : RcException("cripto: $msg")
class ProtoException(msg: String) : RcException("protocolo: $msg")

/** Primitivas equivalentes às de core/src/crypto.rs, só com java.security/javax.crypto. */
object Crypto {
    private val rng = SecureRandom()

    fun rand(n: Int): ByteArray = ByteArray(n).also { rng.nextBytes(it) }

    fun sha256(vararg parts: ByteArray): ByteArray {
        val md = MessageDigest.getInstance("SHA-256")
        for (p in parts) md.update(p)
        return md.digest()
    }

    private fun mac(key: ByteArray): Mac = Mac.getInstance("HmacSHA256").apply {
        init(SecretKeySpec(if (key.isEmpty()) ByteArray(32) else key, "HmacSHA256"))
    }

    fun hmac(key: ByteArray, vararg parts: ByteArray): ByteArray {
        val m = mac(key)
        for (p in parts) m.update(p)
        return m.doFinal()
    }

    fun hmacVerify(key: ByteArray, parts: List<ByteArray>, tag: ByteArray): Boolean =
        MessageDigest.isEqual(hmac(key, *parts.toTypedArray()), tag)

    /** HKDF-SHA256 (RFC 5869). Salt vazio equivale a 32 zeros, como no crate hkdf. */
    fun hkdf(salt: ByteArray, ikm: ByteArray, info: ByteArray, len: Int): ByteArray {
        val prk = hmac(if (salt.isEmpty()) ByteArray(32) else salt, ikm)
        val out = ByteArray(len)
        var t = ByteArray(0)
        var pos = 0
        var ctr = 1
        while (pos < len) {
            t = hmac(prk, t, info, byteArrayOf(ctr.toByte()))
            val n = minOf(t.size, len - pos)
            System.arraycopy(t, 0, out, pos, n)
            pos += n; ctr++
        }
        return out
    }

    /** Chaves de sessão (cliente→servidor, servidor→cliente). */
    fun sessionKeys(pairKey: ByteArray, nClient: ByteArray, nServer: ByteArray): Pair<ByteArray, ByteArray> {
        val okm = hkdf(nClient + nServer, pairKey, "RCLP session v1".toByteArray(), 64)
        return okm.copyOfRange(0, 32) to okm.copyOfRange(32, 64)
    }

    fun contentId(kind: String, name: String, payload: ByteArray): String =
        hex(sha256(kind.toByteArray(), byteArrayOf(0), name.toByteArray(), byteArrayOf(0), payload).copyOf(16))

    // ---------- hex ----------
    private const val H = "0123456789abcdef"
    fun hex(b: ByteArray): String {
        val sb = StringBuilder(b.size * 2)
        for (x in b) { sb.append(H[(x.toInt() shr 4) and 15]); sb.append(H[x.toInt() and 15]) }
        return sb.toString()
    }

    fun unhex(s: String): ByteArray? {
        if (s.length % 2 != 0) return null
        val out = ByteArray(s.length / 2)
        for (i in out.indices) {
            val a = Character.digit(s[2 * i], 16); val b = Character.digit(s[2 * i + 1], 16)
            if (a < 0 || b < 0) return null
            out[i] = (a shl 4 or b).toByte()
        }
        return out
    }

    fun unhexN(s: String, n: Int): ByteArray {
        val v = unhex(s) ?: throw ProtoException("hex inválido")
        if (v.size != n) throw ProtoException("tamanho inválido")
        return v
    }

    // ---------- ECDH P-256 ----------
    val p256: ECParameterSpec by lazy {
        AlgorithmParameters.getInstance("EC").apply { init(ECGenParameterSpec("secp256r1")) }
            .getParameterSpec(ECParameterSpec::class.java)
    }

    private fun be32(v: BigInteger): ByteArray {
        val b = v.toByteArray()
        return when {
            b.size == 32 -> b
            b.size > 32 -> b.copyOfRange(b.size - 32, b.size)
            else -> ByteArray(32 - b.size) + b
        }
    }

    /** Ponto no formato SEC1 não comprimido (65 bytes). */
    fun encodePoint(p: ECPoint): ByteArray = byteArrayOf(4) + be32(p.affineX) + be32(p.affineY)
}

/** Par ECDH P-256 efêmero. A chave pública é SEC1 não comprimida (65 bytes). */
class Ephemeral(private val priv: PrivateKey, val pub: ByteArray) {
    /** Coordenada x do segredo compartilhado (32 bytes). */
    fun agree(peerPub: ByteArray): ByteArray {
        if (peerPub.size != 65 || peerPub[0].toInt() != 4) throw CryptoException("chave pública")
        try {
            val x = BigInteger(1, peerPub.copyOfRange(1, 33))
            val y = BigInteger(1, peerPub.copyOfRange(33, 65))
            val pk = KeyFactory.getInstance("EC").generatePublic(ECPublicKeySpec(ECPoint(x, y), Crypto.p256))
            val ka = KeyAgreement.getInstance("ECDH")
            ka.init(priv)
            ka.doPhase(pk, true)
            val s = ka.generateSecret()
            if (s.size != 32) throw CryptoException("segredo compartilhado")
            return s
        } catch (e: RcException) {
            throw e
        } catch (e: Exception) {
            throw CryptoException("ponto inválido")
        }
    }

    companion object {
        fun generate(): Ephemeral {
            val g = KeyPairGenerator.getInstance("EC")
            g.initialize(ECGenParameterSpec("secp256r1"))
            val kp = g.generateKeyPair()
            return Ephemeral(kp.private, Crypto.encodePoint((kp.public as ECPublicKey).w))
        }

        /** Para vetores de teste: escalar fixo e chave pública calculada. */
        fun fromScalar(scalar: ByteArray): Ephemeral {
            val d = BigInteger(1, scalar)
            val priv = KeyFactory.getInstance("EC").generatePrivate(ECPrivateKeySpec(d, Crypto.p256))
            return Ephemeral(priv, Crypto.encodePoint(P256Math.multiplyBase(d)))
        }
    }
}

/** Multiplicação escalar em afim (só para derivar a chave pública de um escalar fixo nos vetores). */
internal object P256Math {
    private val p = BigInteger("ffffffff00000001000000000000000000000000ffffffffffffffffffffffff", 16)
    private val a = p - BigInteger.valueOf(3)
    private val g = ECPoint(
        BigInteger("6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296", 16),
        BigInteger("4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5", 16),
    )

    private fun add(p1: ECPoint?, p2: ECPoint?): ECPoint? {
        if (p1 == null) return p2
        if (p2 == null) return p1
        val l = if (p1.affineX == p2.affineX) {
            if (p1.affineY != p2.affineY || p1.affineY.signum() == 0) return null
            (p1.affineX.pow(2) * BigInteger.valueOf(3) + a) * (p1.affineY * BigInteger.TWO).modInverse(p) % p
        } else {
            (p2.affineY - p1.affineY) * (p2.affineX - p1.affineX).modInverse(p) % p
        }
        val x = (l * l - p1.affineX - p2.affineX).mod(p)
        val y = (l * (p1.affineX - x) - p1.affineY).mod(p)
        return ECPoint(x, y)
    }

    fun multiplyBase(k: BigInteger): ECPoint {
        var r: ECPoint? = null
        var q: ECPoint? = g
        for (i in 0 until k.bitLength()) {
            if (k.testBit(i)) r = add(r, q)
            q = add(q, q)
        }
        return r ?: throw CryptoException("escalar")
    }
}

enum class Dir(val code: Int) { ClientToServer(1), ServerToClient(2) }

/** Uma direção cifrada: nonce = 4 zeros || contador u64 BE; AAD = "RCLP" || dir. */
class Seal(key: ByteArray, private val dir: Dir) {
    private val k = SecretKeySpec(key, "AES")
    private var ctr = 0L

    private fun cipher(mode: Int): Cipher {
        val n = ByteArray(12)
        for (i in 0 until 8) n[11 - i] = (ctr ushr (8 * i)).toByte()
        return Cipher.getInstance("AES/GCM/NoPadding").apply {
            init(mode, k, GCMParameterSpec(128, n))
            updateAAD(byteArrayOf('R'.code.toByte(), 'C'.code.toByte(), 'L'.code.toByte(), 'P'.code.toByte(), dir.code.toByte()))
        }
    }

    fun seal(plain: ByteArray): ByteArray {
        val out = try { cipher(Cipher.ENCRYPT_MODE).doFinal(plain) } catch (e: Exception) { throw CryptoException("cifrar") }
        ctr++
        return out
    }

    fun open(ct: ByteArray): ByteArray {
        val out = try { cipher(Cipher.DECRYPT_MODE).doFinal(ct) } catch (e: Exception) { throw CryptoException("decifrar") }
        ctr++
        return out
    }
}
