package app.renanclip.core

/**
 * JSON mínimo (sem org.json, que não existe em testes JVM). Valores: null, Boolean, Long, Double,
 * String, List<Any?>, Map<String, Any?>. Saída compacta como a do serde_json (UTF-8 cru, sem espaços).
 */
object Json {
    fun write(v: Any?): String = StringBuilder().also { w(it, v) }.toString()

    private fun w(sb: StringBuilder, v: Any?) {
        when (v) {
            null -> sb.append("null")
            is Boolean -> sb.append(v)
            is Int, is Long -> sb.append(v.toString())
            is Double -> sb.append(v.toString())
            is String -> str(sb, v)
            is Map<*, *> -> {
                sb.append('{')
                var first = true
                for ((k, x) in v) {
                    if (!first) sb.append(',')
                    first = false
                    str(sb, k as String); sb.append(':'); w(sb, x)
                }
                sb.append('}')
            }
            is Iterable<*> -> {
                sb.append('[')
                var first = true
                for (x in v) {
                    if (!first) sb.append(',')
                    first = false
                    w(sb, x)
                }
                sb.append(']')
            }
            else -> throw IllegalArgumentException("tipo JSON inválido: ${v::class}")
        }
    }

    private fun str(sb: StringBuilder, s: String) {
        sb.append('"')
        for (c in s) when {
            c == '"' -> sb.append("\\\"")
            c == '\\' -> sb.append("\\\\")
            c == '\n' -> sb.append("\\n")
            c == '\r' -> sb.append("\\r")
            c == '\t' -> sb.append("\\t")
            c.code < 0x20 -> sb.append("\\u%04x".format(c.code))
            else -> sb.append(c)
        }
        sb.append('"')
    }

    fun parse(s: String): Any? {
        val p = Parser(s)
        p.ws()
        val v = p.value()
        p.ws()
        if (p.i != s.length) throw ProtoException("JSON: lixo no final")
        return v
    }

    @Suppress("UNCHECKED_CAST")
    fun parseObject(s: String): Map<String, Any?> =
        parse(s) as? Map<String, Any?> ?: throw ProtoException("JSON: esperava objeto")

    private class Parser(val s: String) {
        var i = 0
        fun ws() { while (i < s.length && s[i].let { it == ' ' || it == '\n' || it == '\r' || it == '\t' }) i++ }
        fun fail(): Nothing = throw ProtoException("JSON inválido em $i")
        fun value(): Any? {
            if (i >= s.length) fail()
            return when (val c = s[i]) {
                '{' -> obj()
                '[' -> arr()
                '"' -> string()
                't' -> lit("true", true)
                'f' -> lit("false", false)
                'n' -> lit("null", null)
                else -> if (c == '-' || c in '0'..'9') num() else fail()
            }
        }
        fun lit(w: String, v: Any?): Any? { if (!s.startsWith(w, i)) fail(); i += w.length; return v }
        fun num(): Any {
            val st = i
            if (s[i] == '-') i++
            while (i < s.length && (s[i] in '0'..'9' || s[i] in ".eE+-")) i++
            val t = s.substring(st, i)
            return t.toLongOrNull() ?: t.toDoubleOrNull() ?: fail()
        }
        fun string(): String {
            i++
            val sb = StringBuilder()
            while (true) {
                if (i >= s.length) fail()
                val c = s[i++]
                when (c) {
                    '"' -> return sb.toString()
                    '\\' -> {
                        if (i >= s.length) fail()
                        when (val e = s[i++]) {
                            '"' -> sb.append('"'); '\\' -> sb.append('\\'); '/' -> sb.append('/')
                            'b' -> sb.append('\b'); 'f' -> sb.append('\u000c'); 'n' -> sb.append('\n')
                            'r' -> sb.append('\r'); 't' -> sb.append('\t')
                            'u' -> {
                                if (i + 4 > s.length) fail()
                                sb.append(s.substring(i, i + 4).toIntOrNull(16)?.toChar() ?: fail()); i += 4
                            }
                            else -> { if (e.code < 0) fail(); fail() }
                        }
                    }
                    else -> sb.append(c)
                }
            }
        }
        fun arr(): List<Any?> {
            i++; ws()
            val out = ArrayList<Any?>()
            if (s.getOrNull(i) == ']') { i++; return out }
            while (true) {
                ws(); out.add(value()); ws()
                when (s.getOrNull(i++)) { ',' -> continue; ']' -> return out; else -> fail() }
            }
        }
        fun obj(): Map<String, Any?> {
            i++; ws()
            val out = LinkedHashMap<String, Any?>()
            if (s.getOrNull(i) == '}') { i++; return out }
            while (true) {
                ws()
                if (s.getOrNull(i) != '"') fail()
                val k = string(); ws()
                if (s.getOrNull(i++) != ':') fail()
                ws(); out[k] = value(); ws()
                when (s.getOrNull(i++)) { ',' -> continue; '}' -> return out; else -> fail() }
            }
        }
    }
}
