//! Android entry point of OnlyGAZ.
//!
//! `#[bevy_main]` generates the `android_main` symbol that NativeActivity
//! calls on its game thread; everything else lives in the shared game crate.

use bevy::prelude::*;

#[bevy_main]
fn main() {
    veyron_sim::mobile::run_mobile_app();
}
