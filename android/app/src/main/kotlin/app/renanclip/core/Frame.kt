package app.renanclip.core

import java.io.DataInputStream
import java.io.InputStream
import java.io.OutputStream

/** Enquadramento TCP: `u32 BE tamanho || bytes`. Preâmbulo: "RCLP" || versão || modo. */
object Frame {
    val MAGIC = byteArrayOf('R'.code.toByte(), 'C'.code.toByte(), 'L'.code.toByte(), 'P'.code.toByte())
    const val VERSION = 1
    const val MODE_PAIR = 1
    const val MODE_SESSION = 2
    const val MAX_FRAME = 8 * 1024 * 1024

    fun writePreamble(o: OutputStream, mode: Int) {
        o.write(MAGIC + byteArrayOf(VERSION.toByte(), mode.toByte()))
        o.flush()
    }

    fun readPreamble(i: InputStream): Int {
        val b = ByteArray(6)
        DataInputStream(i).readFully(b)
        if (!b.copyOf(4).contentEquals(MAGIC) || b[4].toInt() != VERSION) throw ProtoException("preâmbulo inválido")
        return b[5].toInt()
    }

    fun write(o: OutputStream, data: ByteArray) {
        if (data.size > MAX_FRAME) throw ProtoException("quadro grande demais")
        val buf = ByteArray(4 + data.size)
        buf[0] = (data.size ushr 24).toByte(); buf[1] = (data.size ushr 16).toByte()
        buf[2] = (data.size ushr 8).toByte(); buf[3] = data.size.toByte()
        System.arraycopy(data, 0, buf, 4, data.size)
        o.write(buf) // um único write, como no Rust
        o.flush()
    }

    fun read(i: InputStream, max: Int): ByteArray {
        val d = DataInputStream(i)
        val l = ByteArray(4)
        d.readFully(l)
        val n = ((l[0].toLong() and 255) shl 24) or ((l[1].toLong() and 255) shl 16) or ((l[2].toLong() and 255) shl 8) or (l[3].toLong() and 255)
        if (n > max) throw ProtoException("quadro excede o limite")
        val v = ByteArray(n.toInt())
        d.readFully(v)
        return v
    }
}
