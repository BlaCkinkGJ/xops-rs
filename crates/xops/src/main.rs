//! The standalone game.
//!
//! Two ways to run it: a window, or a headless pass that renders frames to PNG. The
//! second exists because the pixel comparison against the browser build needs frames as
//! files, and because a render difference is easier to find when it is not moving.
//!
//! The game data is not redistributable and is not in this repository, so the mission is
//! named on the command line. The paths inside a mission are relative to the game's own
//! directory, which is why `--dir` exists: run against a data folder rather than the
//! repository root.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use xops_game::core::degree_to_radian;
use xops_game::datafile::{
	get_file_directory, path_from_bytes, BlockDataInterface, MIFInterface, PointDataInterface,
	POINT_P1TYPE_HUMAN,
};
use xops_game::d3dgraphics::D3DGraphics;
use xops_platform::gfx::Gfx;
use xops_platform::input::Input;
use xops_platform::window::{self, App};
use xops_platform::winit::event::WindowEvent;
use xops_platform::winit::window::Window;

/// `VIEW_HEIGHT` from the upstream `gamemain.h`: the eye sits this far above the feet.
/// It belongs in the game's main module once that lands.
const VIEW_HEIGHT: f32 = 19.0;
/// `VIEWANGLE_NORMAL` from the upstream `gamemain.h`, the standard field of view.
const VIEWANGLE_NORMAL: f32 = 65.0;

/// What the camera needs, read out of the mission when it loads.
struct Level {
	camera_x: f32,
	camera_y: f32,
	camera_z: f32,
	camera_rx: f32,
	sky: i32,
	wireframe: bool,
}

/// The game's own frame. The upstream `GameMain` drives this once it is ported.
struct Shell {
	graphics: Option<D3DGraphics>,
	level: Option<Level>,
	mission: Option<PathBuf>,
	wireframe: bool,
	frames: u64,
}

impl Shell {
	fn new(mission: Option<PathBuf>, wireframe: bool) -> Shell {
		Shell {
			graphics: None,
			level: None,
			mission,
			wireframe,
			frames: 0,
		}
	}

	fn graphics(&mut self) -> &mut D3DGraphics {
		self.graphics
			.as_mut()
			.expect("the host calls init before it calls frame")
	}

	/// Loads a mission and its map. Everything the block reader needs comes out of the
	/// `.mif`, and the block textures are named relative to the map's own directory.
	fn load_mission(&mut self, path: &Path) -> Result<(), String> {
		let name = path.to_string_lossy().into_owned();

		let mut mission = MIFInterface::new();
		if mission.load_file_data(name.as_bytes(), false) != 0 {
			return Err(format!("{name}: the mission did not load"));
		}

		let (blockfile, pointfile) = mission.get_datafile_path();
		let mut blocks = BlockDataInterface::new();
		if blocks.load_file_data(blockfile) != 0 {
			return Err(format!(
				"{}: the map did not load",
				path_from_bytes(blockfile).display()
			));
		}
		// The shading is baked into the block data and has to run before anything draws.
		blocks.calculation_blockdata(false);

		let mut directory = [0u8; 260];
		get_file_directory(blockfile, &mut directory);

		// The player's start: the first point that places a human.
		let mut points = PointDataInterface::new();
		let loaded_points = points.load_file_data(pointfile) == 0;
		let spawn = if loaded_points {
			points
				.search_pointdata(0x01, POINT_P1TYPE_HUMAN, 0, 0, 0, 0)
				.1
		} else {
			None
		};

		let (feet_x, feet_y, feet_z, rx) = match spawn {
			Some(point) => (point.x, point.y, point.z, point.r),
			// A map with no spawn point still renders, from the middle of it.
			None => (0.0, 0.0, 0.0, 0.0),
		};

		let sky = mission.get_skynumber();
		self.graphics().load_mapdata(blocks, &directory);
		self.level = Some(Level {
			camera_x: feet_x,
			camera_y: feet_y + VIEW_HEIGHT,
			camera_z: feet_z,
			camera_rx: rx,
			sky,
			wireframe: self.wireframe,
		});

		let (width, height) = self.graphics().gfx_ref().size();
		eprintln!(
			"xops: {name}, map {} ({}x{}), sky {sky}, camera at ({feet_x:.1}, {:.1}, {feet_z:.1})",
			path_from_bytes(blockfile).display(),
			width,
			height,
			feet_y + VIEW_HEIGHT
		);
		Ok(())
	}

	/// One frame of the renderer, which is what the game's own loop will do around it.
	fn draw(&mut self) {
		let level = self.level.take();
		let graphics = self.graphics();

		graphics.start_render();
		if let Some(level) = &level {
			graphics.set_fog(true, level.sky);
			graphics.set_camera(
				level.camera_x,
				level.camera_y,
				level.camera_z,
				level.camera_rx,
				0.0,
				degree_to_radian(VIEWANGLE_NORMAL),
			);
			graphics.render_mapdata(level.wireframe);
			graphics.set_fog(false, 0);
		}
		graphics.end_render();
		self.level = level;
		self.frames += 1;
	}
}

impl App for Shell {
	fn init(&mut self, window: &Arc<Window>, gfx: Gfx) {
		let _ = window.set_title("xops");
		let (width, height) = gfx.size();
		eprintln!("xops: {width}x{height}");
		self.graphics = Some(D3DGraphics::new(gfx, false, 0));

		if let Some(path) = self.mission.clone() {
			if let Err(error) = self.load_mission(&path) {
				eprintln!("xops: {error}");
			}
		}
	}

	fn frame(&mut self, _input: &mut Input) -> bool {
		self.draw();
		true
	}

	fn resized(&mut self, width: u32, height: u32) {
		if let Some(graphics) = &mut self.graphics {
			graphics.set_size(width as i32, height as i32);
		}
	}

	fn event(&mut self, _event: &WindowEvent) -> bool {
		true
	}
}

impl Drop for Shell {
	fn drop(&mut self) {
		if self.frames > 0 {
			eprintln!("xops: {} frames", self.frames);
		}
	}
}

struct Options {
	/// Render this many frames headlessly and write them out, instead of opening a window.
	frames: Option<u64>,
	out: Option<PathBuf>,
	width: u32,
	height: u32,
	/// A `.mif` to load. The paths inside it are relative to the game's own directory.
	mission: Option<PathBuf>,
	/// Where the game's data lives. The mission paths are relative to it, so this is the
	/// directory the binary runs against.
	dir: Option<PathBuf>,
	wireframe: bool,
	help: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			frames: None,
			out: None,
			width: 640,
			height: 480,
			mission: None,
			dir: None,
			wireframe: false,
			help: false,
		}
	}
}

fn parse_args(args: &[String]) -> Result<Options, String> {
	let mut options = Options::default();
	let mut index = 0;
	while index < args.len() {
		match args[index].as_str() {
			"--frames" => {
				index += 1;
				let value = args.get(index).ok_or("--frames needs a count")?;
				options.frames = Some(value.parse().map_err(|_| "--frames needs a number")?);
			}
			"--out" => {
				index += 1;
				let value = args.get(index).ok_or("--out needs a directory")?;
				options.out = Some(PathBuf::from(value));
			}
			"--size" => {
				index += 1;
				let value = args.get(index).ok_or("--size needs WxH")?;
				let (w, h) = value.split_once('x').ok_or("--size needs WxH")?;
				options.width = w.parse().map_err(|_| "--size width")?;
				options.height = h.parse().map_err(|_| "--size height")?;
			}
			"--mission" => {
				index += 1;
				let value = args.get(index).ok_or("--mission needs a path")?;
				options.mission = Some(PathBuf::from(value));
			}
			"--dir" => {
				index += 1;
				let value = args.get(index).ok_or("--dir needs a path")?;
				options.dir = Some(PathBuf::from(value));
			}
			"--wireframe" => options.wireframe = true,
			"--help" | "-h" => options.help = true,
			other => return Err(format!("unknown argument: {other}")),
		}
		index += 1;
	}
	if options.frames.is_some() && options.out.is_none() {
		return Err("--frames needs --out to write the frames somewhere".into());
	}
	if options.width == 0 || options.height == 0 {
		return Err("--size must be positive".into());
	}
	Ok(options)
}

fn usage() -> &'static str {
	"xops [options]

  --mission PATH  a .mif to load and render. Its paths are relative to --dir.
  --dir PATH      the game's data directory, which the mission paths are relative to
  --wireframe     draw the map as a wireframe
  --frames N      render N frames offscreen and write them to --out, then exit
  --out DIR       where those frames go
  --size WxH      frame size (default 640x480)
  -h, --help      this text

With no --frames, opens a window and runs the frame loop."
}

fn main() -> ExitCode {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let options = match parse_args(&args) {
		Ok(options) => options,
		Err(error) => {
			eprintln!("xops: {error}");
			eprintln!("{}", usage());
			return ExitCode::FAILURE;
		}
	};
	if options.help {
		println!("{}", usage());
		return ExitCode::SUCCESS;
	}

	// The mission's own paths are relative to the game directory, so the game directory
	// has to be the working directory before anything loads.
	if let Some(dir) = &options.dir {
		if let Err(error) = std::env::set_current_dir(dir) {
			eprintln!("xops: {}: {error}", dir.display());
			return ExitCode::FAILURE;
		}
	}

	let mission = options.mission.clone();
	let result = match options.frames {
		Some(frames) => headless(&options, frames),
		None => window::run(Shell::new(mission, options.wireframe)),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("xops: {error}");
			ExitCode::FAILURE
		}
	}
}

fn headless(options: &Options, frames: u64) -> Result<(), String> {
	let out = options.out.as_ref().expect("checked in parse_args");
	let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
		backends: wgpu::Backends::all(),
		..Default::default()
	});
	let gfx = Gfx::new(&instance, None, options.width, options.height, None)?;
	let mut shell = Shell::new(options.mission.clone(), options.wireframe);
	shell.graphics = Some(D3DGraphics::new(gfx, false, 0));
	if let Some(path) = options.mission.clone() {
		shell.load_mission(&path)?;
	}

	let mut rendered = Vec::with_capacity(frames as usize);
	for _ in 0..frames {
		shell.draw();
		rendered.push(shell.graphics().gfx_ref().read_frame()?);
	}

	let (width, height) = shell.graphics().gfx_ref().size();
	let pixels: Vec<Vec<u8>> = rendered.into_iter().map(|(_, _, p)| p).collect();
	xops_platform::frame::write_frame_sequence(out, "frame", width, height, &pixels)?;
	println!(
		"wrote {} frames of {width}x{height} to {}",
		pixels.len(),
		out.display()
	);
	Ok(())
}
