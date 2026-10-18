//! The mission event interpreter, from the upstream `event.cpp` and `event.h`.
//!
//! An event is a chain of numbered points. Start at an entry number, and each point says
//! what to wait for and which number comes next. One line runs at most
//! [`TOTAL_EVENTFRAMESTEP`] points per frame, which is what makes a mission hold on a
//! "waiting for" point instead of running away inside a single tick.
//!
//! Upstream `EventControl` holds a `PointDataInterface*` and an `ObjectManager*`, set once
//! by `SetClass`. Here the two are parameters of [`EventControl::process_event_point`],
//! because the object manager is what the collison checks, the AI and the drawing all go
//! through and a struct holding a mutable borrow of it for the whole game would lock out
//! every other user of it. Only `nextp4` and `waitcnt` are per-line state.

use crate::core::{point_param_to_int, ENABLE_CHECKOPENXOPSEVENT, GAMEFPS};
use crate::datafile::{
	PointDataInterface, MAX_POINTMESSAGES, POINT_P1TYPE_AIPATH, POINT_P1TYPE_RAND_AIPATH,
};
use crate::objectmanager::ObjectManager;
use crate::parameter::ID_WEAPON_CASE;

/// Number of steps executed per line in one frame.
pub const TOTAL_EVENTFRAMESTEP: i32 = 6;

/// Distance for deciding "waiting for arrival" and "waiting for case".
pub const DISTANCE_CHECKPOINT: f32 = 25.0;

/// Structure for event parameters.
///
/// Upstream this is filled in by the caller before each call and read back after, because
/// the interpreter is not allowed to touch the mission variables itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EventParameter {
	/// End count.
	pub endcnt: i32,
	/// Mission success flag.
	pub complete: bool,
	/// Event message number.
	pub message_id: i32,
	/// Event message flag.
	pub set_message_id: bool,
}

/// `EventControl`: processes the event points of a mission.
///
/// This class handles one event process (flow). To process several at the same time, make
/// several, which is what the mission loader does: three lines, three machines.
pub struct EventControl {
	/// P4 to process next: identification number.
	nextp4: i16,
	/// Count for waiting time.
	waitcnt: i32,
}

impl EventControl {
	/// Constructor. Upstream takes the two classes here as well, which in Rust are per
	/// call rather than stored.
	pub fn new() -> Self {
		EventControl {
			nextp4: 0,
			waitcnt: 0,
		}
	}

	/// `Reset`: starts the flow at an entry number (-100, -110, -120 and so on).
	pub fn reset(&mut self, entry_p4: i16) {
		self.nextp4 = entry_p4;
		self.waitcnt = 0;
	}

	/// `GetNextP4`: the identification number to process next.
	pub fn get_next_p4(&self) -> i16 {
		self.nextp4
	}

	/// `CheckDead`: whether the target person is dead.
	///
	/// `reverse` inverts the condition, which makes it a survival check.
	pub fn check_dead(objects: &mut ObjectManager, target_p4: i16, reverse: bool) -> bool {
		// A target that does not exist fails the check whichever way round it is.
		let Some(human) = objects.search_human(target_p4) else {
			return false;
		};
		let dead = human.dead_flag();

		if !reverse {
			if !dead {
				return false;
			}
		} else if dead {
			return false;
		}
		true
	}

	/// `CheckArrival`: whether the target person is inside `dist_r` of a position.
	///
	/// `reverse` inverts the condition, which makes it a not-arrived check. Standing
	/// exactly on `dist_r` counts as arrived, because the comparison is strict.
	pub fn check_arrival(
		objects: &mut ObjectManager,
		pos_x: f32,
		pos_y: f32,
		pos_z: f32,
		dist_r: f32,
		target_p4: i16,
		reverse: bool,
	) -> bool {
		let Some(human) = objects.search_human(target_p4) else {
			return false;
		};
		// The rotation the position tuple also carries is not wanted here.
		let (hx, hy, hz, _) = human.pos_data();

		let x = hx - pos_x;
		let y = hy - pos_y;
		let z = hz - pos_z;
		let distance = (x * x + y * y + z * z).sqrt();

		if !reverse {
			if distance > dist_r {
				return false;
			}
		} else if distance < dist_r {
			return false;
		}
		true
	}

	/// `CheckHaveWeapon`: whether the target person owns the given weapon type.
	///
	/// `reverse` inverts the condition, which makes it a not-owned check.
	pub fn check_have_weapon(
		objects: &mut ObjectManager,
		weapon_id: i32,
		target_p4: i16,
		reverse: bool,
	) -> bool {
		// Get all owned weapon objects, as the upstream's `GetWeapon` writes them out.
		// The slots are copied out so the human borrow ends before the weapons are looked
		// up in the manager, which owns them.
		let weapon_slots = match objects.search_human(target_p4) {
			Some(human) => human.weapon().1,
			None => return false,
		};

		// false: not owned, true: owned
		let mut flag = false;
		for slot in weapon_slots.into_iter().flatten() {
			if let Some(weapon) = objects.get_weapon_object(slot) {
				// Check whether the weapon type number matches the target weapon.
				if weapon.param_data().0 == weapon_id {
					flag = true;
				}
			}
		}

		if !reverse {
			if !flag {
				return false;
			}
		} else if flag {
			return false;
		}
		true
	}

	/// `SetMovePathMode`: rewrites the movement mode of the target movement path.
	///
	/// The search finds the point by the p4 it carries, and only an AI path point (p1:3
	/// or p1:8) is rewritten. Returns whether a path was found and rewritten.
	pub fn set_move_path_mode(
		point: &mut PointDataInterface,
		target_p4: i16,
		newmode: i32,
	) -> bool {
		let (count, found) = point.search_pointdata(0x08, 0, 0, 0, target_p4, 0);
		if count > 0 {
			if let Some(mut pathdata) = found.and_then(|data| point.getdata(data.id)) {
				if pathdata.p1 == POINT_P1TYPE_AIPATH
					|| pathdata.p1 == POINT_P1TYPE_RAND_AIPATH
				{
					// The upstream writes p1 to p4 back, so p3 and p4 are rewritten with
					// the values they already carry. Only p2 moves.
					pathdata.p2 = newmode as i16;
					point.set_param(
						pathdata.id,
						pathdata.p1,
						pathdata.p2,
						pathdata.p3,
						pathdata.p4,
					);
					return true;
				}
			}
		}
		false
	}

	/// `SetTeamID`: changes the team number of the target person.
	///
	/// Returns false when no human carries the number.
	pub fn set_team_id(objects: &mut ObjectManager, target_p4: i16, team_id: i32) -> bool {
		let Some(human) = objects.search_human(target_p4) else {
			return false;
		};
		human.set_team_id(team_id);
		true
	}

	/// `CheckBreakSmallObject`: whether the target small object is destroyed.
	///
	/// `reverse` inverts the condition. A small object that does not exist counts as
	/// already destroyed, which is the case the event relies on after the object is gone.
	pub fn check_break_small_object(
		objects: &mut ObjectManager,
		target_p4: i16,
		reverse: bool,
	) -> bool {
		// false: not destroyed, true: destroyed or does not exist
		let mut flag = true;
		if let Some(small_object) = objects.search_smallobject(target_p4) {
			// The search deliberately keeps destroyed objects findable, so the flag is
			// what says whether it is broken.
			if small_object.enable_flag() {
				flag = false;
			}
		}

		if !reverse {
			if !flag {
				return false;
			}
		} else if flag {
			return false;
		}
		true
	}

	/// `WaitSec`: waits for a time, in seconds.
	///
	/// The frame rate is truncated to an integer first, so a second is 33 frames and not
	/// 33.33, and a wait of zero passes on the first call. The counter only goes back to
	/// zero when the wait ends, so the caller has to keep calling until it does.
	pub fn wait_sec(&mut self, sec: i32) -> bool {
		if (GAMEFPS as i32) * sec > self.waitcnt {
			self.waitcnt += 1;
			return false;
		}
		self.waitcnt = 0;
		true
	}

	/// `SetMessage`: asks for a message to be displayed.
	///
	/// An out of range message number still counts as a message being asked for, and the
	/// number already there stays. That is how the caller tells "no message this frame"
	/// from "message number 0".
	fn set_message(set_id: i32, message_id: &mut i32, set_message_id: &mut bool) {
		if 0 <= set_id && set_id < MAX_POINTMESSAGES as i32 {
			*message_id = set_id;
		}
		*set_message_id = true;
	}

	/// `ProcessEventPoint`: runs the event, one point at a time.
	///
	/// Returns the number of steps it executed, which is what the caller uses to see
	/// whether the line made progress. `param` is written, never read, and its
	/// `set_message_id` becomes true when a message step ran.
	pub fn process_event_point(
		&mut self,
		point: &mut PointDataInterface,
		objects: &mut ObjectManager,
		skip_flag: bool,
		param: &mut EventParameter,
	) -> i32 {
		let mut cnt = 0;

		for i in 0..TOTAL_EVENTFRAMESTEP {
			cnt = i;

			// Search for the next point. Upstream dereferences its point data pointer
			// here, so a caller that never set the class crashes rather than returning.
			let nextp4 = self.nextp4;
			let (count, found) = point.search_pointdata(0x08, 0, 0, 0, nextp4, 0);
			if count == 0 {
				return cnt;
			}
			let data = found.expect("the search counted a match");

			// Do not process if the number is invalid. With the OpenXOPS detection event
			// compiled in, point 29 is the one valid number outside 10 to 19.
			if data.p1 < 10 || 19 < data.p1 {
				if !ENABLE_CHECKOPENXOPSEVENT || data.p1 != 29 {
					return cnt;
				}
			}

			// On the first step of a skipped line, go straight to the next point.
			if cnt == 0 && skip_flag {
				self.nextp4 = data.p3;
				continue;
			}

			match data.p1 {
				10 => {
					// Mission accomplished
					param.endcnt += 1;
					param.complete = true;
					return cnt;
				}
				11 => {
					// Mission failed
					param.endcnt += 1;
					param.complete = false;
					return cnt;
				}
				12 => {
					// Waiting for death
					if !Self::check_dead(objects, data.p2, false) {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				13 => {
					// Waiting for arrival
					if !Self::check_arrival(
						objects,
						data.x,
						data.y,
						data.z,
						DISTANCE_CHECKPOINT,
						data.p2,
						false,
					) {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				14 => {
					// Change to walking. The result is ignored, so a target that is not an
					// AI path still moves the line on.
					Self::set_move_path_mode(point, data.p2, 0);
					self.nextp4 = data.p3;
				}
				15 => {
					// Waiting for small object destruction
					if !Self::check_break_small_object(objects, data.p2, false) {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				16 => {
					// Waiting for case: the arrival first, then the case in hand
					if !Self::check_arrival(
						objects,
						data.x,
						data.y,
						data.z,
						DISTANCE_CHECKPOINT,
						data.p2,
						false,
					) {
						return cnt;
					}
					if !Self::check_have_weapon(objects, ID_WEAPON_CASE, data.p2, false) {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				17 => {
					// Waiting for time
					if !self.wait_sec(point_param_to_int(data.p2) as i32) {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				18 => {
					// Message
					Self::set_message(
						point_param_to_int(data.p2) as i32,
						&mut param.message_id,
						&mut param.set_message_id,
					);
					self.nextp4 = data.p3;
				}
				19 => {
					// Team change. The team is the literal 0 upstream and not a parameter
					// of the point, so this always puts the target on team 0.
					Self::set_team_id(objects, data.p2, 0);
					self.nextp4 = data.p3;
				}
				29 => {
					// OpenXOPS check. The gate above keeps this out of reach when the
					// check is compiled out.
					if data.p2 != 0 {
						return cnt;
					}
					self.nextp4 = data.p3;
				}
				// When adding a new event point, write it here. Type number conflicts are
				// strictly forbidden, and the range check above has to be widened with it.
				_ => {}
			}
		}
		cnt
	}
}

impl Default for EventControl {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use std::path::PathBuf;

	// --- point data ------------------------------------------------------

	/// A `.pd1` image: the count, then a position and four parameters per point.
	fn pd1_image(points: &[(f32, f32, f32, i8, i8, i8, i8)]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend_from_slice(&(points.len() as u16).to_le_bytes());
		for &(x, y, z, p1, p2, p3, p4) in points {
			out.extend_from_slice(&x.to_le_bytes());
			out.extend_from_slice(&y.to_le_bytes());
			out.extend_from_slice(&z.to_le_bytes());
			out.extend_from_slice(&0.0f32.to_le_bytes());
			out.extend_from_slice(&[p1 as u8, p2 as u8, p3 as u8, p4 as u8]);
		}
		out
	}

	/// Writes a point file named after the calling test and reads it back, so two tests
	/// writing at the same time cannot collide.
	fn load_points(name: &str, points: &[(f32, f32, f32, i8, i8, i8, i8)]) -> PointDataInterface {
		let path: PathBuf =
			std::env::temp_dir().join(format!("xops_event_{}_{}", std::process::id(), name));
		fs::write(&path, pd1_image(points)).expect("write the point file");

		let mut data = PointDataInterface::new();
		assert_eq!(
			data.load_file_data(path.to_string_lossy().into_owned().into_bytes().as_slice()),
			0
		);
		data
	}

	/// A fresh parameter block, the way the caller fills it in each frame.
	fn param() -> EventParameter {
		EventParameter {
			endcnt: 0,
			complete: false,
			message_id: 0,
			set_message_id: false,
		}
	}

	/// An empty object manager. Every human check fails against it, and every small
	/// object counts as destroyed, which is the state a mission starts in.
	fn empty_objects() -> ObjectManager {
		ObjectManager::new()
	}

	// --- the wait counter ------------------------------------------------

	#[test]
	fn a_second_is_thirty_three_frames() {
		let mut event = EventControl::new();

		// The frame rate is truncated, so 33 calls are refused and the 34th passes.
		for i in 0..33 {
			assert!(!event.wait_sec(1), "call {}", i);
		}
		assert!(event.wait_sec(1));

		// Passing put the counter back, so the next wait starts over.
		assert!(!event.wait_sec(1));
	}

	#[test]
	fn two_seconds_is_sixty_six_frames() {
		let mut event = EventControl::new();
		for i in 0..66 {
			assert!(!event.wait_sec(2), "call {}", i);
		}
		assert!(event.wait_sec(2));
	}

	#[test]
	fn a_zero_second_wait_passes_at_once() {
		let mut event = EventControl::new();
		assert!(event.wait_sec(0));
	}

	#[test]
	fn reset_clears_the_wait_counter() {
		let mut event = EventControl::new();
		event.reset(-100);
		assert_eq!(event.get_next_p4(), -100);

		for _ in 0..10 {
			assert!(!event.wait_sec(1));
		}

		// A reset drops the ten frames of waiting, so the next wait starts over.
		event.reset(-110);
		assert_eq!(event.get_next_p4(), -110);
		for i in 0..33 {
			assert!(!event.wait_sec(1), "call {}", i);
		}
		assert!(event.wait_sec(1));
	}

	// --- the step loop ---------------------------------------------------

	#[test]
	fn the_step_loop_stops_at_six_points() {
		// Seven "change to walking" points in a chain, then the end of the mission. One
		// call must not reach past the sixth.
		let mut points = load_points(
			"event_six.pd1",
			&[
				(0.0, 0.0, 0.0, 14, 1, 11, 10),
				(0.0, 0.0, 0.0, 14, 1, 12, 11),
				(0.0, 0.0, 0.0, 14, 1, 13, 12),
				(0.0, 0.0, 0.0, 14, 1, 14, 13),
				(0.0, 0.0, 0.0, 14, 1, 15, 14),
				(0.0, 0.0, 0.0, 14, 1, 16, 15),
				(0.0, 0.0, 0.0, 10, 0, 0, 16),
			],
		);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();

		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			5
		);
		assert_eq!(event.get_next_p4(), 16);
		assert_eq!(eventparam.endcnt, 0);

		// The next call reaches the end of the mission on its first step.
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);
		assert_eq!(eventparam.endcnt, 1);
		assert!(eventparam.complete);
	}

	#[test]
	fn a_line_that_runs_out_of_points_reports_the_step_it_stopped_on() {
		let mut points = load_points("event_end.pd1", &[(0.0, 0.0, 0.0, 10, 0, 0, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);

		// Nothing carries 999.
		event.reset(999);
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);
		assert_eq!(event.get_next_p4(), 999);
	}

	#[test]
	fn an_invalid_point_number_is_refused() {
		for p1 in [0i8, 9, 20, 28, 30, -1] {
			let mut points = load_points(
				&format!("event_bad{}.pd1", p1),
				&[(0.0, 0.0, 0.0, p1, 0, 99, 10)],
			);
			let mut objects = empty_objects();
			let mut event = EventControl::new();
			event.reset(10);
			let mut eventparam = param();
			assert_eq!(
				event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
				0,
				"p1 {}",
				p1
			);
			assert_eq!(event.get_next_p4(), 10, "p1 {}", p1);
		}
	}

	#[test]
	fn the_openxops_check_point_depends_on_its_parameter() {
		// Point 29 is the OpenXOPS detection event, which is compiled in.
		assert!(ENABLE_CHECKOPENXOPSEVENT);

		let mut objects = empty_objects();
		let mut event = EventControl::new();
		let mut eventparam = param();

		let mut points = load_points("event_check.pd1", &[(0.0, 0.0, 0.0, 29, 1, 99, 10)]);
		event.reset(10);
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);
		assert_eq!(event.get_next_p4(), 10);

		// p2 has to be 0 for it to pass.
		let mut points = load_points("event_check0.pd1", &[(0.0, 0.0, 0.0, 29, 0, 99, 10)]);
		event.reset(10);
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(event.get_next_p4(), 99);
	}

	#[test]
	fn a_skipped_line_ignores_its_conditions_on_the_first_step() {
		// Waiting for arrival at a position nobody is near, skipped.
		let mut points = load_points("event_skip.pd1", &[(0.0, 0.0, 0.0, 13, 1, 99, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();

		// The first step is the skipped one, the second finds nothing at 99.
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, true, &mut eventparam),
			1
		);
		assert_eq!(event.get_next_p4(), 99);
	}

	/// Every point number from 10 to 19 either ends the line or moves it on, so none of
	/// them can be swallowed by the `_` arm. Against an empty object manager the three
	/// that need a live human hold the line, and the rest pass.
	#[test]
	fn every_point_number_in_range_is_handled() {
		let held = [12, 13, 16];
		for p1 in 10..=19i8 {
			let mut points = load_points(
				&format!("event_known{}.pd1", p1),
				&[(0.0, 0.0, 0.0, p1, 0, 99, 10)],
			);
			let mut objects = empty_objects();
			let mut event = EventControl::new();
			event.reset(10);
			let mut eventparam = param();
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam);

			if p1 <= 11 {
				// The mission ends.
				assert_eq!(eventparam.endcnt, 1, "point {}", p1);
			} else if held.contains(&(p1 as i32)) {
				assert_eq!(event.get_next_p4(), 10, "point {} should hold", p1);
				assert_eq!(eventparam.endcnt, 0, "point {}", p1);
			} else {
				assert_eq!(event.get_next_p4(), 99, "point {} should advance", p1);
			}
		}
	}

	#[test]
	fn failure_ends_the_line_too() {
		let mut points = load_points("event_fail.pd1", &[(0.0, 0.0, 0.0, 11, 0, 0, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		eventparam.endcnt = 4;
		eventparam.complete = true;
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);
		assert_eq!(eventparam.endcnt, 5);
		assert!(!eventparam.complete);
	}

	// --- the points that act on the mission ------------------------------

	#[test]
	fn the_message_point_writes_the_number_and_the_flag() {
		let mut points = load_points("event_message.pd1", &[(0.0, 0.0, 0.0, 18, 5, 99, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		eventparam.message_id = 7;
		// The message point is the first step, the search that follows finds nothing.
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(eventparam.message_id, 5);
		assert!(eventparam.set_message_id);
		assert_eq!(event.get_next_p4(), 99);

		// An out of range number keeps the old one and still asks for a message.
		let mut points = load_points(
			"event_message_bad.pd1",
			&[(0.0, 0.0, 0.0, 18, MAX_POINTMESSAGES as i8, 99, 10)],
		);
		event.reset(10);
		eventparam.message_id = 7;
		eventparam.set_message_id = false;
		event.process_event_point(&mut points, &mut objects, false, &mut eventparam);
		assert_eq!(eventparam.message_id, 7);
		assert!(eventparam.set_message_id);
	}

	#[test]
	fn the_change_to_walking_point_rewrites_an_ai_path() {
		let mut points = load_points(
			"event_walk.pd1",
			&[
				// The path point the event names, found by its p4 of 7, already running.
				(0.0, 0.0, 0.0, POINT_P1TYPE_AIPATH as i8, 1, 0, 7),
				// The event point itself, which searches for p4 7.
				(0.0, 0.0, 0.0, 14, 7, 99, 10),
			],
		);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(event.get_next_p4(), 99);

		// p2 is the movement mode and it is now 0, walking.
		let path = points.getdata(0).unwrap();
		assert_eq!(path.p2, 0);
		assert_eq!(path.p1, POINT_P1TYPE_AIPATH);
	}

	#[test]
	fn the_change_to_walking_point_advances_even_when_no_path_matches() {
		// The point names p4 7 and nothing carries it, so nothing is rewritten. The line
		// still moves on, because the upstream ignores the result.
		let mut points = load_points("event_walkmiss.pd1", &[(0.0, 0.0, 0.0, 14, 7, 99, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(event.get_next_p4(), 99);

		// A point that is not a path is left alone as well.
		let mut points = load_points(
			"event_walknotpath.pd1",
			&[
				(0.0, 0.0, 0.0, 5, 1, 0, 7),
				(0.0, 0.0, 0.0, 14, 7, 99, 10),
			],
		);
		event.reset(10);
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(points.getdata(0).unwrap().p2, 1);
	}

	#[test]
	fn a_path_point_is_found_by_the_first_point_that_carries_the_number() {
		// The search scans the file from the start, so a weapon point that happens to
		// carry the same number wins and the path behind it is left alone.
		let mut points = load_points(
			"event_walkfirst.pd1",
			&[
				(0.0, 0.0, 0.0, 2, 1, 0, 7),
				(0.0, 0.0, 0.0, POINT_P1TYPE_RAND_AIPATH as i8, 1, 0, 7),
				(0.0, 0.0, 0.0, 14, 7, 99, 10),
			],
		);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();
		event.process_event_point(&mut points, &mut objects, false, &mut eventparam);

		assert_eq!(points.getdata(0).unwrap().p2, 1);
		assert_eq!(points.getdata(1).unwrap().p2, 1);
	}

	#[test]
	fn the_time_point_waits_out_its_seconds() {
		let mut points = load_points("event_time.pd1", &[(0.0, 0.0, 0.0, 17, 1, 99, 10)]);
		let mut objects = empty_objects();
		let mut event = EventControl::new();
		event.reset(10);
		let mut eventparam = param();

		// One second is 33 frames, and the point is refused for each of them.
		for i in 0..33 {
			assert_eq!(
				event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
				0,
				"call {}",
				i
			);
			assert_eq!(event.get_next_p4(), 10);
		}
		assert_eq!(
			event.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(event.get_next_p4(), 99);
	}

	#[test]
	fn the_wait_counter_is_per_line() {
		// Two lines waiting at the same time do not share a count.
		let mut points = load_points("event_twolines.pd1", &[(0.0, 0.0, 0.0, 17, 1, 99, 10)]);
		let mut objects = empty_objects();
		let mut first = EventControl::new();
		let mut second = EventControl::new();
		first.reset(10);
		second.reset(10);
		let mut eventparam = param();

		for _ in 0..33 {
			first.process_event_point(&mut points, &mut objects, false, &mut eventparam);
		}
		// The first line is one call from passing, the second has not waited at all.
		assert_eq!(
			first.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			1
		);
		assert_eq!(first.get_next_p4(), 99);
		assert_eq!(
			second.process_event_point(&mut points, &mut objects, false, &mut eventparam),
			0
		);
		assert_eq!(second.get_next_p4(), 10);
	}

	// --- the checks against an empty object manager ----------------------

	/// A target that does not exist fails the check whichever way round the condition is,
	/// which is what the upstream null test does before it looks at the flag.
	#[test]
	fn a_missing_human_fails_the_human_checks() {
		let mut objects = empty_objects();
		assert!(!EventControl::check_dead(&mut objects, 1, false));
		assert!(!EventControl::check_dead(&mut objects, 1, true));
		assert!(!EventControl::check_arrival(
			&mut objects,
			0.0,
			0.0,
			0.0,
			DISTANCE_CHECKPOINT,
			1,
			false
		));
		assert!(!EventControl::check_arrival(
			&mut objects,
			0.0,
			0.0,
			0.0,
			DISTANCE_CHECKPOINT,
			1,
			true
		));
		assert!(!EventControl::check_have_weapon(&mut objects, ID_WEAPON_CASE, 1, false));
		assert!(!EventControl::check_have_weapon(&mut objects, ID_WEAPON_CASE, 1, true));
		assert!(!EventControl::set_team_id(&mut objects, 1, 0));
	}

	/// A small object that was never there counts as destroyed, because the event that
	/// waits for it to break has to pass once it is gone.
	#[test]
	fn a_missing_small_object_counts_as_destroyed() {
		let mut objects = empty_objects();
		assert!(EventControl::check_break_small_object(&mut objects, 5, false));
		assert!(!EventControl::check_break_small_object(&mut objects, 5, true));
	}

	/// Nothing is rewritten when the target is not an AI path, and the answer says so.
	#[test]
	fn set_move_path_mode_reports_what_it_did() {
		let mut point = load_points(
			"event_setmode.pd1",
			&[
				(0.0, 0.0, 0.0, POINT_P1TYPE_AIPATH as i8, 2, 0, 7),
				(0.0, 0.0, 0.0, 5, 1, 0, 8),
			],
		);
		// The first match for p4 7 is the path, and it is rewritten.
		assert!(EventControl::set_move_path_mode(&mut point, 7, 1));
		assert_eq!(point.getdata(0).unwrap().p2, 1);

		// p4 8 matches a weapon point, which is not a path.
		assert!(!EventControl::set_move_path_mode(&mut point, 8, 1));
		assert_eq!(point.getdata(1).unwrap().p2, 1);

		// Nothing carries p4 99.
		assert!(!EventControl::set_move_path_mode(&mut point, 99, 1));
	}
}
