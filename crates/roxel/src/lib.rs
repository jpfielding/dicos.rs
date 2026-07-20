//! GPU-accelerated DICOS volume viewer.
//!
//! Provides a 3D volume rendering application using wgpu for ray-casting
//! and egui for the user interface. Supports material-band transfer functions
//! and threat overlay visualization.

pub mod app;
pub mod camera;
pub mod loader;
pub mod renderer;
pub mod slice_view;
pub mod state;
pub mod transfer;
pub mod ui;
pub mod volume;
