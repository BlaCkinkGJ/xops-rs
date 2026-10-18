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
use winit::window::CursorGrabMode;
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
///
/// The app takes ownership of the renderer at `init`, because the game's own graphics
/// class owns it upstream and the frame is presented by the game's own end-of-frame call
/// rather than by the host.
pub trait App {
	/// Called once the window exists. The app takes the renderer from here.
	fn init(&mut self, window: &Arc<Window>, gfx: Gfx);

	/// One simulated frame. Return `false` to quit.
	fn frame(&mut self, input: &mut Input) -> bool;

	/// The window changed size. The app has to resize its renderer.
	fn resized(&mut self, width: u32, height: u32);

	/// A raw window event, before it reaches the frame loop. Return `false` to quit.
	fn event(&mut self, event: &WindowEvent) -> bool {
		let _ = event;
		true
	}
}

struct Host<A: App> {
	app: A,
	window: Option<Arc<Window>>,
	input: Input,
	next_frame: Instant,
	frames: u64,
	/// Whether the cursor is currently kept in the window. Tracked so the platform is only
	/// told when it changes.
	cursor_captured: bool,
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

		let gfx = {
			let size = window.inner_size();
			let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
				backends: wgpu::Backends::all(),
				..Default::default()
			});
			match Gfx::new(
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
			}
		};

		window.request_redraw();
		self.window = Some(window.clone());
		self.next_frame = Instant::now();
		self.app.init(&window, gfx);
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
				self.app
					.resized(size.width.max(1), size.height.max(1));
			}
			_ => {}
		}
	}

	fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_none() {
			event_loop.exit();
			return;
		}

		// Run every frame that is due. A hitch longer than a step catches up rather than
		// skipping, but only up to a bound: a long stall (a debugger, a breakpoint) must
		// not queue hundreds of frames.
		let mut due = 0;
		while Instant::now() >= self.next_frame && due < 4 {
			if !self.app.frame(&mut self.input) {
				event_loop.exit();
				return;
			}
			self.input.end_frame();
			self.next_frame += frame_step();
			self.frames += 1;
			due += 1;
		}
		if due == 4 {
			// Fell too far behind, so drop the backlog instead of accumulating it.
			self.next_frame = Instant::now() + frame_step();
		}

		self.sync_cursor();

		if let Some(window) = &self.window {
			window.request_redraw();
		}

		event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
	}
}

impl<A: App> Host<A> {
	/// Keeps the cursor in the window while the game is using it to look.
	///
	/// The game warps the cursor to the middle of its own screen every frame and reads how
	/// far it moved, which is how the original did it. Two things have to go with that: the
	/// cursor must stay inside the window, or it reaches an edge and the look stops, and it
	/// must be invisible, or it sits in the middle of the screen. A menu wants neither, and
	/// the game says which it is by the flag it sets each frame, so this follows it.
	///
	/// A grab can be refused, on a platform or a session that will not allow it. That is not
	/// fatal: the cursor is simply visible and free, and the game still runs.
	fn sync_cursor(&mut self) {
		let looking = self.input.is_captured();
		if looking == self.cursor_captured {
			return;
		}
		self.cursor_captured = looking;

		let Some(window) = &self.window else {
			return;
		};
		window.set_cursor_visible(!looking);
		let mode = if looking {
			CursorGrabMode::Confined
		} else {
			CursorGrabMode::None
		};
		let _ = window.set_cursor_grab(mode);
	}

	fn new(app: A) -> Host<A> {
		Host {
			app,
			window: None,
			input: Input::default(),
			next_frame: Instant::now(),
			frames: 0,
			cursor_captured: false,
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
