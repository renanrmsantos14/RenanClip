package app.renanclip.core

import app.renanclip.core.Crypto.hex
import app.renanclip.core.Crypto.unhex
import java.io.File
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Prova a compatibilidade byte a byte com o Rust usando docs/vetores.json (gerado pelo núcleo Rust). */
class VectorsTest {
    @Suppress("UNCHECKED_CAST")
    private val v: Map<String, Map<String, Any?>> = run {
        val f = listOf("../../docs/vetores.json", "../docs/vetores.json", "docs/vetores.json").map { File(it) }.first { it.exists() }
        Json.parseObject(f.readText()) as Map<String, Map<String, Any?>>
    }

    private fun b(sec: String, k: String) = unhex(v[sec]!![k] as String)!!
    private fun s(sec: String, k: String) = v[sec]!![k] as String

    @Test fun ecdh_publicas_e_segredo_compartilhado() {
        val a = Ephemeral.fromScalar(b("ecdh", "a_scalar"))
        val bb = Ephemeral.fromScalar(b("ecdh", "b_scalar"))
        assertEquals(s("ecdh", "a_pub"), hex(a.pub))
        assertEquals(s("ecdh", "b_pub"), hex(bb.pub))
        assertEquals(s("ecdh", "shared"), hex(a.agree(bb.pub)))
        assertEquals(s("ecdh", "shared"), hex(bb.agree(a.pub)))
    }

    @Test fun ecdh_aleatorio_concorda_e_rejeita_lixo() {
        val a = Ephemeral.generate(); val c = Ephemeral.generate()
        assertArrayEquals(a.agree(c.pub), c.agree(a.pub))
        assertEquals(65, a.pub.size)
        runCatching { a.agree(ByteArray(65) { 4 }); error("deveria falhar") }.exceptionOrNull().let { assertTrue(it is CryptoException) }
        runCatching { a.agree(ByteArray(10)); error("deveria falhar") }.exceptionOrNull().let { assertTrue(it is CryptoException) }
    }

    private fun transcript() = Transcript(
        b("pairing", "i_id"), b("pairing", "r_id"),
        Ephemeral.fromScalar(b("ecdh", "a_scalar")).pub, Ephemeral.fromScalar(b("ecdh", "b_scalar")).pub,
        b("pairing", "n_i"), b("pairing", "n_r"),
    )

    @Test fun pareamento_commit_transcricao_sas_chave_e_confirmacoes() {
        val t = transcript()
        assertEquals(s("pairing", "commit"), hex(Pairing.commit(t.iPub, t.nI)))
        assertEquals(s("pairing", "transcript_hash"), hex(t.hash()))
        assertEquals(s("pairing", "sas"), t.sas())
        val shared = b("ecdh", "shared")
        val key = t.pairKey(shared)
        assertEquals(s("pairing", "pair_key"), hex(key))
        assertEquals(s("pairing", "confirm_i"), hex(t.confirmMac(key, 'I')))
        assertEquals(s("pairing", "confirm_r"), hex(t.confirmMac(key, 'R')))
        assertTrue(t.confirmOk(key, 'I', b("pairing", "confirm_i")))
        assertFalse(t.confirmOk(key, 'R', b("pairing", "confirm_i")))
    }

    @Test fun chaves_de_sessao() {
        val (c2s, s2c) = Crypto.sessionKeys(b("pairing", "pair_key"), b("session", "n_client"), b("session", "n_server"))
        assertEquals(s("session", "c2s"), hex(c2s))
        assertEquals(s("session", "s2c"), hex(s2c))
    }

    @Suppress("UNCHECKED_CAST")
    private fun list(k: String) = (v["session"]!![k] as List<String>)

    @Test fun quadros_aes_gcm_do_rust_decifram_e_o_kotlin_gera_os_mesmos_bytes() {
        val c2s = b("session", "c2s"); val s2c = b("session", "s2c")
        // decifrar o que o Rust cifrou
        val rx = Seal(c2s, Dir.ClientToServer)
        list("c2s_frames").zip(list("plain_c2s")).forEach { (f, p) -> assertEquals(p, String(rx.open(unhex(f)!!))) }
        val rx2 = Seal(s2c, Dir.ServerToClient)
        list("s2c_frames").zip(list("plain_s2c")).forEach { (f, p) -> assertEquals(p, String(rx2.open(unhex(f)!!))) }
        // gerar os mesmos bytes
        val tx = Seal(c2s, Dir.ClientToServer)
        list("plain_c2s").zip(list("c2s_frames")).forEach { (p, f) -> assertEquals(f, hex(tx.seal(p.toByteArray()))) }
        val tx2 = Seal(s2c, Dir.ServerToClient)
        list("plain_s2c").zip(list("s2c_frames")).forEach { (p, f) -> assertEquals(f, hex(tx2.seal(p.toByteArray()))) }
    }

    @Test fun quadro_ordem_direcao_chave_e_adulteracao() {
        val k = ByteArray(32) { 7 }
        val tx = Seal(k, Dir.ClientToServer)
        val c1 = tx.seal("um".toByteArray()); val c2 = tx.seal("dois".toByteArray())
        assertTrue(runCatching { Seal(k, Dir.ClientToServer).open(c2) }.isFailure) // fora de ordem
        val rx = Seal(k, Dir.ClientToServer)
        assertEquals("um", String(rx.open(c1))); assertEquals("dois", String(rx.open(c2)))
        assertTrue(runCatching { rx.open(c1) }.isFailure) // repetição
        assertTrue(runCatching { Seal(k, Dir.ServerToClient).open(c1) }.isFailure) // reflexão
        assertTrue(runCatching { Seal(ByteArray(32), Dir.ClientToServer).open(c1) }.isFailure) // chave errada
        val bad = c1.copyOf().also { it[0] = (it[0].toInt() xor 1).toByte() }
        assertTrue(runCatching { Seal(k, Dir.ClientToServer).open(bad) }.isFailure) // adulterado
    }

    @Test fun ids_de_conteudo() {
        assertEquals(s("content_id", "text_ola"), Crypto.contentId("text", "", "olá".toByteArray()))
        assertEquals(s("content_id", "file_a_txt"), Crypto.contentId("file", "a.txt", "abc".toByteArray()))
        assertEquals(s("content_id", "image_png"), Crypto.contentId("image", "", byteArrayOf(1, 2, 3)))
    }
}
