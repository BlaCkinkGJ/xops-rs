//! The standalone game.
//!
//! Two ways to run it: a window, or a headless pass that renders frames to PNG. The
//! second exists because the pixel comparison against the browser build needs frames as
//! files, and because a render difference is easier to find when it is not moving.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use xops_game::d3dgraphics::D3DGraphics;
use xops_platform::gfx::Gfx;
use xops_platform::input::Input;
use xops_platform::window::{self, App};
use xops_platform::winit::event::WindowEvent;
use xops_platform::winit::window::Window;

/// The game's own frame. The upstream `GameMain` drives this once it is ported; for now
/// the renderer class starts and ends a frame and the camera points down the level.
struct Shell {
	graphics: Option<D3DGraphics>,
	frames: u64,
}

impl Shell {
	fn new() -> Shell {
		Shell {
			graphics: None,
			frames: 0,
		}
	}

	fn graphics(&mut self) -> &mut D3DGraphics {
		self.graphics
			.as_mut()
			.expect("the host calls init before it calls frame")
	}

	/// One frame of the renderer, which is what the game's own loop will do around it.
	fn draw(&mut self) {
		let graphics = self.graphics();
		graphics.start_render();
		// The default field of view the game uses, in radians, and a camera at head
		// height looking down the level. Replaced by the mission's spawn point.
		graphics.set_camera(
			0.0,
			10.0,
			0.0,
			0.0,
			0.0,
			xops_game::core::degree_to_radian(70.0),
		);
		graphics.end_render();
		self.frames += 1;
	}
}

impl App for Shell {
	fn init(&mut self, window: &Arc<Window>, gfx: Gfx) {
		let _ = window.set_title("xops");
		let (width, height) = gfx.size();
		eprintln!("xops: {width}x{height}");
		self.graphics = Some(D3DGraphics::new(gfx, false, 0));
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

#[derive(Debug)]
struct Options {
	/// Render this many frames headlessly and write them out, instead of opening a window.
	frames: Option<u64>,
	out: Option<PathBuf>,
	width: u32,
	height: u32,
	help: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			frames: None,
			out: None,
			width: 640,
			height: 480,
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

  --frames N     render N frames offscreen and write them to --out, then exit
  --out DIR      where those frames go
  --size WxH     frame size (default 640x480)
  -h, --help     this text

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

	let result = match options.frames {
		Some(frames) => headless(&options, frames),
		None => window::run(Shell::new()),
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
	let mut shell = Shell {
		graphics: Some(D3DGraphics::new(gfx, false, 0)),
		frames: 0,
	};

	let mut rendered = Vec::with_capacity(frames as usize);
	for _ in 0..frames {
		shell.draw();
		let (width, height, pixels) = shell
			.graphics()
			.gfx_ref()
			.read_frame()?;
		rendered.push((width, height, pixels));
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
