//! Constants and helpers from the upstream `main.h` and `main.cpp`.
//!
//! This is the vocabulary every other module uses, so it is ported by hand rather than
//! by rule: a constant that is off by a digit here is off everywhere.

/// The processing time of one frame, in milliseconds. The whole game steps by this.
pub const GAMEFRAMEMS: u32 = 30;

/// Frames per second, `1000 / GAMEFRAMEMS`. Kept as the upstream computes it.
pub const GAMEFPS: f32 = 1000.0 / GAMEFRAMEMS as f32;

// The resolution used when neither the configuration nor its ini says one. Upstream's
// smallest mode, 640x480, is what the game was written for and what it falls back to here
// was, and on a modern display that is a postage stamp. The largest of upstream's own four
// modes is used instead, and the host fits it to the display when that is smaller.
pub const DEFAULT_SCREEN_WIDTH: i32 = 1280;
pub const DEFAULT_SCREEN_HEIGHT: i32 = 960;

/// Directory the ADDON mission list is read from.
pub const ADDONDIR: &[u8] = b"addon";
pub const MAX_ADDONLIST: usize = 128;

/// Number of weapons a human can carry.
pub const TOTAL_HAVEWEAPON: usize = 2;

pub const GAMENAME: &str = "OpenXOPS";
pub const GAMEVERSION: &str = "1.080";

/// The path buffer size the upstream code uses. Windows defaults it to 260 and upstream
/// defines it to the same value elsewhere, so it is one number here.
pub const MAX_PATH: usize = 260;

// ---------------------------------------------------------------------------
// Compile-time switches
// ---------------------------------------------------------------------------
//
// Upstream these are `#define`s that select behaviour, and several of them reproduce
// quirks the game is expected to have. They stay as constants so the branches they guard
// survive the port and can be flipped in one place.

/// Reproduce the bug where an out of range human or weapon type number is used.
pub const ENABLE_BUG_HUMANWEAPON: bool = true;
/// Reproduce the behaviour where a negative team number is used.
pub const ENABLE_BUG_TEAMID: bool = true;
/// Shrink the weapon name display when the name is long.
pub const ENABLE_WEAPONNAME_SCALING: bool = true;
/// Store added small object settings as 8 bit values.
pub const ENABLE_ADDOBJ_PARAM8BIT: bool = true;
/// The debug console, opened with the key the input layer watches.
pub const ENABLE_DEBUGCONSOLE: bool = true;
/// Debug log output to a file.
pub const ENABLE_DEBUGLOG: bool = true;
/// The event that detects another OpenXOPS instance running.
pub const ENABLE_CHECKOPENXOPSEVENT: bool = true;
/// The options screen in the menu.
pub const ENABLE_MENUOPTIONS: bool = true;
/// Load a `.mif` handed over as a command line argument.
pub const ENABLE_AUTOLOADMIF: bool = true;
/// Read additional settings from an ini file.
pub const ENABLE_ADDCONFIG: bool = true;
/// Check that the data folder exists at startup.
pub const ENABLE_CHECKDATADIR: bool = true;

/// Upstream leaves this one commented out, so a missing config file is an error rather
/// than something to silently create.
pub const ENABLE_AUTOCREATECONFIG: bool = false;
/// Upstream leaves this one commented out, so backslash separators are left alone.
pub const ENABLE_PATH_DELIMITER_SLASH: bool = false;

/// A flag bit helper, matching the upstream macros.
pub fn set_flag(value: u32, bit: u32) -> u32 {
	value | bit
}

pub fn del_flag(value: u32, bit: u32) -> u32 {
	value & !bit
}

pub fn get_flag(value: u32, bit: u32) -> bool {
	value & bit != 0
}

/// `DegreeToRadian`.
///
/// `const` because the parameter tables are built from it in constant initialisers, which
/// is what the upstream does with the same macro.
pub const fn degree_to_radian(value: f32) -> f32 {
	std::f32::consts::PI / 180.0 * value
}

/// Normalises an angle into -PI..PI.
///
/// The upstream loops a whole turn at a time rather than taking a remainder, and the
/// loop is kept: a remainder gives a different last bit on some inputs, and this value
/// feeds the camera.
pub fn angle_normalization(r: f32) -> f32 {
	let mut r = r;
	while r > std::f32::consts::PI {
		r -= std::f32::consts::PI * 2.0;
	}
	while r < -std::f32::consts::PI {
		r += std::f32::consts::PI * 2.0;
	}
	r
}

/// Converts the low byte of a point parameter to the value the game stores.
///
/// The widening to `u8` is deliberate: the upstream casts a `signed short int` through
/// `unsigned char`, so a parameter of 256 reads as 0 and one of -1 reads as 255.
pub fn point_param_to_int(value: i16) -> u32 {
	(value as u8) as u32
}

/// Matches a command line argument against a command name.
///
/// The first character of `cmd` is replaced, so either `/` or `-` is accepted, in either
/// case. Arguments of 16 characters or more never match, and a length mismatch fails
/// before the replacement happens.
pub fn check_command_parameter(param: &[u8], cmd: &[u8]) -> bool {
	if param.len() >= 16 || cmd.len() >= 16 {
		return false;
	}
	if param.len() != cmd.len() {
		return false;
	}
	if cmd.is_empty() {
		// Upstream would write to the first byte of a copy of an empty command and
		// compare against a one-character string, which cannot match an empty argument.
		return false;
	}

	let mut probe = cmd.to_vec();
	probe[0] = b'/';
	if param == &probe[..] {
		return true;
	}
	probe[0] = b'-';
	if param == &probe[..] {
		return true;
	}
	for byte in probe.iter_mut() {
		*byte = byte.to_ascii_lowercase();
	}
	probe[0] = b'/';
	if param == &probe[..] {
		return true;
	}
	probe[0] = b'-';
	if param == &probe[..] {
		return true;
	}
	false
}

pub fn set_2byte_little_endian(out: &mut [u8], value: u32) {
	if out.len() < 2 {
		return;
	}
	out[0] = (value & 0x00FF) as u8;
	out[1] = ((value >> 8) & 0x00FF) as u8;
}

pub fn set_4byte_little_endian(out: &mut [u8], value: u32) {
	if out.len() < 4 {
		return;
	}
	out[0] = (value & 0xFF) as u8;
	out[1] = ((value >> 8) & 0xFF) as u8;
	out[2] = ((value >> 16) & 0xFF) as u8;
	out[3] = ((value >> 24) & 0xFF) as u8;
}

pub fn set_2byte_big_endian(out: &mut [u8], value: u32) {
	if out.len() < 2 {
		return;
	}
	out[0] = ((value >> 8) & 0x00FF) as u8;
	out[1] = (value & 0x00FF) as u8;
}

pub fn set_4byte_big_endian(out: &mut [u8], value: u32) {
	if out.len() < 4 {
		return;
	}
	out[0] = ((value >> 24) & 0xFF) as u8;
	out[1] = ((value >> 16) & 0xFF) as u8;
	out[2] = ((value >> 8) & 0xFF) as u8;
	out[3] = (value & 0xFF) as u8;
}

/// Reads two bytes as a little endian number. A short slice reads as zero, which is what
/// the upstream null check amounts to for the callers that pass a checked pointer.
pub fn get_2byte_little_endian(data: &[u8]) -> u32 {
	if data.len() < 2 {
		return 0;
	}
	((data[1] as u32) << 8) + data[0] as u32
}

pub fn get_4byte_little_endian(data: &[u8]) -> u32 {
	if data.len() < 4 {
		return 0;
	}
	((data[3] as u32) << 24) + ((data[2] as u32) << 16) + ((data[1] as u32) << 8) + data[0] as u32
}

pub fn get_2byte_big_endian(data: &[u8]) -> u32 {
	if data.len() < 2 {
		return 0;
	}
	((data[0] as u32) << 8) + data[1] as u32
}

pub fn get_4byte_big_endian(data: &[u8]) -> u32 {
	if data.len() < 4 {
		return 0;
	}
	((data[0] as u32) << 24) + ((data[1] as u32) << 16) + ((data[2] as u32) << 8) + data[3] as u32
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn endian_helpers_round_trip_both_ways() {
		let mut buffer = [0u8; 4];
		set_4byte_little_endian(&mut buffer, 0x1234_5678);
		assert_eq!(buffer, [0x78, 0x56, 0x34, 0x12]);
		assert_eq!(get_4byte_little_endian(&buffer), 0x1234_5678);

		set_4byte_big_endian(&mut buffer, 0x1234_5678);
		assert_eq!(buffer, [0x12, 0x34, 0x56, 0x78]);
		assert_eq!(get_4byte_big_endian(&buffer), 0x1234_5678);

		set_2byte_little_endian(&mut buffer, 0xABCD);
		assert_eq!(&buffer[..2], &[0xCD, 0xAB]);
		assert_eq!(get_2byte_little_endian(&buffer), 0xABCD);

		set_2byte_big_endian(&mut buffer, 0xABCD);
		assert_eq!(&buffer[..2], &[0xAB, 0xCD]);
		assert_eq!(get_2byte_big_endian(&buffer), 0xABCD);
	}

	#[test]
	fn a_short_buffer_is_left_alone_rather_than_overrunning() {
		let mut buffer = [0u8; 1];
		set_4byte_little_endian(&mut buffer, 0xFFFF_FFFF);
		assert_eq!(buffer, [0]);
		assert_eq!(get_4byte_little_endian(&buffer), 0);
	}

	#[test]
	fn point_parameters_read_their_low_byte() {
		// Value 256 stores as 0 and -1 stores as 255, because upstream casts through
		// unsigned char. Getting this wrong shifts every point parameter by 256.
		assert_eq!(point_param_to_int(0), 0);
		assert_eq!(point_param_to_int(255), 255);
		assert_eq!(point_param_to_int(256), 0);
		assert_eq!(point_param_to_int(-1), 255);
		assert_eq!(point_param_to_int(-256), 0);
	}

	#[test]
	fn angles_are_normalised_into_one_turn() {
		assert!((angle_normalization(0.5) - 0.5).abs() < 1e-6);
		let wrapped = angle_normalization(std::f32::consts::PI * 3.0);
		assert!((wrapped - std::f32::consts::PI).abs() < 1e-5, "{wrapped}");
		let wrapped = angle_normalization(-std::f32::consts::PI * 3.0);
		assert!((wrapped + std::f32::consts::PI).abs() < 1e-5, "{wrapped}");
	}

	#[test]
	fn a_command_matches_with_either_prefix_and_either_case() {
		// The first character of the command is the one that gets replaced, so all four
		// spellings are the same argument.
		for spelling in [&b"/Log"[..], b"-Log", b"/log", b"-log"] {
			assert!(
				check_command_parameter(spelling, b"/Log"),
				"{:?} should match",
				std::str::from_utf8(spelling)
			);
		}
	}

	#[test]
	fn a_command_needs_the_same_length_and_stays_under_sixteen() {
		assert!(!check_command_parameter(b"/Logx", b"/Log"));
		assert!(!check_command_parameter(b"/Lo", b"/Log"));
		// Sixteen characters or more never match, however well they line up.
		let long = b"/abcdefghijklmno";
		assert_eq!(long.len(), 16);
		assert!(!check_command_parameter(long, long));
		assert!(!check_command_parameter(b"", b""));
	}
}
