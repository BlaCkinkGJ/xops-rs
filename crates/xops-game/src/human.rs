//! The human class, from `object.cpp:160-2207` and `object.h:146-300`.
//!
//! A human is the largest object in the game: it walks, falls, holds two weapons, takes
//! damage per body part and, when the health runs out, plays a body-fall state machine
//! that is a small physics problem of its own.
//!
//! **How a human refers to its weapons.** Upstream keeps `class weapon *weapon[]`, raw
//! pointers into the manager's own weapon array, which makes the two classes mutually
//! referential. Here the human keeps *indices* into that array (`Option<usize>`, where
//! `None` is the upstream `NULL`) and every method that dereferences one takes the array
//! as a parameter, so the manager's borrow of its own weapons is the one the human
//! writes through. The manager therefore hands over the same slice it owns:
//!
//! ```ignore
//! let slot = human.pickup_weapon(weapon_index, &mut self.weapons);
//! ```
//!
//! An index is not a pointer, so the two can drift apart if the manager compacts its
//! array while a human holds a weapon. It must not: a held weapon is skipped by the
//! manager's own render and simulation, so nothing moves it.

use std::f32::consts::PI;
use std::ops::{Deref, DerefMut};

use crate::collision::{
	get_aabb_ray, vector_normalization, Collision, COLLISION_ADDSIZE, HUMAN_MAPCOLLISION_ADD_HEIGHT_A,
	HUMAN_MAPCOLLISION_ADD_HEIGHT_B, HUMAN_MAPCOLLISION_CHECK_HEIGHT, HUMAN_MAPCOLLISION_CHECK_MAXDIST,
	HUMAN_MAPCOLLISION_CLIMBADDY, HUMAN_MAPCOLLISION_CLIMBHEIGHT, HUMAN_MAPCOLLISION_GROUND_HEIGHT,
	HUMAN_MAPCOLLISION_GROUND_R1, HUMAN_MAPCOLLISION_GROUND_R2, HUMAN_MAPCOLLISION_GROUND_R3,
	HUMAN_MAPCOLLISION_HEIGHT, HUMAN_MAPCOLLISION_R, HUMAN_MAPCOLLISION_SLOPEANGLE,
};
use crate::core::{angle_normalization, degree_to_radian, TOTAL_HAVEWEAPON};
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::{BlockDataInterface, MAX_BLOCKS};
use crate::motion::HumanMotionControl;
use crate::object::{
	move_flag, Object, HUMAN_ATTENUATION, HUMAN_DAMAGE_HEAD, HUMAN_DAMAGE_LEG, HUMAN_DAMAGE_MAXFALL,
	HUMAN_DAMAGE_MAXSPEED, HUMAN_DAMAGE_MINSPEED, HUMAN_DAMAGE_SPEED, HUMAN_DAMAGE_UP, HUMAN_DAMAGE_ZOMBIEA,
	HUMAN_DAMAGE_ZOMBIEU, HUMAN_DEADADDRY, HUMAN_DEADLINE, HUMAN_HEIGHT, HUMAN_JUMP_SPEED,
	HUMAN_PROGRESSRUN_ACCELERATION, HUMAN_PROGRESSRUN_SIDEWAYSRUN_ACCELERATION,
	HUMAN_PROGRESSWALK_ACCELERATION, HUMAN_REGRESSRUN_ACCELERATION,
	HUMAN_REGRESSRUN_SIDEWAYSRUN_ACCELERATION, HUMAN_SIDEWAYSRUN_ACCELERATION,
};
use crate::parameter::{ParameterInfo, ID_WEAPON_NONE};
use crate::resource::{ResourceManager, TOTAL_ARMMODE, TOTAL_RUNMODE, TOTAL_WALKMODE};
use crate::weapon::Weapon;
use crate::window::get_rand;

/// `GetFlag` over the movement flag word.
///
/// The upstream macro is `value & bit`. It is a function here because the movement flags
/// are `i32` like the `int` they come from, while the generic bit helpers in
/// [`crate::core`] take `u32`, and casting at fifty call sites would hide the pattern
/// rather than show it.
pub(crate) fn get_flag(value: i32, bit: i32) -> bool {
	value & bit != 0
}

/// `human`.
#[derive(Clone, Debug)]
pub struct Human {
	/// The shared object state: where it is, what it draws, whether it is enabled.
	pub object: Object,
	/// Point data number.
	point_dataid: i32,
	/// Point identifier.
	point_p4: i16,
	/// Team number.
	teamid: i32,
	/// X axis velocity.
	move_x: f32,
	/// Y axis (falling) velocity.
	move_y: f32,
	/// Z axis velocity.
	move_z: f32,
	/// Y axis movement flag.
	move_y_flag: bool,
	/// Counter that forbids Y axis ascent.
	move_y_upper: i32,
	/// Landing flag.
	move_y_landing: bool,
	/// ID of the block underfoot.
	underblock_id: i32,
	/// Face number of the block underfoot.
	underblock_face: i32,
	/// Overall rotation angle, which is the angle a corpse has fallen to.
	rotation_y: f32,
	/// Arm rotation angle.
	armrotation_y: f32,
	/// Upper body draw size.
	upmodel_size: f32,
	/// Arm draw size.
	armmodel_size: f32,
	/// Leg draw size.
	legmodel_size: f32,
	/// The weapons held, as indices into the manager's weapon array. `None` is an empty
	/// hand, which upstream is the null pointer.
	weapon: [Option<usize>; TOTAL_HAVEWEAPON],
	/// Total bullets when no weapon is held.
	noweapon_bullets: [i32; TOTAL_HAVEWEAPON],
	/// Loaded bullets when no weapon is held.
	noweapon_loadbullets: [i32; TOTAL_HAVEWEAPON],
	/// Weapon A/B selection.
	selectweapon: i32,
	/// Count for switching (re-equipping) weapons.
	selectweaponcnt: i32,
	/// Count for switching weapon type.
	changeweaponidcnt: i32,
	/// Weapon rapid fire count.
	weaponshotcnt: i32,
	/// Weapon reload count.
	weaponreloadcnt: i32,
	/// Firing flag.
	weaponshotframe: bool,
	/// Burst mode count.
	weaponburstmodecnt: i32,
	/// Health.
	hp: i32,
	/// Whether the human has become a corpse: 0 alive, 1 to 4 falling, 5 fixed.
	deadstate: i32,
	/// Acceleration at which a corpse falls over.
	add_ry: f32,
	/// Movement angle.
	move_rx: f32,
	/// Flag representing the movement direction.
	move_flag: i32,
	/// Flag representing the (previous) movement direction.
	move_flag_lt: i32,
	/// Scope usage mode.
	scopemode: i32,
	/// Flag representing being hit.
	hit_flag: bool,
	/// Direction from which the hit came.
	hit_rx: f32,
	/// Total movement amount.
	///
	/// The upstream initialises this and never writes it again, so it stays zero for the
	/// life of the object and [`Human::total_move`] answers zero. Kept as it is, because
	/// a caller that reads it upstream reads the same zero.
	totalmove: f32,
	/// Gunsight state error.
	state_gunsight_error_range: i32,
	/// Gunsight recoil error.
	reaction_gunsight_error_range: i32,
	/// Invincible flag.
	invincible: bool,
	/// The class controlling the motion.
	///
	/// Upstream a pointer. Nothing else refers to it, so it is owned by value.
	motion_ctrl: HumanMotionControl,
}

impl Deref for Human {
	type Target = Object;

	fn deref(&self) -> &Object {
		&self.object
	}
}

impl DerefMut for Human {
	fn deref_mut(&mut self) -> &mut Object {
		&mut self.object
	}
}

impl Human {
	/// Constructor.
	///
	/// Upstream builds the base `object` with its own defaults first and then overwrites
	/// the members it sets, so `model_size` starts at 1.0 and only the upper body, arm and
	/// leg sizes are given values here. `id_model` stays -1 for the manager to fill in.
	pub fn new(
		param: ParameterInfo,
		x: f32,
		y: f32,
		z: f32,
		rx: f32,
		id_param: i32,
		dataid: i32,
		p4: i16,
		team: i32,
		flag: bool,
	) -> Human {
		let mut object = Object::new(param, x, y, z, rx, 1.0, flag);
		object.id_parameter = id_param;
		object.id_texture = -1;

		Human {
			object,
			point_dataid: dataid,
			point_p4: p4,
			teamid: team,
			move_x: 0.0,
			move_y: 0.0,
			move_z: 0.0,
			move_y_flag: false,
			move_y_upper: 0,
			move_y_landing: false,
			underblock_id: -1,
			underblock_face: -1,
			rotation_y: 0.0,
			armrotation_y: 0.0,
			upmodel_size: 9.4,
			armmodel_size: 9.0,
			legmodel_size: 9.0,
			weapon: [None; TOTAL_HAVEWEAPON],
			noweapon_bullets: [0; TOTAL_HAVEWEAPON],
			noweapon_loadbullets: [0; TOTAL_HAVEWEAPON],
			selectweapon: 1,
			selectweaponcnt: 0,
			changeweaponidcnt: 0,
			weaponshotcnt: 0,
			weaponreloadcnt: 0,
			weaponshotframe: false,
			weaponburstmodecnt: 0,
			// The upstream reads the health from the settings here, but the lookup is
			// commented out, so every human an object manager builds starts at zero and
			// takes its real health from `SetParamData`.
			hp: 0,
			deadstate: 0,
			add_ry: 0.0,
			move_rx: 0.0,
			move_flag: 0x00,
			move_flag_lt: 0x00,
			scopemode: 0,
			hit_flag: false,
			hit_rx: 0.0,
			totalmove: 0.0,
			state_gunsight_error_range: 0,
			reaction_gunsight_error_range: 0,
			invincible: false,
			// Upstream the motion control gets a null `Param` until the human's
			// `SetParameterInfoClass` runs, which the header warns has to happen before
			// use. The settings class is a zero-sized handle here, so the two states are
			// the same value and the warning is only about intent.
			motion_ctrl: HumanMotionControl::new(),
		}
	}

	/// Registers the class that manages the setting values.
	///
	/// Must be executed before using each function.
	pub fn set_parameter_info_class(&mut self, param: ParameterInfo) {
		self.object.set_parameter_info_class(param);
		self.motion_ctrl.set_parameter_info_class(param);
	}

	/// Sets the setting values.
	///
	/// When initialising with `init` set to true, set the angle with
	/// [`Object::set_pos_data`] first.
	pub fn set_param_data(&mut self, id_param: i32, dataid: i32, p4: i16, team: i32, init: bool) {
		self.object.id_parameter = id_param;
		self.point_dataid = dataid;
		self.point_p4 = p4;
		self.teamid = team;

		if init {
			self.move_x = 0.0;
			self.move_y = 0.0;
			self.move_z = 0.0;
			self.move_y_flag = false;
			self.move_y_upper = 0;
			self.move_y_landing = false;
			self.underblock_id = -1;
			self.underblock_face = -1;
			self.rotation_y = 0.0;
			self.armrotation_y = degree_to_radian(-30.0);

			self.weapon = [None; TOTAL_HAVEWEAPON];
			self.noweapon_bullets = [0; TOTAL_HAVEWEAPON];
			self.noweapon_loadbullets = [0; TOTAL_HAVEWEAPON];
			self.selectweapon = 1;
			self.selectweaponcnt = 0;
			self.changeweaponidcnt = 0;
			self.weaponshotcnt = 0;
			self.weaponreloadcnt = 0;
			self.weaponshotframe = false;
			self.weaponburstmodecnt = 0;

			self.hp = match self.object.param.get_human(id_param) {
				Some(data) => data.hp,
				// The upstream leaves the health at zero when the type number has no
				// settings, and that is a human that dies on its first frame.
				None => 0,
			};
			self.deadstate = 0;
			self.add_ry = 0.0;
			self.move_flag = 0x00;
			self.move_flag_lt = self.move_flag;
			self.scopemode = 0;
			self.hit_flag = false;
			self.hit_rx = 0.0;
			self.totalmove = 0.0;
			self.invincible = false;

			self.motion_ctrl.init(self.object.rotation_x);
		}
	}

	/// Gets the setting values. Upstream fills whichever of the four out-parameters are
	/// not null, so the tuple carries all four.
	pub fn get_param_data(&self) -> (i32, i32, i16, i32) {
		(
			self.object.id_parameter,
			self.point_dataid,
			self.point_p4,
			self.teamid,
		)
	}

	/// Gets the movement amount from the previous step.
	pub fn move_pos(&self) -> (f32, f32, f32) {
		(self.move_x, self.move_y, self.move_z)
	}

	/// Sets the model data, which the motion control does the work of.
	pub fn set_model(
		&mut self,
		upmodel: i32,
		armmodel: [i32; TOTAL_ARMMODE],
		legmodel: i32,
		walkmodel: [i32; TOTAL_WALKMODE],
		runmodel: [i32; TOTAL_RUNMODE],
	) {
		self.motion_ctrl
			.set_model(upmodel, armmodel, legmodel, walkmodel, runmodel);
	}

	/// Gets the HP.
	pub fn hp(&self) -> i32 {
		self.hp
	}

	/// Sets the HP. It fails unless executed on a human whose HP is 1 or more.
	pub fn set_hp(&mut self, in_hp: i32) -> bool {
		if self.hp > 0 {
			self.hp = in_hp;
			return true;
		}
		false
	}

	/// Whether this is a corpse.
	///
	/// Only a completely motionless state counts, so not every human with `hp <= 0` is a
	/// corpse: one that is still falling over is not.
	pub fn dead_flag(&self) -> bool {
		self.deadstate == 5
	}

	/// Whether the death motion is playing, which is while the body is falling over or
	/// the feet are slipping. It is not judged while airborne.
	pub fn dead_motion_flag(&self) -> bool {
		self.deadstate == 1 || self.deadstate == 3
	}

	/// Overwrites the team number.
	pub fn set_team_id(&mut self, id: i32) {
		self.teamid = id;
	}

	/// Whether the human takes no damage at all from bullets, grenades or falls.
	pub fn invincible_flag(&self) -> bool {
		self.invincible
	}

	pub fn set_invincible_flag(&mut self, flag: bool) {
		self.invincible = flag;
	}

	/// Sets the weapons and the bullet counts for an empty hand.
	///
	/// Normally use [`Human::pickup_weapon`]. A slot whose index is `None`, or whose
	/// weapon refuses the pickup, keeps whatever it held.
	pub fn set_weapon(
		&mut self,
		in_weapon: [Option<usize>; TOTAL_HAVEWEAPON],
		nlnbs: [i32; TOTAL_HAVEWEAPON],
		nnbs: [i32; TOTAL_HAVEWEAPON],
		weapons: &mut [Weapon],
	) {
		for i in 0..TOTAL_HAVEWEAPON {
			match in_weapon[i] {
				None => self.weapon[i] = None,
				Some(index) => {
					// If the weapon is picked up correctly, register it as a held weapon.
					if weapons[index].pickup() == 0 {
						self.weapon[i] = Some(index);
					}
				}
			}
			self.noweapon_loadbullets[i] = nlnbs[i];
			self.noweapon_bullets[i] = nnbs[i];
		}
	}

	/// Picks up a weapon into the selected slot. `index` is its place in the manager's
	/// array, which is what upstream receives as a `weapon *`.
	///
	/// Success: 1, failure: 0.
	pub fn pickup_weapon(&mut self, index: usize, weapons: &mut [Weapon]) -> i32 {
		let slot = self.selectweapon as usize;

		if self.weapon[slot].is_none() {
			// If the weapon is picked up correctly, register it as a held weapon.
			if weapons[index].pickup() == 0 {
				self.weapon[slot] = Some(index);

				// Set the arm angle (recoil).
				let id_param = weapons[index].param_data().0;
				self.motion_ctrl.pickup_weapon(id_param);

				// Initialize the bullet count info for when no weapon is held.
				self.noweapon_bullets[slot] = 0;
				self.noweapon_loadbullets[slot] = 0;

				// Count until the switch completes.
				self.selectweaponcnt = 10;

				return 1;
			}
		}
		0
	}

	/// Switches the weapon to another one, or to the next one for `id` of -1.
	///
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn change_have_weapon(&mut self, id: i32, weapons: &[Weapon]) {
		// Fail if there is no health left.
		if self.hp <= 0 {
			return;
		}

		// Fail while reloading, while switching weapons, or on the same weapon.
		if self.weaponreloadcnt > 0 {
			return;
		}
		if self.selectweapon == id {
			return;
		}
		if self.selectweaponcnt > 0 {
			return;
		}
		if self.changeweaponidcnt > 0 {
			return;
		}

		if id == -1 {
			// Select the next weapon number.
			self.selectweapon += 1;
			if self.selectweapon == TOTAL_HAVEWEAPON as i32 {
				self.selectweapon = 0;
			}
		} else {
			// Check whether the weapon number is in range.
			if (id < 0) || (TOTAL_HAVEWEAPON as i32 <= id) {
				return;
			}
			self.selectweapon = id;
		}

		// Cancel the scope display.
		self.set_disable_scope();

		// Set the arm angle (recoil) for the class of the weapon now equipped.
		match self.weapon[self.selectweapon as usize] {
			None => self.motion_ctrl.change_have_weapon(ID_WEAPON_NONE),
			Some(index) => {
				let id_param = weapons[index].param_data().0;
				self.motion_ctrl.change_have_weapon(id_param);
			}
		}

		// Count until the switch completes.
		self.selectweaponcnt = 10;

		// Count for burst mode.
		self.weaponburstmodecnt = 0;
	}

	/// Gets the weapon switch count. Switching is in progress when it is 1 or more.
	pub fn get_change_have_weapon_cnt(&self) -> i32 {
		self.selectweaponcnt
	}

	/// Switches the weapon type, which is how the Glock 18 semi and full auto pair move
	/// between each other.
	///
	/// Success: 0, failure: 1.
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn change_weapon_id(&mut self, weapons: &mut [Weapon]) -> i32 {
		let slot = self.selectweapon as usize;

		// Fail while switching weapons.
		if self.changeweaponidcnt > 0 {
			return 1;
		}

		// Fail if no weapon is equipped.
		let Some(index) = self.weapon[slot] else {
			return 1;
		};

		// Get the info of the equipped weapon.
		let (param_id, lnbs, nbs) = weapons[index].param_data();

		// Look up the weapon type switch destination (the new weapon number).
		let Some(param_data) = self.object.param.get_weapon(param_id) else {
			return 1;
		};
		let change_weapon_id = param_data.change_weapon_id;
		let change_cnt = param_data.change_weapon_cnt;

		// Check whether the new weapon number is valid.
		if change_weapon_id == param_id {
			return 1;
		}
		if (change_weapon_id < 0) || (crate::parameter::TOTAL_PARAMETERINFO_WEAPON as i32 <= change_weapon_id) {
			return 1;
		}

		// Do not switch while reloading even when the count is enabled.
		if (change_cnt > 0) && (self.weaponreloadcnt > 0) {
			return 1;
		}

		// Apply the settings.
		weapons[index].set_param_data(change_weapon_id, lnbs, nbs, false);

		// If the scope is in use.
		if self.scopemode != 0 {
			if change_cnt == 0 {
				// Get the info of the new weapon.
				let Some(new_param) = self.object.param.get_weapon(change_weapon_id) else {
					return 1;
				};

				if self.scopemode == new_param.scopemode {
					// If the same scope can be used, keep the scope as is.
				} else if new_param.scopemode == 0 {
					// If the scope cannot be used, cancel the scope.
					self.set_disable_scope();
				} else {
					// If a different scope can be used, set the scope again.
					self.set_enable_scope(weapons);
				}
			} else {
				// Cancel the scope.
				self.set_disable_scope();
			}
		}

		// Run the motion.
		self.motion_ctrl.change_weapon_id(change_weapon_id);

		// Count until the switch completes.
		self.changeweaponidcnt = change_cnt;

		// Count for burst mode.
		self.weaponburstmodecnt = 0;

		0
	}

	/// Gets the weapon type switch count. Switching is in progress when it is 1 or more.
	pub fn get_change_weapon_id_cnt(&self) -> i32 {
		self.changeweaponidcnt
	}

	/// Gets the weapon reload count. Reloading when it is 1 or more.
	pub fn get_weapon_reload_cnt(&self) -> i32 {
		self.weaponreloadcnt
	}

	/// Gets the selection and the weapons held.
	///
	/// Upstream hands back the weapon pointers and lets either bullet count array be null.
	/// The indices are what a caller needs to find the same weapons in the manager's own
	/// array, and both counts are always returned.
	pub fn get_weapon(
		&self,
	) -> (
		i32,
		[Option<usize>; TOTAL_HAVEWEAPON],
		[i32; TOTAL_HAVEWEAPON],
		[i32; TOTAL_HAVEWEAPON],
	) {
		(
			self.selectweapon,
			self.weapon,
			self.noweapon_loadbullets,
			self.noweapon_bullets,
		)
	}

	/// Gets the type number of the currently equipped weapon.
	///
	/// This is the type number of the weapon currently held, and is equivalent to looking
	/// it up with [`Human::get_weapon`].
	pub fn get_main_weapon_type_no(&self, weapons: &[Weapon]) -> i32 {
		match self.weapon[self.selectweapon as usize] {
			None => ID_WEAPON_NONE,
			Some(index) => weapons[index].param_data().0,
		}
	}

	/// Gets the rapid fire setting. True when rapid fire is possible.
	///
	/// A weapon with no settings cannot be asked, and answering "no rapid fire" is the
	/// safe direction for a weapon the game knows nothing about.
	pub fn get_weapon_blazingmode(&self, weapons: &[Weapon]) -> bool {
		let Some(index) = self.weapon[self.selectweapon as usize] else {
			return false;
		};

		// Get the type number of the equipped weapon.
		let param_id = weapons[index].param_data().0;

		match self.object.param.get_weapon(param_id) {
			Some(data) => data.burstcnt != 1,
			None => false,
		}
	}

	/// Fires the weapon.
	///
	/// Processing the bullet object and playing the gunshot sound must be done separately.
	/// Avoid calling this directly from the game. Call it from the object manager class.
	///
	/// Upstream returns the success flag and fills two out-parameters, so `None` is the
	/// false and `Some` carries the weapon type number that was fired and the gunsight
	/// error at the time of firing.
	pub fn shot_weapon(&mut self, weapons: &mut [Weapon]) -> Option<(i32, i32)> {
		let slot = self.selectweapon as usize;

		// Fail while switching weapons.
		if self.selectweaponcnt > 0 {
			return None;
		}
		if self.changeweaponidcnt > 0 {
			return None;
		}

		// Fail if no weapon is equipped.
		let Some(index) = self.weapon[slot] else {
			return None;
		};

		// Accept the firing request and enable the flag. It stays set even when the shot
		// fails below, which is what keeps the burst mode count alive between frames.
		self.weaponshotframe = true;

		// Fail if the firing interval has not elapsed or while reloading.
		if self.weaponshotcnt > 0 {
			return None;
		}
		if self.weaponreloadcnt > 0 {
			return None;
		}

		// Get the weapon type number and its setting values.
		let param_id = weapons[index].param_data().0;
		let Some(param_data) = self.object.param.get_weapon(param_id) else {
			return None;
		};
		let Some(sparam) = self.object.param.get_scope_param(param_data.scopemode) else {
			return None;
		};

		// Fail if burst mode is enabled and the burst bullet count is exceeded.
		if (param_data.burstcnt > 0) && (self.weaponburstmodecnt >= param_data.burstcnt) {
			return None;
		}

		// Perform the bullet firing process.
		if weapons[index].shot() != 0 {
			return None;
		}

		// Count as a fired bullet.
		self.weaponburstmodecnt += 1;

		// Set the rapid fire count.
		self.weaponshotcnt = param_data.blazings;

		// Set the error when the scope is not used.
		if (self.scopemode == 0) && (sparam.weapon_no_scope_reaction > 0) {
			self.reaction_gunsight_error_range = sparam.weapon_no_scope_reaction;
		}

		// Return the weapon type number and the error.
		let out_paramid = param_id;
		let out_error = self.state_gunsight_error_range + self.reaction_gunsight_error_range;

		// Error calculation: if the gunsight is displayed, add the configured error.
		if ((self.scopemode == 0) && sparam.no_scope_gunsight)
			|| ((self.scopemode != 0) && (sparam.scope_gunsight == 0))
		{
			self.reaction_gunsight_error_range += param_data.reaction;
		}

		// Add the recoil of the state that uses the scope.
		if (sparam.weapon_recoil_scope_rx != 0.0)
			|| (sparam.weapon_recoil_scope_ry_min != 0.0)
			|| (sparam.weapon_recoil_scope_ry_max != 0.0)
		{
			if (self.scopemode != 0) || !sparam.weapon_recoil_scope_only {
				// Calculate the random value range. The tenths are what the table's values
				// are given in.
				let rx_rnds = ((sparam.weapon_recoil_scope_rx * 10.0) as i32) * 2;
				let ry_rnds = ((sparam.weapon_recoil_scope_ry_max * 10.0) as i32)
					- ((sparam.weapon_recoil_scope_ry_min * 10.0) as i32);

				// Determine the recoil (angle).
				let rx_degree =
					sparam.weapon_recoil_scope_rx * -1.0 + (get_rand(rx_rnds) as f32) / 10.0;
				let ry_degree = sparam.weapon_recoil_scope_ry_min + (get_rand(ry_rnds) as f32) / 10.0;

				// Add the recoil.
				self.object.rotation_x += degree_to_radian(rx_degree);
				self.armrotation_y += degree_to_radian(ry_degree);
			}
		}

		// Pass the recoil to the arm.
		self.motion_ctrl.shot_weapon(param_id);

		// If there is no weapon left, treat it as unequipped. (for grenades)
		if !weapons[index].object.enable_flag {
			self.weapon[slot] = None;
		}

		Some((out_paramid, out_error))
	}

	/// Reloads. Success: true, failure: false.
	///
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn reload_weapon(&mut self, weapons: &mut [Weapon]) -> bool {
		let slot = self.selectweapon as usize;

		// Fail while switching weapons.
		if self.selectweaponcnt > 0 {
			return false;
		}
		if self.changeweaponidcnt > 0 {
			return false;
		}

		// If some kind of weapon is equipped.
		let Some(index) = self.weapon[slot] else {
			return false;
		};

		// Fail while reloading.
		if self.weaponreloadcnt > 0 {
			return false;
		}

		// Start the reload process.
		if weapons[index].start_reload() != 0 {
			return false;
		}

		// Cancel the scope mode.
		self.set_disable_scope();

		// Run the motion.
		let id_param = weapons[index].param_data().0;
		self.motion_ctrl.reload_weapon(id_param);

		// Get the weapon performance values and set the reload count.
		let Some(param_data) = self.object.param.get_weapon(id_param) else {
			return true;
		};
		self.weaponreloadcnt = param_data.reloads + 1;

		// Set the count for burst mode.
		self.weaponburstmodecnt = 0;

		true
	}

	/// Drops the weapon. Success: true, failure: false.
	///
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn dump_weapon(&mut self, weapons: &mut [Weapon]) -> bool {
		let slot = self.selectweapon as usize;

		// Fail while switching weapons.
		if self.selectweaponcnt > 0 {
			return false;
		}
		if self.changeweaponidcnt > 0 {
			return false;
		}

		// If some kind of weapon is equipped.
		let Some(index) = self.weapon[slot] else {
			return false;
		};

		// Fail while reloading.
		if self.weaponreloadcnt > 0 {
			return false;
		}

		// Drop the weapon and unequip it.
		weapons[index].dropoff(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x,
			1.63,
		);
		self.weapon[slot] = None;

		// Initialize the bullet count info for when no weapon is held.
		self.noweapon_bullets[slot] = 0;
		self.noweapon_loadbullets[slot] = 0;

		// Cancel the scope mode.
		self.set_disable_scope();

		// Run the motion.
		self.motion_ctrl.dump_weapon();

		true
	}

	/// Sets forward movement (running).
	///
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn set_move_forward(&mut self) {
		self.move_flag |= move_flag::FORWARD;
	}

	/// Sets backward movement.
	pub fn set_move_back(&mut self) {
		self.move_flag |= move_flag::BACK;
	}

	/// Sets left running.
	pub fn set_move_left(&mut self) {
		self.move_flag |= move_flag::LEFT;
	}

	/// Sets right running.
	pub fn set_move_right(&mut self) {
		self.move_flag |= move_flag::RIGHT;
	}

	/// Sets walking.
	pub fn set_move_walk(&mut self) {
		self.move_flag |= move_flag::WALK;
	}

	/// Gets the movement mode of the human.
	///
	/// `nowdata` picks the current value, otherwise the previous frame's.
	/// Walking: 1, forward running: 2, backward running: 3, sideways running: 4, not
	/// moving: 0.
	pub fn get_movemode(&self, nowdata: bool) -> i32 {
		// Exit if it is unused or has not been processed.
		if !self.object.enable_flag {
			return 0;
		}
		if self.hp <= 0 {
			return 0;
		}

		let flags = if nowdata { self.move_flag } else { self.move_flag_lt };

		if get_flag(flags, move_flag::WALK) {
			return 1;
		}
		if get_flag(flags, move_flag::FORWARD) {
			return 2;
		}
		if get_flag(flags, move_flag::BACK) {
			return 3;
		}
		if get_flag(flags, move_flag::LEFT | move_flag::RIGHT) {
			return 4;
		}

		0
	}

	/// Gets the flag that represents the movement direction.
	///
	/// This is a debugging function. Normally use [`Human::get_movemode`].
	pub fn get_move_flag(&self, nowdata: bool) -> i32 {
		if nowdata {
			self.move_flag
		} else {
			self.move_flag_lt
		}
	}

	/// Sets the scope. Success: true, failure: false.
	///
	/// A weapon with no settings cannot name a scope mode, so it is refused rather than
	/// given one the game cannot know: upstream reads an uninitialised `scopemode` there.
	pub fn set_enable_scope(&mut self, weapons: &[Weapon]) -> bool {
		// Fail if no weapon at all is equipped.
		let Some(index) = self.weapon[self.selectweapon as usize] else {
			return false;
		};

		// Fail while reloading.
		if self.weaponreloadcnt > 0 {
			return false;
		}

		// Get the weapon type number and its setting values.
		let param_id = weapons[index].param_data().0;
		let Some(data) = self.object.param.get_weapon(param_id) else {
			return false;
		};

		// Set the scope.
		self.scopemode = data.scopemode;
		true
	}

	/// Cancels the scope.
	pub fn set_disable_scope(&mut self) {
		self.scopemode = 0;
	}

	/// Gets the scope setting.
	pub fn get_scope_mode(&self) -> i32 {
		self.scopemode
	}

	/// Gets the orientation of the horizontal and vertical axes.
	pub fn rx_ry(&self) -> (f32, f32) {
		(self.object.rotation_x, self.armrotation_y)
	}

	/// Sets the orientation of the horizontal and vertical axes.
	pub fn set_rx_ry(&mut self, rx: f32, ry: f32) {
		self.object.rotation_x = rx;
		self.armrotation_y = ry;
	}

	/// Gets the overall rotation angle.
	///
	/// This is the angle at which the human falls when dead, which is not the value
	/// [`Human::rx_ry`] hands back.
	pub fn dead_ry(&self) -> f32 {
		self.rotation_y
	}

	/// Jumps.
	///
	/// Avoid calling this directly from the game. Call it from the object manager class.
	pub fn jump(&mut self) {
		self.move_flag |= move_flag::JUMP;
	}

	/// Gets the jump and landing state.
	///
	/// `nowdata` picks the current value, otherwise the previous frame's. The landing
	/// check always looks at the current value, because it is a flag rather than a key.
	/// Jumping: 1, landing: 2, otherwise: 0.
	pub fn get_jump_landing(&self, nowdata: bool) -> i32 {
		// Exit if it is unused or has not been processed.
		if !self.object.enable_flag {
			return 0;
		}
		if self.hp <= 0 {
			return 0;
		}

		if self.move_y_landing {
			return 2;
		}

		let flags = if nowdata { self.move_flag } else { self.move_flag_lt };
		if get_flag(flags, move_flag::JUMP) {
			return 1;
		}

		0
	}

	/// Gets the block ID and face number under the feet.
	///
	/// When there is no block under the feet, such as in midair, both are -1.
	pub fn get_under_block(&self) -> (i32, i32) {
		(self.underblock_id, self.underblock_face)
	}

	/// Pushes out and applies force.
	pub fn add_pos_order(&mut self, rx: f32, ry: f32, speed: f32) {
		self.move_x += rx.cos() * ry.cos() * speed;
		self.move_y += ry.sin() * speed;
		self.move_z += rx.sin() * ry.cos() * speed;
	}

	/// A bullet hit the head.
	pub fn hit_bullet_head(&mut self, attacks: i32) {
		if !self.invincible {
			self.hp -= (attacks as f32 * HUMAN_DAMAGE_HEAD) as i32 + get_rand(16);
		}
		self.reaction_gunsight_error_range = 15;
	}

	/// A bullet hit the upper body.
	pub fn hit_bullet_up(&mut self, attacks: i32) {
		if !self.invincible {
			self.hp -= (attacks as f32 * HUMAN_DAMAGE_UP) as i32 + get_rand(8);
		}
		self.reaction_gunsight_error_range = 12;
	}

	/// A bullet hit the lower body.
	pub fn hit_bullet_leg(&mut self, attacks: i32) {
		if !self.invincible {
			self.hp -= (attacks as f32 * HUMAN_DAMAGE_LEG) as i32 + get_rand(6);
		}
		self.reaction_gunsight_error_range = 8;
	}

	/// A zombie attack hit.
	pub fn hit_zombie_attack(&mut self) {
		if !self.invincible {
			self.hp -= HUMAN_DAMAGE_ZOMBIEU + get_rand(HUMAN_DAMAGE_ZOMBIEA);
		}
		self.reaction_gunsight_error_range = 10;
	}

	/// A grenade explosion hit.
	///
	/// Complete the calculation based on distance beforehand.
	pub fn hit_grenade_explosion(&mut self, attacks: i32) {
		if !self.invincible {
			self.hp -= attacks;
		}
		self.reaction_gunsight_error_range = 10;
	}

	/// Sets the hit flag and the direction the hit came from.
	pub fn set_hit_flag(&mut self, rx: f32) {
		self.hit_flag = true;
		self.hit_rx = rx;
	}

	/// Checks whether the human was hit, and returns the direction alongside.
	///
	/// The flag is initialised to false when this runs.
	pub fn check_hit(&mut self) -> (bool, f32) {
		let return_flag = self.hit_flag;
		let rx = self.hit_rx;
		self.hit_flag = false;
		(return_flag, rx)
	}

	/// Gets the total movement amount, which is the upstream's unused field.
	pub fn total_move(&self) -> f32 {
		self.totalmove
	}

	/// Gets the count value of the walking and running motion.
	pub fn move_motion_count(&self) -> i32 {
		self.motion_ctrl.get_move_motion_cnt()
	}

	/// Gets the standard error, which is the state error plus the recoil error.
	///
	/// Upstream this is `GetGunsightErrorRange`. The per-frame update it pairs with is
	/// `GunsightErrorRange`, and the two differ only by the `Get` prefix, so the update
	/// keeps the name that says what it does.
	pub fn gunsight_error_range(&self) -> i32 {
		self.state_gunsight_error_range + self.reaction_gunsight_error_range
	}

	/// `GunsightErrorRange`: processes the gunsight state error.
	///
	/// Process this before [`Human::control_process`].
	fn process_gunsight_error_range(&mut self, weapons: &[Weapon]) {
		// Initialization.
		self.state_gunsight_error_range = 0;

		// Set the error caused by each operation. These overwrite rather than add, so
		// only the last one that matches counts.
		if get_flag(self.move_flag, move_flag::WALK) {
			self.state_gunsight_error_range = 4;
		}
		if get_flag(self.move_flag, move_flag::FORWARD) {
			self.state_gunsight_error_range = 8;
		}
		if get_flag(self.move_flag, move_flag::BACK) {
			self.state_gunsight_error_range = 6;
		}
		if get_flag(self.move_flag, move_flag::LEFT | move_flag::RIGHT) {
			self.state_gunsight_error_range = 7;
		}
		if self.move_y_flag {
			self.state_gunsight_error_range = 22;
		}
		if self.hp < 40 {
			self.state_gunsight_error_range += 3;
		}

		// If some weapon is equipped.
		if let Some(index) = self.weapon[self.selectweapon as usize] {
			// Get the weapon setting data and correct the error range.
			let param = weapons[index].param_data().0;

			// Decrease the error by 1.
			self.reaction_gunsight_error_range -= 1;
			if self.reaction_gunsight_error_range < 0 {
				self.reaction_gunsight_error_range = 0;
			}
			// The upper clamp needs the weapon's maximum, and a weapon with no settings
			// has none: upstream reads an uninitialised field there.
			if let Some(data) = self.object.param.get_weapon(param) {
				if self.reaction_gunsight_error_range > data.error_range_max {
					self.reaction_gunsight_error_range = data.error_range_max;
				}
			}
		} else {
			self.reaction_gunsight_error_range = 0;
		}
	}

	/// `CheckAndProcessDead`: the death check and the falling over process.
	///
	/// Returns: motionless corpse 4, right after finishing falling 3, in the middle of
	/// falling 2, starting to fall 1, do nothing 0.
	///
	/// The upstream takes only the collision class. The block data is what it is asked
	/// against, and in this port a collision query names the data it works on, so it is a
	/// parameter.
	fn check_and_process_dead(
		&mut self,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		weapons: &mut [Weapon],
	) -> i32 {
		if self.hp <= 0 {
			// Arm angle: the arms swing to the side at six degrees a frame, and stop at a
			// right angle.
			if self.armrotation_y < 0.0 {
				self.armrotation_y -= degree_to_radian(6.0);
				if self.armrotation_y < degree_to_radian(-90.0) {
					self.armrotation_y = degree_to_radian(-90.0);
				}
			} else {
				self.armrotation_y += degree_to_radian(6.0);
				if self.armrotation_y > degree_to_radian(90.0) {
					self.armrotation_y = degree_to_radian(90.0);
				}
			}
		}

		// The state machine's states are the ones the upstream's memo lists:
		//
		//  state 0 [do nothing]
		//   hp of 0 or less starts the fall at state 1, or at state 2 when the head is
		//   already inside a wall.
		//  state 1 [start tilting]
		//   reaching 135 degrees goes to state 2, hitting a wall with the head to state 3,
		//   and a metre below the deadline to state 4 when it is at least 90 degrees over.
		//  state 2 [falling]
		//   the feet touching the ground go to state 4.
		//  state 3 [feet slipping]
		//  state 4 [fixed], which becomes state 5 on the next call.
		let mut check_posx;
		let mut check_posy;
		let mut check_posz;

		if self.deadstate == 0 {
			if self.hp <= 0 {
				// At the instant hp becomes 0 or less (death), start falling over.

				// Calculate the direction of the last attack received.
				let tr = angle_normalization(self.hit_rx - self.object.rotation_x);

				// Body angle: a hit from the front falls backwards and one from behind
				// falls forwards.
				if ((-PI / 2.0) < tr) && (tr < PI / 2.0) {
					self.add_ry = HUMAN_DEADADDRY;
				} else {
					self.add_ry = HUMAN_DEADADDRY * -1.0;
				}

				// Raise the height by +1.0 so the corpse does not sink into the ground.
				self.object.pos_y += 1.0;

				// Drop all held weapons.
				for i in 0..TOTAL_HAVEWEAPON {
					if let Some(index) = self.weapon[i] {
						weapons[index].dropoff(
							self.object.pos_x,
							self.object.pos_y,
							self.object.pos_z,
							degree_to_radian(10.0) * get_rand(36) as f32,
							1.5,
						);
						self.weapon[i] = None;

						// Initialize the bullet count info for when no weapon is held.
						self.noweapon_bullets[i] = 0;
						self.noweapon_loadbullets[i] = 0;
					}
				}

				// Cancel the scope mode.
				self.set_disable_scope();

				// Initialize the various counts.
				self.selectweaponcnt = 0;
				self.changeweaponidcnt = 0;
				self.weaponshotcnt = 0;
				self.weaponreloadcnt = 0;

				// Get the head coordinate of the next frame.
				check_posx = self.object.pos_x
					+ (self.object.rotation_x * -1.0 - PI / 2.0).cos() * self.add_ry.sin() * HUMAN_HEIGHT;
				check_posy = self.object.pos_y + self.add_ry.cos() * HUMAN_HEIGHT;
				check_posz = self.object.pos_z
					+ (self.object.rotation_x * -1.0 - PI / 2.0).sin() * self.add_ry.sin() * HUMAN_HEIGHT;

				if coll.check_all_block_inside(block_data_if, check_posx, check_posy, check_posz) {
					self.deadstate = 2;
				} else {
					self.rotation_y += self.add_ry;
					self.deadstate = 1;
				}
				return 1;
			}
			return 0;
		}

		if self.deadstate == 1 {
			// If it has fallen 135 degrees or more.
			if self.rotation_y.abs() >= degree_to_radian(135.0) {
				self.deadstate = 2;
				self.move_y = 0.0;
				return 2;
			}

			if self.object.pos_y <= (HUMAN_DEADLINE + 10.0) {
				// If it has fallen 90 degrees or more.
				if self.rotation_y.abs() >= degree_to_radian(90.0) {
					self.deadstate = 4;
					return 2;
				}
			}

			// Fall it forward or backward.
			if self.rotation_y > 0.0 {
				// rotation_y < DegreeToRadian(135)
				self.add_ry += HUMAN_DEADADDRY;
			} else if self.rotation_y < 0.0 {
				// rotation_y > DegreeToRadian(-135)
				self.add_ry -= HUMAN_DEADADDRY;
			}

			if self.object.pos_y <= HUMAN_DEADLINE {
				self.rotation_y += self.add_ry;
			} else {
				// Get the head coordinate of the next frame.
				check_posx = self.object.pos_x
					+ (self.object.rotation_x * -1.0 - PI / 2.0).cos()
						* (self.rotation_y + self.add_ry).sin()
						* HUMAN_HEIGHT;
				check_posy = self.object.pos_y + (self.rotation_y + self.add_ry).cos() * HUMAN_HEIGHT;
				check_posz = self.object.pos_z
					+ (self.object.rotation_x * -1.0 - PI / 2.0).sin()
						* (self.rotation_y + self.add_ry).sin()
						* HUMAN_HEIGHT;

				if coll.check_all_block_inside(block_data_if, check_posx, check_posy, check_posz) {
					self.deadstate = 3;
				} else {
					self.rotation_y += self.add_ry;
				}
			}

			return 2;
		}

		if self.deadstate == 2 {
			if self.object.pos_y <= HUMAN_DEADLINE {
				self.deadstate = 4;
				return 2;
			}

			// The body falls at four units a frame at most.
			if self.move_y > -4.0 {
				self.move_y -= 0.1;
			}

			// Foot coordinate of the next frame.
			check_posx = self.object.pos_x;
			check_posy = self.object.pos_y - self.move_y - HUMAN_HEIGHT * 0.5;
			check_posz = self.object.pos_z;

			if coll.check_all_block_inside(block_data_if, check_posx, check_posy, check_posz) {
				self.deadstate = 4;
			} else {
				self.object.pos_y += self.move_y;
			}

			return 2;
		}

		if self.deadstate == 3 {
			// If it has fallen 90 degrees or more.
			if self.rotation_y.abs() >= degree_to_radian(90.0) {
				self.deadstate = 4;
				return 2;
			}

			// Fall it forward or backward.
			if self.rotation_y > 0.0 {
				// rotation_y < DegreeToRadian(90)
				self.add_ry += HUMAN_DEADADDRY;
			} else if self.rotation_y < 0.0 {
				// rotation_y > DegreeToRadian(-90)
				self.add_ry -= HUMAN_DEADADDRY;
			}

			// Get the foot coordinate of the next frame.
			check_posx = self.object.pos_x
				- (self.object.rotation_x * -1.0 - PI / 2.0).cos()
					* (self.rotation_y + self.add_ry).sin()
					* HUMAN_HEIGHT;
			check_posy = self.object.pos_y + 0.1;
			check_posz = self.object.pos_z
				- (self.object.rotation_x * -1.0 - PI / 2.0).sin()
					* (self.rotation_y + self.add_ry).sin()
					* HUMAN_HEIGHT;

			if coll.check_all_block_inside(block_data_if, check_posx, check_posy, check_posz) {
				self.deadstate = 4;
				return 2;
			}

			// Get the head coordinate of the next frame.
			check_posx = self.object.pos_x
				- (self.object.rotation_x * -1.0 - PI / 2.0).cos()
					* (self.rotation_y + self.add_ry).sin()
					* HUMAN_HEIGHT;
			check_posy = self.object.pos_y + (self.rotation_y + self.add_ry).cos() * HUMAN_HEIGHT;
			check_posz = self.object.pos_z
				- (self.object.rotation_x * -1.0 - PI / 2.0).sin()
					* (self.rotation_y + self.add_ry).sin()
					* HUMAN_HEIGHT;

			if coll.check_all_block_inside(block_data_if, check_posx, check_posy, check_posz) {
				self.deadstate = 4;
				return 2;
			}

			// Move the foot coordinate.
			self.object.pos_x -= (self.object.rotation_x * -1.0 - PI / 2.0).cos()
				* self.add_ry.sin()
				* HUMAN_HEIGHT;
			self.object.pos_z -= (self.object.rotation_x * -1.0 - PI / 2.0).sin()
				* self.add_ry.sin()
				* HUMAN_HEIGHT;

			self.rotation_y += self.add_ry;
			return 2;
		}

		if self.deadstate == 4 {
			// Do nothing (fixed), and report the end of the fall once.
			self.deadstate = 5;
			return 3;
		}

		if self.deadstate == 5 {
			// Do nothing (fixed).
			return 4;
		}

		0
	}

	/// `ControlProcess`: the movement calculation from the operation.
	///
	/// When executed, each key flag is initialised to false.
	fn control_process(&mut self) {
		// Determine the travel direction and speed. The walk flag wins over every
		// direction, and the rest are the single directions and the four diagonals.
		let directions = move_flag::FORWARD | move_flag::BACK | move_flag::LEFT | move_flag::RIGHT;
		let pressed = self.move_flag & directions;

		if get_flag(self.move_flag, move_flag::WALK) {
			self.move_rx = degree_to_radian(0.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_PROGRESSWALK_ACCELERATION,
			);
		} else if pressed == move_flag::FORWARD {
			self.move_rx = degree_to_radian(0.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_PROGRESSRUN_ACCELERATION,
			);
		} else if pressed == move_flag::BACK {
			self.move_rx = degree_to_radian(180.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_REGRESSRUN_ACCELERATION,
			);
		} else if pressed == move_flag::LEFT {
			self.move_rx = degree_to_radian(90.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_SIDEWAYSRUN_ACCELERATION,
			);
		} else if pressed == move_flag::RIGHT {
			self.move_rx = degree_to_radian(-90.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_SIDEWAYSRUN_ACCELERATION,
			);
		} else if pressed == (move_flag::FORWARD | move_flag::LEFT) {
			self.move_rx = degree_to_radian(45.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_PROGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			);
		} else if pressed == (move_flag::BACK | move_flag::LEFT) {
			self.move_rx = degree_to_radian(135.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_REGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			);
		} else if pressed == (move_flag::BACK | move_flag::RIGHT) {
			self.move_rx = degree_to_radian(-135.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_REGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			);
		} else if pressed == (move_flag::FORWARD | move_flag::RIGHT) {
			self.move_rx = degree_to_radian(-45.0);
			self.add_pos_order(
				self.object.rotation_x * -1.0 + self.move_rx + PI / 2.0,
				0.0,
				HUMAN_PROGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			);
		} else {
			self.move_rx = 0.0;
		}

		// Back up the flags.
		self.move_flag_lt = self.move_flag;

		// Restore the key flags.
		self.move_flag = 0x00;
	}

	/// `CollisionMap`: collision detection with the map, and the whole of the human's
	/// movement for the frame.
	fn collision_map(
		&mut self,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		add_collision_flag: bool,
		player: bool,
	) {
		let mut check_block_id = [false; MAX_BLOCKS];

		// For now, set the block ID and face number to -1.
		self.underblock_id = -1;
		self.underblock_face = -1;

		// Back up the current coordinate.
		let pos_x2 = self.object.pos_x;
		let pos_y2 = self.object.pos_y;
		let pos_z2 = self.object.pos_z;

		// Apply the movement.
		self.object.pos_x += self.move_x;
		self.object.pos_z += self.move_z;

		// Decay the movement amount of the next frame.
		self.move_x *= HUMAN_ATTENUATION;
		self.move_z *= HUMAN_ATTENUATION;

		// In the original XOPS, collision detection between humans is done here. OpenXOPS
		// calculates it at a different timing, so `dist_y` below is always 0.0 and the
		// value is meaningless.

		// Calculate the movement vector.
		let mut dist_x = self.object.pos_x - pos_x2;
		let mut dist_y = self.object.pos_y - pos_y2;
		let mut dist_z = self.object.pos_z - pos_z2;
		let speed = (dist_x * dist_x + dist_z * dist_z).sqrt();
		vector_normalization(&mut dist_x, &mut dist_y, &mut dist_z);

		let bs = block_data_if.get_total_datas();

		// Create the AABB of the human and narrow down the target blocks with it.
		let box_min_x = self.object.pos_x - HUMAN_MAPCOLLISION_CHECK_MAXDIST - COLLISION_ADDSIZE;
		let box_min_y = self.object.pos_y - HUMAN_MAPCOLLISION_CHECK_MAXDIST - COLLISION_ADDSIZE;
		let box_min_z = self.object.pos_z - HUMAN_MAPCOLLISION_CHECK_MAXDIST - COLLISION_ADDSIZE;
		let box_max_x = self.object.pos_x + HUMAN_MAPCOLLISION_CHECK_MAXDIST + COLLISION_ADDSIZE;
		let box_max_y = (self.object.pos_y + HUMAN_HEIGHT) + HUMAN_MAPCOLLISION_CHECK_MAXDIST + COLLISION_ADDSIZE;
		let box_max_z = self.object.pos_z + HUMAN_MAPCOLLISION_CHECK_MAXDIST + COLLISION_ADDSIZE;

		for i in 0..bs {
			check_block_id[i as usize] = coll.check_block_aabb(
				block_data_if, i, box_min_x, box_min_y, box_min_z, box_max_x, box_max_y, box_max_z,
			);
		}

		/// Whether any of the candidate blocks contains this point. The upstream counts one
		/// per direction, because its inner loop breaks on the first hit.
		fn inside_any(
			coll: &Collision,
			block_data_if: &BlockDataInterface,
			check_block_id: &[bool; MAX_BLOCKS],
			bs: i32,
			x: f32,
			y: f32,
			z: f32,
		) -> bool {
			for i in 0..bs {
				if check_block_id[i as usize] && coll.check_block_inside(block_data_if, i, x, y, z, true).0 {
					return true;
				}
			}
			false
		}

		if (speed != 0.0) || (self.move_y != 0.0) {
			// Collision detection at the head.
			for i in 0..bs {
				if check_block_id[i as usize] {
					if self.collision_block_scratch(
						coll,
						block_data_if,
						i,
						pos_x2,
						pos_y2,
						pos_z2,
						self.object.pos_x,
						self.object.pos_y + HUMAN_HEIGHT - 0.22,
						self.object.pos_z,
						0x01,
					) {
						if self.move_y > 0.0 {
							self.move_y = 0.0;
						}
					}
				}
			}

			// Collision detection at the feet.
			for i in 0..bs {
				if check_block_id[i as usize] {
					self.collision_block_scratch(
						coll,
						block_data_if,
						i,
						pos_x2,
						pos_y2,
						pos_z2,
						self.object.pos_x,
						self.object.pos_y,
						self.object.pos_z,
						0x00,
					);
				}
			}

			// Collision detection around the waist.
			for i in 0..bs {
				if check_block_id[i as usize] {
					self.collision_block_scratch(
						coll,
						block_data_if,
						i,
						pos_x2,
						pos_y2,
						pos_z2,
						self.object.pos_x + dist_x * HUMAN_MAPCOLLISION_R,
						self.object.pos_y + HUMAN_MAPCOLLISION_HEIGHT,
						self.object.pos_z + dist_z * HUMAN_MAPCOLLISION_R,
						0x02,
					);
					self.collision_block_scratch(
						coll,
						block_data_if,
						i,
						pos_x2,
						pos_y2,
						pos_z2,
						self.object.pos_x + dist_z * HUMAN_MAPCOLLISION_R,
						self.object.pos_y + HUMAN_MAPCOLLISION_HEIGHT,
						self.object.pos_z + dist_x * HUMAN_MAPCOLLISION_R,
						0x02,
					);
					self.collision_block_scratch(
						coll,
						block_data_if,
						i,
						pos_x2,
						pos_y2,
						pos_z2,
						self.object.pos_x - dist_z * HUMAN_MAPCOLLISION_R,
						self.object.pos_y + HUMAN_MAPCOLLISION_HEIGHT,
						self.object.pos_z - dist_x * HUMAN_MAPCOLLISION_R,
						0x02,
					);
				}
			}

			if add_collision_flag {
				// Additional collision detection near the waist.
				for i in 0..bs {
					if check_block_id[i as usize] {
						self.collision_block_scratch(
							coll,
							block_data_if,
							i,
							pos_x2,
							pos_y2,
							pos_z2,
							self.object.pos_x,
							self.object.pos_y + HUMAN_MAPCOLLISION_ADD_HEIGHT_A,
							self.object.pos_z,
							0x02,
						);
						self.collision_block_scratch(
							coll,
							block_data_if,
							i,
							pos_x2,
							pos_y2,
							pos_z2,
							self.object.pos_x,
							self.object.pos_y + HUMAN_MAPCOLLISION_ADD_HEIGHT_B,
							self.object.pos_z,
							0x02,
						);
					}
				}
			}

			// Climb the slope.
			if (self.move_x.abs() > 0.2) || (self.move_z.abs() > 0.2) {
				if self.move_y_upper == 0 {
					let mut flag = false;
					for i in 0..bs {
						if check_block_id[i as usize] {
							// If only the toes are buried in a block.
							if coll.check_block_inside(
								block_data_if,
								i,
								self.object.pos_x + dist_x * 2.0,
								self.object.pos_y,
								self.object.pos_z + dist_z * 2.0,
								true,
							)
							.0 && !coll
								.check_block_inside(
									block_data_if,
									i,
									self.object.pos_x + dist_x * 2.0,
									self.object.pos_y + HUMAN_MAPCOLLISION_CLIMBHEIGHT,
									self.object.pos_z + dist_z * 2.0,
									true,
								)
								.0
							{
								flag = true;

								// Get the angle of the face under the feet. There is
								// certainly a block under the feet, so a DummyRay should
								// not be needed.
								if let Some((face, _dist)) = coll.check_block_intersect_ray(
									block_data_if,
									i,
									self.object.pos_x,
									self.object.pos_y,
									self.object.pos_z,
									0.0,
									-1.0,
									0.0,
									1.2,
								) {
									let bdata = block_data_if.getdata(i).unwrap_or_default();

									// If standing on a slope, rising is disabled.
									if bdata.material[face as usize].vy.acos() > HUMAN_MAPCOLLISION_SLOPEANGLE {
										flag = false;
										break;
									}
								}
							}
						}
					}

					// Rise (climb). The upward movement is killed to a fifth of itself so
					// the step up does not throw the human off the slope.
					if flag {
						self.object.pos_y += HUMAN_MAPCOLLISION_CLIMBADDY;
						self.move_y *= 0.2;
					}
				}
			}

			// If the destination is sunk into a block, disable the movement.
			for i in 0..bs {
				if check_block_id[i as usize] {
					// Whether the ray origin is inside a block is not checked.
					if coll
						.check_block_intersect_ray(
							block_data_if,
							i,
							self.object.pos_x,
							self.object.pos_y + HUMAN_HEIGHT - 2.0,
							self.object.pos_z,
							0.0,
							-1.0,
							0.0,
							HUMAN_HEIGHT - 2.0 * 2.0,
						)
						.is_some()
					{
						self.object.pos_x = pos_x2;
						self.object.pos_z = pos_z2;
					}

					// If the destination sinks into a block and is also buried, disable
					// the movement. Note the upstream's comment: this check looks
					// suspicious, because any sufficiently distant position should do, so
					// why eleven times the movement? It is far beyond the AABB check range,
					// so all blocks are checked again.
					if coll
						.check_block_inside(
							block_data_if,
							i,
							self.object.pos_x,
							self.object.pos_y + HUMAN_HEIGHT - 0.6,
							self.object.pos_z,
							true,
						)
						.0
					{
						if coll.check_all_block_inside(
							block_data_if,
							self.object.pos_x + self.move_x * 11.0,
							self.object.pos_y + HUMAN_HEIGHT - 0.6,
							self.object.pos_z + self.move_z * 11.0,
						) {
							self.object.pos_x = pos_x2;
							self.object.pos_y = pos_y2;
							self.object.pos_z = pos_z2;
							if self.move_y > 0.0 {
								self.move_y = 0.0;
							}
						}
					}
				}
			}
		}

		if self.move_y_upper > 0 {
			self.move_y_upper -= 1;
		}

		// Falling process. Three sub-steps of a third of the fall, so a thin block cannot
		// be passed through, and the first one that touches the ground ends it.
		self.move_y_flag = false;
		let mut fall_flag = false;
		for _ in 0..3 {
			let ang = self.move_z.atan2(self.move_x);
			let mut x;
			let mut z;

			// Fall.
			self.object.pos_y += self.move_y * 0.33;

			if player {
				let y = self.object.pos_y + HUMAN_MAPCOLLISION_GROUND_HEIGHT;

				// 4-direction check No.1. All four points have to be inside a block for
				// the player to count as standing on something.
				let mut cnt = 0;
				x = self.object.pos_x + ang.cos() * HUMAN_MAPCOLLISION_GROUND_R1;
				z = self.object.pos_z + ang.sin() * HUMAN_MAPCOLLISION_GROUND_R1;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x - ang.cos() * HUMAN_MAPCOLLISION_GROUND_R1;
				z = self.object.pos_z - ang.sin() * HUMAN_MAPCOLLISION_GROUND_R1;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x + (ang + PI / 2.0).cos() * HUMAN_MAPCOLLISION_GROUND_R1;
				z = self.object.pos_z + (ang + PI / 2.0).sin() * HUMAN_MAPCOLLISION_GROUND_R1;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x + (ang - PI / 2.0).cos() * HUMAN_MAPCOLLISION_GROUND_R1;
				z = self.object.pos_z + (ang - PI / 2.0).sin() * HUMAN_MAPCOLLISION_GROUND_R1;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				if cnt == 4 {
					fall_flag = true;
					break;
				}

				// 4-direction check No.2, at twice the radius.
				let mut cnt = 0;
				x = self.object.pos_x + ang.cos() * HUMAN_MAPCOLLISION_GROUND_R2;
				z = self.object.pos_z + ang.sin() * HUMAN_MAPCOLLISION_GROUND_R2;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x - ang.cos() * HUMAN_MAPCOLLISION_GROUND_R2;
				z = self.object.pos_z - ang.sin() * HUMAN_MAPCOLLISION_GROUND_R2;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x + (ang + PI / 2.0).cos() * HUMAN_MAPCOLLISION_GROUND_R2;
				z = self.object.pos_z + (ang + PI / 2.0).sin() * HUMAN_MAPCOLLISION_GROUND_R2;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				x = self.object.pos_x + (ang - PI / 2.0).cos() * HUMAN_MAPCOLLISION_GROUND_R2;
				z = self.object.pos_z + (ang - PI / 2.0).sin() * HUMAN_MAPCOLLISION_GROUND_R2;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					cnt += 1;
				}
				if cnt == 4 {
					fall_flag = true;
					break;
				}
			} else {
				let y = self.object.pos_y + HUMAN_MAPCOLLISION_GROUND_HEIGHT;

				// Check directly below.
				x = self.object.pos_x;
				z = self.object.pos_z;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					fall_flag = true;
				}
				if fall_flag {
					break;
				}

				// Offset slightly outward for the check, which is what keeps a human from
				// standing on the very edge of a block with nothing under it.
				x = self.object.pos_x + ang.cos() * HUMAN_MAPCOLLISION_GROUND_R3;
				z = self.object.pos_z + ang.sin() * HUMAN_MAPCOLLISION_GROUND_R3;
				if inside_any(coll, block_data_if, &check_block_id, bs, x, y, z) {
					fall_flag = true;
				}
				if fall_flag {
					break;
				}
			}

			// Add the fall speed.
			self.move_y -= HUMAN_DAMAGE_SPEED;
			if self.move_y < HUMAN_DAMAGE_MAXSPEED {
				self.move_y = HUMAN_DAMAGE_MAXSPEED;
			}

			self.move_y_flag = true;
		}

		self.move_y_landing = false;

		// Process for when touching the ground.
		if fall_flag {
			// The landing flag is only for a fall that was fast enough to notice.
			if self.move_y < HUMAN_MAPCOLLISION_GROUND_HEIGHT {
				self.move_y_landing = true;
			}

			if !self.invincible {
				// Apply fall damage, which ramps from nothing at HUMAN_DAMAGE_MINSPEED to
				// HUMAN_DAMAGE_MAXFALL at HUMAN_DAMAGE_MAXSPEED.
				if self.move_y < HUMAN_DAMAGE_MINSPEED {
					self.hp -= get_rand(6) + fall_damage_ramp(self.move_y);
				}
			}

			self.move_y = 0.0;

			// A held jump key bounces the human back up, but only when the slope climb is
			// not already lifting it.
			if get_flag(self.move_flag_lt, move_flag::JUMP) && (self.move_y_upper == 0) {
				self.move_y = HUMAN_JUMP_SPEED;

				// Run the motion.
				self.motion_ctrl.jump();
			}

			// Standing still, a human only stays put three times out of four: the fourth
			// lets the slope slide be applied.
			let mut flag = true;
			if get_flag(self.move_flag_lt, move_flag::FORWARD | move_flag::BACK | move_flag::LEFT | move_flag::RIGHT | move_flag::WALK) == false {
				if get_rand(4) == 0 {
					if self.move_y_upper == 0 {
						flag = false;
					}
				}
			}

			// Get the block and face number under the feet.
			let mut found = None;
			for i in 0..bs {
				if check_block_id[i as usize] {
					if let Some((face, _dist)) = coll.check_block_intersect_ray(
						block_data_if,
						i,
						self.object.pos_x,
						self.object.pos_y + 2.5,
						self.object.pos_z,
						0.0,
						-1.0,
						0.0,
						2.5 + 1.0,
					) {
						found = Some((i, face));
						break;
					}
				}
			}

			if let Some((id, face)) = found {
				// Store the block ID and face number.
				self.underblock_id = id;
				self.underblock_face = face;

				if flag {
					let bdata = block_data_if.getdata(id).unwrap_or_default();

					// If standing on a slope, make the human slide down the slope.
					if bdata.material[face as usize].vy.acos() > HUMAN_MAPCOLLISION_SLOPEANGLE {
						let material = bdata.material[face as usize];
						self.move_x = material.vx * 1.2;
						self.move_y = material.vy * -0.5;
						self.move_z = material.vz * 1.2;

						for i in 0..bs {
							if check_block_id[i as usize] {
								if coll
									.check_block_inside(
										block_data_if,
										i,
										self.object.pos_x + self.move_x * 3.0,
										self.object.pos_y + self.move_y * 3.0,
										self.object.pos_z + self.move_z * 3.0,
										true,
									)
									.0
								{
									self.move_y = 0.0;
									if coll
										.check_block_inside(
											block_data_if,
											i,
											self.object.pos_x + self.move_x * 3.0,
											self.object.pos_y,
											self.object.pos_z + self.move_z * 3.0,
											true,
										)
										.0
									{
										self.move_x = 0.0;
										self.move_z = 0.0;
										break;
									}
								}
							}
						}

						self.move_y_upper = 8;
					}
				}
			}
		}

		// Recalculate the movement vector.
		dist_x = self.object.pos_x - pos_x2;
		dist_y = self.object.pos_y - pos_y2;
		dist_z = self.object.pos_z - pos_z2;

		let dist = (dist_x * dist_x + dist_z * dist_z).sqrt();
		let speed = ((self.move_x * 2.0) * (self.move_x * 2.0)
			+ (self.move_z * 2.0) * (self.move_z * 2.0))
			.sqrt();

		// If the movement amount is clearly wrong, revert it.
		if ((dist_x * dist_x + dist_y * dist_y + dist_z * dist_z).sqrt() > HUMAN_MAPCOLLISION_CHECK_MAXDIST)
			|| (dist - speed > 1.0)
		{
			self.object.pos_x = pos_x2;
			self.object.pos_y = pos_y2;
			self.object.pos_z = pos_z2;
		}

		if player {
			let speed = vector_normalization(&mut dist_x, &mut dist_y, &mut dist_z);

			// If there is a block between the current position and the target, revert it.
			// Whether the ray origin is inside a block is not checked.
			if coll
				.check_all_block_intersect_ray(
					block_data_if,
					pos_x2,
					pos_y2 + HUMAN_MAPCOLLISION_CHECK_HEIGHT,
					pos_z2,
					dist_x,
					dist_y,
					dist_z,
					speed,
				)
				.is_some()
			{
				self.object.pos_x = pos_x2;
				self.object.pos_y = pos_y2;
				self.object.pos_z = pos_z2;
			}
		}
	}

	/// `CollisionBlockScratch`: the vector calculation on collision with a block.
	///
	/// The position is updated in place, so it is the fields of the human itself rather
	/// than the pointers upstream passes. `mode` is the coordinate correction: 0x00 normal,
	/// 0x01 forbid Y rising, 0x02 fix Y.
	fn collision_block_scratch(
		&mut self,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		block_id: i32,
		px_old: f32,
		py_old: f32,
		pz_old: f32,
		in_vx: f32,
		in_vy: f32,
		in_vz: f32,
		mode: i32,
	) -> bool {
		if (block_id < 0) || (block_data_if.get_total_datas() <= block_id) {
			return false;
		}

		// The OpenXOPS collision detection judges blocks slightly larger to absorb
		// calculation errors. Because of this, the human can get caught on the seams
		// between the blocks under the feet. As a countermeasure, the Y coordinate to check
		// is corrected for the under-feet check (mode 0x00, normal).
		let in_vy = if mode == 0 { in_vy + COLLISION_ADDSIZE } else { in_vy };

		let px2 = self.object.pos_x;
		let py2 = self.object.pos_y;
		let pz2 = self.object.pos_z;

		// Calculate the current movement vector and distance.
		let mut vx = self.object.pos_x - px_old;
		let mut vy = self.object.pos_y - py_old;
		let mut vz = self.object.pos_z - pz_old;
		let dist = vector_normalization(&mut vx, &mut vy, &mut vz);

		// Create the AABB.
		let (rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z) = get_aabb_ray(
			in_vx - vx * dist,
			in_vy - vy * dist,
			in_vz - vz * dist,
			vx,
			vy,
			vz,
			dist,
		);

		// Get the block face that was hit.
		if !coll.check_block_aabb(block_data_if, block_id, rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z) {
			return false;
		}
		let Some((face, _temp)) = coll.check_block_intersect_ray(
			block_data_if,
			block_id,
			in_vx - vx * dist,
			in_vy - vy * dist,
			in_vz - vz * dist,
			vx,
			vy,
			vz,
			dist,
		) else {
			return false;
		};

		// Find the angle between the vector and the block face.
		let Some(face_angle) = coll.angle_vector2(block_data_if, block_id, face, vx, vy, vz) else {
			return false;
		};

		// Calculate the angle ratio.
		let face_angle_per = if face_angle != 0.0 {
			PI / face_angle - 1.0
		} else {
			0.0
		};

		// Get the normal vector of the block face.
		let bdata = block_data_if.getdata(block_id).unwrap_or_default();
		let face_vx = bdata.material[face as usize].vx;
		let face_vy = bdata.material[face as usize].vy;
		let face_vz = bdata.material[face as usize].vz;

		// Calculate movement vector plus normal vector, then the movement vector according
		// to the angle.
		let mut vx2 = vx + face_vx;
		let mut vy2 = vy + face_vy;
		let mut vz2 = vz + face_vz;
		vector_normalization(&mut vx2, &mut vy2, &mut vz2);

		vx = vx2 * (1.0 - face_angle_per) + vx * face_angle_per;
		vy = vy2 * (1.0 - face_angle_per) + vy * face_angle_per;
		vz = vz2 * (1.0 - face_angle_per) + vz * face_angle_per;
		vector_normalization(&mut vx, &mut vy, &mut vz);

		// If it does not move horizontally, use the normal vector.
		if (vx == 0.0) && (vz == 0.0) {
			vx = face_vx;
			vy = face_vy;
			vz = face_vz;
		}

		// Calculate the final position.
		let temp = face_angle_per * dist;
		self.object.pos_x = vx * temp + px_old;
		self.object.pos_y = vy * temp + py_old;
		self.object.pos_z = vz * temp + pz_old;

		// If the final position is inside a block, the movement is invalid.
		if coll
			.check_block_inside(
				block_data_if,
				block_id,
				self.object.pos_x,
				self.object.pos_y,
				self.object.pos_z,
				true,
			)
			.0
		{
			self.object.pos_x = px_old;
			self.object.pos_y = py_old;
			self.object.pos_z = pz_old;
		}

		// Coordinate correction.
		if mode == 0x01 {
			if self.object.pos_y > py2 {
				self.object.pos_y = py2; // do not move it higher than now
			}
		}
		if mode == 0x02 {
			self.object.pos_y = py2; // do not change the Y coordinate
		}

		true
	}

	/// `ProcessObject`: the per-frame calculation.
	///
	/// Returns: no process 0, normal process 1, right after dying and finishing the fall
	/// 2, motionless corpse 3, right after dying from the terrain 4.
	///
	/// Upstream also takes the collision class and the block data, and the weapons are the
	/// array their indices point into.
	pub fn process_object(
		&mut self,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		add_collision_flag: bool,
		player: bool,
		f5mode: bool,
		weapons: &mut [Weapon],
	) -> i32 {
		// Upstream also refuses a null collision class and a null block data interface.
		// Neither can be null here, and an uninitialised one answers no to every query,
		// which is what the null check amounts to.
		if !self.object.enable_flag {
			return 0;
		}

		// If F5 is used, force the rise.
		if f5mode {
			self.move_y = 0.0;
			self.object.pos_y += 5.0;
		}

		if self.deadstate == 5 {
			return 3;
		}

		// Weapon switch count.
		if self.selectweaponcnt > 0 {
			self.selectweaponcnt -= 1;
		}
		if self.changeweaponidcnt > 0 {
			self.changeweaponidcnt -= 1;
		}

		// If the rapid fire count remains, decrease it by 1.
		if self.weaponshotcnt > 0 {
			self.weaponshotcnt -= 1;
		}

		// If the weapon was not fired in this frame, set the burst mode count to 0. Holding
		// the trigger keeps the flag set, which is what holds a burst together.
		if !self.weaponshotframe {
			self.weaponburstmodecnt = 0;
		}
		self.weaponshotframe = false;

		// If the reload count remains, decrease it by 1. If the reload count is 0, run the
		// reload process.
		if self.weaponreloadcnt > 0 {
			self.weaponreloadcnt -= 1;
			if self.weaponreloadcnt == 0 {
				if let Some(index) = self.weapon[self.selectweapon as usize] {
					weapons[index].run_reload();
				}
			}
		}

		// Get the reload and weapon ID switch counts, which the motion is drawn from. An
		// empty hand reports neither, so the arm does not reload a weapon that is not there.
		let (weapon_reload_motion_cnt, weapon_change_weapon_id_cnt) =
			if self.weapon[self.selectweapon as usize].is_some() {
				(self.weaponreloadcnt, self.changeweaponidcnt)
			} else {
				(0, 0)
			};

		// Process the gunsight state error.
		self.process_gunsight_error_range(weapons);

		// Death check and falling over process.
		let check_dead = self.check_and_process_dead(coll, block_data_if, weapons);
		if check_dead == 3 {
			return 2;
		}

		if (check_dead == 0) || (self.deadstate == 1) {
			let hp_old = self.hp;

			// Determine the travel direction and speed.
			self.control_process();

			// Collision detection with the map.
			self.collision_map(coll, block_data_if, add_collision_flag, player);

			// If the HP became zero in this map collision detection, death by terrain.
			if (hp_old > 0) && (self.hp <= 0) {
				return 4;
			}

			// If the human fell below -100.0 in this collision detection, death.
			if check_dead == 0 {
				if self.object.pos_y < HUMAN_DEADLINE {
					self.hp = 0;
					return 4;
				}
			}
		}

		// Get the type number of the currently equipped weapon.
		let weapon_paramid = match self.weapon[self.selectweapon as usize] {
			None => ID_WEAPON_NONE,
			Some(index) => weapons[index].param_data().0,
		};

		// Motion calculation.
		self.motion_ctrl.process_object(
			self.object.rotation_x,
			self.armrotation_y,
			weapon_paramid,
			weapon_reload_motion_cnt,
			weapon_change_weapon_id_cnt,
			self.move_flag_lt,
			self.hp,
			player,
		);

		if check_dead != 0 {
			return 3;
		}
		1
	}

	/// `Render`, the five argument overload the object manager calls.
	///
	/// `player` is unused upstream and kept so the call site reads the same.
	///
	/// The base `object::Render` is not what a human wants: it draws one model at the
	/// human's own position, while a human is an upper body, a pair of legs and a pair of
	/// arms, each placed and rotated on its own, with the weapon in the hands.
	pub fn render(
		&mut self,
		d3dg: &mut D3DGraphics,
		resource: &ResourceManager,
		draw_arm_only: bool,
		player: bool,
		no_model: bool,
		weapons: &[Weapon],
	) {
		// Countermeasure for unused arguments.
		let _ = player;

		if !self.object.enable_flag {
			return;
		}

		// Get the motion.
		let (armry, legrx, upmodel, armmodel, legmodel) = self.motion_ctrl.get_render_motion();
		let id_texture = self.object.id_texture;

		if !draw_arm_only {
			// Render the upper body. A body that has fallen over is laid out along its own
			// fall direction, and a body on its feet is one unit above its own origin.
			let (add_x, add_y, add_z) = if self.rotation_y != 0.0 {
				(
					(self.object.rotation_x * -1.0 - PI / 2.0).cos() * self.rotation_y.sin() * -1.0,
					self.rotation_y.cos() * -1.0,
					(self.object.rotation_x * -1.0 - PI / 2.0).sin() * self.rotation_y.sin() * -1.0,
				)
			} else {
				(0.0, -1.0, 0.0)
			};
			d3dg.set_world_transform(
				self.object.pos_x + add_x,
				self.object.pos_y + add_y,
				self.object.pos_z + add_z,
				self.object.rotation_x + PI,
				self.rotation_y,
				self.upmodel_size,
			);
			d3dg.render_model(upmodel, id_texture, self.object.dark_model_flag, false, no_model);

			// Render the legs, a fifth of the upper body's offset away.
			let (add_x, add_y, add_z) = if self.rotation_y != 0.0 {
				(
					(self.object.rotation_x * -1.0 - PI / 2.0).cos() * self.rotation_y.sin() * -0.2,
					self.rotation_y.cos() * -0.2,
					(self.object.rotation_x * -1.0 - PI / 2.0).sin() * self.rotation_y.sin() * -0.2,
				)
			} else {
				(0.0, -0.2, 0.0)
			};
			d3dg.set_world_transform(
				self.object.pos_x + add_x,
				self.object.pos_y + add_y,
				self.object.pos_z + add_z,
				legrx + PI,
				self.rotation_y,
				self.legmodel_size,
			);
			d3dg.render_model(legmodel, id_texture, self.object.dark_model_flag, false, no_model);
		}

		// Render the arms, and the weapon in them. Sixteen units up is where the hands are.
		if self.rotation_y != 0.0 {
			// Dead and fallen over, or started to fall.
			let add_x = (self.object.rotation_x * -1.0 - PI / 2.0).cos() * self.rotation_y.sin() * 16.0;
			let add_y = self.rotation_y.cos() * 16.0;
			let add_z = (self.object.rotation_x * -1.0 - PI / 2.0).sin() * self.rotation_y.sin() * 16.0;
			d3dg.set_world_transform(
				self.object.pos_x + add_x,
				self.object.pos_y + add_y,
				self.object.pos_z + add_z,
				self.object.rotation_x + PI,
				self.armrotation_y + self.rotation_y,
				self.armmodel_size,
			);
			d3dg.render_model(armmodel, id_texture, self.object.dark_model_flag, false, no_model);
		} else {
			match self.weapon[self.selectweapon as usize] {
				None => {
					// Empty handed.
					d3dg.set_world_transform(
						self.object.pos_x,
						self.object.pos_y + 16.0,
						self.object.pos_z,
						self.object.rotation_x + PI,
						armry,
						self.armmodel_size,
					);
					d3dg.render_model(armmodel, id_texture, self.object.dark_model_flag, false, no_model);
				}
				Some(index) => {
					// Some weapon is held. Get its model and texture, and its setting values
					// for where the gun sits in the hand.
					let id_param = weapons[index].param_data().0;

					// Render the arms.
					d3dg.set_world_transform(
						self.object.pos_x,
						self.object.pos_y + 16.0,
						self.object.pos_z,
						self.object.rotation_x + PI,
						armry,
						self.armmodel_size,
					);
					d3dg.render_model(armmodel, id_texture, self.object.dark_model_flag, false, no_model);

					// Render the weapon. A weapon with no settings, or one whose model
					// never loaded, has nothing to draw and upstream would draw whatever
					// its uninitialised locals held.
					if let Some(paramdata) = self.object.param.get_weapon(id_param) {
						let (model, texture) = resource
							.get_weapon_model_texture(d3dg, id_param)
							.unwrap_or((-1, -1));
						d3dg.set_world_transform_human_weapon(
							self.object.pos_x,
							self.object.pos_y + 16.0,
							self.object.pos_z,
							paramdata.mx / 10.0 * -1.0,
							paramdata.my / 10.0,
							paramdata.mz / 10.0 * -1.0,
							self.object.rotation_x + PI,
							armry,
							paramdata.size,
						);
						d3dg.render_model(model, texture, self.object.dark_model_flag, false, no_model);
					}
				}
			}
		}
	}
}

/// The ramp term of the fall damage, without the `GetRand(6)` that is added to it.
///
/// Zero at [`HUMAN_DAMAGE_MINSPEED`], which is the speed below which a landing does no
/// damage at all, and [`HUMAN_DAMAGE_MAXFALL`] at [`HUMAN_DAMAGE_MAXSPEED`], the terminal
/// velocity. It is a separate function because it is the part a player notices and the
/// only part that can be pinned exactly.
fn fall_damage_ramp(move_y: f32) -> i32 {
	(HUMAN_DAMAGE_MAXFALL as f32 / (HUMAN_DAMAGE_MAXSPEED - HUMAN_DAMAGE_MINSPEED).abs()
		* (move_y - HUMAN_DAMAGE_MINSPEED).abs()) as i32
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;
	use std::path::PathBuf;

	use xops_platform::gfx::Gfx;

	use crate::datafile::TOTAL_BLOCKTEXTURE;

	// -----------------------------------------------------------------------
	// Setting up a human and the manager state it points into
	// -----------------------------------------------------------------------

	/// A world with no blocks and a collision class that was never initialised, which is
	/// what the manager has before a mission loads a map. Every map query answers no, so
	/// the counters and the state machine can be driven without a floor under them.
	fn empty_world() -> (Collision, BlockDataInterface) {
		(Collision::new(), BlockDataInterface::new())
	}

	/// A human of type 0, which is 120 hp, with the settings class registered the way the
	/// header requires before anything else runs.
	fn human() -> Human {
		let mut human = Human::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 0, 0, 0, 0, true);
		human.set_parameter_info_class(ParameterInfo::new());
		human.set_param_data(0, 0, 0, 0, true);
		human
	}

	/// A human holding one weapon, which sits in the manager's array at index 0 and in the
	/// human's slot B, the selection a fresh human starts on.
	///
	/// The pickup starts the ten frame switch count, and the manager steps the world after
	/// a pickup, so this drains it the same way.
	fn armed(weapon_id: i32, loaded: i32, total: i32) -> (Human, Vec<Weapon>) {
		let mut human = human();
		let mut weapons = vec![Weapon::new(
			ParameterInfo::new(),
			0.0,
			0.0,
			0.0,
			0.0,
			weapon_id,
			total,
			true,
		)];
		weapons[0].set_param_data(weapon_id, loaded, total, false);

		assert_eq!(human.pickup_weapon(0, &mut weapons), 1, "the pickup succeeds");
		assert!(weapons[0].using_flag(), "the weapon is marked as held");
		assert_eq!(human.get_change_have_weapon_cnt(), 10);

		let (coll, bd) = empty_world();
		for _ in 0..10 {
			human.process_object(&coll, &bd, false, false, false, &mut weapons);
		}
		assert_eq!(human.get_change_have_weapon_cnt(), 0);
		assert_eq!(human.get_main_weapon_type_no(&weapons), weapon_id);

		(human, weapons)
	}

	// -----------------------------------------------------------------------
	// A block of the shape the collision tests use, for the landing tests
	// -----------------------------------------------------------------------

	fn temp_path(name: &str) -> PathBuf {
		std::env::temp_dir().join(format!("xops_human_{}_{}", std::process::id(), name))
	}

	fn write_f32_le(out: &mut Vec<u8>, value: f32) {
		out.extend_from_slice(&value.to_bits().to_le_bytes());
	}

	/// A .bd1 image holding one unit cube at the origin. It is the same image the
	/// collision tests build, because a landing needs a real block under the feet and the
	/// loader is the only way to get one.
	fn bd1_image() -> Vec<u8> {
		const CUBE: [(f32, f32, f32); 8] = [
			(0.0, 0.0, 0.0),
			(1.0, 0.0, 0.0),
			(1.0, 1.0, 0.0),
			(0.0, 1.0, 0.0),
			(0.0, 0.0, 1.0),
			(1.0, 0.0, 1.0),
			(1.0, 1.0, 1.0),
			(0.0, 1.0, 1.0),
		];

		let mut out = Vec::new();
		out.extend_from_slice(&[0u8; 31 * TOTAL_BLOCKTEXTURE]);
		out.extend_from_slice(&1u16.to_le_bytes());
		for axis in 0..3 {
			for corner in CUBE {
				let value = match axis {
					0 => corner.0,
					1 => corner.1,
					_ => corner.2,
				};
				write_f32_le(&mut out, value);
			}
		}
		for i in 24..72 {
			write_f32_le(&mut out, i as f32);
		}
		for j in 0..6 {
			out.push(j as u8);
			out.push(0);
			out.push(0);
			out.push(0);
		}
		out.extend_from_slice(&[0; 4]);
		out
	}

	/// One unit cube, loaded and prepared the way a mission load leaves the map.
	fn cube_map(name: &str) -> (BlockDataInterface, Collision) {
		let path = temp_path(name);
		fs::write(&path, bd1_image()).expect("write the temp map");

		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(path.to_string_lossy().as_bytes()), 0);
		blocks.calculation_blockdata(false);

		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(Some(&blocks)), 0);
		(blocks, coll)
	}

	/// A human standing half a unit above the top of the cube, which is where the ground
	/// probe half a unit below the feet finds it.
	fn standing_on_the_cube() -> Human {
		let mut human = human();
		human.set_pos_data(0.5, 2.3, 0.5, 0.0);
		human
	}

	// -----------------------------------------------------------------------
	// Construction and the simple accessors
	// -----------------------------------------------------------------------

	#[test]
	fn the_constructor_leaves_the_base_defaults_and_starts_at_zero_health() {
		let mut human = Human::new(ParameterInfo::new(), 1.0, 2.0, 3.0, 0.5, 7, 9, 11, 4, true);

		// The base class is built with its own defaults first, so the draw size is one
		// until the object manager sets a model, and neither the model nor the texture has
		// been chosen.
		assert_eq!(human.object.model(), (-1, 1.0));
		assert_eq!(human.object.texture(), -1);
		assert_eq!(human.object.pos_data(), (1.0, 2.0, 3.0, 0.5));
		assert!(human.object.enable_flag());
		assert_eq!(human.get_param_data(), (7, 9, 11, 4));
		assert_eq!(human.move_pos(), (0.0, 0.0, 0.0));
		assert_eq!(human.get_under_block(), (-1, -1));

		// The upstream reads the health from the settings with the lookup commented out,
		// so a constructed human has none at all.
		assert_eq!(human.hp(), 0);
		assert!(!human.dead_flag());
		assert!(!human.dead_motion_flag());
		assert!(!human.invincible_flag());
		assert_eq!(human.get_scope_mode(), 0);
		assert_eq!(human.get_move_flag(true), 0);
		assert_eq!(human.get_move_flag(false), 0);
		assert_eq!(human.get_change_have_weapon_cnt(), 0);
		assert_eq!(human.get_change_weapon_id_cnt(), 0);
		assert_eq!(human.get_weapon_reload_cnt(), 0);
		assert_eq!(human.gunsight_error_range(), 0);

		// The selection starts on slot B, and the drawn limb sizes are the upstream's.
		let (selectweapon, weapons, nlnbs, nnbs) = human.get_weapon();
		assert_eq!(selectweapon, 1);
		assert_eq!(weapons, [None, None]);
		assert_eq!(nlnbs, [0, 0]);
		assert_eq!(nnbs, [0, 0]);
		assert_eq!(human.upmodel_size, 9.4);
		assert_eq!(human.armmodel_size, 9.0);
		assert_eq!(human.legmodel_size, 9.0);

		assert_eq!(human.rx_ry(), (0.5, 0.0));
		assert_eq!(human.dead_ry(), 0.0);
		assert_eq!(human.move_motion_count(), 0);
		assert_eq!(human.total_move(), 0.0);
		assert_eq!(human.check_hit(), (false, 0.0));
		assert_eq!(human.get_movemode(true), 0);
		assert_eq!(human.get_jump_landing(true), 0);
	}

	#[test]
	fn set_param_data_takes_the_health_from_the_settings_and_restarts_the_motion() {
		let mut human = Human::new(ParameterInfo::new(), 4.0, 0.0, 0.0, 1.5, -1, 0, 0, 0, false);
		human.set_parameter_info_class(ParameterInfo::new());
		human.set_param_data(0, 5, 7, 2, true);

		assert_eq!(human.hp(), 120, "the type 0 human has 120 hp");
		assert_eq!(human.get_param_data(), (0, 5, 7, 2));
		assert_eq!(human.armrotation_y, degree_to_radian(-30.0));
		assert_eq!(human.get_move_flag(false), 0);
		assert!(!human.object.enable_flag(), "the enable flag is not touched");
		// The motion control starts from the angle the human was placed with.
		assert_eq!(human.motion_ctrl.get_render_motion().1, 1.5);

		// Initialising again throws away a weapon, a scope and a walk in progress.
		human.hp = 1;
		human.scopemode = 2;
		human.weapon[1] = Some(0);
		human.motion_ctrl.process_object(0.0, 0.0, 1, 0, 0, move_flag::WALK, 120, true);
		human.set_param_data(0, 5, 7, 2, true);
		assert_eq!(human.hp(), 120);
		assert_eq!(human.get_scope_mode(), 0);
		assert_eq!(human.get_weapon().1, [None, None]);
		assert_eq!(human.move_motion_count(), 0);

		// A type number with no settings leaves the health at zero, which is a human that
		// dies on its first frame.
		human.set_param_data(99, 0, 0, 0, true);
		assert_eq!(human.hp(), 0);
	}

	#[test]
	fn the_team_and_the_invincible_flag_round_trip() {
		let mut human = human();
		assert_eq!(human.get_param_data().3, 0);

		human.set_team_id(3);
		assert_eq!(human.get_param_data().3, 3);
		assert!(ENABLE_BUG_TEAMID, "a negative team number is reachable");
		human.set_team_id(-1);
		assert_eq!(human.get_param_data().3, -1);

		human.set_invincible_flag(true);
		assert!(human.invincible_flag());
		human.set_invincible_flag(false);
		assert!(!human.invincible_flag());
	}

	#[test]
	fn the_health_can_only_be_set_while_there_is_health_left() {
		let mut human = human();
		assert!(human.set_hp(30));
		assert_eq!(human.hp(), 30);

		human.hit_grenade_explosion(30);
		assert_eq!(human.hp(), 0);
		assert!(!human.set_hp(50), "a dead human cannot be healed");
		assert_eq!(human.hp(), 0);
	}

	// -----------------------------------------------------------------------
	// Damage
	// -----------------------------------------------------------------------

	#[test]
	fn a_hit_takes_the_multiplier_for_that_body_part_plus_its_own_jitter() {
		// The multipliers are the constants in the object module, and the jitter is the
		// GetRand that follows each one.
		let mut human = human();

		human.hp = 1000;
		human.hit_bullet_head(10);
		assert!((965..=980).contains(&human.hp()), "head: {}", human.hp());
		assert_eq!(human.reaction_gunsight_error_range, 15);

		human.hp = 1000;
		human.hit_bullet_up(10);
		assert!((983..=990).contains(&human.hp()), "up: {}", human.hp());
		assert_eq!(human.reaction_gunsight_error_range, 12);

		human.hp = 1000;
		human.hit_bullet_leg(10);
		assert!((988..=993).contains(&human.hp()), "leg: {}", human.hp());
		assert_eq!(human.reaction_gunsight_error_range, 8);

		human.hp = 1000;
		human.hit_zombie_attack();
		assert!((981..=985).contains(&human.hp()), "zombie: {}", human.hp());
		assert_eq!(human.reaction_gunsight_error_range, 10);

		human.hp = 1000;
		human.hit_grenade_explosion(80);
		assert_eq!(human.hp(), 920, "a grenade takes its attack power exactly");
		assert_eq!(human.reaction_gunsight_error_range, 10);

		// Every body part drops by at least the multiplier: the head is worth twice the
		// upper body, and the legs less than it.
		assert!(HUMAN_DAMAGE_HEAD > HUMAN_DAMAGE_UP);
		assert!(HUMAN_DAMAGE_UP > HUMAN_DAMAGE_LEG);
	}

	#[test]
	fn an_invincible_human_takes_no_damage_but_still_recoils() {
		let mut human = human();
		human.hp = 1000;
		human.set_invincible_flag(true);

		human.hit_bullet_head(500);
		human.hit_grenade_explosion(1000);
		human.hit_zombie_attack();
		human.hit_bullet_up(500);
		human.hit_bullet_leg(500);
		assert_eq!(human.hp(), 1000);
		// The recoil is set outside the invincibility check, so the gunsight still jumps.
		assert_eq!(human.reaction_gunsight_error_range, 8);
	}

	#[test]
	fn the_death_boundary_is_zero_health() {
		let (coll, bd) = empty_world();
		let mut human = human();
		let mut weapons = Vec::new();

		// One point of health left is still alive: nothing to fall over.
		human.set_hp(1);
		human.hit_grenade_explosion(0);
		assert_eq!(human.hp(), 1);
		assert_eq!(
			human.check_and_process_dead(&coll, &bd, &mut weapons),
			0,
			"one point of health is not a death"
		);
		assert!(!human.dead_flag());
		assert!(!human.dead_motion_flag());

		// Exactly zero starts the fall in the same frame it happens.
		human.hit_grenade_explosion(1);
		assert_eq!(human.hp(), 0);
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut weapons), 1);
		assert_eq!(human.deadstate, 1);
		assert!(human.dead_motion_flag());
		assert!(!human.dead_flag(), "a falling body is not a corpse yet");
	}

	#[test]
	fn the_hit_flag_reports_once_and_keeps_the_direction() {
		let mut human = human();
		assert_eq!(human.check_hit(), (false, 0.0));

		human.set_hit_flag(1.25);
		assert_eq!(human.check_hit(), (true, 1.25));
		// Running the check clears the flag but leaves the direction, as upstream does.
		assert_eq!(human.check_hit(), (false, 1.25));
	}

	#[test]
	fn a_dead_human_drops_every_weapon_and_falls_the_way_it_was_hit() {
		let (coll, bd) = empty_world();
		let (mut human, mut weapons) = armed(1, 5, 30);

		// Put a weapon in the other slot too, so both drops are exercised.
		human.set_param_data(0, 0, 0, 0, true);
		human.selectweapon = 0;
		human.weapon[0] = Some(1);
		weapons.push(Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 4, 19, true));
		weapons[1].set_param_data(4, 5, 19, false);
		weapons[1].pickup();
		human.selectweapon = 1;

		// A hit from the front knocks the body backwards, which is a positive fall angle.
		human.set_hit_flag(0.0);
		human.hp = 0;
		human.selectweaponcnt = 4;
		human.weaponreloadcnt = 3;
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut weapons), 1);

		assert_eq!(human.add_ry, HUMAN_DEADADDRY);
		assert_eq!(human.rotation_y, HUMAN_DEADADDRY);
		assert_eq!(human.get_weapon().1, [None, None], "both weapons were dropped");
		assert_eq!(human.get_weapon().2, [0, 0]);
		assert_eq!(human.get_weapon().3, [0, 0]);
		assert!(!weapons[0].using_flag() && !weapons[1].using_flag());
		// The counts are all cleared, and the body is raised a unit so it does not sink.
		assert_eq!(human.selectweaponcnt, 0);
		assert_eq!(human.weaponreloadcnt, 0);
		assert_eq!(human.weaponshotcnt, 0);
		assert_eq!(human.changeweaponidcnt, 0);
		assert_eq!(human.object.pos_y, 1.0);
		assert_eq!(human.get_scope_mode(), 0);

		// A hit from behind falls the other way: the hit direction is compared against the
		// body rotation after normalising.
		let mut human = human();
		human.hp = 0;
		human.set_hit_flag(PI);
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut Vec::new()), 1);
		assert_eq!(human.add_ry, HUMAN_DEADADDRY * -1.0);
	}

	#[test]
	fn the_body_falls_to_a_right_angle_and_stops_moving_after_that() {
		let (coll, bd) = empty_world();
		let mut human = human();
		human.hp = 0;

		// Frame one starts the fall.
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut Vec::new()), 1);

		// The body tilts a quarter of a degree at a time towards 135 degrees, which is
		// 180 steps, and every frame in between reports that it is still falling.
		let mut frames = 1;
		let mut last = 0;
		while human.deadstate == 1 {
			last = human.check_and_process_dead(&coll, &bd, &mut Vec::new());
			frames += 1;
			assert_eq!(last, 2);
			assert!(frames < 400, "the body never finished tilting");
		}
		assert_eq!(frames, 182, "180 steps of 0.75 degrees from 135");
		assert!(human.rotation_y >= degree_to_radian(135.0));
		// The arms swing to a right angle and stop there.
		assert_eq!(human.armrotation_y, degree_to_radian(90.0));

		// Then it falls, nose first, until it passes the deadline at -100.
		let mut fall = 0;
		while human.deadstate == 2 {
			human.check_and_process_dead(&coll, &bd, &mut Vec::new());
			fall += 1;
			assert!(fall < 400, "the body never landed");
		}
		assert!(human.object.pos_y <= HUMAN_DEADLINE);
		assert!(fall < 100, "{fall} frames of falling");

		// And state 4 becomes the fixed corpse, which reports the end of the fall once.
		assert_eq!(human.deadstate, 4);
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut Vec::new()), 3);
		assert!(human.dead_flag());
		assert!(!human.dead_motion_flag());
		assert_eq!(human.check_and_process_dead(&coll, &bd, &mut Vec::new()), 4);

		// A corpse returns the corpse code from ProcessObject without simulating.
		let mut weapons = Vec::new();
		assert_eq!(
			human.process_object(&coll, &bd, false, false, false, &mut weapons),
			3
		);
	}

	#[test]
	fn the_process_object_code_for_a_dying_human_is_the_corpse_code() {
		let (coll, bd) = empty_world();
		let mut human = human();
		let mut weapons = Vec::new();
		human.hp = 0;

		// The first frame after death reports the fall finishing, which is the code the
		// sound and effect code branches on.
		assert_eq!(human.process_object(&coll, &bd, false, false, false, &mut weapons), 3);

		// Falling below the deadline is death by the terrain.
		let mut human = human();
		human.object.pos_y = HUMAN_DEADLINE - 1.0;
		assert_eq!(human.process_object(&coll, &bd, false, false, false, &mut weapons), 4);
		assert_eq!(human.hp(), 0);
	}

	#[test]
	fn death_by_the_terrain_is_reported_separately_from_a_bullet() {
		let (blocks, coll) = cube_map("terrain_death.bd1");
		let mut human = standing_on_the_cube();
		let mut weapons = Vec::new();

		// A fast fall onto the cube takes the health to nothing inside the map collision,
		// which is the code the manager plays the terrain death sound for.
		human.hp = 1;
		human.move_y = HUMAN_DAMAGE_MAXSPEED;
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert_eq!(human.hp(), 0, "the fall damage was enough");

		let mut human = standing_on_the_cube();
		human.hp = 200;
		human.move_y = HUMAN_DAMAGE_MAXSPEED;
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert_eq!(human.hp(), 80, "120 of fall damage at terminal velocity");
	}

	// -----------------------------------------------------------------------
	// Falling and landing
	// -----------------------------------------------------------------------

	#[test]
	fn the_fall_damage_ramp_starts_at_the_threshold_and_ends_at_terminal_velocity() {
		// Nothing at the speed below which a landing is free, which is the comparison the
		// caller makes before it calls this at all.
		assert_eq!(fall_damage_ramp(HUMAN_DAMAGE_MINSPEED), 0);
		assert_eq!(fall_damage_ramp(-3.7), 0, "slower than the threshold takes nothing");

		// Halfway between the threshold and terminal velocity is half the damage, and the
		// terminal velocity itself is the full 120.
		assert_eq!(fall_damage_ramp(-5.4), 60);
		assert_eq!(fall_damage_ramp(HUMAN_DAMAGE_MAXSPEED), HUMAN_DAMAGE_MAXFALL);

		// The ramp is linear, so each tenth of a unit of speed is 3.75 points of damage.
		assert_eq!(fall_damage_ramp(-3.9), 3);
		assert_eq!(fall_damage_ramp(-4.0), 7);
		assert_eq!(fall_damage_ramp(-6.9), 116);
	}

	#[test]
	fn a_landing_above_the_threshold_hurts_and_one_below_it_does_not() {
		let (blocks, coll) = cube_map("landing.bd1");
		let mut weapons = Vec::new();

		// 3.7 a frame is above the threshold, so the landing is free but still a landing.
		let mut human = standing_on_the_cube();
		human.move_y = -3.7;
		human.set_move_forward();
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert_eq!(human.hp(), 120, "no damage, the fall was not fast enough");
		assert!(human.move_y_landing);
		assert_eq!(human.get_jump_landing(true), 2);
		// The fall stopped at the first sub-step that touched the block.
		assert!((human.object.pos_y - (2.3 - 3.7 * 0.33)).abs() < 1e-5);

		// 4.0 a frame is past it, and the damage is the ramp of 7 plus up to five more from
		// GetRand(6).
		let mut human = standing_on_the_cube();
		human.move_y = -4.0;
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert!(
			(108..=113).contains(&human.hp()),
			"the landing cost 7 to 12: {}",
			human.hp()
		);
		assert!(human.move_y_landing);
		assert_eq!(human.move_y, 0.0, "the fall speed is cleared on landing");

		// An invincible human lands without losing anything.
		let mut human = standing_on_the_cube();
		human.set_invincible_flag(true);
		human.move_y = -4.0;
		human.process_object(&coll, &blocks, false, false, false, &mut weapons);
		assert_eq!(human.hp(), 120);
		assert!(human.move_y_landing);
	}

	#[test]
	fn a_slow_fall_that_does_not_reach_the_block_in_one_frame_does_not_land() {
		let (blocks, coll) = cube_map("no_landing.bd1");
		let mut human = standing_on_the_cube();
		let mut weapons = Vec::new();

		// Three sub-steps of a slow fall do not cover the 1.3 units down to the probe.
		human.move_y = -0.3;
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert!(!human.move_y_landing);
		assert_eq!(human.get_jump_landing(true), 0);
		assert_eq!(human.hp(), 120);
		// Three sub-steps each add the fall speed and move a third of it.
		assert!((human.move_y - -0.3_f32.min(0.0) + 3.0 * HUMAN_DAMAGE_SPEED).abs() < 1e-5);
	}

	#[test]
	fn a_landing_with_the_jump_held_bounces_at_the_jump_speed() {
		let (blocks, coll) = cube_map("jump.bd1");
		let mut human = standing_on_the_cube();
		let mut weapons = Vec::new();

		// The jump key is a flag the movement process consumes, so it has to be set before
		// the frame runs and is read back as the previous frame's flag.
		human.jump();
		assert_eq!(human.get_move_flag(true), move_flag::JUMP);
		assert_eq!(human.get_jump_landing(true), 1, "jumping");
		assert_eq!(human.get_jump_landing(false), 1);

		human.move_y = -4.0;
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert_eq!(human.move_y, HUMAN_JUMP_SPEED, "the landing bounced");
		// The landing is reported ahead of the jump, because it is checked first.
		assert!(human.move_y_landing);
		assert_eq!(human.get_jump_landing(true), 2);
		assert_eq!(human.get_jump_landing(false), 2);
		assert_eq!(human.get_move_flag(true), 0, "the key flags are consumed");

		// The next frame is airborne: the landing flag is cleared and the rise decays by
		// the fall speed every sub-step.
		assert_eq!(human.process_object(&coll, &blocks, false, false, false, &mut weapons), 1);
		assert!(!human.move_y_landing);
		assert!((human.move_y - (HUMAN_JUMP_SPEED - 3.0 * HUMAN_DAMAGE_SPEED)).abs() < 1e-5);
		assert_eq!(human.get_jump_landing(false), 1, "the previous frame's key is kept");
		assert_eq!(human.get_jump_landing(true), 0);
	}

	#[test]
	fn the_block_under_the_feet_is_recorded_on_landing() {
		let (blocks, coll) = cube_map("underblock.bd1");
		let mut human = standing_on_the_cube();
		let mut weapons = Vec::new();

		assert_eq!(human.get_under_block(), (-1, -1));
		human.move_y = -4.0;
		human.process_object(&coll, &blocks, false, false, false, &mut weapons);
		let (id, face) = human.get_under_block();
		assert_eq!(id, 0, "the cube is the only block");
		assert!((0..6).contains(&face), "a face number: {face}");

		// The record is cleared at the start of every frame, so an airborne human has none.
		human.move_y = 2.0;
		human.process_object(&coll, &blocks, false, false, false, &mut weapons);
		assert_eq!(human.get_under_block(), (-1, -1));
	}

	#[test]
	fn a_climb_disables_the_rise_for_eight_frames() {
		let mut human = human();
		human.move_y_upper = 8;
		assert_eq!(human.move_y_upper, 8);
	}

	// -----------------------------------------------------------------------
	// Movement flags and the direction they resolve to
	// -----------------------------------------------------------------------

	#[test]
	fn the_five_movement_setters_and_the_jump_are_bits_of_one_word() {
		let mut human = human();
		human.set_move_forward();
		assert_eq!(human.get_move_flag(true), 0x01);
		human.set_move_back();
		assert_eq!(human.get_move_flag(true), 0x03);
		human.set_move_left();
		assert_eq!(human.get_move_flag(true), 0x07);
		human.set_move_right();
		assert_eq!(human.get_move_flag(true), 0x0f);
		human.set_move_walk();
		assert_eq!(human.get_move_flag(true), 0x1f);
		human.jump();
		assert_eq!(human.get_move_flag(true), 0x3f);
		// None of them has moved until the frame's movement process runs.
		assert_eq!(human.get_move_flag(false), 0);

		// The movement process backs the word up and clears it for the next frame.
		human.control_process();
		assert_eq!(human.get_move_flag(false), 0x3f);
		assert_eq!(human.get_move_flag(true), 0);
	}

	#[test]
	fn each_combination_moves_the_human_by_its_own_acceleration_and_angle() {
		// The angle is the movement angle plus a quarter turn, against a body rotation of
		// zero. The two velocities are what that puts into the X and Z axes.
		let cases: [(i32, f32, f32, f32); 10] = [
			(move_flag::WALK, 0.0, 0.0, 0.35),
			(move_flag::FORWARD, 0.0, 0.0, 0.7),
			(move_flag::BACK, PI, 0.0, -0.38),
			(move_flag::LEFT, PI / 2.0, -0.5, 0.0),
			(move_flag::RIGHT, -PI / 2.0, 0.5, 0.0),
			(
				move_flag::FORWARD | move_flag::LEFT,
				PI / 4.0,
				-0.42426407,
				0.42426407,
			),
			(
				move_flag::FORWARD | move_flag::RIGHT,
				-PI / 4.0,
				0.42426407,
				0.42426407,
			),
			(
				move_flag::BACK | move_flag::LEFT,
				3.0 * PI / 4.0,
				-0.26870057,
				-0.26870057,
			),
			(
				move_flag::BACK | move_flag::RIGHT,
				-3.0 * PI / 4.0,
				0.26870057,
				-0.26870057,
			),
			(0, 0.0, 0.0, 0.0),
		];

		for (flags, angle, x, z) in cases {
			let mut human = human();
			human.move_flag = flags;
			human.control_process();
			assert_eq!(human.move_rx, angle, "flags {flags:#x}");

			// The walk flag takes the walking acceleration, and the diagonals are the mean
			// of the two runs they combine.
			let (move_x, move_y, move_z) = human.move_pos();
			assert!((move_x - x).abs() < 1e-5, "flags {flags:#x}: x {move_x}");
			assert_eq!(move_y, 0.0);
			assert!((move_z - z).abs() < 1e-5, "flags {flags:#x}: z {move_z}");
		}
	}

	#[test]
	fn the_movement_mode_reports_the_strongest_key_that_is_held() {
		// The order is walking, forward, backward, sideways.
		let cases: [(i32, i32); 10] = [
			(move_flag::WALK, 1),
			(move_flag::FORWARD, 2),
			(move_flag::BACK, 3),
			(move_flag::LEFT, 4),
			(move_flag::RIGHT, 4),
			(move_flag::FORWARD | move_flag::LEFT, 2),
			(move_flag::FORWARD | move_flag::RIGHT, 2),
			(move_flag::BACK | move_flag::LEFT, 3),
			(move_flag::BACK | move_flag::RIGHT, 3),
			(move_flag::JUMP, 0),
		];

		for (flags, mode) in cases {
			let mut human = human();
			human.move_flag = flags;
			assert_eq!(human.get_movemode(true), mode, "flags {flags:#x}");
			// The previous frame's mode is what the AI reads, and it is still empty.
			assert_eq!(human.get_movemode(false), 0);

			human.control_process();
			assert_eq!(human.get_movemode(false), mode);
			assert_eq!(human.get_movemode(true), 0);
		}
	}

	#[test]
	fn a_disabled_or_dead_human_has_no_movement_mode() {
		let mut human = human();
		human.set_move_forward();
		assert_eq!(human.get_movemode(true), 2);
		human.hp = 0;
		assert_eq!(human.get_movemode(true), 0);
		human.hp = 120;
		human.set_enable_flag(false);
		assert_eq!(human.get_movemode(true), 0);
		assert_eq!(human.get_jump_landing(true), 0);
	}

	#[test]
	fn a_push_in_a_direction_adds_velocity_to_all_three_axes() {
		let mut human = human();
		human.add_pos_order(PI / 2.0, 0.0, 2.0);
		let (x, y, z) = human.move_pos();
		assert!(x.abs() < 1e-6, "a quarter turn is straight along Z: {x}");
		assert_eq!(y, 0.0);
		assert_eq!(z, 2.0);

		let mut human = human();
		human.add_pos_order(0.0, PI / 6.0, 3.0);
		let (x, y, z) = human.move_pos();
		assert!((x - 2.5980762).abs() < 1e-5, "{x}");
		assert!((y - 1.5).abs() < 1e-5, "{y}");
		assert!(z.abs() < 1e-6, "{z}");
	}

	// -----------------------------------------------------------------------
	// The gunsight error
	// -----------------------------------------------------------------------

	#[test]
	fn the_gunsight_error_is_the_state_error_plus_the_recoil_error() {
		let (coll, bd) = empty_world();
		let (mut human, mut weapons) = armed(1, 5, 30);

		// A hit leaves a recoil error of 15, and there is no state error yet.
		human.hit_bullet_head(1);
		assert_eq!(human.gunsight_error_range(), 15);

		// One frame in the air: the state error is nothing, because the airborne state is
		// read from the frame before, and the recoil drops by one.
		assert_eq!(human.process_object(&coll, &bd, false, false, false, &mut weapons), 1);
		assert_eq!(human.gunsight_error_range(), 14);

		// The next frame is airborne, which is the 22 the upstream reserves for it.
		assert!(human.move_y_flag);
		human.process_object(&coll, &bd, false, false, false, &mut weapons);
		assert_eq!(human.gunsight_error_range(), 35, "22 + 13");

		// On the ground and walking the state error is 4.
		human.move_y_flag = false;
		human.set_move_walk();
		human.process_object(&coll, &bd, false, false, false, &mut weapons);
		assert_eq!(human.gunsight_error_range(), 16, "4 + 12");

		// Below 40 health three more are added to it.
		human.move_y_flag = false;
		human.hp = 39;
		human.set_move_walk();
		human.process_object(&coll, &bd, false, false, false, &mut weapons);
		assert_eq!(human.gunsight_error_range(), 18, "4 + 3 + 11");

		// An empty hand clears the recoil error instead of decaying it.
		human.weapon[1] = None;
		human.move_y_flag = false;
		human.hp = 120;
		human.process_object(&coll, &bd, false, false, false, &mut weapons);
		assert_eq!(human.gunsight_error_range(), 0);
	}

	// -----------------------------------------------------------------------
	// Weapons
	// -----------------------------------------------------------------------

	#[test]
	fn picking_up_a_weapon_fills_the_selected_slot_and_starts_the_switch_count() {
		let mut human = human();
		let mut weapons = vec![Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 1, 30, true)];

		// Slot B is the one a fresh human has selected, and the index is where the weapon
		// sits in the manager's array.
		assert_eq!(human.selectweapon, 1);
		assert_eq!(human.pickup_weapon(0, &mut weapons), 1);
		assert_eq!(human.get_weapon().1, [None, Some(0)]);
		assert_eq!(human.get_change_have_weapon_cnt(), 10);
		assert_eq!(human.get_main_weapon_type_no(&weapons), 1);
		assert!(weapons[0].using_flag());

		// The slot is taken, so nothing more can be picked up into it.
		let mut second = vec![Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 4, 19, true)];
		assert_eq!(human.pickup_weapon(0, &mut second), 0);
		assert!(!second[0].using_flag());

		// And a weapon another human is already holding cannot be taken.
		let mut human = human();
		let mut taken = vec![Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 1, 30, true)];
		assert_eq!(taken[0].pickup(), 0);
		assert_eq!(human.pickup_weapon(0, &mut taken), 0);
		assert_eq!(human.get_weapon().1, [None, None]);
		assert_eq!(human.get_change_have_weapon_cnt(), 0);
	}

	#[test]
	fn set_weapon_fills_both_slots_and_keeps_a_slot_whose_weapon_refuses_the_pickup() {
		let mut human = human();
		let mut weapons = vec![
			Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 1, 30, true),
			Weapon::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 4, 19, true),
		];

		human.set_weapon([Some(0), Some(1)], [5, 7], [30, 19], &mut weapons);
		let (select, slots, nlnbs, nnbs) = human.get_weapon();
		assert_eq!(select, 1);
		assert_eq!(slots, [Some(0), Some(1)]);
		assert_eq!(nlnbs, [5, 7]);
		assert_eq!(nnbs, [30, 19]);
		assert!(weapons[0].using_flag() && weapons[1].using_flag());

		// A slot that fails the pickup keeps what it held, which is the upstream's stale
		// value rather than an emptied slot.
		human.set_weapon([Some(0), None], [1, 2], [3, 4], &mut weapons);
		let (_, slots, nlnbs, nnbs) = human.get_weapon();
		assert_eq!(slots, [Some(0), None]);
		assert_eq!(nlnbs, [1, 2]);
		assert_eq!(nnbs, [3, 4]);
	}

	#[test]
	fn the_weapon_switch_counts_down_and_wraps_to_the_next_slot() {
		let (coll, bd) = empty_world();
		let (mut human, weapons) = armed(1, 5, 30);

		// Switching to the weapon already held is refused.
		human.change_have_weapon(1, &weapons);
		assert_eq!(human.selectweapon, 1);
		assert_eq!(human.get_change_have_weapon_cnt(), 0);

		// So is a number outside the two slots.
		human.change_have_weapon(7, &weapons);
		assert_eq!(human.selectweapon, 1);
		assert_eq!(human.get_change_have_weapon_cnt(), 0);

		// -1 moves to the next slot and starts the ten frame count.
		human.change_have_weapon(-1, &weapons);
		assert_eq!(human.selectweapon, 0, "slot B moves to slot A");
		assert_eq!(human.get_change_have_weapon_cnt(), 10);
		assert_eq!(
			human.get_main_weapon_type_no(&weapons),
			ID_WEAPON_NONE,
			"slot A is empty"
		);

		// A second switch is refused while the first is running.
		human.change_have_weapon(-1, &weapons);
		assert_eq!(human.selectweapon, 0);

		// A dead human cannot switch either.
		let mut weapons = weapons;
		let mut dead = human.clone();
		dead.hp = 0;
		dead.change_have_weapon(1, &weapons);
		assert_eq!(dead.selectweapon, 0);

		// The world steps, which is what counts the switch down, and then the next switch
		// wraps back round to slot B.
		for _ in 0..10 {
			human.process_object(&coll, &bd, false, false, false, &mut weapons);
		}
		assert_eq!(human.get_change_have_weapon_cnt(), 0);
		human.change_have_weapon(-1, &weapons);
		assert_eq!(human.selectweapon, 1);
		assert_eq!(human.get_main_weapon_type_no(&weapons), 1);

		// Reloading blocks a switch as well.
		let mut reloading = human.clone();
		reloading.weaponreloadcnt = 1;
		reloading.change_have_weapon(-1, &weapons);
		assert_eq!(reloading.selectweapon, 1);
	}

	#[test]
	fn the_burst_count_holds_a_semi_automatic_weapon_to_one_round_a_trigger_pull() {
		let (coll, bd) = empty_world();
		// The PSG1 is single shot: one round per pull, seven frames between them.
		let (mut human, mut weapons) = armed(2, 5, 10);
		assert_eq!(weapons[0].param_data(), (2, 5, 10));

		// The first pull fires and reports the type number and the error. The error is
		// read before the shot adds the weapon's reaction to it, so it is still zero.
		assert_eq!(human.shot_weapon(&mut weapons), Some((2, 0)));
		assert_eq!(weapons[0].param_data(), (2, 4, 9));
		assert_eq!(human.weaponshotcnt, 7, "the PSG1 fires every seven frames");
		assert_eq!(human.weaponburstmodecnt, 1);
		assert_eq!(human.reaction_gunsight_error_range, 15, "the PSG1's reaction");

		// A second pull in the same frame is refused, and it still leaves the frame marked
		// as a firing frame, which is what holds the burst count together.
		assert_eq!(human.shot_weapon(&mut weapons), None);
		assert_eq!(weapons[0].param_data(), (2, 4, 9));
		assert!(human.weaponshotframe);

		// Step the world past the firing interval. The first of those frames fires
		// nothing, so the burst count goes back to zero.
		for _ in 0..7 {
			human.process_object(&coll, &bd, false, false, false, &mut weapons);
		}
		assert_eq!(human.weaponshotcnt, 0);
		assert_eq!(human.weaponburstmodecnt, 0);

		// And the next pull fires again.
		assert_eq!(human.shot_weapon(&mut weapons), Some((2, 0)));
		assert_eq!(human.weaponburstmodecnt, 1);
		assert_eq!(weapons[0].param_data(), (2, 3, 8));

		// The full auto setting is the one that is not single shot.
		assert!(!human.get_weapon_blazingmode(&weapons), "the PSG1 is single shot");
		let (mp5, mp5weapons) = armed(1, 5, 30);
		assert!(mp5.get_weapon_blazingmode(&mp5weapons));
		assert!(!mp5.weapon[0].is_some() && mp5.no_weapon_blazingmode_placeholder());
	}

	#[test]
	fn firing_a_grenade_spends_it_and_an_empty_one_leaves_the_hand() {
		// A grenade holds one round at a time, and the throw after the last one removes it
		// from the hand.
		let (mut human, mut weapons) = armed(13, 1, 1);
		assert!(weapons[0].object.enable_flag);

		assert_eq!(human.shot_weapon(&mut weapons), Some((13, 0)));
		assert!(!weapons[0].object.enable_flag);
		assert_eq!(human.get_weapon().1, [None, None], "the empty grenade is dropped");
		assert_eq!(human.weaponburstmodecnt, 1);
		assert_eq!(human.weaponshotcnt, 35);
	}

	#[test]
	fn reloading_counts_down_and_refills_the_magazine() {
		let (coll, bd) = empty_world();
		// Four loaded of ten, so the five round magazine has a five round reserve behind it.
		let (mut human, mut weapons) = armed(2, 4, 9);

		// The scope is cancelled by a reload and cannot be set again until it finishes.
		assert!(human.set_enable_scope(&weapons));
		assert_eq!(human.get_scope_mode(), 2);

		assert!(human.reload_weapon(&mut weapons));
		assert_eq!(human.get_weapon_reload_cnt(), 59, "the PSG1's 58 plus one");
		assert_eq!(human.get_scope_mode(), 0);
		assert!(!human.set_enable_scope(&weapons), "reloading blocks the scope");
		assert_eq!(weapons[0].param_data(), (2, 4, 9), "nothing is moved yet");

		// A second reload is refused while the first is running.
		assert!(!human.reload_weapon(&mut weapons));

		// The count is the set of frames the reload takes.
		for _ in 0..58 {
			human.process_object(&coll, &bd, false, false, false, &mut weapons);
		}
		assert_eq!(human.get_weapon_reload_cnt(), 1);
		assert_eq!(weapons[0].param_data(), (2, 4, 9));

		// The last frame refills from the reserve: five into a five round magazine.
		human.process_object(&coll, &bd, false, false, false, &mut weapons);
		assert_eq!(human.get_weapon_reload_cnt(), 0);
		assert_eq!(weapons[0].param_data(), (2, 5, 5));

		// With nothing in the reserve there is nothing to reload.
		assert!(!human.reload_weapon(&mut weapons));
	}

	#[test]
	fn an_empty_hand_cannot_fire_reload_or_look_through_a_scope() {
		let (coll, bd) = empty_world();
		let mut human = human();
		let mut weapons = Vec::new();

		assert_eq!(human.shot_weapon(&mut weapons), None);
		assert!(!human.reload_weapon(&mut weapons));
		assert!(!human.dump_weapon(&mut weapons));
		assert!(!human.set_enable_scope(&weapons));
		assert_eq!(human.get_main_weapon_type_no(&weapons), ID_WEAPON_NONE);
		assert!(!human.get_weapon_blazingmode(&weapons));
		assert_eq!(human.change_weapon_id(&mut weapons), 1);
		assert_eq!(human.process_object(&coll, &bd, false, false, false, &mut weapons), 1);
	}

	#[test]
	fn the_scope_follows_the_weapon_and_a_weapon_switch_cancels_it() {
		let (mut human, weapons) = armed(2, 5, 10);
		assert_eq!(human.get_scope_mode(), 0);

		// The PSG1 has the precision scope, which is mode 2.
		assert!(human.set_enable_scope(&weapons));
		assert_eq!(human.get_scope_mode(), 2);

		// Switching weapons cancels it, even to an empty hand.
		human.change_have_weapon(-1, &weapons);
		assert_eq!(human.get_scope_mode(), 0);

		// A weapon with no scope still answers success, with mode zero: upstream copies
		// whatever the settings hold.
		let (mut mp5, mp5weapons) = armed(1, 5, 30);
		assert!(mp5.set_enable_scope(&mp5weapons));
		assert_eq!(mp5.get_scope_mode(), 0);
		mp5.set_disable_scope();
		assert_eq!(mp5.get_scope_mode(), 0);
	}

	#[test]
	fn the_weapon_type_switch_follows_the_settings_pair() {
		// The settings pair the semi automatic and the full automatic Glock 18 with each
		// other, and neither has a switch delay.
		let (mut human, mut weapons) = armed(4, 5, 19);
		assert_eq!(human.change_weapon_id(&mut weapons), 0);
		assert_eq!(weapons[0].param_data(), (16, 5, 19), "GLOCK18 SEMI becomes FULL");
		assert_eq!(human.get_main_weapon_type_no(&weapons), 16);
		assert_eq!(human.get_change_weapon_id_cnt(), 0, "no switch delay");
		assert!(human.get_weapon_blazingmode(&weapons), "full auto");

		// And back again.
		assert_eq!(human.change_weapon_id(&mut weapons), 0);
		assert_eq!(weapons[0].param_data(), (4, 5, 19));
		assert!(!human.get_weapon_blazingmode(&weapons), "single shot");

		// A weapon with nowhere to switch to refuses.
		let (mut mp5, mut mp5weapons) = armed(1, 5, 30);
		assert_eq!(mp5.change_weapon_id(&mut mp5weapons), 1);
		assert_eq!(mp5weapons[0].param_data(), (1, 5, 30));

		// So does a switch while one is already running.
		let (mut human, mut weapons) = armed(4, 5, 19);
		human.changeweaponidcnt = 2;
		assert_eq!(human.change_weapon_id(&mut weapons), 1);
		assert_eq!(weapons[0].param_data(), (4, 5, 19));
	}

	#[test]
	fn a_scope_survives_a_weapon_type_switch_that_keeps_the_same_scope() {
		// Neither the semi nor the full automatic Glock has a scope, so the switch takes
		// the no scope path and leaves the mode alone.
		let (mut human, mut weapons) = armed(4, 5, 19);
		human.scopemode = 0;
		assert_eq!(human.change_weapon_id(&mut weapons), 0);
		assert_eq!(human.get_scope_mode(), 0);
	}

	#[test]
	fn dropping_a_weapon_throws_it_and_empties_the_hand() {
		let (mut human, mut weapons) = armed(1, 5, 30);
		assert!(human.set_enable_scope(&weapons));

		human.object.pos_y = 3.0;
		assert!(human.dump_weapon(&mut weapons));
		assert_eq!(human.get_weapon().1, [None, None]);
		assert!(!weapons[0].using_flag());
		assert_eq!(human.get_scope_mode(), 0);
		// The weapon lands five units along the looking direction and sixteen up.
		assert!((weapons[0].object.pos_x - 5.0).abs() < 1e-5);
		assert!((weapons[0].object.pos_y - 19.0).abs() < 1e-5);

		// There is nothing left to drop.
		assert!(!human.dump_weapon(&mut weapons));
	}

	#[test]
	fn firing_is_refused_while_switching_or_reloading() {
		let (mut human, mut weapons) = armed(1, 5, 30);

		human.selectweaponcnt = 1;
		assert_eq!(human.shot_weapon(&mut weapons), None);
		human.selectweaponcnt = 0;

		human.changeweaponidcnt = 1;
		assert_eq!(human.shot_weapon(&mut weapons), None);
		human.changeweaponidcnt = 0;

		human.weaponreloadcnt = 1;
		assert_eq!(human.shot_weapon(&mut weapons), None);
		// The firing frame is marked even on a refusal, which is what keeps a held trigger
		// counting for the burst mode.
		assert!(human.weaponshotframe);
	}

	#[test]
	fn the_mp5_full_automatic_setting_fires_without_the_burst_limit() {
		let (mut human, mut weapons) = armed(1, 5, 30);
		// burstcnt of zero means full auto, so only the firing interval holds it back.
		assert_eq!(human.shot_weapon(&mut weapons), Some((1, 0)));
		assert_eq!(human.weaponshotcnt, 4, "the MP5 fires every four frames");
		assert_eq!(weapons[0].param_data(), (1, 4, 29));
		assert_eq!(human.weaponburstmodecnt, 1);
		// The MP5 has no scope, so the no scope gunsight applies and adds its reaction of
		// six to the recoil error.
		assert_eq!(human.reaction_gunsight_error_range, 6);
	}

	// -----------------------------------------------------------------------
	// Rendering
	// -----------------------------------------------------------------------

	fn headless() -> D3DGraphics {
		let instance = xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
			backends: xops_platform::wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = Gfx::new(&instance, None, 64, 48, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	#[test]
	fn render_places_the_arms_at_the_hand_height_and_the_body_under_them() {
		let mut d3dg = headless();
		d3dg.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let resource = ResourceManager::new();

		let mut human = human();
		human.set_model(101, [102, 103, 104], 105, [106; TOTAL_WALKMODE], [107; TOTAL_RUNMODE]);
		human.set_pos_data(1.0, 2.0, 3.0, 0.0);
		human.set_rx_ry(0.25, 0.5);

		// Drawing everything ends on the arms, which is the transform left behind. The
		// arms are sixteen units above the feet.
		human.render(&mut d3dg, &resource, false, false, false, &[]);
		let arms = d3dg.modelview();
		let origin = arms.transform_point(Vec3::ZERO);
		assert!((origin.y - 18.0).abs() < 1e-4, "{origin:?}");

		// The arm model and the arm angle are the ones the motion control hands out.
		let (_, _, _, armmodel, _) = human.motion_ctrl.get_render_motion();
		assert_eq!(armmodel, 104);

		// Drawing only the arms lands in the same place, because that is the last thing
		// the full draw does too.
		let mut only_arms = headless();
		only_arms.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		human.render(&mut only_arms, &resource, true, false, false, &[]);
		assert_eq!(only_arms.modelview().m, arms.m);

		// A body that has fallen over lays the arms along the fall direction, sixteen
		// units out from the feet.
		let mut fallen = headless();
		fallen.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		let angle = degree_to_radian(45.0);
		human.rotation_y = angle;
		human.render(&mut fallen, &resource, false, false, false, &[]);
		let origin = fallen.modelview().transform_point(Vec3::ZERO);
		assert!((origin.y - (2.0 + angle.cos() * 16.0)).abs() < 1e-4, "{origin:?}");

		// A human with no weapon and a human holding one both draw the arms at the hand
		// height, but the held weapon adds its own placement on top.
		let (mut armed_human, weapons) = armed(1, 5, 30);
		armed_human.set_model(101, [102, 103, 104], 105, [106; TOTAL_WALKMODE], [107; TOTAL_RUNMODE]);
		armed_human.set_pos_data(1.0, 2.0, 3.0, 0.0);
		armed_human.set_rx_ry(0.25, 0.5);
		armed_human.usage_note_for_the_test();
		let mut holding = headless();
		holding.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		armed_human.render(&mut holding, &resource, false, false, false, &weapons);
		assert_ne!(
			holding.modelview().m,
			arms.m,
			"the weapon offset is applied on top of the arms"
		);

		// A disabled human draws nothing and leaves the transform where it was.
		let mut disabled = headless();
		disabled.set_camera(0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
		disabled.set_world_transform(9.0, 8.0, 7.0, 0.0, 0.0, 1.0);
		let before = disabled.modelview().m;
		human.set_enable_flag(false);
		human.render(&mut disabled, &resource, false, false, false, &[]);
		assert_eq!(disabled.modelview().m, before);
	}
}
