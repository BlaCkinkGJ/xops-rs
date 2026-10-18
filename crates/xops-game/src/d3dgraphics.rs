//! The renderer the game talks to, from the upstream `D3DGraphics`.
//!
//! Upstream this class wraps GL 1.1 directly. Here it wraps [`Gfx`], which is the same
//! shape one layer down, so the call sites read the same. The methods that need a
//! configured screen size, the map data or the text rasteriser are added by the modules
//! that own them, as further `impl` blocks, which keeps this file to the camera, the
//! frame and the transform stack.

use xops_platform::font::TextRenderer;
use xops_platform::gfx::Gfx;
use xops_platform::gl;
use xops_platform::math::{Mat4, Vec3};

use crate::core::{MAX_PATH, TOTAL_HAVEWEAPON};
use crate::datafile::{BlockDataInterface, TOTAL_BLOCKTEXTURE};
use crate::model::ModelTable;

/// Near clipping plane, which is also roughly the minimum draw distance.
pub const CLIPPINGPLANE_NEAR: f32 = 1.0;
/// Far clipping plane, and the fog end, which is why fog stops at the same distance.
pub const CLIPPINGPLANE_FAR: f32 = 800.0;

/// Maximum number of texture slots.
pub const MAX_TEXTURE: usize = 72;

/// The fog's near boundary, from `SetFog`.
pub const FOG_START: f32 = 100.0;
/// The fog's far boundary, which is the far clipping plane.
pub const FOG_END: f32 = 800.0;

pub struct D3DGraphics {
	gfx: Gfx,
	/// Screen size, kept here because the projection and the HUD both need it.
	width: i32,
	height: i32,
	fullscreenflag: bool,
	language_mode: i32,

	camera_x: f32,
	camera_y: f32,
	camera_z: f32,
	camera_rx: f32,
	camera_ry: f32,
	viewangle: f32,

	models: ModelTable,

	/// The texture font, which is a texture like any other once loaded.
	texture_font_fname: [u8; MAX_PATH],
	texture_font: i32,

	/// Where the held weapons are drawn, per slot. Set from the screen size at init.
	hud_myweapon_x: [f32; TOTAL_HAVEWEAPON],
	hud_myweapon_y: [f32; TOTAL_HAVEWEAPON],
	hud_myweapon_z: [f32; TOTAL_HAVEWEAPON],

	/// Z coordinate the system font is drawn at. Upstream resets it per frame.
	system_font_posz: f32,

	/// The system font, discovered from the platform's font list. Absent when the machine
	/// has no usable font, in which case the game runs and only this text is missing.
	text: Option<TextRenderer>,
	/// Pixel size of the normal system font, scaled from the screen height the way the
	/// upstream derives it from the configured height.
	system_font_size: f32,
	/// Pixel size of the small system font, which is a constant upstream.
	system_small_font_size: f32,

	/// The built-in debug console font, 96 glyphs of 16 bytes as one bit per pixel.
	/// Upstream clears it in the constructor and fills it from `InitDebugFontData`.
	debug_font_data: [[u8; 16]; 96],
	/// Texture id of the rasterised debug font, or -1.
	texture_debug_font: i32,

	/// The block data of the loaded map, or nothing before one is loaded. Upstream holds a
	/// pointer to the caller's object and this owns it, because nothing else reads it after
	/// loading.
	block_data: Option<BlockDataInterface>,
	/// Number of blocks in the loaded map.
	bs: i32,
	/// Texture ids of the map's block textures, or -1 for each slot that is not loaded.
	map_texture_id: [i32; TOTAL_BLOCKTEXTURE],
}

impl D3DGraphics {
	/// Takes ownership of the renderer. Upstream builds this in `InitD3D` once the window
	/// and the GL context exist, which is the same point in the sequence here.
	pub fn new(gfx: Gfx, fullscreen: bool, language: i32) -> D3DGraphics {
		let (width, height) = gfx.size();
		let (width, height) = (width as i32, height as i32);
		let mut graphics = D3DGraphics {
			gfx,
			width,
			height,
			fullscreenflag: fullscreen,
			language_mode: language,
			camera_x: 0.0,
			camera_y: 0.0,
			camera_z: 0.0,
			camera_rx: 0.0,
			camera_ry: 0.0,
			viewangle: 0.0,
			models: ModelTable::new(),
			texture_font_fname: [0; MAX_PATH],
			texture_font: -1,
			hud_myweapon_x: [width as f32 - 140.0, width as f32 - 72.0],
			hud_myweapon_y: [height as f32 - 40.0, height as f32 - 25.0],
			hud_myweapon_z: [0.86, 0.93],
			system_font_posz: 1.0,
			text: None,
			system_font_size: (height as f32 / 480.0) * 18.0,
			system_small_font_size: 12.0,
			debug_font_data: [[0; 16]; 96],
			texture_debug_font: -1,
			block_data: None,
			bs: 0,
			map_texture_id: [-1; TOTAL_BLOCKTEXTURE],
		};
		graphics.gfx.viewport(0, 0, width as u32, height as u32);
		graphics
	}

	/// Forward and reverse the window size. The renderer has already resized its targets.
	///
	/// A zero edge is refused rather than stored, because the projection divides by the
	/// aspect and the renderer refuses the same size. Storing it would leave the two out of
	/// step.
	pub fn set_size(&mut self, width: i32, height: i32) {
		if width <= 0 || height <= 0 {
			return;
		}
		self.width = width;
		self.height = height;
		self.gfx.resize(width as u32, height as u32);

		// The HUD weapon's screen position is derived from the screen size, so it has to be
		// derived again when the screen changes. Upstream gets this for free by rerunning
		// `InitD3D` on a resolution change; here the screen can change at any moment,
		// because the window is fitted to the display before the game is even built. Left
		// stale, the held weapon is placed for the old screen and is drawn in the wrong
		// part of the HUD, which is exactly what it looked like.
		self.hud_myweapon_x = [width as f32 - 140.0, width as f32 - 72.0];
		self.hud_myweapon_y = [height as f32 - 40.0, height as f32 - 25.0];
	}

	pub fn width(&self) -> i32 {
		self.width
	}

	pub fn height(&self) -> i32 {
		self.height
	}

	pub fn aspect(&self) -> f32 {
		self.width as f32 / self.height as f32
	}

	/// The renderer the geometry modules draw through.
	pub fn gfx(&mut self) -> &mut Gfx {
		&mut self.gfx
	}

	/// Says that the host draws the frames for the display, so the game's own end of frame
	/// does not have to. Set by the windowed run, which presents; the offscreen one reads
	/// the colour target and needs the render.
	pub fn set_host_presents(&mut self, presents: bool) {
		self.gfx.set_host_presents(presents);
	}

	pub fn gfx_ref(&self) -> &Gfx {
		&self.gfx
	}

	pub fn models(&self) -> &ModelTable {
		&self.models
	}

	pub fn models_mut(&mut self) -> &mut ModelTable {
		&mut self.models
	}

	pub fn set_fullscreen_flag(&mut self, fullscreen: bool) {
		self.fullscreenflag = fullscreen;
	}

	pub fn get_fullscreen_flag(&self) -> bool {
		self.fullscreenflag
	}

	pub fn set_language_mode(&mut self, mode: i32) {
		self.language_mode = mode;
	}

	pub fn get_language_mode(&self) -> i32 {
		self.language_mode
	}

	pub fn hud_weapon_offset(&self, slot: usize) -> (f32, f32, f32) {
		(
			self.hud_myweapon_x[slot],
			self.hud_myweapon_y[slot],
			self.hud_myweapon_z[slot],
		)
	}

	pub fn texture_font(&self) -> i32 {
		self.texture_font
	}

	pub fn set_texture_font(&mut self, id: i32) {
		self.texture_font = id;
	}

	pub fn set_texture_font_name(&mut self, name: &[u8]) {
		let length = name.len().min(MAX_PATH - 1);
		self.texture_font_fname[..length].copy_from_slice(&name[..length]);
		self.texture_font_fname[length] = 0;
	}

	pub fn system_font_posz(&self) -> f32 {
		self.system_font_posz
	}

	pub fn set_system_font_posz(&mut self, posz: f32) {
		self.system_font_posz = posz;
	}

	/// Pixel size of the normal system font.
	pub fn system_font_size(&self) -> f32 {
		self.system_font_size
	}

	/// Pixel size of the small system font.
	pub fn system_small_font_size(&self) -> f32 {
		self.system_small_font_size
	}

	/// Finds and loads a system font. Upstream this is a GDI handle created in `InitD3D`
	/// and the failure is fatal there, so the same sense is kept: an error means the
	/// caller should report it rather than carry on silently.
	pub fn init_system_font(&mut self) -> Result<(), String> {
		let renderer = TextRenderer::new()?;
		self.text = Some(renderer);
		Ok(())
	}

	/// The system font, or an error naming why there is none. Drawing text without one is
	/// impossible rather than merely degraded, so the caller has to decide what to do.
	pub fn text(&mut self) -> Result<&mut TextRenderer, String> {
		self.text
			.as_mut()
			.ok_or_else(|| "no system font was loaded".to_string())
	}

	pub fn debug_font_data(&self) -> &[[u8; 16]; 96] {
		&self.debug_font_data
	}

	pub fn debug_font_data_mut(&mut self) -> &mut [[u8; 16]; 96] {
		&mut self.debug_font_data
	}

	pub fn texture_debug_font(&self) -> i32 {
		self.texture_debug_font
	}

	pub fn set_texture_debug_font(&mut self, id: i32) {
		self.texture_debug_font = id;
	}

	/// The loaded map's block data, if a map is loaded.
	pub fn block_data(&self) -> Option<&BlockDataInterface> {
		self.block_data.as_ref()
	}

	pub fn block_data_mut(&mut self) -> Option<&mut BlockDataInterface> {
		self.block_data.as_mut()
	}

	/// Takes ownership of a map's block data and records how many blocks it holds.
	pub fn set_block_data(&mut self, blocks: BlockDataInterface) {
		self.bs = blocks.get_total_datas();
		self.block_data = Some(blocks);
	}

	pub fn clear_block_data(&mut self) {
		self.block_data = None;
		self.bs = 0;
		self.map_texture_id = [-1; TOTAL_BLOCKTEXTURE];
	}

	/// Number of blocks in the loaded map.
	pub fn bs(&self) -> i32 {
		self.bs
	}

	/// Texture id of one of the map's block textures, or -1.
	pub fn map_texture_id(&self, id: usize) -> i32 {
		*self.map_texture_id.get(id).unwrap_or(&-1)
	}

	pub fn set_map_texture_id(&mut self, id: usize, texture: i32) {
		if let Some(slot) = self.map_texture_id.get_mut(id) {
			*slot = texture;
		}
	}

	// -- frame --------------------------------------------------------------

	/// Begins a frame. The upstream enables the four pieces of state the whole game draws
	/// under, so they are set once here rather than per draw.
	pub fn start_render(&mut self) -> i32 {
		self.gfx.begin_frame();
		self.gfx.clear_colour([0.0, 0.0, 0.0, 1.0]);
		self.gfx
			.clear(gl::GL_COLOR_BUFFER_BIT | gl::GL_DEPTH_BUFFER_BIT);

		self.gfx.enable(gl::GL_DEPTH_TEST);
		self.gfx.enable(gl::GL_CULL_FACE);
		self.gfx.enable(gl::GL_BLEND);
		// The 2D path draws with straight alpha, and so do the effects.
		self.gfx.alpha_test(gl::GL_GREATER, 0.0);
		self.gfx.enable(gl::GL_ALPHA_TEST);

		self.system_font_posz = 1.0;
		0
	}

	/// Ends a frame. Upstream this flushes and swaps buffers, which is what encoding and
	/// presenting the frame amounts to here. The return value keeps the upstream sense:
	/// it reports failure, so false is success.
	pub fn end_render(&mut self) -> bool {
		if let Err(error) = self.gfx.end_frame() {
			eprintln!("frame: {error}");
			return true;
		}
		false
	}

	pub fn reset_zbuffer(&mut self) {
		self.gfx.clear(gl::GL_DEPTH_BUFFER_BIT);
	}

	// -- camera and transforms ---------------------------------------------

	/// Rebuilds both matrices from the camera. Called at the start of every world
	/// transform, which is why the projection is rebuilt too and not only on a resize.
	pub fn reset_world_transform(&mut self) {
		// The up vector flips when the camera passes the horizon, and again exactly at
		// straight up or straight down, to match what DirectX produced. The game relies on
		// it: without the second flip the view rolls over at the poles.
		let mut camera_y_flag = if self.camera_ry.abs() <= std::f32::consts::FRAC_PI_2 {
			1.0
		} else {
			-1.0
		};
		if self.camera_ry.abs() == std::f32::consts::FRAC_PI_2 {
			camera_y_flag *= -1.0;
		}

		self.gfx.matrix_mode(gl::GL_PROJECTION);
		self.gfx.load_identity();
		// The game stores the field of view in radians and GL wants degrees.
		self.gfx.perspective(
			self.viewangle * (180.0 / std::f32::consts::PI),
			self.aspect(),
			CLIPPINGPLANE_NEAR,
			CLIPPINGPLANE_FAR,
		);

		self.gfx.matrix_mode(gl::GL_MODELVIEW);
		self.gfx.load_identity();

		// X is negated because the game's world is left handed and the renderer is not.
		let eye = Vec3::new(self.camera_x * -1.0, self.camera_y, self.camera_z);
		let rx = self.camera_rx * -1.0 + std::f32::consts::PI;
		let center = Vec3::new(
			self.camera_x * -1.0 + rx.cos() * self.camera_ry.cos(),
			self.camera_y + self.camera_ry.sin(),
			self.camera_z + rx.sin() * self.camera_ry.cos(),
		);
		self.gfx
			.mult_look_at(eye, center, Vec3::new(0.0, camera_y_flag, 0.0));
	}

	/// Sets a model's place in the world: position, the two rotation angles it needs, and
	/// a scale. Upstream overloads this on arity, which Rust does not, so the version that
	/// adds a rotation about Z is [`D3DGraphics::set_world_transform_rot`].
	pub fn set_world_transform(&mut self, x: f32, y: f32, z: f32, rx: f32, ry: f32, size: f32) {
		self.set_world_transform_rot(x, y, z, rx, ry, 0.0, size);
	}

	/// The world transform with an extra rotation about Z, used by the few models that
	/// need one.
	pub fn set_world_transform_rot(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		rx: f32,
		ry1: f32,
		ry2: f32,
		size: f32,
	) {
		self.reset_world_transform();
		self.gfx.matrix_mode(gl::GL_MODELVIEW);

		let to_degrees = 180.0 / std::f32::consts::PI;
		self.gfx.translate(x * -1.0, y, z);
		self.gfx.rotate(rx * -1.0 * to_degrees, 0.0, 1.0, 0.0);
		self.gfx.rotate(ry1 * to_degrees, 1.0, 0.0, 0.0);
		self.gfx.rotate(ry2 * -1.0 * to_degrees, 0.0, 0.0, 1.0);
		self.gfx.scale(size, size, size);
	}

	/// The world transform for an effect, which rotates about Z before X rather than after,
	/// so a spark can spin about its own axis.
	pub fn set_world_transform_effect(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		rx: f32,
		ry: f32,
		rt: f32,
		size: f32,
	) {
		self.reset_world_transform();
		self.gfx.matrix_mode(gl::GL_MODELVIEW);

		let to_degrees = 180.0 / std::f32::consts::PI;
		self.gfx.translate(x * -1.0, y, z);
		self.gfx.rotate(rx * -1.0 * to_degrees, 0.0, 1.0, 0.0);
		self.gfx.rotate(ry * -1.0 * to_degrees, 0.0, 0.0, 1.0);
		self.gfx.rotate(rt * to_degrees, 1.0, 0.0, 0.0);
		self.gfx.scale(size, size, size);
	}

	/// The world transform for the position where a human holds its weapon, which needs
	/// the hand offset applied inside the rotation.
	pub fn set_world_transform_human_weapon(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		mx: f32,
		my: f32,
		mz: f32,
		rx: f32,
		ry: f32,
		size: f32,
	) {
		self.reset_world_transform();
		self.gfx.matrix_mode(gl::GL_MODELVIEW);

		let to_degrees = 180.0 / std::f32::consts::PI;
		self.gfx.translate(x * -1.0, y, z);
		self.gfx.rotate(rx * -1.0 * to_degrees, 0.0, 1.0, 0.0);
		self.gfx.rotate(ry * to_degrees, 1.0, 0.0, 0.0);
		self.gfx.translate(mx * -1.0, my, mz);
		self.gfx.scale(size, size, size);
	}

	/// Places the weapon the player is holding, from a screen position rather than a world
	/// one. The HUD offsets are in screen pixels, so they are projected back through the
	/// camera to get a world point, and the model is scaled by a curve the upstream tuned
	/// against the screen height.
	pub fn set_world_transform_player_weapon(
		&mut self,
		rotation: bool,
		camera_rx: f32,
		camera_ry: f32,
		rx: f32,
		size: f32,
	) {
		// Slot 0 is the weapon in hand, slot 1 the one in reserve, and the reserve is
		// drawn unrotated with its back to the camera.
		let slot = if rotation { 0 } else { 1 };
		let (hud_x, hud_y, hud_z) = self.hud_weapon_offset(slot);
		let screen = Vec3::new(hud_x, self.height as f32 - hud_y, hud_z);

		self.reset_world_transform();
		self.gfx.matrix_mode(gl::GL_MODELVIEW);

		let viewport = self.gfx.get_viewport();
		let viewport = (
			viewport.0 as i32,
			viewport.1 as i32,
			viewport.2 as i32,
			viewport.3 as i32,
		);
		let modelview = self.modelview();
		let projection = self.projection();
		let Some(object) =
			xops_platform::math::unproject(screen, &modelview, &projection, viewport)
		else {
			// A singular pipeline means there is no sensible place to put the weapon, so
			// it is left at the origin rather than somewhere arbitrary.
			return;
		};

		// The upstream curve, from the screen height. A taller screen draws a bigger
		// weapon, and the terms are tuned rather than derived.
		let height = self.height as f32;
		let size = size * (0.0004 * height * height - 0.92 * height + 650.0) / 1000.0;

		// Where this ended up, for the trace. This is the one draw whose screen position,
		// distance and scale are all computed, so it is the one that depends on the size the
		// game believes the screen is and on the projection matching it.
		xops_platform::trace::viewmodel(
			screen.x,
			screen.y,
			((object.x - self.camera_x * -1.0).powi(2)
				+ (object.y - self.camera_y).powi(2)
				+ (object.z - self.camera_z).powi(2))
			.sqrt(),
			size,
		);

		let to_degrees = 180.0 / std::f32::consts::PI;
		self.gfx.translate(object.x, object.y, object.z);
		self.gfx.rotate(camera_rx * to_degrees, 0.0, 1.0, 0.0);
		self.gfx
			.rotate(camera_ry * -1.0 * to_degrees, 0.0, 0.0, 1.0);
		if rotation {
			self.gfx.rotate(rx * -1.0 * to_degrees, 0.0, 1.0, 0.0);
		} else {
			self.gfx.rotate(180.0, 0.0, 1.0, 0.0);
		}
		self.gfx.scale(size, size, size);
	}

	/// Draws the frame again, partway to the one before it, for a display that wants frames
	/// more often than the game makes them. See [`xops_platform::gfx::Gfx::present_between`].
	pub fn present_between(&mut self, t: f32) -> bool {
		self.gfx.present_between(t).unwrap_or(false)
	}

	/// `InitD3D` (line 153), minus what the host does for it.
	///
	/// Upstream this creates the window, the GL context and the two fonts. A window and a
	/// wgpu device have to exist before the renderer can be built, so the host makes them and
	/// hands the device over; what is left is the language selection and the fonts: the
	/// texture font the HUD and the menus draw with, which comes from the game's data, and
	/// the console's own font, which the code carries.
	///
	/// Returns 0 on success and 1 on failure, the upstream contract.
	pub fn init_d3d(&mut self, texture_font_filename: &[u8], language: i32) -> i32 {
		self.language_mode = language;

		self.set_texture_font_name(texture_font_filename);
		let font = self.load_texture(texture_font_filename, true, false);
		if font == -1 {
			return 1;
		}
		self.texture_font = font;

		self.init_debug_font_data();
		if !self.load_debug_font_texture() {
			return 1;
		}

		0
	}

	/// `GetWorldTransformPos` equivalent: reads the translation out of the current
	/// modelview, which is the point the last world transform placed the origin at in view
	/// space.
	pub fn get_world_transform_pos(&self) -> (f32, f32, f32) {
		match self.gfx.get_matrix(gl::GL_MODELVIEW_MATRIX) {
			Some(m) => (m[12], m[13], m[14]),
			None => (0.0, 0.0, 0.0),
		}
	}

	/// The current modelview, for callers that need to transform a point themselves.
	pub fn modelview(&self) -> Mat4 {
		match self.gfx.get_matrix(gl::GL_MODELVIEW_MATRIX) {
			Some(m) => Mat4 { m },
			None => Mat4::IDENTITY,
		}
	}

	/// The current projection, which `gluUnProject` needs alongside the modelview.
	pub fn projection(&self) -> Mat4 {
		match self.gfx.get_matrix(gl::GL_PROJECTION_MATRIX) {
			Some(m) => Mat4 { m },
			None => Mat4::IDENTITY,
		}
	}

	/// Turns fog on or off and picks its colour from the mission's sky number.
	///
	/// The colours are not a gradient: each sky has its own fixed haze, and number 2 is
	/// near black because that mission is at night.
	pub fn set_fog(&mut self, enablefog: bool, skynumber: i32) {
		if enablefog {
			let colour = match skynumber {
				1 => [0.25, 0.25 + 0.0625, 0.25],
				2 => [0.0625, 0.0625, 0.0625],
				3 => [0.0, 0.0625, 0.125],
				4 => [0.125, 0.0625, 0.0625],
				5 => [0.25, 0.125, 0.125],
				_ => [0.0, 0.0, 0.0],
			};
			self.gfx.set_fog(true, colour, FOG_START, FOG_END);
		} else {
			self.gfx.set_fog(false, [0.0, 0.0, 0.0], FOG_START, FOG_END);
		}
	}

	/// Places the camera and rebuilds the world transform from it.
	pub fn set_camera(
		&mut self,
		camera_x: f32,
		camera_y: f32,
		camera_z: f32,
		camera_rx: f32,
		camera_ry: f32,
		viewangle: f32,
	) {
		self.gfx
			.viewport(0, 0, self.width as u32, self.height as u32);
		self.camera_x = camera_x;
		self.camera_y = camera_y;
		self.camera_z = camera_z;
		self.camera_rx = camera_rx;
		self.camera_ry = camera_ry;
		self.viewangle = viewangle;
		self.reset_world_transform();
	}

	pub fn camera(&self) -> (f32, f32, f32, f32, f32, f32) {
		(
			self.camera_x,
			self.camera_y,
			self.camera_z,
			self.camera_rx,
			self.camera_ry,
			self.viewangle,
		)
	}

	/// Loads a model into the first free slot, or -1.
	pub fn load_model(&mut self, path: &std::path::Path) -> i32 {
		self.models.load(path)
	}

	pub fn morphing_model(&mut self, id_a: i32, id_b: i32) -> i32 {
		self.models.morph(id_a, id_b)
	}

	pub fn get_total_models(&self) -> i32 {
		self.models.get_total()
	}

	pub fn cleanup_model(&mut self, id: i32) {
		self.models.cleanup(id);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::degree_to_radian;

	fn headless() -> D3DGraphics {
		let instance =
			xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
				backends: xops_platform::wgpu::Backends::all(),
				..Default::default()
			});
		let gfx = Gfx::new(&instance, None, 64, 48, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// Transforms a point by the current modelview, the way a vertex would be.
	fn in_view_space(graphics: &D3DGraphics, world: Vec3) -> Vec3 {
		graphics.modelview().transform_point(world)
	}

	#[test]
	fn the_camera_matches_the_browser_builds_recorded_matrix() {
		// The reference transform, verbatim. The browser build's `?record=1` writes the
		// uniform block of every draw, so this is the modelview the browser build itself used
		// for the first block map strip of the training map, at frame 51, read out of that
		// stream rather than reconstructed here.
		//
		// The camera is the one the same frame's debug overlay prints, to the two decimals it
		// prints: `camera x:120.02 y:28.95 z:-174.95 rx:3.53 ry:-0.52`. That is why the
		// tolerance is a hundredth rather than a millionth: the reference's inputs are only
		// known to two decimal places, so the comparison is as precise as they are.
		const REFERENCE: [f32; 16] = [
			0.379_133_28,
			0.462_671_5,
			-0.801_369_5,
			0.0, //
			0.0,
			0.866_025_15,
			0.500_000_5,
			0.0, //
			0.925_342_1,
			-0.189_566_82,
			0.328_338_95,
			0.0, //
			207.396_21,
			-2.707_754,
			-53.209_976,
			1.0, //
		];

		let mut graphics = headless();
		graphics.set_camera(120.02, 28.95, -174.95, 3.53, -0.52, degree_to_radian(65.0));

		let mine = graphics.modelview().m;

		// The rotation: nine elements, and the reference's angles are known to two decimal
		// places, so a thousandth either way is the inputs and not the arithmetic.
		for i in [0, 1, 2, 4, 5, 6, 8, 9, 10] {
			assert!(
				(mine[i] - REFERENCE[i]).abs() < 5e-3,
				"rotation element {i}: {} against the reference's {}",
				mine[i],
				REFERENCE[i]
			);
		}

		// The translation is held far more loosely, and the reason is arithmetic rather than
		// slack: it is the rotation applied to a camera 200 units from the origin, so a five
		// thousandth error in an angle moves it by tens of thousandths. A tolerance of a
		// couple of units here still fails loudly for anything that matters, because a wrong
		// sign or a swapped axis moves the eye by hundreds.
		for i in 12..15 {
			assert!(
				(mine[i] - REFERENCE[i]).abs() < 2.0,
				"translation element {i}: {} against the reference's {}",
				mine[i],
				REFERENCE[i]
			);
		}
	}

	#[test]
	fn the_camera_looks_down_negative_x_with_no_rotation() {
		// With both angles at zero the upstream builds a look direction of
		// (cos(PI), sin(0), sin(PI)) which is straight down negative X. Pinning that is
		// what makes the world transform tests below meaningful.
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let point = in_view_space(&graphics, Vec3::new(-10.0, 0.0, 0.0));
		assert!((point.x).abs() < 1e-4, "{point:?}");
		assert!((point.y).abs() < 1e-4, "{point:?}");
		assert!((point.z - -10.0).abs() < 1e-4, "{point:?}");
	}

	#[test]
	fn the_world_transform_translates_and_negates_x() {
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);

		// A model placed at world x = 5 has its own origin moved to x = 5, which is five
		// further from a camera looking down negative X.
		graphics.set_world_transform(5.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let point = in_view_space(&graphics, Vec3::new(-10.0, 0.0, 0.0));
		assert!((point.z - -15.0).abs() < 1e-3, "further away: {point:?}");

		// And back at the origin, the same point is ten away again.
		graphics.set_world_transform(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let point = in_view_space(&graphics, Vec3::new(-10.0, 0.0, 0.0));
		assert!((point.z - -10.0).abs() < 1e-3, "{point:?}");
	}

	#[test]
	fn the_hud_weapons_place_follows_the_screen_size() {
		// The weapon drawn in the HUD is placed from its screen offsets, and those are
		// worked out from the screen size. They were worked out once, when the renderer was
		// built, and the window is fitted to the display after that: the game then placed
		// the weapon as if the screen were its old size, which put it in the wrong part of
		// the HUD. Resizing has to move the place with it.
		let mut d3dg = headless();
		d3dg.set_size(1280, 960);
		let (x, y, z) = d3dg.hud_weapon_offset(0);
		assert_eq!((x, y, z), (1140.0, 920.0, 0.86), "the held weapon's place");
		assert_eq!(
			d3dg.hud_weapon_offset(1),
			(1208.0, 935.0, 0.93),
			"the reserve's"
		);

		d3dg.set_size(1920, 1080);
		assert_eq!(
			d3dg.hud_weapon_offset(0),
			(1780.0, 1040.0, 0.86),
			"the place follows the new size"
		);

		// A screen size of nothing is refused rather than stored, which would divide by zero
		// in the aspect the projection uses.
		d3dg.set_size(0, 1080);
		assert_eq!(
			d3dg.hud_weapon_offset(1),
			(1848.0, 1055.0, 0.93),
			"unchanged"
		);
	}

	#[test]
	fn scaling_shrinks_the_model_in_view_space() {
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		graphics.set_world_transform(0.0, 0.0, 0.0, 0.0, 0.0, 0.5);
		// A vertex ten units out in model space lands five units out in view space.
		let point = in_view_space(&graphics, Vec3::new(-10.0, 0.0, 0.0));
		assert!((point.z - -5.0).abs() < 1e-3, "{point:?}");
	}

	#[test]
	fn the_projection_matches_the_field_of_view_and_the_aspect() {
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, std::f32::consts::FRAC_PI_2);
		let projection = graphics.projection();

		// Column 1 row 1 is 1 / tan(fovy / 2), and the game stores fovy in radians.
		let expected = 1.0 / (std::f32::consts::FRAC_PI_4).tan();
		assert!(
			(projection.m[5] - expected).abs() < 1e-4,
			"{} vs {expected}",
			projection.m[5]
		);
		// Column 0 row 0 divides by the aspect, so a wide screen narrows it.
		assert!(
			(projection.m[0] - expected / graphics.aspect()).abs() < 1e-4,
			"{}",
			projection.m[0]
		);
	}

	#[test]
	fn the_far_plane_lands_at_negative_one_in_clip_space() {
		// GL clip space, which the shader then remaps. A double remap would push the far
		// plane to the middle of the depth range and everything would z-fight.
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let projection = graphics.projection();
		let far =
			projection.transform_vec4(xops_platform::Vec4::new(0.0, 0.0, -CLIPPINGPLANE_FAR, 1.0));
		assert!((far.z / far.w - 1.0).abs() < 1e-3, "{}", far.z / far.w);
	}

	#[test]
	fn an_empty_sky_has_the_upstream_fog_colours() {
		let mut graphics = headless();
		// Sky 2 is the night mission, and its haze is darker than a plain black clear.
		graphics.set_fog(true, 2);
		assert!(graphics.gfx_ref().fog_enabled());
		assert_eq!(graphics.gfx_ref().fog_colour(), [0.0625, 0.0625, 0.0625]);
		// A sky number outside the table is black rather than a panic.
		graphics.set_fog(true, 99);
		assert_eq!(graphics.gfx_ref().fog_colour(), [0.0, 0.0, 0.0]);
		// And switching it off actually switches it off.
		graphics.set_fog(false, 2);
		assert!(!graphics.gfx_ref().fog_enabled());
	}

	#[test]
	fn the_fog_range_reaches_the_far_plane() {
		let mut graphics = headless();
		graphics.set_fog(true, 1);
		assert_eq!(graphics.gfx_ref().fog_range(), (FOG_START, FOG_END));
		assert_eq!(FOG_END, CLIPPINGPLANE_FAR);
	}

	#[test]
	fn the_camera_up_vector_flips_past_the_horizon() {
		// Level, then looking down past vertical. The modelview has to differ, because
		// that flip is what stops the view rolling over at the poles.
		let mut graphics = headless();
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let level = graphics.modelview().m;
		graphics.set_camera(0.0, 0.0, 0.0, 0.0, 2.0, 1.0);
		let over = graphics.modelview().m;
		assert_ne!(level, over);
		// The up basis vector is the matrix's second row, which in column-major storage is
		// m[1], m[5], m[9]. Its y component is what flips.
		assert!(level[5] > 0.0, "upright at level: {}", level[5]);
		assert!(over[5] < 0.0, "flipped past vertical: {}", over[5]);
	}

	#[test]
	fn the_hud_weapon_offsets_start_from_the_screen_size() {
		// These are what place the held weapon on screen, so they have to follow the
		// window rather than being fixed numbers.
		let graphics = headless();
		assert_eq!(
			graphics.hud_weapon_offset(0),
			(64.0 - 140.0, 48.0 - 40.0, 0.86)
		);
		assert_eq!(
			graphics.hud_weapon_offset(1),
			(64.0 - 72.0, 48.0 - 25.0, 0.93)
		);
	}

	#[test]
	fn a_frame_records_a_clear_and_the_state_the_game_draws_under() {
		let mut graphics = headless();
		assert_eq!(graphics.start_render(), 0);
		// The four pieces of state the whole game relies on.
		let gfx = graphics.gfx_ref();
		assert!(gfx.depth_test_enabled());
		assert!(gfx.cull_enabled());
		assert!(gfx.blend_enabled());
		assert!(gfx.alpha_test_enabled());
		// And the frame presents without an error on a headless target.
		assert!(
			!graphics.end_render(),
			"end_render reports failure, not success"
		);
	}

	#[test]
	fn a_resize_keeps_the_aspect_in_step() {
		let mut graphics = headless();
		assert_eq!(graphics.width(), 64);
		graphics.set_size(1280, 720);
		assert_eq!((graphics.width(), graphics.height()), (1280, 720));
		assert!((graphics.aspect() - 1280.0 / 720.0).abs() < 1e-6);
		// Zero is refused rather than producing a degenerate projection, and refusing it
		// must leave the tracked size in step with the renderer's.
		graphics.set_size(0, 0);
		assert_eq!((graphics.width(), graphics.height()), (1280, 720));
		assert_eq!(graphics.gfx_ref().size(), (1280, 720));
	}
}
