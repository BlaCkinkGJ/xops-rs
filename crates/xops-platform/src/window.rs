//! The window and the frame loop.
//!
//! The game runs on a fixed 30 ms step and draws once per step, so the host owns the
//! clock rather than the display: a slow frame does not change how much time the world
//! is simulated for. That is the same contract the upstream loop had with `timeGetTime`.

use std::sync::Arc;
use web_time::{Duration, Instant};

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
	/// The window attributes, which a browser build has to hand over ready made because the
	/// canvas comes from the page rather than from the platform.
	#[cfg(target_arch = "wasm32")]
	attributes: Option<WindowAttributes>,
	/// The renderer, made before the loop starts. Requesting a device needs an await, and
	/// nothing inside the event loop can wait for one.
	#[cfg(target_arch = "wasm32")]
	gfx: Option<Gfx>,
}

impl<A: App> ApplicationHandler for Host<A> {
	fn resumed(&mut self, event_loop: &ActiveEventLoop) {
		if self.window.is_some() {
			return;
		}

		#[cfg(not(target_arch = "wasm32"))]
		let attributes = WindowAttributes::default()
			.with_title("xops")
			.with_inner_size(winit::dpi::LogicalSize::new(640.0, 480.0));
		#[cfg(target_arch = "wasm32")]
		let attributes = self
			.attributes
			.take()
			.expect("run_web sets the attributes before it starts the loop");
		let window = match event_loop.create_window(attributes) {
			Ok(window) => Arc::new(window),
			Err(error) => {
				eprintln!("window: {error}");
				event_loop.exit();
				return;
			}
		};

		// Only the desktop builds its renderer here. The web one was built before the loop
		// started, because the browser will not let it wait.
		#[cfg(not(target_arch = "wasm32"))]
		let size = window.inner_size();
		#[cfg(not(target_arch = "wasm32"))]
		let gfx = {
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
		#[cfg(target_arch = "wasm32")]
		let gfx = self.gfx.take().expect("run_web builds the renderer");

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

		if let Some(window) = &self.window {
			window.request_redraw();
		}

		// The desktop is told when the next frame is due so it can sleep until then. A page
		// cannot sleep, and winit's web loop only runs a frame when the browser hands it one,
		// so over there the loop polls and the animation callbacks are what pace it.
		#[cfg(not(target_arch = "wasm32"))]
		event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
	}
}

impl<A: App> Host<A> {
	fn new(app: A) -> Host<A> {
		Host {
			app,
			#[cfg(target_arch = "wasm32")]
			attributes: None,
			#[cfg(target_arch = "wasm32")]
			gfx: None,
			window: None,
			input: Input::default(),
			next_frame: Instant::now(),
			frames: 0,
		}
	}
}

/// Runs the window until the app quits or the window closes.
/// Runs the game in a page, drawing into the canvas the page provides.
///
/// Separate from [`run`] because a browser will not let anything block on the device
/// request: this is the one entry point that has to be awaited, and it makes the renderer
/// before the event loop starts so that nothing inside the loop has to wait.
#[cfg(target_arch = "wasm32")]
pub async fn run_web<A: App>(
	app: A,
	canvas: web_sys::HtmlCanvasElement,
) -> Result<(), String> {
	use winit::platform::web::WindowAttributesExtWebSys;

	console_error_panic_hook::set_once();

	let width = canvas.width().max(1);
	let height = canvas.height().max(1);
	let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
		backends: wgpu::Backends::all(),
		..Default::default()
	});
	let gfx = Gfx::new_web(&instance, canvas.clone(), width, height).await?;

	let mut host = Host::new(app);
	host.attributes = Some(
		WindowAttributes::default()
			.with_canvas(Some(canvas))
			.with_inner_size(winit::dpi::LogicalSize::new(width, height)),
	);
	host.gfx = Some(gfx);

	let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
	event_loop.set_control_flow(ControlFlow::Poll);
	event_loop
		.run_app(&mut host)
		.map_err(|e| format!("event loop: {e}"))
}

pub fn run<A: App>(app: A) -> Result<(), String> {
	let event_loop = EventLoop::new().map_err(|e| format!("event loop: {e}"))?;
	event_loop.set_control_flow(ControlFlow::Wait);
	let mut host = Host::new(app);
	event_loop
		.run_app(&mut host)
		.map_err(|e| format!("event loop: {e}"))
}
