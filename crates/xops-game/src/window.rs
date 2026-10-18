//! The window, from the upstream `window.cpp` and `window.h`.
//!
//! Upstream `WindowControl` is a file scope global (`WindowControl MainWindow` in
//! `main.cpp`) that every translation unit reaches for, and the rest of `window.cpp` is a
//! handful of free functions the game calls by name. Here the class is a value the caller
//! owns and passes, the way `DebugLog` is, and the free functions stay free.
//!
//! What is not here is the Win32 plumbing: the window class, `WndProc`, the message pump,
//! `CreateWindow` and the `HWND` that the input, sound and graphics layers used to ask for.
//! winit owns all of it, in `xops_platform::window`, and `xops_platform::host` is the part
//! of the machine the game still reaches for.

use std::sync::atomic::{AtomicI32, AtomicU32, Ordering};
use std::time::Duration;

use xops_platform::host::{self, HostWindow};

use crate::core::GAMEFRAMEMS;
use crate::debug::{DebugLog, LOG_CHECK};

/// The full width colon the environment line is written with, Shift-JIS. The timestamp
/// cell of the log uses the same two bytes, and the log is bytes, so they are kept.
const SJIS_COLON: &[u8] = b"\x81\x46";

/// The game's window, the upstream `WindowControl`.
///
/// The window itself belongs to the host, so this holds the host's handle to it rather than
/// an `HWND`, and it starts without one: upstream ran `ErrorInfo` before `InitWindow`.
pub struct WindowControl {
	host: HostWindow,
	/// Set by [`WindowControl::close_window`], read by the frame loop.
	closing: bool,
}

impl Default for WindowControl {
	fn default() -> WindowControl {
		WindowControl::new()
	}
}

impl WindowControl {
	/// A window control whose window does not exist yet.
	///
	/// Upstream `SetParam` stored the instance handle and the show state, which winit fills
	/// in itself: the instance is the process, and a winit window is shown when it is
	/// created. There is nothing left to pass.
	pub fn new() -> WindowControl {
		WindowControl {
			host: HostWindow::default(),
			closing: false,
		}
	}

	/// `InitWindow`: puts the title on the window the host created and sets the mode.
	///
	/// Upstream this registered the window class and created the window from it, at the
	/// requested client size, and answered false when the title was null, the size was not
	/// positive or the class or the window could not be created. winit only creates windows
	/// from inside the event loop, so the window arrives here already made
	/// ([`xops_platform::window::run`]) and what is left is the title and the display mode.
	/// The size and the creation failures are the caller's, which asks the event loop for
	/// the window at the config's size, so the only failure left here is an empty title.
	pub fn init_window(&mut self, host: HostWindow, title: &str, fullscreen: bool) -> bool {
		if title.is_empty() {
			return false;
		}

		self.host = host;
		self.host.set_title(title);
		self.host.set_fullscreen(fullscreen);
		true
	}

	/// `ChangeWindowMode`: fullscreen (true) or windowed (false).
	///
	/// False when there is no window, as upstream, which refused the switch on a null
	/// handle.
	pub fn change_window_mode(&mut self, fullscreen: bool) -> bool {
		if !self.host.is_created() {
			return false;
		}

		self.host.set_fullscreen(fullscreen);
		true
	}

	/// `ErrorInfo`: reports an error to the user.
	pub fn error_info(&self, str: &[u8]) {
		self.host.error_dialog(str);
	}

	/// `SetCursorPos` and the client-to-screen step, in client pixels.
	///
	/// This is what `input.cpp`'s `MoveMouseCenter` needs from the window, and the only
	/// window operation the input layer has left. The upstream asked the window for its
	/// handle and called the two Win32 functions itself, which is why upstream's input class
	/// held a pointer to this one. A window that cannot warp the cursor (a Wayland one) is
	/// silent here, as the upstream ignored `SetCursorPos`'s result.
	pub fn set_cursor_position(&self, x: i32, y: i32) {
		self.host.set_cursor_position(x as f64, y as f64);
	}

	/// `CloseWindow`: asks the game to end.
	///
	/// Upstream posted `WM_CLOSE` to the window and the message pump turned that into
	/// `WM_QUIT`, which ended the main loop. winit will not let anyone but the event loop
	/// close a window, so the request is recorded and the frame loop reads
	/// [`WindowControl::close_requested`] and ends the run the same way it ends it when the
	/// user closes the window.
	pub fn close_window(&mut self) {
		self.closing = true;
	}

	/// Whether the game has asked to end, [`WindowControl::close_window`].
	pub fn close_requested(&self) -> bool {
		self.closing
	}
}

/// `GetTimeMS`: milliseconds, which is the platform's clock.
pub fn get_time_ms() -> u32 {
	host::time_ms()
}

/// `GetTimeName`: writes the date and time as `YYYYMMDDhhmmss` into the caller's buffer.
///
/// The buffer is the caller's, as upstream, because the screenshot path appends its own
/// extension to the name. The upstream `%04d%02d%02d%02d%02d%02d` is built as digits
/// directly: there is no formatting to do and nothing to allocate.
pub fn get_time_name(str: &mut [u8]) {
	if str.is_empty() {
		return;
	}

	let (year, month, day, hour, minute, second) = host::time_parts();

	let mut text = [b'0'; 14];
	let mut at = 0;
	for (value, width) in [
		(year as u32, 4usize),
		(month, 2),
		(day, 2),
		(hour, 2),
		(minute, 2),
		(second, 2),
	] {
		let mut value = value;
		for slot in (0..width).rev() {
			text[at + slot] = b'0' + (value % 10) as u8;
			value /= 10;
		}
		at += width;
	}

	// A buffer that cannot hold the name is filled as far as it goes and stays terminated,
	// where the upstream `sprintf` would have written past its end.
	let len = text.len().min(str.len() - 1);
	str[..len].copy_from_slice(&text[..len]);
	str[len] = 0;
}

thread_local! {
	/// The generator's state, the upstream `srand` seed.
	///
	/// Thread local rather than a process global, which is a change from the upstream and
	/// one the game cannot observe: it runs its whole loop on one thread, so a single
	/// sequence is still a single sequence. What it fixes is the tests. They run in
	/// parallel and the AI's behaviour depends on its draws, so with one shared state every
	/// test that exercises the AI perturbs every other one, and a test that passes alone
	/// fails in the suite. Each thread now starts from zero, which is deterministic, and
	/// seeds itself with `init_rand` where a varied sequence is wanted.
	static RAND_STATE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// One step of the Microsoft C runtime's `rand()`, which is the generator the game was
/// built against: the port produces the sequence the upstream did rather than the host's.
///
/// Returns the new state and the value it yields for `num`, which is the runtime's
/// `rand() % num`.
fn rand_next(state: u32, num: i32) -> (u32, i32) {
	let next = state.wrapping_mul(214_013).wrapping_add(2_531_011);
	(next, (((next >> 16) & 0x7fff) as i32) % num)
}

/// `InitRand`: seeds the generator from the tick count.
pub fn init_rand() {
	set_rand_state(get_time_ms());
}

/// Seeds the generator with a known value.
///
/// `init_rand` seeds from the clock, which is what the upstream does and what makes the
/// game's own sequence vary between runs. A test that depends on the sequence wants a
/// fixed seed instead, so that a failure reproduces.
pub fn set_rand_state(state: u32) {
	RAND_STATE.with(|seed| seed.set(state));
}

/// `GetRand`: a value from 0 to `num - 1`, and 0 for a range that has no room.
pub fn get_rand(num: i32) -> i32 {
	if num <= 0 {
		return 0;
	}

	RAND_STATE.with(|seed| {
		let (next, value) = rand_next(seed.get(), num);
		seed.set(next);
		value
	})
}

/// `GetFps`: the frame rate over the last `getcnt` frames.
///
/// The 30 ms frame is the game's, so a rate the display cannot reach reads as the rate the
/// world is stepped at, which is what the debug display wants.
pub fn get_fps(getcnt: i32) -> f32 {
	if getcnt <= 0 {
		return 0.0;
	}

	let count = FPS_COUNT.load(Ordering::Relaxed);
	if count == 0 {
		FPS_PTIME.store(get_time_ms(), Ordering::Relaxed);
	}

	if count == getcnt {
		let interval = get_time_ms().wrapping_sub(FPS_PTIME.load(Ordering::Relaxed));
		FPS_VALUE.store(fps_from(interval, getcnt).to_bits(), Ordering::Relaxed);
		FPS_COUNT.store(0, Ordering::Relaxed);
	} else {
		FPS_COUNT.store(count + 1, Ordering::Relaxed);
	}

	f32::from_bits(FPS_VALUE.load(Ordering::Relaxed))
}

/// The tick the last frame rate was measured from, the upstream `static unsigned int
/// ptimeg`.
static FPS_PTIME: AtomicU32 = AtomicU32::new(0);
/// The frame this measurement is on, the upstream `static int cnt`.
static FPS_COUNT: AtomicI32 = AtomicI32::new(0);
/// The last rate, as `f32` bits: the upstream kept it in a function scope static and the
/// port has no `static mut`.
static FPS_VALUE: AtomicU32 = AtomicU32::new(0);

/// The upstream rate: `1000 / (ms / frames)`, with the interval divided in whole
/// milliseconds first, so a frame under a millisecond long reads as an infinite rate.
fn fps_from(interval_ms: u32, frames: i32) -> f32 {
	1000.0 / (interval_ms / frames as u32) as f32
}

/// The tick the last frame was paced from, the upstream `static unsigned int ptimec`.
static CTRL_PTIME: AtomicU32 = AtomicU32::new(0);

/// `ControlFps`: waits out what is left of the frame's `GAMEFRAMEMS`.
///
/// True when it waited, false when the frame has already had its time. The upstream asked
/// for a 1 ms timer with `timeBeginPeriod`, which has no counterpart in the standard
/// library and was commented out around the sleep anyway.
pub fn control_fps() -> bool {
	let nowtime = get_time_ms();

	if let Some(waittime) = frame_wait(nowtime, CTRL_PTIME.load(Ordering::Relaxed)) {
		std::thread::sleep(Duration::from_millis(waittime as u64));
		CTRL_PTIME.store(get_time_ms(), Ordering::Relaxed);
		return true;
	}

	CTRL_PTIME.store(nowtime, Ordering::Relaxed);
	false
}

/// How long is left of the frame's time, or `None` when the frame is already due.
///
/// The subtraction is the upstream's `unsigned int` one, so a tick that is behind wraps
/// past `GAMEFRAMEMS` and the frame is paced from now rather than waiting a whole wrap.
fn frame_wait(now: u32, last: u32) -> Option<u32> {
	let waittime = GAMEFRAMEMS.wrapping_sub(now.wrapping_sub(last));
	if waittime > 0 && waittime <= GAMEFRAMEMS {
		Some(waittime)
	} else {
		None
	}
}

/// `Round`: half away from zero.
///
/// The upstream is `floorf(r + 0.5f)` above zero and `ceilf(r - 0.5f)` below it, which is
/// what `f32::round` computes, the unused NaN and infinity cases included.
pub fn round(r: f32) -> f32 {
	r.round()
}

/// `GetOperatingEnvironment`: records the machine in the debug log.
///
/// The upstream wrote a second line with `GetSystemDefaultLangID` and
/// `GetUserDefaultLangID`. Those are Win32 calls with no portable counterpart, and the
/// standard library has no locale at all, so the language line is not written.
pub fn get_operating_environment(log: &mut DebugLog) {
	let env = host::operating_environment();

	// The upstream format is `OS Name`, a full width colon in Shift-JIS, then the three
	// fields, all on one line. The log is bytes, so the bytes are built here.
	let mut text = b"OS Name".to_vec();
	text.extend_from_slice(SJIS_COLON);
	text.extend_from_slice(env.name.as_bytes());
	text.push(b' ');
	text.extend_from_slice(env.arch.as_bytes());
	text.extend_from_slice(b" (");
	text.extend_from_slice(env.bits.as_bytes());
	text.push(b')');

	log.write_log(LOG_CHECK, b"Environment", &text);
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::MAX_PATH;
	use std::path::PathBuf;

	/// The name the upstream formats, for a set of parts.
	fn name_of(parts: (i32, u32, u32, u32, u32, u32)) -> String {
		let (year, month, day, hour, minute, second) = parts;
		format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
	}

	/// A path in the temporary directory for the log test.
	fn temp_path(name: &str) -> PathBuf {
		std::env::temp_dir().join(format!("xops-window-{}-{}.html", std::process::id(), name))
	}

	/// The name is a screenshot's file name, so it has to be the clock's digits.
	#[test]
	fn the_time_name_is_the_clock_in_digits() {
		let mut str = [0u8; MAX_PATH];

		// The clock can tick between the two readings, so either side of the call is
		// acceptable and nothing else is.
		let before = host::time_parts();
		get_time_name(&mut str);
		let after = host::time_parts();

		let name = &str[..14];
		assert!(
			name == name_of(before).as_bytes() || name == name_of(after).as_bytes(),
			"the name is not the clock: {:?}",
			str
		);
		assert_eq!(str[14], 0);
	}

	/// The upstream wrote into a `char[MAX_PATH]` without a bound; this must not.
	#[test]
	fn a_short_buffer_is_still_terminated() {
		let mut small = [0xffu8; 4];
		get_time_name(&mut small);
		assert_eq!(small[3], 0);
		assert_ne!(small[..3], [0xff; 3]);

		let mut empty = [0u8; 0];
		get_time_name(&mut empty);
	}

	/// The sequence has to be the runtime's, which is what the upstream called.
	#[test]
	fn the_random_sequence_is_the_runtime() {
		// The first step from state 1, and the value it yields: `2745024 >> 16` masked is
		// 41, and 41 % 16 is 9.
		assert_eq!(rand_next(1, 16), (2_745_024, 9));
	}

	/// A range that has no room answers zero, and the generator is left alone.
	#[test]
	fn a_range_without_room_answers_zero() {
		assert_eq!(get_rand(0), 0);
		assert_eq!(get_rand(-5), 0);
	}

	#[test]
	fn get_rand_stays_in_range() {
		init_rand();
		for _ in 0..64 {
			assert!((0..16).contains(&get_rand(16)));
		}
	}

	/// The frame rate divides the interval in whole milliseconds, as the upstream did.
	#[test]
	fn the_frame_rate_is_the_upstream_arithmetic() {
		assert_eq!(fps_from(300, 10), 1000.0 / 30.0);
		assert_eq!(fps_from(2_000, 100), 1000.0 / 20.0);
		assert_eq!(get_fps(0), 0.0);
		// Under a millisecond per frame the division is by zero, which is an infinite rate.
		assert_eq!(fps_from(5, 10), f32::INFINITY);
	}

	/// The pace has to skip the wait once the frame is due, wrap included.
	#[test]
	fn the_frame_wait_is_skipped_once_the_frame_is_due() {
		assert_eq!(frame_wait(1_000, 1_000), Some(GAMEFRAMEMS));
		assert_eq!(frame_wait(1_010, 1_000), Some(20));
		assert_eq!(frame_wait(1_030, 1_000), None);
		assert_eq!(frame_wait(1_040, 1_000), None);
		// A tick count that wrapped, which the upstream's subtraction also survived: 21
		// ticks to the wrap and 5 past it.
		assert_eq!(frame_wait(5, u32::MAX - 20), Some(4));
	}

	/// A window that does not exist takes the title and refuses the mode, as upstream.
	#[test]
	fn the_window_control_starts_without_a_window() {
		let mut control = WindowControl::new();
		assert!(!control.change_window_mode(true));
		assert!(!control.init_window(HostWindow::default(), "", false));
	}

	/// Closing is a request the frame loop reads, not a message to the window.
	#[test]
	fn closing_is_recorded() {
		let mut control = WindowControl::new();
		assert!(!control.close_requested());

		control.close_window();
		assert!(control.close_requested());
	}

	/// `Round` is half away from zero, on both sides of it.
	#[test]
	fn round_is_half_away_from_zero() {
		assert_eq!(round(2.4), 2.0);
		assert_eq!(round(2.5), 3.0);
		assert_eq!(round(-2.4), -2.0);
		assert_eq!(round(-2.5), -3.0);
		assert_eq!(round(0.0), 0.0);
	}

	/// The environment line has to reach the log, colon and all: it is the only line the
	/// startup writes about the machine.
	#[test]
	fn the_environment_line_reaches_the_log() {
		let path = temp_path("env");
		let _ = std::fs::remove_file(&path);

		let mut log = DebugLog::new();
		log.set_log_name(&path);
		log.enable_output_log();
		log.make_log();
		get_operating_environment(&mut log);
		drop(log);

		let text = xops_platform::fs::read_file(&path).expect("the log was written");
		let _ = std::fs::remove_file(&path);

		let env = host::operating_environment();
		let mut needle = b"OS Name".to_vec();
		needle.extend_from_slice(SJIS_COLON);
		needle.extend_from_slice(env.name.as_bytes());

		assert!(
			text.windows(needle.len()).any(|window| window == needle.as_slice()),
			"the environment line is missing from the log"
		);
	}
}
