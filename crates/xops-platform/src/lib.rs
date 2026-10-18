//! Platform layer.
//!
//! Everything that touches the operating system or the GPU lives here, and nothing
//! above it does. This mirrors the upstream low layer, which the browser port
//! replaced one file at a time.

pub mod gl;
pub mod math;

pub mod font;
pub mod frame;
pub mod gfx;
pub mod host;
pub mod input;
pub mod path;
pub mod sound;
pub mod texture;
pub mod window;

/// Re-exported so the binary and the game crate name one version of the window types.
pub use wgpu;
pub use winit;

pub use math::{Mat4, Vec3, Vec4};
