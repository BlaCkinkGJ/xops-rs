//! `datafile.cpp` and `datafile.h`: the classes that load the game's data files.
//!
//! The loaders take their path as the upstream's `char *fname`, which is Shift-JIS bytes,
//! and they read the file into memory before parsing it. `fgets` and `fread` are mirrored
//! by [`fgets`] and [`take`], which keep the upstream's short-read behaviour.
//!
//! Where upstream reads uninitialised memory the port substitutes zeros, and where it
//! writes past a fixed buffer the port clamps, so every such case is noted at the site.
//! Strings stay bytes because the texture font indexes glyphs by byte value.

use std::path::{Path, PathBuf};
use xops_platform::path::resolve_case_insensitive;

use crate::core::{
	degree_to_radian, get_2byte_little_endian, get_4byte_little_endian, ADDONDIR,
	ENABLE_ADDOBJ_PARAM8BIT, ENABLE_AUTOLOADMIF, MAX_ADDONLIST, MAX_PATH,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of blocks.
pub const MAX_BLOCKS: usize = 160;
/// Total number of textures.
pub const TOTAL_BLOCKTEXTURE: usize = 10;
/// Maximum number of points.
pub const MAX_POINTS: usize = 256;

/// Number of additional small objects.
///
/// If you increase this value, review the values of `MAX_MODEL`, `MAX_TEXTURE` and
/// `MAX_LOADSOUND` as well. The settings for the next additional small object follow the
/// previous settings in the additional small object settings file, with a dummy line
/// ("//" recommended) in between.
pub const MAX_ADDSMALLOBJECT: usize = 1;

/// Maximum number of messages in a .msg file.
pub const MAX_POINTMESSAGES: usize = 16;
/// Maximum number of bytes per line in a .msg file.
pub const MAX_POINTMESSAGEBYTE: usize = 71 + 2;

/// Horizontal axis angle of the light (used for block shading calculations).
pub const LIGHT_RX: f32 = degree_to_radian(190.0);
/// Vertical axis angle of the light (used for block shading calculations).
pub const LIGHT_RY: f32 = degree_to_radian(120.0);

// Type number of P1 for point data.
pub const POINT_P1TYPE_HUMAN: i16 = 1;
pub const POINT_P1TYPE_WEAPON: i16 = 2;
pub const POINT_P1TYPE_AIPATH: i16 = 3;
pub const POINT_P1TYPE_HUMANINFO: i16 = 4;
pub const POINT_P1TYPE_SMALLOBJ: i16 = 5;
pub const POINT_P1TYPE_HUMAN2: i16 = 6;
pub const POINT_P1TYPE_RAND_WEAPON: i16 = 7;
pub const POINT_P1TYPE_RAND_AIPATH: i16 = 8;

/// Size of the briefing text field and of the mission full name field.
const BRIEFINGTEXT_BYTE: usize = 816;
const MISSION_NAME_BYTE: usize = 24;
const MISSION_FULLNAME_BYTE: usize = 64;
/// Number of briefing text lines the loaders read.
const BRIEFING_LINES: usize = 17;

// ---------------------------------------------------------------------------
// Data structures
// ---------------------------------------------------------------------------

/// Structure for block data (used by the [`Blockdata`] structure).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BMaterial {
	/// Texture ID.
	pub texture_id: i32,
	/// Texture UV-U.
	pub u: [f32; 4],
	/// Texture UV-V.
	pub v: [f32; 4],
	/// Center.
	pub center_x: f32,
	/// Center.
	pub center_y: f32,
	/// Center.
	pub center_z: f32,
	/// Normal vector.
	pub vx: f32,
	/// Normal vector.
	pub vy: f32,
	/// Normal vector.
	pub vz: f32,
	/// Shadow.
	pub shadow: f32,
}

/// Structure for block data.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Blockdata {
	/// Data ID.
	pub id: i32,
	/// Position.
	pub x: [f32; 8],
	/// Position.
	pub y: [f32; 8],
	/// Position.
	pub z: [f32; 8],
	/// Material data.
	pub material: [BMaterial; 6],
}

/// Structure for point data.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointdata {
	/// Data ID.
	pub id: i32,
	/// Position.
	pub x: f32,
	/// Position.
	pub y: f32,
	/// Position.
	pub z: f32,
	/// Rotation.
	pub r: f32,
	/// Param.
	pub p1: i16,
	/// Param.
	pub p2: i16,
	/// Param.
	pub p3: i16,
	/// Param.
	pub p4: i16,
}

/// Structure for an additional small object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Addsmallobject {
	/// Model data path.
	pub modelpath: [u8; MAX_PATH],
	/// Texture path.
	pub texturepath: [u8; MAX_PATH],
	/// Size of the collision check.
	pub decide: i32,
	/// Durability.
	pub hp: i32,
	/// Sound data path.
	pub soundpath: [u8; MAX_PATH],
	/// Amount of jump.
	pub jump: i32,
}

impl Default for Addsmallobject {
	fn default() -> Self {
		// Written out because `[u8; MAX_PATH]` is wider than the derived array defaults.
		Addsmallobject {
			modelpath: [0; MAX_PATH],
			texturepath: [0; MAX_PATH],
			decide: 0,
			hp: 0,
			soundpath: [0; MAX_PATH],
			jump: 0,
		}
	}
}

// ---------------------------------------------------------------------------
// Byte helpers
// ---------------------------------------------------------------------------

/// The bytes of a C string: everything before the first NUL.
fn cstr(bytes: &[u8]) -> &[u8] {
	match bytes.iter().position(|&b| b == 0) {
		Some(i) => &bytes[..i],
		None => bytes,
	}
}

/// `strcpy` into a fixed buffer: the value is truncated so a NUL always fits.
fn write_cstr(dst: &mut [u8], src: &[u8]) {
	if dst.is_empty() {
		return;
	}
	let n = src.len().min(dst.len() - 1);
	dst[..n].copy_from_slice(&src[..n]);
	dst[n] = 0;
}

/// Turns a byte path into something the OS opens.
///
/// Game paths are Shift-JIS bytes with backslash separators, and every path that reaches
/// a loader here came out of one of the getters on this module, so this is the one place
/// the conversion lives. `Path` is the only OS-flavoured thing either crate's file layer
/// touches.
///
/// The two platforms want different things. Unix file names are bytes, so the bytes go
/// straight through and a Japanese name resolves as exactly what the data says. Windows
/// names its files in UTF-16, so the bytes are decoded first. Both then take the slash,
/// which is the only separator Unix has.
pub fn path_from_bytes(fname: &[u8]) -> PathBuf {
	let name = cstr(fname);

	#[cfg(unix)]
	{
		use std::os::unix::ffi::OsStrExt;
		let bytes: Vec<u8> = name
			.iter()
			.map(|&byte| if byte == b'\\' { b'/' } else { byte })
			.collect();
		PathBuf::from(std::ffi::OsStr::from_bytes(&bytes))
	}

	#[cfg(not(unix))]
	{
		let (text, _, _) = encoding_rs::SHIFT_JIS.decode(name);
		PathBuf::from(text.replace('\\', "/"))
	}
}

/// Says which file a loader could not open.
///
/// The loaders return a flag, the flag travels up as one bit of an error code, and the
/// message that finally reaches the user is "block data open failed", which names nothing.
/// Without the path there is nothing to act on: the file may be missing, the directory may
/// be the wrong one, or the game may be running somewhere else entirely.
fn report_missing(fname: &[u8]) {
	eprintln!("xops: {}: not found", path_from_bytes(fname).display());
}

fn read_file(fname: &[u8]) -> Option<Vec<u8>> {
	// The game's data names its files in whatever case it was authored with, which is not
	// necessarily the case on disk. See `xops_platform::path`.
	xops_platform::fs::read_file(resolve_case_insensitive(&path_from_bytes(fname))).ok()
}

/// Takes at most `n` bytes, ending the cursor at the end of the data.
fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> &'a [u8] {
	let end = (*pos + n).min(data.len());
	let start = (*pos).min(end);
	*pos = end;
	&data[start..end]
}

/// Reads a little endian `float`. A short slice reads as zero, where the upstream would
/// have left the uninitialised stack value in place.
fn f32_le(data: &[u8]) -> f32 {
	f32::from_bits(get_4byte_little_endian(data))
}

/// `fgets(str, size, fp)`: reads at most `size - 1` bytes and stops after a `'\n'`, which
/// is returned as part of the line. `None` is the upstream NULL, meaning end of file.
fn fgets<'a>(data: &'a [u8], pos: &mut usize, size: usize) -> Option<&'a [u8]> {
	let max = size.saturating_sub(1);
	if max == 0 || *pos >= data.len() {
		return None;
	}
	let start = *pos;
	let mut end = start;
	while end < data.len() && end - start < max {
		let byte = data[end];
		end += 1;
		if byte == b'\n' {
			break;
		}
	}
	*pos = end;
	Some(&data[start..end])
}

/// `fgets` into a fixed array followed by [`delete_linefeed`], the pattern every text
/// loader here repeats. Returns false at end of file, where the upstream `break`s.
fn read_line(dst: &mut [u8], file: &[u8], pos: &mut usize, size: usize) -> bool {
	dst.fill(0);
	match fgets(file, pos, size) {
		None => false,
		Some(line) => {
			let n = line.len().min(dst.len().saturating_sub(1));
			dst[..n].copy_from_slice(&line[..n]);
			delete_linefeed(dst);
			true
		}
	}
}

/// C's `atoi`: optional leading whitespace, an optional sign, then digits, stopping at
/// the first character that is not one. No digits reads as zero.
fn atoi(bytes: &[u8]) -> i32 {
	let s = cstr(bytes);
	let mut i = 0;
	while i < s.len() && (s[i] as char).is_ascii_whitespace() {
		i += 1;
	}
	let mut negative = false;
	if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
		negative = s[i] == b'-';
		i += 1;
	}
	let mut acc: i64 = 0;
	while i < s.len() && s[i].is_ascii_digit() {
		acc = acc.wrapping_mul(10).wrapping_add((s[i] - b'0') as i64);
		i += 1;
	}
	// The upstream overflow is undefined; the wrap keeps the width the callers see.
	let value = acc as i32;
	if negative {
		-value
	} else {
		value
	}
}

/// C's `atof`: leading whitespace, an optional sign, digits with an optional decimal
/// point and an optional exponent. Anything else at the front reads as zero and parsing
/// stops at the first character that cannot continue the number.
///
/// The hexadecimal and infinity forms `strtod` also accepts are not reproduced, no config
/// value uses them.
fn atof(bytes: &[u8]) -> f64 {
	let s = cstr(bytes);
	let mut i = 0;
	while i < s.len() && (s[i] as char).is_ascii_whitespace() {
		i += 1;
	}
	let start = i;
	if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
		i += 1;
	}
	let mut digits = 0usize;
	while i < s.len() && s[i].is_ascii_digit() {
		i += 1;
		digits += 1;
	}
	if i < s.len() && s[i] == b'.' {
		i += 1;
		while i < s.len() && s[i].is_ascii_digit() {
			i += 1;
			digits += 1;
		}
	}
	if digits == 0 {
		return 0.0;
	}
	let mut end = i;
	if i < s.len() && s[i].eq_ignore_ascii_case(&b'e') {
		let mut j = i + 1;
		if j < s.len() && (s[j] == b'+' || s[j] == b'-') {
			j += 1;
		}
		let exponent = j;
		while j < s.len() && s[j].is_ascii_digit() {
			j += 1;
		}
		if j > exponent {
			end = j;
		}
	}
	std::str::from_utf8(&s[start..end])
		.ok()
		.and_then(|text| text.parse::<f64>().ok())
		.unwrap_or(0.0)
}

/// Removes the spaces, comments and quotes from an ini line, which is how the upstream
/// finds `key=value` in it. A comment ends the line outside quotes, and spaces inside
/// quotes are kept.
fn strip_ini_line(line: &[u8]) -> Vec<u8> {
	let mut out = Vec::new();
	let mut quotationmode = 0u8;
	for &byte in line {
		if byte == b'\r' || byte == b'\n' {
			break;
		}
		if quotationmode == 0 {
			if byte == b' ' || byte == b'\t' {
				continue;
			}
			if byte == b';' {
				break;
			}
			if byte == b'\'' {
				quotationmode = 1;
				continue;
			}
			if byte == b'"' {
				quotationmode = 2;
				continue;
			}
		}
		if quotationmode == 1 && byte == b'\'' {
			quotationmode = 0;
			continue;
		}
		if quotationmode == 2 && byte == b'"' {
			quotationmode = 0;
			continue;
		}
		out.push(byte);
	}
	out
}

// ---------------------------------------------------------------------------
// Base class
// ---------------------------------------------------------------------------

/// `DataInterface`: what the data managers share.
///
/// The upstream base also declares `LoadFiledata` and `Getdata`, but the loaders disagree
/// on their arity and payload and nothing upstream reaches a manager through a
/// `DataInterface *`, so only the count is worth a trait here.
pub trait DataInterface {
	/// `GetTotaldatas`: the total number of data items.
	fn get_total_datas(&self) -> i32;
}

// ---------------------------------------------------------------------------
// Block data
// ---------------------------------------------------------------------------

/// `BlockDataInterface`: manages the block data file used as map data.
///
/// Besides loading the file it calculates the shadows of the map, which is what shades
/// it.
///
/// **This is `Clone`, and the copies are not kept in step.** The renderer takes the block
/// data by value because it draws from it every frame, while the screen keeps its own for
/// the collision and the object manager. That is safe only because every path that
/// changes the shading recomputes it and then hands the result over again: the dark screen
/// toggle calls `calculation_blockdata` and then `d3dg.load_mapdata`, and both Recovery
/// paths re-issue `load_mapdata` too. A new caller that recomputes the shading and expects
/// the renderer to notice without being handed the data again would see stale shadow.
#[derive(Clone)]
pub struct BlockDataInterface {
	datas: i32,
	data: [Blockdata; MAX_BLOCKS],
	/// Texture name.
	texture: [[u8; MAX_PATH]; TOTAL_BLOCKTEXTURE],
}

impl BlockDataInterface {
	/// Constructor.
	pub fn new() -> Self {
		BlockDataInterface {
			datas: 0,
			data: [Blockdata::default(); MAX_BLOCKS],
			texture: [[0; MAX_PATH]; TOTAL_BLOCKTEXTURE],
		}
	}

	/// `GetTotaldatas`.
	pub fn get_total_datas(&self) -> i32 {
		self.datas
	}

	/// `LoadFiledata`. Returns 0 on success, 1 on failure.
	pub fn load_file_data(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			report_missing(fname);
			return 1;
		};
		self.load_bd1(&file)
	}

	/// The body of `LoadFiledata`, over the file's bytes.
	fn load_bd1(&mut self, file: &[u8]) -> i32 {
		let mut pos = 0usize;

		// Get the textures, 31 bytes each.
		for i in 0..TOTAL_BLOCKTEXTURE {
			let name = take(file, &mut pos, 31);
			self.texture[i] = [0; MAX_PATH];
			self.texture[i][..name.len()].copy_from_slice(name);
		}

		// Get the number of data items.
		let header = take(file, &mut pos, 2);
		self.datas = get_2byte_little_endian(header) as i32;
		if self.datas > MAX_BLOCKS as i32 {
			self.datas = MAX_BLOCKS as i32;
		}

		// Read as many entries as there are blocks of data.
		for i in 0..self.datas.max(0) as usize {
			// The upstream reads 24 vertex floats, then 48 UV floats, then the 28 byte
			// texture header of the six faces.
			let main = take(file, &mut pos, 72 * 4);
			let mainb = take(file, &mut pos, 28);
			let mut floats = [0f32; 72];
			for (j, value) in floats.iter_mut().enumerate() {
				*value = f32_le(main.get(j * 4..j * 4 + 4).unwrap_or(&[]));
			}

			let mut block = Blockdata {
				id: i as i32,
				..Default::default()
			};
			for j in 0..8 {
				block.x[j] = floats[j];
				block.y[j] = floats[j + 8];
				block.z[j] = floats[j + 16];
			}
			for j in 0..6 {
				block.material[j].texture_id = *mainb.get(j * 4).unwrap_or(&0) as i32;
				block.material[j].u[0] = floats[j * 4 + 24];
				block.material[j].v[0] = floats[j * 4 + 48];
				block.material[j].u[1] = floats[j * 4 + 25];
				block.material[j].v[1] = floats[j * 4 + 49];
				block.material[j].u[2] = floats[j * 4 + 26];
				block.material[j].v[2] = floats[j * 4 + 50];
				block.material[j].u[3] = floats[j * 4 + 27];
				block.material[j].v[3] = floats[j * 4 + 51];
			}
			self.data[i] = block;
		}

		0
	}

	/// `CalculationBlockdata`: calculates the normals and shadows of the block data.
	///
	/// `darkflag` darkens the blocks. Run this once only, after loading.
	pub fn calculation_blockdata(&mut self, darkflag: bool) {
		// The light source angle. `lx` and `lz` use `LIGHT_RX` and `ly` uses `LIGHT_RY`,
		// as upstream.
		let lx = LIGHT_RX.cos();
		let ly = LIGHT_RY.sin();
		let lz = LIGHT_RX.sin();

		for i in 0..self.datas.max(0) as usize {
			let mut block = self.data[i];
			let x = block.x;
			let y = block.y;
			let z = block.z;

			for j in 0..6 {
				let mut vid = [0i32; 4];
				blockdataface(j as i32, Some(&mut vid), None);
				let v = [
					vid[0] as usize,
					vid[1] as usize,
					vid[2] as usize,
					vid[3] as usize,
				];
				let material = &mut block.material[j];

				// Center point of the 4 vertices.
				let mut center_x = 0.0f32;
				let mut center_y = 0.0f32;
				let mut center_z = 0.0f32;
				for k in 0..4 {
					center_x += x[v[k]];
					center_y += y[v[k]];
					center_z += z[v[k]];
				}
				material.center_x = center_x / 4.0;
				material.center_y = center_y / 4.0;
				material.center_z = center_z / 4.0;

				// Face normal (vector) and its length, from the first triangle.
				let xs1 = ((y[v[3]] - y[v[2]]) * (z[v[0]] - z[v[2]]))
					- ((y[v[0]] - y[v[2]]) * (z[v[3]] - z[v[2]]));
				let ys1 = ((z[v[3]] - z[v[2]]) * (x[v[0]] - x[v[2]]))
					- ((z[v[0]] - z[v[2]]) * (x[v[3]] - x[v[2]]));
				let zs1 = ((x[v[3]] - x[v[2]]) * (y[v[0]] - y[v[2]]))
					- ((x[v[0]] - x[v[2]]) * (y[v[3]] - y[v[2]]));
				let g1 = (xs1 * xs1 + ys1 * ys1 + zs1 * zs1).sqrt();

				// Then from the other triangle.
				let xs2 = ((y[v[1]] - y[v[0]]) * (z[v[2]] - z[v[0]]))
					- ((y[v[2]] - y[v[0]]) * (z[v[1]] - z[v[0]]));
				let ys2 = ((z[v[1]] - z[v[0]]) * (x[v[2]] - x[v[0]]))
					- ((z[v[2]] - z[v[0]]) * (x[v[1]] - x[v[0]]));
				let zs2 = ((x[v[1]] - x[v[0]]) * (y[v[2]] - y[v[0]]))
					- ((x[v[2]] - x[v[0]]) * (y[v[1]] - y[v[0]]));
				let g2 = (xs2 * xs2 + ys2 * ys2 + zs2 * zs2).sqrt();

				// Adopt the correct calculation result and normalize.
				let (xs, ys, zs, g) = if g1 > g2 {
					(xs1, ys1, zs1, g1)
				} else {
					(xs2, ys2, zs2, g2)
				};
				material.vx = xs / g;
				material.vy = ys / g;
				material.vz = zs / g;

				// Brightness of the face.
				let rx = material.vx + lx;
				let ry = material.vy + ly;
				let rz = material.vz + lz;
				let a = (rx * rx + ry * ry + rz * rz).sqrt() / 3.0;
				material.shadow = a / 2.0 + if darkflag { 0.3 } else { 0.5 };
			}

			self.data[i] = block;
		}
	}

	/// `GetTexture`: the file name of a texture set in the block data.
	pub fn get_texture(&self, id: i32) -> Option<&[u8; MAX_PATH]> {
		if id < 0 || TOTAL_BLOCKTEXTURE as i32 <= id {
			return None;
		}
		Some(&self.texture[id as usize])
	}

	/// `Getdata`: a copy of one block of data.
	pub fn getdata(&self, id: i32) -> Option<Blockdata> {
		if id < 0 || self.datas <= id {
			return None;
		}
		Some(self.data[id as usize])
	}
}

impl DataInterface for BlockDataInterface {
	fn get_total_datas(&self) -> i32 {
		self.datas
	}
}

impl Default for BlockDataInterface {
	fn default() -> Self {
		Self::new()
	}
}

/// `blockdataface`: the vertex data association of a block face.
///
/// ```text
///	    3 *----*2
///	     /|    /|
///	    / |   / |
///	0 *----*1 |
///	  |7 *-| --*6
///	  |  /  |  /
///	  | /   | /
///	4 *----*5
///
///	C-----D
///	|     |
///	|     |
///	B-----A
///
///	Top[0](ABCD) =  (1032)
///	Bottom[1]     =  (6745)
///	Front[2]      =  (5401)
///	Right[3]      =  (6512)
///	Back[4]       =  (7623)
///	Left[5]       =  (4730)
/// ```
///
/// Either output may be left out, which is what the upstream NULL arguments do.
pub fn blockdataface(
	face_id: i32,
	v_id: Option<&mut [i32; 4]>,
	uv_id: Option<&mut [i32; 4]>,
) -> bool {
	let (v, uv) = match face_id {
		0 => ([1, 0, 3, 2], [2, 3, 0, 1]),
		1 => ([6, 7, 4, 5], [2, 3, 0, 1]),
		2 => ([5, 4, 0, 1], [2, 3, 0, 1]),
		3 => ([6, 5, 1, 2], [2, 3, 0, 1]),
		4 => ([7, 6, 2, 3], [2, 3, 0, 1]),
		5 => ([4, 7, 3, 0], [2, 3, 0, 1]),
		_ => return false,
	};
	if let Some(v_id) = v_id {
		*v_id = v;
	}
	if let Some(uv_id) = uv_id {
		*uv_id = uv;
	}
	true
}

// ---------------------------------------------------------------------------
// Point data
// ---------------------------------------------------------------------------

/// `PointDataInterface`: manages the point data file used as mission data.
///
/// Besides loading the file it provides the point search the AI and the mission use.
pub struct PointDataInterface {
	datas: i32,
	data: [Pointdata; MAX_POINTS],
	/// Event message.
	text: [[u8; MAX_POINTMESSAGEBYTE]; MAX_POINTMESSAGES],
}

impl PointDataInterface {
	/// Constructor.
	pub fn new() -> Self {
		PointDataInterface {
			datas: 0,
			data: [Pointdata::default(); MAX_POINTS],
			text: [[0; MAX_POINTMESSAGEBYTE]; MAX_POINTMESSAGES],
		}
	}

	/// `GetTotaldatas`.
	pub fn get_total_datas(&self) -> i32 {
		self.datas
	}

	/// `LoadFiledata`. Returns 0 on success, 1 on failure.
	pub fn load_file_data(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			report_missing(fname);
			return 1;
		};

		let mut pos = 0usize;
		let header = take(&file, &mut pos, 2);
		self.datas = get_2byte_little_endian(header) as i32;
		if self.datas > MAX_POINTS as i32 {
			self.datas = MAX_POINTS as i32;
		}

		for i in 0..self.datas.max(0) as usize {
			let mainf = take(&file, &mut pos, 4 * 4);
			let mainc = take(&file, &mut pos, 4);
			let mut point = Pointdata {
				id: i as i32,
				..Default::default()
			};
			for (j, value) in [&mut point.x, &mut point.y, &mut point.z, &mut point.r]
				.iter_mut()
				.enumerate()
			{
				**value = f32_le(mainf.get(j * 4..j * 4 + 4).unwrap_or(&[]));
			}
			point.p1 = *mainc.first().unwrap_or(&0) as i8 as i16;
			point.p2 = *mainc.get(1).unwrap_or(&0) as i8 as i16;
			point.p3 = *mainc.get(2).unwrap_or(&0) as i8 as i16;
			point.p4 = *mainc.get(3).unwrap_or(&0) as i8 as i16;
			self.data[i] = point;
		}

		// Initialize the event messages, then load "<same file name>.msg".
		self.text = [[0; MAX_POINTMESSAGEBYTE]; MAX_POINTMESSAGES];
		let mut fname2 = cstr(fname).to_vec();
		if let Some(i) = fname2.iter().rposition(|&b| b == b'.') {
			if i > 0 {
				fname2.truncate(i);
			}
		}
		fname2.extend_from_slice(b".msg");
		self.load_msg_filedata(&fname2);

		0
	}

	/// `LoadMSGFiledata`. Returns 0 on success, 1 on failure.
	fn load_msg_filedata(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			return 1;
		};

		let mut pos = 0usize;
		for i in 0..MAX_POINTMESSAGES {
			let Some(line) = fgets(&file, &mut pos, MAX_POINTMESSAGEBYTE) else {
				break;
			};
			let mut message = [0u8; MAX_POINTMESSAGEBYTE];
			message[..line.len()].copy_from_slice(line);

			// Replace '\r' with '\n' if present, so a CRLF file ends a line with two
			// linefeeds, as upstream.
			for byte in message.iter_mut() {
				if *byte == b'\r' {
					*byte = b'\n';
				}
			}
			self.text[i] = message;
		}

		0
	}

	/// `Getdata`: a copy of one point of data.
	pub fn getdata(&self, id: i32) -> Option<Pointdata> {
		if id < 0 || self.datas <= id {
			return None;
		}
		Some(self.data[id as usize])
	}

	/// `SetParam`: rewrites the parameters of a point. Returns 0 on success, 2 on failure.
	///
	/// Use for anything other than the path used by the AI (p1:3 or p1:8) is not
	/// guaranteed and is not recommended.
	pub fn set_param(&mut self, id: i32, p1: i16, p2: i16, p3: i16, p4: i16) -> i32 {
		if id < 0 || self.datas <= id {
			return 2;
		}
		let point = &mut self.data[id as usize];
		point.p1 = p1;
		point.p2 = p2;
		point.p3 = p3;
		point.p4 = p4;
		0
	}

	/// `GetMessageText`: one line of the .msg file, without its linefeeds removed.
	pub fn get_message_text(&self, id: i32) -> Option<&[u8; MAX_POINTMESSAGEBYTE]> {
		if id < 0 || MAX_POINTMESSAGES as i32 <= id {
			return None;
		}
		Some(&self.text[id as usize])
	}

	/// `SearchPointdata`.
	///
	/// The parameter mask specifies which of p1 to p4 are search targets: bits 1 to 4
	/// stand for p1 to p4, a set bit searches it and a clear bit ignores it, and bits 7
	/// to 5 are ignored.
	///
	/// Returns the number of matching points and the first match, which carries its own
	/// data number in [`Pointdata::id`]. `offset` is the data item the search starts at.
	pub fn search_pointdata(
		&self,
		pmask: u8,
		p1: i16,
		p2: i16,
		p3: i16,
		p4: i16,
		offset: i32,
	) -> (i32, Option<Pointdata>) {
		let mut offset = offset;
		if offset < 0 {
			offset = 0;
		}
		if offset >= self.datas {
			offset = self.datas;
		}

		let mut count = 0;
		let mut first = None;
		for i in offset.max(0) as usize..self.datas.max(0) as usize {
			let data = &self.data[i];
			let matches = ((pmask & 0x01) == 0 || p1 == data.p1)
				&& ((pmask & 0x02) == 0 || p2 == data.p2)
				&& ((pmask & 0x04) == 0 || p3 == data.p3)
				&& ((pmask & 0x08) == 0 || p4 == data.p4);
			if matches {
				if count == 0 {
					first = Some(*data);
				}
				count += 1;
			}
		}
		(count, first)
	}
}

impl DataInterface for PointDataInterface {
	fn get_total_datas(&self) -> i32 {
		self.datas
	}
}

impl Default for PointDataInterface {
	fn default() -> Self {
		Self::new()
	}
}

// ---------------------------------------------------------------------------
// MIF
// ---------------------------------------------------------------------------

/// `MIFInterface`: manages MIF (MISSION INFORMATION FILE).
///
/// The briefing file (.txt) of standard missions is also supported.
pub struct MIFInterface {
	datas: i32,
	/// Whether the file format is .mif.
	mif: bool,
	/// Mission identifier name.
	mission_name: [u8; MISSION_NAME_BYTE],
	/// Mission full name.
	mission_fullname: [u8; MISSION_FULLNAME_BYTE],
	/// Block data file.
	blockfile_path: [u8; MAX_PATH],
	/// Point data file.
	pointfile_path: [u8; MAX_PATH],
	/// Background sky ID (none: 0).
	skynumber: i32,
	/// Briefing image A.
	picturefile_a_path: [u8; MAX_PATH],
	/// Briefing image B (additional).
	picturefile_b_path: [u8; MAX_PATH],
	/// Path to the additional small object information file.
	addsmallobject_path: [u8; MAX_PATH],
	/// Briefing text and body.
	briefingtext: [u8; BRIEFINGTEXT_BYTE],
	/// Perform more collision checks.
	collision: bool,
	/// Make the screen darker.
	screen: bool,
	/// Information about the additional small object.
	add_small_object_data: [Addsmallobject; MAX_ADDSMALLOBJECT],
}

impl MIFInterface {
	/// Constructor.
	pub fn new() -> Self {
		let mut mif = MIFInterface {
			datas: 0,
			mif: false,
			mission_name: [0; MISSION_NAME_BYTE],
			mission_fullname: [0; MISSION_FULLNAME_BYTE],
			blockfile_path: [0; MAX_PATH],
			pointfile_path: [0; MAX_PATH],
			skynumber: 0,
			picturefile_a_path: [0; MAX_PATH],
			picturefile_b_path: [0; MAX_PATH],
			addsmallobject_path: [0; MAX_PATH],
			briefingtext: [0; BRIEFINGTEXT_BYTE],
			collision: false,
			screen: false,
			add_small_object_data: [Addsmallobject::default(); MAX_ADDSMALLOBJECT],
		};
		write_cstr(&mut mif.picturefile_a_path, b"!");
		write_cstr(&mut mif.picturefile_b_path, b"!");
		mif
	}

	/// `GetTotaldatas`: the number of briefing text lines that were read.
	pub fn get_total_datas(&self) -> i32 {
		self.datas
	}

	/// `LoadFiledata`. Returns 0 on success, 1 on failure.
	///
	/// `addfile` also loads the additional file.
	pub fn load_file_data(&mut self, fname: &[u8], addfile: bool) -> i32 {
		self.mif = !check_file_extension(fname, b".txt");

		let result = if self.mif {
			self.load_mission_info_filedata(fname)
		} else {
			self.load_default_text_filedata(fname)
		};
		if result != 0 {
			return 1;
		}

		if ENABLE_AUTOLOADMIF && check_full_path(fname) {
			let mut path = [0u8; MAX_PATH];
			get_file_directory(fname, &mut path);

			// Rewrite each file path as a full path.
			change_exe_path_to_full_path(&path, &mut self.blockfile_path);
			change_exe_path_to_full_path(&path, &mut self.pointfile_path);
			change_exe_path_to_full_path(&path, &mut self.picturefile_a_path);
			change_exe_path_to_full_path(&path, &mut self.picturefile_b_path);
			change_exe_path_to_full_path(&path, &mut self.addsmallobject_path);
		}

		// Reset the additional small object information to the initial values.
		self.add_small_object_data = [Addsmallobject::default(); MAX_ADDSMALLOBJECT];

		// If some kind of additional small object information file is specified.
		let addsmallobject_path = self.addsmallobject_path;
		let path = cstr(&addsmallobject_path);
		if addfile && !path.is_empty() && path != b"!" {
			self.load_add_small_object_filedata(path);
		}

		if ENABLE_AUTOLOADMIF && check_full_path(fname) {
			let mut path = [0u8; MAX_PATH];
			get_file_directory(fname, &mut path);

			for i in 0..MAX_ADDSMALLOBJECT {
				change_exe_path_to_full_path(&path, &mut self.add_small_object_data[i].modelpath);
				change_exe_path_to_full_path(&path, &mut self.add_small_object_data[i].texturepath);
				change_exe_path_to_full_path(&path, &mut self.add_small_object_data[i].soundpath);
			}
		}

		0
	}

	/// `LoadDefaultTextFiledata`: the briefing file (.txt) of a standard mission.
	fn load_default_text_filedata(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			return 1;
		};
		let mut pos = 0usize;
		let mut line = [0u8; 64];

		// Image A, then B, each built as "data\briefing\<name>.bmp".
		for i in 0..2 {
			read_line(&mut line, &file, &mut pos, 64);
			let path = if cstr(&line) == b"!" {
				b"!".to_vec()
			} else {
				let mut path = b"data\\briefing\\".to_vec();
				path.extend_from_slice(cstr(&line));
				path.extend_from_slice(b".bmp");
				path
			};
			if i == 0 {
				write_cstr(&mut self.picturefile_a_path, &path);
			} else {
				write_cstr(&mut self.picturefile_b_path, &path);
			}
		}

		// Background sky number.
		read_line(&mut line, &file, &mut pos, 16);
		self.skynumber = atoi(&line);

		self.read_briefing_text(&file, &mut pos);

		// Initialize the values that could not be read.
		self.mission_name = [0; MISSION_NAME_BYTE];
		self.mission_fullname = [0; MISSION_FULLNAME_BYTE];
		self.blockfile_path = [0; MAX_PATH];
		self.pointfile_path = [0; MAX_PATH];
		self.addsmallobject_path = [0; MAX_PATH];
		self.collision = false;
		self.screen = false;

		0
	}

	/// `LoadMissionInfoFiledata`: a mission file (.mif).
	fn load_mission_info_filedata(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			return 1;
		};
		let mut pos = 0usize;
		let mut line = [0u8; 64];

		// Mission identifier name, then the full name.
		read_line(&mut self.mission_name, &file, &mut pos, MISSION_NAME_BYTE);
		read_line(
			&mut self.mission_fullname,
			&file,
			&mut pos,
			MISSION_FULLNAME_BYTE,
		);

		// Block data file, then point data file.
		read_line(&mut self.blockfile_path, &file, &mut pos, MAX_PATH);
		read_line(&mut self.pointfile_path, &file, &mut pos, MAX_PATH);

		// Background sky number.
		read_line(&mut line, &file, &mut pos, 16);
		self.skynumber = atoi(&line);

		// Collision and screen settings.
		read_line(&mut line, &file, &mut pos, 16);
		match cstr(&line) {
			b"1" => {
				self.collision = true;
				self.screen = false;
			}
			b"2" => {
				self.collision = false;
				self.screen = true;
			}
			b"3" => {
				self.collision = true;
				self.screen = true;
			}
			_ => {
				self.collision = false;
				self.screen = false;
			}
		}

		// Additional small object information file, image A, image B.
		read_line(&mut self.addsmallobject_path, &file, &mut pos, MAX_PATH);
		read_line(&mut self.picturefile_a_path, &file, &mut pos, MAX_PATH);
		read_line(&mut self.picturefile_b_path, &file, &mut pos, MAX_PATH);

		self.read_briefing_text(&file, &mut pos);

		0
	}

	/// The briefing text is up to 17 lines concatenated with their linefeeds kept, and it
	/// is also what `GetTotaldatas` counts.
	fn read_briefing_text(&mut self, file: &[u8], pos: &mut usize) {
		self.briefingtext = [0; BRIEFINGTEXT_BYTE];
		let mut len = 0usize;
		for _ in 0..BRIEFING_LINES {
			let Some(line) = fgets(file, pos, 50) else {
				break;
			};
			// The upstream `strcat`s into 816 bytes without a bound, so a full 17 lines
			// of 49 characters would run past it. The port stops at the last byte.
			let n = line.len().min(BRIEFINGTEXT_BYTE - 1 - len);
			self.briefingtext[len..len + n].copy_from_slice(&line[..n]);
			len += n;
			self.datas += 1;
		}
	}

	/// `LoadAddSmallObjectFiledata`: the additional small object information file.
	fn load_add_small_object_filedata(&mut self, fname: &[u8]) -> i32 {
		let Some(file) = read_file(fname) else {
			return 1;
		};
		let mut pos = 0usize;

		for i in 0..MAX_ADDSMALLOBJECT {
			// Read 6 lines, failing if all 6 cannot be read.
			let mut str1 = [0u8; MAX_PATH];
			let mut str2 = [0u8; MAX_PATH];
			let mut str3 = [0u8; 32];
			let mut str4 = [0u8; 32];
			let mut str5 = [0u8; MAX_PATH];
			let mut str6 = [0u8; 32];
			if !read_line(&mut str1, &file, &mut pos, MAX_PATH) {
				break;
			}
			if !read_line(&mut str2, &file, &mut pos, MAX_PATH) {
				break;
			}
			if !read_line(&mut str3, &file, &mut pos, 16) {
				break;
			}
			if !read_line(&mut str4, &file, &mut pos, 16) {
				break;
			}
			if !read_line(&mut str5, &file, &mut pos, MAX_PATH) {
				break;
			}
			if !read_line(&mut str6, &file, &mut pos, 16) {
				break;
			}

			// Model data path, texture path.
			write_cstr(&mut self.add_small_object_data[i].modelpath, cstr(&str1));
			write_cstr(&mut self.add_small_object_data[i].texturepath, cstr(&str2));

			// Size of the collision check, durability, jump.
			let mut decide = atoi(&str3);
			let mut hp = atoi(&str4);
			let mut jump = atoi(&str6);
			if ENABLE_ADDOBJ_PARAM8BIT {
				decide &= 0x0000007F;
				hp &= 0x0000007F;
				jump &= 0x000000FF;
			}
			self.add_small_object_data[i].decide = decide;
			self.add_small_object_data[i].hp = hp;
			self.add_small_object_data[i].jump = jump;

			// Sound data path.
			write_cstr(&mut self.add_small_object_data[i].soundpath, cstr(&str5));

			// Read the single dummy line that precedes the next additional small object
			// information.
			if i + 1 < MAX_ADDSMALLOBJECT {
				let mut dummy = [0u8; 32];
				if !read_line(&mut dummy, &file, &mut pos, 32) {
					break;
				}
			}
		}

		0
	}

	/// `GetFiletype`: standard format returns false, MIF format returns true.
	pub fn get_filetype(&self) -> bool {
		self.mif
	}

	/// `GetMissionName`.
	pub fn get_mission_name(&self) -> &[u8; MISSION_NAME_BYTE] {
		&self.mission_name
	}

	/// `GetMissionFullname`.
	pub fn get_mission_fullname(&self) -> &[u8; MISSION_FULLNAME_BYTE] {
		&self.mission_fullname
	}

	/// `GetDatafilePath`: the paths of the block data and the point data.
	pub fn get_datafile_path(&self) -> (&[u8; MAX_PATH], &[u8; MAX_PATH]) {
		(&self.blockfile_path, &self.pointfile_path)
	}

	/// `GetSkynumber`: the background sky, 0 to 5, where 0 means "no background".
	pub fn get_skynumber(&self) -> i32 {
		self.skynumber
	}

	/// `GetPicturefilePath`: the paths of the briefing images.
	///
	/// "!" is returned if one is not set, and image B returns "!" when only one image is
	/// used.
	pub fn get_picturefile_path(&self) -> (&[u8; MAX_PATH], &[u8; MAX_PATH]) {
		(&self.picturefile_a_path, &self.picturefile_b_path)
	}

	/// `GetBriefingText`: up to 17 lines, including the line break characters.
	pub fn get_briefing_text(&self) -> &[u8; BRIEFINGTEXT_BYTE] {
		&self.briefingtext
	}

	/// `GetCollisionFlag`: the flag indicating the additional collision checks.
	pub fn get_collision_flag(&self) -> bool {
		self.collision
	}

	/// `GetScreenFlag`: the flag indicating that the screen is darkened.
	pub fn get_screen_flag(&self) -> bool {
		self.screen
	}

	/// `GetAddSmallobjectFile`: the additional small object information file path.
	pub fn get_add_smallobject_file(&self) -> &[u8; MAX_PATH] {
		&self.addsmallobject_path
	}

	/// `GetAddSmallobjectModelPath`.
	pub fn get_add_smallobject_model_path(&self, id: i32) -> Option<&[u8; MAX_PATH]> {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return None;
		}
		Some(&self.add_small_object_data[id as usize].modelpath)
	}

	/// `GetAddSmallobjectTexturePath`.
	pub fn get_add_smallobject_texture_path(&self, id: i32) -> Option<&[u8; MAX_PATH]> {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return None;
		}
		Some(&self.add_small_object_data[id as usize].texturepath)
	}

	/// `GetAddSmallobjectDecide`: the collision check size.
	pub fn get_add_smallobject_decide(&self, id: i32) -> i32 {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return 0;
		}
		self.add_small_object_data[id as usize].decide
	}

	/// `GetAddSmallobjectHP`: the durability.
	pub fn get_add_smallobject_hp(&self, id: i32) -> i32 {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return 0;
		}
		self.add_small_object_data[id as usize].hp
	}

	/// `GetAddSmallobjectSoundPath`.
	pub fn get_add_smallobject_sound_path(&self, id: i32) -> Option<&[u8; MAX_PATH]> {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return None;
		}
		Some(&self.add_small_object_data[id as usize].soundpath)
	}

	/// `GetAddSmallobjectJump`: the amount of jump.
	pub fn get_add_smallobject_jump(&self, id: i32) -> i32 {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return 0;
		}
		self.add_small_object_data[id as usize].jump
	}
}

impl DataInterface for MIFInterface {
	fn get_total_datas(&self) -> i32 {
		self.datas
	}
}

impl Default for MIFInterface {
	fn default() -> Self {
		Self::new()
	}
}

/// `ChangeExePathToFullPath`: rewrites a relative path that refers inside the addon
/// folder from being relative to the .exe to being relative to the .mif.
///
/// Returns true when it converted. A free function rather than a method because it only
/// ever touches its arguments.
fn change_exe_path_to_full_path(dir: &[u8], fname: &mut [u8; MAX_PATH]) -> bool {
	let name = cstr(fname);
	if name.is_empty() || name == b"!" {
		return false;
	}

	// Check whether the path refers inside the addon folder.
	let mut index = 0usize;
	for prefix in [
		ADDONDIR.to_vec(),
		b"\\".iter().chain(ADDONDIR).copied().collect(),
		b"/".iter().chain(ADDONDIR).copied().collect(),
		b".\\".iter().chain(ADDONDIR).copied().collect(),
		b"./".iter().chain(ADDONDIR).copied().collect(),
	] {
		if name.len() >= prefix.len() && name[..prefix.len()] == prefix[..] {
			index = prefix.len();
		}
	}
	if index == 0 {
		return false;
	}

	// Abort if the combined string would exceed MAX_PATH characters.
	let dir = cstr(dir);
	if dir.len() + name.len() - index + 1 >= MAX_PATH - 1 {
		return false;
	}

	// The path up to the .mif plus the file path with the "addon" characters removed.
	let rest = &name[index..];
	let mut combined = Vec::with_capacity(dir.len() + rest.len() + 1);
	combined.extend_from_slice(dir);
	combined.extend_from_slice(rest);
	write_cstr(fname, &combined);
	true
}

// ---------------------------------------------------------------------------
// ADDON list
// ---------------------------------------------------------------------------

/// `AddonList`: manages the .mif files found in a specific directory as an ADDON list.
pub struct AddonList {
	datas: i32,
	/// .mif file name.
	filename: [[u8; MAX_PATH]; MAX_ADDONLIST],
	/// Mission identifier name.
	mission_name: [[u8; MISSION_NAME_BYTE]; MAX_ADDONLIST],
}

impl AddonList {
	/// Constructor.
	pub fn new() -> Self {
		AddonList {
			datas: 0,
			filename: [[0; MAX_PATH]; MAX_ADDONLIST],
			mission_name: [[0; MISSION_NAME_BYTE]; MAX_ADDONLIST],
		}
	}

	/// `GetTotaldatas`.
	pub fn get_total_datas(&self) -> i32 {
		self.datas
	}

	/// `LoadFiledata`: reads the ADDON list. Returns the total number of addons.
	pub fn load_file_data(&mut self, dir: &[u8]) -> i32 {
		if cstr(dir).is_empty() {
			return 0;
		}
		self.datas = 0;

		self.get_mif_list(dir);
		self.get_mission_name(dir);
		self.sort();

		self.datas
	}

	/// `GetMIFlist`: the .mif files in the directory.
	///
	/// The upstream pattern "*.mif" is matched by the Windows API, which does not
	/// distinguish case, so neither does this.
	fn get_mif_list(&mut self, dir: &[u8]) {
		let Ok(entries) =
			xops_platform::fs::read_dir_names(resolve_case_insensitive(&path_from_bytes(dir)))
		else {
			return;
		};
		for entry in entries {
			let name = entry.into_bytes();
			if !check_file_extension(&name, b".mif") {
				continue;
			}
			// The upstream writes past the 128 entry table when a directory holds more;
			// the port stops at the last entry.
			if self.datas as usize >= MAX_ADDONLIST {
				break;
			}
			let slot = self.datas as usize;
			write_cstr(&mut self.filename[slot], &name);
			self.datas += 1;
		}
	}

	/// `GetMissionName`: the mission name of every .mif found.
	fn get_mission_name(&mut self, dir: &[u8]) {
		for i in 0..self.datas.max(0) as usize {
			let mut path = cstr(dir).to_vec();
			path.extend_from_slice(b"\\");
			path.extend_from_slice(cstr(&self.filename[i]));

			let mut mifdata = MIFInterface::new();
			mifdata.load_file_data(&path, false);

			let name = *mifdata.get_mission_name();
			write_cstr(&mut self.mission_name[i], cstr(&name));
		}
	}

	/// `Sort`: sorts the mission names, case insensitively.
	fn sort(&mut self) {
		if self.datas <= 1 {
			return;
		}
		let n = self.datas as usize;
		let mut mission_name_c = vec![[0u8; MISSION_NAME_BYTE]; n];
		for i in 0..n {
			for j in 0..MISSION_NAME_BYTE {
				mission_name_c[i][j] = self.mission_name[i][j].to_ascii_lowercase();
			}
		}

		// Slow bubble sort (sigh).
		for i in 0..n - 1 {
			for j in i + 1..n {
				if cstr(&mission_name_c[i]) > cstr(&mission_name_c[j]) {
					mission_name_c.swap(i, j);
					self.mission_name.swap(i, j);
					self.filename.swap(i, j);
				}
			}
		}
	}

	/// `GetMissionName`: the mission name of one addon.
	pub fn get_mission_name_of(&self, id: i32) -> Option<&[u8; MISSION_NAME_BYTE]> {
		if id < 0 || MAX_ADDONLIST as i32 <= id {
			return None;
		}
		Some(&self.mission_name[id as usize])
	}

	/// `GetFileName`: the file name of one addon.
	pub fn get_filename(&self, id: i32) -> Option<&[u8; MAX_PATH]> {
		if id < 0 || MAX_ADDONLIST as i32 <= id {
			return None;
		}
		Some(&self.filename[id as usize])
	}
}

impl DataInterface for AddonList {
	fn get_total_datas(&self) -> i32 {
		self.datas
	}
}

impl Default for AddonList {
	fn default() -> Self {
		Self::new()
	}
}

// ---------------------------------------------------------------------------
// INI files
// ---------------------------------------------------------------------------

/// `INIFileInterface`: manages INI files.
pub struct INIFileInterface {
	/// The whole file, read when it is loaded: every getter rescans it from the start,
	/// which is what the upstream `fseek(SEEK_SET)` does.
	inifp: Option<Vec<u8>>,
}

impl INIFileInterface {
	/// Constructor.
	pub fn new() -> Self {
		INIFileInterface { inifp: None }
	}

	/// `LoadINIFile`. Returns false on success, true on failure.
	///
	/// If an INI file has already been loaded this fails until [`release_ini_file`] is
	/// used.
	///
	/// [`release_ini_file`]: INIFileInterface::release_ini_file
	pub fn load_ini_file(&mut self, path: &Path) -> bool {
		if self.inifp.is_some() {
			return true;
		}
		match xops_platform::fs::read_file(resolve_case_insensitive(path)) {
			Ok(bytes) => {
				self.inifp = Some(bytes);
				false
			}
			Err(_) => true,
		}
	}

	/// Scans the file for a key, which is the whole of the upstream reader: a section
	/// header has to come first, a key has to be the first thing on its line, and a
	/// section header ends the search.
	fn read_value(&self, section: &[u8], key: &[u8]) -> Option<Vec<u8>> {
		let file = self.inifp.as_ref()?;
		let mut key_eq = key.to_vec();
		key_eq.push(b'=');

		// A section name that is not specified disables the section name check.
		let mut state = section.is_empty();
		let mut pos = 0usize;
		while let Some(line) = fgets(file, &mut pos, 512) {
			// Ignore comment lines.
			if line.first() == Some(&b';') {
				continue;
			}

			// Section name check.
			if !state {
				let mut want = Vec::with_capacity(section.len() + 2);
				want.push(b'[');
				want.extend_from_slice(section);
				want.push(b']');
				if line.starts_with(&want) {
					state = true;
				}
				continue;
			}

			// Ignore anything else that starts a section.
			if line.first() == Some(&b'[') {
				break;
			}

			let stripped = strip_ini_line(line);
			if stripped.starts_with(&key_eq) {
				return Some(stripped[key_eq.len()..].to_vec());
			}
		}
		None
	}

	/// The value or the default, plus the error code the upstream hands back through its
	/// `errorcode` pointer.
	///
	/// `strbuflen` is the size of the upstream's receiving buffer, so the value is
	/// truncated to leave room for the terminator.
	fn resolve(
		&self,
		section: &[u8],
		key: &[u8],
		default: &[u8],
		strbuflen: i32,
	) -> (Vec<u8>, i32) {
		let Some(value) = self.read_value(section, key) else {
			let error = if self.inifp.is_none() { 2 } else { 3 };
			return (default.to_vec(), error);
		};

		// The upstream writes at `buf[strbuflen - 1]`, which is out of bounds at zero, so
		// a length of zero reads as an empty value instead.
		let limit = match strbuflen {
			n if n <= -2 => 0,
			-1 => 512,
			n if n > 512 => 512,
			n => n,
		};
		let n = value.len().min(limit.max(1) as usize - 1);
		(value[..n].to_vec(), 0)
	}

	/// `GetINIFileString`.
	///
	/// An empty `section` disables the section check, which is what the upstream NULL and
	/// empty section name both do.
	///
	/// If the file is not open or the key is not found, the default value is returned.
	/// The upstream error codes (1: invalid argument, 2: file not open, 3: value not
	/// found) reach the caller through the getters below, which are the only ones that
	/// report them.
	pub fn get_ini_file_string(&mut self, section: &[u8], key: &[u8], default: &[u8]) -> Vec<u8> {
		// The upstream clamps this one to the full 512 byte line buffer.
		self.resolve(section, key, default, -1).0
	}

	/// `GetINIFileInt`.
	///
	/// Error codes: 1: invalid argument, 2: file not open, 3: value not found.
	pub fn get_ini_file_int(
		&mut self,
		section: &[u8],
		key: &[u8],
		default: i32,
		errorcode: Option<&mut i32>,
	) -> i32 {
		// The upstream formats the default, hands it to the string getter and reads the
		// result back with atoi.
		let defaultint = default.to_string();
		let (buf, error) = self.resolve(section, key, defaultint.as_bytes(), 64);
		if let Some(code) = errorcode {
			*code = error;
		}
		atoi(&buf)
	}

	/// `GetINIFileFloat`.
	///
	/// Error codes: 1: invalid argument, 2: file not open, 3: value not found.
	pub fn get_ini_file_float(
		&mut self,
		section: &[u8],
		key: &[u8],
		default: f32,
		errorcode: Option<&mut i32>,
	) -> f32 {
		// The upstream formats the default with "%f".
		let defaultfloat = format!("{default:.6}");
		let (buf, error) = self.resolve(section, key, defaultfloat.as_bytes(), 64);
		if let Some(code) = errorcode {
			*code = error;
		}

		// Drop the trailing f or F that marks a single precision literal.
		let mut end = cstr(&buf).len();
		while end > 1 && matches!(buf[end - 1], b'f' | b'F') {
			end -= 1;
		}
		atof(&buf[..end]) as f32
	}

	/// `GetINIFileBool`.
	///
	/// "false" and "0" are false in any case, and everything else is true.
	///
	/// Error codes: 1: invalid argument, 2: file not open, 3: value not found.
	pub fn get_ini_file_bool(
		&mut self,
		section: &[u8],
		key: &[u8],
		default: bool,
		errorcode: Option<&mut i32>,
	) -> bool {
		let defaultbool: &[u8] = if default { b"true" } else { b"false" };
		let (buf, error) = self.resolve(section, key, defaultbool, 64);
		if let Some(code) = errorcode {
			*code = error;
		}
		let lower: Vec<u8> = cstr(&buf).iter().map(|b| b.to_ascii_lowercase()).collect();
		!(lower == b"false" || lower == b"0")
	}

	/// `ReleaseINIFile`.
	pub fn release_ini_file(&mut self) {
		self.inifp = None;
	}
}

impl Default for INIFileInterface {
	fn default() -> Self {
		Self::new()
	}
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// `DeleteLinefeed`: removes the line break characters, for `fgets`.
///
/// Returns 0 when one was replaced and 1 when there was none.
pub fn delete_linefeed(str: &mut [u8]) -> i32 {
	let mut flag = false;
	if let Some(p) = str.iter().position(|&c| c == b'\n') {
		str[p] = 0;
		flag = true;
	}
	if let Some(p) = str.iter().position(|&c| c == b'\r') {
		str[p] = 0;
		flag = true;
	}
	if flag {
		0
	} else {
		1
	}
}

/// `CheckFullPath`: is the path a full path?
///
/// Only a Windows environment is assumed, so anything whose second byte is ':' counts.
pub fn check_full_path(path: &[u8]) -> bool {
	let path = cstr(path);
	if path.len() < 2 {
		return false;
	}
	path[1] == b':'
}

/// `GetFileDirectory`: removes the file name from a file path, keeping the separator.
pub fn get_file_directory(path: &[u8], dir: &mut [u8]) {
	if dir.is_empty() {
		return;
	}
	write_cstr(dir, cstr(path));

	let len = cstr(dir).len();
	if len <= 1 {
		return;
	}
	for i in (1..len).rev() {
		if dir[i] == b'\\' || dir[i] == b'/' {
			dir[i + 1] = 0;
			return;
		}
	}
}

/// `CheckFileExtension`: does the file name end in the extension?
///
/// Case is not distinguished, which the game data relies on: it ships with inconsistent
/// case.
pub fn check_file_extension(filepath: &[u8], checkstr: &[u8]) -> bool {
	let filepath = cstr(filepath);
	let checkstr = cstr(checkstr);
	if filepath.len() >= MAX_PATH || checkstr.len() >= 16 || filepath.len() < checkstr.len() {
		return false;
	}
	for i in (1..filepath.len()).rev() {
		if filepath[i] == b'.' {
			return filepath[i..].eq_ignore_ascii_case(checkstr);
		}
	}
	false
}

/// `CheckDirectory`: does the directory exist?
pub fn check_directory(dir: &[u8]) -> bool {
	xops_platform::fs::exists(resolve_case_insensitive(&path_from_bytes(dir)))
}

#[cfg(test)]
mod tests {
	use super::*;

	// --- helpers ---------------------------------------------------------

	/// A path in the temp directory that this test can write to.
	fn temp_path(name: &str) -> PathBuf {
		std::env::temp_dir().join(format!("xops_datafile_{}_{}", std::process::id(), name))
	}

	fn temp_write(name: &str, bytes: &[u8]) -> Vec<u8> {
		let path = temp_path(name);
		xops_platform::fs::write_file(&path, bytes).expect("write temp file");
		path.to_string_lossy().into_owned().into_bytes()
	}

	fn write_f32_le(out: &mut Vec<u8>, value: f32) {
		out.extend_from_slice(&value.to_bits().to_le_bytes());
	}

	/// A .bd1 image with `count` blocks, each a unit cube: vertex j sits at the corner
	/// the face table implies, and face j gets texture j and UVs that name the face.
	fn bd1_image(textures: &[&[u8]], count: usize) -> Vec<u8> {
		let mut out = Vec::new();
		for i in 0..TOTAL_BLOCKTEXTURE {
			let name = textures.get(i).copied().unwrap_or(b"");
			let mut field = [0u8; 31];
			let n = name.len().min(31);
			field[..n].copy_from_slice(&name[..n]);
			out.extend_from_slice(&field);
		}
		out.extend_from_slice(&(count as u16).to_le_bytes());

		const CUBE: [(f32, f32, f32); 8] = [
			(0.0, 0.0, 0.0),
			(1.0, 0.0, 0.0),
			(1.0, 1.0, 0.0),
			(0.0, 1.0, 0.0),
			(0.0, 0.0, 1.0),
			(1.0, 0.0, 1.0),
			(1.0, 1.0, 1.0),
			(0.0, 1.0, 1.0),
		];
		for _ in 0..count {
			for axis in 0..3 {
				for corner in CUBE {
					let value = match axis {
						0 => corner.0,
						1 => corner.1,
						_ => corner.2,
					};
					write_f32_le(&mut out, value);
				}
			}
			// The UV block: u[face j][k] sits at float 24 + j*4 + k and v[face j][k] at
			// 48 + j*4 + k, so each value names its own slot.
			for i in 24..72 {
				write_f32_le(&mut out, i as f32);
			}
			// The 28 byte header of the six faces: a texture ID and three unused bytes.
			for j in 0..6 {
				out.push(j as u8);
				out.push(0);
				out.push(0);
				out.push(0);
			}
			out.extend_from_slice(&[0; 4]);
		}
		out
	}

	/// A .pd1 image: `points` as (x, y, z, r, p1, p2, p3, p4).
	fn pd1_image(points: &[(f32, f32, f32, f32, i8, i8, i8, i8)]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend_from_slice(&(points.len() as u16).to_le_bytes());
		for &(x, y, z, r, p1, p2, p3, p4) in points {
			write_f32_le(&mut out, x);
			write_f32_le(&mut out, y);
			write_f32_le(&mut out, z);
			write_f32_le(&mut out, r);
			out.extend_from_slice(&[p1 as u8, p2 as u8, p3 as u8, p4 as u8]);
		}
		out
	}

	// --- byte helpers ----------------------------------------------------

	#[test]
	fn delete_linefeed_takes_lf_and_crlf() {
		let mut lf = *b"abc\n";
		assert_eq!(delete_linefeed(&mut lf), 0);
		assert_eq!(cstr(&lf), b"abc");

		let mut crlf = *b"abc\r\n";
		assert_eq!(delete_linefeed(&mut crlf), 0);
		assert_eq!(cstr(&crlf), b"abc");

		let mut bare = *b"abc";
		assert_eq!(delete_linefeed(&mut bare), 1);
		assert_eq!(cstr(&bare), b"abc");
	}

	#[test]
	fn atoi_and_atof_follow_the_c_rules() {
		assert_eq!(atoi(b"42"), 42);
		assert_eq!(atoi(b"  -7x"), -7);
		assert_eq!(atoi(b"abc"), 0);
		assert_eq!(atoi(b"+5"), 5);

		assert_eq!(atof(b"0.5"), 0.5);
		assert_eq!(atof(b" 1.25e2"), 125.0);
		assert_eq!(atof(b"3."), 3.0);
		assert_eq!(atof(b".5"), 0.5);
		assert_eq!(atof(b"junk"), 0.0);
		// An exponent with no digits does not count, so the number ends before it.
		assert_eq!(atof(b"12e"), 12.0);
	}

	#[test]
	fn file_path_helpers() {
		assert!(check_full_path(b"C:\\games\\x.mif"));
		assert!(!check_full_path(b"addon\\x.mif"));
		assert!(!check_full_path(b"c"));

		let mut dir = [0u8; MAX_PATH];
		get_file_directory(b"data\\map10\\temp.bd1", &mut dir);
		assert_eq!(cstr(&dir), b"data\\map10\\");
		get_file_directory(b"addon/foo.mif", &mut dir);
		assert_eq!(cstr(&dir), b"addon/");
		get_file_directory(b"nofile", &mut dir);
		assert_eq!(cstr(&dir), b"nofile");

		// Case is not distinguished, and the last extension is the one that counts.
		assert!(check_file_extension(b"MAP.BD1", b".bd1"));
		assert!(check_file_extension(b"addon\\Foo.MIF", b".mif"));
		assert!(!check_file_extension(b"map.bd1x", b".bd1"));
		assert!(!check_file_extension(b"data\\map10\\temp.bd1", b".mif"));
		// The first byte is never the dot, so a bare extension is not a match.
		assert!(!check_file_extension(b".bd1", b".bd1"));

		assert!(check_directory(
			std::env::temp_dir().to_string_lossy().as_bytes()
		));
		assert!(!check_directory(b"no_such_directory_xyz"));
	}

	#[test]
	fn byte_paths_take_platform_separators() {
		assert_eq!(
			path_from_bytes(b"data\\map10\\temp.bd1"),
			PathBuf::from("data/map10/temp.bd1")
		);
		assert_eq!(
			path_from_bytes(b"data/map10/temp.bd1"),
			PathBuf::from("data/map10/temp.bd1")
		);
		// Everything after the NUL is not part of the path, so a fixed buffer works.
		let mut buffer = [0u8; MAX_PATH];
		write_cstr(&mut buffer, b"data\\x.bd1");
		assert_eq!(path_from_bytes(&buffer), PathBuf::from("data/x.bd1"));
	}

	#[test]
	fn addon_paths_are_rewritten_against_the_mif_directory() {
		let dir = b"C:\\games\\addon\\mymission\\";
		let mut fname = [0u8; MAX_PATH];

		// The "addon" prefix is removed and the path is put under the .mif directory,
		// separator and all.
		write_cstr(&mut fname, b"addon\\map\\temp.bd1");
		assert!(change_exe_path_to_full_path(dir, &mut fname));
		assert_eq!(
			cstr(&fname),
			b"C:\\games\\addon\\mymission\\\\map\\temp.bd1"
		);

		write_cstr(&mut fname, b".\\addon\\op.pd1");
		assert!(change_exe_path_to_full_path(dir, &mut fname));
		assert_eq!(cstr(&fname), b"C:\\games\\addon\\mymission\\\\op.pd1");

		// Anything else is left alone.
		write_cstr(&mut fname, b"data\\map10\\temp.bd1");
		assert!(!change_exe_path_to_full_path(dir, &mut fname));
		assert_eq!(cstr(&fname), b"data\\map10\\temp.bd1");

		write_cstr(&mut fname, b"!");
		assert!(!change_exe_path_to_full_path(dir, &mut fname));
		assert_eq!(cstr(&fname), b"!");
		assert!(!change_exe_path_to_full_path(dir, &mut [0u8; MAX_PATH]));
	}

	#[test]
	fn blockdataface_names_every_face() {
		let mut v = [0i32; 4];
		let mut uv = [0i32; 4];
		assert!(blockdataface(0, Some(&mut v), Some(&mut uv)));
		assert_eq!(v, [1, 0, 3, 2]);
		assert_eq!(uv, [2, 3, 0, 1]);
		assert!(blockdataface(5, Some(&mut v), None));
		assert_eq!(v, [4, 7, 3, 0]);
		assert!(blockdataface(3, None, Some(&mut uv)));
		assert_eq!(uv, [2, 3, 0, 1]);
		assert!(!blockdataface(6, Some(&mut v), Some(&mut uv)));
	}

	// --- block data ------------------------------------------------------

	#[test]
	fn bd1_loads_textures_vertices_and_faces() {
		let image = bd1_image(&[b"block01.bmp", b"block02.bmp"], 2);
		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_bd1(&image), 0);

		assert_eq!(blocks.get_total_datas(), 2);
		assert_eq!(cstr(blocks.get_texture(0).unwrap()), b"block01.bmp");
		assert_eq!(cstr(blocks.get_texture(1).unwrap()), b"block02.bmp");
		assert_eq!(cstr(blocks.get_texture(2).unwrap()), b"");
		assert!(blocks.get_texture(TOTAL_BLOCKTEXTURE as i32).is_none());
		assert!(blocks.get_texture(-1).is_none());

		let block = blocks.getdata(1).expect("block 1");
		assert_eq!(block.id, 1);
		assert_eq!(block.x[6], 1.0);
		assert_eq!(block.y[6], 1.0);
		assert_eq!(block.z[6], 1.0);
		assert!(blocks.getdata(2).is_none());

		assert_eq!(block.material[3].texture_id, 3);
		assert_eq!(block.material[3].u, [36.0, 37.0, 38.0, 39.0]);
		assert_eq!(block.material[3].v, [60.0, 61.0, 62.0, 63.0]);
		assert_eq!(block.material[0].u, [24.0, 25.0, 26.0, 27.0]);
		assert_eq!(block.material[0].v, [48.0, 49.0, 50.0, 51.0]);
	}

	#[test]
	fn bd1_datas_is_capped_at_max_blocks() {
		let mut header = Vec::new();
		header.extend_from_slice(&[0u8; 31 * TOTAL_BLOCKTEXTURE]);
		header.extend_from_slice(&1000u16.to_le_bytes());
		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_bd1(&header), 0);
		assert_eq!(blocks.get_total_datas(), MAX_BLOCKS as i32);
	}

	#[test]
	fn calculation_blockdata_shades_every_face() {
		let image = bd1_image(&[], 1);
		let mut blocks = BlockDataInterface::new();
		blocks.load_bd1(&image);
		blocks.calculation_blockdata(false);

		let block = blocks.getdata(0).unwrap();
		let mut shadows = Vec::new();
		for material in block.material.iter() {
			// Every face of a cube gets a unit normal and a center inside the cube.
			let length =
				(material.vx * material.vx + material.vy * material.vy + material.vz * material.vz)
					.sqrt();
			assert!((length - 1.0).abs() < 1e-5, "normal length {length}");
			for center in [material.center_x, material.center_y, material.center_z] {
				assert!((0.0..=1.0).contains(&center));
			}
			// |light| is sqrt(1.75), so the shadow of a unit normal stays in this band.
			assert!(
				(0.55..=0.89).contains(&material.shadow),
				"shadow {}",
				material.shadow
			);
			shadows.push(material.shadow);
		}

		// Face 0 is the y = 0 face of the cube, so its normal points along z.
		assert_eq!(block.material[0].vx, 0.0);
		assert_eq!(block.material[0].vy, 0.0);
		assert!(block.material[0].vz.abs() == 1.0);
		// Face centers are the average of the 4 vertices of the face.
		assert_eq!(block.material[0].center_x, 0.5);
		assert_eq!(block.material[0].center_y, 0.5);
		assert_eq!(block.material[0].center_z, 0.0);
		assert_eq!(block.material[5].center_z, 0.5);

		// The faces are not all the same, which is the point of the shading.
		assert!(
			shadows.iter().any(|s| (s - shadows[0]).abs() > 0.01),
			"all faces share a shadow: {shadows:?}"
		);

		// The dark flag only shifts the brightness.
		blocks.calculation_blockdata(true);
		let dark = blocks.getdata(0).unwrap();
		for j in 0..6 {
			assert!((block.material[j].shadow - dark.material[j].shadow - 0.2).abs() < 1e-6);
			assert_eq!(block.material[j].vx, dark.material[j].vx);
		}
	}

	#[test]
	fn bd1_is_loaded_from_a_file() {
		let path = temp_write("map.bd1", &bd1_image(&[b"floor.bmp"], 1));
		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(&path), 0);
		assert_eq!(blocks.get_total_datas(), 1);
		assert_eq!(cstr(blocks.get_texture(0).unwrap()), b"floor.bmp");

		assert_eq!(blocks.load_file_data(b"no_such_file.bd1"), 1);
	}

	// --- point data ------------------------------------------------------

	#[test]
	fn pd1_loads_points_and_sign_extends_parameters() {
		let points = [
			(1.0f32, 2.0, 3.0, 90.0, 3i8, 1, 0, 5),
			(4.0, 5.0, 6.0, 0.0, -1, 2, 3, 7),
			(7.0, 8.0, 9.0, 180.0, 3, 9, 9, 7),
		];
		let image = pd1_image(&points);
		let path = temp_write("op.pd1", &image);
		let mut data = PointDataInterface::new();
		assert_eq!(data.load_file_data(&path), 0);
		assert_eq!(data.get_total_datas(), 3);

		let first = data.getdata(0).unwrap();
		assert_eq!((first.x, first.y, first.z, first.r), (1.0, 2.0, 3.0, 90.0));
		assert_eq!((first.p1, first.p2, first.p3, first.p4), (3, 1, 0, 5));
		assert_eq!(first.id, 0);

		// A parameter byte over 127 reads as its signed value.
		assert_eq!(data.getdata(1).unwrap().p1, -1);
		assert!(data.getdata(3).is_none());
		assert_eq!(data.getdata(-1), None);
	}

	#[test]
	fn set_param_writes_the_parameters() {
		let mut data = PointDataInterface::new();
		let path = temp_write(
			"setparam.pd1",
			&pd1_image(&[(0.0, 0.0, 0.0, 0.0, 1, 0, 0, 0)]),
		);
		data.load_file_data(&path);
		assert_eq!(data.set_param(0, 3, 4, 5, 6), 0);
		assert_eq!(data.getdata(0).unwrap().p3, 5);
		assert_eq!(data.set_param(1, 0, 0, 0, 0), 2);
	}

	#[test]
	fn search_pointdata_masks_and_offsets() {
		let points = [
			(0.0f32, 0.0, 0.0, 0.0, 3i8, 0, 0, 5),
			(1.0, 0.0, 0.0, 0.0, 3, 0, 0, 7),
			(2.0, 0.0, 0.0, 0.0, 8, 0, 0, 7),
			(3.0, 0.0, 0.0, 0.0, 3, 0, 0, 7),
		];
		let path = temp_write("search.pd1", &pd1_image(&points));
		let mut data = PointDataInterface::new();
		data.load_file_data(&path);

		// p4 only.
		let (count, first) = data.search_pointdata(0x08, 0, 0, 0, 7, 0);
		assert_eq!(count, 3);
		assert_eq!(first.unwrap().id, 1);
		assert_eq!(first.unwrap().x, 1.0);

		// The offset skips the first two matches.
		let (count, first) = data.search_pointdata(0x08, 0, 0, 0, 7, 2);
		assert_eq!(count, 2);
		assert_eq!(first.unwrap().id, 2);

		// p1 and p4 together.
		let (count, first) = data.search_pointdata(0x09, 3, 0, 0, 7, 0);
		assert_eq!(count, 2);
		assert_eq!(first.unwrap().id, 1);

		// Bits 5 to 7 are ignored, so this is p1 only.
		let (count, first) = data.search_pointdata(0xE1, 3, 0, 0, 0, 0);
		assert_eq!(count, 3);
		assert_eq!(first.unwrap().id, 0);

		// No match, and a mask of zero matches everything.
		assert_eq!(data.search_pointdata(0x01, 99, 0, 0, 0, 0).0, 0);
		let (count, first) = data.search_pointdata(0x00, 0, 0, 0, 0, 0);
		assert_eq!(count, 4);
		assert_eq!(first.unwrap().id, 0);

		// An offset past the end clamps to there, and a negative one to the start.
		assert_eq!(data.search_pointdata(0x00, 0, 0, 0, 0, 99).0, 0);
		assert_eq!(data.search_pointdata(0x00, 0, 0, 0, 0, -5).0, 4);
	}

	#[test]
	fn msg_file_supplies_the_point_messages() {
		let mut msg = Vec::new();
		msg.extend_from_slice(b"first line\r\n");
		msg.extend_from_slice(b"second line\n");
		msg.extend_from_slice(b"third line");
		let path = temp_write("msgs.pd1", &pd1_image(&[(0.0, 0.0, 0.0, 0.0, 1, 0, 0, 0)]));
		temp_write("msgs.msg", &msg);

		let mut data = PointDataInterface::new();
		data.load_file_data(&path);
		// A CRLF file leaves two linefeeds, an LF file one, and the last line has none.
		assert_eq!(cstr(data.get_message_text(0).unwrap()), b"first line\n\n");
		assert_eq!(cstr(data.get_message_text(1).unwrap()), b"second line\n");
		assert_eq!(cstr(data.get_message_text(2).unwrap()), b"third line");
		assert_eq!(cstr(data.get_message_text(3).unwrap()), b"");
		assert!(data.get_message_text(MAX_POINTMESSAGES as i32).is_none());
	}

	// --- MIF -------------------------------------------------------------

	#[test]
	fn mif_file_loads_every_field() {
		let mission = b"MISSION1\nMission One\nmap\\temp.bd1\nmap\\op.pd1\n3\n3\n!\n!\n!\nLine one\nLine two\n";
		let path = temp_write("mission.mif", mission);
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, false), 0);

		assert!(mif.get_filetype());
		assert_eq!(cstr(mif.get_mission_name()), b"MISSION1");
		assert_eq!(cstr(mif.get_mission_fullname()), b"Mission One");
		let (blockfile, pointfile) = mif.get_datafile_path();
		assert_eq!(cstr(blockfile), b"map\\temp.bd1");
		assert_eq!(cstr(pointfile), b"map\\op.pd1");
		assert_eq!(mif.get_skynumber(), 3);
		assert!(mif.get_collision_flag());
		assert!(mif.get_screen_flag());
		assert_eq!(cstr(mif.get_add_smallobject_file()), b"!");
		let (picture_a, picture_b) = mif.get_picturefile_path();
		assert_eq!(cstr(picture_a), b"!");
		assert_eq!(cstr(picture_b), b"!");
		// Both briefing lines keep their linefeeds, and count towards the total.
		assert_eq!(cstr(mif.get_briefing_text()), b"Line one\nLine two\n");
		assert_eq!(mif.get_total_datas(), 2);
	}

	#[test]
	fn mif_flag_numbers_pick_the_flags() {
		for (flag, collision, screen) in [
			(&b"0\n"[..], false, false),
			(b"1\n", true, false),
			(b"2\n", false, true),
			(b"3\n", true, true),
		] {
			let mission = [&b"M\nFull\nb.bd1\np.pd1\n1\n"[..], flag, &b"!\n!\n!\n"[..]].concat();
			let path = temp_write("flags.mif", &mission);
			let mut mif = MIFInterface::new();
			mif.load_file_data(&path, false);
			assert_eq!(mif.get_collision_flag(), collision, "flag {flag:?}");
			assert_eq!(mif.get_screen_flag(), screen, "flag {flag:?}");
		}
	}

	#[test]
	fn txt_briefing_loads_the_standard_format() {
		let briefing = b"briefA\n!\n2\nThe text\nMore text\n";
		let path = temp_write("briefing.txt", briefing);
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, false), 0);

		assert!(!mif.get_filetype());
		let (picture_a, picture_b) = mif.get_picturefile_path();
		assert_eq!(cstr(picture_a), b"data\\briefing\\briefA.bmp");
		assert_eq!(cstr(picture_b), b"!");
		assert_eq!(mif.get_skynumber(), 2);
		assert_eq!(cstr(mif.get_briefing_text()), b"The text\nMore text\n");
		assert_eq!(mif.get_total_datas(), 2);

		// The standard format has no mission name and no data file paths.
		assert_eq!(cstr(mif.get_mission_name()), b"");
		let (blockfile, pointfile) = mif.get_datafile_path();
		assert_eq!(cstr(blockfile), b"");
		assert_eq!(cstr(pointfile), b"");
		assert!(!mif.get_collision_flag());
		assert!(!mif.get_screen_flag());
	}

	#[test]
	fn mif_loads_the_additional_small_object_file() {
		// The path is relative to the working directory upstream, so the test names the
		// file it wrote by its full path.
		let smallso = temp_write(
			"smallso.txt",
			b"smallso.bmp\nsmallso.bmp\n200\n300\nso1.wav\n300\n",
		);
		let mut mission = b"M\nFull\nb.bd1\np.pd1\n0\n0\n".to_vec();
		mission.extend_from_slice(&smallso);
		mission.extend_from_slice(b"\n!\n!\n");
		let path = temp_write("addon.mif", &mission);

		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, true), 0);
		assert_eq!(cstr(mif.get_add_smallobject_file()), smallso.as_slice());
		assert_eq!(
			cstr(mif.get_add_smallobject_model_path(0).unwrap()),
			b"smallso.bmp"
		);
		assert_eq!(
			cstr(mif.get_add_smallobject_texture_path(0).unwrap()),
			b"smallso.bmp"
		);
		assert_eq!(
			cstr(mif.get_add_smallobject_sound_path(0).unwrap()),
			b"so1.wav"
		);
		// The 8 bit settings are masked.
		assert_eq!(mif.get_add_smallobject_decide(0), 200 & 0x7F);
		assert_eq!(mif.get_add_smallobject_hp(0), 300 & 0x7F);
		assert_eq!(mif.get_add_smallobject_jump(0), 300 & 0xFF);

		assert!(mif.get_add_smallobject_model_path(1).is_none());
		assert_eq!(mif.get_add_smallobject_decide(1), 0);

		// Without the additional file the settings stay at their initial values.
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, false), 0);
		assert_eq!(mif.get_add_smallobject_decide(0), 0);
		assert_eq!(cstr(mif.get_add_smallobject_model_path(0).unwrap()), b"");
	}

	#[test]
	fn missing_mif_fails() {
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(b"no_such_mission.mif", false), 1);
	}

	// --- addon list ------------------------------------------------------

	#[test]
	fn addon_list_collects_and_sorts_the_directory() {
		let dir = temp_path("addons");
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("create temp dir");
		xops_platform::fs::write_file(
			dir.join("b.mif"),
			b"BETA\nBeta\nb.bd1\np.pd1\n0\n0\n!\n!\n!\n",
		)
		.unwrap();
		xops_platform::fs::write_file(
			dir.join("a.MIF"),
			b"alpha\nAlpha\na.bd1\np.pd1\n0\n0\n!\n!\n!\n",
		)
		.unwrap();
		xops_platform::fs::write_file(dir.join("ignored.pd1"), b"nope").unwrap();

		let dir_bytes = dir.to_string_lossy().into_owned().into_bytes();
		let mut addons = AddonList::new();
		assert_eq!(addons.load_file_data(&dir_bytes), 2);
		assert_eq!(addons.get_total_datas(), 2);

		// A bubble sort on the lowercased name puts "alpha" first.
		assert_eq!(cstr(addons.get_mission_name_of(0).unwrap()), b"alpha");
		assert_eq!(cstr(addons.get_mission_name_of(1).unwrap()), b"BETA");
		// Case is not distinguished, so the uppercase extension is found too.
		assert!(cstr(addons.get_filename(0).unwrap()).eq_ignore_ascii_case(b"a.MIF"));
		assert_eq!(cstr(addons.get_filename(1).unwrap()), b"b.mif");
		// The upstream bounds these by the table size, not by the number found.
		assert!(addons.get_filename(-1).is_none());
		assert!(addons.get_filename(MAX_ADDONLIST as i32).is_none());

		assert_eq!(addons.load_file_data(b"no_such_dir"), 0);
		let _ = std::fs::remove_dir_all(&dir);
	}

	// --- INI -------------------------------------------------------------

	fn ini_bytes() -> Vec<u8> {
		b"; a comment\n\
		  [Game]\r\n\
		  Language=1\r\n\
		  ShowArm = 2 ; trailing comment\r\n\
		  Quoted = \"hello world\"\r\n\
		  Bad = abc\r\n\
		  Zero = 0\r\n\
		  Toggle = TRUE\r\n\
		  Float = 1.5f\r\n\
		  \r\n\
		  [Sounds]\r\n\
		  MasterVolume = 0.75\r\n\
		  Language = 9\r\n"
			.to_vec()
	}

	#[test]
	fn ini_int_reads_the_section_and_key() {
		let path = temp_path("config.ini");
		xops_platform::fs::write_file(&path, &ini_bytes()).expect("write ini");
		let mut ini = INIFileInterface::new();
		assert!(!ini.load_ini_file(&path), "load should succeed");

		let mut error = -1;
		assert_eq!(
			ini.get_ini_file_int(b"Game", b"Language", 0, Some(&mut error)),
			1
		);
		assert_eq!(error, 0);
		// Leading blanks and a trailing comment around the key.
		assert_eq!(ini.get_ini_file_int(b"Game", b"ShowArm", 0, None), 2);
		// The same key in another section has another value.
		assert_eq!(ini.get_ini_file_int(b"Sounds", b"Language", 0, None), 9);
		// A value that is not a number reads as zero, and the key was found.
		assert_eq!(
			ini.get_ini_file_int(b"Game", b"Bad", 42, Some(&mut error)),
			0
		);
		assert_eq!(error, 0);
	}

	#[test]
	fn ini_reports_a_missing_key_and_a_missing_file() {
		let path = temp_path("errors.ini");
		xops_platform::fs::write_file(&path, &ini_bytes()).expect("write ini");
		let mut error = 0;
		let mut ini = INIFileInterface::new();

		// Not open: the default comes back with error 2.
		assert_eq!(
			ini.get_ini_file_int(b"Game", b"Language", 5, Some(&mut error)),
			5
		);
		assert_eq!(error, 2);

		assert!(!ini.load_ini_file(&path));
		// The key is not there: error 3, and the default is returned.
		assert_eq!(
			ini.get_ini_file_int(b"Game", b"Missing", 7, Some(&mut error)),
			7
		);
		assert_eq!(error, 3);
		// The section is not there either.
		assert_eq!(
			ini.get_ini_file_int(b"Other", b"Language", 7, Some(&mut error)),
			7
		);
		assert_eq!(error, 3);
		// A file that cannot be opened fails, and loading twice fails too.
		assert!(ini.load_ini_file(Path::new("no_such_config.ini")));
		ini.release_ini_file();
		assert!(ini.load_ini_file(Path::new("no_such_config.ini")));
		assert_eq!(
			ini.get_ini_file_int(b"Game", b"Language", 5, Some(&mut error)),
			5
		);
		assert_eq!(error, 2);
	}

	#[test]
	fn ini_float_and_bool() {
		let path = temp_path("types.ini");
		xops_platform::fs::write_file(&path, &ini_bytes()).expect("write ini");
		let mut ini = INIFileInterface::new();
		ini.load_ini_file(&path);
		let mut error = 0;

		// The trailing f that marks a single precision literal is dropped.
		assert_eq!(
			ini.get_ini_file_float(b"Game", b"Float", 1.0, Some(&mut error)),
			1.5
		);
		assert_eq!(error, 0);
		assert_eq!(
			ini.get_ini_file_float(b"Sounds", b"MasterVolume", 1.0, None),
			0.75
		);
		assert_eq!(
			ini.get_ini_file_float(b"Game", b"Missing", 0.25, Some(&mut error)),
			0.25
		);
		assert_eq!(error, 3);

		assert!(ini.get_ini_file_bool(b"Game", b"Toggle", false, Some(&mut error)));
		assert_eq!(error, 0);
		// "0" is false, and the default is used when the key is missing.
		assert!(!ini.get_ini_file_bool(b"Game", b"Zero", true, None));
		assert!(ini.get_ini_file_bool(b"Game", b"Missing", true, Some(&mut error)));
		assert_eq!(error, 3);
		assert!(!ini.get_ini_file_bool(b"Game", b"Missing", false, None));
	}

	#[test]
	fn ini_string_keeps_quoted_spaces() {
		let path = temp_path("strings.ini");
		xops_platform::fs::write_file(&path, &ini_bytes()).expect("write ini");
		let mut ini = INIFileInterface::new();
		ini.load_ini_file(&path);

		assert_eq!(
			ini.get_ini_file_string(b"Game", b"Quoted", b"").as_slice(),
			b"hello world".as_slice()
		);
		// A key that is missing leaves the default.
		assert_eq!(
			ini.get_ini_file_string(b"Game", b"Missing", b"fallback")
				.as_slice(),
			b"fallback".as_slice()
		);
		// A value is not truncated by the string getter, but the typed getters read
		// through the upstream's 64 byte buffer, so they keep at most 63 characters.
		let long = b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
		let mut long_ini = ini_bytes();
		long_ini.extend_from_slice(b"Long = ");
		long_ini.extend_from_slice(long);
		long_ini.push(b'\n');
		xops_platform::fs::write_file(&path, &long_ini).unwrap();
		ini.release_ini_file();
		ini.load_ini_file(&path);
		assert_eq!(
			ini.get_ini_file_string(b"Sounds", b"Long", b"").len(),
			long.len()
		);
		let (value, error) = ini.resolve(b"Sounds", b"Long", b"d", 64);
		assert_eq!(error, 0);
		assert_eq!(value.len(), 63);
	}

	#[test]
	fn ini_without_a_section_stops_at_the_first_header() {
		let path = temp_path("nosection.ini");
		xops_platform::fs::write_file(&path, b"Top = 4\n[Game]\nLanguage = 1\n")
			.expect("write ini");
		let mut ini = INIFileInterface::new();
		ini.load_ini_file(&path);

		// No section name: the scan stops at the first section header, so only a key
		// above it is found.
		assert_eq!(ini.get_ini_file_int(b"", b"Top", 0, None), 4);
		assert_eq!(ini.get_ini_file_int(b"", b"Language", 0, None), 0);
	}
}
