//! The configuration file, from the upstream `config.cpp` and `config.h`.
//!
//! Two files carry the settings. `config.dat` is the raw binary file the upstream reads
//! field by field, and `config-openxops.ini` is the additional settings file that goes
//! through `INIFileInterface`. Both are kept, because the upstream keeps both.

use crate::core::*;
use crate::datafile::INIFileInterface;
use crate::debug::{DebugLog, LOG_COMPLETE, LOG_LOAD};
use std::path::Path;

/// Total number of control keys, `TOTAL_ControlKey` upstream. The name is upstream's, so
/// it keeps the mid-word capital rather than becoming a Rust constant name.
#[allow(non_upper_case_globals)]
pub const TOTAL_ControlKey: usize = 18;

/// Maximum number of characters in the player name, `MAX_PLAYERNAME` upstream.
pub const MAX_PLAYERNAME: usize = 21;

/// The keys the player binds.
///
/// The discriminants are the order the key codes sit in `config.dat` and the index the
/// input layer reads the keyboard state with, so they are written out rather than implied.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum ControlKey {
	KEY_TURNUP = 0,
	KEY_TURNDOWN = 1,
	KEY_TURNLEFT = 2,
	KEY_TURNRIGHT = 3,
	KEY_MOVEFORWARD = 4,
	KEY_MOVEBACKWARD = 5,
	KEY_MOVELEFT = 6,
	KEY_MOVERIGHT = 7,
	KEY_WALK = 8,
	KEY_JUMP = 9,
	KEY_RELOAD = 10,
	KEY_DROPWEAPON = 11,
	KEY_ZOOMSCOPE = 12,
	KEY_SHOTMODE = 13,
	KEY_SWITCHWEAPON = 14,
	KEY_WEAPON1 = 15,
	KEY_WEAPON2 = 16,
	KEY_SHOT = 17,
}

/// Size of `config.dat`: the key codes, eight single byte settings, then the name.
pub const CONFIG_FILE_SIZE: usize = TOTAL_ControlKey + 8 + MAX_PLAYERNAME;

/// Reads `config.dat` one byte at a time, the way the upstream does.
///
/// The upstream never checks what `fread` returned, so a file that ends early leaves the
/// fields after the end holding the last byte that was successfully read. That is
/// reproduced here rather than turned into an error: `last` is what the upstream `char buf`
/// still contains. The first read of an empty file is the one case the upstream leaves
/// undefined (its `buf` is uninitialised), and this reads it as zero.
struct FileCursor<'a> {
	data: &'a [u8],
	pos: usize,
	last: u8,
}

impl<'a> FileCursor<'a> {
	fn next(&mut self) -> u8 {
		if let Some(&byte) = self.data.get(self.pos) {
			self.last = byte;
		}
		self.pos += 1;
		self.last
	}

	/// The upstream reads into a `char`, which is signed on the target it was written
	/// for, and then widens to `int`. A byte of 0xFF therefore reads as -1.
	fn next_int(&mut self) -> i32 {
		self.next() as i8 as i32
	}

	fn next_flag(&mut self) -> bool {
		self.next() != 0x00
	}
}

/// The player's settings, from the upstream `Config` class.
pub struct Config {
	keycode: [i32; TOTAL_ControlKey],
	mouse_sensitivity: i32,
	fullscreen_flag: bool,
	sound_flag: bool,
	blood_flag: bool,
	brightness: i32,
	invert_mouse_flag: bool,
	frameskip_flag: bool,
	another_gunsight_flag: bool,
	player_name: [u8; MAX_PLAYERNAME],

	language: i32,
	show_arm_flag: bool,
	screen_width: i32,
	screen_height: i32,
	mouse_limit_flag: bool,
	mastervolume: f32,
}

impl Config {
	/// The upstream constructor.
	///
	/// The name is one empty string after the constructor, so the bytes past the
	/// terminator are whatever the previous contents of the object were. Here they are
	/// zeroes, which is the only value that can be reasoned about.
	pub fn new() -> Self {
		Self {
			keycode: [0; TOTAL_ControlKey],
			mouse_sensitivity: 0,
			fullscreen_flag: false,
			sound_flag: false,
			blood_flag: false,
			brightness: 0,
			invert_mouse_flag: false,
			frameskip_flag: false,
			another_gunsight_flag: false,
			player_name: [0; MAX_PLAYERNAME],

			language: 0,
			show_arm_flag: false,
			screen_width: 0,
			screen_height: 0,
			mouse_limit_flag: true,
			mastervolume: 0.0,
		}
	}

	/// Reads the configuration file. Returns 0 on success, 1 on failure.
	///
	/// The log is a parameter because the upstream writes to a file scope `OutputLog`.
	pub fn load_file(&mut self, path: impl AsRef<Path>, log: &mut DebugLog) -> i32 {
		let path = path.as_ref();
		log.write_log(LOG_LOAD, b"Config File", path.as_os_str().as_encoded_bytes());

		let Ok(data) = xops_platform::fs::read_file(path) else {
			return 1;
		};

		self.load_from_bytes(&data);

		log.write_log(LOG_COMPLETE, b"", b"");
		0
	}

	/// Writes the configuration file. Returns 0 on success, 1 on failure.
	pub fn save_file(&self, path: impl AsRef<Path>, log: &mut DebugLog) -> i32 {
		let path = path.as_ref();
		log.write_log(LOG_LOAD, b"Config File (Save)", path.as_os_str().as_encoded_bytes());

		if xops_platform::fs::write_file(path, &self.save_to_bytes()).is_err() {
			return 1;
		}

		log.write_log(LOG_COMPLETE, b"", b"");
		0
	}

	/// The body of `LoadFile`, over bytes rather than a file.
	pub fn load_from_bytes(&mut self, data: &[u8]) {
		let mut cur = FileCursor { data, pos: 0, last: 0 };

		for slot in self.keycode.iter_mut() {
			*slot = cur.next_int();
		}
		self.mouse_sensitivity = cur.next_int();
		self.fullscreen_flag = cur.next_flag();
		self.sound_flag = cur.next_flag();
		self.blood_flag = cur.next_flag();
		self.brightness = cur.next_int();
		self.invert_mouse_flag = cur.next_flag();
		self.frameskip_flag = cur.next_flag();
		self.another_gunsight_flag = cur.next_flag();

		for byte in self.player_name.iter_mut() {
			*byte = cur.next();
		}
	}

	/// The body of `SaveFile`. Each value is written back as one byte, so a key code
	/// outside 0..255 is truncated exactly as the upstream `(char)` cast truncates it.
	pub fn save_to_bytes(&self) -> [u8; CONFIG_FILE_SIZE] {
		let mut out = [0u8; CONFIG_FILE_SIZE];
		let mut n = 0;

		for &code in self.keycode.iter() {
			out[n] = code as u8;
			n += 1;
		}
		out[n] = self.mouse_sensitivity as u8;
		n += 1;
		out[n] = self.fullscreen_flag as u8;
		n += 1;
		out[n] = self.sound_flag as u8;
		n += 1;
		out[n] = self.blood_flag as u8;
		n += 1;
		out[n] = self.brightness as u8;
		n += 1;
		out[n] = self.invert_mouse_flag as u8;
		n += 1;
		out[n] = self.frameskip_flag as u8;
		n += 1;
		out[n] = self.another_gunsight_flag as u8;
		n += 1;

		out[n..n + MAX_PLAYERNAME].copy_from_slice(&self.player_name);
		out
	}

	/// Initializes the setting values to their defaults.
	///
	/// `main.cpp` calls this only when the config file is missing and
	/// `ENABLE_AUTOCREATECONFIG` is set, which it is not, so nothing calls it yet.
	pub fn set_default_config(&mut self) {
		self.keycode[ControlKey::KEY_TURNUP as usize] = 0;
		self.keycode[ControlKey::KEY_TURNDOWN as usize] = 1;
		self.keycode[ControlKey::KEY_TURNLEFT as usize] = 2;
		self.keycode[ControlKey::KEY_TURNRIGHT as usize] = 3;
		self.keycode[ControlKey::KEY_MOVEFORWARD as usize] = 54;
		self.keycode[ControlKey::KEY_MOVEBACKWARD as usize] = 50;
		self.keycode[ControlKey::KEY_MOVELEFT as usize] = 32;
		self.keycode[ControlKey::KEY_MOVERIGHT as usize] = 35;
		self.keycode[ControlKey::KEY_WALK as usize] = 16;
		self.keycode[ControlKey::KEY_JUMP as usize] = 17;
		self.keycode[ControlKey::KEY_RELOAD as usize] = 49;
		self.keycode[ControlKey::KEY_DROPWEAPON as usize] = 38;
		self.keycode[ControlKey::KEY_ZOOMSCOPE as usize] = 20;
		self.keycode[ControlKey::KEY_SHOTMODE as usize] = 55;
		self.keycode[ControlKey::KEY_SWITCHWEAPON as usize] = 57;
		self.keycode[ControlKey::KEY_WEAPON1 as usize] = 23;
		self.keycode[ControlKey::KEY_WEAPON2 as usize] = 24;
		self.keycode[ControlKey::KEY_SHOT as usize] = 18;

		self.mouse_sensitivity = 25;
		self.fullscreen_flag = true;
		self.sound_flag = true;
		self.blood_flag = true;
		self.brightness = 4;
		self.invert_mouse_flag = false;
		self.frameskip_flag = false;
		self.another_gunsight_flag = false;

		self.set_player_name(b"xopsplayer");
	}

	/// Gets a key code. An id outside the key list reads as no key, as the upstream clamps.
	pub fn get_keycode(&self, id: i32) -> i32 {
		if id < 0 || TOTAL_ControlKey as i32 <= id {
			return 0;
		}

		self.keycode[id as usize]
	}

	/// Sets a key code. An id outside the key list is ignored.
	pub fn set_keycode(&mut self, id: i32, key: i32) {
		if id < 0 || TOTAL_ControlKey as i32 <= id {
			return;
		}

		self.keycode[id as usize] = key;
	}

	pub fn get_mouse_sensitivity(&self) -> i32 {
		self.mouse_sensitivity
	}

	pub fn set_mouse_sensitivity(&mut self, param: i32) {
		self.mouse_sensitivity = param;
	}

	pub fn get_fullscreen_flag(&self) -> bool {
		self.fullscreen_flag
	}

	pub fn set_fullscreen_flag(&mut self, flag: bool) {
		self.fullscreen_flag = flag;
	}

	pub fn get_sound_flag(&self) -> bool {
		self.sound_flag
	}

	pub fn set_sound_flag(&mut self, flag: bool) {
		self.sound_flag = flag;
	}

	pub fn get_blood_flag(&self) -> bool {
		self.blood_flag
	}

	pub fn set_blood_flag(&mut self, flag: bool) {
		self.blood_flag = flag;
	}

	pub fn get_brightness(&self) -> i32 {
		self.brightness
	}

	pub fn set_brightness(&mut self, param: i32) {
		self.brightness = param;
	}

	pub fn get_invert_mouse_flag(&self) -> bool {
		self.invert_mouse_flag
	}

	pub fn set_invert_mouse_flag(&mut self, flag: bool) {
		self.invert_mouse_flag = flag;
	}

	pub fn get_frameskip_flag(&self) -> bool {
		self.frameskip_flag
	}

	pub fn set_frameskip_flag(&mut self, flag: bool) {
		self.frameskip_flag = flag;
	}

	pub fn get_another_gunsight_flag(&self) -> bool {
		self.another_gunsight_flag
	}

	pub fn set_another_gunsight_flag(&mut self, flag: bool) {
		self.another_gunsight_flag = flag;
	}

	/// Gets the player name, into the caller's buffer.
	///
	/// Returns the length of the stored name, which is what the upstream `strlen` counts,
	/// even when the buffer was too short to take all of it.
	pub fn get_player_name(&self, out_str: &mut [u8]) -> i32 {
		let len = self.player_name_len();
		// The upstream `strcpy` writes the name and its terminator, so the buffer has to
		// keep one byte for the terminator to carry the whole name.
		let copy = len.min(out_str.len().saturating_sub(1));
		out_str[..copy].copy_from_slice(&self.player_name[..copy]);
		if copy < out_str.len() {
			out_str[copy] = 0x00;
		}
		len as i32
	}

	/// Sets the player name, cutting it to the length the field can hold.
	pub fn set_player_name(&mut self, in_str: &[u8]) {
		// `strcpy` stops at the terminator and the upstream then cuts the string to the
		// field with one write, so both happen before the copy.
		let len = in_str
			.iter()
			.position(|&byte| byte == 0x00)
			.unwrap_or(in_str.len())
			.min(MAX_PLAYERNAME - 1);

		self.player_name[..len].copy_from_slice(&in_str[..len]);
		self.player_name[len] = 0x00;
	}

	/// The length of the stored name, `strlen` upstream. A name loaded from a file with no
	/// terminator in it counts as the full field, where the upstream reads past the end.
	fn player_name_len(&self) -> usize {
		self.player_name
			.iter()
			.position(|&byte| byte == 0x00)
			.unwrap_or(MAX_PLAYERNAME)
	}

	/// Gets the name of an original key code. The names are ASCII.
	pub fn get_originalkeycode_string(&self, code: i32) -> &'static [u8] {
		match code {
			0x00 => b"UP",
			0x01 => b"DOWN",
			0x02 => b"LEFT",
			0x03 => b"RIGHT",
			0x04 => b"NUM0",
			0x05 => b"NUM1",
			0x06 => b"NUM2",
			0x07 => b"NUM3",
			0x08 => b"NUM4",
			0x09 => b"NUM5",
			0x0A => b"NUM6",
			0x0B => b"NUM7",
			0x0C => b"NUM8",
			0x0D => b"NUM9",
			0x0E => b"BACKSPACE",
			0x0F => b"ENTER",

			0x10 => b"TAB",
			0x11 => b"SPACE",
			0x12 => b"MOUSE L",
			0x13 => b"MOUSE R",
			0x14 => b"SHIFT",
			0x15 => b"CTRL",
			0x16 => b"0",
			0x17 => b"1",
			0x18 => b"2",
			0x19 => b"3",
			0x1A => b"4",
			0x1B => b"5",
			0x1C => b"6",
			0x1D => b"7",
			0x1E => b"8",
			0x1F => b"9",

			0x20 => b"A",
			0x21 => b"B",
			0x22 => b"C",
			0x23 => b"D",
			0x24 => b"E",
			0x25 => b"F",
			0x26 => b"G",
			0x27 => b"H",
			0x28 => b"I",
			0x29 => b"J",
			0x2A => b"K",
			0x2B => b"L",
			0x2C => b"M",
			0x2D => b"N",
			0x2E => b"O",
			0x2F => b"P",

			0x30 => b"Q",
			0x31 => b"R",
			0x32 => b"S",
			0x33 => b"T",
			0x34 => b"U",
			0x35 => b"V",
			0x36 => b"W",
			0x37 => b"X",
			0x38 => b"Y",
			0x39 => b"Z",
			0x3A => b"/",
			0x3B => b":",
			0x3C => b";",
			0x3D => b"-",
			0x3E => b"@",
			0x3F => b"[",

			0x40 => b"]",
			0x41 => b"BACKSLASH",
			0x42 => b"\\",
			0x43 => b",",
			0x44 => b".",
			0x45 => b"^",
			0x46 => b"NUM*",
			0x47 => b"NUM/",
			0x48 => b"NUM+",
			0x49 => b"NUM-",
			0x4A => b"NUM.",

			_ => b"NULL",
		}
	}

	/// Creates the additional configuration file.
	///
	/// Returns true when the file was written, false when there was nothing to do because
	/// it already exists, or when it could not be written.
	pub fn create_ext_config(&self, path: impl AsRef<Path>, log: &mut DebugLog) -> bool {
		let path = path.as_ref();

		// The file is not needed if it is already there.
		if xops_platform::fs::exists(path) {
			return false;
		}

		log.write_log(
			LOG_LOAD,
			b"Config File (Create .ini)",
			path.as_os_str().as_encoded_bytes(),
		);

		let contents = "[Game]\n\
			;Game language.\n\
			;  0: English + Japanese  (default)\n\
			;  1: English + Korean\n\
			Language = 0\n\
			\n\
			;Show player arm and weapon.\n\
			;  0: OFF (default)\n\
			;  1: ON\n\
			ShowArm = 0\n\
			\n\
			\n\
			[Graphics]\n\
			;WindowSize\n\
			;  0: 640x480  [4:3]\n\
			;  1: 800x600  [4:3]\n\
			;  2:1024x768  [4:3]\n\
			;  3:1280x960  [4:3] (default)\n\
			;  4:1600x1200 [4:3]\n\
			; 10:1280x720  [16:9]\n\
			; 11:1600x900  [16:9]\n\
			; 12:1920x1080 [16:9]\n\
			WindowSize = 3\n\
			\n\
			\n\
			[Input]\n\
			;Limit mouse input control.\n\
			;  0: OFF\n\
			;  1: ON (default)\n\
			MouseLimit = 1\n\
			\n\
			\n\
			[Sounds]\n\
			;Game sound volume. (0.0 ~ 1.0)\n\
			MasterVolume = 1.0\n\
			\n";

		if xops_platform::fs::write_file(path, contents.as_bytes()).is_err() {
			return false;
		}

		log.write_log(LOG_COMPLETE, b"", b"");
		true
	}

	/// Reads the additional configuration file. Returns 0 on success, 1 on failure.
	///
	/// With `ENABLE_ADDCONFIG` off the upstream sets the values in `SetDefaultExtConfig`
	/// here instead of reading anything, and reads as zero either way.
	pub fn load_ext_file(&mut self, path: impl AsRef<Path>, log: &mut DebugLog) -> i32 {
		let path = path.as_ref();
		log.write_log(LOG_LOAD, b"Config File (.ini)", path.as_os_str().as_encoded_bytes());

		let mut ini = INIFileInterface::new();
		ini.load_ini_file(path);

		self.language = ini.get_ini_file_int(b"Game", b"Language", 0, None);

		let mode = ini.get_ini_file_int(b"Game", b"ShowArm", 0, None);
		self.show_arm_flag = mode != 0;

		// The ini's own default, which is a mode number rather than a size. Three is
	// 1280x960, the largest of the four the original offered.
	let mode = ini.get_ini_file_int(b"Graphics", b"WindowSize", 3, None);
		(self.screen_width, self.screen_height) = match mode {
			0 => (640, 480),
			1 => (800, 600),
			2 => (1024, 768),
			3 => (1280, 960),
			4 => (1600, 1200),
			10 => (1280, 720),
			11 => (1600, 900),
			12 => (1920, 1080),
			13 => (2560, 1440),
			14 => (3840, 2160),
			_ => (DEFAULT_SCREEN_WIDTH, DEFAULT_SCREEN_HEIGHT),
		};

		let mode = ini.get_ini_file_int(b"Input", b"MouseLimit", 1, None);
		self.mouse_limit_flag = mode != 0;

		self.mastervolume = ini.get_ini_file_float(b"Sounds", b"MasterVolume", 1.0, None);
		if self.mastervolume < 0.0 {
			self.mastervolume = 0.0;
		}
		if self.mastervolume > 1.0 {
			self.mastervolume = 1.0;
		}

		ini.release_ini_file();

		log.write_log(LOG_COMPLETE, b"", b"");
		0
	}

	/// Initializes the additional setting values to their defaults.
	pub fn set_default_ext_config(&mut self) {
		self.language = 0;
		self.show_arm_flag = false;
		self.screen_width = DEFAULT_SCREEN_WIDTH;
		self.screen_height = DEFAULT_SCREEN_HEIGHT;
		self.mouse_limit_flag = true;
		self.mastervolume = 1.0;
	}

	pub fn get_language(&self) -> i32 {
		self.language
	}

	pub fn get_show_arm_flag(&self) -> bool {
		self.show_arm_flag
	}

	/// Sets the screen size, which the frame loop does when the window is resized: the
	/// game's full-screen overlays are drawn in this size, so leaving it behind draws them
	/// in the old one over part of the window.
	pub fn set_screen_size(&mut self, width: i32, height: i32) {
		self.screen_width = width;
		self.screen_height = height;
	}

	pub fn get_screen_width(&self) -> i32 {
		self.screen_width
	}

	pub fn get_screen_height(&self) -> i32 {
		self.screen_height
	}

	pub fn get_mouse_limit_flag(&self) -> bool {
		self.mouse_limit_flag
	}

	pub fn get_volume(&self) -> f32 {
		self.mastervolume
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A path under the temporary directory, unique to the test that asks for it.
	fn temp_path(name: &str) -> String {
		let mut path = std::env::temp_dir();
		path.push(format!("xops-config-{}-{}.dat", name, std::process::id()));
		path.to_string_lossy().into_owned()
	}

	/// A log that is not enabled, so a test writes no log file.
	fn silent_log() -> DebugLog {
		DebugLog::new()
	}

	#[test]
	fn constructor_matches_the_upstream_initialisers() {
		let config = Config::new();

		for id in 0..TOTAL_ControlKey as i32 {
			assert_eq!(config.get_keycode(id), 0, "key {id}");
		}
		assert_eq!(config.get_mouse_sensitivity(), 0);
		assert_eq!(config.get_brightness(), 0);
		assert!(!config.get_fullscreen_flag());
		assert!(!config.get_sound_flag());
		assert!(!config.get_blood_flag());
		assert!(!config.get_invert_mouse_flag());
		assert!(!config.get_frameskip_flag());
		assert!(!config.get_another_gunsight_flag());
		assert_eq!(config.get_player_name(&mut [0u8; MAX_PLAYERNAME + 1]), 0);

		assert_eq!(config.get_language(), 0);
		assert!(!config.get_show_arm_flag());
		assert_eq!(config.get_screen_width(), 0);
		assert_eq!(config.get_screen_height(), 0);
		assert!(config.get_mouse_limit_flag());
		assert_eq!(config.get_volume(), 0.0);
	}

	#[test]
	fn default_config_has_the_upstream_key_codes() {
		let mut config = Config::new();
		config.set_default_config();

		let expected = [
			(ControlKey::KEY_TURNUP, 0),
			(ControlKey::KEY_TURNDOWN, 1),
			(ControlKey::KEY_TURNLEFT, 2),
			(ControlKey::KEY_TURNRIGHT, 3),
			(ControlKey::KEY_MOVEFORWARD, 54),
			(ControlKey::KEY_MOVEBACKWARD, 50),
			(ControlKey::KEY_MOVELEFT, 32),
			(ControlKey::KEY_MOVERIGHT, 35),
			(ControlKey::KEY_WALK, 16),
			(ControlKey::KEY_JUMP, 17),
			(ControlKey::KEY_RELOAD, 49),
			(ControlKey::KEY_DROPWEAPON, 38),
			(ControlKey::KEY_ZOOMSCOPE, 20),
			(ControlKey::KEY_SHOTMODE, 55),
			(ControlKey::KEY_SWITCHWEAPON, 57),
			(ControlKey::KEY_WEAPON1, 23),
			(ControlKey::KEY_WEAPON2, 24),
			(ControlKey::KEY_SHOT, 18),
		];
		for (key, code) in expected {
			assert_eq!(config.get_keycode(key as i32), code, "{key:?}");
		}

		assert_eq!(config.get_mouse_sensitivity(), 25);
		assert!(config.get_fullscreen_flag());
		assert!(config.get_sound_flag());
		assert!(config.get_blood_flag());
		assert_eq!(config.get_brightness(), 4);
		assert!(!config.get_invert_mouse_flag());
		assert!(!config.get_frameskip_flag());
		assert!(!config.get_another_gunsight_flag());

		let mut name = [0u8; MAX_PLAYERNAME + 1];
		assert_eq!(config.get_player_name(&mut name), 10);
		assert_eq!(&name[..10], b"xopsplayer");
	}

	#[test]
	fn default_ext_config_matches_the_upstream_values() {
		let mut config = Config::new();
		config.set_default_ext_config();

		assert_eq!(config.get_language(), 0);
		assert!(!config.get_show_arm_flag());
		assert_eq!(config.get_screen_width(), DEFAULT_SCREEN_WIDTH);
		assert_eq!(config.get_screen_height(), DEFAULT_SCREEN_HEIGHT);
		assert!(config.get_mouse_limit_flag());
		assert_eq!(config.get_volume(), 1.0);
	}

	#[test]
	fn key_code_accessors_ignore_ids_outside_the_list() {
		let mut config = Config::new();

		assert_eq!(config.get_keycode(-1), 0);
		assert_eq!(config.get_keycode(TOTAL_ControlKey as i32), 0);
		assert_eq!(config.get_keycode(i32::MAX), 0);

		config.set_keycode(ControlKey::KEY_SHOT as i32, 18);
		config.set_keycode(-1, 99);
		config.set_keycode(TOTAL_ControlKey as i32, 99);
		config.set_keycode(i32::MIN, 99);

		assert_eq!(config.get_keycode(ControlKey::KEY_SHOT as i32), 18);
		assert_eq!(config.get_keycode(0), 0);
		assert_eq!(config.get_keycode(TOTAL_ControlKey as i32 - 2), 0);
	}

	#[test]
	fn loaded_bytes_land_in_the_right_fields() {
		// One byte per field, in the order the upstream reads them.
		let mut data = Vec::new();
		for i in 0..TOTAL_ControlKey as u8 {
			data.push(i + 1);
		}
		data.push(200); // mouse sensitivity
		data.push(1); // fullscreen
		data.push(0); // sound
		data.push(1); // blood
		data.push(7); // brightness
		data.push(1); // invert mouse
		data.push(0); // frame skip
		data.push(1); // another gunsight
		data.extend_from_slice(b"player");
		data.extend_from_slice(&[0u8; MAX_PLAYERNAME - 6]);
		assert_eq!(data.len(), CONFIG_FILE_SIZE);

		let mut config = Config::new();
		config.load_from_bytes(&data);

		assert_eq!(config.get_keycode(ControlKey::KEY_TURNUP as i32), 1);
		assert_eq!(config.get_keycode(ControlKey::KEY_SHOT as i32), 18);
		// A byte above 0x7F widens through a signed char, as it does upstream.
		assert_eq!(config.get_mouse_sensitivity(), -56);
		assert!(config.get_fullscreen_flag());
		assert!(!config.get_sound_flag());
		assert!(config.get_blood_flag());
		assert_eq!(config.get_brightness(), 7);
		assert!(config.get_invert_mouse_flag());
		assert!(!config.get_frameskip_flag());
		assert!(config.get_another_gunsight_flag());

		let mut name = [0u8; MAX_PLAYERNAME + 1];
		assert_eq!(config.get_player_name(&mut name), 6);
		assert_eq!(&name[..6], b"player");
	}

	#[test]
	fn a_short_file_leaves_the_last_byte_read_in_the_fields_past_the_end() {
		// Two bytes and nothing else. The upstream `fread` does not check the count, so
		// the second byte stays in `buf` and fills every field after it.
		let mut config = Config::new();
		config.load_from_bytes(&[0x11, 0x22]);

		assert_eq!(config.get_keycode(0), 0x11);
		for id in 1..TOTAL_ControlKey as i32 {
			assert_eq!(config.get_keycode(id), 0x22, "key {id}");
		}
		assert_eq!(config.get_mouse_sensitivity(), 0x22);
		assert!(config.get_fullscreen_flag());
		assert_eq!(config.get_player_name(&mut [0u8; MAX_PLAYERNAME + 1]), MAX_PLAYERNAME as i32);
	}

	#[test]
	fn player_name_is_cut_to_the_field_and_terminated() {
		let mut config = Config::new();

		config.set_player_name(b"a name that is far too long for the field");
		let mut name = [0u8; MAX_PLAYERNAME + 1];
		assert_eq!(config.get_player_name(&mut name), (MAX_PLAYERNAME - 1) as i32);
		assert_eq!(&name[..MAX_PLAYERNAME - 1], b"a name that is far t");
		assert_eq!(name[MAX_PLAYERNAME - 1], 0x00);

		// A short name still writes its terminator over the older, longer one.
		config.set_player_name(b"bob");
		assert_eq!(config.get_player_name(&mut name), 3);
		assert_eq!(&name[..4], b"bob\0");
	}

	#[test]
	fn a_name_longer_than_the_buffer_is_truncated_but_still_counted() {
		let mut config = Config::new();
		config.set_player_name(b"xopsplayer");

		let mut small = [0u8; 4];
		assert_eq!(config.get_player_name(&mut small), 10);
		assert_eq!(&small, b"xop\0");
	}

	#[test]
	fn original_key_code_names_come_from_the_table() {
		let config = Config::new();

		assert_eq!(config.get_originalkeycode_string(0x00), b"UP");
		assert_eq!(config.get_originalkeycode_string(0x12), b"MOUSE L");
		assert_eq!(config.get_originalkeycode_string(0x20), b"A");
		assert_eq!(config.get_originalkeycode_string(0x42), b"\\");
		assert_eq!(config.get_originalkeycode_string(0x4A), b"NUM.");
		assert_eq!(config.get_originalkeycode_string(0x4B), b"NULL");
		assert_eq!(config.get_originalkeycode_string(-1), b"NULL");
	}

	#[test]
	fn a_saved_file_loads_back_field_for_field() {
		let path = temp_path("roundtrip");
		let mut log = silent_log();

		let mut config = Config::new();
		config.set_default_config();
		config.set_default_ext_config();
		config.set_keycode(ControlKey::KEY_SHOT as i32, 42);
		config.set_mouse_sensitivity(13);
		config.set_fullscreen_flag(false);
		config.set_sound_flag(false);
		config.set_blood_flag(false);
		config.set_brightness(9);
		config.set_invert_mouse_flag(true);
		config.set_frameskip_flag(true);
		config.set_another_gunsight_flag(true);
		config.set_player_name(b"nametest");

		assert_eq!(config.save_file(&path, &mut log), 0);
		assert_eq!(xops_platform::fs::read_file(&path).unwrap().len() as u64, CONFIG_FILE_SIZE as u64);

		let mut loaded = Config::new();
		assert_eq!(loaded.load_file(&path, &mut log), 0);

		for id in 0..TOTAL_ControlKey as i32 {
			assert_eq!(loaded.get_keycode(id), config.get_keycode(id), "key {id}");
		}
		assert_eq!(loaded.get_mouse_sensitivity(), config.get_mouse_sensitivity());
		assert_eq!(loaded.get_fullscreen_flag(), config.get_fullscreen_flag());
		assert_eq!(loaded.get_sound_flag(), config.get_sound_flag());
		assert_eq!(loaded.get_blood_flag(), config.get_blood_flag());
		assert_eq!(loaded.get_brightness(), config.get_brightness());
		assert_eq!(loaded.get_invert_mouse_flag(), config.get_invert_mouse_flag());
		assert_eq!(loaded.get_frameskip_flag(), config.get_frameskip_flag());
		assert_eq!(
			loaded.get_another_gunsight_flag(),
			config.get_another_gunsight_flag()
		);

		let mut name = [0u8; MAX_PLAYERNAME + 1];
		let mut loaded_name = [0u8; MAX_PLAYERNAME + 1];
		assert_eq!(loaded.get_player_name(&mut loaded_name), 8);
		assert_eq!(config.get_player_name(&mut name), 8);
		assert_eq!(loaded_name, name);

		// The whole file, not just the accessors, has to match.
		assert_eq!(loaded.save_to_bytes(), config.save_to_bytes());

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn a_missing_file_fails_and_leaves_the_config_alone() {
		let mut log = silent_log();
		let mut config = Config::new();
		config.set_default_config();

		let path = temp_path("missing");
		let _ = std::fs::remove_file(&path);

		assert_eq!(config.load_file(&path, &mut log), 1);
		assert_eq!(config.get_mouse_sensitivity(), 25);
	}

	#[test]
	fn the_additional_settings_file_round_trips_through_the_ini() {
		let path = temp_path("ext.ini");
		let _ = std::fs::remove_file(&path);
		let mut log = silent_log();

		let config = Config::new();
		assert!(config.create_ext_config(&path, &mut log));
		// The file is not to be rewritten when it is already there.
		assert!(!config.create_ext_config(&path, &mut log));

		let mut loaded = Config::new();
		assert_eq!(loaded.load_ext_file(&path, &mut log), 0);
		assert_eq!(loaded.get_language(), 0);
		assert!(!loaded.get_show_arm_flag());
		assert_eq!(loaded.get_screen_width(), DEFAULT_SCREEN_WIDTH);
		assert_eq!(loaded.get_screen_height(), DEFAULT_SCREEN_HEIGHT);
		assert!(loaded.get_mouse_limit_flag());
		assert_eq!(loaded.get_volume(), 1.0);

		// The written file is the one the upstream writes, comments and all.
		let text = String::from_utf8(xops_platform::fs::read_file(&path).unwrap()).unwrap();
		assert!(text.starts_with("[Game]\n;Game language.\n"));
		assert!(text.contains("[Graphics]\n;WindowSize\n"));
		// The two entries the upstream leaves commented out are not in the file.
		assert!(!text.contains("13:2560x1440"));
		assert!(!text.contains("14:3840x2160"));
		assert!(text.ends_with("MasterVolume = 1.0\n\n"));

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn a_missing_ini_falls_back_to_the_defaults() {
		let path = temp_path("absent.ini");
		let _ = std::fs::remove_file(&path);
		let mut log = silent_log();

		let mut config = Config::new();
		assert_eq!(config.load_ext_file(&path, &mut log), 0);
		assert_eq!(config.get_language(), 0);
		assert_eq!(config.get_screen_width(), DEFAULT_SCREEN_WIDTH);
		assert_eq!(config.get_screen_height(), DEFAULT_SCREEN_HEIGHT);
		assert!(config.get_mouse_limit_flag());
		assert_eq!(config.get_volume(), 1.0);
	}

	#[test]
	fn the_volume_is_clamped_to_the_range_the_upstream_clamps_to() {
		let path = temp_path("volume.ini");
		let mut log = silent_log();

		let mut config = Config::new();

		xops_platform::fs::write_file(&path, b"[Sounds]\nMasterVolume = 4.5\n").unwrap();
		config.load_ext_file(&path, &mut log);
		assert_eq!(config.get_volume(), 1.0);

		xops_platform::fs::write_file(&path, b"[Sounds]\nMasterVolume = -2.0\n").unwrap();
		config.load_ext_file(&path, &mut log);
		assert_eq!(config.get_volume(), 0.0);

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn every_resolution_the_upstream_knows_comes_back_the_same() {
		let path = temp_path("size.ini");
		let mut log = silent_log();

		let expected = [
			(0, 640, 480),
			(1, 800, 600),
			(2, 1024, 768),
			(3, 1280, 960),
			(4, 1600, 1200),
			(10, 1280, 720),
			(11, 1600, 900),
			(12, 1920, 1080),
			(13, 2560, 1440),
			(14, 3840, 2160),
			(5, DEFAULT_SCREEN_WIDTH, DEFAULT_SCREEN_HEIGHT),
			(99, DEFAULT_SCREEN_WIDTH, DEFAULT_SCREEN_HEIGHT),
		];

		let mut config = Config::new();
		for (mode, width, height) in expected {
			xops_platform::fs::write_file(&path, format!("[Graphics]\nWindowSize = {mode}\n").as_bytes()).unwrap();
			config.load_ext_file(&path, &mut log);
			assert_eq!(config.get_screen_width(), width, "mode {mode}");
			assert_eq!(config.get_screen_height(), height, "mode {mode}");
		}

		let _ = std::fs::remove_file(&path);
	}
}
