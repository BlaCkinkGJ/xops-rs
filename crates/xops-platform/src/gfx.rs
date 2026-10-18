//! The wgpu renderer behind the fixed-function pipeline.
//!
//! The game draws through client arrays and a transform stack, so this keeps both: a
//! matrix stack with GL's post-multiply order, and an accumulated frame that is replayed
//! into real render passes. Recording first and encoding later is what lets a frame hold
//! more than one pass, which it does: the game clears the depth buffer mid-frame so the
//! sky cannot occlude the level.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};

use crate::gl::{self, Enum};
use crate::math::{Mat4, Vec3};

/// One interleaved vertex, matching the shader's three attributes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable, PartialEq)]
pub struct Vertex {
	pub position: [f32; 3],
	pub uv: [f32; 2],
	pub colour: [f32; 4],
}

pub const VERTEX_FLOATS: usize = 9;

/// The uniform block layout the shader declares.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable, PartialEq)]
pub struct Uniforms {
	pub transform: [f32; 16],
	pub modelview: [f32; 16],
	/// rgb = colour, a > 0.5 means fog is on.
	pub fog: [f32; 4],
	/// x = start, y = end.
	pub fog_range: [f32; 4],
	/// x = alpha reference, 0 disables the test.
	pub params: [f32; 4],
}

pub const UNIFORM_SLOTS: usize = 8192;
const VERTEX_BUFFER_BYTES: u64 = 8 * 1024 * 1024;

/// A texture slot, mirroring the GL texture object the game binds.
struct TextureEntry {
	/// The owner of the resource the view below comes from. Never read directly: the
	/// view and the sampler are what draws use, and this keeps the texture resident.
	#[allow(dead_code)]
	texture: wgpu::Texture,
	view: wgpu::TextureView,
	sampler: wgpu::Sampler,
	width: u32,
	height: u32,
	wrap_s: Enum,
	wrap_t: Enum,
	min_filter: Enum,
	mag_filter: Enum,
}

/// What the renderer was asked to do this frame, in order.
enum Cmd {
	/// Clears the given buffers, which starts a new pass.
	Clear(u32),
	Draw {
		vertex_offset: u32,
		vertex_count: u32,
		uniform_slot: u32,
		texture: usize,
		key: PipelineKey,
	},
}

/// Every draw's matrices, blended between the frame before and the frame now.
///
/// At `t` of zero this is the previous frame and at one it is this one, with the two
/// matrices each draw carries blended straight through: the world is drawn through them, so
/// bending them is what places the whole scene partway through the step it is moving.
fn blend_uniforms(before: &[Uniforms], now: &[Uniforms], t: f32) -> Vec<Uniforms> {
	now.iter()
		.zip(before.iter())
		.map(|(now, before)| {
			let mut blend = *now;
			for i in 0..16 {
				blend.transform[i] =
					before.transform[i] + (now.transform[i] - before.transform[i]) * t;
				blend.modelview[i] =
					before.modelview[i] + (now.modelview[i] - before.modelview[i]) * t;
			}
			blend
		})
		.collect()
}

/// The uniform block for a frame's draws, each at its own dynamic-offset slot.
///
/// Separate from the renderer so that a second frame built from the same draws can be
/// packed without borrowing the renderer's own list.
fn pack_uniforms(uniforms: &[Uniforms], stride: usize) -> Option<Vec<u8>> {
	if uniforms.is_empty() {
		return None;
	}
	let size = std::mem::size_of::<Uniforms>();
	let mut block = vec![0u8; uniforms.len() * stride];
	for (slot, uniforms) in uniforms.iter().enumerate() {
		let offset = slot * stride;
		block[offset..offset + size].copy_from_slice(bytemuck::bytes_of(uniforms));
	}
	Some(block)
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
struct PipelineKey {
	topology: wgpu::PrimitiveTopology,
	front_face: wgpu::FrontFace,
	cull: Option<wgpu::Face>,
	blend: bool,
	alpha_test: bool,
	depth_test: bool,
	depth_write: bool,
}

impl PipelineKey {
	/// Which pipeline a GL draw mode needs. WebGPU has no quad or polygon primitive, so
	/// those fall back to a triangle list the way the upstream GL path folds them.
	fn topology(mode: Enum) -> wgpu::PrimitiveTopology {
		match mode {
			gl::GL_POINTS => wgpu::PrimitiveTopology::PointList,
			gl::GL_LINES => wgpu::PrimitiveTopology::LineList,
			gl::GL_LINE_LOOP | gl::GL_LINE_STRIP => wgpu::PrimitiveTopology::LineStrip,
			gl::GL_TRIANGLES => wgpu::PrimitiveTopology::TriangleList,
			gl::GL_TRIANGLE_STRIP => wgpu::PrimitiveTopology::TriangleStrip,
			gl::GL_TRIANGLE_FAN => wgpu::PrimitiveTopology::TriangleStrip,
			_ => wgpu::PrimitiveTopology::TriangleList,
		}
	}
}

/// The transform stacks and render state, held the way GL holds them.
struct State {
	modelview: Vec<Mat4>,
	projection: Vec<Mat4>,
	matrix_mode: Enum,
	clear_colour: [f32; 4],
	viewport: (u32, u32, u32, u32),

	alpha_test: bool,
	alpha_ref: f32,
	alpha_func: Enum,
	blend: bool,
	depth_test: bool,
	depth_write: bool,
	cull: bool,
	front_face: Enum,
	fog: bool,
	fog_colour: [f32; 3],
	fog_start: f32,
	fog_end: f32,

	bound_texture: usize,
}

impl Default for State {
	fn default() -> Self {
		State {
			modelview: vec![Mat4::IDENTITY],
			projection: vec![Mat4::IDENTITY],
			matrix_mode: gl::GL_MODELVIEW,
			// The game clears to opaque black; see D3DGraphics::StartRender.
			clear_colour: [0.0, 0.0, 0.0, 1.0],
			viewport: (0, 0, 640, 480),
			alpha_test: false,
			alpha_ref: 0.0,
			alpha_func: gl::GL_ALWAYS,
			blend: false,
			depth_test: false,
			depth_write: true,
			cull: false,
			front_face: gl::GL_CCW,
			fog: false,
			fog_colour: [0.0, 0.0, 0.0],
			fog_start: 0.0,
			fog_end: 1.0,
			bound_texture: 0,
		}
	}
}

impl State {
	fn stack(&mut self) -> &mut Vec<Mat4> {
		if self.matrix_mode == gl::GL_PROJECTION {
			&mut self.projection
		} else {
			&mut self.modelview
		}
	}

	fn stack_ref(&self) -> &Vec<Mat4> {
		if self.matrix_mode == gl::GL_PROJECTION {
			&self.projection
		} else {
			&self.modelview
		}
	}

	fn current(&self) -> &Mat4 {
		self.stack_ref().last().expect("a stack always has a top")
	}
}

pub struct Gfx {
	device: wgpu::Device,
	queue: wgpu::Queue,

	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
	colour: wgpu::Texture,
	colour_view: wgpu::TextureView,
	depth: wgpu::TextureView,

	surface: Option<wgpu::Surface<'static>>,
	surface_config: Option<wgpu::SurfaceConfiguration>,

	uniform_layout: wgpu::BindGroupLayout,
	texture_layout: wgpu::BindGroupLayout,
	uniform_buffer: wgpu::Buffer,
	uniform_bind_group: wgpu::BindGroup,
	uniform_stride: u32,
	uniform_slots_used: u32,

	vertex_buffer: wgpu::Buffer,
	vertex_capacity: u64,

	pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,

	textures: Vec<Option<TextureEntry>>,
	texture_bind_groups: HashMap<usize, wgpu::BindGroup>,
	/// 1x1 white, bound when a draw has no texture. GL's untextured path multiplies by
	/// white, so this leaves the vertex colour alone. Held as the owner of the bind group
	/// beside it.
	#[allow(dead_code)]
	white: TextureEntry,
	white_bind_group: wgpu::BindGroup,

	state: State,

	vertices: Vec<Vertex>,
	cmds: Vec<Cmd>,
	uniforms: Vec<Uniforms>,
	/// The frame before this one, kept so that the frames between two of the game's own
	/// steps can be drawn halfway between them. See [`Gfx::present_between`].
	previous_uniforms: Vec<Uniforms>,
	/// Whether the frame before this one drew the same things in the same order, which is
	/// what makes the two comparable draw by draw.
	previous_matched: bool,
	/// Texture parameters that arrived before their texture was uploaded, which the
	/// font atlas does by setting the wrap and then uploading.
	pending_parameters: Vec<(usize, Enum, Enum)>,
}

impl Gfx {
	/// Creates the renderer for a window, or a headless one when no window is given.
	pub fn new(
		instance: &wgpu::Instance,
		window: Option<Arc<winit::window::Window>>,
		width: u32,
		height: u32,
		headless_format: Option<wgpu::TextureFormat>,
	) -> Result<Gfx, String> {
		let surface = match window {
			Some(window) => Some(
				instance
					.create_surface(window)
					.map_err(|e| format!("create_surface: {e}"))?,
			),
			None => None,
		};

		let (adapter, device, queue) = pollster::block_on(Self::request(instance, surface.as_ref()))?;
		Self::build(
			instance,
			surface,
			adapter,
			device,
			queue,
			width,
			height,
			headless_format,
		)
	}

	/// Asks for the adapter and the device. The one place the renderer waits.
	async fn request(
		instance: &wgpu::Instance,
		surface: Option<&wgpu::Surface<'_>>,
	) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue), String> {
		let adapter = instance
			.request_adapter(&wgpu::RequestAdapterOptions {
				power_preference: wgpu::PowerPreference::HighPerformance,
				compatible_surface: surface,
				force_fallback_adapter: false,
			})
			.await
			.ok_or_else(|| "no suitable adapter".to_string())?;

		let (device, queue) = adapter
			.request_device(
				&wgpu::DeviceDescriptor {
					label: Some("xops"),
					required_features: wgpu::Features::empty(),
					required_limits: wgpu::Limits::default(),
					// The device must report failures: a dropped draw is otherwise silent,
					// and that costs an afternoon every time.
					memory_hints: wgpu::MemoryHints::default(),
				},
				None,
			)
			.await
			.map_err(|e| format!("request_device: {e}"))?;

		Ok((adapter, device, queue))
	}

	/// Everything the renderer is built out of, once the device exists.
	#[allow(clippy::too_many_arguments)]
	fn build(
		_instance: &wgpu::Instance,
		surface: Option<wgpu::Surface<'static>>,
		adapter: wgpu::Adapter,
		device: wgpu::Device,
		queue: wgpu::Queue,
		width: u32,
		height: u32,
		headless_format: Option<wgpu::TextureFormat>,
	) -> Result<Gfx, String> {
		device.on_uncaptured_error(Box::new(|error| {
			eprintln!("wgpu error: {error}");
		}));

		let format = match (&surface, headless_format) {
			(Some(_), _) => {
				let caps = surface
					.as_ref()
					.expect("checked")
					.get_capabilities(&adapter);
				// Prefer a non-sRGB target: the game writes 8-bit colours that it treats as
				// final, and an sRGB target would apply a transfer curve the original never
				// applied.
				caps.formats
					.iter()
					.copied()
					.find(|f| !f.is_srgb())
					.unwrap_or(caps.formats[0])
			}
			(None, Some(f)) => f,
			(None, None) => wgpu::TextureFormat::Rgba8Unorm,
		};

		let surface_config = surface.as_ref().map(|surface| {
			let caps = surface.get_capabilities(&adapter);

			// The game paces itself: it sleeps out what is left of each 30 ms frame and
			// then draws. A present that also waits for the display puts the two out of
			// step, because 30 ms is not a multiple of a 60 Hz refresh: the frame lands on
			// the next refresh or the one after it, and the difference between the two
			// reads as a stutter rather than a steady rate. So a mode that does not block
			// is preferred, and vsync is what is left when the surface offers nothing else.
			let present_mode = [
				wgpu::PresentMode::Mailbox,
				wgpu::PresentMode::Immediate,
			]
			.into_iter()
			.find(|mode| caps.present_modes.contains(mode))
			.unwrap_or(wgpu::PresentMode::AutoVsync);
			eprintln!("xops: {width}x{height}, presenting with {present_mode:?}");

			let config = wgpu::SurfaceConfiguration {
				usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
				format,
				width,
				height,
				present_mode,
				alpha_mode: caps.alpha_modes[0],
				view_formats: vec![],
				// One frame in flight, not two. This is a game paced at 33 frames a
				// second, so a queued frame is 30 ms of input lag on top of everything
				// else the frame costs: two of them are 60 ms between the mouse moving and
				// the picture showing it, which is the difference between the mouse feeling
				// immediate and feeling dead. It also decides how long `get_current_texture`
				// blocks, which is what the frame spends most of its time on.
				desired_maximum_frame_latency: 1,
			};
			surface.configure(&device, &config);
			config
		});

		let uniform_stride = {
			let align = device.limits().min_uniform_buffer_offset_alignment as u64;
			let size = std::mem::size_of::<Uniforms>() as u64;
			size.div_ceil(align).max(1) * align
		} as u32;

		let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("uniforms"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: true,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
				},
				count: None,
			}],
		});

		let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("texture"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
						view_dimension: wgpu::TextureViewDimension::D2,
						multisampled: false,
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
			],
		});

		let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("uniforms"),
			size: uniform_stride as u64 * UNIFORM_SLOTS as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("uniforms"),
			layout: &uniform_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
					buffer: &uniform_buffer,
					offset: 0,
					size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
				}),
			}],
		});

		let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("vertices"),
			size: VERTEX_BUFFER_BYTES,
			usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let white = create_white_texture(&device, &queue);
		let white_bind_group = create_texture_bind_group(&device, &texture_layout, &white);

		let (colour, colour_view, depth) = create_targets(&device, format, width, height);

		Ok(Gfx {
			device,
			queue,
			width,
			height,
			format,
			colour,
			colour_view,
			depth,
			surface,
			surface_config,
			uniform_layout,
			texture_layout,
			uniform_buffer,
			uniform_bind_group,
			uniform_stride,
			uniform_slots_used: 0,
			vertex_buffer,
			vertex_capacity: VERTEX_BUFFER_BYTES,
			pipelines: HashMap::new(),
			textures: Vec::new(),
			texture_bind_groups: HashMap::new(),
			white,
			white_bind_group,
			state: State::default(),
			vertices: Vec::new(),
			cmds: Vec::new(),
			uniforms: Vec::new(),
			previous_uniforms: Vec::new(),
			previous_matched: false,
			pending_parameters: Vec::new(),
		})
	}

	pub fn size(&self) -> (u32, u32) {
		(self.width, self.height)
	}

	pub fn aspect(&self) -> f32 {
		self.width as f32 / self.height.max(1) as f32
	}

	pub fn format(&self) -> wgpu::TextureFormat {
		self.format
	}

	/// Resizes the render targets and the swap chain. Safe to call with the same size.
	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 || (width == self.width && height == self.height) {
			return;
		}
		self.width = width;
		self.height = height;
		let (colour, view, depth) = create_targets(&self.device, self.format, width, height);
		self.colour = colour;
		self.colour_view = view;
		self.depth = depth;
		if let (Some(surface), Some(config)) = (&self.surface, &mut self.surface_config) {
			config.width = width;
			config.height = height;
			surface.configure(&self.device, config);
		}
		self.state.viewport = (0, 0, width, height);
	}

	// -- frame recording ----------------------------------------------------

	/// Starts a frame. The game's `StartRender` follows with a clear.
	pub fn begin_frame(&mut self) {
		self.vertices.clear();
		self.cmds.clear();
		self.uniforms.clear();
		self.uniform_slots_used = 0;
	}

	pub fn clear_colour(&mut self, colour: [f32; 4]) {
		self.state.clear_colour = colour;
	}

	pub fn clear(&mut self, bits: u32) {
		self.cmds.push(Cmd::Clear(bits));
	}

	pub fn viewport(&mut self, x: u32, y: u32, width: u32, height: u32) {
		self.state.viewport = (x, y, width, height);
	}

	/// Records a draw over the given vertices, using the current transform and state.
	pub fn draw(&mut self, mode: Enum, vertices: &[Vertex]) {
		if vertices.is_empty() {
			return;
		}
		let offset = self.vertices.len() as u32;
		self.vertices.extend_from_slice(vertices);

		if self.uniform_slots_used as usize >= UNIFORM_SLOTS {
			// The frame asked for more draws than the ring holds. Dropping them silently
			// would look like a rendering bug, so say so and stop growing.
			eprintln!("gfx: out of uniform slots, {} draws this frame", self.cmds.len());
			return;
		}

		let slot = self.uniform_slots_used;
		self.uniform_slots_used += 1;
		self.uniforms.push(self.uniforms_for_state());

		let key = PipelineKey {
			topology: PipelineKey::topology(mode),
			front_face: if self.state.front_face == gl::GL_CW {
				wgpu::FrontFace::Cw
			} else {
				wgpu::FrontFace::Ccw
			},
			cull: if self.state.cull {
				Some(wgpu::Face::Back)
			} else {
				None
			},
			blend: self.state.blend,
			alpha_test: self.state.alpha_test,
			depth_test: self.state.depth_test,
			depth_write: self.state.depth_write,
		};

		self.cmds.push(Cmd::Draw {
			vertex_offset: offset,
			vertex_count: vertices.len() as u32,
			uniform_slot: slot,
			texture: self.state.bound_texture,
			key,
		});
	}

	fn uniforms_for_state(&self) -> Uniforms {
		// GL transforms a vertex by projection * modelview, whatever the current matrix
		// mode is, so both tops are read here regardless of which one is being edited.
		let mv = *self.state.modelview.last().expect("a stack always has a top");
		let projection = *self.state.projection.last().expect("a stack always has a top");
		let transform = projection.mul(&mv);

		Uniforms {
			transform: transform.m,
			modelview: mv.m,
			fog: [
				self.state.fog_colour[0],
				self.state.fog_colour[1],
				self.state.fog_colour[2],
				if self.state.fog { 1.0 } else { 0.0 },
			],
			fog_range: [self.state.fog_start, self.state.fog_end, 0.0, 0.0],
			params: [
				if self.state.alpha_test {
					self.state.alpha_ref
				} else {
					0.0
				},
				0.0,
				0.0,
				0.0,
			],
		}
	}

	/// Encodes the recorded frame, uploading vertices and uniforms first.
	pub fn end_frame(&mut self) -> Result<(), String> {
		if !self.vertices.is_empty() {
			let bytes: &[u8] = bytemuck::cast_slice(&self.vertices);
			if bytes.len() as u64 > self.vertex_capacity {
				let needed = (bytes.len() as u64).next_power_of_two();
				self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("vertices"),
					size: needed,
					usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
					mapped_at_creation: false,
				});
				self.vertex_capacity = needed;
			}
			self.queue.write_buffer(&self.vertex_buffer, 0, bytes);

			self.upload_uniforms();
		}

		// The frame that was just drawn becomes the one the next frame is drawn halfway
		// from, unless the two do not line up: a frame that drew a different number of
		// things, or drew them in a different order, has nothing to be interpolated with.
		self.previous_matched = self.previous_uniforms.len() == self.uniforms.len();
		self.previous_uniforms.clone_from(&self.uniforms);

		self.encode()
	}

	/// Uploads the uniforms for every draw, each at its own dynamic-offset slot.
	///
	/// Writing them packed would leave every draw after the first reading a neighbour's
	/// matrices, which looks like geometry flying off screen for no reason.
	fn upload_uniforms(&mut self) {
		if let Some(block) = pack_uniforms(&self.uniforms, self.uniform_stride as usize) {
			self.queue.write_buffer(&self.uniform_buffer, 0, &block);
		}
	}

	/// Draws the frame again, halfway to the one before it, and presents that.
	///
	/// The game simulates on a fixed 30 ms step, which is what its movement, its animations
	/// and its timers are all written for: a frame of movement is a frame's worth of
	/// movement, and running the step faster would run the game faster. That step is not
	/// what a display wants, though. Thirty milliseconds is 33 frames a second, and a 60 Hz
	/// display shows a frame for 16.7 ms, so the two never line up: some frames are shown
	/// for one refresh and some for two, and that unevenness is what a low frame rate looks
	/// like even when the game is keeping up.
	///
	/// So the display gets what it asks for and the game keeps its own rate. The geometry
	/// drawn is the game's own most recent frame, transformed by a blend of the matrices
	/// from the last two steps: at `t` of zero the previous step, at `t` of one the current
	/// one. Nothing is simulated twice and nothing is drawn late.
	///
	/// The world is drawn through those matrices, so a blend places every object and the
	/// camera part of the way through the step's movement. It is a linear blend of two
	/// rigid transforms and so is not itself rigid: halfway between two quite different
	/// rotations it shrinks the result slightly.
	/// ponytail: linear matrix blend, correct to a tenth of a percent per frame here.
	/// If the game ever turns faster per frame than it does now, blend the angles instead.
	pub fn present_between(&mut self, t: f32) -> Result<bool, String> {
		if !self.previous_matched || self.uniforms.is_empty() || t <= 0.0 {
			// Nothing to blend with -- a frame that drew a different number of things -- or
			// nothing to correct: the frame stands as drawn.
			self.encode()?;
			return Ok(false);
		}
		let t = t.clamp(0.0, 1.0);

		let blended = blend_uniforms(&self.previous_uniforms, &self.uniforms, t);

		if let Some(block) = pack_uniforms(&blended, self.uniform_stride as usize) {
			self.queue.write_buffer(&self.uniform_buffer, 0, &block);
		}
		self.encode()?;
		Ok(true)
	}

	fn encode(&mut self) -> Result<(), String> {
		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("frame"),
			});

		// Walk the recorded commands and open a pass wherever a clear appears. That is
		// the whole trick: the game clears the depth buffer mid-frame on purpose, so a
		// clear is a pass boundary and not just another command.
		let mut index = 0;
		let mut pass_started = false;
		while index < self.cmds.len() {
			let clear_bits = match &self.cmds[index] {
				Cmd::Clear(bits) => Some(*bits),
				Cmd::Draw { .. } => None,
			};

			let (clear_colour, clear_depth) = match clear_bits {
				Some(bits) => (
					bits & gl::GL_COLOR_BUFFER_BIT != 0,
					bits & gl::GL_DEPTH_BUFFER_BIT != 0,
				),
				None => (false, false),
			};
			if clear_bits.is_some() {
				index += 1;
			}

			// Gather every draw until the next clear.
			let draws: Vec<usize> = {
				let mut v = Vec::new();
				let mut i = index;
				while i < self.cmds.len() {
					match &self.cmds[i] {
						Cmd::Clear(_) => break,
						Cmd::Draw { .. } => v.push(i),
					}
					i += 1;
				}
				v
			};

			if draws.is_empty() && clear_bits.is_none() {
				index += 1;
				continue;
			}

			let load_colour = if clear_colour {
				wgpu::LoadOp::Clear(wgpu::Color {
					r: self.state.clear_colour[0] as f64,
					g: self.state.clear_colour[1] as f64,
					b: self.state.clear_colour[2] as f64,
					a: self.state.clear_colour[3] as f64,
				})
			} else {
				wgpu::LoadOp::Load
			};
			let load_depth = if clear_depth || !pass_started {
				wgpu::LoadOp::Clear(1.0)
			} else {
				wgpu::LoadOp::Load
			};

			{
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("frame pass"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: &self.colour_view,
						resolve_target: None,
						ops: wgpu::Operations {
							load: load_colour,
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
						view: &self.depth,
						depth_ops: Some(wgpu::Operations {
							load: load_depth,
							store: wgpu::StoreOp::Store,
						}),
						stencil_ops: None,
					}),
					timestamp_writes: None,
					occlusion_query_set: None,
				});

				pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));

				for &cmd_index in &draws {
					let (key, vertex_offset, vertex_count, uniform_slot, texture) =
						match &self.cmds[cmd_index] {
							Cmd::Draw {
								vertex_offset,
								vertex_count,
								uniform_slot,
								texture,
								key,
								..
							} => (*key, *vertex_offset, *vertex_count, *uniform_slot, *texture),
							Cmd::Clear(_) => unreachable!("clears split the pass list"),
						};

					if !self.pipelines.contains_key(&key) {
						let pipeline = self.build_pipeline(key);
						self.pipelines.insert(key, pipeline);
					}
					let pipeline = &self.pipelines[&key];

					pass.set_pipeline(pipeline);
					pass.set_bind_group(0, &self.uniform_bind_group, &[uniform_slot * self.uniform_stride]);
					let texture_bind = self
						.texture_bind_groups
						.get(&texture)
						.unwrap_or(&self.white_bind_group);
					pass.set_bind_group(1, texture_bind, &[]);
					pass.draw(vertex_offset..vertex_offset + vertex_count, 0..1);
				}
			}

			pass_started = true;
			index += draws.len();
		}

		if !pass_started {
			// A frame with no clear and no draws: clear the colour target so the window
			// is not left showing whatever was there.
			let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("empty frame pass"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &self.colour_view,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(wgpu::Color {
							r: self.state.clear_colour[0] as f64,
							g: self.state.clear_colour[1] as f64,
							b: self.state.clear_colour[2] as f64,
							a: self.state.clear_colour[3] as f64,
						}),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
			});
		}

		if let (Some(surface), Some(_config)) = (&self.surface, &self.surface_config) {
			match surface.get_current_texture() {
				Ok(frame) => {
					encoder.copy_texture_to_texture(
						self.colour.as_image_copy(),
						wgpu::TexelCopyTextureInfo {
							texture: &frame.texture,
							mip_level: 0,
							origin: wgpu::Origin3d::ZERO,
							aspect: wgpu::TextureAspect::All,
						},
						wgpu::Extent3d {
							width: self.width,
							height: self.height,
							depth_or_array_layers: 1,
						},
					);
					self.queue.submit(Some(encoder.finish()));
					frame.present();
					return Ok(());
				}
				Err(wgpu::SurfaceError::Outdated) | Err(wgpu::SurfaceError::Lost) => {
					// Reconfigure and skip this frame rather than dying: the window may
					// just have been resized.
					if let (Some(surface), Some(config)) = (&self.surface, &self.surface_config) {
						surface.configure(&self.device, config);
					}
					self.queue.submit(Some(encoder.finish()));
					return Ok(());
				}
				Err(wgpu::SurfaceError::OutOfMemory) => {
					return Err("surface out of memory".into());
				}
				Err(other) => {
					self.queue.submit(Some(encoder.finish()));
					return Err(format!("surface: {other}"));
				}
			}
		}

		self.queue.submit(Some(encoder.finish()));
		Ok(())
	}

	/// Reads the frame target back as RGBA8.
	pub fn read_frame(&self) -> Result<(u32, u32, Vec<u8>), String> {
		let (width, height) = (self.width, self.height);
		let bytes_per_row = padded_row_bytes(width * 4);
		let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("readback"),
			size: (bytes_per_row as u64) * height as u64,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("readback"),
			});
		encoder.copy_texture_to_buffer(
			self.colour.as_image_copy(),
			wgpu::TexelCopyBufferInfo {
				buffer: &buffer,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(bytes_per_row),
					rows_per_image: Some(height),
				},
			},
			wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
		);
		self.queue.submit(Some(encoder.finish()));

		let slice = buffer.slice(..);
		let (tx, rx) = std::sync::mpsc::channel();
		slice.map_async(wgpu::MapMode::Read, move |result| {
			let _ = tx.send(result);
		});
		self.device.poll(wgpu::Maintain::Wait);
		rx.recv()
			.map_err(|e| format!("readback channel: {e}"))?
			.map_err(|e| format!("readback map: {e}"))?;

		let mapped = slice.get_mapped_range();
		let mut pixels = Vec::with_capacity((width * height * 4) as usize);
		for row in 0..height {
			let start = (row * bytes_per_row) as usize;
			pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
		}
		drop(mapped);
		buffer.unmap();

		// The target may be BGRA. Swizzle so callers always get RGBA, which is what the
		// PNG writer and the parity harness both want.
		if self.format == wgpu::TextureFormat::Bgra8Unorm
			|| self.format == wgpu::TextureFormat::Bgra8UnormSrgb
		{
			for pixel in pixels.chunks_exact_mut(4) {
				pixel.swap(0, 2);
			}
		}

		Ok((width, height, pixels))
	}

	// -- transform stack ----------------------------------------------------

	pub fn matrix_mode(&mut self, mode: Enum) {
		self.state.matrix_mode = mode;
	}

	pub fn load_identity(&mut self) {
		*self.state.stack().last_mut().expect("non-empty") = Mat4::IDENTITY;
	}

	pub fn push_matrix(&mut self) {
		let top = *self.state.current();
		self.state.stack().push(top);
	}

	pub fn pop_matrix(&mut self) {
		let stack = self.state.stack();
		if stack.len() > 1 {
			stack.pop();
		}
	}

	pub fn multiply(&mut self, matrix: &Mat4) {
		self.state
			.stack()
			.last_mut()
			.expect("non-empty")
			.post_mul(matrix);
	}

	pub fn translate(&mut self, x: f32, y: f32, z: f32) {
		self.multiply(&Mat4::translation(x, y, z));
	}

	pub fn rotate(&mut self, angle: f32, x: f32, y: f32, z: f32) {
		let len = (x * x + y * y + z * z).sqrt();
		if len == 0.0 {
			return;
		}
		// glRotatef rotates about the given axis by the given angle in degrees.
		let m = {
			let (x, y, z) = (x / len, y / len, z / len);
			let radians = angle.to_radians();
			let (s, c) = radians.sin_cos();
			let t = 1.0 - c;
			Mat4 {
				m: [
					x * x * t + c,
					y * x * t + z * s,
					z * x * t - y * s,
					0.0,
					x * y * t - z * s,
					y * y * t + c,
					z * y * t + x * s,
					0.0,
					x * z * t + y * s,
					y * z * t - x * s,
					z * z * t + c,
					0.0,
					0.0,
					0.0,
					0.0,
					1.0,
				],
			}
		};
		self.multiply(&m);
	}

	pub fn scale(&mut self, x: f32, y: f32, z: f32) {
		self.multiply(&Mat4::scaling(x, y, z));
	}

	pub fn perspective(&mut self, fovy_degrees: f32, aspect: f32, near: f32, far: f32) {
		self.multiply(&Mat4::perspective(fovy_degrees, aspect, near, far));
	}

	pub fn ortho(
		&mut self,
		left: f32,
		right: f32,
		bottom: f32,
		top: f32,
		near: f32,
		far: f32,
	) {
		let mut m = Mat4::IDENTITY;
		m.m[0] = 2.0 / (right - left);
		m.m[5] = 2.0 / (top - bottom);
		m.m[10] = -2.0 / (far - near);
		m.m[12] = -(right + left) / (right - left);
		m.m[13] = -(top + bottom) / (top - bottom);
		m.m[14] = -(far + near) / (far - near);
		self.multiply(&m);
	}

	pub fn mult_look_at(&mut self, eye: Vec3, center: Vec3, up: Vec3) {
		let mut m = *self.state.current();
		m.mult_look_at(eye, center, up);
		*self.state.stack().last_mut().expect("non-empty") = m;
	}

	/// `glGetFloatv` for the two matrix targets the game reads.
	pub fn get_matrix(&self, pname: Enum) -> Option<[f32; 16]> {
		match pname {
			gl::GL_MODELVIEW_MATRIX => Some(self.state.modelview.last()?.m),
			gl::GL_PROJECTION_MATRIX => Some(self.state.projection.last()?.m),
			_ => None,
		}
	}

	pub fn get_viewport(&self) -> (u32, u32, u32, u32) {
		self.state.viewport
	}

	// -- render state -------------------------------------------------------

	pub fn enable(&mut self, cap: Enum) {
		match cap {
			gl::GL_ALPHA_TEST => self.state.alpha_test = true,
			gl::GL_BLEND => self.state.blend = true,
			gl::GL_CULL_FACE => self.state.cull = true,
			gl::GL_DEPTH_TEST => self.state.depth_test = true,
			gl::GL_FOG => self.state.fog = true,
			_ => {}
		}
	}

	pub fn disable(&mut self, cap: Enum) {
		match cap {
			gl::GL_ALPHA_TEST => self.state.alpha_test = false,
			gl::GL_BLEND => self.state.blend = false,
			gl::GL_CULL_FACE => self.state.cull = false,
			gl::GL_DEPTH_TEST => self.state.depth_test = false,
			gl::GL_FOG => self.state.fog = false,
			_ => {}
		}
	}

	pub fn depth_mask(&mut self, write: bool) {
		self.state.depth_write = write;
	}

	pub fn front_face(&mut self, winding: Enum) {
		self.state.front_face = winding;
	}

	/// The alpha test, which the game uses to cut out foliage and wire fences. A reference
	/// of zero with a greater-than comparison is how it discards fully transparent texels.
	pub fn alpha_test(&mut self, func: Enum, reference: f32) {
		self.state.alpha_func = func;
		self.state.alpha_ref = reference;
	}

	/// Whether each capability is on. The game and the renderer's own tests both ask.
	pub fn depth_test_enabled(&self) -> bool {
		self.state.depth_test
	}

	pub fn cull_enabled(&self) -> bool {
		self.state.cull
	}

	pub fn blend_enabled(&self) -> bool {
		self.state.blend
	}

	pub fn alpha_test_enabled(&self) -> bool {
		self.state.alpha_test
	}

	/// The game switches fog off for the 2D and HUD passes, and back on for the world.
	pub fn set_fog(&mut self, enabled: bool, colour: [f32; 3], start: f32, end: f32) {
		self.state.fog = enabled;
		self.state.fog_colour = colour;
		self.state.fog_start = start;
		self.state.fog_end = end;
	}

	pub fn fog_enabled(&self) -> bool {
		self.state.fog
	}

	pub fn fog_colour(&self) -> [f32; 3] {
		self.state.fog_colour
	}

	pub fn fog_range(&self) -> (f32, f32) {
		(self.state.fog_start, self.state.fog_end)
	}

	// -- textures -----------------------------------------------------------

	pub fn gen_texture(&mut self) -> usize {
		let id = self.textures.len();
		self.textures.push(None);
		id
	}

	/// Uploads decoded RGBA8 pixels into a slot, creating it on first use.
	pub fn upload_texture(&mut self, id: usize, width: u32, height: u32, rgba: &[u8]) {
		let expected = (width * height * 4) as usize;
		if rgba.len() < expected {
			eprintln!(
				"gfx: texture {id} wants {expected} bytes of pixels, got {}",
				rgba.len()
			);
			return;
		}

		let size = wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		};
		let texture = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("texture"),
			size,
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		});
		self.queue.write_texture(
			texture.as_image_copy(),
			rgba,
			wgpu::TexelCopyBufferLayout {
				offset: 0,
				bytes_per_row: Some(width * 4),
				rows_per_image: Some(height),
			},
			size,
		);

		// A parameter set can arrive before the upload, so fold those in and remember the
		// mode the texture actually ends up with.
		let mut wrap_s = self
			.textures
			.get(id)
			.and_then(|entry| entry.as_ref())
			.map(|entry| entry.wrap_s)
			.unwrap_or(gl::GL_REPEAT);
		let mut wrap_t = self
			.textures
			.get(id)
			.and_then(|entry| entry.as_ref())
			.map(|entry| entry.wrap_t)
			.unwrap_or(gl::GL_REPEAT);
		let mut min_filter = self
			.textures
			.get(id)
			.and_then(|entry| entry.as_ref())
			.map(|entry| entry.min_filter)
			.unwrap_or(gl::GL_NEAREST);
		let mut mag_filter = self
			.textures
			.get(id)
			.and_then(|entry| entry.as_ref())
			.map(|entry| entry.mag_filter)
			.unwrap_or(gl::GL_NEAREST);
		for (pending_id, pname, value) in &self.pending_parameters {
			if *pending_id != id {
				continue;
			}
			match *pname {
				gl::GL_TEXTURE_WRAP_S => wrap_s = *value,
				gl::GL_TEXTURE_WRAP_T => wrap_t = *value,
				gl::GL_TEXTURE_MIN_FILTER => min_filter = *value,
				gl::GL_TEXTURE_MAG_FILTER => mag_filter = *value,
				_ => {}
			}
		}
		self.pending_parameters.retain(|(pending_id, _, _)| *pending_id != id);

		let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
		let sampler = make_sampler(&self.device, wrap_s, wrap_t, min_filter, mag_filter);
		let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("texture"),
			layout: &self.texture_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&sampler),
				},
			],
		});

		if id >= self.textures.len() {
			self.textures.resize_with(id + 1, || None);
		}
		self.textures[id] = Some(TextureEntry {
			texture,
			view,
			sampler,
			width,
			height,
			wrap_s,
			wrap_t,
			min_filter,
			mag_filter,
		});
		self.texture_bind_groups.insert(id, bind_group);
	}

	/// `glTexParameteri`. The sampler is rebuilt straight away when the texture exists,
	/// so a wrap mode set after the upload still applies to later draws.
	pub fn tex_parameter(&mut self, target: Enum, pname: Enum, value: Enum) {
		if target != gl::GL_TEXTURE_2D {
			return;
		}
		let id = self.state.bound_texture;
		if !matches!(
			pname,
			gl::GL_TEXTURE_WRAP_S
				| gl::GL_TEXTURE_WRAP_T
				| gl::GL_TEXTURE_MIN_FILTER
				| gl::GL_TEXTURE_MAG_FILTER
		) {
			return;
		}

		let Some(Some(entry)) = self.textures.get_mut(id) else {
			// Not uploaded yet, so the value has nowhere to go. Keep it for the upload.
			self.pending_parameters.push((id, pname, value));
			return;
		};

		match pname {
			gl::GL_TEXTURE_WRAP_S => entry.wrap_s = value,
			gl::GL_TEXTURE_WRAP_T => entry.wrap_t = value,
			gl::GL_TEXTURE_MIN_FILTER => entry.min_filter = value,
			_ => entry.mag_filter = value,
		}
		entry.sampler = make_sampler(
			&self.device,
			entry.wrap_s,
			entry.wrap_t,
			entry.min_filter,
			entry.mag_filter,
		);
		let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("texture"),
			layout: &self.texture_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&entry.view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&entry.sampler),
				},
			],
		});
		self.texture_bind_groups.insert(id, bind_group);
	}

	pub fn bind_texture(&mut self, id: usize) {
		self.state.bound_texture = id;
	}

	pub fn delete_texture(&mut self, id: usize) {
		if let Some(entry) = self.textures.get_mut(id) {
			*entry = None;
		}
		self.texture_bind_groups.remove(&id);
	}

	pub fn texture_size(&self, id: usize) -> Option<(u32, u32)> {
		self.textures
			.get(id)
			.and_then(|entry| entry.as_ref())
			.map(|entry| (entry.width, entry.height))
	}
}

fn make_sampler(
	device: &wgpu::Device,
	wrap_s: Enum,
	wrap_t: Enum,
	min_filter: Enum,
	mag_filter: Enum,
) -> wgpu::Sampler {
	let address = |wrap: Enum| match wrap {
		gl::GL_CLAMP_TO_EDGE | gl::GL_CLAMP => wgpu::AddressMode::ClampToEdge,
		_ => wgpu::AddressMode::Repeat,
	};
	let filter = |f: Enum| match f {
		gl::GL_LINEAR | gl::GL_LINEAR_MIPMAP_NEAREST | gl::GL_LINEAR_MIPMAP_LINEAR => {
			wgpu::FilterMode::Linear
		}
		_ => wgpu::FilterMode::Nearest,
	};
	device.create_sampler(&wgpu::SamplerDescriptor {
		label: Some("texture"),
		address_mode_u: address(wrap_s),
		address_mode_v: address(wrap_t),
		address_mode_w: wgpu::AddressMode::ClampToEdge,
		mag_filter: filter(mag_filter),
		min_filter: filter(min_filter),
		mipmap_filter: wgpu::FilterMode::Nearest,
		..Default::default()
	})
}

impl Gfx {
	/// Builds the pipeline one draw needs. The combinations actually used are few, so
	/// they are built on demand and cached rather than enumerated up front.
	fn build_pipeline(&self, key: PipelineKey) -> wgpu::RenderPipeline {
		let shader = self
			.device
			.create_shader_module(wgpu::ShaderModuleDescriptor {
				label: Some("render"),
				source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/render.wgsl"))),
			});

		let layout = self
			.device
			.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("render"),
				bind_group_layouts: &[&self.uniform_layout, &self.texture_layout],
				push_constant_ranges: &[],
			});

		let attributes = [
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Float32x3,
				offset: 0,
				shader_location: 0,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Float32x2,
				offset: 12,
				shader_location: 1,
			},
			wgpu::VertexAttribute {
				format: wgpu::VertexFormat::Float32x4,
				offset: 20,
				shader_location: 2,
			},
		];

		let blend = if key.blend {
			Some(wgpu::BlendState {
				color: wgpu::BlendComponent {
					src_factor: wgpu::BlendFactor::SrcAlpha,
					dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
					operation: wgpu::BlendOperation::Add,
				},
				alpha: wgpu::BlendComponent {
					src_factor: wgpu::BlendFactor::One,
					dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
					operation: wgpu::BlendOperation::Add,
				},
			})
		} else {
			None
		};

		self.device
			.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("render"),
				layout: Some(&layout),
				vertex: wgpu::VertexState {
					module: &shader,
					entry_point: Some("vs_main"),
					compilation_options: wgpu::PipelineCompilationOptions::default(),
					buffers: &[wgpu::VertexBufferLayout {
						array_stride: (VERTEX_FLOATS * 4) as u64,
						step_mode: wgpu::VertexStepMode::Vertex,
						attributes: &attributes,
					}],
				},
				primitive: wgpu::PrimitiveState {
					topology: key.topology,
					strip_index_format: None,
					front_face: key.front_face,
					// GL culls whichever winding is not the front face; WebGPU names the
					// face to discard, and the game only ever culls the back.
					cull_mode: key.cull,
					unclipped_depth: false,
					polygon_mode: wgpu::PolygonMode::Fill,
					conservative: false,
				},
				depth_stencil: Some(wgpu::DepthStencilState {
					format: wgpu::TextureFormat::Depth24Plus,
					depth_write_enabled: key.depth_write,
					depth_compare: if key.depth_test {
						wgpu::CompareFunction::Less
					} else {
						wgpu::CompareFunction::Always
					},
					stencil: wgpu::StencilState::default(),
					bias: wgpu::DepthBiasState::default(),
				}),
				multisample: wgpu::MultisampleState::default(),
				fragment: Some(wgpu::FragmentState {
					module: &shader,
					entry_point: Some("fs_main"),
					compilation_options: wgpu::PipelineCompilationOptions::default(),
					targets: &[Some(wgpu::ColorTargetState {
						format: self.format,
						blend,
						write_mask: wgpu::ColorWrites::ALL,
					})],
				}),
				multiview: None,
				cache: None,
			})
	}
}

fn padded_row_bytes(row: u32) -> u32 {
	// copy_texture_to_buffer wants the row stride aligned to 256 bytes.
	row.div_ceil(256) * 256
}

fn create_targets(
	device: &wgpu::Device,
	format: wgpu::TextureFormat,
	width: u32,
	height: u32,
) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView) {
	let size = wgpu::Extent3d {
		width: width.max(1),
		height: height.max(1),
		depth_or_array_layers: 1,
	};
	let colour = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("frame colour"),
		size,
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT
			| wgpu::TextureUsages::COPY_SRC
			| wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let depth = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("frame depth"),
		size,
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Depth24Plus,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		view_formats: &[],
	});
	let colour_view = colour.create_view(&wgpu::TextureViewDescriptor::default());
	let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
	(colour, colour_view, depth_view)
}

/// The 1x1 white texture an untextured draw binds.
fn create_white_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> TextureEntry {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("white"),
		size: wgpu::Extent3d {
			width: 1,
			height: 1,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba8Unorm,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	queue.write_texture(
		texture.as_image_copy(),
		&[255, 255, 255, 255],
		wgpu::TexelCopyBufferLayout {
			offset: 0,
			bytes_per_row: Some(4),
			rows_per_image: Some(1),
		},
		wgpu::Extent3d {
			width: 1,
			height: 1,
			depth_or_array_layers: 1,
		},
	);
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
		label: Some("white"),
		address_mode_u: wgpu::AddressMode::ClampToEdge,
		address_mode_v: wgpu::AddressMode::ClampToEdge,
		address_mode_w: wgpu::AddressMode::ClampToEdge,
		mag_filter: wgpu::FilterMode::Nearest,
		min_filter: wgpu::FilterMode::Nearest,
		..Default::default()
	});
	TextureEntry {
		texture,
		view,
		sampler,
		width: 1,
		height: 1,
		wrap_s: gl::GL_CLAMP_TO_EDGE,
		wrap_t: gl::GL_CLAMP_TO_EDGE,
		min_filter: gl::GL_NEAREST,
		mag_filter: gl::GL_NEAREST,
	}
}

fn create_texture_bind_group(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	entry: &TextureEntry,
) -> wgpu::BindGroup {
	device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("texture"),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::TextureView(&entry.view),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::Sampler(&entry.sampler),
			},
		],
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_blend_of_two_frames_is_each_of_them_at_its_ends() {
		// What the display is shown between two of the game's steps. At the ends it has to
		// be the frames themselves, or the picture would be wrong at the moment the game's
		// own frame is drawn; in between it is halfway, which is where an object that moved
		// between the two frames is drawn.
		let mut before = Uniforms::zeroed();
		let mut now = Uniforms::zeroed();
		for i in 0..16 {
			before.transform[i] = i as f32;
			before.modelview[i] = i as f32 * 2.0;
			now.transform[i] = i as f32 + 10.0;
			now.modelview[i] = i as f32 * 2.0 + 20.0;
		}

		let at_zero = blend_uniforms(&[before], &[now], 0.0);
		assert_eq!(at_zero[0], before, "t of nought is the frame before");
		let at_one = blend_uniforms(&[before], &[now], 1.0);
		assert_eq!(at_one[0], now, "t of one is the frame now");

		let half = blend_uniforms(&[before], &[now], 0.5);
		assert_eq!(half[0].transform[3], before.transform[3] + 5.0);
		assert_eq!(half[0].modelview[3], before.modelview[3] + 10.0);

		// The renderer's own colour and fog settings are the current frame's: only the two
		// matrices are a blend.
		assert_eq!(half[0].fog, now.fog);
	}

	#[test]
	fn vertex_matches_the_shader_attributes() {
		assert_eq!(std::mem::size_of::<Vertex>(), VERTEX_FLOATS * 4);
		// Two 16-float matrices and three vec4s, which is the 176-byte block the browser
		// port's recorder writes and the parity harness compares against.
		assert_eq!(std::mem::size_of::<Uniforms>(), 16 * 4 * 2 + 4 * 4 * 3);
		assert_eq!(std::mem::size_of::<Uniforms>(), 176);
	}

	#[test]
	fn gl_modes_map_to_the_nearest_topology() {
		assert_eq!(
			PipelineKey::topology(gl::GL_TRIANGLE_STRIP),
			wgpu::PrimitiveTopology::TriangleStrip
		);
		assert_eq!(
			PipelineKey::topology(gl::GL_TRIANGLES),
			wgpu::PrimitiveTopology::TriangleList
		);
		assert_eq!(
			PipelineKey::topology(gl::GL_LINE_STRIP),
			wgpu::PrimitiveTopology::LineStrip
		);
		assert_eq!(
			PipelineKey::topology(gl::GL_POINTS),
			wgpu::PrimitiveTopology::PointList
		);
		// WebGPU has no quads, so those take the triangle list the upstream path uses.
		assert_eq!(
			PipelineKey::topology(gl::GL_QUADS),
			wgpu::PrimitiveTopology::TriangleList
		);
	}

	#[test]
	fn readback_rows_are_aligned_for_the_copy() {
		assert_eq!(padded_row_bytes(640 * 4), 2560);
		assert_eq!(padded_row_bytes(100 * 4), 512);
		assert_eq!(padded_row_bytes(1), 256);
	}
}
