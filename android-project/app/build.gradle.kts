plugins {
    alias(libs.plugins.android.application)
}

android {
    namespace = "dev.sanctum.app"
    compileSdk = 35
    ndkVersion = "30.0.16248370"

    defaultConfig {
        applicationId = "dev.sanctum.app"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    signingConfigs {
        create("release") {
            storeFile = rootProject.file("../crates/sanctum/android/debug.keystore")
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("release")
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

val ndkBuild = tasks.register<Exec>("ndkBuild") {
    val isRelease = gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }
    val repoRoot = rootDir.parentFile
    workingDir = repoRoot
    val profile = if (isRelease) "release" else "dev"
    val userHome = System.getProperty("user.home")
    val hostOs = System.getProperty("os.name").lowercase()
    val defaultSdkDir = when {
        hostOs.contains("win") -> "${System.getenv("LOCALAPPDATA")}/Android/Sdk"
        hostOs.contains("mac") -> "$userHome/Library/Android/sdk"
        else -> "$userHome/Android/Sdk"
    }
    val sdkDir = System.getenv("ANDROID_HOME")
        ?: System.getenv("ANDROID_SDK_ROOT")
        ?: defaultSdkDir
    val ndkDir = System.getenv("ANDROID_NDK_ROOT")
        ?: System.getenv("ANDROID_NDK_HOME")
        ?: "$sdkDir/ndk/30.0.16248370"
    environment("ANDROID_NDK_HOME", ndkDir)
    environment("ANDROID_NDK_ROOT", ndkDir)
    environment("ANDROID_HOME", sdkDir)
    environment("JAVA_HOME", System.getenv("JAVA_HOME") ?: System.getProperty("java.home"))
    commandLine(
        "cargo", "apk", "--",
        "build",
        "-p", "sanctum",
        "--lib",
        "--features", "android-backend",
        "--target", "aarch64-linux-android",
        "--profile", profile,
    )
}

// Sync, rather than merely copy, so a library rename cannot leave a stale
// native binary in the APK.
val copyJniLib = tasks.register<Sync>("copyJniLib") {
    val isRelease = gradle.startParameter.taskNames.any { it.contains("Release", ignoreCase = true) }
    val profile = if (isRelease) "release" else "debug"
    dependsOn(ndkBuild)
    from(rootDir.parentFile.resolve("target/aarch64-linux-android/$profile/libsanctum_core.so"))
    into("$projectDir/src/main/jniLibs/arm64-v8a")
}

tasks.named("preBuild") {
    dependsOn(copyJniLib)
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.documentfile)
}
