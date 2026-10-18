//! The window and the frame loop.
//!
//! The game runs on a fixed 30 ms step and draws once per step, so the host owns the
//! clock rather than the display: a slow frame does not change how much time the world
//! is simulated for. That is the same contract the upstream loop had with `timeGetTime`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{CursorGrabMode, Window, WindowAttributes, WindowId};

use crate::gfx::Gfx;
use crate::input::Input;
use crate::trace;

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
	/// The size the window is created at, before [`App::init`] runs.
	///
	/// The default is what the original game used. An app whose own screen size has to
	/// match its window, as this one's does, returns that here rather than resizing after
	/// the first frame.
	fn window_size(&self) -> (u32, u32) {
		(640, 480)
	}

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

	/// Which screen is up, for the trace. "menu" and "mission" are different reports.
	fn screen(&self) -> &'static str {
		"unknown"
	}

	/// Draws the last frame again, partway to the one before it, and presents that.
	///
	/// The host asks for this between the game's own steps, so a display can be given a
	/// frame on every one of its refreshes while the game keeps its own rate.
	///
	/// The return says whether the frame was blended: false means the frame went out as the
	/// game drew it, which happens when there is nothing to blend it with. It is what the
	/// trace counts, so that a fallback is visible rather than silent.
	fn present_between(&mut self, t: f32) -> bool {
		let _ = t;
		false
	}
}

struct Host<A: App> {
	app: A,
	window: Option<Arc<Window>>,
	input: Input,
	next_frame: Instant,
	/// When the last frame started, which is what the actual frame interval is measured
	/// against: the game asks for a rate, and this is the one it gets.
	last_frame_start: Instant,
	/// When the last burst of frames ended, which is what the loop's idle time is measured
	/// from.
	last_burst_end: Option<Instant>,
	/// Whether the display is given a frame per refresh between the steps at all.
	interpolating: bool,
	/// The display's refresh rate in millihertz, where it can be read.
	monitor_refresh: Option<u32>,
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

		// The size the app asks for, fitted to the display it is opening on: a window larger
		// than the screen cannot be seen, and the largest of the original's resolutions is
		// taller than a laptop's screen.
		let (width, height) = self.app.window_size();
		let (width, height) = fit_to_display(event_loop, width.max(1), height.max(1));
		let attributes = WindowAttributes::default()
			.with_title("xops")
			.with_inner_size(winit::dpi::LogicalSize::new(width, height));
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

		self.monitor_refresh = event_loop
			.primary_monitor()
			.and_then(|monitor| monitor.refresh_rate_millihertz());
		trace::note(&format!(
			"display refresh {:?} mHz, so a frame every {:.2} ms; interpolation {}",
			self.monitor_refresh,
			self.refresh().as_secs_f64() * 1000.0,
			if self.interpolating { "on" } else { "off" }
		));
		self.window = Some(window.clone());
		self.next_frame = Instant::now();
		self.app.init(&window, gfx);
	}

	/// The raw device events, which carry the mouse's own movement. They are a separate
	/// callback because they are not about the window: a warp of the cursor produces window
	/// events but never one of these.
	fn device_event(
		&mut self,
		_event_loop: &ActiveEventLoop,
		_device_id: DeviceId,
		event: DeviceEvent,
	) {
		self.input.device_event(&event);
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

		// What the host gained or lost between the last burst and this one. A value near
		// the frame step is the loop working, and a second is the loop asleep.
		let idle = self.last_burst_end.map(|end| end.elapsed().as_secs_f64() * 1000.0);
		if let Some(idle) = idle {
			trace::wait(idle);
		}

		// Run every frame that is due. A hitch longer than a step catches up rather than
		// skipping, but only up to a bound: a long stall (a debugger, a breakpoint) must
		// not queue hundreds of frames.
		let mut due = 0;
		while Instant::now() >= self.next_frame && due < 4 {
			let started = Instant::now();
			let interval = started.duration_since(self.last_frame_start).as_secs_f64() * 1000.0;
			self.last_frame_start = started;
			if !self.app.frame(&mut self.input) {
				event_loop.exit();
				return;
			}
			trace::frame(started.elapsed().as_secs_f64() * 1000.0, interval);
			trace::screen(self.app.screen(), self.input.is_captured());
			trace::report();
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

		// No `request_redraw` here. The game presents its own frame at the end of its own
		// step, so a redraw request asks for nothing; worse, it is answered immediately
		// with a `RedrawRequested`, which comes back through `window_event` and lands in
		// `about_to_wait` again. That is an endless loop around the event loop, burning a
		// core and running hundreds of thousands of times a second while the game's frames
		// themselves are paced by the wait below.
		// The frames the display is given between this step and the next.
		//
		// A display wants a frame every 16.7 ms and the game makes one every 30, and 30 is not
		// a multiple of 16.7: without this the display is given one frame for a refresh here
		// and two there, and that unevenness is what reads as a low frame rate even when the
		// game is keeping up. Each of these frames is the game's own last frame drawn partway
		// to the one before it, so nothing is simulated twice and the game keeps its own speed.
		//
		// Paced with the small sleep rather than with the event loop's timer, which is the one
		// thing about this that had to be measured: a wait of one refresh handed to the event
		// loop comes back after twenty or thirty milliseconds here, so half the refreshes would
		// go by with nothing new on them. The sleep below re-reads the clock between short
		// sleeps and therefore keeps time. The surface is a mailbox, so the display takes the
		// newest frame at each of its own refreshes and drops the rest.
		if self.interpolating {
			let refresh = self.refresh();
			loop {
				let now = Instant::now();
				if now >= self.next_frame {
					break;
				}
				let t = self.interpolation();
				if self.app.present_between(t) {
					trace::presented_at(t);
				} else {
					trace::presented_at(t);
					trace::presented_unblended();
				}
				crate::host::sleep_until((now + refresh).min(self.next_frame));
			}
		}

		self.last_burst_end = Some(Instant::now());

		// The host owns the clock, which is the arrangement this file describes at the top:
		// the game runs on a fixed step and the frame is drawn once per step, so waiting
		// until the next one is due is the frame rate control. The game's own one has to know
		// that and not add a second sleep of its own.
		event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
	}
}

/// Fits a window size to the display, keeping its shape.
///
/// Nothing is done when the display is large enough, which is the usual case. On a screen
/// smaller than the request the window is scaled down to leave room for the desktop around
/// it, because a window the size of the screen has no title bar to move it by.
fn fit_to_display(
	event_loop: &ActiveEventLoop,
	width: u32,
	height: u32,
) -> (u32, u32) {
	let Some(monitor) = event_loop.primary_monitor() else {
		return (width, height);
	};
	let size = monitor.size();
	let scale = monitor.scale_factor() as f64;
	let (available_width, available_height) = (
		(size.width as f64 / scale) * 0.9,
		(size.height as f64 / scale) * 0.9,
	);
	if width as f64 <= available_width && height as f64 <= available_height {
		return (width, height);
	}
	let shrink = (available_width / width as f64).min(available_height / height as f64);
	(
		(width as f64 * shrink).max(1.0) as u32,
		(height as f64 * shrink).max(1.0) as u32,
	)
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

		// The flag the cursor warp reads, so the game's own per-frame warp can be skipped
		// while the cursor is held. It has to be: see `set_cursor_position`.
		crate::host::set_looking(looking);

		let Some(window) = &self.window else {
			return;
		};

		// `Locked` is the only mode macOS implements, and it is exactly the one the game
		// wants: the system stops moving the cursor with the mouse and every mouse event
		// carries the movement as a delta instead, which is where the game's movement comes
		// from. (`Confined` is refused there, so asking for it does nothing at all.)
		let mode = if looking {
			CursorGrabMode::Locked
		} else {
			CursorGrabMode::None
		};
		let _ = window.set_cursor_grab(mode);
		window.set_cursor_visible(!looking);
	}

	/// Where the picture is between two of the game's steps: nought is the step that ran last
	/// and one is the step that is due next.
	///
	/// Measured against the step's own schedule, not against a clock taken when the step
	/// happened to start. The two are not the same thing: a step takes a millisecond or two,
	/// so a clock started with it runs out early, and the moment a step ran the factor would
	/// jump back from past one to nought and the picture would go backwards once a step.
	/// Against the schedule it cannot: the factor is the part of the current step that this
	/// frame has reached, and it reaches one exactly as the next step falls due.
	fn interpolation(&self) -> f32 {
		let due = self.next_frame - frame_step();
		let elapsed = Instant::now().duration_since(due).as_secs_f64();
		(elapsed / frame_step().as_secs_f64()) as f32
	}

	fn new(app: A) -> Host<A> {
		Host {
			app,
			window: None,
			input: Input::default(),
			next_frame: Instant::now(),
			last_frame_start: Instant::now(),
			last_burst_end: None,
			interpolating: interpolating(),
			monitor_refresh: None,
			frames: 0,
			cursor_captured: false,
		}
	}
}

/// How often the display wants a frame.
///
/// A display whose rate cannot be read is taken for 60 Hz, which is what almost every one of
/// them is.
impl<A: App> Host<A> {
	fn refresh(&self) -> Duration {
		let millihertz = self.monitor_refresh.unwrap_or(DEFAULT_REFRESH_MILLIHERTZ);
		if millihertz == 0 {
			return Duration::from_micros(16_667);
		}
		Duration::from_micros(1_000_000_000 / millihertz as u64)
	}
}

/// 60 Hz in the units a monitor reports it in.
const DEFAULT_REFRESH_MILLIHERTZ: u32 = 60_000;

/// Whether the display is given a frame of its own between the game's steps.
///
/// On by default, because a display that is only given 33 frames a second while it wants 60
/// shows them unevenly. `XOPS_INTERPOLATE=0` turns it off, which leaves exactly the frames
/// the game makes and nothing else: the original's own look.
fn interpolating() -> bool {
	static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
	*ON.get_or_init(|| !std::env::var("XOPS_INTERPOLATE").is_ok_and(|value| value == "0"))
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
