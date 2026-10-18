//! The game.
//!
//! No window, no GPU and no OS types reach in here. Everything the game needs from the
//! outside arrives through [`xops_platform`], which is what makes the game logic
//! testable without a display and what keeps a graphics backend swappable.

pub mod config;
pub mod core;
pub mod datafile;
pub mod d3dgraphics;
pub mod debug;
pub mod model;
pub mod parameter;

pub use core::*;
