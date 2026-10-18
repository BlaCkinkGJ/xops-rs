//! The rest of the object layer: small objects, bullets, grenades and effects, from
//! `object.cpp:2536-3233`.
//!
//! The four are the short-lived half of the object table. The manager creates them in
//! bulk and reuses the slots, so most of what they do is advance one frame of a state
//! machine and report whether they are finished.
//!
//! `grenade` derives from `bullet` upstream. Here it holds a [`Bullet`] by value, because
//! the two override methods with different signatures (`SetParamData`, `ProcessObject`)
//! so the upstream does not dispatch one through the other's slot, and composition keeps
//! the position, team and fuse count in one place instead of copying four fields.

use std::f32::consts::PI;

use crate::collision::{Collision, COLLISION_ADDSIZE};
use crate::core::{degree_to_radian, GAMEFPS};
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::{BlockDataInterface, MIFInterface, MAX_ADDSMALLOBJECT};
use crate::object::{effect_type, Object, BULLET_DESTROYFRAME, GRENADE_DESTROYFRAME};
use crate::parameter::{
	ParameterInfo, ID_WEAPON_GRENADE, SMALLOBJECT_SCALE, TOTAL_PARAMETERINFO_SMALLOBJECT,
};
use crate::window::get_rand;

/// `SMALLOBJECT_COLLISIONSCALE` from `objectmanager.h`. The manager multiplies a small
/// object's `decide` by this to get its hit radius.
pub const SMALLOBJECT_COLLISIONSCALE: f32 = 0.13;

/// The slot number of an additional (mission-added) small object.
///
/// Upstream repeats this range check in five places. `None` means the id is one of the
/// built-in small objects and the value comes from the parameter table instead.
fn add_smallobject_id(id_param: i32) -> Option<i32> {
	let total = TOTAL_PARAMETERINFO_SMALLOBJECT as i32;
	if (total..total + MAX_ADDSMALLOBJECT as i32).contains(&id_param) {
		Some(id_param - total)
	} else {
		None
	}
}

// ---------------------------------------------------------------------------
// smallobject
// ---------------------------------------------------------------------------

/// `smallobject`, a prop that can be shot and breaks.
#[derive(Clone, Debug)]
pub struct SmallObject {
	pub object: Object,
	rotation_y: f32,
	point_p4: i16,
	hp: i32,
	jump_rx: f32,
	move_rx: f32,
	add_rx: f32,
	add_ry: f32,
	jump_cnt: i32,
}

impl SmallObject {
	/// The upstream takes the MIF pointer here and stores it. The file manager lives for
	/// the whole mission, so the port passes it to the methods that need it instead of
	/// storing a borrow inside every small object.
	pub fn new(
		param: ParameterInfo,
		x: f32,
		y: f32,
		z: f32,
		rx: f32,
		id_param: i32,
		p4: i16,
		flag: bool,
	) -> SmallObject {
		let mut object = Object::new(param, x, y, z, rx, SMALLOBJECT_SCALE, flag);
		object.id_parameter = id_param;

		let mut hp = 0;
		if let Some(data) = param.get_small_object(id_param) {
			hp = data.hp;
		}

		SmallObject {
			object,
			rotation_y: 0.0,
			point_p4: p4,
			hp,
			jump_rx: 0.0,
			move_rx: 0.0,
			add_rx: 0.0,
			add_ry: 0.0,
			jump_cnt: 0,
		}
	}

	pub fn set_param_data(
		&mut self,
		id_param: i32,
		p4: i16,
		init: bool,
		mif: Option<&MIFInterface>,
	) {
		self.rotation_y = 0.0;
		self.object.id_parameter = id_param;
		self.point_p4 = p4;

		if init {
			self.hp = 0;
			self.jump_rx = 0.0;
			self.move_rx = 0.0;
			self.add_rx = 0.0;
			self.add_ry = 0.0;
			self.jump_cnt = 0;

			if let Some(id) = add_smallobject_id(id_param) {
				if let Some(mif) = mif {
					self.hp = mif.get_add_smallobject_hp(id);
				}
			} else if let Some(data) = self.object.param.get_small_object(id_param) {
				self.hp = data.hp;
			}
		}
	}

	pub fn param_data(&self) -> (i32, i16) {
		(self.object.id_parameter, self.point_p4)
	}

	pub fn hp(&self) -> i32 {
		self.hp
	}

	/// The jump strength for this object, from the mission file for an added object or
	/// from the table for a built-in one.
	fn jump_value(&self, mif: Option<&MIFInterface>) -> i32 {
		if let Some(id) = add_smallobject_id(self.object.id_parameter) {
			mif.map(|mif| mif.get_add_smallobject_jump(id)).unwrap_or(0)
		} else {
			self.object
				.param
				.get_small_object(self.object.id_parameter)
				.map(|data| data.jump)
				.unwrap_or(0)
		}
	}

	/// `smallobject::CollisionMap`. Drops the object onto the block below it and returns
	/// how far it moved, so the caller can tell a landing from no floor at all.
	pub fn collision_map(
		&mut self,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
	) -> f32 {
		let Some((_, _, mut dist)) = coll.check_all_block_intersect_ray(
			block_data_if,
			self.object.pos_x,
			self.object.pos_y + COLLISION_ADDSIZE,
			self.object.pos_z,
			0.0,
			-1.0,
			0.0,
			1000.0,
		) else {
			return 0.0;
		};

		// The decide value is a tenth-of-a-unit size, not a distance.
		if let Some(id) = add_smallobject_id(self.object.id_parameter) {
			if let Some(mif) = mif {
				dist -= mif.get_add_smallobject_decide(id) as f32 / 10.0;
			}
		} else if let Some(data) = self.object.param.get_small_object(self.object.id_parameter) {
			dist -= data.decide as f32 / 10.0;
		}

		self.object.pos_y -= dist;
		dist
	}

	pub fn hit_bullet(&mut self, attacks: i32, mif: Option<&MIFInterface>) {
		self.hp -= attacks;
		if self.hp <= 0 {
			self.destruction(mif);
		}
	}

	/// Upstream warns the caller to apply the distance falloff before this, so the value
	/// here is the damage the explosion does at this object's position.
	pub fn hit_grenade_explosion(&mut self, attacks: i32, mif: Option<&MIFInterface>) {
		self.hp -= attacks;
		if self.hp <= 0 {
			self.destruction(mif);
		}
	}

	/// Start the death flight. The object stays enabled and keeps moving until
	/// [`Self::process_object`] has counted out one second.
	pub fn destruction(&mut self, mif: Option<&MIFInterface>) {
		let jump = self.jump_value(mif);

		self.hp = 0;
		self.jump_cnt = jump;
		self.jump_rx = degree_to_radian(10.0) * get_rand(36) as f32;
		self.move_rx = jump as f32 * 0.04243;
		self.add_rx = degree_to_radian(1.0) * get_rand(20) as f32;
		self.add_ry = degree_to_radian(1.0) * get_rand(20) as f32;
	}

	/// `smallobject::ProcessObject`. 0 when there is nothing to do, 1 while the broken
	/// pieces fly, 2 on the frame it is removed.
	pub fn process_object(&mut self, mif: Option<&MIFInterface>) -> i32 {
		if !self.object.enable_flag {
			return 0;
		}
		if self.hp > 0 {
			return 0;
		}

		let jump = self.jump_value(mif);
		let cnt = jump - self.jump_cnt;

		self.object.pos_x += self.jump_rx.cos() * self.move_rx;
		self.object.pos_y += self.jump_cnt as f32 * 0.1;
		self.object.pos_z += self.jump_rx.sin() * self.move_rx;
		self.object.rotation_x += self.add_rx;
		self.rotation_y += self.add_ry;

		self.jump_cnt -= 1;

		// The upstream casts the frame rate to int first, so the limit is 33 frames, not
		// 33.33.
		if cnt > GAMEFPS as i32 {
			self.object.enable_flag = false;
			return 2;
		}

		1
	}

	pub fn render(&mut self, d3dg: &mut D3DGraphics, nomodel: bool) {
		if !self.object.enable_flag {
			return;
		}

		d3dg.set_world_transform(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x,
			self.rotation_y,
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

// ---------------------------------------------------------------------------
// bullet
// ---------------------------------------------------------------------------

/// `bullet`, the straight-flying projectile.
#[derive(Clone, Debug)]
pub struct Bullet {
	pub object: Object,
	rotation_y: f32,
	attacks: i32,
	penetration: i32,
	speed: f32,
	teamid: i32,
	humanid: i32,
	ontargetcnt: f32,
	cnt: i32,
}

impl Bullet {
	pub fn new(param: ParameterInfo, modelid: i32, textureid: i32) -> Bullet {
		// Upstream leaves the position and both angles unset, because the constructor only
		// covers the parts a bullet needs before `SetPosData` is called.
		let mut object = Object::new(param, 0.0, 0.0, 0.0, 0.0, 1.0, false);
		object.id_model = modelid;
		object.id_texture = textureid;

		Bullet {
			object,
			rotation_y: 0.0,
			attacks: 0,
			penetration: 0,
			speed: 0.0,
			teamid: 0,
			humanid: 0,
			ontargetcnt: 0.0,
			cnt: 0,
		}
	}

	pub fn set_pos_data(&mut self, x: f32, y: f32, z: f32, rx: f32, ry: f32) {
		self.object.pos_x = x;
		self.object.pos_y = y;
		self.object.pos_z = z;
		self.object.rotation_x = rx;
		self.rotation_y = ry;
	}

	pub fn set_param_data(
		&mut self,
		attacks: i32,
		penetration: i32,
		speed: f32,
		teamid: i32,
		humanid: i32,
		ontargetcnt: f32,
		init: bool,
	) {
		self.attacks = attacks;
		self.penetration = penetration;
		self.speed = speed;
		self.teamid = teamid;
		self.humanid = humanid;
		self.ontargetcnt = ontargetcnt;

		if init {
			self.cnt = 0;
		}
	}

	pub fn pos_data(&self) -> (f32, f32, f32, f32, f32) {
		(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x,
			self.rotation_y,
		)
	}

	pub fn param_data(&self) -> (i32, i32, f32, i32, i32, f32) {
		(
			self.attacks,
			self.penetration,
			self.speed,
			self.teamid,
			self.humanid,
			self.ontargetcnt,
		)
	}

	/// The frame count, which is the only public way to see the lifetime.
	pub fn count(&self) -> i32 {
		self.cnt
	}

	/// `bullet::ProcessObject`. The bullet flies until [`BULLET_DESTROYFRAME`] frames have
	/// passed, then disables itself. The return value is always 0, so the caller only
	/// looks at the enable flag.
	pub fn process_object(&mut self) -> i32 {
		if !self.object.enable_flag {
			return 0;
		}

		if self.cnt > BULLET_DESTROYFRAME {
			self.object.enable_flag = false;
			return 0;
		}

		self.object.pos_x += self.object.rotation_x.cos() * self.rotation_y.cos() * self.speed;
		self.object.pos_y += self.rotation_y.sin() * self.speed;
		self.object.pos_z += self.object.rotation_x.sin() * self.rotation_y.cos() * self.speed;
		self.cnt += 1;

		0
	}

	pub fn render(&mut self, d3dg: &mut D3DGraphics, nomodel: bool) {
		if !self.object.enable_flag {
			return;
		}
		// The first frame has not moved yet, so drawing it there makes it look like it
		// pierced whatever is close to the muzzle.
		if self.cnt == 0 {
			return;
		}

		d3dg.set_world_transform(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x * -1.0 - PI / 2.0,
			self.rotation_y,
			self.object.model_size,
		);
		// A bullet is never darkened.
		d3dg.render_model(self.object.id_model, self.object.id_texture, false, false, nomodel);
	}
}

// ---------------------------------------------------------------------------
// grenade
// ---------------------------------------------------------------------------

/// `grenade`, the bouncing projectile with a fuse.
///
/// It holds a [`Bullet`] rather than inheriting one, so the position, the team and the
/// fuse count have a single home. The two method sets do not overlap: the upstream
/// `SetParamData` and `ProcessObject` overloads hide the bullet's, which is why the
/// signatures differ and why nothing here forwards to the bullet's simulation.
#[derive(Clone, Debug)]
pub struct Grenade {
	pub bullet: Bullet,
	move_x: f32,
	move_y: f32,
	move_z: f32,
}

impl Grenade {
	pub fn new(param: ParameterInfo, modelid: i32, textureid: i32) -> Grenade {
		let mut bullet = Bullet::new(param, modelid, textureid);
		if let Some(data) = bullet.object.param.get_weapon(ID_WEAPON_GRENADE) {
			bullet.object.model_size = data.size;
		}

		Grenade {
			bullet,
			move_x: 0.0,
			move_y: 0.0,
			move_z: 0.0,
		}
	}

	/// The bullet's position setter. Upstream `SetPosData` must run before the grenade's
	/// `SetParamData`, because the launch velocity is built from the angles it sets.
	pub fn set_pos_data(&mut self, x: f32, y: f32, z: f32, rx: f32, ry: f32) {
		self.bullet.set_pos_data(x, y, z, rx, ry);
	}

	pub fn pos_data(&self) -> (f32, f32, f32, f32, f32) {
		self.bullet.pos_data()
	}

	pub fn set_param_data(
		&mut self,
		speed: f32,
		teamid: i32,
		humanid: i32,
		ontargetcnt: f32,
		init: bool,
	) {
		self.move_x = self.bullet.object.rotation_x.cos() * self.bullet.rotation_y.cos() * speed;
		self.move_y = self.bullet.rotation_y.sin() * speed;
		self.move_z = self.bullet.object.rotation_x.sin() * self.bullet.rotation_y.cos() * speed;
		self.bullet.teamid = teamid;
		self.bullet.humanid = humanid;
		self.bullet.ontargetcnt = ontargetcnt;

		if init {
			self.bullet.cnt = 0;
		}
	}

	pub fn param_data(&self) -> (f32, i32, i32, f32) {
		(
			self.get_speed(),
			self.bullet.teamid,
			self.bullet.humanid,
			self.bullet.ontargetcnt,
		)
	}

	pub fn get_speed(&self) -> f32 {
		(self.move_x * self.move_x + self.move_y * self.move_y + self.move_z * self.move_z).sqrt()
	}

	/// The frame count, which is the fuse.
	pub fn count(&self) -> i32 {
		self.bullet.cnt
	}

	/// `grenade::ProcessObject`. 0 for a normal frame, 1 for a bounce and 2 for the frame
	/// the fuse runs out. The caller turns the 2 into an explosion.
	pub fn process_object(&mut self, coll: &Collision, block_data_if: &BlockDataInterface) -> i32 {
		if !self.bullet.object.enable_flag {
			return 0;
		}

		if self.bullet.cnt > GRENADE_DESTROYFRAME {
			self.bullet.object.enable_flag = false;
			return 2;
		}

		// A grenade at rest still burns its fuse, so the count has to advance.
		if self.move_x == 0.0 && self.move_y == 0.0 && self.move_z == 0.0 {
			self.bullet.cnt += 1;
			return 0;
		}

		let max_dist = (self.move_x * self.move_x + self.move_y * self.move_y
			+ self.move_z * self.move_z)
			.sqrt();

		if let Some((id, face, _)) = coll.check_all_block_intersect_ray(
			block_data_if,
			self.bullet.object.pos_x,
			self.bullet.object.pos_y,
			self.bullet.object.pos_z,
			self.move_x / max_dist,
			self.move_y / max_dist,
			self.move_z / max_dist,
			max_dist,
		) {
			let angle = coll
				.angle_vector(block_data_if, id, face, self.move_x, self.move_y, self.move_z)
				.unwrap_or(0.0);
			let (vx, vy, vz) = coll
				.reflect_vector(block_data_if, id, face, self.move_x, self.move_y, self.move_z)
				.unwrap_or((self.move_x, self.move_y, self.move_z));

			// Hitting a face head on loses the most speed, a glancing hit almost none.
			let acceleration = angle * -1.0 * 0.2546 + 0.7;

			self.move_x = vx * acceleration;
			self.move_y = vy * acceleration;
			self.move_z = vz * acceleration;

			// It does not move on the frame of the collision, only slows down.
			self.move_x *= 0.98;
			self.move_y = (self.move_y - 0.17) * 0.98;
			self.move_z *= 0.98;

			self.bullet.cnt += 1;
			return 1;
		}

		self.bullet.object.pos_x += self.move_x;
		self.bullet.object.pos_y += self.move_y;
		self.bullet.object.pos_z += self.move_z;

		self.move_x *= 0.98;
		self.move_y = (self.move_y - 0.17) * 0.98;
		self.move_z *= 0.98;

		self.bullet.cnt += 1;
		0
	}

	pub fn render(&mut self, d3dg: &mut D3DGraphics, nomodel: bool) {
		if !self.bullet.object.enable_flag {
			return;
		}

		d3dg.set_world_transform_rot(
			self.bullet.object.pos_x,
			self.bullet.object.pos_y,
			self.bullet.object.pos_z,
			self.bullet.object.rotation_x * -1.0 - PI / 2.0,
			0.0,
			PI / 2.0,
			self.bullet.object.model_size,
		);
		d3dg.render_model(
			self.bullet.object.id_model,
			self.bullet.object.id_texture,
			self.bullet.object.dark_model_flag,
			false,
			nomodel,
		);
	}
}

// ---------------------------------------------------------------------------
// effect
// ---------------------------------------------------------------------------

/// `effect`, one billboard or board of a muzzle flash, blood, smoke or a shell casing.
///
/// `brightness` is kept because the upstream keeps it, but nothing ever reads it: neither
/// renderer takes a brightness argument for a board. The upstream adds to it every frame
/// and forgets it.
#[derive(Clone, Debug)]
pub struct Effect {
	pub object: Object,
	move_x: f32,
	move_y: f32,
	move_z: f32,
	addmove_y: f32,
	rotation_y: f32,
	rotation_texture: f32,
	addrotation_texture: f32,
	addmodel_size: f32,
	pub brightness: i32,
	addbrightness: i32,
	alpha: f32,
	add_alpha: f32,
	cnt: i32,
	/// A combination of [`effect_type`] flags.
	r#type: i32,
}

impl Effect {
	pub fn new(
		param: ParameterInfo,
		x: f32,
		y: f32,
		z: f32,
		size: f32,
		rotation: f32,
		texture: i32,
		count: i32,
	) -> Effect {
		let mut object = Object::new(param, x, y, z, 0.0, size, count > 0);
		object.id_texture = texture;

		Effect {
			object,
			move_x: 0.0,
			move_y: 0.0,
			move_z: 0.0,
			addmove_y: 0.0,
			rotation_y: 0.0,
			rotation_texture: rotation,
			addrotation_texture: 0.0,
			addmodel_size: 0.0,
			brightness: 0,
			addbrightness: 0,
			alpha: 1.0,
			add_alpha: 0.0,
			cnt: count,
			r#type: effect_type::NORMAL,
		}
	}

	/// `effect::SetParamData`. Note the upstream assigns `addrotation` to `rotation_x`
	/// rather than to `rotation_texture`, and the passed `rotation` to the texture
	/// rotation. Behaviour kept, because the callers are tuned around it.
	pub fn set_param_data(
		&mut self,
		in_move_x: f32,
		in_move_y: f32,
		in_move_z: f32,
		in_addmove_y: f32,
		rotation: f32,
		addrotation: f32,
		size: f32,
		addsize: f32,
		count: i32,
		settype: i32,
	) {
		self.move_x = in_move_x;
		self.move_y = in_move_y;
		self.move_z = in_move_z;
		self.addmove_y = in_addmove_y;
		self.object.rotation_x = addrotation;
		self.rotation_y = 0.0;
		self.rotation_texture = rotation;
		self.addrotation_texture = addrotation;
		self.object.model_size = size;
		self.addmodel_size = addsize;
		self.cnt = count;
		self.r#type = settype;
	}

	pub fn set_texture(
		&mut self,
		id: i32,
		in_brightness: i32,
		in_addbrightness: i32,
		in_alpha: f32,
		in_addalpha: f32,
	) {
		self.object.id_texture = id;
		self.brightness = in_brightness;
		self.addbrightness = in_addbrightness;
		self.alpha = in_alpha;
		self.add_alpha = in_addalpha;
	}

	pub fn set_rx_ry(&mut self, rx: f32, ry: f32) {
		self.object.rotation_x = rx;
		self.rotation_y = ry;
	}

	pub fn texture_id(&self) -> i32 {
		self.object.id_texture
	}

	pub fn get_move(&self) -> (f32, f32, f32) {
		(self.move_x, self.move_y, self.move_z)
	}

	pub fn collide_map_flag(&self) -> bool {
		self.r#type & effect_type::COLLIDEMAP != 0
	}

	/// The count, exposed for the manager and for tests.
	pub fn count(&self) -> i32 {
		self.cnt
	}

	pub fn alpha(&self) -> f32 {
		self.alpha
	}

	/// `effect::ProcessObject`. 0 when disabled, 1 for a normal frame, 2 on the frame it
	/// finishes. The 2 is what tells the manager the slot is free to reuse.
	pub fn process_object(&mut self) -> i32 {
		if !self.object.enable_flag {
			return 0;
		}

		if self.cnt <= 0 {
			self.object.enable_flag = false;
			return 2;
		}

		self.object.pos_x += self.move_x;
		self.object.pos_y += self.move_y;
		self.object.pos_z += self.move_z;
		self.move_y += self.addmove_y;

		self.rotation_texture += self.addrotation_texture;
		self.object.model_size += self.addmodel_size;

		self.brightness += self.addbrightness;
		self.alpha += self.add_alpha;

		// Fading out is the one bound the upstream enforces. A negative alpha is not
		// clamped to zero, it simply ends the effect.
		if self.alpha <= 0.0 {
			self.object.enable_flag = false;
			self.cnt = 0;
			return 2;
		}

		// A count of 255 or more is a permanent effect, so it never runs down.
		if self.cnt < 255 {
			self.cnt -= 1;
		}
		1
	}

	/// `effect::Render`. A billboard turns to face the camera before it draws, unless the
	/// type says not to.
	pub fn render(
		&mut self,
		d3dg: &mut D3DGraphics,
		camera_rx: f32,
		camera_ry: f32,
		nomodel: bool,
	) {
		if !self.object.enable_flag {
			return;
		}

		if self.r#type & effect_type::NOBILLBOARD == 0 {
			self.object.rotation_x = camera_rx * -1.0;
			self.rotation_y = camera_ry;
		}

		d3dg.set_world_transform_effect(
			self.object.pos_x,
			self.object.pos_y,
			self.object.pos_z,
			self.object.rotation_x,
			self.rotation_y,
			self.rotation_texture,
			self.object.model_size,
		);
		d3dg.render_board(self.object.id_texture, self.alpha, nomodel);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn empty_world() -> (Collision, BlockDataInterface) {
		(Collision::new(), BlockDataInterface::new())
	}

	// -- smallobject --------------------------------------------------------

	#[test]
	fn a_small_object_reads_its_health_and_scale_from_the_table() {
		let object = SmallObject::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 0, 7, true);
		assert_eq!(object.hp(), 6);
		assert_eq!(object.object.model_size, 5.0);
		assert_eq!(object.param_data(), (0, 7));

		let pc = SmallObject::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 1, 0, true);
		assert_eq!(pc.hp(), 60);
	}

	#[test]
	fn damage_accumulates_and_destruction_starts_the_flight() {
		let mut object = SmallObject::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 0, 0, true);

		// The can has 6 health, so 4 and then 2 is exactly the threshold.
		object.hit_bullet(4, None);
		assert_eq!(object.hp(), 2);
		assert!(object.object.enable_flag());
		assert_eq!(object.process_object(None), 0);

		object.hit_bullet(2, None);
		assert_eq!(object.hp(), 0);
		// The can jumps 10, and the launch speed is the jump scaled by 0.04243.
		assert_eq!(object.jump_cnt, 10);
		assert!((object.move_rx - 10.0 * 0.04243).abs() < 1e-6);
		assert_eq!(object.process_object(None), 1);
	}

	#[test]
	fn the_death_flight_ends_after_one_second_of_frames() {
		let mut object = SmallObject::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 0, 0, true);
		object.hit_grenade_explosion(6, None);
		assert_eq!(object.hp(), 0);

		// Jump 10 means the first call counts 0, so the 34th call counts 33 and still
		// runs, and the 35th counts 34 and removes it.
		for _ in 0..34 {
			assert_eq!(object.process_object(None), 1);
		}
		assert!(object.object.enable_flag());
		assert_eq!(object.process_object(None), 2);
		assert!(!object.object.enable_flag());
	}

	#[test]
	fn an_undamaged_small_object_does_not_process() {
		let mut object = SmallObject::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 0.0, 8, 0, true);
		assert!(!object.object.enable_flag() || object.hp() > 0);
		assert_eq!(object.process_object(None), 0);
		assert_eq!(object.object.pos_data(), (0.0, 0.0, 0.0, 0.0));
	}

	#[test]
	fn a_small_object_falls_to_the_ground_only_when_there_is_one() {
		let (coll, block_data) = empty_world();
		let mut object = SmallObject::new(ParameterInfo::new(), 0.0, 10.0, 0.0, 0.0, 0, 0, true);
		// An uninitialised collision has no blocks, so nothing is below it.
		assert_eq!(object.collision_map(&coll, &block_data, None), 0.0);
		assert_eq!(object.object.pos_y, 10.0);
	}

	// -- bullet -------------------------------------------------------------

	#[test]
	fn a_bullet_dies_at_the_frame_limit() {
		let mut bullet = Bullet::new(ParameterInfo::new(), 0, 0);
		bullet.object.enable_flag = true;
		bullet.set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
		bullet.set_param_data(10, 2, 5.0, 1, 3, 1.0, true);

		// 41 calls leave the count at 41, which is one past the limit but has not been
		// tested yet. The 42nd call sees it and removes the bullet.
		for expected in 1..=41 {
			assert_eq!(bullet.process_object(), 0);
			assert_eq!(bullet.count(), expected);
			assert!(bullet.object.enable_flag());
		}
		assert_eq!(bullet.process_object(), 0);
		assert_eq!(bullet.count(), BULLET_DESTROYFRAME + 1);
		assert!(!bullet.object.enable_flag());
	}

	#[test]
	fn a_bullet_flies_straight_along_its_angles() {
		let mut bullet = Bullet::new(ParameterInfo::new(), 0, 0);
		bullet.object.enable_flag = true;
		bullet.set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
		bullet.set_param_data(10, 2, 5.0, 1, 3, 1.0, true);

		for _ in 0..40 {
			bullet.process_object();
		}
		// 40 frames at 5 units each, all on X because both angles are zero.
		assert_eq!(bullet.pos_data(), (200.0, 0.0, 0.0, 0.0, 0.0));
		assert_eq!(bullet.count(), 40);
	}

	#[test]
	fn bullet_parameters_round_trip() {
		let mut bullet = Bullet::new(ParameterInfo::new(), 3, 4);
		bullet.set_param_data(44, 0, 7.5, 2, 9, 0.5, true);
		assert_eq!(bullet.param_data(), (44, 0, 7.5, 2, 9, 0.5));
		assert_eq!(bullet.object.model(), (3, 1.0));
		assert_eq!(bullet.object.texture(), 4);
	}

	// -- grenade ------------------------------------------------------------

	#[test]
	fn a_grenade_explodes_on_the_frame_after_the_fuse_runs_out() {
		let (coll, block_data) = empty_world();
		let mut grenade = Grenade::new(ParameterInfo::new(), 1, 1);
		grenade.bullet.object.enable_flag = true;
		grenade.set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
		grenade.set_param_data(8.0, 1, 2, 1.0, true);

		// 101 calls put the count at 101. The 101st call was tested with 100 and ran.
		for expected in 1..=100 {
			assert_eq!(grenade.process_object(&coll, &block_data), 0);
			assert_eq!(grenade.count(), expected);
		}
		assert_eq!(grenade.process_object(&coll, &block_data), 0);
		assert_eq!(grenade.count(), GRENADE_DESTROYFRAME + 1);
		assert!(grenade.bullet.object.enable_flag());

		assert_eq!(grenade.process_object(&coll, &block_data), 2);
		assert!(!grenade.bullet.object.enable_flag());
	}

	#[test]
	fn a_grenade_in_flight_moves_and_loses_speed() {
		let (coll, block_data) = empty_world();
		let mut grenade = Grenade::new(ParameterInfo::new(), 1, 1);
		grenade.bullet.object.enable_flag = true;
		grenade.set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
		grenade.set_param_data(10.0, 1, 2, 1.0, true);

		assert_eq!(grenade.process_object(&coll, &block_data), 0);
		// x gains the full 10, then the horizontal speed is scaled by 0.98. The vertical
		// speed has no gravity term on the first frame apart from the -0.17 step.
		let (x, y, z, _, _) = grenade.pos_data();
		assert_eq!(x, 10.0);
		assert!(y.abs() < 1e-6, "{y}");
		assert!(z.abs() < 1e-6, "{z}");
		assert!((grenade.get_speed() - (9.8f32 * 9.8 + 0.1666 * 0.1666).sqrt()).abs() < 1e-4);
	}

	#[test]
	fn a_resting_grenade_still_burns_its_fuse() {
		let (coll, block_data) = empty_world();
		let mut grenade = Grenade::new(ParameterInfo::new(), 1, 1);
		grenade.bullet.object.enable_flag = true;
		grenade.set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
		grenade.set_param_data(0.0, 1, 2, 1.0, true);

		assert_eq!(grenade.process_object(&coll, &block_data), 0);
		assert_eq!(grenade.count(), 1);
		assert_eq!(grenade.pos_data(), (0.0, 0.0, 0.0, 0.0, 0.0));
		assert_eq!(grenade.get_speed(), 0.0);
	}

	// -- effect -------------------------------------------------------------

	#[test]
	fn an_effect_runs_its_count_down_and_reports_the_last_frame() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, -1, 3);
		assert!(effect.object.enable_flag());

		assert_eq!(effect.process_object(), 1);
		assert_eq!(effect.count(), 2);
		assert_eq!(effect.process_object(), 1);
		assert_eq!(effect.count(), 1);
		assert_eq!(effect.process_object(), 1);
		assert_eq!(effect.count(), 0);
		assert_eq!(effect.process_object(), 2);
		assert!(!effect.object.enable_flag());
	}

	#[test]
	fn a_zero_count_effect_never_starts() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, -1, 0);
		assert!(!effect.object.enable_flag());
		assert_eq!(effect.process_object(), 0);
	}

	#[test]
	fn alpha_fades_to_zero_and_the_effect_ends_where_it_hits_it() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, 1, 100);
		effect.set_texture(1, 0, 0, 1.0, -0.25);

		for expected in [0.75, 0.5, 0.25] {
			assert_eq!(effect.process_object(), 1);
			assert_eq!(effect.alpha(), expected);
			assert!(effect.object.enable_flag());
		}

		// The fourth step lands exactly on zero, which is the end condition.
		assert_eq!(effect.process_object(), 2);
		assert_eq!(effect.alpha(), 0.0);
		assert_eq!(effect.count(), 0);
		assert!(!effect.object.enable_flag());
	}

	#[test]
	fn brightness_has_no_bound_upstream() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, 1, 100);
		effect.set_texture(1, 250, 3, 1.0, 0.0);
		for _ in 0..4 {
			effect.process_object();
		}
		// Nothing clamps it, and nothing reads it either. The value keeps climbing.
		assert_eq!(effect.brightness, 262);
	}

	#[test]
	fn a_long_count_stops_decrementing_at_255() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, 1, 300);
		assert_eq!(effect.count(), 300);
		assert_eq!(effect.process_object(), 1);
		assert_eq!(effect.count(), 300);
	}

	#[test]
	fn the_type_bits_select_collision_and_billboarding() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, 1, 5);
		effect.set_param_data(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5, effect_type::NORMAL);
		assert!(!effect.collide_map_flag());

		effect.set_param_data(
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			1.0,
			0.0,
			5,
			effect_type::NORMAL | effect_type::COLLIDEMAP,
		);
		assert!(effect.collide_map_flag());
		assert_eq!(effect.texture_id(), 1);

		// A NOBILLBOARD effect without COLLIDEMAP still has no map collision.
		effect.set_param_data(
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			1.0,
			0.0,
			5,
			effect_type::NOBILLBOARD,
		);
		assert!(!effect.collide_map_flag());
	}

	#[test]
	fn the_effect_ramps_position_scale_and_texture_rotation() {
		let mut effect = Effect::new(ParameterInfo::new(), 0.0, 0.0, 0.0, 1.0, 0.0, 1, 10);
		effect.set_param_data(2.0, 1.0, -1.0, -0.25, 0.5, 0.1, 2.0, 0.5, 10, effect_type::NORMAL);

		assert_eq!(effect.process_object(), 1);
		assert_eq!(effect.object.pos_data(), (2.0, 1.0, -1.0, 0.1));
		assert_eq!(effect.get_move(), (2.0, 0.75, -1.0));
		assert_eq!(effect.rotation_texture, 0.6);
		assert_eq!(effect.object.model_size, 2.5);
	}

	#[test]
	fn the_collision_scale_constant_keeps_its_upstream_value() {
		assert_eq!(SMALLOBJECT_COLLISIONSCALE, 0.13);
		assert_eq!(COLLISION_ADDSIZE, 0.01);
	}
}
