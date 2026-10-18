//! What the game asks of the operating system.
//!
//! `window.rs` owns the event loop, the window and the frame step, which is the part of
//! the upstream `window.cpp` that belongs to the host. What is left is everything the game
//! asked Windows for directly: the millisecond tick, the date, the machine it runs on, the
//! display mode, the client size, the title, the cursor and the error dialog. They live
//! here so that the game crate can name them without naming a window type, the same way it
//! draws without naming a wgpu type.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use winit::dpi::PhysicalPosition;
use winit::window::{Fullscreen, Window};

/// Milliseconds since the first call, the upstream `GetTimeMS`.
///
/// Windows counted from system start, which is a base the game can never observe, because
/// every use is a difference of two readings. The count is the upstream `unsigned int`, so
/// it wraps at 2^32 ms like `timeGetTime` did and the game's subtraction of two readings
/// stays modular.
pub fn time_ms() -> u32 {
	static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);
	BASE.elapsed().as_millis() as u32
}

/// Seconds from the epoch, the upstream `time(NULL)`.
fn unix_time() -> i64 {
	match SystemTime::now().duration_since(UNIX_EPOCH) {
		Ok(since) => since.as_secs() as i64,
		Err(_) => 0,
	}
}

/// The date and time as (year, month, day, hour, minute, second), the upstream
/// `GetTimeName`'s clock.
///
/// The upstream took it from `localtime`. The standard library has no local time, and
/// getting one means a dependency that reads the time zone database, so this is UTC. The
/// only caller turns it into a file name, and a file name has the same shape either way.
///
/// The upstream `GetTimeName` formats this into `YYYYMMDDhhmmss`, which the game crate
/// does, so the platform side stops at the parts.
pub fn time_parts() -> (i32, u32, u32, u32, u32, u32) {
	civil_from_unix(unix_time())
}

/// Breaks seconds from the epoch into a civil date and time, in UTC.
fn civil_from_unix(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
	let days = secs.div_euclid(86_400);
	let rem = secs.rem_euclid(86_400);

	// Days from 1970-01-01 to 0000-03-01, then the 400 year era the day falls in.
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	// A year starting in March, so the leap day lands at the end of it.
	let mp = (5 * doy + 2) / 153;
	let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
	let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
	let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };

	(
		year as i32,
		month,
		day,
		(rem / 3600) as u32,
		(rem / 60 % 60) as u32,
		(rem % 60) as u32,
	)
}

/// The machine, as the startup log describes it, the upstream `GetOperatingEnvironment`.
///
/// The upstream read the marketing name and the display version out of the registry and
/// asked for the processor architecture. The standard library knows the names of the
/// target a binary was built for and not the version of the running system, so the name is
/// the family and the version slot carries the architecture: it says which build of the
/// port is running, which is what a version line is for.
pub struct OperatingEnvironment {
	/// `ProductName` upstream: `macos`, `linux` or `windows`.
	pub name: &'static str,
	/// The architecture, in the slot upstream filled from `DisplayVersion`.
	pub arch: &'static str,
	/// `OSbit` upstream: `64bit`, `32bit` or `unknown bit`.
	pub bits: &'static str,
}

/// Reads what the startup log says about the machine.
pub fn operating_environment() -> OperatingEnvironment {
	OperatingEnvironment {
		name: std::env::consts::OS,
		arch: std::env::consts::ARCH,
		// The upstream switched on the processor architecture, which is the width of a
		// pointer here.
		bits: match std::mem::size_of::<usize>() * 8 {
			64 => "64bit",
			32 => "32bit",
			_ => "unknown bit",
		},
	}
}

/// Sleeps for about this long.
///
/// A single long sleep is not reliable: macOS coalesces timers to save power, so a request
/// for thirty milliseconds comes back after seventy or a hundred, and a game that paces
/// itself with one runs at a third of its intended rate. Sleeping in small steps and looking
/// at the clock between them keeps the total near what was asked for, because an over-long
/// step simply ends the loop early.
///
/// The granularity is small enough to be accurate and large enough not to spin.
pub fn sleep_millis(millis: u32) {
	sleep_until(std::time::Instant::now() + std::time::Duration::from_millis(millis as u64));
}

/// Sleeps until an instant, in small steps, the same way and for the same reason.
///
/// The event loop's own wait is not usable for this: on this system a wait of sixteen
/// milliseconds comes back after twenty or thirty, so anything that wants to happen between
/// the game's steps has to keep its own clock, and the only clock that keeps time here is
/// this one, re-reading it between short sleeps.
pub fn sleep_until(deadline: std::time::Instant) {
	let step = std::time::Duration::from_millis(2);
	loop {
		let now = std::time::Instant::now();
		if now >= deadline {
			return;
		}
		std::thread::sleep((deadline - now).min(step));
	}
}

/// Whether the game is using the mouse to look, which decides whether the cursor is held.
///
/// The host sets it each frame from the flag the game sets, because the game's own per-frame
/// cursor warp has to know: a warp while the cursor is held cancels the hold.
static LOOKING: AtomicBool = AtomicBool::new(false);

/// Records whether the game is looking, for [`HostWindow::set_cursor_position`].
pub fn set_looking(looking: bool) {
	LOOKING.store(looking, Ordering::Relaxed);
}

fn looking() -> bool {
	LOOKING.load(Ordering::Relaxed)
}

/// The window, as much of it as the game's window code uses, the upstream `WindowControl`.
///
/// It may have no window yet: upstream ran `ErrorInfo` before `InitWindow`, so the
/// class existed with a null handle and its window calls answered for that state. The same
/// state is [`HostWindow::default`] here, and the game's `WindowControl` starts in it.
pub struct HostWindow {
	window: Option<Arc<Window>>,
	fullscreen: bool,
}

impl Default for HostWindow {
	/// A window that does not exist yet.
	fn default() -> HostWindow {
		HostWindow {
			window: None,
			fullscreen: false,
		}
	}
}

impl HostWindow {
	/// Takes the window the event loop created, the upstream `InitWindow`'s `CreateWindow`.
	///
	/// The mode is read back rather than assumed, because the caller may have asked winit
	/// for a fullscreen window when it created this one.
	pub fn new(window: Arc<Window>) -> HostWindow {
		let fullscreen = window.fullscreen().is_some();
		HostWindow {
			window: Some(window),
			fullscreen,
		}
	}

	/// Whether a window exists, the upstream `hWnd != NULL`.
	pub fn is_created(&self) -> bool {
		self.window.is_some()
	}

	/// The window title, `CreateWindow`'s title argument upstream.
	pub fn set_title(&self, title: &str) {
		if let Some(window) = &self.window {
			window.set_title(title);
		}
	}

	/// `GetClientRect`: the size of the drawing area, in physical pixels. `None` when the
	/// window does not exist, where the upstream call failed and left the rectangle alone.
	pub fn client_size(&self) -> Option<(u32, u32)> {
		self.window.as_ref().map(|window| {
			let size = window.inner_size();
			(size.width, size.height)
		})
	}

	/// `ChangeWindowMode`: fullscreen (true) or windowed (false).
	///
	/// Upstream switched the display resolution to the game's size and restored the desktop
	/// resolution on the way out. winit will not change the mode of a monitor, so
	/// fullscreen is borderless on the monitor the window is on, and windowed is `None`.
	/// The flag is the mode the last call asked for, which is the mode the window is in
	/// barring a window manager that overrode it.
	pub fn set_fullscreen(&mut self, fullscreen: bool) {
		self.fullscreen = fullscreen;
		if let Some(window) = &self.window {
			window.set_fullscreen(match fullscreen {
				true => Some(Fullscreen::Borderless(None)),
				false => None,
			});
		}
	}

	/// The display mode the window is in, the upstream `ChangeWindowMode`'s argument read
	/// back.
	pub fn fullscreen(&self) -> bool {
		self.fullscreen
	}

	/// `SetCursorPos` together with the client-to-screen step: puts the cursor at a point
	/// in the window's client area, in physical pixels.
	///
	/// The other half, reading the cursor, has no winit call at all: the input layer tracks
	/// [`winit::event::WindowEvent::CursorMoved`] instead, which is where the game's
	/// `GetCursorPos` equivalent lives. A window that cannot warp the cursor (a Wayland
	/// one) reports that here, and it is dropped rather than handled, because the upstream
	/// ignored `SetCursorPos`'s result too and the game plays on either way.
	pub fn set_cursor_position(&self, x: f64, y: f64) {
		// Skipped while the game is looking, and that is not an optimisation.
		//
		// The game asks for this every frame, which is the original's `MoveMouseCenter`: on
		// Windows the cursor had to be kept away from the edge of the window. Two things go
		// wrong with it here. warping a cursor that is held does nothing useful, because a
		// held cursor does not move; and on macOS the warp re-associates the pointer with
		// the cursor, which cancels the hold that the look depends on, so the warp undoes
		// the grab every frame it is called. The movement does not come from the cursor's
		// position in any case, so nothing is lost by leaving it alone.
		if looking() {
			crate::trace::warp(false);
			return;
		}

		if let Some(window) = &self.window {
			let result = window.set_cursor_position(PhysicalPosition::new(x, y));
			crate::trace::warp(result.is_ok());
		} else {
			crate::trace::warp(false);
		}
	}

	/// `MessageBox`: reports a fatal error to the user.
	///
	/// winit has no dialog, and the port's other failure paths report to stderr (see
	/// `window::run`), so a fatal error goes there. The upstream message is a string in the
	/// local codepage, which is Shift-JIS here, so it is decoded before it is printed: the
	/// game's strings stay bytes everywhere else.
	pub fn error_dialog(&self, message: &[u8]) {
		// The upstream returned on a null message, which is an empty slice here.
		if message.is_empty() {
			return;
		}
		eprintln!("error: {}", decode(message));
	}
}

/// The error text, decoded from the game's Shift-JIS.
///
/// `font.rs` has the same conversion for the strings it draws. It is four lines, so it is
/// not worth a shared module, and this copy stops at the NUL the way the upstream
/// `MessageBox` did.
fn decode(message: &[u8]) -> String {
	let end = message.iter().position(|&b| b == 0).unwrap_or(message.len());
	encoding_rs::SHIFT_JIS.decode(&message[..end]).0.into_owned()
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::Duration;

	/// The tick count is the game's clock, so it must never go backwards and it must move.
	#[test]
	fn the_tick_count_advances() {
		let first = time_ms();

		let start = Instant::now();
		let mut last = first;
		while start.elapsed() < Duration::from_millis(50) {
			let now = time_ms();
			assert!(now >= last, "the tick count went backwards: {now} after {last}");
			last = now;
		}

		assert!(last > first, "the tick count did not advance in 50 ms");
	}

	/// The display mode has to be readable without a display, because the config decides it
	/// before the window exists.
	#[test]
	fn the_fullscreen_flag_round_trips() {
		let mut host = HostWindow::default();
		assert!(!host.is_created());
		assert!(!host.fullscreen());

		host.set_fullscreen(true);
		assert!(host.fullscreen());

		host.set_fullscreen(false);
		assert!(!host.fullscreen());

		// A window that does not exist has no client rectangle to report.
		assert_eq!(host.client_size(), None);
	}

	/// The civil conversion is the one piece of arithmetic between the clock and the log.
	#[test]
	fn the_clock_breaks_into_a_civil_date() {
		assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
		assert_eq!(civil_from_unix(946_684_800), (2000, 1, 1, 0, 0, 0));
		// A leap day, the case the March based year exists for.
		assert_eq!(civil_from_unix(951_782_400), (2000, 2, 29, 0, 0, 0));
		assert_eq!(civil_from_unix(1_700_000_000), (2023, 11, 14, 22, 13, 20));
		// A second before the epoch, so the rounding is towards the past.
		assert_eq!(civil_from_unix(-1), (1969, 12, 31, 23, 59, 59));
	}

	/// The log line needs a name and a width, whatever the platform answers.
	#[test]
	fn the_machine_is_described() {
		let env = operating_environment();
		assert!(!env.name.is_empty());
		assert!(env.bits == "64bit" || env.bits == "32bit");
	}
}
