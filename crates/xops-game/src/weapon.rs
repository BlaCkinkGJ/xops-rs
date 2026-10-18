//! The weapon class, from `object.cpp:2208-2535` and `object.h:234-266`.
//!
//! A weapon is either lying in the world after being dropped or held by a human. The
//! `usingflag` is which: a held weapon is skipped by the object manager's render and
//! simulation and drawn by the hand that holds it instead. The counters are total rounds
//! and loaded rounds, and the difference between them is the reserve a reload draws from.

use std::f32::consts::PI;

use crate::collision::Collision;
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::BlockDataInterface;
use crate::object::Object;
use crate::parameter::{ParameterInfo, ID_WEAPON_GRENADE};
use crate::resource::ResourceManager;

/// `weapon`.
#[derive(Clone, Debug)]
pub struct Weapon {
	pub object: Object,
	move_x: f32,
	move_y: f32,
	move_z: f32,
	/// Held by a human, so the object manager neither moves nor draws it.
	usingflag: bool,
	/// Total rounds, loaded and reserve.
	bullets: i32,
	/// Rounds in the magazine.
	loadbullets: i32,
	/// Coordinates are still moving, for a dropped weapon falling to the ground.
	motionflag: bool,
}

impl Weapon {
	pub fn new(
		param: ParameterInfo,
		x: f32,
		y: f32,
		z: f32,
		rx: f32,
		id_param: i32,
		nbs: i32,
		flag: bool,
	) -> Weapon {
		// The upstream leaves `model_size` unset when the settings are missing, because it
		// is a member of an object built without a constructor call. Zero is the value a
		// fresh allocation has in the port.
		let mut model_size = 0.0;
		if let Some(data) = param.get_weapon(id_param) {
			model_size = data.size;
		}

		let mut object = Object::new(param, x, y, z, rx, model_size, flag);
		object.id_parameter = id_param;
		// The upstream comments out the model and texture lookup from the settings and
		// leaves both at -1 for the resource manager to fill in.
		object.id_model = -1;
		object.id_texture = -1;

		Weapon {
			object,
			move_x: 0.0,
			move_y: 0.0,
			move_z: 0.0,
			usingflag: false,
			bullets: nbs,
			loadbullets: 0,
			motionflag: true,
		}
	}

	/// `weapon::SetPosData`. Also stops any drop motion, since the caller is placing it.
	pub fn set_pos_data(&mut self, x: f32, y: f32, z: f32, rx: f32) {
		self.object.pos_x = x;
		self.object.pos_y = y;
		self.object.pos_z = z;
		self.move_x = 0.0;
		self.move_y = 0.0;
		self.move_z = 0.0;
		self.object.rotation_x = rx;
	}

	pub fn set_param_data(&mut self, id_param: i32, lnbs: i32, nbs: i32, init: bool) {
		self.object.id_parameter = id_param;
		self.bullets = nbs;
		self.loadbullets = lnbs;

		if init {
			self.usingflag = false;
			self.motionflag = true;
		}
	}

	/// Upstream fills whichever of the three out-parameters are not null.
	pub fn param_data(&self) -> (i32, i32, i32) {
		(self.object.id_parameter, self.loadbullets, self.bullets)
	}

	pub fn using_flag(&self) -> bool {
		self.usingflag
	}

	/// `weapon::Pickup`. 0 on success, 1 when someone already holds it.
	pub fn pickup(&mut self) -> i32 {
		if self.usingflag {
			return 1;
		}
		self.usingflag = true;
		0
	}

	/// `weapon::Dropoff`. Throws it five units along the looking direction, 16 up, and
	/// removes it from the holder. The initial velocity is the same direction.
	pub fn dropoff(&mut self, x: f32, y: f32, z: f32, rx: f32, speed: f32) {
		let direction = rx * -1.0 + PI / 2.0;
		let (sin, cos) = direction.sin_cos();

		self.move_x = cos * speed;
		self.move_y = 0.0;
		self.move_z = sin * speed;
		self.object.pos_x = x + cos * 5.0;
		self.object.pos_y = y + 16.0 + self.move_y;
		self.object.pos_z = z + sin * 5.0;
		self.object.rotation_x = rx + PI;

		self.usingflag = false;
		self.motionflag = true;
	}

	/// `weapon::Shot`. 0 on success, 1 when it cannot fire.
	///
	/// The rapid fire interval and the reload timer live in `human`, so the checks here are
	/// the ones a weapon can answer alone: it has a firing setting, it has rounds, and the
	/// setting fires at least one projectile.
	pub fn shot(&mut self) -> i32 {
		if self.loadbullets == 0 {
			return 1;
		}

		let Some(param_data) = self.object.param.get_weapon(self.object.id_parameter) else {
			return 1;
		};
		let param_pellet = param_data.pellet;
		if param_pellet <= 0 {
			return 1;
		}

		// A grenade is one round per throw, so the reserve and the magazine empty
		// together. The last throw removes the weapon and the throw before it reloads
		// the next grenade automatically.
		if self.object.id_parameter == ID_WEAPON_GRENADE {
			self.bullets -= 1;
			self.loadbullets -= 1;

			if (self.bullets - self.loadbullets) <= 0 {
				self.object.enable_flag = false;
				self.usingflag = false;
			} else if self.loadbullets <= 0 {
				self.loadbullets = 1;
			}
			return 0;
		}

		self.loadbullets -= 1;
		self.bullets -= 1;
		0
	}

	/// `weapon::StartReload`. 0 when there is a reserve to draw from, else 1.
	pub fn start_reload(&mut self) -> i32 {
		if (self.bullets - self.loadbullets) == 0 {
			return 1;
		}
		0
	}

	/// `weapon::RunReload`. Unlike [`Self::start_reload`] this refills at once, because the
	/// reload time is the caller's business.
	///
	/// The upstream has the two branches the wrong way round, or at least its comments
	/// say so: when the reserve is smaller than the magazine it keeps only the reserve and
	/// loses the rounds already loaded. Behaviour is kept, because the counters the human
	/// UI shows come from these two numbers.
	pub fn run_reload(&mut self) -> i32 {
		if (self.bullets - self.loadbullets) == 0 {
			return 1;
		}

		let nbsmax = match self.object.param.get_weapon(self.object.id_parameter) {
			Some(data) => data.nbsmax,
			None => 0,
		};

		if (self.bullets - self.loadbullets) < nbsmax {
			self.bullets -= self.loadbullets;
			self.loadbullets = self.bullets;
		} else {
			self.bullets -= self.loadbullets;
			self.loadbullets = nbsmax;
		}

		0
	}

	/// `weapon::ResetWeaponParam`, the F6 and F7 cheat path.
	///
	/// Only a live weapon can be changed. A grenade with an empty magazine is given one,
	/// because otherwise the cheat would hand back a weapon that cannot fire.
	pub fn reset_weapon_param(
		&mut self,
		resource: &ResourceManager,
		d3dg: &D3DGraphics,
		id_param: i32,
		lnbs: i32,
		nbs: i32,
	) -> bool {
		if !self.object.enable_flag {
			return false;
		}

		self.object.id_parameter = id_param;
		self.bullets = nbs;
		self.loadbullets = lnbs;

		if id_param == ID_WEAPON_GRENADE && self.bullets > 0 && self.loadbullets == 0 {
			self.loadbullets = 1;
		}

		if let Some((model, texture)) = resource.get_weapon_model_texture(d3dg, id_param) {
			self.object.id_model = model;
			self.object.id_texture = texture;
		}
		if let Some(param_data) = self.object.param.get_weapon(id_param) {
			self.object.model_size = param_data.size;
		}

		true
	}

	/// `weapon::ProcessObject`, the free fall a dropped weapon goes through.
	///
	/// Returns 0 in every path, as upstream does.
	pub fn process_object(&mut self, coll: &Collision, block_data_if: &BlockDataInterface) -> i32 {
		if !self.object.enable_flag {
			return 0;
		}
		if self.usingflag || !self.motionflag {
			return 0;
		}

		// Air drag and gravity. The vertical speed is capped at 1.8 per half step, which
		// is what keeps a dropped weapon from tunnelling through a floor.
		self.move_x *= 0.96;
		self.move_z *= 0.96;
		if self.move_y > -1.8 {
			self.move_y -= 0.3;
		}

		if coll.check_all_block_inside(
			block_data_if,
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
		) {
			self.motionflag = false;
			return 0;
		}

		let mut max_dist = (self.move_x * self.move_x + self.move_z * self.move_z).sqrt();
		if max_dist < 0.1 {
			max_dist = 0.0;
			self.move_x = 0.0;
			self.move_z = 0.0;
		}

		// The dummy ray reports no distance, only that the move is blocked, and the
		// upstream initialises its distance output to 0.0 on every hit. So the step back
		// is always the same tenth of a unit.
		if max_dist > 0.0
			&& coll
				.check_all_block_intersect_dummy_ray(
					block_data_if,
					self.object.pos_x,
					self.object.pos_y,
					self.object.pos_z,
					self.move_x / max_dist,
					0.0,
					self.move_z / max_dist,
					max_dist,
				)
				.is_some()
		{
			let dist = -0.1;
			self.object.pos_x += self.move_x / max_dist * dist;
			self.object.pos_z += self.move_z / max_dist * dist;
			self.move_x = 0.0;
			self.move_z = 0.0;
		}

		// The fall is split in two so a half step cannot pass through a thin block.
		for _ in 0..2 {
			if coll.check_all_block_inside(
				block_data_if,
				self.object.pos_x,
				self.object.pos_y + self.move_y * 0.5,
				self.object.pos_z,
			) {
				self.motionflag = false;
				return 0;
			}
			self.object.pos_y += self.move_y * 0.5;
		}

		self.object.pos_x += self.move_x;
		self.object.pos_z += self.move_z;
		0
	}

	/// `weapon::Render`. Laid flat, so the model lies on its side when it is on the ground.
	pub fn render(&mut self, d3dg: &mut D3DGraphics, nomodel: bool) {
		if !self.object.enable_flag || self.usingflag {
			return;
		}

		d3dg.set_world_transform_rot(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x,
			0.0,
			PI / 2.0,
			self.object.model_size,
		);
		d3dg.render_model(
			self.object.id_model,
			self.object.id_texture,
			self.object.dark_model_flag,
			false,
			nomodel,
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn empty_world() -> (Collision, BlockDataInterface) {
		(Collision::new(), BlockDataInterface::new())
	}

	fn rifle() -> Weapon {
		Weapon::new(ParameterInfo::new(), 1.0, 2.0, 3.0, 0.5, 1, 8, true)
	}

	#[test]
	fn the_constructor_reads_the_rounds_and_the_draw_size() {
		let weapon = rifle();
		assert_eq!(weapon.object.pos_data(), (1.0, 2.0, 3.0, 0.5));
		assert_eq!(weapon.param_data(), (1, 0, 8));
		assert_eq!(weapon.object.model(), (-1, 7.5));
		assert_eq!(weapon.object.texture(), -1);
		assert!(!weapon.using_flag());
		assert!(weapon.object.enable_flag());
	}

	#[test]
	fn shooting_spends_a_round_from_both_counts() {
		let mut weapon = rifle();
		weapon.set_param_data(1, 5, 8, true);
		assert_eq!(weapon.shot(), 0);
		assert_eq!(weapon.param_data(), (1, 4, 7));
		assert_eq!(weapon.shot(), 0);
		assert_eq!(weapon.param_data(), (1, 3, 6));
	}

	#[test]
	fn an_empty_magazine_or_a_weapon_that_fires_nothing_fails() {
		let mut weapon = rifle();
		weapon.set_param_data(1, 0, 8, true);
		assert_eq!(weapon.shot(), 1);

		// ID_WEAPON_NONE has a pellet count of 0, so it cannot fire even with rounds.
		weapon.set_param_data(0, 5, 8, true);
		assert_eq!(weapon.shot(), 1);

		// Both failures leave the counters alone.
		assert_eq!(weapon.param_data(), (0, 5, 8));
	}

	#[test]
	fn the_last_grenade_removes_the_weapon_and_earlier_ones_reload() {
		let mut weapon = Weapon::new(
			ParameterInfo::new(),
			0.0,
			0.0,
			0.0,
			0.0,
			ID_WEAPON_GRENADE,
			3,
			true,
		);
		weapon.set_param_data(ID_WEAPON_GRENADE, 1, 3, true);

		// Two left after the throw: the next grenade is loaded at once.
		assert_eq!(weapon.shot(), 0);
		assert_eq!(weapon.param_data(), (ID_WEAPON_GRENADE, 1, 2));
		assert!(weapon.object.enable_flag());

		// The last one takes the weapon out of the world.
		assert_eq!(weapon.shot(), 0);
		assert_eq!(weapon.param_data(), (ID_WEAPON_GRENADE, 1, 1));
		assert!(weapon.object.enable_flag());

		assert_eq!(weapon.shot(), 0);
		assert_eq!(weapon.param_data(), (ID_WEAPON_GRENADE, 0, 0));
		assert!(!weapon.object.enable_flag());
	}

	#[test]
	fn a_reload_needs_a_reserve() {
		let mut weapon = rifle();
		weapon.set_param_data(1, 8, 8, true);
		assert_eq!(weapon.start_reload(), 1);
		weapon.set_param_data(1, 8, 10, true);
		assert_eq!(weapon.start_reload(), 0);
	}

	#[test]
	fn run_reload_keeps_the_upstream_counter_arithmetic() {
		// Reserve of 3 against a 30 round magazine: the upstream keeps the reserve alone
		// and drops the loaded rounds.
		let mut weapon = rifle();
		weapon.set_param_data(1, 5, 8, true);
		assert_eq!(weapon.run_reload(), 0);
		assert_eq!(weapon.param_data(), (1, 3, 3));

		// Reserve of 80: the magazine fills to 30 and the reserve loses the old rounds.
		weapon.set_param_data(1, 20, 100, true);
		assert_eq!(weapon.run_reload(), 0);
		assert_eq!(weapon.param_data(), (1, 30, 80));

		// A full magazine is the failure case.
		weapon.set_param_data(1, 30, 30, true);
		assert_eq!(weapon.run_reload(), 1);
	}

	#[test]
	fn pickup_only_succeeds_once() {
		let mut weapon = rifle();
		assert_eq!(weapon.pickup(), 0);
		assert!(weapon.using_flag());
		assert_eq!(weapon.pickup(), 1);
	}

	#[test]
	fn dropping_throws_it_forward_and_up() {
		let mut weapon = rifle();
		assert_eq!(weapon.pickup(), 0);
		assert!(weapon.using_flag());

		// Looking along +X: rx 0 turns into a PI/2 drop direction, so all of the throw is
		// on Z and the weapon lands five units forward and sixteen up.
		weapon.dropoff(0.0, 0.0, 0.0, 0.0, 2.0);
		assert!(!weapon.using_flag());
		let (x, y, z, rx) = weapon.object.pos_data();
		assert!(x.abs() < 1e-6, "{x}");
		assert_eq!(y, 16.0);
		assert_eq!(z, 5.0);
		assert!((rx - PI).abs() < 1e-6);
	}

	#[test]
	fn a_dropped_weapon_falls_and_slides_one_frame_at_a_time() {
		let (coll, block_data) = empty_world();
		let mut weapon = rifle();
		weapon.dropoff(0.0, 0.0, 0.0, 0.0, 2.0);

		// Drag turns the horizontal speed 2.0 into 1.92 and gravity starts at -0.3. The
		// fall is applied in two half steps of -0.15, so the height drops by 0.3 and the
		// horizontal move is applied once at the end.
		assert_eq!(weapon.process_object(&coll, &block_data), 0);
		let (x, y, z, _) = weapon.object.pos_data();
		assert!(x.abs() < 1e-6, "{x}");
		assert!((y - 15.7).abs() < 1e-5, "{y}");
		assert!((z - 6.92).abs() < 1e-5, "{z}");

		// The next frame drops by the new vertical speed and slides on.
		assert_eq!(weapon.process_object(&coll, &block_data), 0);
		let (_, y, z, _) = weapon.object.pos_data();
		assert!((y - 15.1).abs() < 1e-5, "{y}");
		assert!((z - 8.7632).abs() < 1e-4, "{z}");
	}

	#[test]
	fn a_held_or_disabled_weapon_does_not_move() {
		let (coll, block_data) = empty_world();
		let mut weapon = rifle();
		weapon.dropoff(0.0, 0.0, 0.0, 0.0, 2.0);
		let before = weapon.object.pos_data();

		weapon.pickup();
		assert_eq!(weapon.process_object(&coll, &block_data), 0);
		assert_eq!(weapon.object.pos_data(), before);

		weapon.object.set_enable_flag(false);
		assert_eq!(weapon.process_object(&coll, &block_data), 0);
		assert_eq!(weapon.object.pos_data(), before);
	}
}
