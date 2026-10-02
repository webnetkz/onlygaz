// Build logic of the Android project, in plain Java so that it needs nothing
// beyond the Gradle API: the cargo build of libonlygaz.so (CargoBuildTask) and
// the phone-sized copy of the game assets (PrepareGameAssetsTask, GameAssets).
// GameAssets also runs standalone from android/build_apk.sh.

plugins {
    java
}

dependencies {
    implementation(gradleApi())
}

tasks.withType<JavaCompile>().configureEach {
    options.release.set(11)
    options.encoding = "UTF-8"
}
