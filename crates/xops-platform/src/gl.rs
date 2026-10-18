//! The OpenGL names the game refers to.
//!
//! The game is written against GL 1.1's fixed pipeline and its own `D3DGraphics` class
//! carries the calls. The renderer below keeps the same names so a line of upstream
//! code can be read against its Rust equivalent without a lookup table. Values are the
//! ones from `gl.h`, and they are load-bearing: the recorded draw stream the browser
//! build emits carries these numbers, and the parity harness compares against it.

#![allow(dead_code)]

pub type Enum = u32;

// Primitives, as the client-array draw mode.
pub const GL_POINTS: Enum = 0x0000;
pub const GL_LINES: Enum = 0x0001;
pub const GL_LINE_LOOP: Enum = 0x0002;
pub const GL_LINE_STRIP: Enum = 0x0003;
pub const GL_TRIANGLES: Enum = 0x0004;
pub const GL_TRIANGLE_STRIP: Enum = 0x0005;
pub const GL_TRIANGLE_FAN: Enum = 0x0006;
pub const GL_QUADS: Enum = 0x0007;
pub const GL_QUAD_STRIP: Enum = 0x0008;
pub const GL_POLYGON: Enum = 0x0009;

// Clear bits and masks.
pub const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
pub const GL_DEPTH_BUFFER_BIT: u32 = 0x0000_0100;
pub const GL_STENCIL_BUFFER_BIT: u32 = 0x0000_0400;

// Client array types.
pub const GL_VERTEX_ARRAY: Enum = 0x8074;
pub const GL_COLOR_ARRAY: Enum = 0x8076;
pub const GL_TEXTURE_COORD_ARRAY: Enum = 0x8078;
pub const GL_NORMAL_ARRAY: Enum = 0x8075;

// Matrix modes.
pub const GL_MODELVIEW: Enum = 0x1700;
pub const GL_PROJECTION: Enum = 0x1701;
pub const GL_TEXTURE: Enum = 0x1702;

// Capabilities.
pub const GL_ALPHA_TEST: Enum = 0x0BC0;
pub const GL_BLEND: Enum = 0x0BE2;
pub const GL_CULL_FACE: Enum = 0x0B44;
pub const GL_DEPTH_TEST: Enum = 0x0B71;
pub const GL_FOG: Enum = 0x0B60;
pub const GL_LIGHTING: Enum = 0x0B50;
pub const GL_LIGHT0: Enum = 0x4000;
pub const GL_LIGHT1: Enum = 0x4001;
pub const GL_NORMALIZE: Enum = 0x0BA1;
pub const GL_SCISSOR_TEST: Enum = 0x0C11;
pub const GL_TEXTURE_2D: Enum = 0x0DE1;
pub const GL_COLOR_MATERIAL: Enum = 0x0B57;

// Alpha test.
pub const GL_NEVER: Enum = 0x0200;
pub const GL_LESS: Enum = 0x0201;
pub const GL_EQUAL: Enum = 0x0202;
pub const GL_LEQUAL: Enum = 0x0203;
pub const GL_GREATER: Enum = 0x0204;
pub const GL_NOTEQUAL: Enum = 0x0205;
pub const GL_GEQUAL: Enum = 0x0206;
pub const GL_ALWAYS: Enum = 0x0207;

// Blend factors.
pub const GL_ZERO: Enum = 0;
pub const GL_ONE: Enum = 1;
pub const GL_SRC_ALPHA: Enum = 0x0302;
pub const GL_ONE_MINUS_SRC_ALPHA: Enum = 0x0303;
pub const GL_DST_ALPHA: Enum = 0x0304;
pub const GL_ONE_MINUS_DST_ALPHA: Enum = 0x0305;
pub const GL_SRC_COLOR: Enum = 0x0300;
pub const GL_ONE_MINUS_SRC_COLOR: Enum = 0x0301;
pub const GL_DST_COLOR: Enum = 0x0306;
pub const GL_ONE_MINUS_DST_COLOR: Enum = 0x0307;
pub const GL_SRC_ALPHA_SATURATE: Enum = 0x0308;

// Face culling and winding.
pub const GL_FRONT: Enum = 0x0404;
pub const GL_BACK: Enum = 0x0405;
pub const GL_FRONT_AND_BACK: Enum = 0x0408;
pub const GL_CW: Enum = 0x0900;
pub const GL_CCW: Enum = 0x0901;

// Depth functions reuse the comparison names above.

// Texture parameters.
pub const GL_TEXTURE_MIN_FILTER: Enum = 0x2801;
pub const GL_TEXTURE_MAG_FILTER: Enum = 0x2800;
pub const GL_TEXTURE_WRAP_S: Enum = 0x2802;
pub const GL_TEXTURE_WRAP_T: Enum = 0x2803;
pub const GL_NEAREST: Enum = 0x2600;
pub const GL_LINEAR: Enum = 0x2601;
pub const GL_NEAREST_MIPMAP_NEAREST: Enum = 0x2700;
pub const GL_LINEAR_MIPMAP_NEAREST: Enum = 0x2701;
pub const GL_NEAREST_MIPMAP_LINEAR: Enum = 0x2702;
pub const GL_LINEAR_MIPMAP_LINEAR: Enum = 0x2703;
pub const GL_TEXTURE_MAX_ANISOTROPY_EXT: Enum = 0x84FE;
pub const GL_REPEAT: Enum = 0x2901;
pub const GL_CLAMP: Enum = 0x2900;
pub const GL_CLAMP_TO_EDGE: Enum = 0x812F;
pub const GL_CLAMP_TO_BORDER: Enum = 0x812D;

// Pixel formats accepted by the texture upload path.
pub const GL_ALPHA: Enum = 0x1906;
pub const GL_RGB: Enum = 0x1907;
pub const GL_RGBA: Enum = 0x1908;
pub const GL_LUMINANCE: Enum = 0x1909;
pub const GL_LUMINANCE_ALPHA: Enum = 0x190A;
pub const GL_BGRA: Enum = 0x80E1;

pub const GL_UNSIGNED_BYTE: Enum = 0x1401;
pub const GL_UNSIGNED_SHORT: Enum = 0x1403;
pub const GL_UNSIGNED_INT: Enum = 0x1405;
pub const GL_FLOAT: Enum = 0x1406;

// glGetFloatv targets.
pub const GL_MODELVIEW_MATRIX: Enum = 0x0BA6;
pub const GL_PROJECTION_MATRIX: Enum = 0x0BA7;
pub const GL_TEXTURE_MATRIX: Enum = 0x0BA8;
pub const GL_VIEWPORT: Enum = 0x0BA2;
pub const GL_MATRIX_MODE: Enum = 0x0BA0;

// Fog.
pub const GL_FOG_MODE: Enum = 0x0B65;
pub const GL_FOG_COLOR: Enum = 0x0B66;
pub const GL_FOG_DENSITY: Enum = 0x0B62;
pub const GL_FOG_START: Enum = 0x0B63;
pub const GL_FOG_END: Enum = 0x0B64;
pub const GL_EXP: Enum = 0x0800;
pub const GL_EXP2: Enum = 0x0801;

// Hints.
pub const GL_PERSPECTIVE_CORRECTION_HINT: Enum = 0x0C50;
pub const GL_POINT_SMOOTH_HINT: Enum = 0x0C51;
pub const GL_LINE_SMOOTH_HINT: Enum = 0x0C52;
pub const GL_POLYGON_SMOOTH_HINT: Enum = 0x0C53;
pub const GL_FOG_HINT: Enum = 0x0C54;
pub const GL_DONT_CARE: Enum = 0x1100;
pub const GL_FASTEST: Enum = 0x1101;
pub const GL_NICEST: Enum = 0x1102;

// Shading model, which the game sets once for flat block faces.
pub const GL_FLAT: Enum = 0x1D00;
pub const GL_SMOOTH: Enum = 0x1D01;
pub const GL_SHADE_MODEL: Enum = 0x0B54;

// Light material and model targets, used by the block shading path.
pub const GL_AMBIENT: Enum = 0x1200;
pub const GL_DIFFUSE: Enum = 0x1201;
pub const GL_POSITION: Enum = 0x1203;
pub const GL_AMBIENT_AND_DIFFUSE: Enum = 0x1602;
pub const GL_SPECULAR: Enum = 0x1202;
pub const GL_EMISSION: Enum = 0x1600;
pub const GL_LIGHT_MODEL_AMBIENT: Enum = 0x0B53;
pub const GL_FRONT_AND_BACK_MATERIAL: Enum = 0x0408;
pub const GL_LIGHT_MODEL_TWO_SIDE: Enum = 0x0B52;

// String queries the version banner reads.
pub const GL_VENDOR: Enum = 0x1F00;
pub const GL_RENDERER: Enum = 0x1F01;
pub const GL_VERSION: Enum = 0x1F02;
pub const GL_EXTENSIONS: Enum = 0x1F03;

/// A packed colour, in the layout the game builds with `GetColorCode`: the low byte is
/// red and the high byte is alpha, matching `D3DCOLOR` and the GL 2D path's `RGBA`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rgba(pub u32);

impl Rgba {
	pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Rgba {
		Rgba((a as u32) << 24 | (b as u32) << 16 | (g as u32) << 8 | r as u32)
	}

	pub fn r(self) -> u8 {
		(self.0 & 0xFF) as u8
	}
	pub fn g(self) -> u8 {
		((self.0 >> 8) & 0xFF) as u8
	}
	pub fn b(self) -> u8 {
		((self.0 >> 16) & 0xFF) as u8
	}
	pub fn a(self) -> u8 {
		((self.0 >> 24) & 0xFF) as u8
	}

	/// The same packed value as four floats in 0..1, red first.
	pub fn to_floats(self) -> [f32; 4] {
		[
			self.r() as f32 / 255.0,
			self.g() as f32 / 255.0,
			self.b() as f32 / 255.0,
			self.a() as f32 / 255.0,
		]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn colour_code_packs_red_low_alpha_high() {
		// The game hands these values straight to the 2D path and reads them back out of
		// the recorder, so a channel swap here would repaint the whole HUD.
		let c = Rgba::new(0x11, 0x22, 0x33, 0x44);
		assert_eq!(c.r(), 0x11);
		assert_eq!(c.g(), 0x22);
		assert_eq!(c.b(), 0x33);
		assert_eq!(c.a(), 0x44);
		assert_eq!(c.0, 0x4433_2211);
	}

	#[test]
	fn floats_are_in_unit_range() {
		let f = Rgba::new(255, 128, 0, 255).to_floats();
		assert_eq!(f[0], 1.0);
		assert_eq!(f[2], 0.0);
		assert_eq!(f[3], 1.0);
		assert!((f[1] - 0.501_960_8).abs() < 1e-6);
	}
}
