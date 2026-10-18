//! Keyboard and mouse state, in the shape the game's input code expects.
//!
//! The upstream input code reads a 256-entry keyboard state table indexed by Windows
//! virtual-key code and a cursor position, and it accumulates a one-frame mouse
//! movement clamped to the range a signed byte can hold. Keeping those three shapes
//! here means the game's input layer ports without a translation table in the middle.

use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

/// Windows virtual-key codes. The game indexes its keyboard table with these, so the
/// numbers are part of the interface, not an implementation detail.
pub mod vk {
	pub const BACK: usize = 0x08;
	pub const TAB: usize = 0x09;
	pub const RETURN: usize = 0x0D;
	pub const SHIFT: usize = 0x10;
	pub const CONTROL: usize = 0x11;
	pub const MENU: usize = 0x12; // Alt
	pub const PAUSE: usize = 0x13;
	pub const CAPITAL: usize = 0x14;
	pub const ESCAPE: usize = 0x1B;
	pub const SPACE: usize = 0x20;
	pub const PRIOR: usize = 0x21; // Page Up
	pub const NEXT: usize = 0x22; // Page Down
	pub const END: usize = 0x23;
	pub const HOME: usize = 0x24;
	pub const LEFT: usize = 0x25;
	pub const UP: usize = 0x26;
	pub const RIGHT: usize = 0x27;
	pub const DOWN: usize = 0x28;
	pub const SNAPSHOT: usize = 0x2C; // Print Screen
	pub const INSERT: usize = 0x2D;
	pub const DELETE: usize = 0x2E;
	pub const LWIN: usize = 0x5B;
	pub const RWIN: usize = 0x5C;
	pub const NUMPAD0: usize = 0x60;
	pub const NUMPAD9: usize = 0x69;
	pub const MULTIPLY: usize = 0x6A;
	pub const ADD: usize = 0x6B;
	pub const SUBTRACT: usize = 0x6D;
	pub const DECIMAL: usize = 0x6E;
	pub const DIVIDE: usize = 0x6F;
	pub const F1: usize = 0x70;
	pub const F12: usize = 0x7B;
	pub const NUMLOCK: usize = 0x90;
	pub const SCROLL: usize = 0x91;
	pub const OEM_COMMA: usize = 0xBC;
	pub const OEM_MINUS: usize = 0xBD;
	pub const OEM_PERIOD: usize = 0xBE;
	pub const LSHIFT: usize = 0xA0;
	pub const RSHIFT: usize = 0xA1;
	pub const LCONTROL: usize = 0xA2;
	pub const RCONTROL: usize = 0xA3;
	pub const LMENU: usize = 0xA4;
	pub const RMENU: usize = 0xA5;
}

/// The movement the game clamps to a signed byte. Reproduced exactly, because the
/// turn rate the player feels depends on the clamp.
pub const MOUSE_DELTA_MAX: i32 = 127;
pub const MOUSE_DELTA_MIN: i32 = -128;

#[derive(Debug)]
pub struct Input {
	/// Indexed by Windows virtual-key code, exactly like `GetKeyboardState`.
	keys: [bool; 256],
	/// Cursor position in client pixels, the `GetCursorPos` equivalent.
	mouse_x: i32,
	mouse_y: i32,
	/// Accumulated movement since the last call to [`Input::take_mouse_movement`].
	delta_x: i32,
	delta_y: i32,
	/// Per-button counters, so a click is not lost between frames.
	button_down: [u32; 3],
	button_up: [u32; 3],
	/// Whether the pointer is grabbed for looking around.
	captured: bool,
	/// Set when the window is asked to close.
	pub quit: bool,
}

impl Default for Input {
	fn default() -> Self {
		Input {
			keys: [false; 256],
			mouse_x: 0,
			mouse_y: 0,
			delta_x: 0,
			delta_y: 0,
			button_down: [0; 3],
			button_up: [0; 3],
			captured: false,
			quit: false,
		}
	}
}

impl Input {
	pub fn key(&self, vk: usize) -> bool {
		vk < self.keys.len() && self.keys[vk]
	}

	pub fn mouse_position(&self) -> (i32, i32) {
		(self.mouse_x, self.mouse_y)
	}

	pub fn is_captured(&self) -> bool {
		self.captured
	}

	/// Consumes the accumulated mouse movement, clamped the way the upstream code
	/// clamps it per frame.
	pub fn take_mouse_movement(&mut self, clamp: bool) -> (i32, i32) {
		let mut dx = self.delta_x;
		let mut dy = self.delta_y;
		if clamp {
			dx = dx.clamp(MOUSE_DELTA_MIN, MOUSE_DELTA_MAX);
			dy = dy.clamp(MOUSE_DELTA_MIN, MOUSE_DELTA_MAX);
		}
		self.delta_x = 0;
		self.delta_y = 0;
		(dx, dy)
	}

	/// Whether a mouse button was pressed since the last frame. Buttons are 0 left, 1
	/// right, 2 middle, matching the game's ordering.
	pub fn take_button_down(&mut self, button: usize) -> bool {
		if button >= 3 || self.button_down[button] == 0 {
			return false;
		}
		self.button_down[button] -= 1;
		true
	}

	pub fn take_button_up(&mut self, button: usize) -> bool {
		if button >= 3 || self.button_up[button] == 0 {
			return false;
		}
		self.button_up[button] -= 1;
		true
	}

	/// Clears the per-frame click counters. Key state is a level, not an edge, so it
	/// survives across frames the way `GetKeyboardState` does.
	pub fn end_frame(&mut self) {
		self.button_down = [0; 3];
		self.button_up = [0; 3];
	}

	pub fn handle(&mut self, event: &WindowEvent) {
		match event {
			WindowEvent::KeyboardInput { event, .. } => {
				if let PhysicalKey::Code(code) = event.physical_key {
					if let Some(vk) = keycode_to_vk(code) {
						self.keys[vk] = event.state == ElementState::Pressed;
						// Windows reports the generic shift/control/alt codes as pressed
						// when either side is, and the game reads the generic ones.
						match code {
							KeyCode::ShiftLeft | KeyCode::ShiftRight => {
								self.keys[vk::SHIFT] = event.state == ElementState::Pressed;
							}
							KeyCode::ControlLeft | KeyCode::ControlRight => {
								self.keys[vk::CONTROL] = event.state == ElementState::Pressed;
							}
							KeyCode::AltLeft | KeyCode::AltRight => {
								self.keys[vk::MENU] = event.state == ElementState::Pressed;
							}
							_ => {}
						}
					}
				}
			}
			WindowEvent::CursorMoved { position, .. } => {
				let (x, y) = (position.x.round() as i32, position.y.round() as i32);
				if self.captured {
					self.delta_x += x - self.mouse_x;
					self.delta_y += y - self.mouse_y;
				}
				self.mouse_x = x;
				self.mouse_y = y;
			}
			WindowEvent::MouseInput { state, button, .. } => {
				let index = match button {
					MouseButton::Left => 0,
					MouseButton::Right => 1,
					MouseButton::Middle => 2,
					_ => return,
				};
				match state {
					ElementState::Pressed => self.button_down[index] += 1,
					ElementState::Released => self.button_up[index] += 1,
				}
			}
			WindowEvent::Focused(false) => {
				// Losing focus must not leave keys stuck down, which is what Windows does
				// when it hands the keyboard to another window.
				self.keys = [false; 256];
			}
			_ => {}
		}
	}

	pub fn set_captured(&mut self, captured: bool) {
		self.captured = captured;
	}
}

/// Maps a physical key to the Windows virtual-key code the game expects.
///
/// Returns `None` for keys the game never asks about, so an unmapped key is ignored
/// rather than being folded onto an unrelated code.
fn keycode_to_vk(code: KeyCode) -> Option<usize> {
	let vk = match code {
		KeyCode::Backspace => vk::BACK,
		KeyCode::Tab => vk::TAB,
		KeyCode::Enter | KeyCode::NumpadEnter => vk::RETURN,
		KeyCode::ShiftLeft => vk::LSHIFT,
		KeyCode::ShiftRight => vk::RSHIFT,
		KeyCode::ControlLeft => vk::LCONTROL,
		KeyCode::ControlRight => vk::RCONTROL,
		KeyCode::AltLeft => vk::LMENU,
		KeyCode::AltRight => vk::RMENU,
		KeyCode::Pause => vk::PAUSE,
		KeyCode::CapsLock => vk::CAPITAL,
		KeyCode::Escape => vk::ESCAPE,
		KeyCode::Space => vk::SPACE,
		KeyCode::PageUp => vk::PRIOR,
		KeyCode::PageDown => vk::NEXT,
		KeyCode::End => vk::END,
		KeyCode::Home => vk::HOME,
		KeyCode::ArrowLeft => vk::LEFT,
		KeyCode::ArrowUp => vk::UP,
		KeyCode::ArrowRight => vk::RIGHT,
		KeyCode::ArrowDown => vk::DOWN,
		KeyCode::PrintScreen => vk::SNAPSHOT,
		KeyCode::Insert => vk::INSERT,
		KeyCode::Delete => vk::DELETE,
		KeyCode::SuperLeft => vk::LWIN,
		KeyCode::SuperRight => vk::RWIN,
		KeyCode::NumLock => vk::NUMLOCK,
		KeyCode::ScrollLock => vk::SCROLL,
		KeyCode::Comma => vk::OEM_COMMA,
		KeyCode::Minus => vk::OEM_MINUS,
		KeyCode::Period => vk::OEM_PERIOD,
		KeyCode::Numpad0 => vk::NUMPAD0,
		KeyCode::Numpad1 => vk::NUMPAD0 + 1,
		KeyCode::Numpad2 => vk::NUMPAD0 + 2,
		KeyCode::Numpad3 => vk::NUMPAD0 + 3,
		KeyCode::Numpad4 => vk::NUMPAD0 + 4,
		KeyCode::Numpad5 => vk::NUMPAD0 + 5,
		KeyCode::Numpad6 => vk::NUMPAD0 + 6,
		KeyCode::Numpad7 => vk::NUMPAD0 + 7,
		KeyCode::Numpad8 => vk::NUMPAD0 + 8,
		KeyCode::Numpad9 => vk::NUMPAD9,
		KeyCode::NumpadMultiply => vk::MULTIPLY,
		KeyCode::NumpadAdd => vk::ADD,
		KeyCode::NumpadSubtract => vk::SUBTRACT,
		KeyCode::NumpadDecimal => vk::DECIMAL,
		KeyCode::NumpadDivide => vk::DIVIDE,
		KeyCode::F1 => vk::F1,
		KeyCode::F2 => vk::F1 + 1,
		KeyCode::F3 => vk::F1 + 2,
		KeyCode::F4 => vk::F1 + 3,
		KeyCode::F5 => vk::F1 + 4,
		KeyCode::F6 => vk::F1 + 5,
		KeyCode::F7 => vk::F1 + 6,
		KeyCode::F8 => vk::F1 + 7,
		KeyCode::F9 => vk::F1 + 8,
		KeyCode::F10 => vk::F1 + 9,
		KeyCode::F11 => vk::F1 + 10,
		KeyCode::F12 => vk::F12,
		KeyCode::Digit0 => 0x30,
		KeyCode::Digit1 => 0x31,
		KeyCode::Digit2 => 0x32,
		KeyCode::Digit3 => 0x33,
		KeyCode::Digit4 => 0x34,
		KeyCode::Digit5 => 0x35,
		KeyCode::Digit6 => 0x36,
		KeyCode::Digit7 => 0x37,
		KeyCode::Digit8 => 0x38,
		KeyCode::Digit9 => 0x39,
		KeyCode::KeyA => 0x41,
		KeyCode::KeyB => 0x42,
		KeyCode::KeyC => 0x43,
		KeyCode::KeyD => 0x44,
		KeyCode::KeyE => 0x45,
		KeyCode::KeyF => 0x46,
		KeyCode::KeyG => 0x47,
		KeyCode::KeyH => 0x48,
		KeyCode::KeyI => 0x49,
		KeyCode::KeyJ => 0x4A,
		KeyCode::KeyK => 0x4B,
		KeyCode::KeyL => 0x4C,
		KeyCode::KeyM => 0x4D,
		KeyCode::KeyN => 0x4E,
		KeyCode::KeyO => 0x4F,
		KeyCode::KeyP => 0x50,
		KeyCode::KeyQ => 0x51,
		KeyCode::KeyR => 0x52,
		KeyCode::KeyS => 0x53,
		KeyCode::KeyT => 0x54,
		KeyCode::KeyU => 0x55,
		KeyCode::KeyV => 0x56,
		KeyCode::KeyW => 0x57,
		KeyCode::KeyX => 0x58,
		KeyCode::KeyY => 0x59,
		KeyCode::KeyZ => 0x5A,
		_ => return None,
	};
	Some(vk)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn movement_is_clamped_to_a_signed_byte() {
		let mut input = Input::default();
		input.set_captured(true);
		input.mouse_x = 0;
		input.mouse_y = 0;
		// A fast flick, larger than a byte can carry.
		input.handle(&WindowEvent::CursorMoved {
			device_id: unsafe { std::mem::zeroed() },
			position: winit::dpi::PhysicalPosition::new(400.0, -300.0),
		});
		let (dx, dy) = input.take_mouse_movement(true);
		assert_eq!(dx, 127);
		assert_eq!(dy, -128);
		// Consumed, so the next frame sees nothing.
		assert_eq!(input.take_mouse_movement(true), (0, 0));
	}

	#[test]
	fn movement_is_not_accumulated_while_uncaptured() {
		let mut input = Input::default();
		input.handle(&WindowEvent::CursorMoved {
			device_id: unsafe { std::mem::zeroed() },
			position: winit::dpi::PhysicalPosition::new(50.0, 50.0),
		});
		assert_eq!(input.take_mouse_movement(true), (0, 0));
		assert_eq!(input.mouse_position(), (50, 50));
	}

	#[test]
	fn letters_and_digits_land_on_their_vk_codes() {
		assert_eq!(keycode_to_vk(KeyCode::KeyW), Some(0x57));
		assert_eq!(keycode_to_vk(KeyCode::Digit5), Some(0x35));
		assert_eq!(keycode_to_vk(KeyCode::Escape), Some(0x1B));
	}
}
