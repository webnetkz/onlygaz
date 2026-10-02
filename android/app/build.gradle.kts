// OnlyGAZ for Android: the app module.
//
// Every build
//   1. compiles the Rust game (android/Cargo.toml) into lib/<abi>/libonlygaz.so
//      with cargo and the NDK's clang (task cargoBuildDebug / cargoBuildRelease),
//   2. copies the repository's assets/ folder with phone-sized textures
//      (prepareDebugGameAssets / prepareReleaseGameAssets),
//   3. packages both with MainActivity (a NativeActivity).
//
//   Run in Android Studio      -> "OnlyGAZ Debug" (kz.webnet.onlygaz.debug)
//   ./gradlew bundleRelease    -> app/build/outputs/bundle/release/app-release.aab
//   ./gradlew assembleRelease  -> app/build/outputs/apk/release/app-release.apk

import java.util.Properties
import kz.webnet.onlygaz.build.CargoBuildTask
import kz.webnet.onlygaz.build.PrepareGameAssetsTask

plugins {
    id("com.android.application")
}

// ------------------------------------------------------------------ settings

val localProperties = Properties().apply {
    val file = rootProject.file("local.properties")
    if (file.isFile) file.inputStream().use { load(it) }
}

/** A setting from -P / gradle.properties, else from android/local.properties. */
fun setting(name: String): String? =
    providers.gradleProperty(name).orNull ?: localProperties.getProperty(name)

fun requiredSetting(name: String): String =
    setting(name) ?: throw GradleException("$name is missing from android/gradle.properties")

val gameVersionCode = requiredSetting("onlygaz.versionCode").toInt()
val gameVersionName = requiredSetting("onlygaz.versionName")
val gameMinSdk = requiredSetting("onlygaz.minSdk").toInt()
val gameTargetSdk = requiredSetting("onlygaz.targetSdk").toInt()
val gameNdkVersion = setting("onlygaz.ndkVersion") ?: "27.0.12077973"

/** Repository root: the shared game crate and its assets/ folder. */
val repositoryDirectory: File = rootProject.projectDir.parentFile

val androidSdkPath: String? = localProperties.getProperty("sdk.dir")
    ?: providers.environmentVariable("ANDROID_HOME").orNull
    ?: providers.environmentVariable("ANDROID_SDK_ROOT").orNull

// Run in Android Studio passes the ABIs of the selected phone or emulator:
// compile only the first one the game supports. Other builds use onlygaz.abis.
val supportedAbis = listOf("arm64-v8a", "x86_64", "armeabi-v7a", "x86")
val deviceAbi: String? = providers.gradleProperty("android.injected.build.abi").orNull
    ?.split(',')
    ?.map { it.trim() }
    ?.firstOrNull { it in supportedAbis }
val nativeAbis: List<String> = if (deviceAbi != null) {
    listOf(deviceAbi)
} else {
    (setting("onlygaz.abis") ?: "arm64-v8a").split(',').map { it.trim() }.filter { it.isNotEmpty() }
}
for (abi in nativeAbis) {
    require(abi in supportedAbis) { "Unsupported ABI '$abi' in onlygaz.abis, use $supportedAbis" }
}

val cargoArguments: List<String> = (setting("onlygaz.cargoArgs") ?: "")
    .split(' ')
    .map { it.trim() }
    .filter { it.isNotEmpty() }
val cargoEnvironment: Map<String, String> = (setting("onlygaz.cargoEnv") ?: "")
    .split(';')
    .map { it.trim() }
    .filter { it.contains('=') }
    .associate { it.substringBefore('=').trim() to it.substringAfter('=') }

// Release signing key: the git-ignored android/keystore/ folder (created by
// android/build_apk.sh). Without it release builds stay unsigned and Android
// Studio's "Generate Signed App Bundle" wizard can sign them.
val keystoreDirectory = rootProject.file("keystore")
val keystoreProperties = Properties().apply {
    val file = keystoreDirectory.resolve("keystore.properties")
    if (file.isFile) file.inputStream().use { load(it) }
}
val releaseKeystore: File? = keystoreProperties.getProperty("storeFile")
    ?.let { keystoreDirectory.resolve(it) }
    ?.takeIf { it.isFile }

// ------------------------------------------------------------------- android

android {
    namespace = "kz.webnet.onlygaz"
    compileSdk = gameTargetSdk
    ndkVersion = gameNdkVersion

    defaultConfig {
        applicationId = "kz.webnet.onlygaz"
        minSdk = gameMinSdk
        targetSdk = gameTargetSdk
        versionCode = gameVersionCode
        versionName = gameVersionName
        manifestPlaceholders["appLabel"] = "OnlyGAZ"
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        getByName("debug") {
            // Installs next to the release version and keeps its own settings.
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
            manifestPlaceholders["appLabel"] = "OnlyGAZ Debug"
        }
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            // Stored uncompressed and 16 KB aligned: loaded in place, nothing
            // is extracted on install.
            useLegacyPackaging = false
        }
    }

    lint {
        // The only Java code is the small MainActivity.
        checkReleaseBuilds = false
    }
}

// ----------------------------------------------------------- Rust and assets

fun cargoBuildTask(buildType: String, profile: String) =
    tasks.register<CargoBuildTask>("cargoBuild" + buildType.replaceFirstChar { it.uppercaseChar() }) {
        group = "onlygaz"
        description = "Compiles libonlygaz.so for $nativeAbis with cargo (profile $profile)."
        crateDirectory.set(rootProject.layout.projectDirectory)
        cargoProfile.set(profile)
        abis.set(nativeAbis)
        apiLevel.set(gameMinSdk)
        ndkVersion.set(gameNdkVersion)
        sdkDirectory.set(androidSdkPath)
        ndkDirectory.set(setting("onlygaz.ndkDir"))
        cargoExecutable.set(setting("onlygaz.cargo"))
        extraCargoArguments.set(cargoArguments)
        extraEnvironment.set(cargoEnvironment)
        outputDirectory.set(layout.buildDirectory.dir("generated/onlygaz/jniLibs/$buildType"))
    }

fun prepareAssetsTask(buildType: String) =
    tasks.register<PrepareGameAssetsTask>(
        "prepare" + buildType.replaceFirstChar { it.uppercaseChar() } + "GameAssets"
    ) {
        group = "onlygaz"
        description = "Copies assets/ with phone-sized textures for the $buildType build."
        sourceDirectory.set(repositoryDirectory.resolve("assets"))
        outputDirectory.set(layout.buildDirectory.dir("generated/onlygaz/assets/$buildType"))
    }

// Debug builds use the android-dev profile (release optimisation without LTO,
// see android/Cargo.toml): playable speed with much faster rebuilds.
val cargoBuildDebug = cargoBuildTask("debug", setting("onlygaz.debugCargoProfile") ?: "android-dev")
val cargoBuildRelease = cargoBuildTask("release", "release")
val prepareDebugGameAssets = prepareAssetsTask("debug")
val prepareReleaseGameAssets = prepareAssetsTask("release")

androidComponents {
    onVariants { variant ->
        val release = variant.buildType == "release"
        variant.sources.jniLibs?.addGeneratedSourceDirectory(
            if (release) cargoBuildRelease else cargoBuildDebug
        ) { it.outputDirectory }
        variant.sources.assets?.addGeneratedSourceDirectory(
            if (release) prepareReleaseGameAssets else prepareDebugGameAssets
        ) { it.outputDirectory }
    }
}
