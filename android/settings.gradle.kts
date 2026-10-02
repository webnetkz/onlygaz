// OnlyGAZ for Android: open this folder (android/) in Android Studio.
//
// The game is the Rust library built from android/Cargo.toml (the shared game
// crate is one directory up); app/ packages it with the assets into an APK or
// an Android App Bundle. See android/README.md.

pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "OnlyGAZ"
include(":app")
