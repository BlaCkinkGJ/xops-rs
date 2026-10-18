//! The runtime trace, switched on with `XOPS_TRACE=1`.
//!
//! Three things about this port cannot be settled by a test, because they depend on the
//! machine rather than on the code: whether the frame rate the game asks for is the one it
//! gets, whether the mouse's movement arrives at all, and whether the game takes it. A
//! report of "the mouse does nothing" and a report of "the frame rate is wrong" look
//! identical from outside, and the difference is only visible in the numbers, so the
//! numbers are printed.
//!
//! One line a second, with everything needed to tell the candidates apart:
//!
//! - `interval` is the time between one frame starting and the next. It is the actual
//!   frame rate, as opposed to the one the game is trying to run at, and a value near
//!   twice the frame step means two things are pacing the loop at once.
//! - `step` is what the game's own frame cost, measured around it. A large one means the
//!   game's work, a small one with a large interval means waiting.
//! - `motion events` and `delta` are what the platform received from the mouse device.
//!   Zero here with a mouse moving means the platform is not being told, and no fix above
//!   this layer can help.
//! - `takes` is how often the game read the movement and how much it read. Zero here with
//!   a nonzero `delta` means the movement arrived and was thrown away, which is a
//!   different bug in a different file.
//! - `looking` is the flag the game sets: a menu says no and a player says yes, and the
//!   platform gathers no movement at all while it says no.
//! - `warps` counts the cursor warps, asked for and skipped.
//! - `aim` is the player's view angle in radians, which is the end of the chain: if the
//!   delta arrives and this does not move, the game is not using what it read.
//!
//! Everything is atomic and the printing happens once a second, so a frame that takes
//! longer than a second prints one line rather than flooding.

use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// When the trace started printing, and when it last did.
static START: OnceLock<std::time::Instant> = OnceLock::new();
static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);

static ENABLED: AtomicU64 = AtomicU64::new(u64::MAX);

/// Whether the trace is on. Read from the environment once.
pub fn enabled() -> bool {
	let cached = ENABLED.load(Ordering::Relaxed);
	if cached != u64::MAX {
		return cached == 1;
	}
	let on = std::env::var_os("XOPS_TRACE").is_some_and(|value| value != "0");
	ENABLED.store(u64::from(on), Ordering::Relaxed);
	on
}

/// The counters, in the order the frame fills them.
#[derive(Default)]
struct Counters {
	frames: u64,
	interval_ms: f64,
	interval_max_ms: f64,
	step_ms: f64,
	step_max_ms: f64,
	waits: u64,
	wait_ms: f64,
	motion_events: u64,
	motion_dx: i64,
	motion_dy: i64,
	takes: u64,
	taken_dx: i64,
	taken_dy: i64,
	warps: u64,
	warps_skipped: u64,
	presented: u64,
	presented_unblended: u64,
	unmatched_draws: u64,
	shear_worst: f64,
	present_gaps: u64,
	present_gap_mean_ms: f64,
	present_gap_min_ms: f64,
	present_gap_max_ms: f64,
	present_duplicates: u64,
	present_t_mean: f64,
	present_t_max: f64,
	present_t_duplicates: u64,
	screen: &'static str,
	looking: bool,
	aim_x: f32,
	aim_y: f32,
	view: Option<(f32, f32, f32, f32)>,
}

static FRAMES: AtomicU64 = AtomicU64::new(0);
static INTERVAL_MS: AtomicU64 = AtomicU64::new(0);
static INTERVAL_MAX_MS: AtomicU64 = AtomicU64::new(0);
static STEP_MS: AtomicU64 = AtomicU64::new(0);
static STEP_MAX_MS: AtomicU64 = AtomicU64::new(0);
static WAITS: AtomicU64 = AtomicU64::new(0);
static WAIT_MS: AtomicU64 = AtomicU64::new(0);
static MOTION_EVENTS: AtomicU64 = AtomicU64::new(0);
static MOTION_DX: AtomicI64 = AtomicI64::new(0);
static MOTION_DY: AtomicI64 = AtomicI64::new(0);
static TAKES: AtomicU64 = AtomicU64::new(0);
static TAKEN_DX: AtomicI64 = AtomicI64::new(0);
static TAKEN_DY: AtomicI64 = AtomicI64::new(0);
static WARPS: AtomicU64 = AtomicU64::new(0);
static PRESENTED: AtomicU64 = AtomicU64::new(0);
static PRESENTED_UNBLENDED: AtomicU64 = AtomicU64::new(0);
/// The gaps between presented frames, which is what smoothness is: a mean of a refresh
/// with a maximum of two refreshes is smooth, and a run of near-zero gaps followed by a long
/// one is not, however many frames there were.
static PRESENT_GAP_MS: AtomicU64 = AtomicU64::new(0);
static PRESENT_GAP_MAX_MS: AtomicU64 = AtomicU64::new(0);
static PRESENT_GAP_MIN_MS: AtomicU64 = AtomicU64::new(0);
static PRESENT_GAPS: AtomicU64 = AtomicU64::new(0);
static PRESENT_DUPLICATES: AtomicU64 = AtomicU64::new(0);
static LAST_PRESENT_AT: AtomicU64 = AtomicU64::new(0);
/// The interpolation factor at each present, summed and counted, so that a factor that is
/// stuck at one end or repeating is visible.
static PRESENT_T_SUM: AtomicU64 = AtomicU64::new(0);
static PRESENT_T_MAX: AtomicU64 = AtomicU64::new(0);
static PRESENT_T_DUPLICATES: AtomicU64 = AtomicU64::new(0);
static LAST_T: AtomicU64 = AtomicU64::new(0);
/// Draws that could not be matched to the same draw a frame before, and how far from rigid
/// the worst blend of the second was.
static UNMATCHED_DRAWS: AtomicU64 = AtomicU64::new(0);
static SHEAR_WORST: AtomicU64 = AtomicU64::new(0);
static WARPS_SKIPPED: AtomicU64 = AtomicU64::new(0);
/// The screen's name, which changes rarely and is only read once a second.
static SCREEN: Mutex<&'static str> = Mutex::new("none");
static LOOKING: AtomicU64 = AtomicU64::new(0);
static AIM_X: AtomicU64 = AtomicU64::new(0);
static AIM_Y: AtomicU64 = AtomicU64::new(0);
/// The held weapon drawn in the HUD, as screen position, distance and scale.
static VIEW_SET: AtomicU64 = AtomicU64::new(0);
static VIEW_X: AtomicU64 = AtomicU64::new(0);
static VIEW_Y: AtomicU64 = AtomicU64::new(0);
static VIEW_DIST: AtomicU64 = AtomicU64::new(0);
static VIEW_SIZE: AtomicU64 = AtomicU64::new(0);

/// Milliseconds as an atomic, which is all the counters need.
fn store(cell: &AtomicU64, value: f64) {
	cell.store(value.to_bits(), Ordering::Relaxed);
}

fn load(cell: &AtomicU64) -> f64 {
	f64::from_bits(cell.load(Ordering::Relaxed))
}

/// One frame: how long the game's own step took, and how long it was since the frame
/// before it started. Both in milliseconds.
pub fn frame(step_ms: f64, interval_ms: f64) {
	if !enabled() {
		return;
	}
	FRAMES.fetch_add(1, Ordering::Relaxed);
	store(&STEP_MS, load(&STEP_MS) + step_ms);
	store(&STEP_MAX_MS, load(&STEP_MAX_MS).max(step_ms));
	if interval_ms > 0.0 && interval_ms < 10_000.0 {
		store(&INTERVAL_MS, load(&INTERVAL_MS) + interval_ms);
		store(&INTERVAL_MAX_MS, load(&INTERVAL_MAX_MS).max(interval_ms));
	}
}

/// Time spent waiting rather than working, when the host knows it.
pub fn wait(ms: f64) {
	if !enabled() {
		return;
	}
	WAITS.fetch_add(1, Ordering::Relaxed);
	store(&WAIT_MS, load(&WAIT_MS) + ms);
}

/// The mouse's own motion, as the platform received it.
pub fn motion(dx: f64, dy: f64) {
	if !enabled() {
		return;
	}
	MOTION_EVENTS.fetch_add(1, Ordering::Relaxed);
	MOTION_DX.fetch_add(dx.round() as i64, Ordering::Relaxed);
	MOTION_DY.fetch_add(dy.round() as i64, Ordering::Relaxed);
}

/// The movement the game read, which is where the platform's accumulation ends up.
pub fn movement_taken(dx: i32, dy: i32) {
	if !enabled() {
		return;
	}
	TAKES.fetch_add(1, Ordering::Relaxed);
	TAKEN_DX.fetch_add(dx as i64, Ordering::Relaxed);
	TAKEN_DY.fetch_add(dy as i64, Ordering::Relaxed);
}

/// A frame drawn for the display rather than for the game's own step.
pub fn presented() {
	if enabled() {
		PRESENTED.fetch_add(1, Ordering::Relaxed);
	}
}

/// Draws that had no counterpart in the frame before, so were not blended.
pub fn unmatched_draws(count: u64) {
	if enabled() {
		UNMATCHED_DRAWS.fetch_add(count, Ordering::Relaxed);
	}
}

/// How far from rigid the worst blend of a frame was.
pub fn blend_shear(fraction: f64) {
	if enabled() {
		store(&SHEAR_WORST, load(&SHEAR_WORST).max(fraction));
	}
}

/// A presented frame, at the time it went out, with the factor it was blended by.
///
/// The gap since the previous one is what makes the picture even or not: a present every
/// refresh is smooth, and two presents a millisecond apart followed by a long gap is a
/// stutter however high the count for the second is.
pub fn presented_at(t: f32) {
	if !enabled() {
		return;
	}
	PRESENTED.fetch_add(1, Ordering::Relaxed);

	let now = std::time::Instant::now();
	let start = *START.get_or_init(|| now);
	let millis = now.duration_since(start).as_secs_f64() * 1000.0;

	let previous = LAST_PRESENT_AT.swap(millis.to_bits(), Ordering::Relaxed);
	if previous != 0 {
		let gap = millis - f64::from_bits(previous);
		PRESENT_GAPS.fetch_add(1, Ordering::Relaxed);
		store(&PRESENT_GAP_MS, load(&PRESENT_GAP_MS) + gap);
		store(&PRESENT_GAP_MAX_MS, load(&PRESENT_GAP_MAX_MS).max(gap));
		let seen = PRESENT_GAPS.load(Ordering::Relaxed);
		if seen == 1 || gap < load(&PRESENT_GAP_MIN_MS) {
			store(&PRESENT_GAP_MIN_MS, gap);
		}
		if gap < 5.0 {
			// Two frames the display cannot tell apart, which is a frame wasted.
			PRESENT_DUPLICATES.fetch_add(1, Ordering::Relaxed);
		}
	}

	let last_t = f32::from_bits(LAST_T.swap(u64::from(t.to_bits()), Ordering::Relaxed) as u32);
	store(&PRESENT_T_SUM, load(&PRESENT_T_SUM) + t as f64);
	store(&PRESENT_T_MAX, load(&PRESENT_T_MAX).max(t as f64));
	if (t - last_t).abs() < 0.02 {
		PRESENT_T_DUPLICATES.fetch_add(1, Ordering::Relaxed);
	}
}

/// A frame drawn for the display again without being blended, which is the fallback: the
/// frame did not line up with the one before it.
pub fn presented_unblended() {
	if enabled() {
		PRESENTED_UNBLENDED.fetch_add(1, Ordering::Relaxed);
	}
}

/// A cursor warp, which the game asks for every frame, and whether it was performed.
pub fn warp(warped: bool) {
	if !enabled() {
		return;
	}
	if warped {
		WARPS.fetch_add(1, Ordering::Relaxed);
	} else {
		WARPS_SKIPPED.fetch_add(1, Ordering::Relaxed);
	}
}

/// The screen the game is running, and whether it says the mouse is looking.
pub fn screen(name: &'static str, looking: bool) {
	if !enabled() {
		return;
	}
	*SCREEN.lock().unwrap_or_else(|error| error.into_inner()) = name;
	LOOKING.store(u64::from(looking), Ordering::Relaxed);
}

/// The player's view angle, the far end of the mouse chain.
pub fn aim(rx: f32, ry: f32) {
	if !enabled() {
		return;
	}
	AIM_X.store(u64::from(rx.to_bits()), Ordering::Relaxed);
	AIM_Y.store(u64::from(ry.to_bits()), Ordering::Relaxed);
}

/// The HUD weapon's placement, which is the one draw whose position and size are computed
/// rather than placed in the world, and so the one that depends on the screen size.
pub fn viewmodel(screen_x: f32, screen_y: f32, distance: f32, size: f32) {
	if !enabled() {
		return;
	}
	VIEW_SET.store(1, Ordering::Relaxed);
	store(&VIEW_X, screen_x as f64);
	store(&VIEW_Y, screen_y as f64);
	store(&VIEW_DIST, distance as f64);
	store(&VIEW_SIZE, size as f64);
}

/// The scale half of the same record, which is only known a few lines later.
pub fn viewmodel_scale(_x: f32, _y: f32, _distance: f32, size: f32) {
	if !enabled() {
		return;
	}
	store(&VIEW_SIZE, size as f64);
}

/// Every field, read and cleared. Takes the counters, so two calls in the same second
/// report the two halves rather than the second half twice.
fn take() -> Counters {
	let frames = FRAMES.swap(0, Ordering::Relaxed);
	let interval = load(&INTERVAL_MS);
	let mut counters = Counters {
		frames,
		interval_ms: interval,
		interval_max_ms: load(&INTERVAL_MAX_MS),
		step_ms: load(&STEP_MS),
		step_max_ms: load(&STEP_MAX_MS),
		waits: WAITS.swap(0, Ordering::Relaxed),
		wait_ms: load(&WAIT_MS),
		motion_events: MOTION_EVENTS.swap(0, Ordering::Relaxed),
		motion_dx: MOTION_DX.swap(0, Ordering::Relaxed),
		motion_dy: MOTION_DY.swap(0, Ordering::Relaxed),
		takes: TAKES.swap(0, Ordering::Relaxed),
		taken_dx: TAKEN_DX.swap(0, Ordering::Relaxed),
		taken_dy: TAKEN_DY.swap(0, Ordering::Relaxed),
		warps: WARPS.swap(0, Ordering::Relaxed),
		warps_skipped: WARPS_SKIPPED.swap(0, Ordering::Relaxed),
		presented: PRESENTED.swap(0, Ordering::Relaxed),
		presented_unblended: PRESENTED_UNBLENDED.swap(0, Ordering::Relaxed),
		unmatched_draws: UNMATCHED_DRAWS.swap(0, Ordering::Relaxed),
		shear_worst: load(&SHEAR_WORST),
		present_gaps: PRESENT_GAPS.swap(0, Ordering::Relaxed),
		present_gap_mean_ms: load(&PRESENT_GAP_MS),
		present_gap_min_ms: load(&PRESENT_GAP_MIN_MS),
		present_gap_max_ms: load(&PRESENT_GAP_MAX_MS),
		present_duplicates: PRESENT_DUPLICATES.swap(0, Ordering::Relaxed),
		present_t_mean: load(&PRESENT_T_SUM),
		present_t_max: load(&PRESENT_T_MAX),
		present_t_duplicates: PRESENT_T_DUPLICATES.swap(0, Ordering::Relaxed),
		screen: *SCREEN.lock().unwrap_or_else(|error| error.into_inner()),
		looking: LOOKING.load(Ordering::Relaxed) == 1,
		aim_x: f32::from_bits(AIM_X.load(Ordering::Relaxed) as u32),
		aim_y: f32::from_bits(AIM_Y.load(Ordering::Relaxed) as u32),
		view: if VIEW_SET.swap(0, Ordering::Relaxed) == 1 {
			Some((
				load(&VIEW_X) as f32,
				load(&VIEW_Y) as f32,
				load(&VIEW_DIST) as f32,
				load(&VIEW_SIZE) as f32,
			))
		} else {
			None
		},
	};
	for cell in [
		&INTERVAL_MS,
		&INTERVAL_MAX_MS,
		&STEP_MS,
		&STEP_MAX_MS,
		&WAIT_MS,
		&PRESENT_GAP_MS,
		&PRESENT_GAP_MAX_MS,
		&PRESENT_GAP_MIN_MS,
		&PRESENT_T_SUM,
		&PRESENT_T_MAX,
		&SHEAR_WORST,
	] {
		store(cell, 0.0);
	}
	counters.interval_ms = if frames == 0 {
		0.0
	} else {
		counters.interval_ms / frames as f64
	};
	counters.step_ms = if frames == 0 {
		0.0
	} else {
		counters.step_ms / frames as f64
	};
	counters.wait_ms = if counters.waits == 0 {
		0.0
	} else {
		counters.wait_ms / counters.waits as f64
	};
	counters.present_t_mean = if counters.presented == 0 {
		0.0
	} else {
		counters.present_t_mean / counters.presented as f64
	};
	counters.present_gap_mean_ms = if counters.present_gaps == 0 {
		0.0
	} else {
		counters.present_gap_mean_ms / counters.present_gaps as f64
	};
	counters
}

/// Prints the last second, at most once a second. Called once a frame.
///
/// The rate limit is the whole point: called once per frame this would print the frame
/// that just ran and report `1 fps` whatever the real rate is, which is exactly the kind
/// of number that sends the next hour in the wrong direction.
pub fn report() {
	if !enabled() {
		return;
	}
	// At most one line a second, which is what makes `frames` a frame rate rather than a
	// count of the single frame that led here.
	let now = std::time::Instant::now();
	let start = *START.get_or_init(|| now);
	{
		let mut last = LAST.lock().unwrap_or_else(|error| error.into_inner());
		match *last {
			Some(previous) if now.duration_since(previous) < std::time::Duration::from_secs(1) => {
				return;
			}
			_ => *last = Some(now),
		}
	}

	let counters = take();
	if counters.frames == 0 && counters.motion_events == 0 && counters.takes == 0 {
		return;
	}
	let seconds = now.duration_since(start).as_secs_f64();

	eprintln!(
		"xops: t={:.1}s | {} steps | presented {} ({} unblended) | interval {:.1}ms (max {:.0}) | step {:.1}ms (max {:.0}) | wait {:.1}ms x{} | \
		 mouse: {} motion events delta({},{}) | {} takes delta({},{}) | looking {} | warps {} ({} skipped) | \
		 aim {:.3},{:.3} | screen {}",
		seconds,
		counters.frames,
		counters.presented,
		counters.presented_unblended,
		counters.interval_ms,
		counters.interval_max_ms,
		counters.step_ms,
		counters.step_max_ms,
		counters.wait_ms,
		counters.waits,
		counters.motion_events,
		counters.motion_dx,
		counters.motion_dy,
		counters.takes,
		counters.taken_dx,
		counters.taken_dy,
		if counters.looking { "yes" } else { "no" },
		counters.warps,
		counters.warps_skipped,
		counters.aim_x,
		counters.aim_y,
		counters.screen,
	);
	if counters.unmatched_draws > 0 || counters.shear_worst > 0.01 {
		eprintln!(
			"xops:   draws: {} unmatched, worst blend {}% away from rigid",
			counters.unmatched_draws,
			counters.shear_worst * 100.0
		);
	}
	if counters.present_gaps > 0 {
		eprintln!(
			"xops:   present gaps: mean {:.2}ms min {:.2} max {:.2} over {}, {} under 5ms | t mean {:.2} max {:.2}, {} repeats",
			counters.present_gap_mean_ms,
			counters.present_gap_min_ms,
			counters.present_gap_max_ms,
			counters.present_gaps,
			counters.present_duplicates,
			counters.present_t_mean,
			counters.present_t_max,
			counters.present_t_duplicates,
		);
	}
	if let Some((x, y, distance, size)) = counters.view {
		eprintln!(
			"xops:   hud weapon: screen ({x:.0},{y:.0}) at {distance:.2} units, scale {size:.3}"
		);
	}
}

/// A line that is not a per-second report, for the startup facts.
pub fn note(text: &str) {
	if enabled() {
		eprintln!("xops: {text}");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_counters_add_up_and_are_cleared() {
		// Off by default, so nothing is counted until the test turns it on.
		ENABLED.store(1, Ordering::Relaxed);
		let _ = take();

		frame(1.0, 30.0);
		frame(3.0, 62.0);
		motion(10.0, -4.0);
		movement_taken(10, -4);
		warp(true);
		warp(false);

		let counters = take();
		assert_eq!(counters.frames, 2);
		assert_eq!(counters.step_ms, 2.0, "the mean of 1 and 3");
		assert_eq!(counters.step_max_ms, 3.0);
		assert_eq!(counters.interval_ms, 46.0, "the mean of 30 and 62");
		assert_eq!(counters.interval_max_ms, 62.0);
		assert_eq!(counters.motion_events, 1);
		assert_eq!((counters.motion_dx, counters.motion_dy), (10, -4));
		assert_eq!(counters.takes, 1);
		assert_eq!((counters.taken_dx, counters.taken_dy), (10, -4));
		assert_eq!(counters.warps, 1);
		assert_eq!(counters.warps_skipped, 1);

		// Read once, so the second read is empty.
		let again = take();
		assert_eq!(again.frames, 0);
		assert_eq!(again.motion_events, 0);
		ENABLED.store(u64::MAX, Ordering::Relaxed);
	}
}
