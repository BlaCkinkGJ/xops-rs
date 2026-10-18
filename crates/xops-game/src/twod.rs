//! The 2D half of `D3DGraphics`, from `d3dgraphics-opengl.cpp`.
//!
//! Every function here is a sibling of the camera and frame methods in
//! [`crate::d3dgraphics`], so it is one more `impl D3DGraphics` block rather than a
//! separate type. The upstream file draws the HUD, the menus and the console through
//! these calls, which is why they all share the same orthographic pass and the same
//! texture-id space as the 3D path.
//!
//! Strings arrive as Shift-JIS bytes and stay bytes. The texture font indexes its
//! glyphs by byte value, so decoding before that point would pick the wrong glyph, and
//! a byte at or above 0x80 is a real index rather than a negative number.

use std::path::Path;

use xops_platform::font::TextBitmap;
use xops_platform::frame;
use xops_platform::gl::{self, Rgba};
use xops_platform::gfx::Vertex;
use xops_platform::math::Vec3;

use crate::d3dgraphics::{D3DGraphics, MAX_TEXTURE};

/// The texture slot the normal system font uploads into.
///
/// The game's own textures occupy `0..MAX_TEXTURE`, so the two slots above that range
/// cannot be handed out by the loader. They are also pushed into the table on first use,
/// which keeps a later `Gfx::gen_texture` from returning them. Uploading into them is
/// what bumps the table, and the cost of that is one 1x1 texture.
const SYSTEM_FONT_TEXTURE: usize = MAX_TEXTURE;
/// The texture slot the small system font uploads into.
const SYSTEM_SMALL_FONT_TEXTURE: usize = MAX_TEXTURE + 1;

/// A texture id with nothing behind it, which the renderer resolves to its 1x1 white
/// texture. This is the equivalent of the upstream `glDisable(GL_TEXTURE_2D)` for the
/// untextured 2D primitives: without it they would sample whatever the previous draw
/// left bound.
const UNTEXTURED: usize = usize::MAX;

/// The length the upstream `strlen` would see: everything before the first NUL.
///
/// The game keeps its strings in fixed C buffers, so a fixed-size byte array usually has
/// stale bytes after the terminator and must not be drawn or measured whole.
fn c_str_len(bytes: &[u8]) -> usize {
	bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len())
}

impl D3DGraphics {
	// -- text sizing --------------------------------------------------------

	/// The longest line in a string, newlines allowed.
	///
	/// Upstream counts `char`s, so a two-byte Shift-JIS character is two. That is what
	/// the centring arithmetic expects, and it is why this walks bytes rather than
	/// decoded characters.
	pub fn str_max_line_len(string: &[u8]) -> i32 {
		let mut maxlen = 0;
		let mut count = 0;

		for &byte in &string[..c_str_len(string)] {
			if byte == b'\n' {
				if maxlen < count {
					maxlen = count;
				}
				count = 0;
			} else {
				count += 1;
			}
		}

		if maxlen < count {
			maxlen = count;
		}

		maxlen
	}

	/// The texture-font cell size after `limitlen` characters have to fit.
	///
	/// Upstream shrinks the cell's width and height by the ratio of the limit to the
	/// string length, in one step rather than by wrapping the text.
	pub fn get_font_text_size(
		string: &[u8],
		base_w: i32,
		base_h: i32,
		limitlen: i32,
	) -> (i32, i32) {
		let length = c_str_len(string);
		if limitlen <= 0 {
			return (base_w, base_h);
		}
		if length as i32 <= limitlen {
			return (base_w, base_h);
		}

		let percent = limitlen as f32 / length as f32;
		(
			(base_w as f32 * percent) as i32,
			(base_h as f32 * percent) as i32,
		)
	}

	/// Packs four floats into the colour integer the draw calls take.
	///
	/// The packing is [`Rgba::new`], which is the layout `D3DCOLOR` uses and the one the
	/// renderer's `Vertex` wants. Upstream clamps nothing: a float outside 0..1 is cast
	/// to `unsigned char` and is undefined there. Rust's cast saturates, which is the
	/// closest defined answer and keeps a caller's mistake from wrapping to black.
	pub fn get_color_code(red: f32, green: f32, blue: f32, alpha: f32) -> u32 {
		Rgba::new(
			(red * 255.0) as u8,
			(green * 255.0) as u8,
			(blue * 255.0) as u8,
			(alpha * 255.0) as u8,
		)
		.0
	}

	// -- system font (GDI upstream, canvas in the browser build) ------------

	/// The 2D pass the 2D draws share.
	///
	/// `glOrtho(0, width, height, 0, -1, 1)` puts y = 0 at the top, which is why every
	/// caller passes screen coordinates straight through. The modelview matrix becomes
	/// `gluLookAt(0, 0, 1, 0, 0, 0, 0, 1, 0)`, which is the identity with a negated z,
	/// so a positive z draws nearer.
	pub fn start_2d_render(&mut self) {
		let (width, height) = (self.width() as f32, self.height() as f32);

		let gfx = self.gfx();
		gfx.matrix_mode(gl::GL_PROJECTION);
		gfx.push_matrix();
		gfx.load_identity();
		gfx.ortho(0.0, width, height, 0.0, -1.0, 1.0);

		gfx.matrix_mode(gl::GL_MODELVIEW);
		gfx.push_matrix();
		gfx.load_identity();
		gfx.mult_look_at(Vec3::new(0.0, 0.0, 1.0), Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));

		gfx.disable(gl::GL_DEPTH_TEST);
		gfx.disable(gl::GL_CULL_FACE);
	}

	/// Undoes [`D3DGraphics::start_2d_render`] and leaves the 3D state behind it.
	///
	/// The `load_identity` before each pop is dead code upstream (the pop throws the
	/// identity away) but is kept so the two files still line up.
	pub fn end_2d_render(&mut self) {
		let gfx = self.gfx();
		gfx.matrix_mode(gl::GL_PROJECTION);
		gfx.load_identity();
		gfx.pop_matrix();

		gfx.matrix_mode(gl::GL_MODELVIEW);
		gfx.load_identity();
		gfx.pop_matrix();

		gfx.enable(gl::GL_DEPTH_TEST);
		gfx.enable(gl::GL_CULL_FACE);
	}

	/// Draws a string with the system font.
	///
	/// The rasteriser turns the whole string into one RGBA bitmap, white with the glyph
	/// coverage in alpha, so the vertex colour tints it. That is the same arrangement
	/// the canvas build settled on, and it keeps the Japanese text working without a
	/// bundled font.
	///
	/// The texture is a fresh upload every call. `TextRenderer` caches the raster, but
	/// the upload cannot reuse the texture because the slot would be overwritten before
	/// the frame is encoded, so each call creates a texture, view, sampler, bind group
	/// and a queue write. At a hundred labels a frame that is the first thing to fix if
	/// text ever shows up in a profile: cache the uploaded bitmap by (text, size) here
	/// and re-upload only on a miss.
	pub fn draw_2d_ms_font_text(&mut self, x: i32, y: i32, string: &[u8], color: u32) {
		// The GDI version places the baseline, so the +18 makes the top edge land where
		// the caller expects. The small font adds 12 for the same reason.
		let y = y + 18;
		let pixel_size = self.system_font_size();

		let bitmap = match self.text() {
			Ok(text) => match text.rasterize(string, pixel_size) {
				Ok(bitmap) => bitmap,
				Err(error) => {
					eprintln!("system font: {error}");
					return;
				}
			},
			Err(_) => return,
		};

		self.reserve_font_textures();
		self.start_2d_render();
		// Text is layered by system_font_posz, which needs the depth test that the rest
		// of the 2D pass turns off.
		self.gfx().enable(gl::GL_DEPTH_TEST);
		let z = self.system_font_posz();
		self.upload_system_texture(SYSTEM_FONT_TEXTURE, &bitmap);
		self.draw_text_quad(
			x as f32,
			y as f32,
			bitmap.width as f32,
			bitmap.height as f32,
			z,
			Rgba(color).to_floats(),
		);
		self.set_system_font_posz(z - 0.01);
		self.end_2d_render();
	}

	/// [`D3DGraphics::draw_2d_ms_font_text`] from a 640x480 layout.
	pub fn draw_2d_ms_font_text_scaling(&mut self, x: i32, y: i32, string: &[u8], color: u32) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_ms_font_text(
			(scaling_x * x as f32) as i32,
			(scaling_y * y as f32) as i32,
			string,
			color,
		);
	}

	/// Draws a string centred inside a `w` by `h` box.
	///
	/// `h` is unused, exactly as upstream, because the vertical position is the caller's.
	pub fn draw_2d_ms_font_text_center(
		&mut self,
		x: i32,
		y: i32,
		w: i32,
		h: i32,
		string: &[u8],
		color: u32,
	) {
		let _ = h;
		let fonthalfsize = (self.height() as f32 / 480.0 * 9.0) as i32;
		let offset = w / 2 - (Self::str_max_line_len(string) * fonthalfsize / 2);
		self.draw_2d_ms_font_text(x + offset, y, string, color);
	}

	/// [`D3DGraphics::draw_2d_ms_font_text_center`] from a 640x480 layout.
	pub fn draw_2d_ms_font_text_center_scaling(
		&mut self,
		x: i32,
		y: i32,
		w: i32,
		h: i32,
		string: &[u8],
		color: u32,
	) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_ms_font_text_center(
			(scaling_x * x as f32) as i32,
			(scaling_y * y as f32) as i32,
			w,
			h,
			string,
			color,
		);
	}

	/// Draws a string with the small system font.
	pub fn draw_2d_ms_small_font_text(&mut self, x: i32, y: i32, string: &[u8], color: u32) {
		let y = y + 12;
		let pixel_size = self.system_small_font_size();

		let bitmap = match self.text() {
			Ok(text) => match text.rasterize(string, pixel_size) {
				Ok(bitmap) => bitmap,
				Err(error) => {
					eprintln!("system font: {error}");
					return;
				}
			},
			Err(_) => return,
		};

		self.reserve_font_textures();
		self.start_2d_render();
		self.gfx().enable(gl::GL_DEPTH_TEST);
		let z = self.system_font_posz();
		self.upload_system_texture(SYSTEM_SMALL_FONT_TEXTURE, &bitmap);
		self.draw_text_quad(
			x as f32,
			y as f32,
			bitmap.width as f32,
			bitmap.height as f32,
			z,
			Rgba(color).to_floats(),
		);
		self.set_system_font_posz(z - 0.01);
		self.end_2d_render();
	}

	/// Keeps the two system-font slots out of the game's texture-id range.
	///
	/// `Gfx::upload_texture` grows the texture table to fit the id, so populating the
	/// last one is enough to make the next `gen_texture` return an id past both. The
	/// placeholder is a transparent pixel that is replaced by real text before any draw
	/// uses the slot.
	fn reserve_font_textures(&mut self) {
		if self
			.gfx_ref()
			.texture_size(SYSTEM_SMALL_FONT_TEXTURE)
			.is_some()
		{
			return;
		}
		self.gfx().upload_texture(
			SYSTEM_SMALL_FONT_TEXTURE,
			1,
			1,
			&[0, 0, 0, 0],
		);
	}

	/// Uploads a rasterised string into a font slot.
	///
	/// Linear filtering and clamping to the edge, like the browser port. The bitmap has
	/// one transparent pixel of padding on every side, so linear sampling cannot drag a
	/// neighbouring slot's pixels in at the seam.
	fn upload_system_texture(&mut self, id: usize, bitmap: &TextBitmap) {
		let gfx = self.gfx();
		gfx.bind_texture(id);
		gfx.upload_texture(id, bitmap.width, bitmap.height, &bitmap.rgba);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_WRAP_S, gl::GL_CLAMP_TO_EDGE);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_WRAP_T, gl::GL_CLAMP_TO_EDGE);
	}

	/// One textured quad, in the 2D pass's screen coordinates.
	fn draw_text_quad(
		&mut self,
		x: f32,
		y: f32,
		width: f32,
		height: f32,
		z: f32,
		colour: [f32; 4],
	) {
		let vertices = [
			Vertex {
				position: [x, y, z],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x + width, y, z],
				uv: [1.0, 0.0],
				colour,
			},
			Vertex {
				position: [x, y + height, z],
				uv: [0.0, 1.0],
				colour,
			},
			Vertex {
				position: [x + width, y + height, z],
				uv: [1.0, 1.0],
				colour,
			},
		];
		self.gfx().draw(gl::GL_TRIANGLE_STRIP, &vertices);
	}

	// -- texture font -------------------------------------------------------

	/// Draws a string with the game's texture font.
	///
	/// The atlas is 16 by 16 glyph cells and is indexed by byte value, which is the
	/// reason the strings stay bytes from the data file to here. `fontwidth` and
	/// `fontheight` are the cell's drawn size, not the atlas's.
	pub fn draw_2d_texture_font_text(
		&mut self,
		x: i32,
		y: i32,
		string: &[u8],
		color: u32,
		fontwidth: i32,
		fontheight: i32,
	) {
		if self.texture_font() == -1 {
			return;
		}
		let length = c_str_len(string);
		if length == 0 {
			return;
		}

		let colour = Rgba(color).to_floats();
		let font_u = 1.0 / 16.0;
		let font_v = 1.0 / 16.0;
		let mut vertices = Vec::with_capacity(length * 6);

		for (i, &byte) in string[..length].iter().enumerate() {
			// Upstream reads a signed char and adds 256 when negative, so the index is
			// always the byte itself.
			let w = byte as i32;
			let t_u = (w % 16) as f32 * font_u;
			let t_v = (w / 16) as f32 * font_v;
			let left = x as f32 + i as f32 * fontwidth as f32;
			let right = left + fontwidth as f32;
			let top = y as f32;
			let bottom = top + fontheight as f32;

			// Six vertices per glyph in one strip. The triangles that join one glyph to
			// the next are degenerate because the last two vertices repeat the first
			// two, so the strip draws the same quads the upstream array describes.
			for (position, uv) in [
				([left, top, 0.0], [t_u, t_v]),
				([left, top, 0.0], [t_u, t_v]),
				([right, top, 0.0], [t_u + font_u, t_v]),
				([left, bottom, 0.0], [t_u, t_v + font_v]),
				([right, bottom, 0.0], [t_u + font_u, t_v + font_v]),
				([right, bottom, 0.0], [t_u + font_u, t_v + font_v]),
			] {
				vertices.push(Vertex {
					position,
					uv,
					colour,
				});
			}
		}

		let texture = self.texture_font() as usize;
		self.start_2d_render();
		self.gfx().bind_texture(texture);
		self.gfx().draw(gl::GL_TRIANGLE_STRIP, &vertices);
		self.end_2d_render();
	}

	/// [`D3DGraphics::draw_2d_texture_font_text`] from a 640x480 layout.
	pub fn draw_2d_texture_font_text_scaling(
		&mut self,
		x: i32,
		y: i32,
		string: &[u8],
		color: u32,
		fontwidth: i32,
		fontheight: i32,
	) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_texture_font_text(
			(scaling_x * x as f32) as i32,
			(scaling_y * y as f32) as i32,
			string,
			color,
			(scaling_x * fontwidth as f32) as i32,
			(scaling_x * fontheight as f32) as i32,
		);
	}

	/// Draws texture-font text centred on the screen, with `x` as an extra offset.
	pub fn draw_2d_texture_font_text_center(
		&mut self,
		x: i32,
		y: i32,
		string: &[u8],
		color: u32,
		fontwidth: i32,
		fontheight: i32,
	) {
		let length = c_str_len(string) as i32;
		let centred = (self.width() - length * fontwidth) / 2 + x;
		self.draw_2d_texture_font_text(centred, y, string, color, fontwidth, fontheight);
	}

	/// [`D3DGraphics::draw_2d_texture_font_text_center`] from a 640x480 layout.
	pub fn draw_2d_texture_font_text_center_scaling(
		&mut self,
		x: i32,
		y: i32,
		string: &[u8],
		color: u32,
		fontwidth: i32,
		fontheight: i32,
	) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_texture_font_text_center(
			(scaling_x * x as f32) as i32,
			(scaling_y * y as f32) as i32,
			string,
			color,
			(scaling_x * fontwidth as f32) as i32,
			(scaling_x * fontheight as f32) as i32,
		);
	}

	// -- debug font ---------------------------------------------------------

	/// Fills `debug_font_data` with the built-in 8x16 console font.
	///
	/// 96 glyphs of 16 bytes, one bit per pixel, taken from `d3dgraphics-debugfont.cpp`.
	/// Upstream requires this before `LoadDebugFontTexture` and does not call it itself,
	/// so the same contract is kept.
	pub fn init_debug_font_data(&mut self) {
		*self.debug_font_data_mut() = DEBUG_FONT_DATA;
	}

	/// Rasterises `debug_font_data` into a 128x128 atlas and uploads it.
	///
	/// Sixteen glyphs across and eight rows down, 8x16 each. The first two rows are
	/// blank because they hold the control codes. Returns false if it is already loaded
	/// or there is no free texture slot, as upstream does.
	pub fn load_debug_font_texture(&mut self) -> bool {
		if self.texture_debug_font() != -1 {
			return false;
		}

		let mut id = None;
		for candidate in 0..MAX_TEXTURE {
			if self.gfx_ref().texture_size(candidate).is_none() {
				id = Some(candidate);
				break;
			}
		}
		let Some(id) = id else {
			return false;
		};

		let charwidth = 8usize;
		let charheight = 16usize;
		let width = charwidth * 16;
		let height = charheight * 8;

		// Leftmost pixel is the high bit of the row, which is the order the upstream bit
		// mask walk produces.
		let mut rgba = vec![0u8; width * height * 4];
		for glyph in 0..96 {
			let cell_x = glyph % 16;
			let cell_y = 2 + glyph / 16;
			let rows = self.debug_font_data()[glyph];
			for line_y in 0..charheight {
				let bits = rows[line_y];
				for line_x in 0..charwidth {
					if bits & (0x80 >> line_x) == 0 {
						continue;
					}
					let x = cell_x * charwidth + line_x;
					let y = cell_y * charheight + line_y;
					let index = (y * width + x) * 4;
					rgba[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
				}
			}
		}

		let gfx = self.gfx();
		gfx.bind_texture(id);
		gfx.upload_texture(id, width as u32, height as u32, &rgba);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR);
		// 128x128 is a power of two, so upstream's WebGL clamp is not needed and the
		// default repeat mode cannot be sampled past the last texel with UVs of 0..1.

		self.set_texture_debug_font(id as i32);
		true
	}

	/// Draws a string with the built-in debug console font, 8x16 per character.
	pub fn draw_2d_texture_debug_font_text(&mut self, x: i32, y: i32, string: &[u8], color: u32) {
		if self.texture_debug_font() == -1 {
			return;
		}
		let length = c_str_len(string);
		if length == 0 {
			return;
		}

		let fontwidth = 8;
		let fontheight = 16;
		let colour = Rgba(color).to_floats();
		let font_u = 1.0 / 16.0;
		let font_v = 1.0 / 8.0;
		let mut vertices = Vec::with_capacity(length * 6);

		for (i, &byte) in string[..length].iter().enumerate() {
			// A set high bit is a negative signed char upstream, and upstream maps that
			// to a space rather than to glyphs 128..255. The texture font does the
			// opposite, so the difference is kept.
			let w = (byte as i8) as i32;
			let w = if w < 0 { ' ' as i32 } else { w };
			let t_u = (w % 16) as f32 * font_u;
			let t_v = (w / 16) as f32 * font_v;
			let left = x as f32 + i as f32 * fontwidth as f32;
			let right = left + fontwidth as f32;
			let top = y as f32;
			let bottom = top + fontheight as f32;

			for (position, uv) in [
				([left, top, 0.0], [t_u, t_v]),
				([left, top, 0.0], [t_u, t_v]),
				([right, top, 0.0], [t_u + font_u, t_v]),
				([left, bottom, 0.0], [t_u, t_v + font_v]),
				([right, bottom, 0.0], [t_u + font_u, t_v + font_v]),
				([right, bottom, 0.0], [t_u + font_u, t_v + font_v]),
			] {
				vertices.push(Vertex {
					position,
					uv,
					colour,
				});
			}
		}

		let texture = self.texture_debug_font() as usize;
		self.start_2d_render();
		self.gfx().bind_texture(texture);
		self.gfx().draw(gl::GL_TRIANGLE_STRIP, &vertices);
		self.end_2d_render();
	}

	// -- primitives ---------------------------------------------------------

	pub fn draw_2d_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: u32) {
		let colour = Rgba(color).to_floats();
		let vertices = [
			Vertex {
				position: [x1 as f32, y1 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x2 as f32, y2 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
		];

		self.start_2d_render();
		self.gfx().bind_texture(UNTEXTURED);
		self.gfx().draw(gl::GL_LINE_STRIP, &vertices);
		self.end_2d_render();
	}

	/// The 17 points of the 16-gon upstream calls a circle.
	///
	/// The count comes from the loop bound, not from the radius: every radius draws the
	/// same 17 vertices, so a small one is a visibly faceted ring rather than a dot.
	fn cycle_points(x: i32, y: i32, r: i32) -> [(f32, f32); 17] {
		let mut points = [(0.0f32, 0.0f32); 17];
		for (i, point) in points.iter_mut().enumerate() {
			let angle = std::f32::consts::PI * 2.0 / 16.0 * i as f32;
			point.0 = x as f32 + angle.cos() * r as f32;
			point.1 = y as f32 + angle.sin() * r as f32;
		}
		points
	}

	pub fn draw_2d_cycle(&mut self, x: i32, y: i32, r: i32, color: u32) {
		let colour = Rgba(color).to_floats();
		let vertices: Vec<Vertex> = Self::cycle_points(x, y, r)
			.iter()
			.map(|&(px, py)| Vertex {
				position: [px, py, 0.0],
				uv: [0.0, 0.0],
				colour,
			})
			.collect();

		self.start_2d_render();
		self.gfx().bind_texture(UNTEXTURED);
		self.gfx().draw(gl::GL_LINE_STRIP, &vertices);
		self.end_2d_render();
	}

	pub fn draw_2d_box(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: u32) {
		let colour = Rgba(color).to_floats();
		let vertices = [
			Vertex {
				position: [x1 as f32, y1 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x2 as f32, y1 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x1 as f32, y2 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x2 as f32, y2 as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
		];

		self.start_2d_render();
		self.gfx().bind_texture(UNTEXTURED);
		self.gfx().draw(gl::GL_TRIANGLE_STRIP, &vertices);
		self.end_2d_render();
	}

	pub fn draw_2d_box_scaling(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, color: u32) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_box(
			(scaling_x * x1 as f32) as i32,
			(scaling_y * y1 as f32) as i32,
			(scaling_x * x2 as f32) as i32,
			(scaling_y * y2 as f32) as i32,
			color,
		);
	}

	/// Draws a whole texture, with `alpha` multiplying the colour.
	///
	/// Blending is already source-alpha over one-minus-source-alpha, so alpha is a
	/// vertex-colour component rather than a mode switch.
	pub fn draw_2d_texture(
		&mut self,
		x: i32,
		y: i32,
		id: i32,
		width: i32,
		height: i32,
		alpha: f32,
	) {
		if id == -1 {
			return;
		}

		let colour = [1.0, 1.0, 1.0, alpha];
		let vertices = [
			Vertex {
				position: [x as f32, y as f32, 0.0],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [(x + width) as f32, y as f32, 0.0],
				uv: [1.0, 0.0],
				colour,
			},
			Vertex {
				position: [x as f32, (y + height) as f32, 0.0],
				uv: [0.0, 1.0],
				colour,
			},
			Vertex {
				position: [(x + width) as f32, (y + height) as f32, 0.0],
				uv: [1.0, 1.0],
				colour,
			},
		];

		self.start_2d_render();
		self.gfx().bind_texture(id as usize);
		self.gfx().draw(gl::GL_TRIANGLE_STRIP, &vertices);
		self.end_2d_render();
	}

	pub fn draw_2d_texture_scaling(
		&mut self,
		x: i32,
		y: i32,
		id: i32,
		width: i32,
		height: i32,
		alpha: f32,
	) {
		let scaling_x = self.height() as f32 / 480.0;
		let scaling_y = self.height() as f32 / 480.0;
		self.draw_2d_texture(
			(scaling_x * x as f32) as i32,
			(scaling_y * y as f32) as i32,
			id,
			(scaling_x * width as f32) as i32,
			(scaling_y * height as f32) as i32,
			alpha,
		);
	}

	// -- screenshot ---------------------------------------------------------

	/// Writes the last rendered frame to a PNG. Returns false on failure, as upstream.
	///
	/// Upstream asks GL for RGB (dropping alpha, which a 24-bit BMP cannot hold), reads
	/// bottom row first and writes those rows into a bottom-up BMP, and swaps red and
	/// blue on the way. All three cancel out for a viewer, so the picture is upright and
	/// correctly coloured. Here the frame is already top row first, which is PNG's
	/// order, and RGBA, so nothing is flipped or swapped. The alpha is written rather
	/// than dropped, and it is already opaque because the clear colour has alpha 1.
	///
	/// The caller must have finished a frame with `end_render` first, because encoding
	/// happens there.
	pub fn save_screenshot(&mut self, filename: &Path) -> bool {
		let (width, height, pixels) = match self.gfx_ref().read_frame() {
			Ok(frame) => frame,
			Err(error) => {
				eprintln!("screenshot: {error}");
				return false;
			}
		};

		match frame::write_png(filename, width, height, &pixels) {
			Ok(()) => true,
			Err(error) => {
				eprintln!("screenshot: {error}");
				false
			}
		}
	}
}

/// The debug console font, byte for byte from `d3dgraphics-debugfont.cpp`.
///
/// 96 glyphs for byte values 0x20 to 0x7F. Each glyph is 16 rows and the high bit of a
/// row is the leftmost pixel.
const DEBUG_FONT_DATA: [[u8; 16]; 96] = [
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x10, 0x38, 0x38, 0x38, 0x38, 0x38, 0x10, 0x10, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00],
	[0x6C, 0x6C, 0x6C, 0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x6C, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x6C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x10, 0x10, 0x7C, 0xC2, 0xC0, 0xC0, 0x7C, 0x06, 0x06, 0x86, 0x7C, 0x10, 0x10, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x61, 0x63, 0x06, 0x0C, 0x18, 0x30, 0x63, 0xC3, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x30, 0x58, 0x50, 0x20, 0x50, 0x56, 0xCE, 0xCE, 0xCC, 0x4C, 0x7A, 0x00, 0x00, 0x00, 0x00],
	[0x0C, 0x1C, 0x18, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC0, 0x60, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x60, 0xC0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x44, 0x6C, 0x28, 0xFE, 0x28, 0x6C, 0x44, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x30, 0xFC, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x08, 0x10, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x30, 0x30, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x01, 0x03, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x18, 0x18, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x06, 0x0E, 0x0C, 0x18, 0x30, 0x62, 0xFE, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0x86, 0x06, 0x06, 0x06, 0x1C, 0x06, 0x06, 0x06, 0x86, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x04, 0x0C, 0x1C, 0x2C, 0x4C, 0x8C, 0x8C, 0xFE, 0x0C, 0x0C, 0x1E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7E, 0x60, 0x60, 0x60, 0x60, 0x7C, 0x06, 0x06, 0x06, 0x46, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x3C, 0x62, 0x60, 0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7E, 0x46, 0x46, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x3C, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x06, 0x06, 0x46, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C, 0x04, 0x08, 0x00, 0x00],
	[0x00, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x3C, 0x66, 0x66, 0x06, 0x0C, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xDE, 0xDE, 0xDE, 0xDC, 0xC0, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x10, 0x38, 0x6C, 0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xFC, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x66, 0x66, 0xFC, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x3C, 0x66, 0xC2, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC2, 0x66, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xF8, 0x6C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x6C, 0xF8, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xFC, 0x66, 0x60, 0x60, 0x64, 0x7C, 0x64, 0x60, 0x60, 0x66, 0xFC, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xFC, 0x66, 0x60, 0x60, 0x64, 0x7C, 0x64, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0xC0, 0xDE, 0xC6, 0xC6, 0x66, 0x3E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x1E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xCC, 0xD8, 0xF0, 0xE0, 0xF0, 0xD8, 0xCC, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xF0, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x66, 0x66, 0xFC, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xEE, 0xEE, 0xFE, 0xFE, 0xFE, 0xD6, 0xD6, 0xD6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xE6, 0xE6, 0xF6, 0xF6, 0xDE, 0xDE, 0xCE, 0xC6, 0xC6, 0xC2, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xFC, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x60, 0xF0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xD6, 0xD6, 0x7C, 0x10, 0x18, 0x0C, 0x00],
	[0x00, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0xFC, 0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0x7C, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x7E, 0x5A, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x6C, 0x6C, 0x38, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xD6, 0xD6, 0xD6, 0xD6, 0xD6, 0x7C, 0x7C, 0x6C, 0x28, 0x28, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x6C, 0x38, 0x6C, 0x6C, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x66, 0x66, 0x66, 0x66, 0x7E, 0x3C, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xFE, 0x06, 0x0E, 0x1C, 0x18, 0x38, 0x30, 0x70, 0xE0, 0xC0, 0xFE, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x1E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x1E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xF0, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0xF0, 0x00, 0x00, 0x00, 0x00],
	[0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00],
	[0x60, 0x70, 0x30, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xE0, 0x60, 0x60, 0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0xFC, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x3C, 0x66, 0xC0, 0xC0, 0xC0, 0x66, 0x3C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x1C, 0x0C, 0x0C, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xFE, 0xC0, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x1C, 0x36, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x30, 0x30, 0x78, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x06, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0x7E, 0x0C, 0xC6, 0xC6, 0x7C, 0x00],
	[0x00, 0xC0, 0xC0, 0xC0, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x18, 0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x0C, 0x0C, 0x00, 0x1C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0x78, 0x00],
	[0x00, 0xC0, 0xC0, 0xC0, 0xC0, 0xC6, 0xCC, 0xD8, 0xF0, 0xD8, 0xCC, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xEC, 0x7E, 0x56, 0x56, 0x56, 0x56, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xFC, 0x66, 0x66, 0x66, 0x66, 0x66, 0x7C, 0x60, 0x60, 0xF0, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x7A, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x7C, 0x0C, 0x0C, 0x1E, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xD8, 0xEC, 0xCC, 0xCC, 0xC0, 0xC0, 0xC0, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC0, 0x7C, 0x06, 0xC6, 0x7C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xCE, 0x76, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x6C, 0x38, 0x10, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xD6, 0xD6, 0x7C, 0x6C, 0x28, 0x28, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0x6C, 0x38, 0x6C, 0xC6, 0xC6, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0x6C, 0x7C, 0x3C, 0x18, 0x18, 0x30, 0x60, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xFE, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x0E, 0x18, 0x18, 0x18, 0x18, 0x70, 0x18, 0x18, 0x18, 0x18, 0x0E, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0xE0, 0x30, 0x30, 0x30, 0x30, 0x1C, 0x30, 0x30, 0x30, 0x30, 0xE0, 0x00, 0x00, 0x00, 0x00],
	[0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
	[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
];

#[cfg(test)]
mod tests {
	use super::*;
	use xops_platform::gfx::Gfx;
	use xops_platform::wgpu;

	fn headless(width: u32, height: u32) -> Gfx {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});
		Gfx::new(&instance, None, width, height, None).expect("a headless renderer")
	}

	/// Runs `body` inside a frame and returns the read-back pixels, top row first.
	fn render(graphics: &mut D3DGraphics, body: impl FnOnce(&mut D3DGraphics)) -> Vec<u8> {
		graphics.start_render();
		body(graphics);
		assert!(!graphics.end_render(), "frame encoding failed");
		let (_, _, pixels) = graphics.gfx_ref().read_frame().expect("read frame");
		pixels
	}

	fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
		let index = ((y * width + x) * 4) as usize;
		[
			pixels[index],
			pixels[index + 1],
			pixels[index + 2],
			pixels[index + 3],
		]
	}

	#[test]
	fn str_max_line_len_counts_the_longest_run_between_newlines() {
		assert_eq!(D3DGraphics::str_max_line_len(b"abc"), 3);
		assert_eq!(D3DGraphics::str_max_line_len(b"ab\ncdef\nx"), 4);
		assert_eq!(D3DGraphics::str_max_line_len(b"abcdef\nab"), 6);
		assert_eq!(D3DGraphics::str_max_line_len(b""), 0);
		// A trailing newline must not hide the line before it.
		assert_eq!(D3DGraphics::str_max_line_len(b"abcdef\n"), 6);
		// A NUL ends the string, because the game passes fixed C buffers.
		assert_eq!(D3DGraphics::str_max_line_len(b"ab\0cdef"), 2);
		// Shift-JIS bytes count individually, which is what the centring wants.
		assert_eq!(D3DGraphics::str_max_line_len(&[0x82, 0xA0, 0x82, 0xA2]), 4);
	}

	#[test]
	fn get_font_text_size_shrinks_only_past_the_limit() {
		// A limit of zero or less leaves the cell alone.
		assert_eq!(D3DGraphics::get_font_text_size(b"abcdef", 20, 24, 0), (20, 24));
		assert_eq!(
			D3DGraphics::get_font_text_size(b"abcdef", 20, 24, -3),
			(20, 24)
		);
		// A string that fits is not grown.
		assert_eq!(D3DGraphics::get_font_text_size(b"abc", 20, 24, 3), (20, 24));
		assert_eq!(D3DGraphics::get_font_text_size(b"abc", 20, 24, 8), (20, 24));
		// Half the limit halves the cell, with the C truncation kept.
		assert_eq!(D3DGraphics::get_font_text_size(b"abcdef", 20, 24, 3), (10, 12));
		assert_eq!(D3DGraphics::get_font_text_size(b"abcdef", 21, 25, 4), (14, 16));
		// Bytes after a NUL do not count towards the length.
		assert_eq!(D3DGraphics::get_font_text_size(b"abcd\0ef", 20, 24, 2), (10, 12));
	}

	#[test]
	fn get_color_code_matches_the_packed_layout() {
		assert_eq!(
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0),
			Rgba::new(255, 0, 0, 255).0
		);
		assert_eq!(
			D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0),
			Rgba::new(0, 255, 0, 255).0
		);
		assert_eq!(
			D3DGraphics::get_color_code(0.0, 0.0, 1.0, 0.5),
			Rgba::new(0, 0, 255, 127).0
		);
		assert_eq!(
			D3DGraphics::get_color_code(0.75, 0.75, 0.75, 1.0),
			Rgba::new(191, 191, 191, 255).0
		);
		// Rust's cast saturates where the C cast is undefined, so out-of-range values
		// clamp rather than wrapping.
		assert_eq!(
			D3DGraphics::get_color_code(1.5, -0.5, 2.0, 1.0),
			Rgba::new(255, 0, 255, 255).0
		);
		// And the accessors read the same value back.
		let packed = Rgba(D3DGraphics::get_color_code(0.25, 0.5, 0.75, 1.0));
		assert_eq!((packed.r(), packed.g(), packed.b(), packed.a()), (63, 127, 191, 255));
	}

	#[test]
	fn a_cycle_is_seventeen_points_at_any_radius() {
		for radius in [0, 1, 7, 100] {
			let points = D3DGraphics::cycle_points(40, 30, radius);
			assert_eq!(points.len(), 17);
			for &(x, y) in &points {
				let distance = ((x - 40.0).powi(2) + (y - 30.0).powi(2)).sqrt();
				assert!(
					(distance - radius as f32).abs() < 0.001,
					"radius {radius} produced a point at {distance}"
				);
			}
			// The last point closes the loop.
			assert!((points[0].0 - points[16].0).abs() < 0.001);
			assert!((points[0].1 - points[16].1).abs() < 0.001);
		}
	}

	#[test]
	fn a_2d_box_lands_where_screen_coordinates_put_it() {
		let width = 64;
		let mut graphics = D3DGraphics::new(headless(width, width), false, 0);
		let pixels = render(&mut graphics, |graphics| {
			graphics.draw_2d_box(
				10,
				10,
				30,
				30,
				D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0),
			);
		});

		// Inside the box.
		assert_eq!(pixel(&pixels, width, 20, 20), [255, 0, 0, 255]);
		assert_eq!(pixel(&pixels, width, 11, 11), [255, 0, 0, 255]);
		// y = 0 is the top of the screen in this projection, so the box is in the
		// upper-left quadrant and the pixels below and right of it stay clear.
		assert_eq!(pixel(&pixels, width, 20, 5), [0, 0, 0, 255]);
		assert_eq!(pixel(&pixels, width, 5, 20), [0, 0, 0, 255]);
		assert_eq!(pixel(&pixels, width, 20, 50), [0, 0, 0, 255]);
		assert_eq!(pixel(&pixels, width, 50, 20), [0, 0, 0, 255]);
	}

	#[test]
	fn system_font_text_reaches_the_target_when_a_font_exists() {
		// A 480 line screen makes the normal font 18px, the size the menus use.
		let width = 320;
		let height = 480;
		let mut graphics = D3DGraphics::new(headless(width, height), false, 0);
		if graphics.init_system_font().is_err() {
			// No system font on this machine, which the game reports at startup.
			return;
		}

		graphics.start_render();
		let started = std::time::Instant::now();
		for _ in 0..50 {
			graphics.draw_2d_ms_font_text(
				4,
				4,
				b"A",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			);
		}
		let per_draw = started.elapsed() / 50;
		assert!(!graphics.end_render(), "frame encoding failed");
		let (_, _, pixels) = graphics.gfx_ref().read_frame().expect("read frame");
		eprintln!("one system-font draw (raster cached, one upload): {per_draw:?}");

		// "A" is drawn 18px down from y = 4 and is roughly 12 by 18, so ink is somewhere
		// in this band. Counting lit pixels avoids pinning an exact glyph shape.
		let lit = (20..60)
			.flat_map(|y| (0..60).map(move |x| (x, y)))
			.filter(|&(x, y)| pixel(&pixels, width, x, y)[0] > 0)
			.count();
		assert!(lit > 10, "only {lit} lit pixels");
	}

	#[test]
	fn the_texture_font_indexes_glyphs_by_byte_value() {
		let mut graphics = D3DGraphics::new(headless(16, 16), false, 0);

		// A 16 by 16 cell atlas, with only the cell for byte 0x82 (130) filled. If the
		// byte were treated as a signed char and added to 256, or decoded, this cell
		// would not be the one sampled.
		let mut atlas = vec![0u8; 128 * 128 * 4];
		let cell_x = (0x82 % 16) * 8;
		let cell_y = (0x82 / 16) * 8;
		for y in cell_y..cell_y + 8 {
			for x in cell_x..cell_x + 8 {
				let index = (y * 128 + x) * 4;
				atlas[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
			}
		}
		let id = graphics.gfx().gen_texture();
		graphics.gfx().upload_texture(id, 128, 128, &atlas);
		graphics.set_texture_font(id as i32);

		let pixels = render(&mut graphics, |graphics| {
			graphics.draw_2d_texture_font_text(
				0,
				0,
				&[0x82],
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				8,
				8,
			);
		});

		assert_eq!(pixel(&pixels, 16, 3, 3), [255, 255, 255, 255]);
		// Outside the one glyph's 8x8 quad nothing was drawn.
		assert_eq!(pixel(&pixels, 16, 8, 3), [0, 0, 0, 255]);
		assert_eq!(pixel(&pixels, 16, 3, 8), [0, 0, 0, 255]);
	}

	#[test]
	fn the_debug_font_atlas_puts_the_high_bit_on_the_left() {
		let mut graphics = D3DGraphics::new(headless(16, 16), false, 0);
		graphics.init_debug_font_data();
		assert!(graphics.load_debug_font_texture(), "atlas upload failed");

		let pixels = render(&mut graphics, |graphics| {
			graphics.draw_2d_texture_debug_font_text(
				0,
				0,
				b"A",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			);
		});

		// Glyph 'A' (0x41) has 0x10 on its second row, so the high bit is the leftmost
		// pixel: x = 3 is lit and x = 0 and x = 7 are not.
		assert!(pixel(&pixels, 16, 3, 1)[0] > 100);
		assert_eq!(pixel(&pixels, 16, 0, 1), [0, 0, 0, 255]);
		assert_eq!(pixel(&pixels, 16, 7, 1), [0, 0, 0, 255]);
		// The top row of 'A' is blank.
		assert_eq!(pixel(&pixels, 16, 3, 0), [0, 0, 0, 255]);
	}
}
