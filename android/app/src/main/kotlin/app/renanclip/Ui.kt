package app.renanclip

import android.app.Activity
import android.content.Context
import android.content.res.Configuration
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.text.TextUtils
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/** Paleta e construtores de view (sem XML, sem bibliotecas). */
class Pal(val dark: Boolean) {
    val bg = if (dark) 0xFF121212.toInt() else 0xFFFAFAF7.toInt()
    val surface = if (dark) 0xFF1D1D20.toInt() else 0xFFFFFFFF.toInt()
    val text = if (dark) 0xFFECECEE.toInt() else 0xFF1B1B1F.toInt()
    val sub = if (dark) 0xFF9A9AA3.toInt() else 0xFF6B6B73.toInt()
    val accent = if (dark) 0xFF6FCFB5.toInt() else 0xFF1F6F5C.toInt()
    val onAccent = if (dark) 0xFF0B1F19.toInt() else 0xFFFFFFFF.toInt()
    val line = if (dark) 0xFF2E2E33.toInt() else 0xFFE4E4E0.toInt()
    val danger = if (dark) 0xFFFF8A80.toInt() else 0xFFB3261E.toInt()
}

object Ui {
    fun dp(ctx: Context, v: Int) = (v * ctx.resources.displayMetrics.density + 0.5f).toInt()

    /** Escolhe o tema (claro/escuro/sistema) antes de montar a tela. */
    fun pal(ctx: Context): Pal {
        val t = runCatching { Rc.engine.settings().theme }.getOrDefault("auto")
        val sysDark = (ctx.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES
        return Pal(when (t) { "escuro" -> true; "claro" -> false; else -> sysDark })
    }

    fun applyTheme(a: Activity): Pal {
        val p = pal(a)
        a.setTheme(if (p.dark) R.style.AppDark else R.style.AppLight)
        return p
    }

    fun rounded(color: Int, radiusDp: Int, ctx: Context, stroke: Int = 0): GradientDrawable = GradientDrawable().apply {
        setColor(color); cornerRadius = dp(ctx, radiusDp).toFloat()
        if (stroke != 0) setStroke(dp(ctx, 1), stroke)
    }

    fun label(ctx: Context, s: String, sp: Float = 15f, color: Int, bold: Boolean = false) = TextView(ctx).apply {
        text = s; setTextSize(TypedValue.COMPLEX_UNIT_SP, sp); setTextColor(color)
        if (bold) setTypeface(typeface, Typeface.BOLD)
    }

    fun button(ctx: Context, p: Pal, s: String, primary: Boolean = false, onClick: () -> Unit) = Button(ctx).apply {
        text = s; isAllCaps = false; setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
        setTextColor(if (primary) p.onAccent else p.accent)
        background = rounded(if (primary) p.accent else p.surface, 10, ctx, if (primary) 0 else p.line)
        minHeight = dp(ctx, 44); minimumHeight = dp(ctx, 44)
        setPadding(dp(ctx, 14), 0, dp(ctx, 14), 0)
        stateListAnimator = null
        setOnClickListener { onClick() }
    }

    fun gap(ctx: Context, h: Int) = View(ctx).apply { layoutParams = LinearLayout.LayoutParams(1, dp(ctx, h)) }

    /** Rodapé fixo e discreto com a versão do build; não captura toque nem foco. */
    fun version(ctx: Context, p: Pal) = TextView(ctx).apply {
        text = "v${BuildConfig.VERSION_NAME} · ${BuildConfig.BUILD_TIME}"
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f); setTextColor(p.sub); alpha = 0.7f
        gravity = Gravity.END; isClickable = false; isFocusable = false
        importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
        setPadding(dp(ctx, 12), dp(ctx, 2), dp(ctx, 12), dp(ctx, 4))
        maxLines = 1; ellipsize = TextUtils.TruncateAt.END
    }

    /** Estrutura padrão: [conteúdo] + rodapé de versão. Devolve (raiz, conteúdo). */
    fun screen(a: Activity, p: Pal): Pair<LinearLayout, LinearLayout> {
        val root = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL; setBackgroundColor(p.bg); fitsSystemWindows = true }
        val content = LinearLayout(a).apply { orientation = LinearLayout.VERTICAL }
        root.addView(content, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
        root.addView(version(a, p), LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        return root to content
    }

    fun header(a: Activity, p: Pal, title: String, back: Boolean): LinearLayout = LinearLayout(a).apply {
        orientation = LinearLayout.HORIZONTAL; gravity = Gravity.CENTER_VERTICAL
        setPadding(dp(a, 16), dp(a, 12), dp(a, 8), dp(a, 8))
        if (back) addView(label(a, "‹", 28f, p.accent).apply {
            setPadding(0, 0, dp(a, 14), dp(a, 4)); contentDescription = "Voltar"; setOnClickListener { a.finish() }
        })
        addView(label(a, title, 22f, p.text, true), LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f))
    }
}
