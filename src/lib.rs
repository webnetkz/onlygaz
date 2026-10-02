//! Offline driving simulator built on Bevy and Avian3D.
//!
//! See `README.md` for the project overview. The crate is split into a library
//! (this file) so that the physics can be exercised from integration tests, and
//! a thin binary (`src/main.rs`) that only assembles the Bevy application.

pub mod asset_paths;
pub mod audio;
pub mod camera;
pub mod game;
pub mod mobile;
pub mod settings;
pub mod ui;
pub mod vehicle;
pub mod world;
