//! `HumanMotionControl`, from `object.cpp:3234-3563` and `object.h:355-400`.
//!
//! The animation controller the `human` class owns. It decides which limb model is drawn
//! and at what angle, from the movement flags and the weapon state, and it is what makes
//! walking look like walking: the leg model walks through a fixed twelve or eight frame
//! cycle while the arms are swung by the reaction angle a shot leaves behind.
//!
//! Upstream this is a `class HumanMotionControl *MotionCtrl` owned by `human`. Nothing
//! else refers to it, so it is owned by value here.

use std::f32::consts::PI;

use crate::core::{angle_normalization, degree_to_radian};
use crate::human::get_flag;
use crate::object::{move_flag, ARMRAD_NOWEAPON, ARMRAD_RELOADWEAPON};
use crate::parameter::{ParameterInfo, ID_WEAPON_GRENADE, ID_WEAPON_NONE};
use crate::resource::{TOTAL_ARMMODE, TOTAL_RUNMODE, TOTAL_WALKMODE};

/// `HumanMotionControl`.
#[derive(Clone, Debug)]
pub struct HumanMotionControl {
	/// The class that manages the setting values.
	///
	/// Upstream a pointer, left null until `SetParameterInfoClass` is called. The tables
	/// are `const` items in this port and the class is a zero-sized handle, so holding one
	/// costs nothing and there is no pointer to keep in sync.
	param: ParameterInfo,
	id_upmodel: i32,
	id_armmodel: [i32; TOTAL_ARMMODE],
	/// leg (idle)
	id_legmodel: i32,
	/// leg (walking)
	id_walkmodel: [i32; TOTAL_WALKMODE],
	/// leg (running)
	id_runmodel: [i32; TOTAL_RUNMODE],
	/// Relative angle for raising and lowering the arm.
	reaction_y: f32,
	/// Slow down the raising and lowering of the arm.
	slowarm: bool,
	armmodel_rotation_y: f32,
	legrotation_x: f32,
	/// Walking motion count.
	walkcnt: i32,
	/// Running motion count.
	runcnt: i32,
	/// Arm model to draw.
	armmodelid: i32,
	/// Leg model to draw.
	legmodelid: i32,
}

impl HumanMotionControl {
	/// Constructor. Upstream takes a `ParameterInfo` that the only call site leaves null.
	pub fn new() -> HumanMotionControl {
		HumanMotionControl {
			param: ParameterInfo::new(),
			id_upmodel: -1,
			id_armmodel: [-1; TOTAL_ARMMODE],
			id_legmodel: -1,
			id_walkmodel: [-1; TOTAL_WALKMODE],
			id_runmodel: [-1; TOTAL_RUNMODE],
			reaction_y: 0.0,
			slowarm: false,
			armmodel_rotation_y: 0.0,
			legrotation_x: 0.0,
			walkcnt: 0,
			runcnt: 0,
			armmodelid: -1,
			legmodelid: -1,
		}
	}

	/// Registers the class that manages the setting values.
	///
	/// Upstream warns this has to run before the class is used, because the pointer starts
	/// null.
	pub fn set_parameter_info_class(&mut self, param: ParameterInfo) {
		self.param = param;
	}

	/// Sets the model data. The array counts are [`TOTAL_ARMMODE`], [`TOTAL_WALKMODE`] and
	/// [`TOTAL_RUNMODE`] and the arrays keep their upstream bounds.
	pub fn set_model(
		&mut self,
		upmodel: i32,
		armmodel: [i32; TOTAL_ARMMODE],
		legmodel: i32,
		walkmodel: [i32; TOTAL_WALKMODE],
		runmodel: [i32; TOTAL_RUNMODE],
	) {
		self.id_upmodel = upmodel;
		self.id_armmodel = armmodel;
		self.id_legmodel = legmodel;
		self.id_walkmodel = walkmodel;
		self.id_runmodel = runmodel;
	}

	/// `Init`. The leg angle starts at the rotation the human was placed with, which is
	/// what the human passes as `rotation_x`.
	pub fn init(&mut self, rx: f32) {
		self.reaction_y = 0.0;
		self.slowarm = false;
		self.armmodel_rotation_y = 0.0;
		self.legrotation_x = rx;
		self.walkcnt = 0;
		self.runcnt = 0;
		self.armmodelid = -1;
		self.legmodelid = -1;
	}

	/// `PickupWeapon`. The type number is unused upstream, and kept so the call sites read
	/// against the original.
	pub fn pickup_weapon(&mut self, _weapon_paramid: i32) {
		// Set the arm angle (recoil).
		self.reaction_y = ARMRAD_RELOADWEAPON;
		self.slowarm = true;
	}

	/// `ChangeHaveWeapon`. The type number is unused upstream.
	pub fn change_have_weapon(&mut self, _weapon_paramid: i32) {
		self.reaction_y = ARMRAD_RELOADWEAPON;
		self.slowarm = true;
	}

	/// `ChangeWeaponID`. A dummy function upstream, kept for expansion.
	pub fn change_weapon_id(&mut self, _weapon_paramid: i32) {}

	/// `ShotWeapon`: pass the recoil to the arm.
	///
	/// A grenade always throws the arm up by 20 degrees. Everything else takes the angle
	/// from the scope settings, or from the weapon's own reaction when those are negative.
	/// When the weapon has no settings the upstream reads uninitialised memory and returns
	/// without touching `slowarm`, and leaving the arm as it was is the defined version of
	/// that.
	pub fn shot_weapon(&mut self, weapon_paramid: i32) {
		if weapon_paramid == ID_WEAPON_GRENADE {
			self.reaction_y = degree_to_radian(20.0);
		} else {
			let Some(paramdata) = self.param.get_weapon(weapon_paramid) else {
				return;
			};
			// `GetScopeParam` answers for every id of 0 or more, and the table stores the
			// scope mode of every weapon, so this cannot be the missing case.
			let Some(sparam) = self.param.get_scope_param(paramdata.scopemode) else {
				return;
			};

			if sparam.weapon_motion_reaction_y >= 0.0 {
				self.reaction_y = sparam.weapon_motion_reaction_y;
			} else {
				self.reaction_y = degree_to_radian(0.5) * paramdata.reaction as f32;
			}
		}
		self.slowarm = false;
	}

	/// `ReloadWeapon`. A dummy function upstream, kept for expansion.
	pub fn reload_weapon(&mut self, _weapon_paramid: i32) {}

	/// `DumpWeapon`. A dummy function upstream, kept for expansion.
	pub fn dump_weapon(&mut self) {}

	/// `Jump`. A dummy function upstream, kept for expansion.
	pub fn jump(&mut self) {}

	/// `ProcessObject`: the motion calculation, run once per frame from the human's own
	/// `ProcessObject`.
	///
	/// `move_flag` is the human's `MoveFlag_lt`, the flags of the frame that just ended,
	/// and `hp` decides whether the legs animate at all.
	pub fn process_object(
		&mut self,
		rotation_x: f32,
		armrotation_y: f32,
		weapon_paramid: i32,
		reload_cnt: i32,
		change_weapon_id_cnt: i32,
		move_flag: i32,
		hp: i32,
		player_flag: bool,
	) {
		let mut arm_model_id;

		if !self.slowarm {
			// Recoil from firing and so on. It halves towards zero each frame, and below a
			// hundredth of a degree it is over.
			if self.reaction_y > 0.0 {
				if self.reaction_y > degree_to_radian(0.01) {
					self.reaction_y *= 0.5;
				} else {
					self.reaction_y = 0.0;
				}
			}
			if self.reaction_y < 0.0 {
				if self.reaction_y < degree_to_radian(-0.01) {
					self.reaction_y *= 0.5;
				} else {
					self.reaction_y = 0.0;
				}
			}
		} else {
			// A pickup or a switch raises the arm slowly, two degrees a frame, and stops
			// once it is within two degrees of the aim.
			if (degree_to_radian(-2.0) < self.reaction_y)
				&& (self.reaction_y < degree_to_radian(2.0))
			{
				self.reaction_y = 0.0;
				self.slowarm = false;
			}
			if self.reaction_y > 0.0 {
				self.reaction_y += degree_to_radian(-2.0);
			}
			if self.reaction_y < 0.0 {
				self.reaction_y += degree_to_radian(2.0);
			}
		}

		// If reloading, set the arm angle again.
		if reload_cnt > 0 {
			self.reaction_y = ARMRAD_RELOADWEAPON;
		}

		// If the weapon ID is being switched, set the arm angle again.
		if change_weapon_id_cnt > 0 {
			self.reaction_y = ARMRAD_RELOADWEAPON;
		}

		if weapon_paramid == ID_WEAPON_NONE {
			// empty handed
			if player_flag {
				self.armmodel_rotation_y = ARMRAD_NOWEAPON;
			} else {
				// An AI looks where the human looks.
				self.armmodel_rotation_y = armrotation_y;
			}
			arm_model_id = 0;
		} else {
			// Some weapon is held. The upstream leaves the arm angle and the model at these
			// two values when the weapon has no settings, having read them out of an
			// uninitialised `WeaponParameter`, so this is the defined version of that.
			self.armmodel_rotation_y = 0.0;
			arm_model_id = 0;

			if let Some(paramdata) = self.param.get_weapon(weapon_paramid) {
				// Determine the arm shape and angle.
				if paramdata.weapon_p == 0 {
					self.armmodel_rotation_y = armrotation_y + self.reaction_y;
					arm_model_id = 1;
				}
				if paramdata.weapon_p == 1 {
					self.armmodel_rotation_y = armrotation_y + self.reaction_y;
					arm_model_id = 2;
				}
				if paramdata.weapon_p == 2 {
					self.armmodel_rotation_y = ARMRAD_NOWEAPON;
					arm_model_id = 0;
				}
			}
		}

		// Determine the travel direction. These are the same nine directions the human's
		// ControlProcess resolves, and the walk flag wins over all of them.
		let direction =
			move_flag & (move_flag::FORWARD | move_flag::BACK | move_flag::LEFT | move_flag::RIGHT);
		let move_rx = if get_flag(move_flag, move_flag::WALK) {
			degree_to_radian(0.0)
		} else if direction == move_flag::FORWARD {
			degree_to_radian(0.0)
		} else if direction == move_flag::BACK {
			degree_to_radian(180.0)
		} else if direction == move_flag::LEFT {
			degree_to_radian(90.0)
		} else if direction == move_flag::RIGHT {
			degree_to_radian(-90.0)
		} else if direction == (move_flag::FORWARD | move_flag::LEFT) {
			degree_to_radian(45.0)
		} else if direction == (move_flag::BACK | move_flag::LEFT) {
			degree_to_radian(135.0)
		} else if direction == (move_flag::BACK | move_flag::RIGHT) {
			degree_to_radian(-135.0)
		} else if direction == (move_flag::FORWARD | move_flag::RIGHT) {
			degree_to_radian(-45.0)
		} else {
			0.0
		};

		// The legs only follow the travel direction while the human is alive.
		if hp <= 0 {
			self.legrotation_x = rotation_x;
		} else {
			// A leg that points away from the body is drawn the other way round, so the
			// model never faces backwards.
			let move_rx2 = if move_rx.abs() > degree_to_radian(90.0) {
				move_rx + PI
			} else {
				move_rx
			};
			let move_rx2 = angle_normalization(move_rx2);

			// Gradually toward that orientation.
			self.legrotation_x = self.legrotation_x * 0.85 + (rotation_x + move_rx2 * -1.0) * 0.15;
		}

		// Set the arm model.
		self.armmodelid = self.id_armmodel[arm_model_id as usize];

		// Set the leg model: standing still unless the human is alive and moving.
		self.legmodelid = self.id_legmodel;
		if hp > 0 {
			if get_flag(move_flag, move_flag::WALK) {
				self.legmodelid =
					self.id_walkmodel[(self.walkcnt / 3 % TOTAL_WALKMODE as i32) as usize]; // walking
				self.walkcnt += 1;
				self.runcnt = 0;
			} else {
				if get_flag(move_flag, move_flag::LEFT | move_flag::RIGHT) {
					self.legmodelid =
						self.id_runmodel[(self.runcnt / 3 % TOTAL_RUNMODE as i32) as usize]; // running sideways
				}
				if get_flag(move_flag, move_flag::FORWARD) {
					self.legmodelid =
						self.id_runmodel[(self.runcnt / 2 % TOTAL_RUNMODE as i32) as usize]; // running forward
				}
				if get_flag(move_flag, move_flag::BACK) {
					self.legmodelid =
						self.id_runmodel[(self.runcnt / 4 % TOTAL_RUNMODE as i32) as usize]; // running backward
				}
				self.walkcnt = 0;
				self.runcnt += 1;
			}
		}
	}

	/// `GetRenderMotion`. Upstream fills five out-parameters, any of which may be null, so
	/// the tuple carries all five and the caller takes what it wants.
	///
	/// The order is the upstream one: `(arm_rotation_y, leg_rotation_x, upmodel, armmodel,
	/// legmodel)`.
	pub fn get_render_motion(&self) -> (f32, f32, i32, i32, i32) {
		(
			self.armmodel_rotation_y,
			self.legrotation_x,
			self.id_upmodel,
			self.armmodelid,
			self.legmodelid,
		)
	}

	/// `GetMoveMotionCnt`: the count of whichever of the two motion cycles is running, for
	/// the footstep sound check.
	pub fn get_move_motion_cnt(&self) -> i32 {
		if self.walkcnt > 0 {
			return self.walkcnt;
		}
		self.runcnt
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The twelve run models and eight walk models are distinct numbers so a test can tell
	/// which slot of the cycle was drawn.
	fn controller() -> HumanMotionControl {
		let mut ctrl = HumanMotionControl::new();
		ctrl.set_parameter_info_class(ParameterInfo::new());
		ctrl.set_model(
			100,
			[101, 102, 103],
			110,
			[120, 121, 122, 123, 124, 125, 126, 127],
			[140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151],
		);
		ctrl
	}

	#[test]
	fn the_constructor_and_init_leave_the_same_state() {
		let ctrl = HumanMotionControl::new();
		assert_eq!(ctrl.id_upmodel, -1);
		assert_eq!(ctrl.id_armmodel, [-1; TOTAL_ARMMODE]);
		assert_eq!(ctrl.id_legmodel, -1);
		assert_eq!(ctrl.id_walkmodel, [-1; TOTAL_WALKMODE]);
		assert_eq!(ctrl.id_runmodel, [-1; TOTAL_RUNMODE]);
		assert_eq!(ctrl.reaction_y, 0.0);
		assert!(!ctrl.slowarm);
		assert_eq!(ctrl.get_render_motion(), (0.0, 0.0, -1, -1, -1));
		assert_eq!(ctrl.get_move_motion_cnt(), 0);

		// Init takes the leg angle from the caller and clears everything else.
		let mut ctrl = controller();
		ctrl.process_object(0.0, 0.0, 1, 0, 0, move_flag::WALK, 100, true);
		ctrl.init(1.25);
		assert_eq!(ctrl.legrotation_x, 1.25);
		assert_eq!(ctrl.walkcnt, 0);
		assert_eq!(ctrl.runcnt, 0);
		assert_eq!(ctrl.armmodelid, -1);
		assert_eq!(ctrl.legmodelid, -1);
		assert_eq!(ctrl.reaction_y, 0.0);
		assert!(!ctrl.slowarm);
	}

	#[test]
	fn walking_moves_the_leg_model_one_slot_every_three_frames() {
		let mut ctrl = controller();
		let mut seen = Vec::new();

		// Twenty four frames is three whole cycles of the eight walk models.
		for _ in 0..24 {
			ctrl.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::WALK, 100, true);
			seen.push(ctrl.legmodelid);
		}

		assert_eq!(
			seen,
			vec![
				120, 120, 120, 121, 121, 121, 122, 122, 122, 123, 123, 123, 124, 124, 124, 125,
				125, 125, 126, 126, 126, 127, 127, 127
			]
		);

		// The walking count is what the footstep check reads, and it runs with the model.
		assert_eq!(ctrl.walkcnt, 24);
		assert_eq!(ctrl.runcnt, 0);
		assert_eq!(ctrl.get_move_motion_cnt(), 24);

		// A twenty fifth frame wraps back to the first walk model.
		ctrl.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::WALK, 100, true);
		assert_eq!(ctrl.legmodelid, 120);
	}

	#[test]
	fn running_forward_steps_twice_as_fast_as_walking() {
		let mut ctrl = controller();
		let mut seen = Vec::new();
		for _ in 0..4 {
			ctrl.process_object(
				0.0,
				0.0,
				ID_WEAPON_NONE,
				0,
				0,
				move_flag::FORWARD,
				100,
				true,
			);
			seen.push(ctrl.legmodelid);
		}
		// runcnt / 2, so two frames per slot.
		assert_eq!(seen, vec![140, 140, 141, 141]);
		assert_eq!(ctrl.walkcnt, 0);
		assert_eq!(ctrl.runcnt, 4);
		// With no walking count the count comes from the run cycle.
		assert_eq!(ctrl.get_move_motion_cnt(), 4);
	}

	#[test]
	fn running_backward_is_the_slowest_cycle_and_sideways_sits_between() {
		// Backward is runcnt / 4 and sideways is runcnt / 3, so after eight frames the
		// backward cycle is still on its second model while the sideways one is on its
		// third.
		let mut back = controller();
		for _ in 0..8 {
			back.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::BACK, 100, true);
		}
		assert_eq!(back.legmodelid, 141, "the second of the twelve");
		assert_eq!(back.runcnt, 8);

		let mut side = controller();
		for _ in 0..8 {
			side.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::LEFT, 100, true);
		}
		assert_eq!(side.legmodelid, 142, "the third of the twelve");
		assert_eq!(side.runcnt, 8);

		// A diagonal is still a run, and the forward case is checked last, so the forward
		// slot wins over the sideways one in the same frame.
		let mut diagonal = controller();
		for _ in 0..6 {
			diagonal.process_object(
				0.0,
				0.0,
				ID_WEAPON_NONE,
				0,
				0,
				move_flag::FORWARD | move_flag::LEFT,
				100,
				true,
			);
		}
		assert_eq!(diagonal.legmodelid, 142);
	}

	#[test]
	fn a_dead_human_locks_the_legs_to_the_body_rotation_and_stops_the_cycle() {
		let mut ctrl = controller();
		// Get the cycle running first.
		for _ in 0..5 {
			ctrl.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::WALK, 100, true);
		}
		assert_eq!(ctrl.legmodelid, 121);
		let walkcnt = ctrl.walkcnt;

		// At zero health the legs follow the body and the counts stand still.
		ctrl.process_object(0.75, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::WALK, 0, true);
		assert_eq!(ctrl.legrotation_x, 0.75);
		assert_eq!(ctrl.walkcnt, walkcnt);
		assert_eq!(ctrl.legmodelid, 110);
	}

	#[test]
	fn the_leg_angle_chases_the_travel_direction_a_fraction_at_a_time() {
		let mut ctrl = controller();
		// A human facing -X that runs forward wants the legs at -X too: move_rx is 0, so
		// the target is the body rotation itself.
		ctrl.process_object(
			1.0,
			0.0,
			ID_WEAPON_NONE,
			0,
			0,
			move_flag::FORWARD,
			100,
			true,
		);
		// 0 * 0.85 + (1.0 - 0) * 0.15
		assert!(
			(ctrl.legrotation_x - 0.15).abs() < 1e-6,
			"{}",
			ctrl.legrotation_x
		);

		// Running backward is more than a right angle from the body, so the leg is drawn
		// from the other side: move_rx of PI becomes 0 after normalisation.
		let mut back = controller();
		back.process_object(1.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::BACK, 100, true);
		// move_rx2 = normalise(PI + PI) = 0, so the same 0.15.
		assert!(
			(back.legrotation_x - 0.15).abs() < 1e-6,
			"{}",
			back.legrotation_x
		);

		// Sideways is a quarter turn, and the leg lags behind it.
		let mut side = controller();
		side.process_object(0.0, 0.0, ID_WEAPON_NONE, 0, 0, move_flag::LEFT, 100, true);
		let expected = (degree_to_radian(-90.0)) * 0.15;
		assert!(
			(side.legrotation_x - expected).abs() < 1e-6,
			"{}",
			side.legrotation_x
		);
	}

	#[test]
	fn an_empty_hand_aims_the_arm_at_the_rest_angle_for_the_player_and_with_the_body_for_an_ai() {
		let mut ctrl = controller();

		ctrl.process_object(0.0, 0.5, ID_WEAPON_NONE, 0, 0, 0, 100, true);
		assert_eq!(ctrl.armmodel_rotation_y, ARMRAD_NOWEAPON);
		assert_eq!(ctrl.armmodelid, 101);

		ctrl.process_object(0.0, 0.5, ID_WEAPON_NONE, 0, 0, 0, 100, false);
		assert_eq!(ctrl.armmodel_rotation_y, 0.5);
		assert_eq!(ctrl.armmodelid, 101);
	}

	#[test]
	fn the_arm_shape_follows_the_weapon_equip_method() {
		// MP5 holds the weapon with two hands: arm model 1.
		let mut mp5 = controller();
		mp5.process_object(0.0, 0.25, 1, 0, 0, 0, 100, true);
		assert_eq!(mp5.armmodel_rotation_y, 0.25);
		assert_eq!(mp5.armmodelid, 102);

		// The pistols hold it one handed: arm model 2.
		let mut pistol = controller();
		pistol.process_object(0.0, 0.25, 4, 0, 0, 0, 100, true);
		assert_eq!(pistol.armmodel_rotation_y, 0.25);
		assert_eq!(pistol.armmodelid, 103);

		// The null weapon rests the arm and keeps arm model 0.
		let mut none = controller();
		none.process_object(0.0, 0.25, ID_WEAPON_NONE, 0, 0, 0, 100, false);
		assert_eq!(none.armmodelid, 101);
	}

	#[test]
	fn firing_swings_the_arm_and_the_swing_halves_away() {
		let mut ctrl = controller();
		// MP5, whose scope table leaves the reaction to the weapon: 0.5 degrees times its
		// reaction of 6.
		ctrl.shot_weapon(1);
		assert!(!ctrl.slowarm);
		let kick = degree_to_radian(0.5) * 6.0;
		assert!((ctrl.reaction_y - kick).abs() < 1e-9, "{}", ctrl.reaction_y);

		// The arm angle the renderer gets is the aim plus the swing.
		ctrl.process_object(0.0, 0.0, 1, 0, 0, 0, 100, true);
		assert_eq!(ctrl.armmodel_rotation_y, kick * 0.5);

		// And the swing is gone within sixteen frames. Halving from 0.0524 radians needs
		// ten steps to reach the hundredth of a degree it stops at.
		let mut frames = 0;
		while ctrl.reaction_y != 0.0 {
			ctrl.process_object(0.0, 0.0, 1, 0, 0, 0, 100, true);
			frames += 1;
			assert!(frames < 32, "the reaction never settled");
			assert!(ctrl.reaction_y.abs() <= kick);
		}
		assert_eq!(frames, 9);
	}

	#[test]
	fn a_grenade_throws_the_arm_up_twenty_degrees() {
		let mut ctrl = controller();
		ctrl.shot_weapon(ID_WEAPON_GRENADE);
		assert_eq!(ctrl.reaction_y, degree_to_radian(20.0));
		assert!(!ctrl.slowarm);
	}

	#[test]
	fn picking_up_a_weapon_raises_the_arm_two_degrees_a_frame() {
		let mut ctrl = controller();
		ctrl.pickup_weapon(1);
		assert_eq!(ctrl.reaction_y, ARMRAD_RELOADWEAPON);
		assert!(ctrl.slowarm);

		// Twenty degrees at two degrees a frame, then the guard clears the flag.
		let mut frames = 0;
		while ctrl.slowarm {
			ctrl.process_object(0.0, 0.0, 1, 0, 0, 0, 100, true);
			frames += 1;
			assert!(frames < 32, "the arm never settled");
		}
		assert_eq!(frames, 10);
		assert_eq!(ctrl.reaction_y, 0.0);
	}

	#[test]
	fn reloading_and_switching_a_weapon_type_reset_the_arm_angle() {
		let mut ctrl = controller();
		ctrl.shot_weapon(1);
		ctrl.process_object(0.0, 0.0, 1, 1, 0, 0, 100, true);
		assert_eq!(ctrl.reaction_y, ARMRAD_RELOADWEAPON);

		let mut ctrl = controller();
		ctrl.shot_weapon(1);
		ctrl.process_object(0.0, 0.0, 1, 0, 3, 0, 100, true);
		assert_eq!(ctrl.reaction_y, ARMRAD_RELOADWEAPON);
	}

	#[test]
	fn the_weapon_type_is_unused_by_the_three_dummy_entry_points() {
		let mut ctrl = controller();
		let before = ctrl.reaction_y;
		ctrl.change_weapon_id(4);
		ctrl.reload_weapon(4);
		ctrl.dump_weapon();
		ctrl.jump();
		assert_eq!(ctrl.reaction_y, before);
		assert!(!ctrl.slowarm);
	}

	#[test]
	fn a_weapon_switch_and_a_pickup_both_raise_the_arm() {
		let mut ctrl = controller();
		ctrl.change_have_weapon(ID_WEAPON_NONE);
		assert_eq!(ctrl.reaction_y, ARMRAD_RELOADWEAPON);
		assert!(ctrl.slowarm);
	}
}
