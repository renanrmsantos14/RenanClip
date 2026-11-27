import java.text.SimpleDateFormat
import java.util.Date
import java.util.TimeZone

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// Versão do projeto: version.json na raiz do repositório (fallback 0.0.0).
val versionName0: String = run {
    val f = rootProject.file("../version.json")
    val m = if (f.exists()) Regex("\"version\"\\s*:\\s*\"([0-9]+\\.[0-9]+\\.[0-9]+)\"").find(f.readText()) else null
    m?.groupValues?.get(1) ?: "0.0.0"
}
val versionCode0: Int = versionName0.split('.').let { (a, b, c) -> a.toInt() * 1_000_000 + b.toInt() * 1_000 + c.toInt() }.coerceAtLeast(1)
val buildStamp: String = SimpleDateFormat("dd/MM/yyyy HH:mm").apply { timeZone = TimeZone.getTimeZone("America/Sao_Paulo") }.format(Date())

android {
    namespace = "app.renanclip"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.renanclip"
        minSdk = 26
        targetSdk = 35
        versionCode = versionCode0
        versionName = versionName0
        buildConfigField("String", "BUILD_TIME", "\"$buildStamp\"")
    }

    buildFeatures { buildConfig = true }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Sem keystore de release no repositório: assine fora, no ambiente de publicação.
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    testOptions { unitTests.isReturnDefaultValues = true }
    lint { abortOnError = false }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
