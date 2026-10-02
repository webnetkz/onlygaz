//! Preview the phone build on a desktop.
//!
//! ```sh
//! cargo run --release --features mobile --example mobile_preview
//! ```
//!
//! Opens a landscape, phone-shaped window with the touch interface. The mouse
//! acts as a finger on the on-screen pedals and buttons; the keyboard keeps
//! working as well. Tilt steering needs a real phone and falls back to the
//! on-screen arrows here.

fn main() {
    veyron_sim::mobile::run_mobile_app();
}
