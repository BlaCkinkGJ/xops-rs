//! The standalone game.
//!
//! Two ways to run it: a window, or a headless pass that renders frames to PNG. The
//! second exists because the pixel comparison against the browser build needs frames as
//! files, and because a render difference is easier to find when it is not moving.

use std::path::PathBuf;
use std::process::ExitCode;

use xops_platform::frame;
use xops_platform::gfx::Gfx;
use xops_platform::input::Input;
use xops_platform::window::{self, App};
use xops_platform::winit::event::WindowEvent;
use xops_platform::winit::window::Window;

/// The game's own frame, which the upstream `GameMain` will drive once it is ported.
struct Shell {
	frames: u64,
}

impl Shell {
	fn new() -> Shell {
		Shell { frames: 0 }
	}
}

impl App for Shell {
	fn init(&mut self, window: &std::sync::Arc<Window>, gfx: &mut Gfx, _input: &mut Input) {
		let _ = window.set_title("xops");
		let (width, height) = gfx.size();
		eprintln!("xops: {width}x{height}");
	}

	fn frame(&mut self, gfx: &mut Gfx, _input: &mut Input) -> bool {
		gfx.begin_frame();
		gfx.clear(xops_platform::gl::GL_COLOR_BUFFER_BIT | xops_platform::gl::GL_DEPTH_BUFFER_BIT);
		self.frames += 1;
		true
	}

	fn resized(&mut self, _gfx: &mut Gfx, _width: u32, _height: u32) {}

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
	let mut gfx = Gfx::new(&instance, None, options.width, options.height, None)?;
	let mut app = Shell::new();
	let mut input = Input::default();

	let mut rendered = Vec::with_capacity(frames as usize);
	for _ in 0..frames {
		if !app.frame(&mut gfx, &mut input) {
			break;
		}
		input.end_frame();
		gfx.end_frame()?;
		let (width, height, pixels) = gfx.read_frame()?;
		rendered.push((width, height, pixels));
	}

	let (width, height) = gfx.size();
	let pixels: Vec<Vec<u8>> = rendered.into_iter().map(|(_, _, p)| p).collect();
	frame::write_frame_sequence(out, "frame", width, height, &pixels)?;
	println!("wrote {} frames of {width}x{height} to {}", pixels.len(), out.display());
	Ok(())
}
