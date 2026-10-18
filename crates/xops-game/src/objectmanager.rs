//! The object manager, from `objectmanager.cpp` and `objectmanager.h`.
//!
//! The manager is the game's object table: one pool per kind of object, a slot per
//! object, and the per-frame machinery that walks them in order. Every pool is allocated
//! once, at construction, and a pool member is free when its enable flag is false. That is
//! the upstream's `useflag` test, and it is the only thing that decides where the next
//! object goes, so the slot a new object lands in is the lowest free one.
//!
//! **Where the collaborators live.** The mission data the manager works on (the block
//! data, the point data, the MIF and the collision class) is *not* stored here. It is
//! passed in as a parameter to the methods that need it, the way [`crate::human`],
//! [`crate::weapon`] and [`crate::effects`] already take it. Three reasons:
//!
//! * Every other ported module takes it that way, so a game loop that owns the mission and
//!   hands it out by reference is the existing convention rather than a new one.
//! * The AI (`ai.cpp`) is constructed with the same block data, point data and collision
//!   class. A manager that owned them by value could not hand them back out without
//!   giving up the ownership it took.
//! * A `&mut` of any of them is needed elsewhere (the point data is written back after a
//!   mission edit, the MIF is reloaded), and a stored borrow would forbid it.
//!
//! What the manager does own is the part that is nothing but its own state: the six pools,
//! the per-human score arrays, and the object log. `ParameterInfo` is a zero sized handle
//! to constant tables, so it is stored by value (it is `Copy`, and there is no other
//! instance to disagree with).
//!
//! **Two mutable objects at once.** The upstream aliases freely: `CollideHuman` mutates two
//! humans, and a bullet hit mutates the bullet and the human it hit. Nothing here reaches
//! for `Rc<RefCell<_>>`. The two cases use the two shapes the borrow checker accepts:
//!
//! * Two members of the *same* pool are split with [`slice::split_at_mut`], which is what
//!   [`ObjectManager::collide_human`] does, and the comment there says why the split point
//!   is the higher index.
//! * Members of *different* pools are borrowed as separate fields, which the compiler
//!   allows because the two fields are disjoint.

use std::f32::consts::PI;

use crate::collision::{
	check_target_angle, collide_cylinder, collide_cylinder_inside, collide_sphere_inside, Collision,
};
use crate::config::Config;
use crate::core::{
	degree_to_radian, point_param_to_int, ENABLE_BUG_HUMANWEAPON, ENABLE_BUG_TEAMID, ENABLE_DEBUGCONSOLE,
	GAMEFPS, TOTAL_HAVEWEAPON,
};
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::{
	BlockDataInterface, MIFInterface, PointDataInterface, Pointdata, MAX_ADDSMALLOBJECT,
	POINT_P1TYPE_HUMAN, POINT_P1TYPE_HUMAN2, POINT_P1TYPE_HUMANINFO, POINT_P1TYPE_RAND_WEAPON,
	POINT_P1TYPE_SMALLOBJ, POINT_P1TYPE_WEAPON,
};
use crate::effects::{Bullet, Effect, Grenade, SmallObject, SMALLOBJECT_COLLISIONSCALE};
use crate::human::Human;
use crate::object::{
	effect_type, BULLET_SPEEDSCALE, HUMAN_DAMAGE_GRENADE_HEAD, HUMAN_DAMAGE_GRENADE_LEG, HUMAN_DAMAGE_HEAD,
	HUMAN_DAMAGE_LEG, HUMAN_DAMAGE_UP, HUMAN_DAMAGE_ZOMBIEU, HUMAN_HEIGHT, MAX_DAMAGE_GRENADE_DISTANCE,
	SMALLOBJECT_DAMAGE_GRENADE,
};
use crate::parameter::{
	ParameterInfo, ID_WEAPON_GRENADE, ID_WEAPON_NONE, SMALLOBJECT_SCALE, TOTAL_PARAMETERINFO_HUMAN,
	TOTAL_PARAMETERINFO_SMALLOBJECT, TOTAL_PARAMETERINFO_WEAPON,
};
use crate::resource::ResourceManager;
use crate::soundmanager::SoundManager;
use crate::weapon::Weapon;
use crate::window::get_rand;

/// `VIEW_HEIGHT` from `gamemain.h`.
///
/// `gamemain` has no port yet, and this is the only constant of it the manager needs. It
/// moves there when that file arrives.
pub const VIEW_HEIGHT: f32 = 19.0;

/// `MAX_HUMAN` from `objectmanager.h`.
///
/// The header warns this must be a multiple of 24 for the AI level tuning, and 96 is.
pub const MAX_HUMAN: usize = 96;
/// `MAX_WEAPON`.
pub const MAX_WEAPON: usize = 200;
/// `MAX_SMALLOBJECT`.
pub const MAX_SMALLOBJECT: usize = 40;
/// `MAX_BULLET`.
pub const MAX_BULLET: usize = 128;
/// `MAX_GRENADE`.
pub const MAX_GRENADE: usize = 32;
/// `MAX_EFFECT`.
pub const MAX_EFFECT: usize = 256;

/// `WEAPONSHOT_HEIGHT`: the height a bullet leaves the muzzle at.
pub const WEAPONSHOT_HEIGHT: f32 = VIEW_HEIGHT;

/// `TOTAL_WEAPON_AUTOBULLET`: a multiple of the loaded magazine a virtual weapon starts
/// with.
pub const TOTAL_WEAPON_AUTOBULLET: i32 = 3;

/// `HUMAN_PICKUPWEAPON_L`: how far below a human's feet a weapon can still be picked up.
pub const HUMAN_PICKUPWEAPON_L: f32 = -2.0;
/// `HUMAN_PICKUPWEAPON_H`: how far above a human's feet a weapon can still be picked up.
pub const HUMAN_PICKUPWEAPON_H: f32 = 16.0;
/// `HUMAN_PICKUPWEAPON_R`: how far away a weapon can still be picked up.
pub const HUMAN_PICKUPWEAPON_R: f32 = 3.8;

/// `HUMAN_HUMANCOLLISION_R`: the radius two humans push each other apart at.
pub const HUMAN_HUMANCOLLISION_R: f32 = 2.5;

/// `HUMAN_BULLETCOLLISION_HEAD_H`: the height of the head hitbox above the shoulders.
pub const HUMAN_BULLETCOLLISION_HEAD_H: f32 = 2.9;
/// `HUMAN_BULLETCOLLISION_HEAD_R`.
pub const HUMAN_BULLETCOLLISION_HEAD_R: f32 = 1.5;
/// `HUMAN_BULLETCOLLISION_UP_H`: the height of the upper body hitbox.
pub const HUMAN_BULLETCOLLISION_UP_H: f32 = 7.0;
/// `HUMAN_BULLETCOLLISION_UP_R`.
pub const HUMAN_BULLETCOLLISION_UP_R: f32 = 2.4;
/// `HUMAN_BULLETCOLLISION_LEG_H`: the height of the leg hitbox.
pub const HUMAN_BULLETCOLLISION_LEG_H: f32 = 10.5;
/// `HUMAN_BULLETCOLLISION_LEG_R`.
pub const HUMAN_BULLETCOLLISION_LEG_R: f32 = 2.6;

/// `MAX_OBJECTMANAGER_LOGLEN`: how many lines the object log shows.
pub const MAX_OBJECTMANAGER_LOGLEN: usize = 3;
/// `MAX_OBJECTMANAGER_LOGLINES`: the character capacity of one log line.
pub const MAX_OBJECTMANAGER_LOGLINES: usize = 64;
/// `MAX_OBJECTMANAGER_LOGCNT`: how many frames a log line stays up.
///
/// Upstream `(int)(5.0f*GAMEFPS)`, and the cast is kept: the truncation is the count.
pub const MAX_OBJECTMANAGER_LOGCNT: i32 = (5.0 * GAMEFPS) as i32;

/// `effectdata`: one entry of the effect draw order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Effectdata {
	/// Data number.
	id: i32,
	/// Squared distance from the camera.
	dist: f32,
}

/// Trims a fixed log line at its terminator, which is where the upstream `strcpy` put it.
fn cstr(bytes: &[u8]) -> &[u8] {
	let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
	&bytes[..len]
}

// ---------------------------------------------------------------------------
// BulletObjectHumanIndex
// ---------------------------------------------------------------------------

/// `BulletObjectHumanIndex`, the list of humans one bullet has already hit.
///
/// A bullet with penetration passes through a human and must not hit the same body again
/// on the next step of its path, so each bullet carries one of these.
#[derive(Clone, Debug)]
pub struct BulletObjectHumanIndex {
	human_index: [bool; MAX_HUMAN],
}

impl BulletObjectHumanIndex {
	pub fn new() -> Self {
		BulletObjectHumanIndex {
			human_index: [false; MAX_HUMAN],
		}
	}

	/// `Init`.
	pub fn init(&mut self) {
		self.human_index = [false; MAX_HUMAN];
	}

	/// `GetIndexFlag`.
	pub fn index_flag(&self, id: i32) -> bool {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return false;
		}
		self.human_index[id as usize]
	}

	/// `SetIndexFlag`.
	pub fn set_index_flag(&mut self, id: i32) {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return;
		}
		self.human_index[id as usize] = true;
	}
}

impl Default for BulletObjectHumanIndex {
	fn default() -> Self {
		Self::new()
	}
}

// ---------------------------------------------------------------------------
// ObjectManagerLog
// ---------------------------------------------------------------------------

/// `ObjectManagerLog`, the three line overlay that says what happened to the objects.
///
/// A line carries a frame countdown, its own colour and its text. The count limit is
/// [`MAX_OBJECTMANAGER_LOGCNT`], which is five seconds of frames. The renderer is not
/// stored: the colours come from `get_color_code`, which is a plain function, and the draw
/// takes the renderer as an argument.
pub struct ObjectManagerLog {
	/// Frames left for each line, -1 for an empty line.
	text_cnt: [i32; MAX_OBJECTMANAGER_LOGLEN],
	/// The text of each line, terminated with a zero byte.
	text_str: [[u8; MAX_OBJECTMANAGER_LOGLINES]; MAX_OBJECTMANAGER_LOGLEN],
	/// The colour of each line.
	text_color: [u32; MAX_OBJECTMANAGER_LOGLEN],
}

impl ObjectManagerLog {
	pub fn new() -> Self {
		ObjectManagerLog {
			text_cnt: [-1; MAX_OBJECTMANAGER_LOGLEN],
			text_str: [[0; MAX_OBJECTMANAGER_LOGLINES]; MAX_OBJECTMANAGER_LOGLEN],
			text_color: [0; MAX_OBJECTMANAGER_LOGLEN],
		}
	}

	/// `ClearLog`.
	pub fn clear_log(&mut self) {
		for i in 0..MAX_OBJECTMANAGER_LOGLEN {
			self.text_cnt[i] = -1;
			self.text_str[i] = [0; MAX_OBJECTMANAGER_LOGLINES];
			self.text_color[i] = 0;
		}
	}

	/// `InfoLog`: a plain status line.
	pub fn info_log(&mut self, text: &[u8]) {
		self.add_text_log(
			MAX_OBJECTMANAGER_LOGCNT,
			text,
			D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0),
		);
	}

	/// `AddHuman`: a human was placed. An ally is cyan, an enemy is orange.
	pub fn add_human(&mut self, human_id: i32, team_id: i32, player_team_id: i32) {
		let color = if player_team_id == team_id {
			D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
		} else {
			D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0)
		};

		let text = format!("Added human[{human_id}]");
		self.add_text_log(MAX_OBJECTMANAGER_LOGCNT, text.as_bytes(), color);
	}

	/// `DiedHuman`. A human that died on its own reads as a death, one shot by another
	/// reads as a kill, and a kill on the shooter's own team is flagged as a team kill.
	pub fn died_human(
		&mut self,
		shot_human_id: i32,
		hit_human_id: i32,
		shot_human_team_id: i32,
		hit_human_team_id: i32,
		player_team_id: i32,
	) {
		let color = if player_team_id == hit_human_team_id {
			D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0)
		} else {
			D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
		};

		let text = if shot_human_id == -1 || shot_human_id == hit_human_id {
			format!("human[{hit_human_id}] died")
		} else {
			let mut text = format!("human[{shot_human_id}] killed human[{hit_human_id}]");
			if shot_human_team_id == hit_human_team_id {
				text.push_str(" (TK)");
			}
			text
		};

		self.add_text_log(MAX_OBJECTMANAGER_LOGCNT, text.as_bytes(), color);
	}

	/// `ReviveHuman`.
	pub fn revive_human(&mut self, human_id: i32, team_id: i32, player_team_id: i32) {
		let color = if player_team_id == team_id {
			D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
		} else {
			D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0)
		};

		let text = format!("Revived human[{human_id}]");
		self.add_text_log(MAX_OBJECTMANAGER_LOGCNT, text.as_bytes(), color);
	}

	/// `AddSmallObject`.
	pub fn add_small_object(&mut self, obj_id: i32) {
		let text = format!("Added SmallObject[{obj_id}]");
		self.add_text_log(
			MAX_OBJECTMANAGER_LOGCNT,
			text.as_bytes(),
			D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0),
		);
	}

	/// `BreakSmallObject`. The upstream spells it "Breaked", and the overlay is compared
	/// byte for byte, so it stays.
	pub fn break_small_object(&mut self, obj_id: i32) {
		let text = format!("Breaked SmallObject[{obj_id}]");
		self.add_text_log(
			MAX_OBJECTMANAGER_LOGCNT,
			text.as_bytes(),
			D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0),
		);
	}

	/// `AddTextLog`. Writes to the first free line, or shifts every line up one and writes
	/// to the last. Returns whether a shift happened.
	fn add_text_log(&mut self, cnt: i32, text: &[u8], color: u32) -> bool {
		for i in 0..MAX_OBJECTMANAGER_LOGLEN {
			if self.text_cnt[i] == -1 {
				self.text_cnt[i] = cnt;
				set_log_line(&mut self.text_str[i], text);
				self.text_color[i] = color;
				return false;
			}
		}

		for i in 0..MAX_OBJECTMANAGER_LOGLEN - 1 {
			self.text_cnt[i] = self.text_cnt[i + 1];
			self.text_str[i] = self.text_str[i + 1];
			self.text_color[i] = self.text_color[i + 1];
		}

		let last = MAX_OBJECTMANAGER_LOGLEN - 1;
		self.text_cnt[last] = cnt;
		set_log_line(&mut self.text_str[last], text);
		self.text_color[last] = color;
		true
	}

	/// `Process`: count each line down and drop the ones that reach zero.
	pub fn process(&mut self) {
		for i in 0..MAX_OBJECTMANAGER_LOGLEN {
			if self.text_cnt[i] == 0 {
				self.text_cnt[i] = -1;
				self.text_str[i] = [0; MAX_OBJECTMANAGER_LOGLINES];
				self.text_color[i] = 0;
			} else if self.text_cnt[i] > 0 {
				self.text_cnt[i] -= 1;
			}
		}

		for i in 0..MAX_OBJECTMANAGER_LOGLEN - 1 {
			if self.text_cnt[i] == 0 {
				for j in i..MAX_OBJECTMANAGER_LOGLEN - 1 {
					self.text_cnt[j] = self.text_cnt[j + 1];
					self.text_str[j] = self.text_str[j + 1];
					self.text_color[j] = self.text_color[j + 1];
				}

				let last = MAX_OBJECTMANAGER_LOGLEN - 1;
				self.text_cnt[last] = -1;
				self.text_str[last] = [0; MAX_OBJECTMANAGER_LOGLINES];
				self.text_color[last] = 0;
			}
		}
	}

	/// `Render`: drawn bottom line first, so the newest line ends up above the older ones.
	pub fn render(&mut self, d3dg: &mut D3DGraphics, x: i32, y: i32) {
		if !ENABLE_DEBUGCONSOLE {
			return;
		}

		let mut y = y;
		for i in (0..MAX_OBJECTMANAGER_LOGLEN).rev() {
			if self.text_cnt[i] >= 0 {
				let text = cstr(&self.text_str[i]);
				let shadow = D3DGraphics::get_color_code(0.1, 0.1, 0.1, 1.0);
				d3dg.draw_2d_texture_debug_font_text(x + 3, y + 3, text, shadow);
				d3dg.draw_2d_texture_debug_font_text(x + 2, y + 2, text, self.text_color[i]);
			}
			y += 20;
		}
	}
}

impl Default for ObjectManagerLog {
	fn default() -> Self {
		Self::new()
	}
}

/// `strcpy` into a fixed line: the text, then a terminator, with the tail left alone.
fn set_log_line(dst: &mut [u8; MAX_OBJECTMANAGER_LOGLINES], text: &[u8]) {
	let len = text.len().min(MAX_OBJECTMANAGER_LOGLINES - 1);
	dst[..len].copy_from_slice(&text[..len]);
	dst[len] = 0;
}

// ---------------------------------------------------------------------------
// ObjectManager
// ---------------------------------------------------------------------------

/// `ObjectManager`.
pub struct ObjectManager {
	/// Humans.
	human_index: Vec<Human>,
	/// Weapons, both those lying in the world and those a human holds.
	weapon_index: Vec<Weapon>,
	/// Small objects.
	small_object_index: Vec<SmallObject>,
	/// Bullets.
	bullet_index: Vec<Bullet>,
	/// Grenades.
	grenade_index: Vec<Grenade>,
	/// Effects.
	effect_index: Vec<Effect>,

	/// Frame count, which is what decides which weapons are checked for a pickup.
	framecnt: u32,

	/// Number of hits, per human.
	human_ontarget: [f32; MAX_HUMAN],
	/// Number of enemies defeated, per human.
	human_kill: [i32; MAX_HUMAN],
	/// Number of enemy head hits, per human.
	human_headshot: [i32; MAX_HUMAN],
	/// Firing flag, for the muzzle flash, per human.
	human_shot_flag: [bool; MAX_HUMAN],
	/// Shell casing count when firing, per human.
	human_yakkyou_cnt: [i32; MAX_HUMAN],
	/// The humans each bullet has already hit.
	bullet_obj_human_index: Vec<BulletObjectHumanIndex>,

	/// Additional collision.
	add_collision_flag: bool,
	/// Whether friendly fire is enabled, per human.
	friendly_fire: [bool; MAX_HUMAN],
	/// The data number of the human the player controls.
	player_human_id: i32,
	/// Human texture refresh flag, for the fake texture humans (mystery humans).
	human_frame_texture_refresh: [bool; MAX_HUMAN],

	/// The game setting values.
	param: ParameterInfo,
	/// The object log.
	object_log: ObjectManagerLog,
}

impl ObjectManager {
	/// Constructor.
	///
	/// Every pool is built here and never grows again, which is what the upstream's
	/// `new human[MAX_HUMAN]` and friends amount to. `ParameterInfo` is a zero sized handle
	/// to constant tables, so there is nothing to register it with: the pool members are
	/// built with it.
	pub fn new() -> Self {
		let param = ParameterInfo::new();

		let mut human_index = Vec::with_capacity(MAX_HUMAN);
		for _ in 0..MAX_HUMAN {
			human_index.push(Human::new(param, 0.0, 0.0, 0.0, 0.0, 0, -1, 0, 0, false));
		}

		let mut weapon_index = Vec::with_capacity(MAX_WEAPON);
		for _ in 0..MAX_WEAPON {
			weapon_index.push(Weapon::new(param, 0.0, 0.0, 0.0, 0.0, 0, 0, false));
		}

		let mut small_object_index = Vec::with_capacity(MAX_SMALLOBJECT);
		for _ in 0..MAX_SMALLOBJECT {
			small_object_index.push(SmallObject::new(param, 0.0, 0.0, 0.0, 0.0, 0, 0, false));
		}

		let mut bullet_index = Vec::with_capacity(MAX_BULLET);
		for _ in 0..MAX_BULLET {
			bullet_index.push(Bullet::new(param, -1, -1));
		}

		let mut grenade_index = Vec::with_capacity(MAX_GRENADE);
		for _ in 0..MAX_GRENADE {
			grenade_index.push(Grenade::new(param, -1, -1));
		}

		let mut effect_index = Vec::with_capacity(MAX_EFFECT);
		for _ in 0..MAX_EFFECT {
			effect_index.push(Effect::new(param, 0.0, 0.0, 0.0, 1.0, 0.0, -1, 0));
		}

		ObjectManager {
			human_index,
			weapon_index,
			small_object_index,
			bullet_index,
			grenade_index,
			effect_index,

			framecnt: 0,

			human_ontarget: [0.0; MAX_HUMAN],
			human_kill: [0; MAX_HUMAN],
			human_headshot: [0; MAX_HUMAN],
			human_shot_flag: [false; MAX_HUMAN],
			human_yakkyou_cnt: [-1; MAX_HUMAN],
			bullet_obj_human_index: (0..MAX_BULLET).map(|_| BulletObjectHumanIndex::new()).collect(),

			add_collision_flag: false,
			friendly_fire: [false; MAX_HUMAN],
			player_human_id: 0,
			human_frame_texture_refresh: [false; MAX_HUMAN],

			param,
			object_log: ObjectManagerLog::new(),
		}
	}

	/// `SetClass`, reduced to the part the port still needs.
	///
	/// Upstream this stored eight borrowed classes and pushed the setting class into every
	/// pool member. Neither survives: the pool members are built with the setting class and
	/// the classes are method parameters now. What is left is the pair of models a bullet
	/// and a grenade draw with, which come out of the loaded map and so have to be handed
	/// to pools that were built before it.
	pub fn set_class(&mut self, resource: &ResourceManager) {
		self.apply_bullet_model(resource);
	}

	/// The bullet and grenade model and texture, applied to both pools.
	fn apply_bullet_model(&mut self, resource: &ResourceManager) {
		if let (Some((model, texture)), Some(data)) = (
			resource.get_bullet_model_texture(0),
			self.param.get_bullet(0),
		) {
			for bullet in self.bullet_index.iter_mut() {
				bullet.object.set_model(model, data.size);
				bullet.object.set_texture(texture);
			}
		}

		if let (Some((model, texture)), Some(data)) = (
			resource.get_bullet_model_texture(1),
			self.param.get_bullet(1),
		) {
			for grenade in self.grenade_index.iter_mut() {
				grenade.bullet.object.set_model(model, data.size);
				grenade.bullet.object.set_texture(texture);
			}
		}
	}

	// -- adding ------------------------------------------------------------

	/// `AddHumanIndex` from point data.
	///
	/// The private half of the upstream overload pair, and the one [`Self::load_point_data`]
	/// uses. The game loop places humans at coordinates and calls [`Self::add_human_index`].
	///
	/// `infodata` is the human info point that describes this human, and it is what names
	/// the type, the team and the texture. `data.p1` says whether the human gets a second
	/// weapon, `data.p4` is the point identifier.
	///
	/// Returns the data number, or -1.
	pub fn add_human_index_data(
		&mut self,
		data: Pointdata,
		infodata: Pointdata,
		random_flag: bool,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> i32 {
		let human_param = self.param.get_human(infodata.p2 as i32);

		// If an invalid human type number is specified, the so called mystery human
		// appears: map texture 0 for a texture, and 0 health, which is instant death.
		// Upstream this guard is an `#ifndef ENABLE_BUG_HUMANWEAPON`, and the constant is
		// on, so the mystery human is reachable and the branch below is the live one.
		if !ENABLE_BUG_HUMANWEAPON && human_param.is_none() {
			return -1;
		}

		let (upmodel, armmodel, legmodel, walkmodel, runmodel) = resource.get_human_model();

		let mut px = data.x;
		let py = data.y;
		let mut pz = data.z;
		let mut rx = data.r;
		if random_flag {
			px += 0.001 * get_rand(100 * 2) as f32 - 0.1;
			pz += 0.001 * get_rand(100 * 2) as f32 - 0.1;
			rx += degree_to_radian(0.01) * get_rand(80 * 2) as f32 - degree_to_radian(0.8);
		}

		let mut human_index_id = -1;
		for j in 0..MAX_HUMAN {
			if !self.human_index[j].enable_flag() {
				self.human_index[j].set_pos_data(px, py, pz, rx);
				self.human_index[j]
					.set_param_data(infodata.p2 as i32, data.id, data.p4, infodata.p3 as i32, true);

				if let Some(human_param) = human_param {
					let id = resource.get_human_texture(infodata.p2 as i32);
					if id == -1 {
						// The texture could not be obtained, so it is retried every frame
						// from `Render`.
						self.human_frame_texture_refresh[j] = true;
					}

					self.human_index[j].set_texture(id);
					self.human_index[j].set_model(
						upmodel[human_param.model as usize],
						armmodel,
						legmodel,
						walkmodel,
						runmodel,
					);
				} else {
					// The map texture and the first upper body model, which is what the
					// bug flag makes an unknown human type draw with.
					self.human_index[j].set_texture(d3dg.map_texture_id(0));
					self.human_index[j].set_model(upmodel[0], armmodel, legmodel, walkmodel, runmodel);
				}

				self.human_index[j].set_enable_flag(true);
				human_index_id = j as i32;
				break;
			}
		}

		if human_index_id == -1 {
			return -1;
		}

		if let Some(human_param) = human_param {
			// Weapon B, as a virtual weapon.
			let mut weapon = [None; TOTAL_HAVEWEAPON];
			let lnbslist = [0; TOTAL_HAVEWEAPON];
			let nbslist = [0; TOTAL_HAVEWEAPON];

			let mut weapon_index_id = self.add_visual_weapon_index(human_param.weapon[0], true, d3dg, resource);
			if weapon_index_id != -1 {
				weapon[0] = Some(weapon_index_id as usize);
			}

			// Weapon A, only for the point type that carries two.
			if data.p1 == POINT_P1TYPE_HUMAN {
				weapon_index_id = self.add_visual_weapon_index(human_param.weapon[1], true, d3dg, resource);
				if weapon_index_id != -1 {
					weapon[1] = Some(weapon_index_id as usize);
				}
			}

			self.human_index[human_index_id as usize].set_weapon(
				weapon,
				lnbslist,
				nbslist,
				&mut self.weapon_index,
			);
		}

		self.init_human_shot_counters(human_index_id);

		// If this is the player, record the number.
		if (data.p1 == POINT_P1TYPE_HUMAN || data.p1 == POINT_P1TYPE_HUMAN2) && data.p4 == 0 {
			self.player_human_id = human_index_id;
		}

		human_index_id
	}

	/// `AddHumanIndex` at coordinates, the public half of the upstream overload pair.
	///
	/// The game loop, the menu and the mission editor place humans with this one, and it
	/// takes the weapon type numbers directly instead of reading them out of a human info
	/// point.
	pub fn add_human_index(
		&mut self,
		px: f32,
		py: f32,
		pz: f32,
		rx: f32,
		character_id: i32,
		team_id: i32,
		weapon_id: [i32; TOTAL_HAVEWEAPON],
		param4: i16,
		random_flag: bool,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> i32 {
		let human_param = self.param.get_human(character_id);

		if !ENABLE_BUG_HUMANWEAPON && human_param.is_none() {
			return -1;
		}

		let (upmodel, armmodel, legmodel, walkmodel, runmodel) = resource.get_human_model();

		let mut px = px;
		let mut pz = pz;
		let mut rx = rx;
		if random_flag {
			px += 0.001 * get_rand(100 * 2) as f32 - 0.1;
			pz += 0.001 * get_rand(100 * 2) as f32 - 0.1;
			rx += degree_to_radian(0.01) * get_rand(80 * 2) as f32 - degree_to_radian(0.8);
		}

		let mut human_index_id = -1;
		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				self.human_index[i].set_pos_data(px, py, pz, rx);
				self.human_index[i].set_param_data(character_id, -1, param4, team_id, true);

				if let Some(human_param) = human_param {
					let id = resource.get_human_texture(character_id);
					if id == -1 {
						self.human_frame_texture_refresh[i] = true;
					}

					self.human_index[i].set_texture(id);
					self.human_index[i].set_model(
						upmodel[human_param.model as usize],
						armmodel,
						legmodel,
						walkmodel,
						runmodel,
					);
				} else {
					self.human_index[i].set_texture(d3dg.map_texture_id(0));
					self.human_index[i].set_model(upmodel[0], armmodel, legmodel, walkmodel, runmodel);
				}

				self.human_index[i].set_enable_flag(true);
				human_index_id = i as i32;
				break;
			}
		}

		if human_index_id == -1 {
			return -1;
		}

		if human_param.is_some() {
			let mut weapon = [None; TOTAL_HAVEWEAPON];

			for (i, id) in weapon_id.iter().enumerate() {
				let weapon_index_id = self.add_visual_weapon_index(*id, true, d3dg, resource);
				if weapon_index_id != -1 {
					weapon[i] = Some(weapon_index_id as usize);
				}
			}

			self.human_index[human_index_id as usize].set_weapon(
				weapon,
				[0; TOTAL_HAVEWEAPON],
				[0; TOTAL_HAVEWEAPON],
				&mut self.weapon_index,
			);
		}

		let player_team_id = self.player_team_id();
		self.object_log
			.add_human(human_index_id, team_id, player_team_id);

		human_index_id
	}

	/// The firing flag and shell casing counter a fresh human starts with.
	fn init_human_shot_counters(&mut self, human_index_id: i32) {
		self.human_shot_flag[human_index_id as usize] = false;
		self.human_yakkyou_cnt[human_index_id as usize] = -1;
	}

	/// `AddWeaponIndex` from point data.
	///
	/// The private half of the upstream overload pair, used by [`Self::load_point_data`].
	/// The game loop places weapons by adding a human that carries them, or by
	/// [`Self::add_visual_weapon_index`].
	///
	/// Returns the data number, or -1.
	pub fn add_weapon_index(&mut self, data: Pointdata, d3dg: &D3DGraphics, resource: &ResourceManager) -> i32 {
		// Temporarily register the weapon type number and the bullet count.
		let mut weapon_id = data.p2 as i32;
		let mut nbs = point_param_to_int(data.p3) as i32;

		// A random weapon picks one of its two types, and starts fully loaded either way.
		if data.p1 == POINT_P1TYPE_RAND_WEAPON {
			if get_rand(2) == 0 {
				let Some(weapon_param) = self.param.get_weapon(data.p2 as i32) else {
					return -1;
				};
				nbs = weapon_param.nbsmax * TOTAL_WEAPON_AUTOBULLET;
			} else {
				let Some(weapon_param) = self.param.get_weapon(data.p3 as i32) else {
					return -1;
				};
				weapon_id = data.p3 as i32;
				nbs = weapon_param.nbsmax * TOTAL_WEAPON_AUTOBULLET;
			}
		}

		if weapon_id == ID_WEAPON_NONE {
			return -1;
		}

		let Some((model, texture)) = resource.get_weapon_model_texture(d3dg, weapon_id) else {
			return -1;
		};
		let Some(weapon_param) = self.param.get_weapon(weapon_id) else {
			return -1;
		};

		for i in 0..MAX_WEAPON {
			if !self.weapon_index[i].object.enable_flag() {
				self.weapon_index[i].set_pos_data(data.x, data.y, data.z, data.r);
				self.weapon_index[i].set_param_data(weapon_id, 0, nbs, true);
				self.weapon_index[i].object.set_model(model, weapon_param.size);
				self.weapon_index[i].object.set_texture(texture);
				self.weapon_index[i].object.set_enable_flag(true);
				self.weapon_index[i].run_reload();
				return i as i32;
			}
		}

		-1
	}

	/// `AddVisualWeaponIndex`: a weapon a human holds from the start.
	///
	/// It is a weapon object like any other, so a human holds an index into the same pool
	/// that the weapons lying on the map live in. `load_bullet` off gives it no rounds.
	pub fn add_visual_weapon_index(
		&mut self,
		weapon_id: i32,
		load_bullet: bool,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> i32 {
		if weapon_id == ID_WEAPON_NONE {
			return -1;
		}

		let Some(weapon_param) = self.param.get_weapon(weapon_id) else {
			return -1;
		};
		let Some((model, texture)) = resource.get_weapon_model_texture(d3dg, weapon_id) else {
			return -1;
		};

		for i in 0..MAX_WEAPON {
			if !self.weapon_index[i].object.enable_flag() {
				self.weapon_index[i].set_pos_data(0.0, 0.0, 0.0, 0.0);
				if load_bullet {
					self.weapon_index[i].set_param_data(
						weapon_id,
						weapon_param.nbsmax,
						weapon_param.nbsmax * TOTAL_WEAPON_AUTOBULLET,
						true,
					);
				} else {
					self.weapon_index[i].set_param_data(weapon_id, 0, 0, true);
				}
				self.weapon_index[i].object.set_model(model, weapon_param.size);
				self.weapon_index[i].object.set_texture(texture);
				self.weapon_index[i].object.set_enable_flag(true);
				return i as i32;
			}
		}

		-1
	}

	/// `AddSmallObjectIndex` from point data.
	///
	/// The private half of the upstream overload pair, used by [`Self::load_point_data`].
	/// `data.p3` non zero asks the object to be dropped onto the block under it.
	pub fn add_small_object_index_data(
		&mut self,
		data: Pointdata,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
		resource: &ResourceManager,
	) -> i32 {
		let Some((model, texture)) = resource.get_small_object_model_texture(data.p2 as i32) else {
			return -1;
		};

		for i in 0..MAX_SMALLOBJECT {
			if !self.small_object_index[i].object.enable_flag() {
				self.small_object_index[i].object.set_pos_data(data.x, data.y, data.z, data.r);
				self.small_object_index[i]
					.set_param_data(data.p2 as i32, data.p4, true, mif);
				self.small_object_index[i].object.set_model(model, SMALLOBJECT_SCALE);
				self.small_object_index[i].object.set_texture(texture);
				self.small_object_index[i].object.set_enable_flag(true);

				if data.p3 != 0 {
					self.small_object_index[i].collision_map(coll, block_data_if, mif);
				}

				return i as i32;
			}
		}

		-1
	}

	/// `AddSmallObjectIndex` at coordinates, the public half of the upstream overload pair.
	///
	/// Used during the game, for a prop a mission script places.
	pub fn add_small_object_index(
		&mut self,
		px: f32,
		py: f32,
		pz: f32,
		rx: f32,
		param_id: i32,
		map_coll: bool,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
		resource: &ResourceManager,
	) -> i32 {
		let Some((model, texture)) = resource.get_small_object_model_texture(param_id) else {
			return -1;
		};

		for i in 0..MAX_SMALLOBJECT {
			if !self.small_object_index[i].object.enable_flag() {
				self.small_object_index[i].object.set_pos_data(px, py, pz, rx);
				self.small_object_index[i].set_param_data(param_id, 0, true, mif);
				self.small_object_index[i].object.set_model(model, SMALLOBJECT_SCALE);
				self.small_object_index[i].object.set_texture(texture);
				self.small_object_index[i].object.set_enable_flag(true);

				if map_coll {
					self.small_object_index[i].collision_map(coll, block_data_if, mif);
				}

				self.object_log.add_small_object(i as i32);

				return i as i32;
			}
		}

		-1
	}

	/// `AddEffect`: a billboard with a lifetime.
	///
	/// Fails with -1 when every effect slot is taken, which the callers of the blood and
	/// smoke effects ignore.
	#[allow(clippy::too_many_arguments)]
	pub fn add_effect(
		&mut self,
		pos_x: f32,
		pos_y: f32,
		pos_z: f32,
		move_x: f32,
		move_y: f32,
		move_z: f32,
		addmove_y: f32,
		rotation: f32,
		addrotation: f32,
		size: f32,
		addsize: f32,
		texture: i32,
		brightness: i32,
		addbrightness: i32,
		alpha: f32,
		addalpha: f32,
		settype: i32,
		count: i32,
	) -> i32 {
		for i in 0..MAX_EFFECT {
			if !self.effect_index[i].object.enable_flag() {
				self.effect_index[i].object.set_pos_data(pos_x, pos_y, pos_z, 0.0);
				self.effect_index[i].set_param_data(
					move_x,
					move_y,
					move_z,
					addmove_y,
					rotation,
					addrotation,
					size,
					addsize,
					count,
					settype,
				);
				self.effect_index[i].set_texture(texture, brightness, addbrightness, alpha, addalpha);
				self.effect_index[i].object.set_enable_flag(true);
				return i as i32;
			}
		}

		-1
	}

	/// `AddMapEffect`: an effect that sticks to a block face instead of facing the camera.
	///
	/// The face's normal gives the two angles that lay the board flat on it.
	#[allow(clippy::too_many_arguments)]
	pub fn add_map_effect(
		&mut self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		pos_x: f32,
		pos_y: f32,
		pos_z: f32,
		size: f32,
		rotation: f32,
		texture: i32,
		brightness: i32,
		addbrightness: i32,
		alpha: f32,
		addalpha: f32,
		count: i32,
	) -> i32 {
		let Some(data) = block_data_if.getdata(id) else {
			return -1;
		};
		let material = &data.material[face as usize];
		let (vx, vy, vz) = (material.vx, material.vy, material.vz);

		let rx = vz.atan2(vx) * -1.0 + PI;
		let ry = vy.atan2((vx * vx + vz * vz).sqrt()) * -1.0;

		for i in 0..MAX_EFFECT {
			if !self.effect_index[i].object.enable_flag() {
				self.effect_index[i].object.set_pos_data(pos_x, pos_y, pos_z, 0.0);
				self.effect_index[i].set_param_data(
					0.0,
					0.0,
					0.0,
					0.0,
					rotation,
					0.0,
					size,
					0.0,
					count,
					effect_type::NOBILLBOARD,
				);
				self.effect_index[i].set_rx_ry(rx, ry);
				self.effect_index[i].set_texture(texture, brightness, addbrightness, alpha, addalpha);
				self.effect_index[i].object.set_enable_flag(true);
				return i as i32;
			}
		}

		-1
	}

	// -- combat helpers ----------------------------------------------------

	/// `SetHumanBlood`: the spray of a hit, plus one drop per ten points of damage.
	///
	/// `flowing` is the upstream's `NotRobot`, so a robot does not bleed, and it also picks
	/// the effect type: a bleeding human's drops collide with the map so they land on it.
	pub fn set_human_blood(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		damage: i32,
		flowing: bool,
		resource: &ResourceManager,
		config: &Config,
	) {
		if !config.get_blood_flag() {
			return;
		}

		let addtype = if flowing {
			effect_type::COLLIDEMAP
		} else {
			effect_type::NORMAL
		};

		self.add_effect(
			x,
			y,
			z,
			0.0,
			0.0,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			10.0,
			0.2,
			resource.get_effect_blood_texture(),
			10,
			0,
			0.5,
			-0.04,
			addtype,
			(GAMEFPS * 0.4) as i32,
		);

		if flowing {
			for _ in 0..(damage / 10) {
				let rx = degree_to_radian(10.0) * get_rand(36) as f32;
				self.add_effect(
					x + rx.cos() * 1.0,
					y + (get_rand(20) - 10) as f32 / 10.0,
					z + rx.sin() * 1.0,
					rx.cos() * 0.5,
					get_rand(5) as f32 * 0.1 + 0.5,
					rx.sin() * 0.5,
					-0.09,
					degree_to_radian(10.0) * get_rand(36) as f32,
					0.0,
					get_rand(40) as f32 * 0.1 + 0.1,
					0.1,
					resource.get_effect_blood_texture(),
					10,
					0,
					1.0,
					0.0,
					addtype,
					(GAMEFPS * 0.5) as i32,
				);
			}
		}
	}

	/// `CollideHuman`: the cylinder test that keeps two humans from standing in each other.
	///
	/// Takes two *indices* rather than two `&mut Human`. The two are members of one pool,
	/// so `split_at_mut` at the higher index is what makes both borrows disjoint, and the
	/// caller only ever passes `a < b`, which makes the split point the higher one. The
	/// upstream pushes both apart, so it needs both mutably and this is the smallest way
	/// to say that.
	pub fn collide_human(&mut self, a: usize, b: usize) -> bool {
		let (a, b) = if a < b { (a, b) } else { (b, a) };

		let (left, right) = self.human_index.split_at_mut(b);
		let Some(in_human_a) = left.get_mut(a) else {
			return false;
		};
		let Some(in_human_b) = right.first_mut() else {
			return false;
		};

		// Not initialised, or dead, is not tested.
		if !in_human_a.enable_flag() || !in_human_b.enable_flag() {
			return false;
		}
		if in_human_a.hp() <= 0 || in_human_b.hp() <= 0 {
			return false;
		}

		let (h1_x, h1_y, h1_z, _) = in_human_a.pos_data();
		let (h2_x, h2_y, h2_z, _) = in_human_b.pos_data();

		if let Some((angle, length)) = collide_cylinder(
			h1_x,
			h1_y,
			h1_z,
			HUMAN_HUMANCOLLISION_R,
			HUMAN_HEIGHT - 0.5,
			h2_x,
			h2_y,
			h2_z,
			HUMAN_HUMANCOLLISION_R,
			HUMAN_HEIGHT - 0.5,
		) {
			in_human_a.add_pos_order(angle, 0.0, length / 2.0);
			in_human_b.add_pos_order(angle + PI, 0.0, length / 2.0);
			return true;
		}

		false
	}

	/// `CollideBullet`: the bullet's whole lifetime, one step of
	/// [`BULLET_SPEEDSCALE`] at a time.
	///
	/// The bullet's state is read out into locals first and written back at the end. That
	/// is what the upstream does too (`SetParamData` at the bottom), and it is what keeps
	/// the bullet's slot and the humans it hits from being borrowed at the same time.
	///
	/// Returns whether anything was hit.
	pub fn collide_bullet(
		&mut self,
		object_id: usize,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
		resource: &ResourceManager,
		sound: &mut SoundManager,
		config: &Config,
	) -> bool {
		let Some(in_bullet) = self.bullet_index.get(object_id) else {
			return false;
		};
		if !in_bullet.object.enable_flag() {
			return false;
		}

		let (bx, by, bz, brx, bry) = in_bullet.pos_data();
		let (attacks, penetration, speed, teamid, humanid, ontargetcnt) = in_bullet.param_data();
		let mut attacks = attacks;
		let mut penetration = penetration;

		let vx = brx.cos() * bry.cos();
		let vy = bry.sin();
		let vz = brx.sin() * bry.cos();

		// 0: no block, 1: a block is on the path but not yet judged, 2: judged.
		let mut mapflag = 0;
		let mut map_dist = 0.0;
		let mut collide_flag = false;

		if let Some((_, _, dist)) =
			coll.check_all_block_intersect_ray(block_data_if, bx, by, bz, vx, vy, vz, speed)
		{
			mapflag = 1;
			map_dist = dist;
		}

		// The resolution of the path, one step of BULLET_SPEEDSCALE.
		let maxcnt = (speed / BULLET_SPEEDSCALE).round() as i32;

		for cnt in 0..maxcnt {
			// If the penetration power does not remain, the bullet is done.
			if penetration < 0 {
				self.bullet_index[object_id].object.set_enable_flag(false);
				mapflag = 0;
				break;
			}

			let bvx = bx + vx * BULLET_SPEEDSCALE * (cnt + 1) as f32;
			let bvy = by + vy * BULLET_SPEEDSCALE * (cnt + 1) as f32;
			let bvz = bz + vz * BULLET_SPEEDSCALE * (cnt + 1) as f32;

			for i in 0..MAX_HUMAN {
				// A bullet never hits the human that fired it.
				if i as i32 == humanid {
					continue;
				}
				if !self.human_index[i].enable_flag() {
					continue;
				}
				if self.human_index[i].hp() <= 0 {
					continue;
				}
				// A human the bullet already passed through is not hit again.
				if self.bullet_obj_human_index[object_id].index_flag(i as i32) {
					continue;
				}

				let (ox, oy, oz, _) = self.human_index[i].pos_data();
				let h_teamid = self.human_index[i].get_param_data().3;

				// Friendly fire off means an ally is not hit at all.
				if !self.friendly_fire[i] && h_teamid == teamid {
					continue;
				}

				// Head.
				if collide_cylinder_inside(
					ox,
					oy + HUMAN_BULLETCOLLISION_LEG_H + HUMAN_BULLETCOLLISION_UP_H,
					oz,
					HUMAN_BULLETCOLLISION_HEAD_R,
					HUMAN_BULLETCOLLISION_HEAD_H,
					bvx,
					bvy,
					bvz,
				) {
					self.hit_bullet_human(
						i,
						0,
						bvx,
						bvy,
						bvz,
						brx,
						attacks,
						humanid,
						ontargetcnt,
						resource,
						sound,
						config,
					);
					self.reset_bullet_human_index(object_id, i);

					attacks = (attacks as f32 * 0.5) as i32;
					penetration -= 1;
					collide_flag = true;
				}

				// Upper body.
				if collide_cylinder_inside(
					ox,
					oy + HUMAN_BULLETCOLLISION_LEG_H,
					oz,
					HUMAN_BULLETCOLLISION_UP_R,
					HUMAN_BULLETCOLLISION_UP_H,
					bvx,
					bvy,
					bvz,
				) {
					self.hit_bullet_human(
						i,
						1,
						bvx,
						bvy,
						bvz,
						brx,
						attacks,
						humanid,
						ontargetcnt,
						resource,
						sound,
						config,
					);
					self.reset_bullet_human_index(object_id, i);

					attacks = (attacks as f32 * 0.6) as i32;
					penetration -= 1;
					collide_flag = true;
				}

				// Legs.
				if collide_cylinder_inside(
					ox,
					oy,
					oz,
					HUMAN_BULLETCOLLISION_LEG_R,
					HUMAN_BULLETCOLLISION_LEG_H,
					bvx,
					bvy,
					bvz,
				) {
					self.hit_bullet_human(
						i,
						2,
						bvx,
						bvy,
						bvz,
						brx,
						attacks,
						humanid,
						ontargetcnt,
						resource,
						sound,
						config,
					);
					self.reset_bullet_human_index(object_id, i);

					attacks = (attacks as f32 * 0.7) as i32;
					penetration -= 1;
					collide_flag = true;
				}
			}

			// The penetration power can also run out during the human test.
			if penetration < 0 {
				self.bullet_index[object_id].object.set_enable_flag(false);
				mapflag = 0;
				break;
			}

			for i in 0..MAX_SMALLOBJECT {
				if !self.small_object_index[i].object.enable_flag() {
					continue;
				}

				let (ox, oy, oz, _) = self.small_object_index[i].object.pos_data();
				let (id, _) = self.small_object_index[i].param_data();

				// Additional small objects carry their size in the mission file, the
				// built in ones in the setting table.
				let decide = if (TOTAL_PARAMETERINFO_SMALLOBJECT as i32 <= id)
					&& (id <= (TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT - 1) as i32)
				{
					mif.map_or(0, |mif| {
						mif.get_add_smallobject_decide(id - TOTAL_PARAMETERINFO_SMALLOBJECT as i32)
					}) as f32
						* SMALLOBJECT_COLLISIONSCALE
				} else {
					self.param
						.get_small_object(id)
						.map_or(0, |param| param.decide) as f32
						* SMALLOBJECT_COLLISIONSCALE
				};

				if collide_sphere_inside(ox, oy, oz, decide, bvx, bvy, bvz) {
					let attack2 = (attacks as f32 * 0.25).floor() as i32;
					self.hit_bullet_small_object(i, bvx, bvy, bvz, attack2, teamid, mif, resource, sound);

					attacks = (attacks as f32 * 0.7).floor() as i32;
					collide_flag = true;
				}
			}

			if mapflag > 0 && coll.check_all_block_inside(block_data_if, bvx, bvy, bvz) {
				self.hit_bullet_map(
					bx + vx * (map_dist - 1.0),
					by + vy * (map_dist - 1.0),
					bz + vz * (map_dist - 1.0),
					teamid,
					resource,
					sound,
				);

				penetration -= 1;
				if penetration >= 0 {
					attacks = (attacks as f32 * 0.6) as i32;
				}

				mapflag = 2;
				collide_flag = true;
			}
		}

		// The path had a block on it but the collision test never landed inside one, which
		// is what a thin block looks like from the side.
		if mapflag == 1 {
			self.hit_bullet_map(
				bx + vx * (map_dist - 1.0),
				by + vy * (map_dist - 1.0),
				bz + vz * (map_dist - 1.0),
				teamid,
				resource,
				sound,
			);

			if penetration > 0 {
				attacks = (attacks as f32 * 0.75) as i32;
			} else {
				attacks = (attacks as f32 * 0.55) as i32;
			}

			collide_flag = true;
		}

		self.bullet_index[object_id].set_param_data(
			attacks,
			penetration,
			speed,
			teamid,
			humanid,
			ontargetcnt,
			false,
		);

		collide_flag
	}

	/// The upstream list is reset on every hit and only the last human hit is recorded,
	/// which is what keeps a bullet from hitting the same body twice.
	fn reset_bullet_human_index(&mut self, object_id: usize, hit_human_id: usize) {
		self.bullet_obj_human_index[object_id].init();
		self.bullet_obj_human_index[object_id].set_index_flag(hit_human_id as i32);
	}

	/// `HitBulletMap`: the smoke puff and the sound of a bullet into a wall.
	pub fn hit_bullet_map(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		team_id: i32,
		resource: &ResourceManager,
		sound: &mut SoundManager,
	) {
		self.add_effect(
			x,
			y,
			z,
			0.0,
			0.05,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			3.0,
			0.24,
			resource.get_effect_smoke_texture(),
			20,
			0,
			0.63,
			-0.03,
			effect_type::NORMAL,
			20,
		);

		sound.hit_map(x, y, z, team_id);
	}

	/// `HitBulletHuman`: the damage, the spray, the sound, the score and the log.
	///
	/// `hit_id` is the location: head 0, upper body 1, legs 2. The damage dealt is the
	/// human's own (`HitBulletHead` and friends apply the multiplier), and `damage` here
	/// is the same arithmetic done again for the blood effect, which is the only thing the
	/// manager's copy of it is used for.
	#[allow(clippy::too_many_arguments)]
	pub fn hit_bullet_human(
		&mut self,
		hit_human_id: usize,
		hit_id: i32,
		x: f32,
		y: f32,
		z: f32,
		brx: f32,
		attacks: i32,
		shot_human_id: i32,
		ontargetcnt: f32,
		resource: &ResourceManager,
		sound: &mut SoundManager,
		config: &Config,
	) {
		let Some(hit_human) = self.human_index.get(hit_human_id) else {
			return;
		};
		if !hit_human.enable_flag() || hit_human.dead_flag() {
			return;
		}

		let shot_human_team_id = self
			.human_index
			.get(shot_human_id.max(0) as usize)
			.map_or(0, |human| human.get_param_data().3);
		let hit_human_team_id = hit_human.get_param_data().3;
		let hp_old = hit_human.hp();

		match hit_id {
			0 => self.human_index[hit_human_id].hit_bullet_head(attacks),
			1 => self.human_index[hit_human_id].hit_bullet_up(attacks),
			_ => self.human_index[hit_human_id].hit_bullet_leg(attacks),
		}
		self.human_index[hit_human_id].add_pos_order(brx, 0.0, 1.0);

		// `ENABLE_BUG_TEAMID` is on: when both team numbers are negative, the larger one
		// belongs to the shooter and the hit direction is not set, which is the upstream's
		// way of handling the team numbers a mission file can contain.
		let mut flag = true;
		if ENABLE_BUG_TEAMID && hit_human_team_id < 0 && shot_human_team_id < 0 {
			if hit_human_team_id < shot_human_team_id {
				flag = false;
			}
		}
		if flag {
			self.human_index[hit_human_id].set_hit_flag(brx * -1.0 - PI / 2.0);
		}

		// Whether it is a robot, which decides whether it bleeds.
		let param_id = self.human_index[hit_human_id].get_param_data().0;
		let not_robot = self
			.param
			.get_human(param_id)
			.is_none_or(|param| param.r#type != 1);

		let damage = match hit_id {
			0 => (attacks as f32 * HUMAN_DAMAGE_HEAD) as i32,
			1 => (attacks as f32 * HUMAN_DAMAGE_UP) as i32,
			_ => (attacks as f32 * HUMAN_DAMAGE_LEG) as i32,
		};
		self.set_human_blood(x, y, z, damage, not_robot, resource, config);

		sound.hit_human(x, y, z, hit_id, shot_human_team_id);

		if 0 <= shot_human_id && shot_human_id < MAX_HUMAN as i32 {
			let shot = shot_human_id as usize;
			self.human_ontarget[shot] += ontargetcnt;
			if hit_id == 0 {
				self.human_headshot[shot] += 1;
			}
			if hp_old > 0 && self.human_index[hit_human_id].hp() <= 0 {
				// Alive before the damage and dead after it: this shot is the one that
				// killed.
				self.human_kill[shot] += 1;
			}
		}

		if hp_old > 0 && self.human_index[hit_human_id].hp() <= 0 {
			let player_team_id = self.player_team_id();
			self.object_log.died_human(
				shot_human_id,
				hit_human_id as i32,
				shot_human_team_id,
				hit_human_team_id,
				player_team_id,
			);
		}
	}

	/// `HitBulletSmallObject`: the damage and the two smoke puffs.
	pub fn hit_bullet_small_object(
		&mut self,
		hit_small_object_id: usize,
		x: f32,
		y: f32,
		z: f32,
		attacks: i32,
		team_id: i32,
		mif: Option<&MIFInterface>,
		resource: &ResourceManager,
		sound: &mut SoundManager,
	) {
		if !self.small_object_index[hit_small_object_id].object.enable_flag() {
			return;
		}

		// Nothing to damage.
		if self.small_object_index[hit_small_object_id].hp() <= 0 {
			return;
		}

		self.small_object_index[hit_small_object_id].hit_bullet(attacks, mif);

		let texture = resource.get_effect_smoke_texture();
		self.add_effect(
			x,
			y,
			z,
			0.0,
			0.0,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			1.0,
			0.1,
			texture,
			10,
			0,
			0.625,
			-0.047,
			effect_type::NORMAL,
			255,
		);
		self.add_effect(
			x,
			y,
			z,
			0.0,
			0.01 * (get_rand(10) - 5) as f32,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			1.0,
			0.1,
			texture,
			10,
			0,
			0.625,
			-0.047,
			effect_type::NORMAL,
			255,
		);

		let (id, _) = self.small_object_index[hit_small_object_id].param_data();
		sound.hit_small_object(x, y, z, id, team_id);

		if self.small_object_index[hit_small_object_id].hp() <= 0 {
			self.object_log.break_small_object(hit_small_object_id as i32);
		}
	}

	/// `GrenadeExplosion`: the blast, and everything it does to the humans and small
	/// objects around it.
	///
	/// The damage is a straight falloff from the centre to [`MAX_DAMAGE_GRENADE_DISTANCE`],
	/// computed twice per human: once at the feet and once at the head, and blocked
	/// separately by the map at each. The two are added.
	///
	/// Returns whether the blast hit anything.
	pub fn grenade_explosion(
		&mut self,
		grenade_id: usize,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
		resource: &ResourceManager,
		sound: &mut SoundManager,
		config: &Config,
	) -> bool {
		let Some(in_grenade) = self.grenade_index.get(grenade_id) else {
			return false;
		};

		// The grenade is read out first: everything below borrows the humans and the small
		// objects, which are different pools but the same `self`.
		let (gx, gy, gz, _, _) = in_grenade.pos_data();
		let (_, teamid, humanid, _) = in_grenade.param_data();

		let mut returnflag = false;

		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}
			if self.human_index[i].dead_flag() {
				continue;
			}

			let (hx, hy, hz, _) = self.human_index[i].pos_data();
			let mut x = hx - gx;
			let mut y = hy - gy;
			let mut z = hz - gz;
			let mut r = (x * x + y * y + z * z).sqrt();

			// A blast this far away cannot reach anyone.
			if r > MAX_DAMAGE_GRENADE_DISTANCE + HUMAN_HEIGHT {
				continue;
			}

			let mut total_damage = 0;

			// At the feet.
			y = hy + 2.0 - gy;
			r = (x * x + y * y + z * z).sqrt();
			// A block in the way blocks the blast.
			if coll
				.check_all_block_intersect_ray(block_data_if, gx, gy, gz, x / r, y / r, z / r, r)
				.is_none()
			{
				let damage =
					HUMAN_DAMAGE_GRENADE_LEG - (HUMAN_DAMAGE_GRENADE_LEG as f32 / MAX_DAMAGE_GRENADE_DISTANCE * r) as i32;
				if damage > 0 {
					total_damage += damage;
				}
			}

			// At the head.
			y = hy + (HUMAN_HEIGHT - 2.0) - gy;
			r = (x * x + y * y + z * z).sqrt();
			if coll
				.check_all_block_intersect_ray(block_data_if, gx, gy, gz, x / r, y / r, z / r, r)
				.is_none()
			{
				let damage = HUMAN_DAMAGE_GRENADE_HEAD
					- (HUMAN_DAMAGE_GRENADE_HEAD as f32 / MAX_DAMAGE_GRENADE_DISTANCE * r) as i32;
				if damage > 0 {
					total_damage += damage;
				}
			}

			if total_damage <= 0 {
				continue;
			}

			let hp_old = self.human_index[i].hp();
			self.human_index[i].hit_grenade_explosion(total_damage);
			let hit_human_team_id = self.human_index[i].get_param_data().3;

			if 0 <= humanid && humanid < MAX_HUMAN as i32 && self.human_index[i].hp() <= 0 {
				self.human_kill[humanid as usize] += 1;
			}

			self.set_human_blood(hx, hy + 15.0, hz, total_damage, false, resource, config);

			// The direction the blast throws the body in.
			x = gx - hx;
			y = gy - (hy + 1.0);
			z = gz - hz;

			let arx = z.atan2(x);
			let ary = if (y.atan2((x * x + z * z).sqrt())).sin() < 0.0 {
				// Flying upward: the vertical angle is computed.
				let y2 = gy - (hy + HUMAN_HEIGHT);
				y2.atan2((x * x + z * z).sqrt()) + PI
			} else {
				// Flying downward: the vertical angle is invalid, which is what keeps the
				// blast from sinking into the ground.
				0.0
			};

			let mut flag = true;
			if ENABLE_BUG_TEAMID && hit_human_team_id < 0 && teamid < 0 {
				if hit_human_team_id < teamid {
					flag = false;
				}
			}
			if flag {
				self.human_index[i].set_hit_flag(arx * -1.0 + PI / 2.0);
			}

			// Wind pressure, which falls off by the distance again.
			self.human_index[i].add_pos_order(
				arx,
				ary,
				2.2 / MAX_DAMAGE_GRENADE_DISTANCE
					* (MAX_DAMAGE_GRENADE_DISTANCE - (x * x + y * y + z * z).sqrt()),
			);

			if hp_old > 0 && self.human_index[i].hp() <= 0 {
				let player_team_id = self.player_team_id();
				self.object_log
					.died_human(humanid, i as i32, teamid, hit_human_team_id, player_team_id);
			}

			returnflag = true;
		}

		for i in 0..MAX_SMALLOBJECT {
			if !self.small_object_index[i].object.enable_flag() {
				continue;
			}

			let (sx, sy, sz, _) = self.small_object_index[i].object.pos_data();
			let x = sx - gx;
			let y = sy - gy;
			let z = sz - gz;
			let r = (x * x + y * y + z * z).sqrt();

			if r > MAX_DAMAGE_GRENADE_DISTANCE {
				continue;
			}

			if coll
				.check_all_block_intersect_ray(block_data_if, gx, gy, gz, x / r, y / r, z / r, r)
				.is_some()
			{
				continue;
			}

			let (id, _) = self.small_object_index[i].param_data();
			let damage = SMALLOBJECT_DAMAGE_GRENADE
				- (SMALLOBJECT_DAMAGE_GRENADE as f32 / MAX_DAMAGE_GRENADE_DISTANCE * r) as i32;
			self.small_object_index[i].hit_grenade_explosion(damage, mif);

			sound.hit_small_object(sx, sy, sz, id, teamid);

			if self.small_object_index[i].hp() <= 0 {
				self.object_log.break_small_object(i as i32);
			}

			returnflag = true;
		}

		// The flash.
		self.add_effect(
			gx,
			gy,
			gz,
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			0.0,
			40.0,
			0.0,
			resource.get_effect_mflash_texture(),
			0,
			0,
			0.63,
			0.0,
			effect_type::NORMAL,
			2,
		);

		// The smoke, in four layers.
		self.add_effect(
			gx,
			gy,
			gz,
			0.0,
			0.1,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			36.0,
			0.2,
			resource.get_effect_smoke_texture(),
			220,
			0,
			0.6,
			-0.01,
			effect_type::NORMAL,
			50,
		);
		self.add_effect(
			gx + (get_rand(7) as f32 / 10.0 - 0.3),
			gy + (get_rand(5) as f32 / 10.0 - 0.2),
			gz + (get_rand(7) as f32 / 10.0 - 0.3),
			get_rand(5) as f32 / 10.0 - 0.2,
			get_rand(3) as f32 / 10.0,
			get_rand(5) as f32 / 10.0 - 0.2,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			(get_rand(11) as f32 + 25.0) * 2.0,
			0.3,
			resource.get_effect_smoke_texture(),
			0,
			0,
			0.47,
			-0.004,
			effect_type::NORMAL,
			120,
		);
		self.add_effect(
			gx + (get_rand(13) as f32 / 10.0 - 0.6),
			gy + (get_rand(11) as f32 / 10.0 - 0.5),
			gz + (get_rand(13) as f32 / 10.0 - 0.6),
			get_rand(7) as f32 / 10.0 - 0.3,
			get_rand(7) as f32 / 10.0 - 0.3,
			get_rand(7) as f32 / 10.0 - 0.3,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			degree_to_radian(1.0),
			(get_rand(21) as f32 + 20.0) * 2.0,
			0.2,
			resource.get_effect_smoke_texture(),
			0,
			0,
			0.4,
			-0.004,
			effect_type::NORMAL,
			100,
		);
		self.add_effect(
			gx + (get_rand(13) as f32 / 10.0 - 0.6),
			gy + (get_rand(11) as f32 / 10.0 - 0.5),
			gz + (get_rand(13) as f32 / 10.0 - 0.6),
			get_rand(7) as f32 / 10.0 - 0.3,
			get_rand(7) as f32 / 10.0 - 0.3,
			get_rand(7) as f32 / 10.0 - 0.3,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			degree_to_radian(-1.0),
			(get_rand(21) as f32 + 20.0) * 2.0,
			0.2,
			resource.get_effect_smoke_texture(),
			0,
			0,
			0.4,
			-0.004,
			effect_type::NORMAL,
			100,
		);

		sound.grenade_explosion(gx, gy, gz, teamid);

		returnflag
	}

	/// `DeadEffect`: the smoke a destroyed robot gives off.
	///
	/// An ordinary human does not smoke when it falls, so this does nothing for one.
	pub fn dead_effect(&mut self, human_id: usize, resource: &ResourceManager) {
		let param_id = match self.human_index.get(human_id) {
			Some(human) => human.get_param_data().0,
			None => return,
		};

		let Some(data) = self.param.get_human(param_id) else {
			return;
		};
		if data.r#type != 1 {
			return;
		}

		let (hx, hy, hz, hrx) = match self.human_index.get(human_id) {
			Some(human) => human.pos_data(),
			None => return,
		};
		let hry = self.human_index[human_id].dead_ry();

		// The waist, which is where the smoke comes out.
		let hx = hx + (hrx * -1.0 - PI / 2.0).cos() * hry.sin() * HUMAN_HEIGHT / 2.0;
		let hz = hz + (hrx * -1.0 - PI / 2.0).sin() * hry.sin() * HUMAN_HEIGHT / 2.0;

		let rnd = degree_to_radian(10.0) * get_rand(18) as f32;
		for (dx, dy, dz, mx, my, mz, rot) in [
			(1.0, 1.0, 1.0, 0.1, 0.05, 0.1, rnd),
			(1.0, -1.0, -1.0, 0.2, 0.05, -0.2, rnd * -1.0),
			(-1.0, -1.0, -1.0, -0.3, 0.05, -0.3, rnd),
		] {
			self.add_effect(
				hx + dx,
				hy + dy,
				hz + dz,
				mx,
				my,
				mz,
				0.0,
				rot,
				0.0,
				get_rand(30) as f32 * 0.1 + 30.0,
				0.1,
				resource.get_effect_smoke_texture(),
				10,
				0,
				1.0,
				-1.0 / (GAMEFPS * 3.0),
				effect_type::NORMAL,
				GAMEFPS as i32 * 3,
			);
		}
	}

	/// `CollideBlood`: whether an effect is a blood drop that just reached the map.
	///
	/// On a hit the caller turns it into a blood stain on that block face and frees the
	/// effect. Returning the block, the face and the point is the three out parameters of
	/// the upstream.
	pub fn collide_blood(
		&self,
		in_effect: &Effect,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		resource: &ResourceManager,
		config: &Config,
	) -> Option<(i32, i32, f32, f32, f32)> {
		if !in_effect.object.enable_flag() {
			return None;
		}
		if !in_effect.collide_map_flag() {
			return None;
		}
		if in_effect.texture_id() != resource.get_effect_blood_texture() {
			return None;
		}
		if !config.get_blood_flag() {
			return None;
		}

		let (x, y, z, _) = in_effect.object.pos_data();
		let (move_x, move_y, move_z) = in_effect.get_move();

		// A drop that is not moving cannot reach anything.
		if move_x == 0.0 && move_y == 0.0 && move_z == 0.0 {
			return None;
		}

		let dist = (move_x * move_x + move_y * move_y + move_z * move_z).sqrt();
		let vx = move_x / dist;
		let vy = move_y / dist;
		let vz = move_z / dist;

		let (id, face, dist2) =
			coll.check_all_block_intersect_ray(block_data_if, x, y, z, vx, vy, vz, dist)?;

		// Float it slightly above the map, so the stain does not z fight with the face.
		let dist2 = dist2 - 1.0;

		Some((id, face, x + vx * dist2, y + vy * dist2, z + vz * dist2))
	}

	/// `PickupWeapon`: whether a human is standing in reach of a weapon, and picks it up.
	///
	/// The weapon has to be enabled and unheld, the human alive and not a zombie, the
	/// weapon within the two height limits, and the flat distance strictly within
	/// [`HUMAN_PICKUPWEAPON_R`].
	pub fn pickup_weapon(&mut self, human_id: usize, weapon_id: usize) {
		let Some(in_human) = self.human_index.get(human_id) else {
			return;
		};
		if !in_human.enable_flag() || in_human.hp() <= 0 {
			return;
		}

		// A zombie does not pick weapons up.
		if self
			.param
			.get_human(in_human.get_param_data().0)
			.is_some_and(|param| param.r#type == 2)
		{
			return;
		}

		let Some(in_weapon) = self.weapon_index.get(weapon_id) else {
			return;
		};
		if !in_weapon.object.enable_flag() || in_weapon.using_flag() {
			return;
		}

		let (human_x, human_y, human_z, _) = self.human_index[human_id].pos_data();
		let (weapon_x, weapon_y, weapon_z, _) = self.weapon_index[weapon_id].object.pos_data();

		if (human_y + HUMAN_PICKUPWEAPON_L) <= weapon_y && weapon_y <= (human_y + HUMAN_PICKUPWEAPON_H) {
			let x = human_x - weapon_x;
			let z = human_z - weapon_z;
			if x * x + z * z < HUMAN_PICKUPWEAPON_R * HUMAN_PICKUPWEAPON_R {
				// The human registers the weapon as held. The two are in different pools,
				// so the two field borrows are disjoint and no split is needed.
				self.human_index[human_id].pickup_weapon(weapon_id, &mut self.weapon_index);
			}
		}
	}

	/// `CleanupPointDataToObject`: free every object the point data placed.
	pub fn cleanup_point_data_to_object(&mut self, d3dg: &mut D3DGraphics, resource: &mut ResourceManager) {
		for human in self.human_index.iter_mut() {
			human.set_enable_flag(false);
		}
		for weapon in self.weapon_index.iter_mut() {
			weapon.object.set_enable_flag(false);
		}
		for object in self.small_object_index.iter_mut() {
			object.object.set_enable_flag(false);
		}
		for bullet in self.bullet_index.iter_mut() {
			bullet.object.set_enable_flag(false);
		}
		for grenade in self.grenade_index.iter_mut() {
			grenade.bullet.object.set_enable_flag(false);
		}
		for effect in self.effect_index.iter_mut() {
			effect.object.set_enable_flag(false);
		}

		resource.cleanup_human_texture(d3dg);
	}

	/// `LoadPointData`: build the world the mission file asks for.
	///
	/// The walk is in file order and each object takes the lowest free slot of its pool,
	/// so the slot an object lands in is a function of the file, and the player's data
	/// number is the human whose point identifier is zero.
	pub fn load_point_data(
		&mut self,
		d3dg: &mut D3DGraphics,
		resource: &mut ResourceManager,
		point_data: &PointDataInterface,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
	) {
		self.add_collision_flag = false;
		self.player_human_id = 0;

		for i in 0..MAX_HUMAN {
			self.human_shot_flag[i] = false;
			self.human_yakkyou_cnt[i] = -1;
			self.friendly_fire[i] = false;
			self.human_frame_texture_refresh[i] = false;
		}

		for i in 0..point_data.get_total_datas() {
			let Some(data) = point_data.getdata(i) else {
				continue;
			};

			// A human.
			if data.p1 == POINT_P1TYPE_HUMAN || data.p1 == POINT_P1TYPE_HUMAN2 {
				// The human info point that describes this human: p1 is the point type and
				// p4 is the human type number.
				let (found, human_info) =
					point_data.search_pointdata(0x01 | 0x08, POINT_P1TYPE_HUMANINFO, 0, 0, data.p2, 0);

				if found == 0 {
					// A human with no info point is added as 'special black A'. This is
					// the upstream's bug reproduction, and it keeps a malformed mission
					// playable instead of dropping the human.
					let Some(human_param) = self.param.get_human(0) else {
						continue;
					};
					let mut weapon = human_param.weapon;

					resource.add_human_texture(d3dg, 0);

					if data.p1 == POINT_P1TYPE_HUMAN2 {
						weapon[1] = ID_WEAPON_NONE;
					}

					if data.p4 == 0 && self.player_human_id == 0 {
						self.player_human_id = self.add_human_index(
							data.x,
							data.y,
							data.z,
							data.r,
							0,
							0,
							weapon,
							data.p4,
							true,
							d3dg,
							resource,
						);
					} else {
						self.add_human_index(
							data.x, data.y, data.z, data.r, 0, 0, weapon, data.p4, true, d3dg, resource,
						);
					}
				} else if let Some(human_info) = human_info {
					resource.add_human_texture(d3dg, human_info.p2 as i32);
					self.add_human_index_data(data, human_info, true, d3dg, resource);
				}
			}

			if data.p1 == POINT_P1TYPE_WEAPON || data.p1 == POINT_P1TYPE_RAND_WEAPON {
				self.add_weapon_index(data, d3dg, resource);
			}

			if data.p1 == POINT_P1TYPE_SMALLOBJ {
				self.add_small_object_index_data(data, coll, block_data_if, mif, resource);
			}
		}

		self.object_log.clear_log();
		self.object_log.info_log(b"Set object...");
	}

	/// `Recovery`: re-apply every model and texture after the renderer lost them, and drop
	/// every effect.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics, resource: &mut ResourceManager) {
		let (upmodel, armmodel, legmodel, walkmodel, runmodel) = resource.get_human_model();

		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}

			let human_id = self.human_index[i].get_param_data().0;
			let human_param = self.param.get_human(human_id);

			if !ENABLE_BUG_HUMANWEAPON && human_param.is_none() {
				self.human_index[i].set_enable_flag(false);
				continue;
			}

			if let Some(human_param) = human_param {
				resource.add_human_texture(d3dg, human_id);

				let id = resource.get_human_texture(human_id);
				if id == -1 {
					self.human_frame_texture_refresh[i] = true;
				}

				self.human_index[i].set_texture(id);
				self.human_index[i].set_model(
					upmodel[human_param.model as usize],
					armmodel,
					legmodel,
					walkmodel,
					runmodel,
				);
			} else {
				self.human_index[i].set_texture(d3dg.map_texture_id(0));
				self.human_index[i].set_model(upmodel[0], armmodel, legmodel, walkmodel, runmodel);
			}
		}

		for i in 0..MAX_WEAPON {
			if !self.weapon_index[i].object.enable_flag() {
				continue;
			}

			let weapon_id = self.weapon_index[i].param_data().0;
			let Some((model, texture)) = resource.get_weapon_model_texture(d3dg, weapon_id) else {
				continue;
			};
			let Some(weapon_param) = self.param.get_weapon(weapon_id) else {
				continue;
			};

			self.weapon_index[i].object.set_model(model, weapon_param.size);
			self.weapon_index[i].object.set_texture(texture);
		}

		for i in 0..MAX_SMALLOBJECT {
			if !self.small_object_index[i].object.enable_flag() {
				continue;
			}

			let (small_object_id, _) = self.small_object_index[i].param_data();
			let Some((model, texture)) = resource.get_small_object_model_texture(small_object_id) else {
				continue;
			};

			self.small_object_index[i].object.set_model(model, SMALLOBJECT_SCALE);
			self.small_object_index[i].object.set_texture(texture);
		}

		self.apply_bullet_model(resource);

		for effect in self.effect_index.iter_mut() {
			effect.object.set_enable_flag(false);
		}

		self.object_log.info_log(b"Recovery object...");
	}

	// -- the small accessors ----------------------------------------------

	/// `SetAddCollisionFlag`.
	pub fn set_add_collision_flag(&mut self, flag: bool) {
		self.add_collision_flag = flag;
	}

	/// `GetFriendlyFireFlag`. An id outside the human range reads as off.
	pub fn get_friendly_fire_flag(&self, id: i32) -> bool {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return false;
		}
		self.friendly_fire[id as usize]
	}

	/// `SetFriendlyFireFlag`.
	pub fn set_friendly_fire_flag(&mut self, id: i32, flag: bool) {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return;
		}
		self.friendly_fire[id as usize] = flag;
	}

	/// `GetPlayerID`.
	pub fn get_player_id(&self) -> i32 {
		self.player_human_id
	}

	/// `SetPlayerID`.
	pub fn set_player_id(&mut self, id: i32) {
		self.player_human_id = id;
	}

	/// The name the object log shows in the log's own strings. A record id outside the
	/// table is never valid, so this is not the log.
	fn player_team_id(&self) -> i32 {
		self.human_index
			.get(self.player_human_id.max(0) as usize)
			.map_or(0, |human| human.get_param_data().3)
	}

	/// `GetHumanObject`.
	pub fn get_human_object(&self, id: i32) -> Option<&Human> {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return None;
		}
		self.human_index.get(id as usize)
	}

	/// The same, writable. Upstream hands out a `human *`, so the game loop and the AI
	/// both work on the object the manager owns.
	pub fn get_human_object_mut(&mut self, id: i32) -> Option<&mut Human> {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return None;
		}
		self.human_index.get_mut(id as usize)
	}

	/// `GetPlayerHumanObject`.
	pub fn get_player_human_object(&self) -> Option<&Human> {
		self.get_human_object(self.player_human_id)
	}

	/// The same, writable.
	pub fn get_player_human_object_mut(&mut self) -> Option<&mut Human> {
		self.get_human_object_mut(self.player_human_id)
	}

	/// The human pool and the weapon pool together.
	///
	/// `human::SetWeapon` registers the weapons it is handed as held, which is a flag on
	/// the weapon itself, so the two pools have to be borrowed at once. Upstream both are
	/// file scope arrays and this is what the debug console's delete operations need.
	pub fn humans_and_weapons(&mut self) -> (&mut [Human], &mut [Weapon]) {
		(&mut self.human_index, &mut self.weapon_index)
	}

	/// The weapon pool, which the weapon type lookups need: `human::GetWeaponBlazingmode`
	/// and `human::GetMainWeaponTypeNO` both read a weapon the human only holds the index
	/// of. Upstream the two pools are file scope arrays and this is a plain read.
	pub fn weapons(&self) -> &[Weapon] {
		&self.weapon_index
	}

	/// `GetHumanObjectID`. Upstream compares the two pointers, and comparing the two
	/// addresses is the same question here.
	pub fn get_human_object_id(&self, object: &Human) -> i32 {
		for i in 0..self.human_index.len() {
			if std::ptr::eq(&self.human_index[i], object) {
				return i as i32;
			}
		}
		-1
	}

	/// `GetWeaponObject`.
	pub fn get_weapon_object(&self, id: i32) -> Option<&Weapon> {
		if id < 0 || MAX_WEAPON as i32 <= id {
			return None;
		}
		self.weapon_index.get(id as usize)
	}

	/// The same, writable.
	pub fn get_weapon_object_mut(&mut self, id: i32) -> Option<&mut Weapon> {
		if id < 0 || MAX_WEAPON as i32 <= id {
			return None;
		}
		self.weapon_index.get_mut(id as usize)
	}

	/// `GetSmallObject`.
	pub fn get_small_object(&self, id: i32) -> Option<&SmallObject> {
		if id < 0 || MAX_SMALLOBJECT as i32 <= id {
			return None;
		}
		self.small_object_index.get(id as usize)
	}

	/// The same, writable.
	pub fn get_small_object_mut(&mut self, id: i32) -> Option<&mut SmallObject> {
		if id < 0 || MAX_SMALLOBJECT as i32 <= id {
			return None;
		}
		self.small_object_index.get_mut(id as usize)
	}

	/// `GetBulletObject`.
	pub fn get_bullet_object(&self, id: i32) -> Option<&Bullet> {
		if id < 0 || MAX_BULLET as i32 <= id {
			return None;
		}
		self.bullet_index.get(id as usize)
	}

	/// The same, writable.
	pub fn get_bullet_object_mut(&mut self, id: i32) -> Option<&mut Bullet> {
		if id < 0 || MAX_BULLET as i32 <= id {
			return None;
		}
		self.bullet_index.get_mut(id as usize)
	}

	/// `GetBulletObjectID`.
	pub fn get_bullet_object_id(&self, object: &Bullet) -> i32 {
		for i in 0..self.bullet_index.len() {
			if std::ptr::eq(&self.bullet_index[i], object) {
				return i as i32;
			}
		}
		-1
	}

	/// `GetNewBulletObject`: the first unused bullet.
	///
	/// Upstream is used for the pool member's place in it, which is what a caller needs to
	/// find the same object again with [`Self::get_bullet_object_mut`], so this is the
	/// index.
	pub fn get_new_bullet_object(&self) -> Option<usize> {
		(0..MAX_BULLET).find(|&i| !self.bullet_index[i].object.enable_flag())
	}

	/// `GetNewGrenadeObject`: the first unused grenade, as an index.
	pub fn get_new_grenade_object(&self) -> Option<usize> {
		(0..MAX_GRENADE).find(|&i| !self.grenade_index[i].bullet.object.enable_flag())
	}

	/// `SearchHuman`: the first enabled human whose point identifier matches.
	///
	/// The scan is linear and stops at the first match, and duplicate identifiers in a
	/// mission file resolve to the first point in the file. The index is returned, so the
	/// caller reads or writes the object with
	/// [`Self::get_human_object`] / [`Self::get_human_object_mut`].
	pub fn search_human(&self, p4: i16) -> Option<usize> {
		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}
			if self.human_index[i].get_param_data().2 == p4 {
				return Some(i);
			}
		}
		None
	}

	/// `SearchSmallobject`: the first small object whose point identifier matches.
	///
	/// Note the upstream scans this pool whether the object is enabled or not, so a small
	/// object that has been destroyed is still found. That is deliberate there and it is
	/// kept here.
	pub fn search_smallobject(&self, p4: i16) -> Option<usize> {
		for i in 0..MAX_SMALLOBJECT {
			if self.small_object_index[i].param_data().1 == p4 {
				return Some(i);
			}
		}
		None
	}

	/// `GetTotalObjects`: how many of each pool are in use.
	pub fn get_total_objects(&self) -> (i32, i32, i32, i32, i32, i32) {
		let humans = self.human_index.iter().filter(|o| o.enable_flag()).count() as i32;
		let weapons = self
			.weapon_index
			.iter()
			.filter(|o| o.object.enable_flag())
			.count() as i32;
		let small_objects = self
			.small_object_index
			.iter()
			.filter(|o| o.object.enable_flag())
			.count() as i32;
		let bullets = self
			.bullet_index
			.iter()
			.filter(|o| o.object.enable_flag())
			.count() as i32;
		let grenades = self
			.grenade_index
			.iter()
			.filter(|o| o.bullet.object.enable_flag())
			.count() as i32;
		let effects = self
			.effect_index
			.iter()
			.filter(|o| o.object.enable_flag())
			.count() as i32;

		(humans, weapons, small_objects, bullets, grenades, effects)
	}

	// -- movement ---------------------------------------------------------

	/// `MoveForward`.
	pub fn move_forward(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.set_move_forward();
		}
	}

	/// `MoveBack`.
	pub fn move_back(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.set_move_back();
		}
	}

	/// `MoveLeft`.
	pub fn move_left(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.set_move_left();
		}
	}

	/// `MoveRight`.
	pub fn move_right(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.set_move_right();
		}
	}

	/// `MoveWalk`.
	pub fn move_walk(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.set_move_walk();
		}
	}

	/// `MoveJump`.
	pub fn move_jump(&mut self, human_id: i32) {
		if let Some(human) = self.get_human_object_mut(human_id) {
			human.jump();
		}
	}

	// -- firing and weapons -----------------------------------------------

	/// `ShotWeapon`.
	///
	/// Normal bullet fired: 1, grenade fired: 2, failure: 0.
	pub fn shot_weapon(&mut self, human_id: i32, sound: &mut SoundManager) -> i32 {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return 0;
		}
		let human_id = human_id as usize;

		let (pos_x, pos_y, pos_z, _) = self.human_index[human_id].pos_data();
		let teamid = self.human_index[human_id].get_param_data().3;
		let (rotation_x, armrotation_y) = self.human_index[human_id].rx_ry();

		let playerflag = human_id as i32 == self.player_human_id;

		// The request to fire, which also fails while reloading or switching weapons.
		let Some((weapon_paramid, gunsight_error_range)) =
			self.human_index[human_id].shot_weapon(&mut self.weapon_index)
		else {
			return 0;
		};

		let Some(param_data) = self.param.get_weapon(weapon_paramid) else {
			return 0;
		};
		let grenadeflag = weapon_paramid == ID_WEAPON_GRENADE;

		// The error range, never below the weapon's own minimum.
		let mut error_range = gunsight_error_range;
		if error_range < param_data.error_range_min {
			error_range = param_data.error_range_min;
		}

		// The firing angle with no error.
		let mut rx = rotation_x * -1.0 + PI / 2.0;
		let mut ry = armrotation_y;

		if error_range != 0 {
			let error_range_x = get_rand(error_range * 2 + 1) - error_range;
			let error_range_y = get_rand(error_range * 2 + 1) - error_range;
			rx += error_range_x as f32 * degree_to_radian(0.15);
			ry += error_range_y as f32 * degree_to_radian(0.15);
		}

		if !grenadeflag {
			// One pass per pellet, which is one for every weapon but a shotgun.
			for _ in 0..param_data.pellet {
				let (attacks, ontargetcnt, rx2, ry2) = if param_data.pellet > 1 {
					// The attack power and the count on hit are split between the pellets,
					// which adds up to twice the listed power over all of them.
					let attacks = (param_data.attacks as f32 / (param_data.pellet as f32 / 2.0)) as i32;
					let ontargetcnt = 1.0 / (param_data.pellet as f32 / 2.0);

					// And the pellets are spread around the aim.
					let a = degree_to_radian(10.0) * get_rand(36) as f32;
					let len = get_rand(5) * 2 + 5;
					(
						attacks,
						ontargetcnt,
						rx + a.cos() * len as f32 * degree_to_radian(0.15),
						ry + a.sin() * len as f32 * degree_to_radian(0.15),
					)
				} else {
					(param_data.attacks, 1.0, rx, ry)
				};

				let Some(bullet_id) = self.get_new_bullet_object() else {
					return 0;
				};

				self.bullet_index[bullet_id].set_pos_data(pos_x, pos_y + WEAPONSHOT_HEIGHT, pos_z, rx2, ry2);
				self.bullet_index[bullet_id].set_param_data(
					attacks,
					param_data.penetration,
					param_data.speed as f32 * BULLET_SPEEDSCALE,
					teamid,
					human_id as i32,
					ontargetcnt,
					true,
				);
				self.bullet_index[bullet_id].object.set_enable_flag(true);

				// The anti-human list starts empty for every new bullet.
				self.bullet_obj_human_index[bullet_id].init();
			}

			self.human_shot_flag[human_id] = true;
			self.human_yakkyou_cnt[human_id] = param_data.yakkyou_delay;
		} else {
			let Some(grenade_id) = self.get_new_grenade_object() else {
				return 0;
			};

			self.grenade_index[grenade_id].set_pos_data(pos_x, pos_y + WEAPONSHOT_HEIGHT, pos_z, rx, ry);
			self.grenade_index[grenade_id].set_param_data(8.0, teamid, human_id as i32, 1.0, true);
			self.grenade_index[grenade_id].bullet.object.set_enable_flag(true);
		}

		if param_data.soundvolume > 0 {
			sound.shot_weapon(
				pos_x,
				pos_y + WEAPONSHOT_HEIGHT,
				pos_z,
				weapon_paramid,
				teamid,
				playerflag,
			);
		}

		if grenadeflag {
			return 2;
		}
		1
	}

	/// `ShotWeaponEffect`: the muzzle flash and the two puffs of smoke.
	///
	/// The coordinates come from the weapon's own offset run through the modelview, so the
	/// flash lines up with the muzzle of whatever model the human is drawing.
	pub fn shot_weapon_effect(
		&mut self,
		human_id: i32,
		d3dg: &mut D3DGraphics,
		resource: &ResourceManager,
	) {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return;
		}
		let human_id = human_id as usize;

		let (pos_x, pos_y, pos_z, _) = self.human_index[human_id].pos_data();
		let (rotation_x, armrotation_y) = self.human_index[human_id].rx_ry();

		let weapon_paramid = self.human_index[human_id].get_main_weapon_type_no(&self.weapon_index);
		let Some(param_data) = self.param.get_weapon(weapon_paramid) else {
			return;
		};

		let flashsize = if param_data.silencer {
			1.2
		} else {
			0.06 * param_data.attacks as f32
		};
		let smokesize1 = 0.04 * param_data.attacks as f32;
		let smokesize2 = 0.032 * param_data.attacks as f32;

		let rx = rotation_x * -1.0 + PI / 2.0;
		let emx = rx.cos() * armrotation_y.cos();
		let emy = armrotation_y.sin();
		let emz = rx.sin() * armrotation_y.cos();

		d3dg.set_world_transform_human_weapon(
			pos_x,
			pos_y + 16.0,
			pos_z,
			param_data.flashx / 10.0,
			param_data.flashy / 10.0,
			param_data.flashz / 10.0,
			rotation_x,
			armrotation_y * -1.0,
			1.0,
		);
		let (x, y, z) = d3dg.get_world_transform_pos();
		d3dg.reset_world_transform();

		self.add_effect(
			x,
			y,
			z,
			0.0,
			0.0,
			0.0,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			flashsize,
			0.0,
			resource.get_effect_mflash_texture(),
			0,
			0,
			0.63,
			0.0,
			effect_type::NORMAL,
			1,
		);

		// The smoke starts a tenth of a unit behind the flash.
		d3dg.set_world_transform_human_weapon(
			pos_x,
			pos_y + 16.0,
			pos_z,
			param_data.flashx / 10.0,
			param_data.flashy / 10.0,
			param_data.flashz / 10.0 - 0.1,
			rotation_x,
			armrotation_y * -1.0,
			1.0,
		);
		let (x, y, z) = d3dg.get_world_transform_pos();
		d3dg.reset_world_transform();

		self.add_effect(
			x,
			y,
			z,
			emx * 0.15,
			emy * 0.15 + 0.02,
			emz * 0.15,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			smokesize1,
			0.3,
			resource.get_effect_smoke_texture(),
			0,
			0,
			0.47,
			-0.047,
			effect_type::NORMAL,
			12,
		);
		self.add_effect(
			x,
			y,
			z,
			emx * 0.45,
			emy * 0.45,
			emz * 0.45,
			0.0,
			degree_to_radian(10.0) * get_rand(36) as f32,
			0.0,
			smokesize2,
			0.36,
			resource.get_effect_smoke_texture(),
			0,
			0,
			0.47,
			-0.055,
			effect_type::NORMAL,
			10,
		);
	}

	/// `ShotWeaponYakkyou`: the spent cartridge thrown out of the weapon.
	pub fn shot_weapon_yakkyou(
		&mut self,
		human_id: i32,
		d3dg: &mut D3DGraphics,
		resource: &ResourceManager,
	) {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return;
		}
		let human_id = human_id as usize;

		let (pos_x, pos_y, pos_z, _) = self.human_index[human_id].pos_data();
		let (rotation_x, armrotation_y) = self.human_index[human_id].rx_ry();

		let weapon_paramid = self.human_index[human_id].get_main_weapon_type_no(&self.weapon_index);
		let Some(param_data) = self.param.get_weapon(weapon_paramid) else {
			return;
		};

		let yakkyousize = if param_data.yakkyou_size >= 0.0 {
			param_data.yakkyou_size / 10.0
		} else {
			0.012 * param_data.attacks as f32
		};

		let rx = rotation_x * -1.0 + PI / 2.0;

		let mx = (rx - PI / 2.0).cos() * param_data.yakkyou_sx * 0.12;
		let my = (param_data.yakkyou_sy + (get_rand(3) - 1) as f32) * 0.12;
		let mz = (rx - PI / 2.0).sin() * param_data.yakkyou_sx * 0.12;

		d3dg.set_world_transform_human_weapon(
			pos_x,
			pos_y + 16.0,
			pos_z,
			param_data.yakkyou_px / 10.0,
			param_data.yakkyou_py / 10.0,
			param_data.yakkyou_pz / 10.0,
			rotation_x,
			armrotation_y * -1.0,
			1.0,
		);
		let (x, y, z) = d3dg.get_world_transform_pos();
		d3dg.reset_world_transform();

		self.add_effect(
			x,
			y,
			z,
			mx,
			my,
			mz,
			-0.15,
			degree_to_radian(10.0) * get_rand(36) as f32,
			degree_to_radian((get_rand(61) - 30) as f32),
			yakkyousize,
			0.0,
			resource.get_effect_yakkyou_texture(),
			0,
			0,
			1.0,
			0.0,
			effect_type::NORMAL,
			(GAMEFPS * 0.75) as i32,
		);
	}

	/// `ReloadWeapon`.
	pub fn reload_weapon(&mut self, human_id: i32, sound: &mut SoundManager) -> bool {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return false;
		}
		let human_id = human_id as usize;

		if !self.human_index[human_id].enable_flag() || self.human_index[human_id].hp() <= 0 {
			return false;
		}

		if self.human_index[human_id].reload_weapon(&mut self.weapon_index) {
			let (x, y, z, _) = self.human_index[human_id].pos_data();
			let id = self.human_index[human_id].get_param_data().3;
			sound.reload_weapon(x, y + 16.0, z, id);
			return true;
		}

		false
	}

	/// `ChangeHaveWeapon`: switch to a held weapon, or to the next one for `id` of -1.
	pub fn change_have_weapon(&mut self, human_id: i32, id: i32) {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return;
		}
		let human_id = human_id as usize;

		self.human_index[human_id].change_have_weapon(id, &self.weapon_index);
		self.human_yakkyou_cnt[human_id] = -1;
	}

	/// `ChangeWeaponID`: switch the weapon type. Success: 0, failure: 1.
	pub fn change_weapon_id(
		&mut self,
		human_id: i32,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> i32 {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return 1;
		}
		let human_id = human_id as usize;

		if self.human_index[human_id].change_weapon_id(&mut self.weapon_index) != 0 {
			return 1;
		}

		let (selectweapon, weapon, _, _) = self.human_index[human_id].get_weapon();
		let Some(index) = weapon[selectweapon as usize] else {
			return 1;
		};

		let weapon_id = self.weapon_index[index].param_data().0;
		let Some((model, texture)) = resource.get_weapon_model_texture(d3dg, weapon_id) else {
			return 1;
		};
		let Some(param_data) = self.param.get_weapon(weapon_id) else {
			return 1;
		};
		let weapon_size = param_data.size;

		self.weapon_index[index].object.set_model(model, weapon_size);
		self.weapon_index[index].object.set_texture(texture);

		self.human_yakkyou_cnt[human_id] = -1;

		0
	}

	/// `DumpWeapon`.
	pub fn dump_weapon(&mut self, human_id: i32) -> bool {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return false;
		}
		let human_id = human_id as usize;

		self.human_yakkyou_cnt[human_id] = -1;

		self.human_index[human_id].dump_weapon(&mut self.weapon_index)
	}

	/// `ChangeScopeMode`.
	pub fn change_scope_mode(&mut self, human_id: i32) {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return;
		}
		let human_id = human_id as usize;

		if self.human_index[human_id].get_scope_mode() == 0 {
			self.human_index[human_id].set_enable_scope(&self.weapon_index);
		} else {
			self.human_index[human_id].set_disable_scope();
		}
	}

	/// `CheatAddBullet`.
	pub fn cheat_add_bullet(
		&mut self,
		human_id: i32,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> bool {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return false;
		}
		let human_id = human_id as usize;

		let (selectweapon, weapon, _, _) = self.human_index[human_id].get_weapon();
		let Some(index) = weapon[selectweapon as usize] else {
			return false;
		};

		let (id_param, lnbs, nbs) = self.weapon_index[index].param_data();
		let Some(param_data) = self.param.get_weapon(id_param) else {
			return false;
		};

		// Add a magazine's worth, up to whatever the weapon allows.
		let nbs = nbs + param_data.nbsmax;
		self.weapon_index[index].reset_weapon_param(resource, d3dg, id_param, lnbs, nbs);

		true
	}

	/// `CheatNewWeapon`: give a human a weapon type, or take its weapon away for
	/// [`ID_WEAPON_NONE`].
	pub fn cheat_new_weapon(
		&mut self,
		human_id: i32,
		new_weapon_id: i32,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> bool {
		if human_id < 0 || MAX_HUMAN as i32 <= human_id {
			return false;
		}
		if new_weapon_id < 0 || TOTAL_PARAMETERINFO_WEAPON as i32 <= new_weapon_id {
			return false;
		}
		let human_id = human_id as usize;

		let (selectweapon, weapon, mut lnbslist, mut nbslist) = self.human_index[human_id].get_weapon();
		let selectweapon = selectweapon as usize;

		// Nothing to do: no weapon held and no weapon offered.
		if weapon[selectweapon].is_none() && new_weapon_id == ID_WEAPON_NONE {
			return false;
		}

		if weapon[selectweapon].is_none() {
			// Place the new weapon and hand it over.
			let dataid = self.add_visual_weapon_index(new_weapon_id, false, d3dg, resource);
			if dataid == -1 {
				return false;
			}
			let dataid = dataid as usize;

			let mut weapon = weapon;
			weapon[selectweapon] = Some(dataid);
			self.weapon_index[dataid].reset_weapon_param(
				resource,
				d3dg,
				new_weapon_id,
				lnbslist[selectweapon],
				nbslist[selectweapon],
			);

			// The counts for an empty hand start over now that a weapon is held.
			lnbslist[selectweapon] = 0;
			nbslist[selectweapon] = 0;

			self.human_index[human_id].set_weapon(weapon, lnbslist, nbslist, &mut self.weapon_index);

			self.human_yakkyou_cnt[human_id] = -1;
			return true;
		}

		let index = weapon[selectweapon].expect("a weapon is held");
		let (_, lnbs, nbs) = self.weapon_index[index].param_data();

		if new_weapon_id == ID_WEAPON_NONE {
			// Take the weapon away, keeping the bullet counts for when it comes back.
			self.weapon_index[index].object.set_enable_flag(false);

			let mut weapon = weapon;
			weapon[selectweapon] = None;
			lnbslist[selectweapon] = lnbs;
			nbslist[selectweapon] = nbs;
			self.human_index[human_id].set_weapon(weapon, lnbslist, nbslist, &mut self.weapon_index);

			self.human_index[human_id].set_disable_scope();
		} else {
			// Change the weapon in place, carrying the bullet counts over.
			self.weapon_index[index]
				.reset_weapon_param(resource, d3dg, new_weapon_id, lnbs, nbs);
		}

		self.human_yakkyou_cnt[human_id] = -1;
		true
	}

	/// `CheckZombieAttack`: whether a zombie's arm reaches the target. Judgment only, and
	/// the caller calls [`Self::hit_zombie_attack`] when this says yes.
	///
	/// The attack point is the tip of the arm, two units out from the zombie at eye
	/// height, and the target has to be within 3.3 units of it on the flat and inside the
	/// target's own height.
	///
	/// Upstream adds `VIEW_HEIGHT` to the zombie's Y twice, once when it reads the
	/// position and once when it computes the attack point. That looks like a bug and the
	/// behaviour is kept, because the reach of a zombie is game feel.
	pub fn check_zombie_attack(&self, my_human_id: usize, enemy_human_id: usize) -> bool {
		let (Some(my_human), Some(enemy_human)) = (
			self.human_index.get(my_human_id),
			self.human_index.get(enemy_human_id),
		) else {
			return false;
		};

		if !my_human.enable_flag() || my_human.hp() <= 0 {
			return false;
		}
		if !enemy_human.enable_flag() || enemy_human.dead_flag() {
			return false;
		}

		let (mx, my, mz, mrx) = my_human.pos_data();
		let mteam = my_human.get_param_data().3;
		let my = my + VIEW_HEIGHT;
		let (tx, ty, tz, _) = enemy_human.pos_data();
		let tteam = enemy_human.get_param_data().3;
		let ty = ty + VIEW_HEIGHT;

		// An ally is not attacked.
		if mteam == tteam {
			return false;
		}

		let attack_point_x = mx + (mrx * -1.0 + PI / 2.0).cos() * 2.0;
		let attack_point_y = my + VIEW_HEIGHT - 0.5;
		let attack_point_z = mz + (mrx * -1.0 + PI / 2.0).sin() * 2.0;
		let ax = attack_point_x - tx;
		let az = attack_point_z - tz;

		if (ax * ax + az * az) < 3.3 * 3.3 {
			if attack_point_y >= ty && attack_point_y <= (ty + HUMAN_HEIGHT) {
				return true;
			}
		}

		false
	}

	/// `HitZombieAttack`: the damage, the blood and the sound of a zombie's claw.
	pub fn hit_zombie_attack(
		&mut self,
		my_human_id: usize,
		enemy_human_id: usize,
		resource: &ResourceManager,
		sound: &mut SoundManager,
		config: &Config,
	) {
		let Some(enemy_human) = self.human_index.get(enemy_human_id) else {
			return;
		};
		if !enemy_human.enable_flag() || enemy_human.dead_flag() {
			return;
		}
		if self.human_index.get(my_human_id).is_none() {
			return;
		}

		let my_human_data_id = self.human_index[my_human_id].get_param_data().1;
		let my_human_team_id = self.human_index[my_human_id].get_param_data().3;
		let enemy_human_data_id = self.human_index[enemy_human_id].get_param_data().1;
		let enemy_human_team_id = self.human_index[enemy_human_id].get_param_data().3;

		let (mx, _, mz, _) = self.human_index[my_human_id].pos_data();
		let (tx, ty, tz, _) = self.human_index[enemy_human_id].pos_data();
		let ty = ty + VIEW_HEIGHT;

		// The direction the attack came from.
		let arx = (mz - tz).atan2(mx - tx);

		let param_id = self.human_index[enemy_human_id].get_param_data().0;
		let not_robot = self
			.param
			.get_human(param_id)
			.is_none_or(|param| param.r#type != 1);

		let hp_old = self.human_index[enemy_human_id].hp();
		self.human_index[enemy_human_id].hit_zombie_attack();

		let mut flag = true;
		if ENABLE_BUG_TEAMID && enemy_human_team_id < 0 && my_human_team_id < 0 {
			if enemy_human_team_id < my_human_team_id {
				flag = false;
			}
		}
		if flag {
			self.human_index[enemy_human_id].set_hit_flag(arx * -1.0 + PI / 2.0);
		}

		self.set_human_blood(tx, ty, tz, HUMAN_DAMAGE_ZOMBIEU, not_robot, resource, config);

		sound.hit_human(tx, ty, tz, 3, my_human_team_id);

		if hp_old > 0 && self.human_index[enemy_human_id].hp() <= 0 {
			let player_team_id = self.player_team_id();
			self.object_log.died_human(
				my_human_data_id,
				enemy_human_data_id,
				my_human_team_id,
				enemy_human_team_id,
				player_team_id,
			);
		}
	}

	/// `HumanResuscitation`: revive a dead human, with empty hands.
	pub fn human_resuscitation(&mut self, id: i32) -> bool {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return false;
		}
		let id = id as usize;

		if !self.human_index[id].enable_flag() || !self.human_index[id].dead_flag() {
			return false;
		}

		let (id_param, dataid, p4, team) = self.human_index[id].get_param_data();
		if id_param < 0 || TOTAL_PARAMETERINFO_HUMAN as i32 <= id_param {
			// An unknown human cannot be revived.
			return false;
		}
		self.human_index[id].set_param_data(id_param, dataid, p4, team, true);

		let player_team_id = self.player_team_id();
		self.object_log.revive_human(id as i32, team, player_team_id);

		true
	}

	/// `CheckGameOverorComplete`.
	///
	/// Game clear: 1, game over: 2, no decision: 0.
	///
	/// The game clear test runs first, because the original XOPS decides a draw as a
	/// clear, and the enemy side is counted as total health rather than as bodies.
	pub fn check_game_overor_complete(&self) -> i32 {
		let my_team_id = self.player_team_id();
		let mut total_enemy_hp = 0;

		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}

			let team_id = self.human_index[i].get_param_data().3;
			if team_id != my_team_id && self.human_index[i].hp() > 0 {
				total_enemy_hp += self.human_index[i].hp();
			}
		}

		if total_enemy_hp == 0 {
			return 1;
		}

		// The enabled test is the upstream's guard against a cheat having moved the player
		// number onto a slot nobody occupies.
		if let Some(player) = self.human_index.get(self.player_human_id.max(0) as usize) {
			if player.enable_flag() && player.dead_flag() {
				return 2;
			}
		}

		0
	}

	/// `GetObjectInfoTag`: the text the debug overlay shows for whatever the camera is
	/// pointing at.
	///
	/// Returns the colour and the text, or `None` when nothing was close enough and within
	/// ten degrees in both axes. The text is built byte for byte as the overlay wants it,
	/// and the closest of the three pools wins because each one shrinks the search radius.
	pub fn get_object_info_tag(
		&self,
		camera_x: f32,
		camera_y: f32,
		camera_z: f32,
		camera_rx: f32,
		camera_ry: f32,
	) -> Option<(u32, Vec<u8>)> {
		let mut dist = 50.0;
		let mut color = 0;
		let mut info = Vec::new();

		let player_team_id = self.player_team_id();

		for i in 0..MAX_HUMAN {
			if i as i32 == self.player_human_id {
				continue;
			}
			if !self.human_index[i].enable_flag() {
				continue;
			}

			let (px, py, pz, _) = self.human_index[i].pos_data();

			// The human test is horizontal only, which is why the vertical angle is 0.
			if let Some((rx, _, r)) =
				check_target_angle(camera_x, camera_y, camera_z, camera_rx, 0.0, px, py, pz, dist)
			{
				if rx.abs() < degree_to_radian(10.0) {
					let team = self.human_index[i].get_param_data().3;
					let hp = self.human_index[i].hp();
					if team == player_team_id {
						color = D3DGraphics::get_color_code(0.0, 0.0, 1.0, 1.0);
						info = format!("Human[{i}]  HP {hp} : Friend").into_bytes();
					} else {
						color = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);
						info = format!("Human[{i}]  HP {hp} : Enemy").into_bytes();
					}
					dist = r.sqrt();
				}
			}
		}

		for i in 0..MAX_WEAPON {
			if !self.weapon_index[i].object.enable_flag() || self.weapon_index[i].using_flag() {
				continue;
			}

			let (px, py, pz, _) = self.weapon_index[i].object.pos_data();
			let (_, lnbs, nbs) = self.weapon_index[i].param_data();

			if let Some((rx, ry, r)) = check_target_angle(
				camera_x, camera_y, camera_z, camera_rx, camera_ry, px, py, pz, dist,
			) {
				if rx.abs() < degree_to_radian(10.0) && ry.abs() < degree_to_radian(10.0) {
					color = D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0);
					info = format!("Weapon[{i}]  {lnbs}:{}", nbs - lnbs).into_bytes();
					dist = r.sqrt();
				}
			}
		}

		for i in 0..MAX_SMALLOBJECT {
			if !self.small_object_index[i].object.enable_flag() {
				continue;
			}

			let (px, py, pz, _) = self.small_object_index[i].object.pos_data();

			if let Some((rx, ry, r)) = check_target_angle(
				camera_x, camera_y, camera_z, camera_rx, camera_ry, px, py, pz, dist,
			) {
				if rx.abs() < degree_to_radian(10.0) && ry.abs() < degree_to_radian(10.0) {
					color = D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0);
					info = format!("SmallObject[{i}]  HP {}", self.small_object_index[i].hp()).into_bytes();
					dist = r.sqrt();
				}
			}
		}

		if info.is_empty() {
			return None;
		}
		Some((color, info))
	}

	/// `Process`: one frame of the whole object layer.
	///
	/// Always returns 0, and the order of the passes is the game's: the humans first, so a
	/// shot fired this frame is a bullet that moves next frame.
	#[allow(clippy::too_many_arguments)]
	pub fn process(
		&mut self,
		cmd_f5id: i32,
		demomode: bool,
		screen: bool,
		d3dg: &mut D3DGraphics,
		resource: &ResourceManager,
		coll: &Collision,
		block_data_if: &BlockDataInterface,
		mif: Option<&MIFInterface>,
		sound: &mut SoundManager,
		config: &Config,
	) -> i32 {
		// This frame's battle record starts empty.
		for i in 0..MAX_HUMAN {
			self.human_ontarget[i] = 0.0;
			self.human_kill[i] = 0;
			self.human_headshot[i] = 0;
		}

		for i in 0..MAX_HUMAN {
			let player = self.player_human_id == i as i32;
			let cmd_f5 = i as i32 == cmd_f5id;

			let rtn = self.human_index[i].process_object(
				coll,
				block_data_if,
				self.add_collision_flag,
				player,
				cmd_f5,
				&mut self.weapon_index,
			);
			if rtn == 2 {
				self.dead_effect(i, resource);
			}
			if rtn == 4 {
				let team_id = self.human_index[i].get_param_data().3;
				let player_team_id = self.player_team_id();
				self.object_log.died_human(-1, i as i32, -1, team_id, player_team_id);
			}

			self.human_index[i].set_dark_model_flag(screen);

			// The block under the feet names the footstep sound, and there is no footstep
			// sound off the edge of the map.
			let (block_id, block_face) = self.human_index[i].get_under_block();
			let block_texture_id = if block_id != -1 {
				block_data_if
					.getdata(block_id)
					.map_or(-1, |data| data.material[block_face as usize].texture_id)
			} else {
				-1
			};

			let move_mode = self.human_index[i].get_movemode(false);
			let jump_landing_mode = self.human_index[i].get_jump_landing(false);

			if (1..=4).contains(&move_mode) {
				let (posx, posy, posz, _) = self.human_index[i].pos_data();
				let team_id = self.human_index[i].get_param_data().3;
				let cnt = self.human_index[i].move_motion_count();
				sound.set_footsteps(posx, posy, posz, block_texture_id, move_mode - 1, team_id, cnt);
			}

			if jump_landing_mode == 1 || jump_landing_mode == 2 {
				let (posx, posy, posz, _) = self.human_index[i].pos_data();
				let team_id = self.human_index[i].get_param_data().3;
				// Jumping is mode 4 and landing mode 5.
				let mode = if jump_landing_mode == 1 { 4 } else { 5 };
				sound.set_footsteps(posx, posy, posz, block_texture_id, mode, team_id, 0);
			}
		}

		for i in 0..MAX_WEAPON {
			self.weapon_index[i].process_object(coll, block_data_if);
			self.weapon_index[i].object.set_dark_model_flag(screen);
		}

		for i in 0..MAX_SMALLOBJECT {
			self.small_object_index[i].process_object(mif);
			self.small_object_index[i].object.set_dark_model_flag(screen);
		}

		if !demomode {
			for i in 0..MAX_BULLET {
				// The collision first, so a bullet that hits something this frame stops
				// where it hit rather than a step later.
				self.collide_bullet(i, coll, block_data_if, mif, resource, sound, config);
				self.bullet_index[i].process_object();
				self.bullet_index[i].object.set_dark_model_flag(screen);

				if self.bullet_index[i].object.enable_flag() {
					let (_, _, speed, teamid, _, _) = self.bullet_index[i].param_data();
					let (bx, by, bz, brx, bry) = self.bullet_index[i].pos_data();
					let mx = brx.cos() * bry.cos() * speed;
					let my = bry.sin() * speed;
					let mz = brx.sin() * bry.cos() * speed;
					sound.passing_bullet(bx, by, bz, mx, my, mz, teamid);
				}
			}
		} else {
			// The menu and the briefing screens have no bullets.
			for bullet in self.bullet_index.iter_mut() {
				if bullet.object.enable_flag() {
					bullet.object.set_enable_flag(false);
				}
			}
		}

		for i in 0..MAX_HUMAN {
			if self.human_shot_flag[i] {
				self.shot_weapon_effect(i as i32, d3dg, resource);
				self.human_shot_flag[i] = false;
			}

			if self.human_yakkyou_cnt[i] == 0 {
				self.shot_weapon_yakkyou(i as i32, d3dg, resource);
			}
			if self.human_yakkyou_cnt[i] > -1 {
				self.human_yakkyou_cnt[i] -= 1;
			}
		}

		for i in 0..MAX_EFFECT {
			let blood = self.collide_blood(&self.effect_index[i], coll, block_data_if, resource, config);
			if let Some((id, face, pos_x, pos_y, pos_z)) = blood {
				self.add_map_effect(
					block_data_if,
					id,
					face,
					pos_x,
					pos_y,
					pos_z,
					10.0,
					degree_to_radian(10.0) * get_rand(18) as f32,
					resource.get_effect_blood_texture(),
					0,
					0,
					1.0,
					-0.002,
					GAMEFPS as i32 * 20,
				);
				self.effect_index[i].object.set_enable_flag(false);
			} else {
				self.effect_index[i].process_object();
				self.effect_index[i].object.set_dark_model_flag(screen);
			}
		}

		if !demomode {
			for i in 0..MAX_GRENADE {
				let speed = self.grenade_index[i].get_speed();

				let rcr = self.grenade_index[i].process_object(coll, block_data_if);
				self.grenade_index[i].bullet.object.set_dark_model_flag(screen);

				// A bounce, if it was moving fast enough to be worth hearing.
				if rcr == 1 && speed > 3.4 {
					let (x, y, z, _, _) = self.grenade_index[i].pos_data();
					let (_, teamid, _, _) = self.grenade_index[i].param_data();
					sound.grenade_bound(x, y, z, teamid);
				}

				if rcr == 2 {
					self.grenade_explosion(i, coll, block_data_if, mif, resource, sound, config);
				}
			}
		} else {
			for grenade in self.grenade_index.iter_mut() {
				if grenade.bullet.object.enable_flag() {
					grenade.bullet.object.set_enable_flag(false);
				}
			}
		}

		// Weapon pickup. Walking every weapon from every human every frame is wasted work,
		// so the upstream halves it: even frames test the even numbered weapons and odd
		// frames the odd ones.
		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}
			let mut j = (self.framecnt % 2) as usize;
			while j < MAX_WEAPON {
				self.pickup_weapon(i, j);
				j += 2;
			}
		}

		// Humans pushing each other apart, each pair once.
		for i in 0..MAX_HUMAN {
			if !self.human_index[i].enable_flag() {
				continue;
			}
			for j in (i + 1)..MAX_HUMAN {
				if self.human_index[j].enable_flag() {
					self.collide_human(i, j);
				}
			}
		}

		self.object_log.process();
		self.framecnt += 1;

		0
	}

	/// `GetHumanShotInfo`: this frame's hit count, kills and headshots for one human.
	pub fn get_human_shot_info(&self, id: i32) -> Option<(f32, i32, i32)> {
		if id < 0 || MAX_HUMAN as i32 <= id {
			return None;
		}
		let id = id as usize;
		Some((self.human_ontarget[id], self.human_kill[id], self.human_headshot[id]))
	}

	/// `SortEffect`: fill `data` with the live effects, farthest first.
	///
	/// The upstream is an insertion sort and it is kept, because an insertion sort is
	/// stable: two effects at the same distance keep their pool order, and an unstable
	/// sort would let them swap frame to frame and flicker. The comparison is on the
	/// squared distance, so the square roots are never taken.
	fn sort_effect(&self, camera_x: f32, camera_y: f32, camera_z: f32, data: &mut [Effectdata]) -> usize {
		let mut cnt = 0;

		for i in 0..MAX_EFFECT {
			if !self.effect_index[i].object.enable_flag() {
				continue;
			}
			if cnt >= data.len() {
				break;
			}

			let (ex, ey, ez, _) = self.effect_index[i].object.pos_data();
			let x = ex - camera_x;
			let y = ey - camera_y;
			let z = ez - camera_z;
			data[cnt].id = i as i32;
			data[cnt].dist = x * x + y * y + z * z;

			cnt += 1;
		}

		for i in 1..cnt {
			let temp = data[i];
			let mut j = i;
			while j > 0 && data[j - 1].dist < temp.dist {
				data[j] = data[j - 1];
				j -= 1;
			}
			data[j] = temp;
		}

		cnt
	}

	/// `Render`: the whole object layer, one frame.
	///
	/// Effects are drawn last and farthest first, with the depth write off, which is what
	/// lets overlapping smoke blend into itself instead of cutting holes in itself.
	#[allow(clippy::too_many_arguments)]
	pub fn render(
		&mut self,
		d3dg: &mut D3DGraphics,
		resource: &ResourceManager,
		camera_x: f32,
		camera_y: f32,
		camera_z: f32,
		camera_rx: f32,
		camera_ry: f32,
		hide_player: i32,
		no_model: bool,
	) {
		d3dg.reset_world_transform();

		for i in 0..MAX_HUMAN {
			// A human whose texture never loaded refreshes it from its neighbour, which is
			// how a mystery human ends up looking like the human before it.
			if self.human_frame_texture_refresh[i] && i > 0 {
				let weapon_type =
					self.human_index[i - 1].get_main_weapon_type_no(&self.weapon_index);

				if weapon_type != ID_WEAPON_NONE {
					if let Some((_, texture)) = resource.get_weapon_model_texture(d3dg, weapon_type) {
						self.human_index[i].set_texture(texture);
					}
				} else {
					let texture = self.human_index[i - 1].texture();
					self.human_index[i].set_texture(texture);
				}
			}

			let draw_arm = if hide_player == 0 {
				false
			} else if self.player_human_id != i as i32 {
				false
			} else if hide_player == 2 {
				true
			} else {
				// The player is not drawn at all, which is the first person view.
				continue;
			};

			let player = self.player_human_id == i as i32;

			// The human's own render takes the weapon pool read only.
			let weapons = &self.weapon_index;
			self.human_index[i].render(d3dg, resource, draw_arm, player, no_model, weapons);
		}

		for i in 0..MAX_WEAPON {
			self.weapon_index[i].render(d3dg, no_model);
		}

		for i in 0..MAX_SMALLOBJECT {
			self.small_object_index[i].render(d3dg, no_model);
		}

		for i in 0..MAX_BULLET {
			self.bullet_index[i].render(d3dg, no_model);
		}

		for i in 0..MAX_GRENADE {
			self.grenade_index[i].render(d3dg, no_model);
		}

		// Effects blend into each other, so they are drawn with the depth write off and
		// behind each other, farthest first. This is the pair `StartEffectRender` and
		// `EndEffectRender` do upstream, written out here because the renderer wrapper has
		// no method for them.
		d3dg.gfx().depth_mask(false);

		let mut data = [Effectdata::default(); MAX_EFFECT];
		let cnt = self.sort_effect(camera_x, camera_y, camera_z, &mut data);
		for entry in data.iter().take(cnt) {
			self.effect_index[entry.id as usize].render(d3dg, camera_rx, camera_ry, no_model);
		}

		d3dg.gfx().depth_mask(true);
	}

	/// `RenderLog`.
	pub fn render_log(&mut self, d3dg: &mut D3DGraphics, x: i32, y: i32) {
		self.object_log.render(d3dg, x, y);
	}

	/// `Cleanup`.
	pub fn cleanup(&mut self, d3dg: &mut D3DGraphics, resource: &mut ResourceManager) {
		self.cleanup_point_data_to_object(d3dg, resource);
	}
}

impl Default for ObjectManager {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::Config;
	use crate::datafile::{PointDataInterface, TOTAL_BLOCKTEXTURE};
	use crate::parameter::HUMAN_PARAMETER;
	use xops_platform::gfx::Gfx;

	// --- helpers ---------------------------------------------------------

	/// A renderer with no window and no surface, the same shape the renderer's own tests
	/// use. Only the paths that have to touch the renderer get one.
	fn headless() -> D3DGraphics {
		let instance = xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
			backends: xops_platform::wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = Gfx::new(&instance, None, 64, 48, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// A sound manager with no sound class, so every call just queues.
	fn sound() -> SoundManager {
		SoundManager::new()
	}

	/// The player's settings, with the defaults applied when blood is wanted.
	fn config(blood: bool) -> Config {
		let mut config = Config::new();
		if blood {
			config.set_default_config();
		}
		config
	}

	/// A collision class that has never been initialised, so every query answers no. That
	/// is what an empty map is: nothing is in the way.
	fn empty_world() -> (Collision, BlockDataInterface) {
		(Collision::new(), BlockDataInterface::new())
	}

	/// A map holding one unit cube at the origin, loaded and prepared the way a mission
	/// load leaves it. Copied from the human tests: a human that has to keep standing, or
	/// land, needs a real block under it and the loader is the only way to get one.
	fn one_cube_map(name: &str) -> (BlockDataInterface, Collision) {
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

		let mut image = Vec::new();
		image.extend_from_slice(&[0u8; 31 * TOTAL_BLOCKTEXTURE]);
		image.extend_from_slice(&1u16.to_le_bytes());
		for axis in 0..3 {
			for corner in CUBE {
				let value = match axis {
					0 => corner.0,
					1 => corner.1,
					_ => corner.2,
				};
				image.extend_from_slice(&value.to_bits().to_le_bytes());
			}
		}
		for i in 24..72 {
			image.extend_from_slice(&(i as f32).to_bits().to_le_bytes());
		}
		for j in 0..6 {
			image.extend_from_slice(&[j as u8, 0, 0, 0]);
		}
		image.extend_from_slice(&[0; 4]);

		let path = temp_path(name);
		xops_platform::fs::write_file(&path, &image).expect("write the temp map");

		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(path.to_string_lossy().as_bytes()), 0);
		blocks.calculation_blockdata(false);

		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(Some(&blocks)), 0);
		(blocks, coll)
	}

	/// Places a human with type 0's health at a position, on a team.
	fn place_human(mgr: &mut ObjectManager, slot: usize, x: f32, y: f32, z: f32, team: i32, rx: f32) {
		mgr.human_index[slot].set_pos_data(x, y, z, rx);
		mgr.human_index[slot].set_param_data(0, 0, slot as i16, team, true);
		mgr.human_index[slot].object.set_enable_flag(true);
	}

	/// Places a weapon nobody holds.
	fn place_weapon(mgr: &mut ObjectManager, slot: usize, x: f32, y: f32, z: f32) {
		mgr.weapon_index[slot].set_pos_data(x, y, z, 0.0);
		mgr.weapon_index[slot].set_param_data(1, 0, 30, true);
		mgr.weapon_index[slot].object.set_enable_flag(true);
	}

	/// Empties the hands of human 0, so the next pickup has a slot to go into.
	fn clear_hands(mgr: &mut ObjectManager) {
		mgr.human_index[0].set_weapon(
			[None; TOTAL_HAVEWEAPON],
			[0; TOTAL_HAVEWEAPON],
			[0; TOTAL_HAVEWEAPON],
			&mut mgr.weapon_index,
		);
	}

	/// The parameter a human type of a given kind is at, found rather than hard coded so
	/// the test does not break when the table is edited.
	fn human_type_of(kind: i32) -> i32 {
		(0..TOTAL_PARAMETERINFO_HUMAN as i32)
			.find(|&i| HUMAN_PARAMETER[i as usize].r#type == kind)
			.expect("the table has a human of every kind")
	}

	fn temp_path(name: &str) -> std::path::PathBuf {
		std::env::temp_dir().join(format!("xops_objectmanager_{}_{}", std::process::id(), name))
	}

	/// A .pd1 image: a two byte count and then four floats and four signed bytes a point.
	fn pd1_image(points: &[(f32, f32, f32, f32, i8, i8, i8, i8)]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend_from_slice(&(points.len() as u16).to_le_bytes());
		for &(x, y, z, r, p1, p2, p3, p4) in points {
			for value in [x, y, z, r] {
				out.extend_from_slice(&value.to_bits().to_le_bytes());
			}
			out.extend_from_slice(&[p1 as u8, p2 as u8, p3 as u8, p4 as u8]);
		}
		out
	}

	fn point_data(name: &str, points: &[(f32, f32, f32, f32, i8, i8, i8, i8)]) -> PointDataInterface {
		let path = temp_path(name);
		xops_platform::fs::write_file(&path, &pd1_image(points)).expect("write temp point file");
		let mut data = PointDataInterface::new();
		assert_eq!(
			data.load_file_data(path.to_string_lossy().as_bytes()),
			0,
			"the temp point file loads"
		);
		data
	}

	// --- the pools -------------------------------------------------------

	#[test]
	fn every_pool_is_allocated_at_its_upstream_size_and_starts_empty() {
		let mgr = ObjectManager::new();

		assert_eq!(mgr.human_index.len(), MAX_HUMAN);
		assert_eq!(mgr.weapon_index.len(), MAX_WEAPON);
		assert_eq!(mgr.small_object_index.len(), MAX_SMALLOBJECT);
		assert_eq!(mgr.bullet_index.len(), MAX_BULLET);
		assert_eq!(mgr.grenade_index.len(), MAX_GRENADE);
		assert_eq!(mgr.effect_index.len(), MAX_EFFECT);

		assert_eq!(mgr.bullet_obj_human_index.len(), MAX_BULLET);
		assert_eq!(mgr.get_total_objects(), (0, 0, 0, 0, 0, 0));
		assert_eq!(mgr.get_player_id(), 0);
		assert_eq!(mgr.framecnt, 0);
	}

	#[test]
	fn a_freed_slot_is_the_one_the_next_object_takes() {
		let mut mgr = ObjectManager::new();
		let d3dg = headless();
		let resource = ResourceManager::new();

		// The next human goes into the lowest free slot, which slot 5 being taken does
		// not change.
		mgr.human_index[5].object.set_enable_flag(true);
		let first = mgr.add_human_index(0.0, 0.0, 0.0, 0.0, 0, 1, [0, 0], 3, false, &d3dg, &resource);
		assert_eq!(first, 0);
		let second = mgr.add_human_index(0.0, 0.0, 0.0, 0.0, 0, 1, [0, 0], 3, false, &d3dg, &resource);
		assert_eq!(second, 1);

		// Human type 0 has 120 health and the team it was placed with.
		let human = mgr.get_human_object(0).expect("human 0");
		assert_eq!(human.hp(), 120);
		assert_eq!(human.get_param_data(), (0, -1, 3, 1));
		assert_eq!(human.pos_data().0, 0.0);

		mgr.bullet_index[7].object.set_enable_flag(true);
		assert_eq!(mgr.get_total_objects(), (3, 0, 0, 1, 0, 0));

		// A cleanup frees every pool, and the next add starts from slot 0 again.
		mgr.cleanup(&mut headless(), &mut ResourceManager::new());
		assert_eq!(mgr.get_total_objects(), (0, 0, 0, 0, 0, 0));

		let again = mgr.add_human_index(0.0, 0.0, 0.0, 0.0, 0, 1, [0, 0], 3, false, &d3dg, &resource);
		assert_eq!(again, 0);
	}

	#[test]
	fn the_effect_pool_hands_out_slots_in_order_and_takes_them_back() {
		let mut mgr = ObjectManager::new();

		let count = 3;
		let add = |mgr: &mut ObjectManager, x: f32, count: i32| {
			mgr.add_effect(
				x, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7, 0, 0, 1.0, 0.0,
				effect_type::NORMAL, count,
			)
		};

		assert_eq!(add(&mut mgr, 0.0, count), 0);
		assert_eq!(add(&mut mgr, 0.0, count), 1);
		assert_eq!(mgr.get_total_objects().5, 2);

		// A three frame effect survives three frames and retires on the fourth.
		assert_eq!(mgr.effect_index[0].count(), 3);
		for _ in 0..3 {
			assert_eq!(mgr.effect_index[0].process_object(), 1, "still counting down");
		}
		assert!(mgr.effect_index[0].object.enable_flag());
		assert_eq!(mgr.effect_index[0].count(), 0);
		assert_eq!(mgr.effect_index[0].process_object(), 2, "the count ran out");
		assert!(!mgr.effect_index[0].object.enable_flag());
		assert_eq!(mgr.get_total_objects().5, 1);

		// The retired slot is the next one handed out.
		assert_eq!(add(&mut mgr, 0.0, count), 0);
	}

	// --- the point data walk ---------------------------------------------

	#[test]
	fn the_point_data_walk_builds_the_humans_weapons_and_objects_the_mission_asks_for() {
		let mut mgr = ObjectManager::new();
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let (coll, blocks) = empty_world();

		// Human type 2 is 'special green A': 110 health, texture 16, and the weapons 3
		// and 6. The search matches the info point's p4 against the human point's p2, and
		// the type itself is read from the info point's p2, so both carry the number.
		let points = [
			(0.0, 0.0, 0.0, 0.0, POINT_P1TYPE_HUMANINFO as i8, 2, 1, 2),
			(10.0, 2.0, 20.0, 1.5, POINT_P1TYPE_HUMAN as i8, 2, 0, 3),
			(30.0, 0.0, 40.0, 0.0, POINT_P1TYPE_WEAPON as i8, 5, 7, 0),
			(50.0, 0.0, 60.0, 0.0, POINT_P1TYPE_SMALLOBJ as i8, 1, 0, 9),
		];
		let points = point_data("walk.pd1", &points);

		mgr.set_class(&resource);
		mgr.load_point_data(&mut d3dg, &mut resource, &points, &coll, &blocks, None);

		// The human, placed where the mission says and described by its info point. The
		// mission walk asks for the random correction, so the position is within a tenth
		// of a unit of the point and the angle within 0.8 degrees.
		let human = mgr.get_human_object(0).expect("the human");
		let (px, py, pz, prx) = human.pos_data();
		assert!(human.enable_flag());
		assert!((px - 10.0).abs() <= 0.1, "{px}");
		assert!((pz - 20.0).abs() <= 0.1, "{pz}");
		assert!((prx - 1.5).abs() <= degree_to_radian(0.8), "{prx}");
		assert_eq!(py, 2.0);
		assert_eq!(human.hp(), 110);
		// The data number is the human's own point index in the file, which is 1.
		assert_eq!(human.get_param_data(), (2, 1, 3, 1));

		// Its texture could not be loaded from a temp directory, so it retries every
		// frame, which is the mystery human path.
		assert_eq!(resource.get_human_texture(2), -1);
		assert!(mgr.human_frame_texture_refresh[0]);

		// The two weapons the human type carries went into the first two weapon slots,
		// and the weapon standing on its own into the third.
		let (selectweapon, weapon, _, _) = human.get_weapon();
		assert_eq!(selectweapon, 1);
		assert_eq!(weapon, [Some(0), Some(1)]);
		assert!(mgr.weapon_index[0].using_flag(), "a held weapon is in use");
		assert_eq!(mgr.weapon_index[0].param_data().0, 3);
		assert_eq!(mgr.weapon_index[1].param_data().0, 6);

		let standing = mgr.get_weapon_object(2).expect("the standing weapon");
		assert!(standing.object.enable_flag());
		assert!(!standing.using_flag());
		assert_eq!(standing.param_data().0, 5);
		assert_eq!(standing.object.pos_data(), (30.0, 0.0, 40.0, 0.0));

		// The small object, with the identifier its point carried and the table's health.
		let prop = mgr.get_small_object(0).expect("the small object");
		assert_eq!(prop.param_data(), (1, 9));
		assert_eq!(prop.hp(), 60);
		assert_eq!(prop.object.pos_data(), (50.0, 0.0, 60.0, 0.0));

		assert_eq!(mgr.get_total_objects(), (1, 3, 1, 0, 0, 0));
	}

	#[test]
	fn a_human_point_with_no_info_point_becomes_the_black_human_and_the_player() {
		let mut mgr = ObjectManager::new();
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let (coll, blocks) = empty_world();

		// p4 of 0 marks the human the player controls.
		let points = [(5.0, 0.0, 6.0, 0.0, POINT_P1TYPE_HUMAN2 as i8, 7, 3, 0)];
		let points = point_data("noinfo.pd1", &points);
		mgr.load_point_data(&mut d3dg, &mut resource, &points, &coll, &blocks, None);

		let human = mgr.get_human_object(0).expect("the human");
		assert_eq!(human.get_param_data(), (0, -1, 0, 0));
		assert_eq!(human.hp(), 120);
		assert_eq!(mgr.get_player_id(), 0);

		// A HUMAN2 point carries one weapon, and the second is disabled by the fallback.
		let (_, weapon, _, _) = human.get_weapon();
		assert_eq!(weapon, [Some(0), None]);
	}

	#[test]
	fn a_later_duplicate_identifier_resolves_to_the_first_human_in_the_file() {
		let mut mgr = ObjectManager::new();
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let (coll, blocks) = empty_world();

		// Both humans have p4 of 4, and the search returns the first one in the file.
		let points = [
			(0.0, 0.0, 0.0, 0.0, POINT_P1TYPE_HUMANINFO as i8, 2, 1, 2),
			(1.0, 0.0, 0.0, 0.0, POINT_P1TYPE_HUMAN as i8, 2, 0, 4),
			(2.0, 0.0, 0.0, 0.0, POINT_P1TYPE_HUMAN as i8, 2, 0, 4),
		];
		let points = point_data("dupe.pd1", &points);
		mgr.load_point_data(&mut d3dg, &mut resource, &points, &coll, &blocks, None);

		assert_eq!(mgr.search_human(4), Some(0));
		let second = mgr.get_human_object(1).expect("the second human");
		assert!((second.pos_data().0 - 2.0).abs() <= 0.1, "{}", second.pos_data().0);
		assert_eq!(mgr.search_human(99), None);
	}

	// --- the pickup ------------------------------------------------------

	#[test]
	fn the_pickup_range_ends_exactly_at_its_two_limits() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);

		// At the radius but not inside it.
		place_weapon(&mut mgr, 1, HUMAN_PICKUPWEAPON_R, 0.0, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 1);
		assert!(!mgr.weapon_index[1].using_flag(), "3.8 is not inside 3.8");

		// A hundredth inside it is.
		place_weapon(&mut mgr, 2, HUMAN_PICKUPWEAPON_R - 0.01, 0.0, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 2);
		assert!(mgr.weapon_index[2].using_flag());

		// At the upper height limit.
		place_weapon(&mut mgr, 3, 0.0, HUMAN_PICKUPWEAPON_H, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 3);
		assert!(mgr.weapon_index[3].using_flag());

		// A tenth above it is not.
		place_weapon(&mut mgr, 4, 0.0, HUMAN_PICKUPWEAPON_H + 0.1, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 4);
		assert!(!mgr.weapon_index[4].using_flag());

		// At the lower height limit.
		place_weapon(&mut mgr, 5, 0.0, HUMAN_PICKUPWEAPON_L, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 5);
		assert!(mgr.weapon_index[5].using_flag());

		// A tenth below it is not.
		place_weapon(&mut mgr, 6, 0.0, HUMAN_PICKUPWEAPON_L - 0.1, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 6);
		assert!(!mgr.weapon_index[6].using_flag());

		// The distance is flat: height does not make up for it.
		place_weapon(&mut mgr, 7, HUMAN_PICKUPWEAPON_R + 0.5, 16.0, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 7);
		assert!(!mgr.weapon_index[7].using_flag());
	}

	#[test]
	fn a_zombie_and_a_corpse_pick_nothing_up() {
		let mut mgr = ObjectManager::new();

		// A zombie.
		let zombie = human_type_of(2);
		mgr.human_index[0].set_param_data(zombie, 0, 0, 0, true);
		mgr.human_index[0].set_pos_data(0.0, 0.0, 0.0, 0.0);
		mgr.human_index[0].object.set_enable_flag(true);
		place_weapon(&mut mgr, 0, 0.0, 0.0, 0.0);
		mgr.pickup_weapon(0, 0);
		assert!(!mgr.weapon_index[0].using_flag(), "a zombie does not pick up");

		// A dead human.
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
		mgr.human_index[0].hit_grenade_explosion(200);
		// The upstream does not clamp the health, so it goes below zero rather than to it.
		assert_eq!(mgr.human_index[0].hp(), -80);
		place_weapon(&mut mgr, 0, 0.0, 0.0, 0.0);
		clear_hands(&mut mgr);
		mgr.pickup_weapon(0, 0);
		assert!(!mgr.weapon_index[0].using_flag(), "a corpse does not pick up");

		// A robot does: only the zombie type is refused.
		let robot = human_type_of(1);
		mgr.human_index[0].set_param_data(robot, 0, 0, 0, true);
		mgr.human_index[0].set_pos_data(0.0, 0.0, 0.0, 0.0);
		mgr.human_index[0].object.set_enable_flag(true);
		place_weapon(&mut mgr, 0, 0.0, 0.0, 0.0);
		mgr.pickup_weapon(0, 0);
		assert!(mgr.weapon_index[0].using_flag());
	}

	// --- damage ----------------------------------------------------------

	#[test]
	fn the_hit_location_picks_the_damage_multiplier() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(false);
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 0, 0.0);

		// Attacks of 20, and the three bands do not overlap. The base is the multiplier
		// applied in f32 and truncated, which is 40, 20 and 14, and the human adds its own
		// roll on top: up to 15 for the head, 7 for the body and 5 for the legs.
		for (hit_id, base, roll) in [(0, 40, 15), (1, 20, 7), (2, 14, 5)] {
			place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
			let before = mgr.human_index[0].hp();
			assert_eq!(before, 120);

			mgr.hit_bullet_human(
				0,
				hit_id,
				0.0,
				0.0,
				0.0,
				0.0,
				20,
				1,
				1.0,
				&resource,
				&mut sound(),
				&config,
			);

			let lost = before - mgr.human_index[0].hp();
			assert!(
				(base..=base + roll).contains(&lost),
				"hit {hit_id} cost {lost}, outside {base}..={}",
				base + roll
			);
		}
	}

	#[test]
	fn the_blood_effect_count_is_the_damage_divided_by_ten() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(true);
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 0, 0.0);

		// One effect is the spray and the rest are one drop per ten points of damage. At
		// 20 attacks the truncated damage is 40, 20 and 14, so the drops are 4, 2 and 1.
		for (hit_id, effects) in [(0, 5), (1, 3), (2, 2)] {
			for effect in mgr.effect_index.iter_mut() {
				effect.object.set_enable_flag(false);
			}
			place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);

			mgr.hit_bullet_human(
				0,
				hit_id,
				0.0,
				0.0,
				0.0,
				0.0,
				20,
				1,
				1.0,
				&resource,
				&mut sound(),
				&config,
			);

			assert_eq!(mgr.get_total_objects().5, effects, "hit {hit_id}");
		}
	}

	#[test]
	fn the_blood_spray_is_not_drawn_when_the_setting_is_off() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 0, 0.0);
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);

		mgr.hit_bullet_human(
			0,
			0,
			0.0,
			0.0,
			0.0,
			0.0,
			20,
			1,
			1.0,
			&resource,
			&mut sound(),
			&config(false),
		);

		// The damage still lands, only the effects are missing.
		assert_eq!(mgr.get_total_objects().5, 0);
		assert!(mgr.human_index[0].hp() < 120);
	}

	#[test]
	fn a_hit_scores_for_the_human_that_fired_it() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(false);
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 1, 0.0);

		mgr.hit_bullet_human(
			1,
			0,
			0.0,
			0.0,
			0.0,
			0.0,
			20,
			0,
			0.5,
			&resource,
			&mut sound(),
			&config,
		);

		assert_eq!(mgr.get_human_shot_info(0), Some((0.5, 0, 1)));

		// A killing blow counts as a kill, and the head hit is already counted.
		mgr.human_index[1].set_hp(30);
		mgr.hit_bullet_human(
			1,
			0,
			0.0,
			0.0,
			0.0,
			0.0,
			20,
			0,
			0.5,
			&resource,
			&mut sound(),
			&config,
		);
		let (ontarget, kill, headshot) = mgr.get_human_shot_info(0).expect("human 0");
		assert_eq!(kill, 1);
		assert_eq!(headshot, 2);
		assert_eq!(ontarget, 1.0);
		assert!(mgr.human_index[1].hp() <= 0);
	}

	// --- the grenade -----------------------------------------------------

	#[test]
	fn the_grenade_falloff_runs_from_the_centre_to_the_maximum_distance() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(false);
		let (coll, blocks) = empty_world();
		let mut shots = Vec::new();

		// The blast is computed twice per human, at the feet and at the head, and the two
		// are added. With the human and the grenade in the same spot that is
		// (80 - 2) + (100 - 22) = 156, and the wind pressure is
		// 2.2 / 80 * (80 - 1) = 2.1725 upwards.
		for (gx, expected_damage, expected_hit) in [
			(0.0, 156, true),
			(79.0, 1, true),
			(MAX_DAMAGE_GRENADE_DISTANCE, 0, false),
			(110.0, 0, false),
		] {
			place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
			mgr.human_index[0].set_hp(500);
			mgr.human_index[0].object.set_enable_flag(true);

			mgr.grenade_index[0].set_pos_data(gx, 0.0, 0.0, 0.0, 0.0);
			mgr.grenade_index[0].set_param_data(8.0, 0, 1, 1.0, true);
			mgr.grenade_index[0].bullet.object.set_enable_flag(true);

			let hit = mgr.grenade_explosion(
				0,
				&coll,
				&blocks,
				None,
				&resource,
				&mut sound(),
				&config,
			);

			assert_eq!(hit, expected_hit, "at {gx}");
			assert_eq!(500 - mgr.human_index[0].hp(), expected_damage, "at {gx}");
			shots.push(mgr.human_index[0].move_pos().1);
		}

		// The blast throws the body up, and only the point blank one is worth pinning.
		assert!((shots[0] - 2.1725).abs() < 1e-3, "{}", shots[0]);
		assert_eq!(shots[2], 0.0, "a blast that does nothing moves nothing");
	}

	#[test]
	fn the_grenade_falloff_is_checked_against_a_small_object_too() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(false);
		let (coll, blocks) = empty_world();

		// Small object 1 is the pc, with 60 health. The blast damage is
		// 80 - (80 / 80 * r), so one unit out is 79 and it is destroyed, and 79 units out
		// is the single point of damage the falloff still allows.
		let blast = |mgr: &mut ObjectManager, x: f32| {
			mgr.small_object_index[0].object.set_pos_data(x, 0.0, 0.0, 0.0);
			mgr.small_object_index[0].set_param_data(1, 5, true, None);
			mgr.small_object_index[0].object.set_enable_flag(true);

			mgr.grenade_index[0].set_pos_data(0.0, 0.0, 0.0, 0.0, 0.0);
			mgr.grenade_index[0].set_param_data(8.0, 0, 1, 1.0, true);

			mgr.grenade_explosion(0, &coll, &blocks, None, &resource, &mut sound(), &config)
		};

		assert!(blast(&mut mgr, 1.0));
		// A destroyed object reports zero rather than a negative, because the destruction
		// state is what the draw reads.
		assert_eq!(mgr.small_object_index[0].hp(), 0);

		assert!(blast(&mut mgr, 79.0));
		assert_eq!(mgr.small_object_index[0].hp(), 60 - (80 - 79));

		// At exactly the maximum distance the falloff has reached zero damage. The object
		// is left alone, but the blast still counts as having reached it, because the
		// upstream test is the ray and not the damage.
		assert!(blast(&mut mgr, MAX_DAMAGE_GRENADE_DISTANCE));
		assert_eq!(mgr.small_object_index[0].hp(), 60);
	}

	// --- the team checks -------------------------------------------------

	#[test]
	fn the_zombie_reach_is_3_3_units_on_the_flat_and_one_body_height_up() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
		place_human(&mut mgr, 1, 0.0, 0.0, 2.0, 1, 0.0);

		// The attack point is two units in front, and the target is inside it.
		assert!(mgr.check_zombie_attack(0, 1));

		// Just inside the reach.
		mgr.human_index[1].set_pos_data(0.0, 0.0, 2.0 + 3.29, 0.0);
		assert!(mgr.check_zombie_attack(0, 1));

		// Just outside it.
		mgr.human_index[1].set_pos_data(0.0, 0.0, 2.0 + 3.31, 0.0);
		assert!(!mgr.check_zombie_attack(0, 1));

		// A member of the same team is never a target.
		mgr.human_index[1].set_pos_data(0.0, 0.0, 2.0, 0.0);
		mgr.human_index[1].set_param_data(0, 0, 1, 0, true);
		assert!(!mgr.check_zombie_attack(0, 1));
	}

	#[test]
	fn friendly_fire_is_off_until_it_is_set_and_an_id_outside_the_pool_reads_as_off() {
		let mut mgr = ObjectManager::new();

		assert!(!mgr.get_friendly_fire_flag(0));
		mgr.set_friendly_fire_flag(0, true);
		assert!(mgr.get_friendly_fire_flag(0));
		assert!(!mgr.get_friendly_fire_flag(1));

		assert!(!mgr.get_friendly_fire_flag(-1));
		assert!(!mgr.get_friendly_fire_flag(MAX_HUMAN as i32));
		mgr.set_friendly_fire_flag(MAX_HUMAN as i32, true);
		assert!(!mgr.get_friendly_fire_flag(MAX_HUMAN as i32));
	}

	#[test]
	fn a_bullet_passes_through_its_own_team_until_friendly_fire_is_on() {
		let mut mgr = ObjectManager::new();
		let resource = ResourceManager::new();
		let config = config(false);
		let (coll, blocks) = empty_world();

		// The shooter is human 1, the target human 0, and both are on team 0. The bullet
		// leaves the shooter one unit off the ground and travels one step of
		// BULLET_SPEEDSCALE, which puts it inside the target's leg cylinder.
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 0, 0.0);
		place_human(&mut mgr, 0, 2.0, 0.0, 0.0, 0, 0.0);

		let fire = |mgr: &mut ObjectManager| {
			mgr.bullet_index[0].set_pos_data(0.0, 1.5, 0.0, 0.0, 0.0);
			mgr.bullet_index[0].set_param_data(20, 0, BULLET_SPEEDSCALE, 0, 1, 1.0, true);
			mgr.bullet_index[0].object.set_enable_flag(true);
			mgr.bullet_obj_human_index[0].init();
			mgr.collide_bullet(0, &coll, &blocks, None, &resource, &mut sound(), &config)
		};

		assert!(!fire(&mut mgr), "an ally is not hit");
		assert_eq!(mgr.human_index[0].hp(), 120);

		mgr.set_friendly_fire_flag(0, true);
		assert!(fire(&mut mgr), "with friendly fire on it is a hit");
		let lost = 120 - mgr.human_index[0].hp();
		assert!((14..=19).contains(&lost), "leg damage was {lost}");
	}

	// --- the game state --------------------------------------------------

	#[test]
	fn the_game_is_clear_when_the_enemy_side_has_no_health_left() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 0, 0.0);
		place_human(&mut mgr, 1, 0.0, 0.0, 0.0, 1, 0.0);

		// An enemy is alive.
		assert_eq!(mgr.check_game_overor_complete(), 0);

		// Its health runs out.
		mgr.human_index[1].hit_grenade_explosion(1000);
		assert!(mgr.human_index[1].hp() <= 0);
		assert_eq!(mgr.check_game_overor_complete(), 1);
	}

	#[test]
	fn the_game_is_over_when_the_player_is_a_corpse_and_a_draw_is_a_clear() {
		let mut mgr = ObjectManager::new();
		let mut d3dg = headless();
		let resource = ResourceManager::new();
		let config = config(false);
		// A floor, so the human that has to survive the fall stands on something.
		let (blocks, coll) = one_cube_map("gameover.bd1");
		let mut sound = sound();

		let alive = human_type_of(0);
		mgr.human_index[0].set_pos_data(0.5, 2.3, 0.5, 0.0);
		mgr.human_index[0].set_param_data(alive, 0, 0, 0, true);
		mgr.human_index[0].object.set_enable_flag(true);

		// No enemies at all is a clear, not a game over.
		assert_eq!(mgr.check_game_overor_complete(), 1);

		// An enemy keeps the mission going until the player falls.
		mgr.human_index[1].set_pos_data(0.5, 2.3, 0.5, 0.0);
		mgr.human_index[1].set_param_data(alive, 0, 0, 1, true);
		mgr.human_index[1].object.set_enable_flag(true);
		assert_eq!(mgr.check_game_overor_complete(), 0);

		// Losing all health is not yet a game over: the body is still falling over, and
		// the upstream waits for the motion to settle.
		mgr.human_index[0].set_hit_flag(0.0);
		mgr.human_index[0].hit_grenade_explosion(1000);
		assert!(mgr.human_index[0].hp() <= 0);
		assert_eq!(mgr.check_game_overor_complete(), 0);

		// Which it does over the next frames of the death motion.
		let mut frames = 0;
		while frames < 400 && !mgr.human_index[0].dead_flag() {
			mgr.process(
				-1,
				false,
				false,
				&mut d3dg,
				&resource,
				&coll,
				&blocks,
				None,
				&mut sound,
				&config,
			);
			frames += 1;
		}

		assert!(mgr.human_index[0].dead_flag(), "the fall ended in {frames} frames");
		assert_eq!(mgr.check_game_overor_complete(), 2);
	}

	#[test]
	fn the_object_info_tag_names_the_closest_thing_in_view_and_its_team() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 1, 10.0, 0.0, 0.0, 1, 0.0);
		// The player looks along +X from the origin, and the human is straight ahead: the
		// angle to a target is atan2(z, x) - camera_rx, so a camera angle of zero points
		// down +X.
		mgr.human_index[0].set_param_data(0, 0, 0, 0, true);
		mgr.human_index[0].object.set_enable_flag(true);

		let (color, text) = mgr
			.get_object_info_tag(0.0, 0.0, 0.0, 0.0, 0.0)
			.expect("the human is in view");
		assert_eq!(text, b"Human[1]  HP 120 : Enemy");
		assert_eq!(color, D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0));

		// The same human on the player's team reads as a friend.
		mgr.human_index[1].set_param_data(0, 0, 1, 0, true);
		let (color, text) = mgr
			.get_object_info_tag(0.0, 0.0, 0.0, 0.0, 0.0)
			.expect("the human is in view");
		assert_eq!(text, b"Human[1]  HP 120 : Friend");
		assert_eq!(color, D3DGraphics::get_color_code(0.0, 0.0, 1.0, 1.0));

		// Looking the other way finds nothing.
		assert!(mgr.get_object_info_tag(0.0, 0.0, 0.0, PI, 0.0).is_none());

		// And neither does a target past the fifty unit reach.
		assert!(mgr.get_object_info_tag(-100.0, 0.0, 0.0, 0.0, 0.0).is_none());
	}

	// --- the sort --------------------------------------------------------

	#[test]
	fn the_effects_are_sorted_farthest_first_and_equal_distances_keep_their_order() {
		let mut mgr = ObjectManager::new();

		let add = |mgr: &mut ObjectManager, x: f32, z: f32| {
			mgr.add_effect(
				x, 0.0, z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 7, 0, 0, 1.0, 0.0,
				effect_type::NORMAL, 100,
			)
		};

		add(&mut mgr, 10.0, 0.0); // 0: 100 away squared
		add(&mut mgr, 0.0, 0.0); // 1: on the camera
		add(&mut mgr, 20.0, 0.0); // 2: 400 away squared
		add(&mut mgr, 0.0, 5.0); // 3: 25 away squared
		add(&mut mgr, 5.0, 0.0); // 4: also 25, added later

		let mut data = [Effectdata::default(); MAX_EFFECT];
		let count = mgr.sort_effect(0.0, 0.0, 0.0, &mut data);
		assert_eq!(count, 5);

		let ids: Vec<i32> = data.iter().take(count).map(|entry| entry.id).collect();
		let dists: Vec<f32> = data.iter().take(count).map(|entry| entry.dist).collect();
		assert_eq!(ids, vec![2, 0, 3, 4, 1]);
		assert_eq!(dists, vec![400.0, 100.0, 25.0, 25.0, 0.0]);

		// Disabled effects are not in the list at all.
		mgr.effect_index[2].object.set_enable_flag(false);
		let count = mgr.sort_effect(0.0, 0.0, 0.0, &mut data);
		assert_eq!(count, 4);
		assert_eq!(data[0].id, 0);
	}
}
