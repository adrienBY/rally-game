plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val runNumber = (System.getenv("GITHUB_RUN_NUMBER") ?: "1").toInt()

android {
    namespace = "com.fogwood.rally"
    compileSdk = 35
    defaultConfig {
        applicationId = "com.fogwood.rally"
        minSdk = 26
        targetSdk = 35
        versionCode = runNumber
        versionName = "1.0.$runNumber"
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86_64") }
    }
    // One permanent key for every build, so each new APK installs over the previous one.
    // CI decrypts android/signing/fogwood-release.jks.enc with the FOGWOOD_KEYSTORE_PASSWORD secret.
    val ksFile = System.getenv("FOGWOOD_KEYSTORE")?.let { file(it) }?.takeIf { it.exists() }
    val ksPass = System.getenv("FOGWOOD_KEYSTORE_PASSWORD")
    signingConfigs {
        if (ksFile != null && ksPass != null) create("fogwood") {
            storeFile = ksFile; storePassword = ksPass; keyAlias = "fogwood"; keyPassword = ksPass
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("fogwood") ?: signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    androidResources { noCompress += listOf("html", "js") }
    packaging { jniLibs { useLegacyPackaging = false } }
    lint { abortOnError = false; checkReleaseBuilds = false }
}

dependencies {
    implementation("androidx.webkit:webkit:1.12.1")
    implementation("androidx.core:core-ktx:1.13.1")
}
