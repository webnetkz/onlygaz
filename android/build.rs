//! Link flags for the Android library.
//!
//! Android 15+ devices may use 16 KB memory pages, and Google Play requires
//! apps targeting Android 15 to support them: every loadable segment of the
//! native library must be aligned to 16 KB (older phones with 4 KB pages load
//! such a library just as well).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-arg=-Wl,-z,max-page-size=16384");
    }
}
