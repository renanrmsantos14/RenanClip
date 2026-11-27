package app.renanclip.core

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolTest {
    // ---------- JSON, mensagens e quadros ----------
    @Test fun json_ida_e_volta_com_acentos_e_escapes() {
        val m = mapOf("t" to "x", "s" to "olá \"q\" \\ \n\t\u0001 😀", "n" to 5L, "l" to listOf(1L, "a", null, true))
        assertEquals(m, Json.parseObject(Json.write(m)))
        assertEquals("aéb", Json.parse("\"a\\u00e9b\""))
    }

    @Test fun mensagens_tem_o_formato_do_serde() {
        assertEquals("{\"t\":\"ping\"}", Msg.Ping.toJson())
        assertEquals("{\"t\":\"want\",\"ids\":[\"a\",\"b\"]}", Msg.Want(listOf("a", "b")).toJson())
        assertEquals("{\"t\":\"inv\",\"items\":[[\"a\",5]]}", Msg.Inv(listOf("a" to 5L)).toJson())
        assertEquals("{\"t\":\"bye\",\"revoked\":true}", Msg.Bye(true).toJson())
        val clip = Msg.Clip(ClipMeta("i", 9, "text", "text/plain", "", 3, true, "oi"))
        assertEquals(clip, Msg.parse(clip.toJson()))
        assertEquals(Msg.Pong, Msg.parse("{\"t\":\"pong\"}"))
        // o Rust omite campos com default e ignora os desconhecidos
        assertEquals(Msg.Clip(ClipMeta("i", 1, "text", text = "x")), Msg.parse("{\"t\":\"clip\",\"id\":\"i\",\"ts\":1,\"kind\":\"text\",\"text\":\"x\",\"extra\":1}"))
    }

    @Test fun quadros_preambulo_e_limite() {
        val o = ByteArrayOutputStream()
        Frame.writePreamble(o, Frame.MODE_PAIR)
        Frame.write(o, "ola".toByteArray())
        assertEquals("RCLP\u0001\u0001\u0000\u0000\u0000\u0003ola", String(o.toByteArray(), Charsets.ISO_8859_1))
        val i = ByteArrayInputStream(o.toByteArray())
        assertEquals(Frame.MODE_PAIR, Frame.readPreamble(i))
        assertEquals("ola", String(Frame.read(i, 100)))
        assertTrue(runCatching { Frame.read(ByteArrayInputStream(byteArrayOf(0, 0, 0, 3, 1, 2, 3)), 2) }.isFailure)
        assertTrue(runCatching { Frame.readPreamble(ByteArrayInputStream("GET / HTTP/1.1".toByteArray())) }.isFailure)
    }

    @Test fun resolver_porta_padrao_e_explicita() {
        assertEquals(47823, Resolver.resolve("127.0.0.1").port)
        assertEquals(1234, Resolver.resolve("127.0.0.1:1234").port)
        assertEquals(9, Resolver.resolve("[::1]:9").port)
        assertTrue(runCatching { Resolver.resolve("127.0.0.1:abc") }.isFailure)
        assertTrue(runCatching { Resolver.resolve("") }.isFailure)
        assertTrue(runCatching { Resolver.resolve("nome-que-nao-existe.invalid") }.isFailure)
    }

    @Test fun busca_sem_acento_e_regras_de_lapide() {
        val s = MemoryStore()
        val n = NewItem("text", "", "text/plain", "Reunião às 15h – ÁGUA", "a")
        assertEquals(Upsert.Inserted, s.upsert("a".repeat(32), n, 10, 100, null))
        assertEquals(1, s.history("reuniao as 15h", null, 10).size)
        assertEquals(1, s.history("AGUA", null, 10).size)
        assertEquals(Upsert.Unchanged, s.upsert("a".repeat(32), n, 10, 100, null))
        assertEquals(Upsert.Bumped, s.upsert("a".repeat(32), n, 10, 200, null))
        s.delete("a".repeat(32), 300)
        assertEquals(0, s.history("", null, 10).size)
        assertEquals(Upsert.Unchanged, s.upsert("a".repeat(32), n, 10, 250, null)) // lápide impede ressuscitar
        assertEquals(Upsert.Inserted, s.upsert("a".repeat(32), n, 10, 400, null))
    }

    // ---------- pareamento (soquete real em loopback) ----------
    private class Side(val res: Result<Paired>, val sas: String)

    private fun runPair(acceptI: Boolean, acceptR: Boolean): Pair<Side, Side> {
        val srv = ServerSocket(0, 1, java.net.InetAddress.getLoopbackAddress())
        var r: Side? = null
        val th = Thread {
            val s = srv.accept()
            var sas = ""
            val res = runCatching {
                Pairing.respond(s.getInputStream(), s.getOutputStream(), ByteArray(16) { 2 }, "Celular") { _, _, c -> sas = c; acceptR }
            }
            r = Side(res, sas); s.close()
        }
        th.start()
        val c = Socket(java.net.InetAddress.getLoopbackAddress(), srv.localPort)
        var sasI = ""
        val resI = runCatching {
            Pairing.initiate(c.getInputStream(), c.getOutputStream(), ByteArray(16) { 1 }, "PC") { _, _, x -> sasI = x; acceptI }
        }
        th.join(10_000); c.close(); srv.close()
        return Side(resI, sasI) to r!!
    }

    @Test fun pareamento_mesmo_codigo_e_mesma_chave() {
        val (i, r) = runPair(true, true)
        assertEquals(i.sas, r.sas); assertEquals(6, i.sas.length)
        val pi = i.res.getOrThrow(); val pr = r.res.getOrThrow()
        assertEquals(Crypto.hex(pi.key), Crypto.hex(pr.key))
        assertEquals("Celular", pi.peerName); assertEquals("PC", pr.peerName)
        assertEquals(Crypto.hex(ByteArray(16) { 2 }), Crypto.hex(pi.peerId))
    }

    @Test fun pareamento_recusa_de_um_lado_falha_os_dois_e_chaves_sao_novas() {
        var (i, r) = runPair(true, false); assertTrue(i.res.isFailure && r.res.isFailure)
        i = runPair(false, true).first; assertTrue(i.res.isFailure)
        val a = runPair(true, true).first.res.getOrThrow().key; val b = runPair(true, true).first.res.getOrThrow().key
        assertNotEquals(Crypto.hex(a), Crypto.hex(b))
    }

    @Test fun mitm_ve_codigos_e_chaves_diferentes() {
        val iE = Ephemeral.generate(); val rE = Ephemeral.generate(); val m1 = Ephemeral.generate(); val m2 = Ephemeral.generate()
        val id1 = ByteArray(16) { 1 }; val id2 = ByteArray(16) { 2 }
        val tI = Transcript(id1, id2, iE.pub, m1.pub, Crypto.rand(16), Crypto.rand(16))
        val tR = Transcript(id1, id2, m2.pub, rE.pub, Crypto.rand(16), Crypto.rand(16))
        val kI = tI.pairKey(iE.agree(m1.pub)); val kR = tR.pairKey(rE.agree(m2.pub))
        assertNotEquals(Crypto.hex(kI), Crypto.hex(kR))
        assertTrue(!tR.confirmOk(kR, 'I', tI.confirmMac(kI, 'I')))
    }

    // ---------- duas instâncias completas: pareamento + sessão + sincronização ----------
    private class Node(name: String) {
        val store = MemoryStore()
        val events = CopyOnWriteArrayList<Event>()
        lateinit var engine: Engine
        val applied = CopyOnWriteArrayList<Payload>()
        init {
            engine = Engine(store, name, 0) { e ->
                events.add(e)
                if (e is Event.Apply) applied.add(e.payload)
                if (e is Event.PairPrompt) engine.pairDecide(e.pairId, true)
            }
            engine.start()
        }
    }

    private fun waitFor(what: String, ms: Long = 15_000, c: () -> Boolean) {
        val end = System.currentTimeMillis() + ms
        while (System.currentTimeMillis() < end) { if (c()) return; Thread.sleep(20) }
        throw AssertionError("tempo esgotado: $what")
    }

    @Test fun duas_instancias_pareiam_conectam_e_sincronizam() {
        val a = Node("PC"); val b = Node("Celular")
        try {
            a.engine.openPairing(30)
            b.engine.pairWith("127.0.0.1:${a.engine.port}")
            waitFor("pareados") { a.store.peers().size == 1 && b.store.peers().size == 1 }
            assertEquals("127.0.0.1:${a.engine.port}", b.store.peers()[0].addr) // endereço digitado guardado
            assertEquals(Crypto.hex(a.store.peerKey(b.engine.myHex)!!), Crypto.hex(b.store.peerKey(a.engine.myHex)!!))
            waitFor("online") { a.engine.onlineCount() == 1 && b.engine.onlineCount() == 1 }

            // texto ao vivo: B aplica
            val id = a.engine.localCopy(LocalCopy.text("olá, mundo"))!!
            waitFor("texto aplicado") { b.applied.isNotEmpty() }
            assertEquals(Payload.Text("olá, mundo"), b.applied[0])
            assertEquals(1, b.engine.history("OLA", null, 10).size)
            assertNull(a.engine.localCopy(LocalCopy.text("   "))) // vazio ignorado
            assertNull(a.engine.localCopy(LocalCopy("text", text = "segredo", sensitive = true)))

            // imagem de 600 KiB (3 chunks) e arquivo
            val img = ByteArray(600 * 1024) { (it * 31).toByte() }
            val imgId = a.engine.localCopy(LocalCopy.image(img))!!
            waitFor("imagem") { b.applied.any { it is Payload.Image } }
            assertTrue((b.applied.first { it is Payload.Image } as Payload.Image).png.contentEquals(img))
            a.engine.localCopy(LocalCopy.file("a.txt", "text/plain", "abc".toByteArray()))
            waitFor("arquivo") { b.applied.any { it is Payload.File } }
            assertEquals("a.txt", (b.applied.first { it is Payload.File } as Payload.File).name)
            assertEquals(imgId, b.store.history("", "image", 5)[0].id)

            // limite de blob: acima de 10 MB é ignorado
            assertNull(a.engine.localCopy(LocalCopy.image(ByteArray(11 * 1024 * 1024))))

            // lápide: apagar em A não deixa B ressuscitar e o item some do histórico de A
            a.engine.delete(id)
            assertNull(a.store.get(id))
            assertNotNull(b.store.get(id))
        } finally { a.engine.shutdown(); b.engine.shutdown() }
    }

    @Test fun historico_antigo_chega_por_inv_want_sem_aplicar() {
        val a = Node("PC"); val b = Node("Celular")
        try {
            a.engine.localCopy(LocalCopy.text("antigo 1")); a.engine.localCopy(LocalCopy.text("antigo 2"))
            a.engine.openPairing(30)
            b.engine.pairWith("127.0.0.1:${a.engine.port}")
            waitFor("histórico reposto") { b.store.count() == 2 }
            assertTrue(b.applied.isEmpty()) // live=false: só guarda
        } finally { a.engine.shutdown(); b.engine.shutdown() }
    }

    @Test fun revogar_derruba_e_apaga_a_chave_dos_dois_lados() {
        val a = Node("PC"); val b = Node("Celular")
        try {
            a.engine.openPairing(30)
            b.engine.pairWith("127.0.0.1:${a.engine.port}")
            waitFor("online") { a.engine.onlineCount() == 1 && b.engine.onlineCount() == 1 }
            a.engine.revokePeer(b.engine.myHex)
            waitFor("revogado nos dois") { b.store.peers().isEmpty() && a.store.peers().isEmpty() }
        } finally { a.engine.shutdown(); b.engine.shutdown() }
    }

    @Test fun pareamento_sem_janela_aberta_e_ignorado() {
        val a = Node("PC"); val b = Node("Celular")
        try {
            b.engine.pairWith("127.0.0.1:${a.engine.port}")
            waitFor("falha") { b.events.any { it is Event.PairDone && !it.ok } }
            assertTrue(b.store.peers().isEmpty())
        } finally { a.engine.shutdown(); b.engine.shutdown() }
    }
}
