//! The object base class and the constants the whole object layer uses.
//!
//! Upstream `object` is a base class with virtual methods and the subclasses override them.
//! The manager holds one typed array per kind rather than one polymorphic array, so
//! nothing here needs virtual dispatch: `Human`, `Weapon`, `SmallObject`, `Bullet`,
//! `Grenade` and `Effect` are plain structs that embed this one by value, which is the
//! direct translation of the inheritance.

use crate::core::degree_to_radian;
use crate::d3dgraphics::D3DGraphics;
use crate::parameter::ParameterInfo;

// ---------------------------------------------------------------------------
// Constants, exactly as the upstream header defines them
// ---------------------------------------------------------------------------

// Movement. The note in the upstream header applies: when changing these, review the
// collision constants too.
pub const HUMAN_PROGRESSRUN_ACCELERATION: f32 = 0.7;
pub const HUMAN_SIDEWAYSRUN_ACCELERATION: f32 = 0.5;
pub const HUMAN_REGRESSRUN_ACCELERATION: f32 = 0.38;
pub const HUMAN_PROGRESSRUN_SIDEWAYSRUN_ACCELERATION: f32 = (HUMAN_PROGRESSRUN_ACCELERATION
	+ HUMAN_SIDEWAYSRUN_ACCELERATION)
	/ 2.0;
pub const HUMAN_REGRESSRUN_SIDEWAYSRUN_ACCELERATION: f32 = HUMAN_REGRESSRUN_ACCELERATION;
pub const HUMAN_PROGRESSWALK_ACCELERATION: f32 = 0.35;
pub const HUMAN_ATTENUATION: f32 = 0.5;
pub const HUMAN_JUMP_SPEED: f32 = 2.2;
pub const HUMAN_HEIGHT: f32 = 20.0;

// Damage.
pub const HUMAN_DAMAGE_HEAD: f32 = 2.0;
pub const HUMAN_DAMAGE_UP: f32 = 1.0;
pub const HUMAN_DAMAGE_LEG: f32 = 0.7;
pub const HUMAN_DAMAGE_ZOMBIEU: i32 = 15;
pub const HUMAN_DAMAGE_ZOMBIEA: i32 = 5;
pub const HUMAN_DAMAGE_SPEED: f32 = 0.066;
pub const HUMAN_DAMAGE_MINSPEED: f32 = -3.8;
pub const HUMAN_DAMAGE_MAXSPEED: f32 = -7.0;
pub const HUMAN_DAMAGE_MAXFALL: i32 = 120;
pub const MAX_DAMAGE_GRENADE_DISTANCE: f32 = 80.0;
pub const HUMAN_DAMAGE_GRENADE_HEAD: i32 = 100;
pub const HUMAN_DAMAGE_GRENADE_LEG: i32 = 80;
pub const SMALLOBJECT_DAMAGE_GRENADE: i32 = 80;

// Arm angles.
pub const ARMRAD_NOWEAPON: f32 = degree_to_radian(-90.0 + 20.0);
pub const ARMRAD_RELOADWEAPON: f32 = degree_to_radian(-20.0);

// The map collision constants. Several are documented upstream as tuned for the standard
// SCHOOL map, so they keep their values rather than being tidied into something rounder.
pub const HUMAN_MAPCOLLISION_R: f32 = 2.8;
pub const HUMAN_MAPCOLLISION_HEIGHT: f32 = 10.0;
pub const HUMAN_MAPCOLLISION_ADD_HEIGHT_A: f32 = 9.0;
pub const HUMAN_MAPCOLLISION_ADD_HEIGHT_B: f32 = 13.0;
pub const HUMAN_MAPCOLLISION_SLOPEANGLE: f32 = degree_to_radian(50.0);
pub const HUMAN_MAPCOLLISION_CLIMBHEIGHT: f32 = 3.2;
pub const HUMAN_MAPCOLLISION_CLIMBADDY: f32 = 0.4;
pub const HUMAN_MAPCOLLISION_GROUND_HEIGHT: f32 = -0.5;
pub const HUMAN_MAPCOLLISION_GROUND_R1: f32 = 0.15;
pub const HUMAN_MAPCOLLISION_GROUND_R2: f32 = 0.50;
pub const HUMAN_MAPCOLLISION_GROUND_R3: f32 = 0.30;
pub const HUMAN_MAPCOLLISION_CHECK_MAXDIST: f32 = 12.0;
pub const HUMAN_MAPCOLLISION_CHECK_HEIGHT: f32 = HUMAN_HEIGHT - 3.0;

pub const HUMAN_DEADLINE: f32 = -100.0;
pub const HUMAN_DEADADDRY: f32 = degree_to_radian(0.75);

// Projectiles.
pub const BULLET_SPEEDSCALE: f32 = 2.5;
pub const BULLET_DESTROYFRAME: i32 = 40;
pub const GRENADE_DESTROYFRAME: i32 = 100;

/// Constants representing the state of a human's legs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HumanLegState {
	Stop = 0,
	Walk,
	Run,
}

/// Flags for the human movement operations.
pub mod move_flag {
	pub const FORWARD: i32 = 0x01;
	pub const BACK: i32 = 0x02;
	pub const LEFT: i32 = 0x04;
	pub const RIGHT: i32 = 0x08;
	pub const WALK: i32 = 0x10;
	pub const JUMP: i32 = 0x20;
}

/// The effect type.
pub mod effect_type {
	/// A plain billboard.
	pub const NORMAL: i32 = 0x00;
	/// Do not billboard it, so the model keeps its own rotation.
	pub const NOBILLBOARD: i32 = 0x01;
	/// Give it map collision, for shell casings that have to land.
	pub const COLLIDEMAP: i32 = 0x02;
}

/// The shared part of every object: where it is, what it draws, and whether it is alive.
///
/// Embedded by value in each concrete object, the way the upstream inherits from it.
#[derive(Clone, Debug)]
pub struct Object {
	/// The setting tables. Upstream a pointer set once before use. Here the tables are
	/// constants and this is a zero-sized handle, so holding it costs nothing and there is
	/// no pointer to keep in sync.
	pub param: ParameterInfo,
	pub pos_x: f32,
	pub pos_y: f32,
	pub pos_z: f32,
	pub rotation_x: f32,
	pub model_size: f32,
	pub id_parameter: i32,
	pub id_model: i32,
	pub id_texture: i32,
	pub dark_model_flag: bool,
	pub enable_flag: bool,
}

impl Object {
	pub fn new(param: ParameterInfo, x: f32, y: f32, z: f32, rx: f32, size: f32, flag: bool) -> Object {
		Object {
			param,
			pos_x: x,
			pos_y: y,
			pos_z: z,
			rotation_x: rx,
			model_size: size,
			id_parameter: 0,
			id_model: -1,
			id_texture: -1,
			dark_model_flag: false,
			enable_flag: flag,
		}
	}

	/// Registers the class that manages the setting values.
	pub fn set_parameter_info_class(&mut self, param: ParameterInfo) {
		self.param = param;
	}

	pub fn set_pos_data(&mut self, x: f32, y: f32, z: f32, rx: f32) {
		self.pos_x = x;
		self.pos_y = y;
		self.pos_z = z;
		self.rotation_x = rx;
	}

	/// Upstream fills whichever of four out-parameters are not null, so a caller can ask
	/// for any subset. A tuple covers that without writing through pointers.
	pub fn pos_data(&self) -> (f32, f32, f32, f32) {
		(self.pos_x, self.pos_y, self.pos_z, self.rotation_x)
	}

	pub fn set_enable_flag(&mut self, flag: bool) {
		self.enable_flag = flag;
	}

	pub fn enable_flag(&self) -> bool {
		self.enable_flag
	}

	pub fn set_model(&mut self, id: i32, size: f32) {
		self.id_model = id;
		self.model_size = size;
	}

	pub fn model(&self) -> (i32, f32) {
		(self.id_model, self.model_size)
	}

	pub fn set_texture(&mut self, id: i32) {
		self.id_texture = id;
	}

	pub fn texture(&self) -> i32 {
		self.id_texture
	}

	pub fn set_dark_model_flag(&mut self, flag: bool) {
		self.dark_model_flag = flag;
	}

	/// The base does no simulation, so this is the upstream's zero.
	pub fn process_object(&mut self) -> i32 {
		0
	}

	/// Places the object in the world and draws its model.
	pub fn render(&mut self, d3dg: &mut D3DGraphics, nomodel: bool) {
		if !self.enable_flag {
			return;
		}
		d3dg.set_world_transform(self.pos_x, self.pos_y, self.pos_z, self.rotation_x, 0.0, self.model_size);
		d3dg.render_model(self.id_model, self.id_texture, self.dark_model_flag, false, nomodel);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_derived_constants_match_the_upstream_expressions() {
		// These are written as expressions upstream. If the arithmetic is transcribed
		// wrongly the value drifts and nothing else notices.
		assert_eq!(
			HUMAN_PROGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			(HUMAN_PROGRESSRUN_ACCELERATION + HUMAN_SIDEWAYSRUN_ACCELERATION) / 2.0
		);
		assert_eq!(
			HUMAN_REGRESSRUN_SIDEWAYSRUN_ACCELERATION,
			HUMAN_REGRESSRUN_ACCELERATION
		);
		assert_eq!(HUMAN_MAPCOLLISION_CHECK_HEIGHT, HUMAN_HEIGHT - 3.0);
		assert!((ARMRAD_NOWEAPON - (-70.0f32).to_radians()).abs() < 1e-6);
		assert!((HUMAN_MAPCOLLISION_SLOPEANGLE - 50.0f32.to_radians()).abs() < 1e-6);
		assert!((HUMAN_DEADADDRY - 0.75f32.to_radians()).abs() < 1e-6);
	}

	#[test]
	fn a_new_object_starts_disabled_or_enabled_as_asked() {
		let param = ParameterInfo::new();
		let object = Object::new(param, 1.0, 2.0, 3.0, 0.5, 2.0, true);
		assert_eq!(object.pos_data(), (1.0, 2.0, 3.0, 0.5));
		assert_eq!(object.model(), (-1, 2.0));
		assert_eq!(object.texture(), -1);
		assert!(object.enable_flag());
		assert!(!object.dark_model_flag);
		// The upstream zeroes these in the constructor rather than leaving them set.
		assert_eq!(object.id_parameter, 0);
	}

	#[test]
	fn position_round_trips_through_the_setter() {
		let mut object = Object::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 1.0, true);
		object.set_pos_data(-4.0, 5.5, 6.25, 1.75);
		assert_eq!(object.pos_data(), (-4.0, 5.5, 6.25, 1.75));
	}

	#[test]
	fn the_move_flags_are_distinct_bits() {
		// They are combined with bitwise or upstream, so a duplicated value would make two
		// directions inseparable.
		let flags = [
			move_flag::FORWARD,
			move_flag::BACK,
			move_flag::LEFT,
			move_flag::RIGHT,
			move_flag::WALK,
			move_flag::JUMP,
		];
		let mut seen = 0;
		for flag in flags {
			assert_eq!(flag.count_ones(), 1, "{flag:#x} is not a single bit");
			assert_eq!(seen & flag, 0, "{flag:#x} repeats an earlier flag");
			seen |= flag;
		}
		assert_eq!(seen, 0x3F);
	}

	#[test]
	fn the_leg_states_and_effect_types_keep_their_values() {
		assert_eq!(HumanLegState::Stop as i32, 0);
		assert_eq!(HumanLegState::Walk as i32, 1);
		assert_eq!(HumanLegState::Run as i32, 2);
		assert_eq!(effect_type::NORMAL, 0x00);
		assert_eq!(effect_type::NOBILLBOARD, 0x01);
		assert_eq!(effect_type::COLLIDEMAP, 0x02);
	}
}
