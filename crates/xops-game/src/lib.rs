//! The game.
//!
//! No window, no GPU and no OS types reach in here. Everything the game needs from the
//! outside arrives through [`xops_platform`], which is what makes the game logic
//! testable without a display and what keeps a graphics backend swappable.

pub mod collision;
pub mod config;
pub mod core;
pub mod datafile;
pub mod d3dgraphics;
pub mod debug;
pub mod gamemain;
pub mod gamemenu;
pub mod gui_object;
pub mod human;
pub mod motion;
pub mod input;
pub mod maingame;
pub mod map;
pub mod model;
pub mod object;
pub mod objectmanager;
pub mod effects;
pub mod event;
pub mod weapon;
pub mod parameter;
pub mod statemachine;
pub mod twod;
pub mod window;

pub mod resource;
pub mod scene;
pub mod sound;
pub mod soundmanager;

pub use core::*;

pub mod ai;
pub mod briefing;
