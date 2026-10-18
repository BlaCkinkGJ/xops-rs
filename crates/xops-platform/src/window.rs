//! The window and the frame loop.
//!
//! The game runs on a fixed 30 ms step and draws once per step, so the host owns the
//! clock rather than the display: a slow frame does not change how much time the world
//! is simulated for. That is the same contract the upstream loop had with `timeGetTime`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::gfx::Gfx;
use crate::input::Input;

/// Processing time per frame, from the upstream `GAMEFRAMEMS`.
pub const FRAME_MS: u64 = 30;

/// The frame step as a duration.
pub fn frame_step() -> Duration {
	Duration::from_millis(FRAME_MS)
}

/// What the host calls back into.
pub trait App {
	/// Called once the window and renderer exist.
	fn init(&mut self, window: &Arc<Window>, gfx: &mut Gfx, input: &mut Input);

	/// One simulated frame. Return `false` to quit.
	fn frame(&mut self, gfx: &mut Gfx, input: &mut Input) -> bool;

	/// The window changed size. The renderer has already been resized.
	fn resized(&mut self, gfx: &mut Gfx, width: u32, height: u32);

	/// A raw window event, before it reaches the frame loop. Return `false` to quit.
	fn event(&mut self, event: &WindowEvent) -> bool {
		let _ = event;
		true
	}
}

struct Host<A: App> {
	app: A,
	window: Option<Arc<Window>>,
	gfx: Option<Gfx>,
	input: Input,
	next_frame: Instant,
	frames: u64,
}

impl<A: App> ApplicationHandler for Host<A> {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}

		let attributes = WindowAttributes::default()
			.with_title("xops")
			.with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
		let window = match event_loop.create_window(attributes) {
			Ok(window) => Arc::new(window),
			Err(error) => {
				eprintln!("window: {error}");
				event_loop.exit();
				return;
			}
		};

		let size = window.inner_size();
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = match Gfx::new(
			&instance,
			Some(window.clone()),
			size.width.max(1),
			size.height.max(1),
			None,
		) {
			Ok(gfx) => gfx,
			Err(error) => {
				eprintln!("renderer: {error}");
				event_loop.exit();
				return;
			}
		};

		window.request_redraw();
		self.window = Some(window);
		self.gfx = Some(gfx);
		self.next_frame = Instant::now();

		let window = self.window.as_ref().expect("just stored").clone();
		let gfx = self.gfx.as_mut().expect("just stored");
		self.app.init(&window, gfx, &mut self.input);
	}

	fn window_event(
		&mut self,
		event_loop: &ActiveEventLoop,
		_window_id: WindowId,
		event: WindowEvent,
	) {
		self.input.handle(&event);

		if !self.app.event(&event) {
			event_loop.exit();
			return;
		}

		match event {
			WindowEvent::CloseRequested => event_loop.exit(),
			WindowEvent::Resized(size) => {
				if let Some(gfx) = &mut self.gfx {
					gfx.resize(size.width.max(1), size.height.max(1));
					self.app.resized(gfx, size.width.max(1), size.height.max(1));
				}
			}
			_ => {}
		}
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		let Some(gfx) = &mut self.gfx else {
			event_loop.exit();
			return;
		};

		// Run every frame that is due. A hitch longer than a step catches up rather than
		// skipping, but only up to a bound: a long stall (a debugger, a breakpoint) must
		// not queue hundreds of frames.
		let mut due = 0;
		while Instant::now() >= self.next_frame && due < 4 {
			if !self.app.frame(gfx, &mut self.input) {
				event_loop.exit();
				return;
			}
			self.input.end_frame();
			if let Err(error) = gfx.end_frame() {
				eprintln!("frame: {error}");
				event_loop.exit();
				return;
			}
			self.next_frame += frame_step();
			self.frames += 1;
			due += 1;
		}
		if due == 4 {
			// Fell too far behind, so drop the backlog instead of accumulating it.
			self.next_frame = Instant::now() + frame_step();
		}

		if let Some(window) = &self.window {
			window.request_redraw();
		}
		event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
	}
}

impl<A: App> Host<A> {
	fn new(app: A) -> Host<A> {
		Host {
			app,
			window: None,
			gfx: None,
			input: Input::default(),
			next_frame: Instant::now(),
			frames: 0,
		}
	}
}

/// Runs the window until the app quits or the window closes.
pub fn run<A: App>(app: A) -> Result<(), String> {
	let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let mut host = Host::new(app);
	event_loop
		.run_app(&mut host)
		.map_err(|e| format!("event loop: {e}"))
}
