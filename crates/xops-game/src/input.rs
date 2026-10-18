//! The input layer, from the upstream `input.cpp` and `input.h`.
//!
//! The upstream class reads the keyboard and the mouse through one of three
//! interchangeable back ends, chosen by `INPUT_INTERFACE`. The one kept here is
//! `INPUT_INTERFACE_DEFAULT`, the Raw Input one, because its state model is exactly what
//! the platform layer produces: a keyboard table filled by device events, mouse movement
//! accumulated between frames, and button flags set by the events too. The other two
//! would add device code that has no place above the platform: WindowsInput would read
//! the keys with `GetKeyboardState` and derive the movement by differencing the cursor
//! against the previous frame, and DirectInput would create and acquire the keyboard and
//! mouse devices and would use DirectInput scan codes instead of virtual-key codes.
//!
//! The platform's counterparts, one per upstream field:
//!
//! - `keys_proc`, the table the Raw Input procedure fills, is [`xops_platform::input::Input::key`],
//!   indexed by Windows virtual-key code.
//! - `mx_proc` and `my_proc`, the movement the procedure accumulated, is
//!   [`xops_platform::input::Input::take_mouse_movement`].
//! - `mbl_proc` and `mbr_proc`, the button flags the procedure filled, are
//!   [`xops_platform::input::Input::take_button_down`] and
//!   [`xops_platform::input::Input::take_button_up`].
//!
//! What is not here: the upstream `InitInput` and `DestroyInput`, which register the raw
//! input devices, install the window procedure, hide the cursor and release it all again.
//! The host owns the window and the devices, so there is nothing left of those two.
//!
//! [`InputControl`] starts as a value the caller owns, the way the debug log does. The
//! upstream keeps one at file scope and every screen reaches for it.

use xops_platform::input::{vk, Input};

use crate::config::Config;
use crate::window::WindowControl;

/// The `VK_OEM_*` codes the key table names that the platform's table does not.
///
/// The platform names the three a physical key maps to on its own (comma, minus, period).
/// The rest are reachable only through a rebinding, so they are named here, with the
/// `windows.h` names and values the upstream switch is written with.
const VK_OEM_1: i32 = 0xBA;
const VK_OEM_2: i32 = 0xBF;
const VK_OEM_3: i32 = 0xC0;
const VK_OEM_4: i32 = 0xDB;
const VK_OEM_5: i32 = 0xDC;
const VK_OEM_6: i32 = 0xDD;
const VK_OEM_7: i32 = 0xDE;
const VK_OEM_102: i32 = 0xE2;
const VK_OEM_PLUS: i32 = 0xBB;

/// The input devices, the upstream `InputControl`.
///
/// The keyboard table is a byte per virtual-key code because that is what the upstream
/// stores and what its edge checks test the top bit of. Windows fills it with 0x80 for a
/// key that is down and 0x00 for one that is up, and nothing here reads any other bit.
pub struct InputControl {
	/// Limits one frame of mouse movement to the range a signed byte holds.
	mouse_limit_flag: bool,
	/// The keys this frame, the upstream `keys`.
	keys: [u8; 256],
	/// The keys the frame before, the upstream `keys_lt`.
	keys_lt: [u8; 256],
	/// The mouse coordinate or movement the last retrieval stored.
	mx: i32,
	my: i32,
	/// The mouse buttons this frame and the frame before.
	mbl: bool,
	mbr: bool,
	mbl_lt: bool,
	mbr_lt: bool,
}

impl Default for InputControl {
	fn default() -> InputControl {
		InputControl::new()
	}
}

impl InputControl {
	/// A control that has read nothing yet. The upstream constructor.
	pub fn new() -> InputControl {
		InputControl {
			// The upstream starts with the limit on, which is also what the game's
			// default config asks for.
			mouse_limit_flag: true,
			keys: [0; 256],
			keys_lt: [0; 256],
			mx: 0,
			my: 0,
			mbl: false,
			mbr: false,
			mbl_lt: false,
			mbr_lt: false,
		}
	}

	/// `SetMouseLimitFlag`: the switch the options screen and the game's config drive.
	///
	/// `main.cpp` sets it from `GameConfig.GetMouseLimitFlag()` once the window exists.
	/// With it on, one frame of movement cannot exceed what a signed byte holds, which is
	/// what the original game did and what a player feels as the turn rate.
	pub fn set_mouse_limit_flag(&mut self, flag: bool) {
		self.mouse_limit_flag = flag;
	}

	/// `GetInputState`: takes this frame's state out of the platform.
	///
	/// `mousemode` is upstream's: false stores the cursor position in client coordinates,
	/// true stores the movement since the last call. The movement is consumed either way
	/// because the upstream clears its accumulator in both modes, and the limit reaches
	/// the platform's clamp as the same flag the upstream tests, over the relative reading
	/// only.
	///
	/// The keys are copied from the platform's table into this frame's and the previous
	/// frame's, which is the upstream's two `memcpy`s. That is also why the edge checks
	/// below read a table rather than consuming an edge: a screen asks for the same key
	/// more than once in a frame, and every ask has to see the same frame.
	pub fn get_input_state(&mut self, input: &mut Input, mousemode: bool) {
		self.keys_lt = self.keys;
		for (code, key) in self.keys.iter_mut().enumerate() {
			*key = if input.key(code) { 0x80 } else { 0x00 };
		}

		let (x, y) = input.mouse_position();
		let (movement_x, movement_y) = input.take_mouse_movement(self.mouse_limit_flag && mousemode);
		if mousemode {
			self.mx = movement_x;
			self.my = movement_y;
		} else {
			self.mx = x;
			self.my = y;
		}

		self.mbl_lt = self.mbl;
		self.mbr_lt = self.mbr;
		// The platform counts the presses and the releases of the frame, and the upstream's
		// proc flags end the frame holding whatever the last event said, which a release
		// after a press in one frame also leaves behind.
		if input.take_button_down(0) {
			self.mbl = true;
		}
		if input.take_button_up(0) {
			self.mbl = false;
		}
		if input.take_button_down(1) {
			self.mbr = true;
		}
		if input.take_button_up(1) {
			self.mbr = false;
		}
	}

	/// `MoveMouseCenter`: puts the cursor in the middle of the game's own screen size.
	///
	/// The upstream converts the point from client to screen coordinates and calls
	/// `SetCursorPos`, and this is the host's cursor warp. The Raw Input back end keeps no
	/// cursor history, so there is nothing else to update: the WindowsInput back end would
	/// also have to record the point as the previous frame's.
	pub fn move_mouse_center(&self, window: &WindowControl, config: &Config) {
		window.set_cursor_position(config.get_screen_width() / 2, config.get_screen_height() / 2);
	}

	/// `CheckKeyNow`: whether the key is down now.
	pub fn check_key_now(&self, id: i32) -> bool {
		if id < 0 || 256 <= id {
			return false;
		}

		self.keys[id as usize] & 0x80 != 0
	}

	/// `CheckKeyDown`: whether the key went down this frame.
	pub fn check_key_down(&self, id: i32) -> bool {
		if id < 0 || 256 <= id {
			return false;
		}

		self.keys_lt[id as usize] & 0x80 == 0 && self.keys[id as usize] & 0x80 != 0
	}

	/// `CheckKeyUp`: whether the key came up this frame.
	pub fn check_key_up(&self, id: i32) -> bool {
		if id < 0 || 256 <= id {
			return false;
		}

		self.keys_lt[id as usize] & 0x80 != 0 && self.keys[id as usize] & 0x80 == 0
	}

	/// `GetMouseMovement`: the coordinates or the movement the last
	/// [`InputControl::get_input_state`] stored, depending on the mode it was asked for.
	pub fn get_mouse_movement(&self) -> (i32, i32) {
		(self.mx, self.my)
	}

	/// `CheckMouseButtonNowL`: whether the left button is down now.
	pub fn check_mouse_button_now_l(&self) -> bool {
		self.mbl
	}

	/// `CheckMouseButtonDownL`: whether the left button went down this frame.
	pub fn check_mouse_button_down_l(&self) -> bool {
		!self.mbl_lt && self.mbl
	}

	/// `CheckMouseButtonUpL`: whether the left button came up this frame.
	pub fn check_mouse_button_up_l(&self) -> bool {
		self.mbl_lt && !self.mbl
	}

	/// `CheckMouseButtonNowR`: whether the right button is down now.
	pub fn check_mouse_button_now_r(&self) -> bool {
		self.mbr
	}

	/// `CheckMouseButtonDownR`: whether the right button went down this frame.
	pub fn check_mouse_button_down_r(&self) -> bool {
		!self.mbr_lt && self.mbr
	}

	/// `CheckMouseButtonUpR`: whether the right button came up this frame.
	pub fn check_mouse_button_up_r(&self) -> bool {
		self.mbr_lt && !self.mbr
	}

	/// `CheckInputAll`: the first rebindable key released this frame, or -1.
	///
	/// The upstream also answers for the DirectInput table's split shift and control
	/// codes, and those two branches are not here: with the raw input table kept, 0x14 and
	/// 0x15 are the generic `VK_SHIFT` and `VK_CONTROL`, which the platform sets when
	/// either side of the key is down, so the table never names the split codes and the
	/// branches could not be reached.
	pub fn check_input_all(&self) -> i32 {
		for code in 0..=0x4A {
			let keycode = originalkeycode_to_dinputdef(code);

			if keycode == -1 && self.check_mouse_button_up_l() {
				return code;
			}
			if keycode == -2 && self.check_mouse_button_up_r() {
				return code;
			}
			if self.check_key_up(keycode) {
				return code;
			}
		}

		-1
	}
}

/// `OriginalkeycodeToDinputdef`: the retrieval keycode of a stored one.
///
/// The original keycodes are the 0x00 to 0x4A the config file stores and the key
/// rebinding screen produces. The retrieval keycodes are what the keyboard table is
/// indexed by, which the upstream spells in DirectInput scan codes and which is the
/// Windows virtual-key codes here, because that is the table the platform fills. The name
/// is kept, so this reads against the upstream line for line.
///
/// 1 or more: a virtual-key code, -1: mouse left, -2: mouse right, 0: no key.
pub fn originalkeycode_to_dinputdef(code: i32) -> i32 {
	match code {
		0x00 => vk::UP as i32,
		0x01 => vk::DOWN as i32,
		0x02 => vk::LEFT as i32,
		0x03 => vk::RIGHT as i32,
		0x04 => vk::NUMPAD0 as i32,
		0x05 => vk::NUMPAD0 as i32 + 1,
		0x06 => vk::NUMPAD0 as i32 + 2,
		0x07 => vk::NUMPAD0 as i32 + 3,
		0x08 => vk::NUMPAD0 as i32 + 4,
		0x09 => vk::NUMPAD0 as i32 + 5,
		0x0A => vk::NUMPAD0 as i32 + 6,
		0x0B => vk::NUMPAD0 as i32 + 7,
		0x0C => vk::NUMPAD0 as i32 + 8,
		0x0D => vk::NUMPAD9 as i32,
		0x0E => vk::BACK as i32,
		0x0F => vk::RETURN as i32,

		0x10 => vk::TAB as i32,
		0x11 => vk::SPACE as i32,
		0x12 => -1, // MOUSE L
		0x13 => -2, // MOUSE R
		0x14 => vk::SHIFT as i32,
		0x15 => vk::CONTROL as i32,
		0x16 => b'0' as i32,
		0x17 => b'1' as i32,
		0x18 => b'2' as i32,
		0x19 => b'3' as i32,
		0x1A => b'4' as i32,
		0x1B => b'5' as i32,
		0x1C => b'6' as i32,
		0x1D => b'7' as i32,
		0x1E => b'8' as i32,
		0x1F => b'9' as i32,

		0x20 => b'A' as i32,
		0x21 => b'B' as i32,
		0x22 => b'C' as i32,
		0x23 => b'D' as i32,
		0x24 => b'E' as i32,
		0x25 => b'F' as i32,
		0x26 => b'G' as i32,
		0x27 => b'H' as i32,
		0x28 => b'I' as i32,
		0x29 => b'J' as i32,
		0x2A => b'K' as i32,
		0x2B => b'L' as i32,
		0x2C => b'M' as i32,
		0x2D => b'N' as i32,
		0x2E => b'O' as i32,
		0x2F => b'P' as i32,

		0x30 => b'Q' as i32,
		0x31 => b'R' as i32,
		0x32 => b'S' as i32,
		0x33 => b'T' as i32,
		0x34 => b'U' as i32,
		0x35 => b'V' as i32,
		0x36 => b'W' as i32,
		0x37 => b'X' as i32,
		0x38 => b'Y' as i32,
		0x39 => b'Z' as i32,
		0x3A => VK_OEM_2,
		0x3B => VK_OEM_1,
		0x3C => VK_OEM_PLUS,
		0x3D => vk::OEM_MINUS as i32,
		0x3E => VK_OEM_3,
		0x3F => VK_OEM_4,

		0x40 => VK_OEM_6,
		0x41 => VK_OEM_102,
		0x42 => VK_OEM_5,
		0x43 => vk::OEM_COMMA as i32,
		0x44 => vk::OEM_PERIOD as i32,
		0x45 => VK_OEM_7,
		0x46 => vk::MULTIPLY as i32,
		0x47 => vk::DIVIDE as i32,
		0x48 => vk::ADD as i32,
		0x49 => vk::SUBTRACT as i32,
		0x4A => vk::DECIMAL as i32,

		_ => 0,
	}
}

/// `GetEscKeycode`: the key that leaves a screen.
pub fn get_esc_keycode() -> i32 {
	vk::ESCAPE as i32
}

/// `GetHomeKeycode`: the key the debug console opens with.
pub fn get_home_keycode() -> i32 {
	vk::HOME as i32
}

/// `GetFunctionKeycode`: F1 to F12, and 0 for a number outside that.
pub fn get_function_keycode(key: i32) -> i32 {
	match key {
		1 => vk::F1 as i32,
		2 => vk::F1 as i32 + 1,
		3 => vk::F1 as i32 + 2,
		4 => vk::F1 as i32 + 3,
		5 => vk::F1 as i32 + 4,
		6 => vk::F1 as i32 + 5,
		7 => vk::F1 as i32 + 6,
		8 => vk::F1 as i32 + 7,
		9 => vk::F1 as i32 + 8,
		10 => vk::F1 as i32 + 9,
		11 => vk::F1 as i32 + 10,
		12 => vk::F12 as i32,

		_ => 0,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::{Config, ControlKey, TOTAL_ControlKey};
	use xops_platform::winit::dpi::PhysicalPosition;
	use xops_platform::winit::event::WindowEvent;

	/// A cursor move, which is where the platform accumulates movement from.
	fn move_cursor(x: f64, y: f64) -> WindowEvent {
		WindowEvent::CursorMoved {
			// The event is never routed anywhere, so the device it names is never read.
			device_id: unsafe { std::mem::zeroed() },
			position: PhysicalPosition::new(x, y),
		}
	}

	/// The upstream limit, and the reason the options screen has a switch for it.
	#[test]
	fn a_frame_of_mouse_movement_is_clamped_to_a_signed_byte() {
		let mut platform = Input::default();
		platform.set_captured(true);
		platform.handle(&move_cursor(400.0, -300.0));

		let mut control = InputControl::new();
		control.get_input_state(&mut platform, true);
		assert_eq!(control.get_mouse_movement(), (127, -128));

		// The accumulator was consumed, so a frame that moved nowhere reads nothing.
		control.get_input_state(&mut platform, true);
		assert_eq!(control.get_mouse_movement(), (0, 0));
	}

	/// The same frame with the switch off, which is what a player turning it off gets.
	#[test]
	fn the_limit_flag_off_leaves_the_frame_of_movement_alone() {
		let mut platform = Input::default();
		platform.set_captured(true);
		platform.handle(&move_cursor(400.0, -300.0));

		let mut control = InputControl::new();
		control.set_mouse_limit_flag(false);
		control.get_input_state(&mut platform, true);
		assert_eq!(control.get_mouse_movement(), (400, -300));
	}

	/// The two modes read different things, and the absolute one drops the movement.
	#[test]
	fn absolute_mode_reads_the_cursor_and_drops_the_movement() {
		let mut platform = Input::default();
		platform.handle(&move_cursor(30.0, 5.0));
		platform.set_captured(true);
		platform.handle(&move_cursor(40.0, 9.0));

		let mut control = InputControl::new();
		control.get_input_state(&mut platform, false);
		assert_eq!(control.get_mouse_movement(), (40, 9));

		// The movement of that frame was dropped by the absolute read, not held for the
		// next one.
		control.get_input_state(&mut platform, true);
		assert_eq!(control.get_mouse_movement(), (0, 0));
	}

	/// The edge checks are the previous frame's table against this one's.
	#[test]
	fn the_key_edges_are_the_previous_frame_compared_with_this_one() {
		let mut control = InputControl::new();
		control.keys[0x57] = 0x80;

		assert!(control.check_key_now(0x57));
		assert!(control.check_key_down(0x57));
		assert!(!control.check_key_up(0x57));

		control.keys_lt = control.keys;
		assert!(control.check_key_now(0x57));
		assert!(!control.check_key_down(0x57));
		assert!(!control.check_key_up(0x57));

		control.keys[0x57] = 0x00;
		assert!(!control.check_key_now(0x57));
		assert!(!control.check_key_down(0x57));
		assert!(control.check_key_up(0x57));

		// A key id outside the table is not a key.
		assert!(!control.check_key_now(-1));
		assert!(!control.check_key_now(256));
		assert!(!control.check_key_up(-1));
		assert!(!control.check_key_up(256));
	}

	/// Every rebindable key, from the code the config stores to the code the keyboard
	/// table is read with. A wrong row here is a key that does nothing or a key that does
	/// something else.
	#[test]
	fn every_control_key_translates_its_default_binding_to_a_virtual_key() {
		let mut config = Config::new();
		config.set_default_config();

		let expected: [(ControlKey, i32); TOTAL_ControlKey] = [
			(ControlKey::KEY_TURNUP, vk::UP as i32),
			(ControlKey::KEY_TURNDOWN, vk::DOWN as i32),
			(ControlKey::KEY_TURNLEFT, vk::LEFT as i32),
			(ControlKey::KEY_TURNRIGHT, vk::RIGHT as i32),
			(ControlKey::KEY_MOVEFORWARD, b'W' as i32),
			(ControlKey::KEY_MOVEBACKWARD, b'S' as i32),
			(ControlKey::KEY_MOVELEFT, b'A' as i32),
			(ControlKey::KEY_MOVERIGHT, b'D' as i32),
			(ControlKey::KEY_WALK, vk::TAB as i32),
			(ControlKey::KEY_JUMP, vk::SPACE as i32),
			(ControlKey::KEY_RELOAD, b'R' as i32),
			(ControlKey::KEY_DROPWEAPON, b'G' as i32),
			(ControlKey::KEY_ZOOMSCOPE, vk::SHIFT as i32),
			(ControlKey::KEY_SHOTMODE, b'X' as i32),
			(ControlKey::KEY_SWITCHWEAPON, b'Z' as i32),
			(ControlKey::KEY_WEAPON1, b'1' as i32),
			(ControlKey::KEY_WEAPON2, b'2' as i32),
			(ControlKey::KEY_SHOT, -1),
		];

		for (key, code) in expected {
			let bound = config.get_keycode(key as i32);
			assert_eq!(
				originalkeycode_to_dinputdef(bound),
				code,
				"{key:?} is bound to {bound:#04X}"
			);
		}
	}

	/// The key codes the rest of the game asks for by name.
	#[test]
	fn the_named_key_codes_are_the_windows_ones() {
		assert_eq!(get_esc_keycode(), 0x1B);
		assert_eq!(get_home_keycode(), 0x24);
		assert_eq!(get_function_keycode(1), 0x70);
		assert_eq!(get_function_keycode(12), 0x7B);
		assert_eq!(get_function_keycode(0), 0);
		assert_eq!(get_function_keycode(13), 0);
	}

	/// The original keycodes the screens name by number rather than through the config:
	/// the menu and the camera keys, the text entry keys and both minus and period keys.
	/// A wrong row here is a key that stops working rather than one that is rebound.
	#[test]
	fn the_key_codes_the_screens_ask_for_by_number() {
		let expected = [
			(0x00, vk::UP as i32),
			(0x01, vk::DOWN as i32),
			(0x02, vk::LEFT as i32),
			(0x03, vk::RIGHT as i32),
			(0x08, vk::NUMPAD0 as i32 + 4),
			(0x09, vk::NUMPAD0 as i32 + 5),
			(0x0A, vk::NUMPAD0 as i32 + 6),
			(0x0C, vk::NUMPAD0 as i32 + 8),
			(0x0E, vk::BACK as i32),
			(0x0F, vk::RETURN as i32),
			(0x11, vk::SPACE as i32),
			(0x14, vk::SHIFT as i32),
			(0x15, vk::CONTROL as i32),
			(0x3D, vk::OEM_MINUS as i32),
			(0x43, vk::OEM_COMMA as i32),
			(0x44, vk::OEM_PERIOD as i32),
			(0x48, vk::ADD as i32),
			(0x49, vk::SUBTRACT as i32),
			(0x4A, vk::DECIMAL as i32),
		];

		for (original, code) in expected {
			assert_eq!(
				originalkeycode_to_dinputdef(original),
				code,
				"original keycode {original:#04X}"
			);
		}

		// The table stops at 0x4A, and the mouse is the two negatives.
		assert_eq!(originalkeycode_to_dinputdef(0x4B), 0);
		assert_eq!(originalkeycode_to_dinputdef(-1), 0);
		assert_eq!(originalkeycode_to_dinputdef(0x12), -1);
		assert_eq!(originalkeycode_to_dinputdef(0x13), -2);
	}
}
