//! The standalone game: the opening screen, the menu, a mission, the result.
//!
//! The data is not redistributable and is not in this repository, so `--dir` names the
//! directory the game's own paths are relative to. The game changes into it before
//! anything loads, the way the original's working directory is its own folder.
//!
//! Two modes, both the original's. With no arguments the game runs from the opening
//! screen. With `--mission` it starts straight into that mission, which is the upstream
//! `ENABLE_AUTOLOADMIF` path and the mode a comparison run uses, since it skips the menu
//! that would otherwise have to be clicked through.
//!
//! `--frames` renders offscreen instead of opening a window. That exists because the
//! pixel comparison against the reference build wants frames as files, and because a
//! render difference is easier to find when nothing is moving.

use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::process::ExitCode;
use std::sync::Arc;

use xops_game::config::Config;
use xops_game::core::{GAMENAME, GAMEVERSION};
use xops_game::d3dgraphics::D3DGraphics;
use xops_game::debug::DebugLog;
use xops_game::gamemain::init_game;
use xops_game::input::InputControl;
use xops_game::resource::ResourceManager;
use xops_game::scheduler::{process_screen, Stage};
use xops_game::sound::SoundControl;
use xops_game::soundmanager::SoundManager;
use xops_game::statemachine::StateMachine;
use xops_game::window::{control_fps, WindowControl};
use xops_platform::gfx::Gfx;
use xops_platform::host::HostWindow;
use xops_platform::input::Input;
use xops_platform::window::{self, App};
use xops_platform::winit::event::WindowEvent;
use xops_platform::winit::window::Window;

/// The configuration file, the upstream `config.dat`.
const CONFIG_FILE: &str = "config.dat";
/// The additional settings file, which only the extended configuration reads.
const EXT_CONFIG_FILE: &str = "config-openxops.ini";

/// Everything the frame loop owns. Upstream these are file scope values in `main.cpp` and
/// `gamemain.cpp`, described there as a set of singletons.
struct Game {
	stage: Option<Stage<'static>>,
	sound: SoundManager,
	sound_ctrl: SoundControl,
	config: Config,
	input_ctrl: InputControl,
	state: StateMachine,
	d3dg: D3DGraphics,
	window: WindowControl,
	log: DebugLog,
	/// What `ProcessScreen` reads for the frame skip, and nothing else: the screens count
	/// their own frames.
	framecnt: u32,
}

impl Game {
	/// The upstream `InitGame`: the renderer, the input, the sound, the resource loads and
	/// the addeons, in that order, because each one's setup is what the next one reads.
	///
	/// The mission path is the upstream `mode`. A mission means the briefing and the main
	/// screen only, started directly at the main screen, with the mission file read here
	/// rather than by the menu.
	fn start(
		config: Config,
		mut log: DebugLog,
		mut sound_ctrl: SoundControl,
		gfx: Gfx,
		window: WindowControl,
		mission: Option<&Path>,
	) -> Result<Game, String> {
		let mut d3dg = D3DGraphics::new(gfx, config.get_fullscreen_flag(), config.get_language());
		let mut input_ctrl = InputControl::new();
		let mut state = StateMachine::new();

		// The resource table is reached by the world through a mutable borrow and outlives
		// every screen, so it is leaked rather than owned by a structure that would then
		// hold both it and the world that borrows it. It lives exactly as long as the
		// process, which is what a leak means here.
		let resource: &'static mut ResourceManager = Box::leak(Box::new(ResourceManager::new()));

		// The upstream `InitGame`, which reads the texture font, loads the resource, sets
		// the mouse limit, sets the volume, and then either starts a named mission or
		// builds the addon list the menu needs.
		//
		// The mode is 2, the upstream "main screen only": a mission named on the command
		// line is read here, and the menu is skipped because nothing would be clicked
		// through it.
		let path = mission.map(|path| path.as_os_str().as_encoded_bytes().to_vec());
		let mode = if path.is_some() { 2 } else { 0 };
		let world = init_game(
			&config,
			&mut d3dg,
			&mut input_ctrl,
			&mut sound_ctrl,
			resource,
			&mut state,
			mode,
			path.as_deref(),
		)?;

		log.write_log(
			xops_game::debug::LOG_CHECK,
			b"Startup",
			format!("{GAMENAME} {GAMEVERSION}").as_bytes(),
		);

		Ok(Game {
			stage: Some(Stage::Idle(world)),
			sound: SoundManager::new(),
			sound_ctrl,
			config,
			input_ctrl,
			state,
			d3dg,
			window,
			log,
			framecnt: 0,
		})
	}

	/// One pass of the upstream loop: the screen's phase, then the frame rate control.
	fn step(&mut self, input: &mut Input) -> bool {
		if self.state.get_state() == xops_game::statemachine::State::STATE_EXIT {
			return false;
		}

		self.input_ctrl.get_input_state(input, false);
		process_screen(
			&mut self.stage,
			&mut self.state,
			&mut self.window,
			&mut self.config,
			&mut self.input_ctrl,
			input,
			&mut self.d3dg,
			&mut self.sound,
			&mut self.sound_ctrl,
			&mut self.log,
			self.framecnt,
		);
		control_fps();
		self.framecnt = self.framecnt.wrapping_add(1);
		true
	}

	/// Renders the frame that has been recorded and hands the pixels back, for the
	/// offscreen mode. The windowed mode presents it instead.
	#[cfg(not(target_arch = "wasm32"))]
	fn frame_pixels(&mut self) -> Result<(u32, u32, Vec<u8>), String> {
		self.d3dg.gfx_ref().read_frame()
	}
}

impl Drop for Game {
	/// The upstream `CleanupGame`, in its order.
	fn drop(&mut self) {
		if let Some(stage) = self.stage.take() {
			let world = stage.into_world();
			world
				.resource
				.destroy_resource(&mut self.d3dg, &mut self.sound_ctrl);
		}
	}
}

/// The host's application. It holds the game once the window exists, because the renderer
/// is built from the surface the host creates.
struct Shell {
	config: Config,
	log: DebugLog,
	sound_ctrl: SoundControl,
	window: WindowControl,
	mission: Option<PathBuf>,
	game: Option<Game>,
	frames: u64,
}

impl Shell {
	fn new(mission: Option<PathBuf>) -> Shell {
		Shell {
			config: Config::new(),
			log: DebugLog::new(),
			sound_ctrl: SoundControl::new(),
			window: WindowControl::new(),
			mission,
			game: None,
			frames: 0,
		}
	}

	/// The upstream preamble: the data directory, the configuration, the sound device and
	/// the window, in the order the original does them.
	fn prepare(&mut self, host: HostWindow) -> Result<(), String> {
		#[cfg(not(target_arch = "wasm32"))]
		if !Path::new("data").is_dir() {
			return Err("the data directory is missing. Copy the game data next to here".into());
		}

		if self.config.load_file(CONFIG_FILE, &mut self.log) == 1 {
			// The upstream `ENABLE_AUTOCREATECONFIG`: write the defaults out rather than
			// refuse to start, which is what makes a first run work.
			self.config.set_default_config();
			self.config.save_file(CONFIG_FILE, &mut self.log);
		}
		self.config.create_ext_config(EXT_CONFIG_FILE, &mut self.log);
		self.config.load_ext_file(EXT_CONFIG_FILE, &mut self.log);

		if self.sound_ctrl.init_sound() != 0 {
			return Err("the sound device did not open".into());
		}

		let fullscreen = self.config.get_fullscreen_flag();
		if !self
			.window
			.init_window(host, &format!("{GAMENAME} {GAMEVERSION}"), fullscreen)
		{
			return Err("the window did not initialize".into());
		}
		Ok(())
	}
}

impl App for Shell {
	fn init(&mut self, window: &Arc<Window>, gfx: Gfx) {
		if let Err(error) = self.prepare(HostWindow::new(window.clone())) {
			eprintln!("xops: {error}");
			std::process::exit(1);
		}

		let config = std::mem::replace(&mut self.config, Config::new());
		let log = std::mem::replace(&mut self.log, DebugLog::new());
		let sound_ctrl = std::mem::replace(&mut self.sound_ctrl, SoundControl::new());
		let window_control = std::mem::replace(&mut self.window, WindowControl::new());
		match Game::start(
			config,
			log,
			sound_ctrl,
			gfx,
			window_control,
			self.mission.as_deref(),
		) {
			Ok(game) => self.game = Some(game),
			Err(error) => {
				eprintln!("xops: {error}");
				std::process::exit(1);
			}
		}
	}

	fn frame(&mut self, input: &mut Input) -> bool {
		let Some(game) = self.game.as_mut() else {
			return false;
		};
		self.frames += 1;
		game.step(input)
	}

	fn resized(&mut self, width: u32, height: u32) {
		if let Some(game) = &mut self.game {
			game.d3dg.set_size(width as i32, height as i32);
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

#[cfg(not(target_arch = "wasm32"))]
struct Options {
	/// Render this many frames offscreen and write them out, instead of opening a window.
	frames: Option<u64>,
	out: Option<PathBuf>,
	width: u32,
	height: u32,
	/// A `.mif` to start directly in. Its paths are relative to `--dir`.
	mission: Option<PathBuf>,
	/// Where the game's data lives. The game's paths are relative to it, so this is the
	/// directory the process runs in.
	dir: Option<PathBuf>,
	help: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for Options {
	fn default() -> Self {
		Options {
			frames: None,
			out: None,
			width: 640,
			height: 480,
			mission: None,
			dir: None,
			help: false,
		}
	}
}

#[cfg(not(target_arch = "wasm32"))]
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
			"-h" | "--help" => options.help = true,
			other => return Err(format!("{other}: unknown option")),
		}
		index += 1;
	}

	if options.frames.is_some() && options.out.is_none() {
		return Err("--frames needs --out".into());
	}
	if options.width == 0 || options.height == 0 {
		return Err("--size must be positive".into());
	}
	Ok(options)
}

#[cfg(not(target_arch = "wasm32"))]
fn usage() -> &'static str {
	"xops [options]

  --dir PATH      the game's directory, which its own paths are relative to
  --mission PATH  start straight into this .mif instead of at the opening screen
  --frames N      render N frames offscreen and write them to --out, then exit
  --out DIR       where those frames go
  --size WxH      frame size (default 640x480)
  -h, --help      this text

With no --frames, opens a window and runs the game."
}

/// A browser has no `main`: the page calls [`start`], which is what the module's own start
/// hook runs. The binary target still needs one.
#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
	wasm_bindgen_futures::spawn_local(async {
		if let Err(error) = start_web().await {
			web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&error));
		}
	});
}

/// The web entry point. The page calls this once its canvas exists.
///
/// There are no arguments and no working directory in a browser, so what carries the
/// command line's work here is the page's own address: `?mission=` names a mission to start
/// straight into, and with none the game opens on the opening screen, the same two modes
/// the desktop build has.
#[cfg(target_arch = "wasm32")]
async fn start_web() -> Result<(), String> {
	use wasm_bindgen::JsCast;

	let window = web_sys::window().ok_or("there is no window")?;
	let document = window.document().ok_or("there is no document")?;
	let canvas = document
		.get_element_by_id("xops")
		.ok_or("the page has no element named xops")?
		.dyn_into::<web_sys::HtmlCanvasElement>()
		.map_err(|_| "the element named xops is not a canvas")?;

	let mission = window
		.location()
		.search()
		.ok()
		.and_then(|search| mission_from_query(&search));

	// Everything the game reads, fetched once. The game's own loaders are synchronous and
	// nothing in them can wait on a request, so the waiting happens here, before the first
	// of them runs.
	xops_platform::fs::preload().await?;

	window::run_web(Shell::new(mission), canvas).await
}

/// The `mission=` value out of a query string, which is how a page names a mission.
#[cfg(target_arch = "wasm32")]
fn mission_from_query(search: &str) -> Option<PathBuf> {
	for pair in search.trim_start_matches('?').split('&') {
		let (key, value) = pair.split_once('=')?;
		if key == "mission" {
			let decoded = js_sys::decode_uri_component(value).ok()?;
			return decoded
				.as_string()
				.map(PathBuf::from)
				.filter(|path| !path.as_os_str().is_empty());
		}
	}
	None
}

#[cfg(not(target_arch = "wasm32"))]
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

	// The game's own paths are relative to its directory, so that has to be the working
	// directory before anything loads.
	if let Some(dir) = &options.dir {
		if let Err(error) = std::env::set_current_dir(dir) {
			eprintln!("xops: {}: {error}", dir.display());
			return ExitCode::FAILURE;
		}
	}

	let result = match options.frames {
		Some(frames) => headless(&options, frames),
		None => window::run(Shell::new(options.mission.clone())),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(error) => {
			eprintln!("xops: {error}");
			ExitCode::FAILURE
		}
	}
}

/// Runs the game with no window. The frames are read back after each pass, which is only
/// possible offscreen, so this is also the only mode that can show a frame as a file.
#[cfg(not(target_arch = "wasm32"))]
fn headless(options: &Options, frames: u64) -> Result<(), String> {
	let out = options.out.as_ref().expect("checked in parse_args");
	let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
		backends: wgpu::Backends::all(),
		..Default::default()
	});
	let gfx = Gfx::new(&instance, None, options.width, options.height, None)?;

	let mut shell = Shell::new(options.mission.clone());
	shell.prepare(HostWindow::default())?;

	let config = std::mem::replace(&mut shell.config, Config::new());
	let log = std::mem::replace(&mut shell.log, DebugLog::new());
	let sound_ctrl = std::mem::replace(&mut shell.sound_ctrl, SoundControl::new());
	let window = std::mem::replace(&mut shell.window, WindowControl::new());
	let mut game = Game::start(
		config,
		log,
		sound_ctrl,
		gfx,
		window,
		options.mission.as_deref(),
	)?;

	let mut input = Input::default();
	let mut rendered = Vec::with_capacity(frames as usize);
	for _ in 0..frames {
		if !game.step(&mut input) {
			break;
		}
		rendered.push(game.frame_pixels()?);
	}

	let (width, height) = game.d3dg.gfx_ref().size();
	let pixels: Vec<Vec<u8>> = rendered.into_iter().map(|(_, _, p)| p).collect();
	xops_platform::frame::write_frame_sequence(out, "frame", width, height, &pixels)?;
	println!(
		"wrote {} frames of {width}x{height} to {}",
		pixels.len(),
		out.display()
	);
	Ok(())
}
