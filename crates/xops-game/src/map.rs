//! The model, texture, map and 3D-effect half of `D3DGraphics`, from
//! `d3dgraphics-opengl.cpp`.
//!
//! The camera, the frame and the transform stack are in [`crate::d3dgraphics`] and the 2D
//! path is in [`crate::twod`]. This is everything that draws world geometry, which is the
//! texture registry, the map's blocks, the model table, the effect overlays and the debug
//! lines. It is one more `impl` block on `D3DGraphics`, because upstream is one class.
//!
//! Two things about the shape of it. The planar client arrays the upstream hands to
//! `glVertexPointer`, `glColorPointer` and `glTexCoordPointer` are interleaved into
//! [`Vertex`] here, because that is what the renderer takes. And a texture id of -1 is
//! "draw this untextured": the renderer has no such state, so those draws sample a 1x1
//! white texture that multiplies the vertex colour by one, which is what
//! `glDisable(GL_TEXTURE_2D)` did.

use std::io::Read;
use std::path::Path;

use xops_platform::gfx::Vertex;
use xops_platform::gl::{self, Rgba};
use xops_platform::texture;

use crate::datafile::{blockdataface, path_from_bytes, BlockDataInterface, Blockdata, TOTAL_BLOCKTEXTURE};
use crate::d3dgraphics::{D3DGraphics, MAX_TEXTURE};

/// The slot the untextured draws bind. It names no texture, so the renderer resolves it
/// to its 1x1 white texture. `twod` uses the same sentinel for the 2D primitives.
const UNTEXTURED: usize = usize::MAX;

/// The bytes of a C string: everything before the first NUL.
///
/// The game keeps its paths in fixed buffers, so the tail of one has stale bytes in it.
fn cstr(bytes: &[u8]) -> &[u8] {
	match bytes.iter().position(|&byte| byte == 0) {
		Some(end) => &bytes[..end],
		None => bytes,
	}
}

/// The first four bytes of a file, which is all the header check reads.
///
/// Upstream's `fread` into an uninitialised buffer leaves stack garbage in the bytes it
/// does not read, so a file shorter than four bytes could in principle match a format
/// from whatever happened to be there. Zeros match nothing, which fails the load.
fn file_head(path: &Path) -> [u8; 4] {
	let mut head = [0u8; 4];
	if let Ok(mut file) = std::fs::File::open(path) {
		let _ = file.read(&mut head);
	}
	head
}

/// Appends one block face's vertices, in the order the upstream writes them.
///
/// The order is `v[1], v[1], v[0], v[2], v[3], v[3]`. The repeat at each end is what lets
/// a whole texture group draw as a single triangle strip: the triangle joining two faces
/// is built from the last two vertices of one and the first of the next, which are the
/// same point, so it is degenerate and covers nothing. The two triangles that do cover
/// the face are `v[1] v[0] v[2]` and `v[0] v[2] v[3]`.
fn push_face(vertices: &mut Vec<Vertex>, data: &Blockdata, face_id: usize) {
	let mut v_id = [0i32; 4];
	let mut uv_id = [0i32; 4];
	if !blockdataface(face_id as i32, Some(&mut v_id), Some(&mut uv_id)) {
		return;
	}

	let material = &data.material[face_id];
	let shadow = material.shadow;

	for k in [1usize, 1, 0, 2, 3, 3] {
		let v = v_id[k] as usize;
		let uv = uv_id[k] as usize;
		vertices.push(Vertex {
			// The X negation the whole game draws with, because the data is left handed.
			position: [data.x[v] * -1.0, data.y[v], data.z[v]],
			uv: [material.u[uv], material.v[uv]],
			// A face is flat shaded: one brightness for all six of its vertices, with the
			// alpha the texture is modulated by.
			colour: [shadow, shadow, shadow, 1.0],
		});
	}
}

impl D3DGraphics {
	// -- textures -----------------------------------------------------------

	/// `CheckTextureFileExtension` (line 973) is
	/// [`xops_platform::texture::check_texture_file_extension`] and
	/// `CheckTextureFileTypeFlag` (line 1014) is
	/// [`xops_platform::texture::check_texture_file_type_flag`], which the load below
	/// calls. They live in the platform crate because they are the front half of the four
	/// decoders there, which are the same four the upstream dispatches to.

	/// The renderer's slot for a game texture id.
	///
	/// The game says "no texture" with -1, and it also names textures that failed to
	/// load. The upstream reaches for `glDisable(GL_TEXTURE_2D)` in both cases, which is
	/// the untextured sentinel here.
	fn texture_slot(&self, texture_id: i32) -> usize {
		if texture_id >= 0 && self.gfx_ref().texture_size(texture_id as usize).is_some() {
			texture_id as usize
		} else {
			UNTEXTURED
		}
	}

	/// `LoadTexture` (line 1050). Returns the texture's data number, or -1.
	///
	/// The data numbers are the renderer's texture slots: the upstream keeps its own
	/// `ptextures[MAX_TEXTURE]` table and scans it for a free `useflag`, and the renderer's
	/// table answers the same question with a slot that has no texture in it. The two
	/// agree on the range, so a game texture id is a slot id.
	pub fn load_texture(&mut self, filename: &[u8], texturefont: bool, black_transparent: bool) -> i32 {
		// Upstream ignores this one. The DirectX backend does too, and the texture font
		// path that would have used it is in `twod`.
		let _ = texturefont;

		// Find a free data number. Upstream refuses to go to the disk without one.
		let Some(id) = (0..MAX_TEXTURE).find(|&slot| self.gfx_ref().texture_size(slot).is_none()) else {
			return -1;
		};

		// The extension first, then the file's own header, which overrides it.
		//
		// The path is resolved once here because both the header read and the decode below
		// open it, and the game's data names its files in whatever case it was authored
		// with while the file on disk may differ. See `xops_platform::path`.
		let path = xops_platform::path::resolve_case_insensitive(&path_from_bytes(filename));
		let format = texture::check_texture_file_extension(filename, texture::FORMAT_UNKNOWN);
		let format = texture::check_texture_file_type_flag(&file_head(&path), format);

		// Unsupported format, which is also the answer for a file that cannot be opened.
		if format == texture::FORMAT_UNKNOWN {
			return -1;
		}

		let Ok(decoded) = texture::decode(&path, format, black_transparent) else {
			return -1;
		};

		// Upstream multiplies the vertex colour by the texture instead of replacing it,
		// which is what the shader does for every draw here.
		let gfx = self.gfx();
		gfx.bind_texture(id);
		gfx.upload_texture(id, decoded.width, decoded.height, &decoded.rgba);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR);
		gfx.tex_parameter(gl::GL_TEXTURE_2D, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR);

		// Upstream clamps the wrap mode for a non-power-of-two texture, but only in the
		// browser build, where the GL emulation could not repeat one. wgpu repeats any
		// size, so the default `GL_REPEAT` stands, as it does in the upstream native path.
		id as i32
	}

	/// `GetTextureSize` (line 1845). The two out-parameters and the status become the
	/// `Option`, which reports the same failures: an id out of range and a slot with no
	/// texture in it.
	pub fn get_texture_size(&self, id: i32) -> Option<(i32, i32)> {
		if id < 0 {
			return None;
		}
		let (width, height) = self.gfx_ref().texture_size(id as usize)?;
		Some((width as i32, height as i32))
	}

	/// `SetTexture` (line 1861).
	///
	/// Upstream remembers the last name it bound so that a repeat costs nothing. A draw
	/// here records the binding it was issued under, so there is no cache worth keeping
	/// and no stale entry to drop when a texture is released, which is the whole reason
	/// that version had to reset the cache in two places. An id that is -1 or names an
	/// unloaded slot binds the untextured sentinel instead of leaving whatever the
	/// previous draw bound.
	pub fn set_texture(&mut self, texture_id: i32) {
		let slot = self.texture_slot(texture_id);
		self.gfx().bind_texture(slot);
	}

	/// `CleanupTexture` (line 1875).
	pub fn cleanup_texture(&mut self, id: i32) {
		if !(0..MAX_TEXTURE as i32).contains(&id) {
			return;
		}
		// Releasing a slot that has no texture in it is a no-op in the renderer.
		self.gfx().delete_texture(id as usize);
	}

	/// `GetTotalTextures` (line 1896), counting the slots in use.
	pub fn get_total_textures(&self) -> i32 {
		(0..MAX_TEXTURE)
			.filter(|&slot| self.gfx_ref().texture_size(slot).is_some())
			.count() as i32
	}

	// -- the map ------------------------------------------------------------

	/// `LoadMapdata` (line 2235).
	///
	/// Takes the block data, the way the upstream takes the pointer to it, and loads the
	/// block textures from `directory`, which is the mission's data directory and ends in
	/// a separator (upstream passes `"data\\map10\\"`). Paths in the game data use
	/// backslashes, which `path_from_bytes` turns into separators at the boundary.
	///
	/// A slot the block data leaves empty is set to -1, which draws that material
	/// untextured. A texture that fails to load leaves -1 behind too, so a mission with a
	/// missing file still renders, which is what the upstream does.
	pub fn load_mapdata(&mut self, blocks: BlockDataInterface, directory: &[u8]) {
		self.set_block_data(blocks);

		for i in 0..TOTAL_BLOCKTEXTURE as i32 {
			// The name is copied out before the load, because loading borrows the
			// renderer mutably and that cannot happen while the block data is borrowed.
			let name = self
				.block_data()
				.and_then(|blocks| blocks.get_texture(i))
				.map(|name| cstr(name).to_vec())
				.unwrap_or_default();

			if name.is_empty() {
				// Not specified, so the slot stays empty.
				self.set_map_texture_id(i as usize, -1);
				continue;
			}

			// "directory + file name", which upstream `strcat`s into one MAX_PATH buffer.
			// A long enough pair overflows that buffer, which a Vec does not.
			let mut full = cstr(directory).to_vec();
			full.extend_from_slice(&name);

			let texture_id = self.load_texture(&full, false, false);
			self.set_map_texture_id(i as usize, texture_id);
		}
	}

	/// `RenderMapdata` (line 2269). This is the frame rate.
	///
	/// The upstream sizes three planar arrays for every face of every block, then walks
	/// the blocks once per texture group to fill them, so it reads a `blockdata` out of
	/// the interface `TOTAL_BLOCKTEXTURE` times per block (94 blocks here, so 940 copies
	/// of 484 bytes) and draws one strip per group. Nothing about a block changes between
	/// groups, so this reads each block once and files its faces into one bucket per
	/// texture, which yields the same vertices in the same order for the same ten draws
	/// at a tenth of the copying.
	///
	/// The vertices themselves are rebuilt every frame, as the upstream rebuilds them,
	/// rather than cached against the load. `Blockdata::calculation_blockdata` rewrites
	/// the face shadows in place, and the game calls it again when the dark screen is
	/// toggled, so a cache keyed on the load would render the old shading. At 94 blocks
	/// the rebuild is 564 six vertex faces, which is 20 KB, so there is nothing to win by
	/// caching it. What is left of the cost per frame is the interleave into [`Vertex`],
	/// which the renderer needs and the upstream never paid because it handed the planar
	/// arrays to GL.
	pub fn render_mapdata(&mut self, wireframe: bool) {
		if self.block_data().is_none() {
			return;
		}
		let bs = self.bs();

		if wireframe {
			// The debugging view: the twelve edges of every block, in green.
			let linecolor = Self::get_color_code(0.0, 1.0, 0.0, 1.0);
			for i in 0..bs {
				let Some(data) = self.block_data().and_then(|blocks| blocks.getdata(i)) else {
					continue;
				};
				// The four edges of the top face, then the four of the bottom face, then
				// the four uprights.
				for k in 0..4 {
					let j = (k + 1) % 4;
					self.render_line(data.x[k], data.y[k], data.z[k], data.x[j], data.y[j], data.z[j], linecolor);
					self.render_line(
						data.x[k + 4],
						data.y[k + 4],
						data.z[k + 4],
						data.x[j + 4],
						data.y[j + 4],
						data.z[j + 4],
						linecolor,
					);
					self.render_line(data.x[k], data.y[k], data.z[k], data.x[k + 4], data.y[k + 4], data.z[k + 4], linecolor);
				}
			}
			return;
		}

		let buckets = self.map_buckets();

		// Which slots are in use has to be read before the renderer is borrowed.
		let drawable: Vec<(usize, usize)> = (0..TOTAL_BLOCKTEXTURE)
			.filter(|texture_id| !buckets[*texture_id].is_empty())
			.map(|texture_id| {
				(
					self.texture_slot(self.map_texture_id(texture_id)),
					texture_id,
				)
			})
			.collect();

		let gfx = self.gfx();
		// The strips are drawn from their first vertex, and a strip's triangle-index
		// parity sets the winding, so starting at 0 reverses every triangle relative to
		// starting at 1. The upstream draws from 1 natively and from 0 under the browser
		// build, where it compensates by making the clockwise winding the front face for
		// the duration of the draw. This renderer draws from 0 too, so it needs the same
		// compensation: without it the back faces are kept and the level is drawn inside
		// out, which still looks like a level until the camera moves.
		gfx.front_face(gl::GL_CW);
		for (slot, texture_id) in drawable {
			gfx.bind_texture(slot);
			gfx.draw(gl::GL_TRIANGLE_STRIP, &buckets[texture_id]);
		}
		gfx.front_face(gl::GL_CCW);
	}

	/// Groups the map's faces by the texture they use, one bucket per block texture.
	///
	/// This is the batching the upstream reaches by walking every block once per texture
	/// group. Split out from the draw so it can be checked on its own: the sizes are a
	/// property of the map and the grouping rule, not of the camera, so they can be
	/// compared against the browser build's recorded draw stream. That comparison is where
	/// the expected sizes in the tests came from, and it is what caught that the two
	/// implementations agree vertex for vertex rather than only approximately.
	pub fn map_buckets(&self) -> [Vec<Vertex>; TOTAL_BLOCKTEXTURE] {
		let mut buckets: [Vec<Vertex>; TOTAL_BLOCKTEXTURE] = std::array::from_fn(|_| Vec::new());
		let bs = self.bs();

		for i in 0..bs {
			let Some(data) = self.block_data().and_then(|blocks| blocks.getdata(i)) else {
				continue;
			};
			for j in 0..6 {
				// A face whose texture id falls outside the block texture range matches no
				// group upstream, so it is never drawn. That is kept: a map with a tenth
				// texture on a face renders it as a hole, not as untextured geometry.
				let texture_id = data.material[j].texture_id;
				if texture_id < 0 {
					continue;
				}
				let Some(bucket) = buckets.get_mut(texture_id as usize) else {
					continue;
				};
				push_face(bucket, &data, j);
			}
		}

		buckets
	}

	/// `GetMapTextureID` (line 2402). -1 covers both an id out of range and a slot with
	/// no texture, which is what the caller checks for anyway.
	pub fn get_map_texture_id(&self, id: i32) -> i32 {
		if !(0..TOTAL_BLOCKTEXTURE as i32).contains(&id) {
			return -1;
		}
		self.map_texture_id(id as usize)
	}

	/// `CleanupMapdata` (line 2409).
	pub fn cleanup_mapdata(&mut self) {
		for i in 0..TOTAL_BLOCKTEXTURE as i32 {
			let texture_id = self.get_map_texture_id(i);
			self.cleanup_texture(texture_id);
		}

		// Upstream leaves the texture ids pointing at freed slots here and only clears
		// the block data, which is safe because the render checks for a loaded map first.
		// `clear_block_data` also puts the ids back to -1, so a stale map texture cannot
		// survive into the next map.
		self.clear_block_data();
	}

	// -- models and boards --------------------------------------------------

	/// `RenderModel` (line 2427).
	///
	/// `darkflag` picks the darkened colour array, which the model parser fills at 0.8
	/// against the lit 1.0. `nomodel` draws the three axes instead, which is how the game
	/// shows a model's orientation without the model. `hudmode` is unused upstream in both
	/// backends, and it stays in the signature to keep the call sites readable against it.
	pub fn render_model(&mut self, id_model: i32, id_texture: i32, darkflag: bool, hudmode: bool, nomodel: bool) {
		let _ = hudmode;

		if id_model == -1 {
			return;
		}

		// Upstream fails on a slot that is not loaded before it looks at `nomodel`, so a
		// stand-in is only drawn for a model that is there. The table is read through the
		// block below, which ends the borrow before the axes draw with the renderer.
		let Some(model) = self.models().get(id_model) else {
			return;
		};

		if nomodel {
			// The stand-in is drawn from -1 to 1 on each axis, so it sits at the model's
			// own scale.
			let colour = Self::get_color_code(1.0, 0.5, 0.0, 1.0);
			self.render_line(1.0, 0.0, 0.0, -1.0, 0.0, 0.0, colour);
			self.render_line(0.0, 1.0, 0.0, 0.0, -1.0, 0.0, colour);
			self.render_line(0.0, 0.0, 1.0, 0.0, 0.0, -1.0, colour);
			return;
		}

		let colour_ary = if darkflag {
			&model.color_gray_ary
		} else {
			&model.color_ary
		};

		// Upstream hands GL the three planar arrays it already holds. The renderer wants
		// one interleaved slice, so this is a copy of `polygons * 6` vertices that the
		// upstream did not make. A model is at most a few thousand vertices.
		let count = model.vertex_count().max(0) as usize;
		let vertices: Vec<Vertex> = model
			.vertex_ary
			.chunks_exact(3)
			.zip(colour_ary.chunks_exact(4))
			.zip(model.tex_coord_ary.chunks_exact(2))
			.map(|((position, colour), uv)| Vertex {
				position: [position[0], position[1], position[2]],
				uv: [uv[0], uv[1]],
				colour: [colour[0], colour[1], colour[2], colour[3]],
			})
			.take(count)
			.collect();

		let slot = self.texture_slot(id_texture);
		let gfx = self.gfx();
		// Same compensation as the map: the strip is drawn from its first vertex here,
		// which reverses every triangle, so the front face is flipped for the draw.
		gfx.front_face(gl::GL_CW);
		gfx.bind_texture(slot);
		gfx.draw(gl::GL_TRIANGLE_STRIP, &vertices);
		gfx.front_face(gl::GL_CCW);
	}

	/// `RenderBoard` (line 2497). A one-unit quad on the x = 0 plane, in whatever world
	/// transform the caller set up, which is how the game shows a nameplate.
	pub fn render_board(&mut self, id_texture: i32, alpha: f32, nomodel: bool) {
		if id_texture == -1 {
			return;
		}
		if self.gfx_ref().texture_size(id_texture as usize).is_none() {
			return;
		}

		if nomodel {
			let colour = Self::get_color_code(0.75, 0.75, 0.75, 1.0);
			self.render_line(0.0, 0.5, -0.5, 0.0, -0.5, -0.5, colour);
			self.render_line(0.0, -0.5, -0.5, 0.0, -0.5, 0.5, colour);
			self.render_line(0.0, -0.5, 0.5, 0.0, 0.5, 0.5, colour);
			self.render_line(0.0, 0.5, 0.5, 0.0, 0.5, -0.5, colour);
			return;
		}

		// The quad's four corners, with the texture's corners in the same order. Upstream
		// writes them straight into the arrays, and the repeat is the alpha for each
		// vertex.
		let colour = [1.0, 1.0, 1.0, alpha];
		let corners = [
			(0.5, 0.5, 1.0, 0.0),
			(-0.5, 0.5, 1.0, 1.0),
			(0.5, -0.5, 0.0, 0.0),
			(-0.5, -0.5, 0.0, 1.0),
		];
		let vertices: [Vertex; 4] = corners.map(|(y, z, u, v)| Vertex {
			position: [0.0, y, z],
			uv: [u, v],
			colour,
		});

		let slot = self.texture_slot(id_texture);
		let gfx = self.gfx();
		gfx.bind_texture(slot);
		gfx.draw(gl::GL_TRIANGLE_STRIP, &vertices);
	}

	// -- effects and overlays ----------------------------------------------

	/// `StartEffectRender` (line 2560), for the effects that must not be occluded.
	///
	/// Upstream has two versions. The native one turns the depth test off, which lets
	/// muzzle flashes, smoke and blood show through walls: the comment on the other branch
	/// says so, and the DirectX backend it was written to match only stopped writing depth
	/// (`D3DRS_ZWRITEENABLE = FALSE`) and kept the test. That is the branch ported here,
	/// because it is also what the browser build renders and therefore what the reference
	/// frames show. Effects still cannot be seen through a wall, and they no longer hide
	/// each other by writing depth over the geometry behind them.
	pub fn start_effect_render(&mut self) {
		self.gfx().depth_mask(false);
	}

	/// `EndEffectRender` (line 2574), which puts the depth write back.
	pub fn end_effect_render(&mut self) {
		self.gfx().depth_mask(true);
	}

	/// `ScreenBrightness` (line 2587).
	///
	/// A quad, not a blend state change: `Draw2DBox` covers the whole screen with white at
	/// `0.02 * brightness` alpha. Zero is the upstream's early out and draws nothing at
	/// all, which is why it is not just a zero alpha.
	pub fn screen_brightness(&mut self, width: i32, height: i32, brightness: i32) {
		if brightness == 0 {
			return;
		}

		let alpha = 0.02 * brightness as f32;
		self.draw_2d_box(0, 0, width, height, Self::get_color_code(1.0, 1.0, 1.0, alpha));
	}

	/// `RenderCenterline` (line 2598), the debug axes at the world origin, drawn from
	/// wherever the camera is.
	pub fn render_centerline(&mut self) {
		self.reset_world_transform();
		self.render_line(100.0, 0.0, 0.0, -100.0, 0.0, 0.0, Self::get_color_code(1.0, 0.0, 0.0, 1.0));
		self.render_line(0.0, 100.0, 0.0, 0.0, -100.0, 0.0, Self::get_color_code(0.0, 1.0, 0.0, 1.0));
		self.render_line(0.0, 0.0, 100.0, 0.0, 0.0, -100.0, Self::get_color_code(0.0, 0.0, 1.0, 1.0));
	}

	/// `Renderline` (line 2607), a two point line in the current world transform.
	pub fn render_line(&mut self, x1: f32, y1: f32, z1: f32, x2: f32, y2: f32, z2: f32, color: u32) {
		let colour = Rgba(color).to_floats();
		let vertices = [
			Vertex {
				// The same X negation the rest of the world drawing does.
				position: [x1 * -1.0, y1, z1],
				uv: [0.0, 0.0],
				colour,
			},
			Vertex {
				position: [x2 * -1.0, y2, z2],
				uv: [0.0, 0.0],
				colour,
			},
		];

		let gfx = self.gfx();
		gfx.bind_texture(UNTEXTURED);
		gfx.draw(gl::GL_LINE_STRIP, &vertices);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use xops_platform::gfx::Gfx;

	/// A square target, so a pixel coordinate is the same distance from the centre
	/// whichever axis it is on.
	const SIZE: u32 = 64;

	/// The real game data. `cargo test` runs a test binary from its package directory, so
	/// these are relative to the crate root and not to the machine. The map is the
	/// training map, which is the one the game itself starts on.
	const MAP_DIR: &[u8] = b"../../web/data/map0/";
	const MAP_FILE: &[u8] = b"../../web/data/map0/temp.bd1";
	/// One of the block textures that map names, and one it does not.
	const BMP_TEXTURE: &[u8] = b"../../web/data/map0/kabe.bmp";
	const OTHER_TEXTURE: &[u8] = b"../../web/data/map0/siba.bmp";
	/// A model with three vertices, small enough that the test has to scale it up.
	const MODEL_FILE: &str = "../../web/data/model/arm00.x";

	fn headless() -> D3DGraphics {
		let instance = xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
			backends: xops_platform::wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = Gfx::new(&instance, None, SIZE, SIZE, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// The map's blocks, loaded and shaded the way the game loads them.
	fn map_blocks() -> BlockDataInterface {
		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(MAP_FILE), 0, "the training map loads");
		blocks.calculation_blockdata(false);
		blocks
	}

	#[test]
	fn the_map_batches_exactly_as_the_browser_build_does() {
		// The expected sizes are not derived here. They are read out of the browser
		// build's recorded draw stream for this map, which is what makes this a check
		// against the reference rather than against the port's own arithmetic.
		//
		// The reference records, immediately after the sky:
		//
		//   draw 5 1 1 0 1 1 ... 1512
		//   draw 5 1 1 0 1 1 ...  252
		//   draw 5 1 1 0 1 1 ...  252
		//   draw 5 1 1 0 1 1 ...  144
		//   draw 5 1 1 0 1 1 ... 1200
		//   draw 5 1 1 0 1 1 ...   24
		//
		// Topology 5 is a triangle strip, and the counts match this batching size for
		// size in the same order. That is the map renderer agreeing with the browser build
		// vertex for vertex, which a rendered image could not establish this precisely
		// because the camera would have to be identical too.
		if !std::path::Path::new("../../web/data/map0/temp.bd1").is_file() {
			eprintln!("skipping: the game data is not present");
			return;
		}

		let mut graphics = headless();
		graphics.set_block_data(map_blocks());

		let sizes: Vec<usize> = graphics
			.map_buckets()
			.iter()
			.map(Vec::len)
			.filter(|size| *size > 0)
			.collect();
		assert_eq!(sizes, vec![1512, 252, 252, 144, 1200, 24]);
		assert_eq!(sizes.iter().sum::<usize>(), 3384);
	}

	/// Draws one frame of `body` and reads it back.
	fn render(graphics: &mut D3DGraphics, body: impl FnOnce(&mut D3DGraphics)) -> Vec<u8> {
		assert_eq!(graphics.start_render(), 0);
		body(graphics);
		assert!(!graphics.end_render(), "end_render reports failure, not success");
		let (_, _, pixels) = graphics.gfx_ref().read_frame().expect("read the frame");
		pixels
	}

	fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
		let index = ((y * SIZE + x) * 4) as usize;
		[
			pixels[index],
			pixels[index + 1],
			pixels[index + 2],
			pixels[index + 3],
		]
	}

	/// How many pixels are not the black the frame clears to.
	fn lit(pixels: &[u8]) -> usize {
		pixels
			.chunks_exact(4)
			.filter(|pixel| pixel[0] != 0 || pixel[1] != 0 || pixel[2] != 0)
			.count()
	}

	/// The distinct colours a frame paints, without the black it cleared to and without
	/// the alpha channel, which is 255 everywhere.
	fn painted(pixels: &[u8]) -> Vec<[u8; 3]> {
		pixels
			.chunks_exact(4)
			.map(|pixel| [pixel[0], pixel[1], pixel[2]])
			.filter(|colour| *colour != [0, 0, 0])
			.collect::<std::collections::BTreeSet<_>>()
			.into_iter()
			.collect()
	}

	/// A quad in normalised device coordinates, for the tests that want geometry in a
	/// known place without a camera in the way. The projection is the identity until a
	/// camera is set, so the vertex z is the depth.
	fn ndc_quad(graphics: &mut D3DGraphics, z: f32, colour: [f32; 4]) {
		let vertices: [Vertex; 4] = [(-0.8, -0.8), (0.8, -0.8), (-0.8, 0.8), (0.8, 0.8)].map(|(x, y)| Vertex {
			position: [x, y, z],
			uv: [0.0, 0.0],
			colour,
		});
		let gfx = graphics.gfx();
		gfx.bind_texture(UNTEXTURED);
		gfx.draw(gl::GL_TRIANGLE_STRIP, &vertices);
	}

	const LIGHT: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
	const MID: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

	#[test]
	fn a_texture_takes_a_slot_and_gives_it_back() {
		let mut graphics = headless();
		assert_eq!(graphics.get_total_textures(), 0);

		let id = graphics.load_texture(BMP_TEXTURE, false, false);
		assert!(id >= 0, "a real .bmp loads");
		let (width, height) = graphics.get_texture_size(id).expect("a loaded texture has a size");
		assert!(width > 0 && height > 0, "{width}x{height}");
		assert_eq!(graphics.get_total_textures(), 1);

		// A second load takes the next slot and leaves the first alone.
		let other = graphics.load_texture(OTHER_TEXTURE, false, false);
		assert_ne!(id, other);
		assert_eq!(graphics.get_total_textures(), 2);

		graphics.cleanup_texture(id);
		assert!(graphics.get_texture_size(id).is_none());
		assert_eq!(graphics.get_total_textures(), 1);

		// An id outside the range is refused rather than panicking, on both the getter and
		// the release.
		assert!(graphics.get_texture_size(-1).is_none());
		assert!(graphics.get_texture_size(MAX_TEXTURE as i32).is_none());
		graphics.cleanup_texture(-1);
		graphics.cleanup_texture(MAX_TEXTURE as i32);
		assert_eq!(graphics.get_total_textures(), 1);
	}

	#[test]
	fn a_texture_that_cannot_be_read_loads_as_nothing() {
		let mut graphics = headless();

		// No such file: the extension says jpeg, so only the failed read can refuse it.
		assert_eq!(graphics.load_texture(b"../../web/data/map0/nope.jpg", false, false), -1);
		// A file whose extension means nothing to the game, whatever its bytes are.
		assert_eq!(graphics.load_texture(MAP_FILE, false, false), -1);
		assert_eq!(graphics.get_total_textures(), 0, "a failed load takes no slot");
	}

	#[test]
	fn loading_a_map_loads_the_textures_it_names() {
		let mut graphics = headless();
		graphics.load_mapdata(map_blocks(), MAP_DIR);

		assert_eq!(graphics.bs(), 94, "the training map is 94 blocks");
		// The map names six textures in its ten slots and leaves the rest empty.
		let loaded = (0..TOTAL_BLOCKTEXTURE as i32)
			.filter(|&i| graphics.get_map_texture_id(i) >= 0)
			.count();
		assert_eq!(loaded, 6);
		assert_eq!(graphics.get_total_textures(), 6);
		assert_eq!(graphics.get_map_texture_id(6), -1, "an empty slot stays empty");
		assert_eq!(graphics.get_map_texture_id(-1), -1);
		assert_eq!(graphics.get_map_texture_id(TOTAL_BLOCKTEXTURE as i32), -1);

		graphics.cleanup_mapdata();
		assert_eq!(graphics.bs(), 0);
		assert!(graphics.block_data().is_none());
		assert_eq!(graphics.get_total_textures(), 0, "the map's textures go with it");
	}

	#[test]
	fn a_map_whose_textures_are_missing_still_loads() {
		let mut graphics = headless();
		graphics.load_mapdata(map_blocks(), b"../../web/data/map0/not_here/");

		// The blocks are there and every texture slot failed, which has to stay a
		// renderable state rather than an error, because a mission with one missing file
		// is a normal thing to hand the game.
		assert_eq!(graphics.bs(), 94);
		for i in 0..TOTAL_BLOCKTEXTURE as i32 {
			assert_eq!(graphics.get_map_texture_id(i), -1);
		}
		assert_eq!(graphics.get_total_textures(), 0);
	}

	/// Places the camera clear of the map on one side of it, looking along the x axis.
	///
	/// With both angles at zero the camera looks at increasing game x: the transform
	/// negates X on the way in, so the world it faces is the one with the larger x. From
	/// below the map's smallest x it therefore looks at the map, and from above the
	/// largest x it looks away from it.
	fn camera_facing_the_map(graphics: &mut D3DGraphics, facing: bool) {
		let x = if facing { -300.0 } else { 300.0 };
		graphics.set_camera(x, 25.0, -35.0, 0.0, 0.0, 1.0);
	}

	#[test]
	fn the_map_draws_where_the_camera_looks() {
		let mut graphics = headless();
		graphics.load_mapdata(map_blocks(), MAP_DIR);
		camera_facing_the_map(&mut graphics, true);

		let pixels = render(&mut graphics, |graphics| graphics.render_mapdata(false));
		assert!(lit(&pixels) > 2000, "the map fills the view, {} pixels lit", lit(&pixels));
	}

	#[test]
	fn the_map_draws_nothing_when_the_camera_looks_away() {
		let mut graphics = headless();
		graphics.load_mapdata(map_blocks(), MAP_DIR);
		// The same camera on the other side of the map, looking away from it.
		camera_facing_the_map(&mut graphics, false);

		let pixels = render(&mut graphics, |graphics| graphics.render_mapdata(false));
		assert_eq!(lit(&pixels), 0, "nothing is in front of the camera");
	}

	#[test]
	fn the_wireframe_view_draws_the_block_edges() {
		let mut graphics = headless();
		graphics.load_mapdata(map_blocks(), MAP_DIR);
		camera_facing_the_map(&mut graphics, true);

		let pixels = render(&mut graphics, |graphics| graphics.render_mapdata(true));
		assert!(lit(&pixels) > 200, "{} pixels lit", lit(&pixels));
		// The wireframe is green, which the shaded view is not.
		let green = pixels
			.chunks_exact(4)
			.filter(|pixel| pixel[1] > 0 && pixel[0] == 0 && pixel[2] == 0)
			.count();
		assert_eq!(green, lit(&pixels), "every lit pixel is a green line");
	}

	#[test]
	fn a_model_draws_with_the_colour_array_its_flag_picks() {
		let mut graphics = headless();
		let id = graphics.load_model(Path::new(MODEL_FILE));
		assert!(id >= 0, "a real model loads");

		// The model is a few centimetres across, so it needs a scale to fill any of the
		// target. Its near face sits on x = 0 and the camera sits behind that, which is
		// the same placement the game uses for a model at a human's feet.
		graphics.set_camera(-2.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		graphics.set_world_transform(0.0, 0.0, 0.0, 0.0, 0.0, 8.0);

		// Untextured, the vertex colour is the whole answer, so the lit array paints
		// white and the darkened one paints 0.8 of white. Neither is empty, and each is
		// flat, which is what makes the flat shaded model flat.
		let lit_frame = render(&mut graphics, |graphics| graphics.render_model(id, -1, false, false, false));
		assert!(lit(&lit_frame) > 100);
		assert_eq!(painted(&lit_frame), vec![[255, 255, 255]]);

		let dark_frame = render(&mut graphics, |graphics| graphics.render_model(id, -1, true, false, false));
		assert!(lit(&dark_frame) > 100);
		assert_eq!(painted(&dark_frame), vec![[204, 204, 204]], "0.8 of white");

		// nomodel replaces the model with its axes, which are orange.
		let axes = render(&mut graphics, |graphics| graphics.render_model(id, -1, false, false, true));
		assert!(lit(&axes) > 20);
		assert!(axes.chunks_exact(4).any(|pixel| pixel == [255, 127, 0, 255]), "an orange axis line");

		// An id with nothing behind it draws nothing, which is the branch the callers that
		// pass a model that failed to load depend on.
		let empty = render(&mut graphics, |graphics| graphics.render_model(-1, -1, false, false, false));
		assert_eq!(lit(&empty), 0);
		let empty = render(&mut graphics, |graphics| graphics.render_model(id + 1, -1, false, false, false));
		assert_eq!(lit(&empty), 0);
	}

	#[test]
	fn a_board_is_a_quad_that_its_alpha_fades() {
		let mut graphics = headless();
		let texture = graphics.load_texture(BMP_TEXTURE, false, false);
		assert!(texture >= 0);

		graphics.set_camera(-2.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		graphics.set_world_transform(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);

		let opaque = render(&mut graphics, |graphics| graphics.render_board(texture, 1.0, false));
		assert!(lit(&opaque) > 100, "the board is drawn");

		// Alpha 0 leaves the frame as it was cleared, which is also the case the alpha
		// test in `start_render` rejects.
		let clear = render(&mut graphics, |graphics| graphics.render_board(texture, 0.0, false));
		assert_eq!(lit(&clear), 0);

		// A board with no texture is not drawn at all, so the texture failing to load
		// leaves the caller with nothing rather than a white square.
		let clear = render(&mut graphics, |graphics| graphics.render_board(-1, 1.0, false));
		assert_eq!(lit(&clear), 0);
		let clear = render(&mut graphics, |graphics| graphics.render_board(texture + 1, 1.0, false));
		assert_eq!(lit(&clear), 0);

		// nomodel is the four grey edges instead of the quad.
		let edges = render(&mut graphics, |graphics| graphics.render_board(texture, 1.0, true));
		assert!(lit(&edges) > 20);
		assert!(edges.chunks_exact(4).any(|pixel| pixel == [191, 191, 191, 255]), "a grey edge");
	}

	#[test]
	fn screen_brightness_washes_the_whole_frame_and_a_zero_draws_nothing() {
		let mut graphics = headless();
		let (width, height) = (graphics.width(), graphics.height());

		// Zero is the early out, so the frame is the black it cleared to.
		let dark = render(&mut graphics, |graphics| graphics.screen_brightness(width, height, 0));
		assert_eq!(lit(&dark), 0);

		// Brightness 4 is white at 0.08 alpha over everything, which lifts the black to 20
		// without filling it, and reaches every corner of the frame.
		let bright = render(&mut graphics, |graphics| graphics.screen_brightness(width, height, 4));
		assert_eq!(lit(&bright), (SIZE * SIZE) as usize, "the whole frame");
		assert_eq!(pixel(&bright, 0, 0), [20, 20, 20, 255]);
		assert_eq!(pixel(&bright, SIZE - 1, SIZE - 1), [20, 20, 20, 255]);
	}

	#[test]
	fn the_debug_lines_land_where_their_points_are() {
		let mut graphics = headless();
		// The camera looks at increasing game x, so anything at a larger x is in front of
		// it. It also sits clear of the world origin, because the centerline is drawn
		// around that origin and an axis through the eye itself is degenerate.
		graphics.set_camera(-50.0, 0.0, 0.0, 0.0, 0.0, 1.0);

		let red = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);
		let pixels = render(&mut graphics, |graphics| {
			graphics.render_line(0.0, -20.0, -20.0, 0.0, 20.0, 20.0, red)
		});
		assert!(lit(&pixels) > 10, "the line is drawn");
		assert_eq!(painted(&pixels), vec![[255, 0, 0]], "the line is its own colour");

		// The centerline is the three axes at the world origin, and it resets the world
		// transform itself, or the last model's origin would move it.
		graphics.set_world_transform(50.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let pixels = render(&mut graphics, |graphics| graphics.render_centerline());
		assert!(lit(&pixels) > 20);
		// The x axis runs along the view direction and lands on the centre point, where it
		// is one pixel or none at all, so only the other two are asserted.
		let colours = painted(&pixels);
		assert!(colours.contains(&[0, 255, 0]), "the y axis is green: {colours:?}");
		assert!(colours.contains(&[0, 0, 255]), "the z axis is blue: {colours:?}");
	}

	#[test]
	fn an_effect_is_still_hidden_by_what_was_drawn_before_it() {
		let mut graphics = headless();
		// A near quad first, then an effect behind it: the depth test keeps running while
		// the effect draws, so the wall wins. The upstream native path turned the test off
		// here and let effects show through walls.
		let pixels = render(&mut graphics, |graphics| {
			ndc_quad(graphics, -0.5, LIGHT);
			graphics.start_effect_render();
			ndc_quad(graphics, 0.5, MID);
			graphics.end_effect_render();
		});
		assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 255, 255, 255], "the wall");
	}

	#[test]
	fn an_effect_does_not_hide_what_is_drawn_after_it() {
		let mut graphics = headless();
		// The effect draws nearest and must not write depth, so the quad behind it, which
		// is nearer than the cleared depth buffer, still draws over it. If the effect had
		// written depth the second quad would be rejected and the frame would stay mid
		// grey.
		let pixels = render(&mut graphics, |graphics| {
			graphics.start_effect_render();
			ndc_quad(graphics, -0.5, MID);
			graphics.end_effect_render();
			ndc_quad(graphics, 0.0, LIGHT);
		});
		assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 255, 255, 255]);
	}
}
