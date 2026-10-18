//! The fixed setting tables from the upstream `parameter.cpp`.
//!
//! These are the preset values the game is tuned around: human, weapon, small object,
//! bullet, official mission, AI level and scope settings. Every number is transcribed
//! from upstream and is game feel, so none of them are tidied or adjusted.
//!
//! Upstream `ParameterInfo` allocates the tables in `InitInfo()` and frees them in
//! `DestroyInfo()`. They never change over the life of the process, so here they are
//! `const` items and those two functions have nothing left to do.
//!
//! Strings are Shift-JIS byte sequences upstream and are kept as `&'static [u8]`, never
//! `String`, because the texture font indexes glyphs by byte value. In this particular
//! translation unit every literal turned out to be plain ASCII (see the port notes), so
//! the bytes on screen and the bytes in the file agree.

use crate::core::{degree_to_radian, ENABLE_BUG_HUMANWEAPON, TOTAL_HAVEWEAPON};

/// Number of human settings.
pub const TOTAL_PARAMETERINFO_HUMAN: usize = 43;
/// Total number of human textures.
pub const TOTAL_HUMANTEXTURE: usize = 30;
/// Number of weapon settings.
pub const TOTAL_PARAMETERINFO_WEAPON: usize = 23;
/// Number of small object settings.
pub const TOTAL_PARAMETERINFO_SMALLOBJECT: usize = 12;
/// Number of bullet object settings.
pub const TOTAL_PARAMETERINFO_BULLET: usize = 2;
/// Number of official missions.
pub const TOTAL_OFFICIALMISSION: usize = 58;
/// Number of AI level settings.
pub const TOTAL_PARAMETERINFO_AILEVEL: usize = 6;
/// Number of scope settings.
pub const TOTAL_PARAMETERINFO_SCOPE: usize = 4;

/// Weapon number treated as none. `Weapon[ID_WEAPON_NONE]` must stay the empty weapon.
pub const ID_WEAPON_NONE: i32 = 0;
/// Weapon number treated as a hand grenade. `Weapon[ID_WEAPON_GRENADE]` must stay a grenade.
pub const ID_WEAPON_GRENADE: i32 = 13;
/// Weapon number treated as a case. `Weapon[ID_WEAPON_CASE]` must stay a case.
pub const ID_WEAPON_CASE: i32 = 15;

/// BD1 file name of the official mission.
pub const OFFICIALMISSION_BD1: &[u8] = b"temp.bd1";

/// Small object draw scale.
pub const SMALLOBJECT_SCALE: f32 = 5.0;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// Human settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HumanParameter {
	/// texture number
	pub texture: i32,
	/// model number
	pub model: i32,
	/// health
	pub hp: i32,
	/// AI level
	pub ai_level: i32,
	/// tracking ability when the AI attacks (zombies only)
	pub ai_trackability: i32,
	/// weapons
	pub weapon: [i32; TOTAL_HAVEWEAPON],
	/// type
	pub r#type: i32,
}

/// Weapon settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponParameter {
	/// weapon name
	pub name: &'static [u8],
	/// model file name
	pub model: &'static [u8],
	/// texture file name
	pub texture: &'static [u8],
	/// attack power
	pub attacks: i32,
	/// penetration power
	pub penetration: i32,
	/// firing interval
	pub blazings: i32,
	/// bullet speed
	pub speed: i32,
	/// magazine capacity
	pub nbsmax: i32,
	/// reload time
	pub reloads: i32,
	/// recoil angle
	pub reaction: i32,
	/// bullet trajectory error, minimum
	pub error_range_min: i32,
	/// bullet trajectory error, maximum
	pub error_range_max: i32,
	/// gun position X
	pub mx: f32,
	/// gun position Y
	pub my: f32,
	/// gun position Z
	pub mz: f32,
	/// muzzle flash position X
	pub flashx: f32,
	/// muzzle flash position Y
	pub flashy: f32,
	/// muzzle flash position Z
	pub flashz: f32,
	/// spent cartridge ejection position X
	pub yakkyou_px: f32,
	/// spent cartridge ejection position Y
	pub yakkyou_py: f32,
	/// spent cartridge ejection position Z
	pub yakkyou_pz: f32,
	/// spent cartridge ejection initial velocity X
	pub yakkyou_sx: f32,
	/// spent cartridge ejection initial velocity Y
	pub yakkyou_sy: f32,
	/// cartridge draw scale (negative values are computed from the attack power)
	pub yakkyou_size: f32,
	/// spent cartridge ejection delay count
	pub yakkyou_delay: i32,
	/// number of rounds fired (0: full auto, 1: semi auto, 2 or more: burst mode)
	pub burstcnt: i32,
	/// scope mode
	pub scopemode: i32,
	/// draw scale
	pub size: f32,
	/// gunshot sound number
	pub soundid: i32,
	/// gunshot sound volume
	pub soundvolume: i32,
	/// whether a silencer is present
	pub silencer: bool,
	/// equip method
	pub weapon_p: i32,
	/// weapon to switch to
	pub change_weapon_id: i32,
	/// weapon switch time
	pub change_weapon_cnt: i32,
	/// number of projectiles fired (pellet count)
	pub pellet: i32,
}

/// Small object settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmallObjectParameter {
	/// model name
	pub model: &'static [u8],
	/// texture name
	pub texture: &'static [u8],
	/// collision detection size
	pub decide: i32,
	/// durability
	pub hp: i32,
	/// hit sound effect number
	pub sound: i32,
	/// how far it jumps
	pub jump: i32,
}

/// Bullet object settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BulletParameter {
	/// model name
	pub model: &'static [u8],
	/// texture name
	pub texture: &'static [u8],
	/// draw scale
	pub size: f32,
}

/// Official mission settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OfficialMissionParameter {
	/// mission identifier
	pub name: &'static [u8],
	/// mission official name
	pub fullname: &'static [u8],
	/// data directory
	pub directory: &'static [u8],
	/// file name (.pd1/.txt)
	pub txt: &'static [u8],
	/// flag indicating an additional collision check
	pub collision: bool,
	/// flag to darken the screen
	pub screen: bool,
}

/// AI level settings (performance values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AIParameter {
	/// aiming ability
	pub aiming: i32,
	/// aggressiveness
	pub attack: i32,
	/// detection ability
	pub search: i32,
	/// allowed error
	pub limitserror: i32,
}

/// Scope settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScopeParameter {
	/// field of view angle
	pub view_angle: f32,
	/// input sensitivity
	pub mouse_range: f32,
	/// reticle display when using the scope (0: normal, 1: simple, 2: precision)
	pub scope_gunsight: i32,
	/// reticle display in the normal view
	pub no_scope_gunsight: bool,
	/// recoil when firing normally
	pub weapon_no_scope_reaction: i32,
	/// recoil when firing (motion)
	pub weapon_motion_reaction_y: f32,
	/// scope recoil when firing, only while scoped
	pub weapon_recoil_scope_only: bool,
	/// scope recoil when firing, horizontal axis (degrees, in 0.1f steps)
	pub weapon_recoil_scope_rx: f32,
	/// scope recoil when firing, vertical axis minimum (degrees, in 0.1f steps)
	pub weapon_recoil_scope_ry_min: f32,
	/// scope recoil when firing, vertical axis maximum (degrees, in 0.1f steps)
	pub weapon_recoil_scope_ry_max: f32,
	/// angle within which the AI acquires an enemy when firing
	pub ai_shot_angle: f32,
	/// angle within which the AI acquires an enemy when firing (long range)
	pub ai_shot_angle_long: f32,
	/// AI enemy detection distance, additive term (normal state)
	pub ai_add_search_enemy_max_dist_normal: f32,
	/// AI enemy detection distance, additive term (alert state)
	pub ai_add_search_enemy_max_dist_caution: f32,
}

// ---------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------

/// Upstream `Scope[0]`. `Scope[3]` is assigned from it, and in Rust a const
/// cannot refer to itself, so it is named here and used twice.
pub const SCOPE_PARAMETER_0: ScopeParameter = ScopeParameter {
	// no scope
	view_angle: degree_to_radian(65.0),
	mouse_range: 0.0100,
	scope_gunsight: 0,
	no_scope_gunsight: true,
	weapon_no_scope_reaction: 0,
	weapon_motion_reaction_y: -1.0,
	weapon_recoil_scope_only: false,
	weapon_recoil_scope_rx: 0.0,
	weapon_recoil_scope_ry_min: 0.0,
	weapon_recoil_scope_ry_max: 0.0,
	ai_shot_angle: degree_to_radian(8.0),
	ai_shot_angle_long: degree_to_radian(4.0),
	ai_add_search_enemy_max_dist_normal: 0.0,
	ai_add_search_enemy_max_dist_caution: 0.0,
};

/// Upstream `Human`. 43 entries.
pub const HUMAN_PARAMETER: [HumanParameter; TOTAL_PARAMETERINFO_HUMAN] = [
	HumanParameter {
		// special black A
		texture: 14,
		model: 5,
		hp: 120,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [21, 8],
		r#type: 0,
	},
	HumanParameter {
		// special black B
		texture: 14,
		model: 5,
		hp: 120,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [21, 2],
		r#type: 0,
	},
	HumanParameter {
		// special green A
		texture: 16,
		model: 0,
		hp: 110,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [3, 6],
		r#type: 0,
	},
	HumanParameter {
		// special green B
		texture: 16,
		model: 0,
		hp: 110,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [3, 10],
		r#type: 0,
	},
	HumanParameter {
		// special green C
		texture: 16,
		model: 0,
		hp: 110,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [3, 2],
		r#type: 0,
	},
	HumanParameter {
		// special white A
		texture: 18,
		model: 0,
		hp: 110,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [4, 7],
		r#type: 0,
	},
	HumanParameter {
		// special white B
		texture: 18,
		model: 0,
		hp: 110,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [4, 2],
		r#type: 0,
	},
	HumanParameter {
		// bald
		texture: 5,
		model: 0,
		hp: 100,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [4, 0],
		r#type: 0,
	},
	HumanParameter {
		// special purple
		texture: 17,
		model: 0,
		hp: 110,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [22, 14],
		r#type: 0,
	},
	HumanParameter {
		// special blue
		texture: 15,
		model: 5,
		hp: 120,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [3, 1],
		r#type: 0,
	},
	HumanParameter {
		// combat robot
		texture: 13,
		model: 0,
		hp: 2500,
		ai_level: 4,
		ai_trackability: -1,
		weapon: [6, 12],
		r#type: 1,
	},
	HumanParameter {
		// suit black SG
		texture: 4,
		model: 1,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [18, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit black
		texture: 4,
		model: 0,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [5, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit gray
		texture: 11,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// police officer
		texture: 8,
		model: 2,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [3, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit brown
		texture: 9,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// shirt man 1
		texture: 23,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [17, 0],
		r#type: 0,
	},
	HumanParameter {
		// Middle Eastern soldier
		texture: 6,
		model: 0,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [17, 10],
		r#type: 0,
	},
	HumanParameter {
		// woman
		texture: 25,
		model: 3,
		hp: 90,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// blond man
		texture: 0,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// citizen 1
		texture: 1,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// citizen 2
		texture: 2,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// shirt man 1 SG
		texture: 23,
		model: 1,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// blond man SG
		texture: 0,
		model: 1,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// citizen 1 SG
		texture: 1,
		model: 1,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// citizen 2 SG
		texture: 2,
		model: 1,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// soldier 1 A
		texture: 19,
		model: 4,
		hp: 115,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [5, 9],
		r#type: 0,
	},
	HumanParameter {
		// soldier 1 B
		texture: 20,
		model: 4,
		hp: 115,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [5, 12],
		r#type: 0,
	},
	HumanParameter {
		// soldier 2
		texture: 21,
		model: 4,
		hp: 115,
		ai_level: 2,
		ai_trackability: -1,
		weapon: [13, 10],
		r#type: 0,
	},
	HumanParameter {
		// zombie 1
		texture: 26,
		model: 0,
		hp: 1000,
		ai_level: 0,
		ai_trackability: 1,
		weapon: [0, 0],
		r#type: 2,
	},
	HumanParameter {
		// zombie 2
		texture: 27,
		model: 0,
		hp: 1000,
		ai_level: 0,
		ai_trackability: 3,
		weapon: [0, 0],
		r#type: 2,
	},
	HumanParameter {
		// zombie 3
		texture: 28,
		model: 0,
		hp: 1000,
		ai_level: 0,
		ai_trackability: 0,
		weapon: [0, 0],
		r#type: 2,
	},
	HumanParameter {
		// zombie 4
		texture: 29,
		model: 3,
		hp: 950,
		ai_level: 0,
		ai_trackability: 2,
		weapon: [0, 0],
		r#type: 2,
	},
	HumanParameter {
		// suit navy
		texture: 12,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit navy SG
		texture: 12,
		model: 1,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [17, 0],
		r#type: 0,
	},
	HumanParameter {
		// general
		texture: 7,
		model: 0,
		hp: 95,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [17, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit blue
		texture: 10,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// suit blue SG
		texture: 10,
		model: 1,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [18, 0],
		r#type: 0,
	},
	HumanParameter {
		// shirt man 2 SG
		texture: 24,
		model: 1,
		hp: 100,
		ai_level: 1,
		ai_trackability: -1,
		weapon: [6, 0],
		r#type: 0,
	},
	HumanParameter {
		// soldier 3
		texture: 22,
		model: 0,
		hp: 105,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [5, 20],
		r#type: 0,
	},
	HumanParameter {
		// soldier 3 SG
		texture: 22,
		model: 1,
		hp: 105,
		ai_level: 3,
		ai_trackability: -1,
		weapon: [21, 0],
		r#type: 0,
	},
	HumanParameter {
		// Gates
		texture: 3,
		model: 0,
		hp: 100,
		ai_level: 0,
		ai_trackability: -1,
		weapon: [0, 0],
		r#type: 0,
	},
	HumanParameter {
		// Gates SG
		texture: 3,
		model: 1,
		hp: 100,
		ai_level: 4,
		ai_trackability: -1,
		weapon: [22, 0],
		r#type: 0,
	},
];

/// Upstream `HumanTexturePath`. 30 entries.
pub const HUMAN_TEXTURE_PATH: [&[u8]; TOTAL_HUMANTEXTURE] = [
	b"./data/model/civ1.bmp",
	b"./data/model/civ2.bmp",
	b"./data/model/civ3.bmp",
	b"./data/model/gates.bmp",
	b"./data/model/gs.bmp",
	b"./data/model/hage.bmp",
	b"./data/model/islam.bmp",
	b"./data/model/islam2.bmp",
	b"./data/model/police.bmp",
	b"./data/model/riiman.bmp",
	b"./data/model/riiman_b.bmp",
	b"./data/model/riiman_g.bmp",
	b"./data/model/riiman_k.bmp",
	b"./data/model/robot.bmp",
	b"./data/model/soldier_black.bmp",
	b"./data/model/soldier_blue.bmp",
	b"./data/model/soldier_green.bmp",
	b"./data/model/soldier_violet.bmp",
	b"./data/model/soldier_white.bmp",
	b"./data/model/soldier0.bmp",
	b"./data/model/soldier1.bmp",
	b"./data/model/soldier2.bmp",
	b"./data/model/soldier3.bmp",
	b"./data/model/syatu.bmp",
	b"./data/model/syatu2.bmp",
	b"./data/model/woman.bmp",
	b"./data/model/zombie1.bmp",
	b"./data/model/zombie2.bmp",
	b"./data/model/zombie3.bmp",
	b"./data/model/zombie4.bmp",
];

/// Upstream `Weapon`. 23 entries.
pub const WEAPON_PARAMETER: [WeaponParameter; TOTAL_PARAMETERINFO_WEAPON] = [
	WeaponParameter {
		name: b"NONE", // must be set to the ID_WEAPON_NONE constant
		model: b"",
		texture: b"",
		attacks: 0,
		penetration: 0,
		blazings: 0,
		speed: 0,
		nbsmax: 0,
		reloads: 0,
		reaction: 0,
		error_range_min: 0,
		error_range_max: 0,
		mx: 0.0,
		my: 0.0,
		mz: 0.0,
		flashx: 0.0,
		flashy: 0.0,
		flashz: 0.0,
		yakkyou_px: 0.0,
		yakkyou_py: 0.0,
		yakkyou_pz: 0.0,
		yakkyou_sx: 0.0,
		yakkyou_sy: 0.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 9.0,
		soundid: 0,
		soundvolume: 0,
		silencer: false,
		weapon_p: 2,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 0,
	},
	WeaponParameter {
		name: b"MP5",
		model: b"./data/model/weapon/mp5.x",
		texture: b"./data/model/weapon/mp5.bmp",
		attacks: 44,
		penetration: 0,
		blazings: 4,
		speed: 10,
		nbsmax: 30,
		reloads: 50,
		reaction: 6,
		error_range_min: 2,
		error_range_max: 21,
		mx: 8.0,
		my: 9.0,
		mz: 35.0,
		flashx: 8.0,
		flashy: 15.0,
		flashz: 68.0,
		yakkyou_px: 8.0,
		yakkyou_py: 12.0,
		yakkyou_pz: 35.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 7.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 7.5,
		soundid: 0,
		soundvolume: 92,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"PSG1",
		model: b"./data/model/weapon/psg1.x",
		texture: b"./data/model/weapon/psg1.bmp",
		attacks: 70,
		penetration: 2,
		blazings: 7,
		speed: 14,
		nbsmax: 5,
		reloads: 58,
		reaction: 15,
		error_range_min: 0,
		error_range_max: 19,
		mx: 8.0,
		my: 8.0,
		mz: 44.0,
		flashx: 8.0,
		flashy: 13.0,
		flashz: 108.0,
		yakkyou_px: 8.0,
		yakkyou_py: 13.0,
		yakkyou_pz: 38.0,
		yakkyou_sx: 6.0,
		yakkyou_sy: 6.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 2,
		size: 8.0,
		soundid: 1,
		soundvolume: 98,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"M92F",
		model: b"./data/model/weapon/m92f.x",
		texture: b"./data/model/weapon/m92f.bmp",
		attacks: 41,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 15,
		reloads: 40,
		reaction: 10,
		error_range_min: 4,
		error_range_max: 26,
		mx: 2.0,
		my: 7.0,
		mz: 57.0,
		flashx: 2.0,
		flashy: 15.0,
		flashz: 73.0,
		yakkyou_px: 2.0,
		yakkyou_py: 15.0,
		yakkyou_pz: 56.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 7.0,
		soundid: 1,
		soundvolume: 90,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"GLOCK18 SEMI",
		model: b"./data/model/weapon/glock18.x",
		texture: b"./data/model/weapon/glock18.bmp",
		attacks: 38,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 19,
		reloads: 38,
		reaction: 9,
		error_range_min: 4,
		error_range_max: 26,
		mx: 2.0,
		my: 8.0,
		mz: 56.0,
		flashx: 2.0,
		flashy: 13.0,
		flashz: 70.0,
		yakkyou_px: 2.0,
		yakkyou_py: 14.0,
		yakkyou_pz: 54.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 5.0,
		soundid: 0,
		soundvolume: 90,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: 16,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"DESERT EAGLE",
		model: b"./data/model/weapon/de.x",
		texture: b"./data/model/weapon/de.bmp",
		attacks: 68,
		penetration: 1,
		blazings: 5,
		speed: 10,
		nbsmax: 7,
		reloads: 41,
		reaction: 13,
		error_range_min: 4,
		error_range_max: 26,
		mx: 2.0,
		my: 11.0,
		mz: 59.0,
		flashx: 2.0,
		flashy: 14.0,
		flashz: 75.0,
		yakkyou_px: 2.0,
		yakkyou_py: 15.0,
		yakkyou_pz: 56.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 11.0,
		soundid: 2,
		soundvolume: 96,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"MAC10",
		model: b"./data/model/weapon/mac10.x",
		texture: b"./data/model/weapon/mac10.bmp",
		attacks: 47,
		penetration: 0,
		blazings: 3,
		speed: 10,
		nbsmax: 30,
		reloads: 45,
		reaction: 9,
		error_range_min: 4,
		error_range_max: 25,
		mx: 2.0,
		my: 4.0,
		mz: 54.0,
		flashx: 2.0,
		flashy: 15.0,
		flashz: 73.0,
		yakkyou_px: 2.0,
		yakkyou_py: 16.0,
		yakkyou_pz: 52.0,
		yakkyou_sx: 2.0,
		yakkyou_sy: 5.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 5.5,
		soundid: 3,
		soundvolume: 90,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"UMP",
		model: b"./data/model/weapon/ump.x",
		texture: b"./data/model/weapon/ump.bmp",
		attacks: 50,
		penetration: 0,
		blazings: 5,
		speed: 10,
		nbsmax: 25,
		reloads: 50,
		reaction: 7,
		error_range_min: 2,
		error_range_max: 19,
		mx: 8.0,
		my: 6.0,
		mz: 28.0,
		flashx: 8.0,
		flashy: 13.0,
		flashz: 64.0,
		yakkyou_px: 8.0,
		yakkyou_py: 11.0,
		yakkyou_pz: 41.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 6.0,
		soundid: 4,
		soundvolume: 94,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"P90",
		model: b"./data/model/weapon/p90.x",
		texture: b"./data/model/weapon/p90.bmp",
		attacks: 42,
		penetration: 1,
		blazings: 4,
		speed: 11,
		nbsmax: 50,
		reloads: 68,
		reaction: 6,
		error_range_min: 3,
		error_range_max: 18,
		mx: 8.0,
		my: 10.0,
		mz: 30.0,
		flashx: 8.0,
		flashy: 12.0,
		flashz: 59.0,
		yakkyou_px: 8.0,
		yakkyou_py: 4.0,
		yakkyou_pz: 18.0,
		yakkyou_sx: -1.0,
		yakkyou_sy: 0.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 6.5,
		soundid: 3,
		soundvolume: 92,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"M4",
		model: b"./data/model/weapon/m4.x",
		texture: b"./data/model/weapon/m4.bmp",
		attacks: 61,
		penetration: 1,
		blazings: 4,
		speed: 11,
		nbsmax: 30,
		reloads: 54,
		reaction: 8,
		error_range_min: 3,
		error_range_max: 19,
		mx: 9.0,
		my: 6.0,
		mz: 37.0,
		flashx: 9.0,
		flashy: 13.0,
		flashz: 91.0,
		yakkyou_px: 10.0,
		yakkyou_py: 12.0,
		yakkyou_pz: 40.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 6.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 6.8,
		soundid: 3,
		soundvolume: 96,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"AK47",
		model: b"./data/model/weapon/ak47.x",
		texture: b"./data/model/weapon/ak47.bmp",
		attacks: 65,
		penetration: 1,
		blazings: 4,
		speed: 11,
		nbsmax: 30,
		reloads: 52,
		reaction: 9,
		error_range_min: 3,
		error_range_max: 20,
		mx: 9.0,
		my: 11.0,
		mz: 39.0,
		flashx: 9.0,
		flashy: 9.0,
		flashz: 86.0,
		yakkyou_px: 9.0,
		yakkyou_py: 12.0,
		yakkyou_pz: 41.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 5.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 6.0,
		soundid: 1,
		soundvolume: 96,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"AUG",
		model: b"./data/model/weapon/aug.x",
		texture: b"./data/model/weapon/aug.bmp",
		attacks: 60,
		penetration: 1,
		blazings: 5,
		speed: 11,
		nbsmax: 30,
		reloads: 56,
		reaction: 7,
		error_range_min: 2,
		error_range_max: 18,
		mx: 8.0,
		my: 10.0,
		mz: 37.0,
		flashx: 8.0,
		flashy: 12.0,
		flashz: 77.0,
		yakkyou_px: 8.0,
		yakkyou_py: 11.0,
		yakkyou_pz: 15.0,
		yakkyou_sx: 3.0,
		yakkyou_sy: 5.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 1,
		size: 7.8,
		soundid: 4,
		soundvolume: 98,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"M249",
		model: b"./data/model/weapon/m249.x",
		texture: b"./data/model/weapon/m249.bmp",
		attacks: 59,
		penetration: 1,
		blazings: 4,
		speed: 11,
		nbsmax: 100,
		reloads: 85,
		reaction: 7,
		error_range_min: 4,
		error_range_max: 22,
		mx: 8.0,
		my: 12.0,
		mz: 44.0,
		flashx: 9.0,
		flashy: 13.0,
		flashz: 102.0,
		yakkyou_px: 9.0,
		yakkyou_py: 11.0,
		yakkyou_pz: 45.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 7.5,
		soundid: 3,
		soundvolume: 98,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"GRENADE", // must be set to the ID_WEAPON_GRENADE constant
		model: b"./data/model/weapon/grenade.x",
		texture: b"./data/model/weapon/grenade.bmp",
		attacks: 0,
		penetration: 0,
		blazings: 35,
		speed: 0,
		nbsmax: 1,
		reloads: 0,
		reaction: 0,
		error_range_min: 6,
		error_range_max: 20,
		mx: 1.0,
		my: 6.0,
		mz: 57.0,
		flashx: 1.0,
		flashy: 6.0,
		flashz: 57.0,
		yakkyou_px: 0.0,
		yakkyou_py: 0.0,
		yakkyou_pz: 0.0,
		yakkyou_sx: 0.0,
		yakkyou_sy: 0.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 8.0,
		soundid: 0,
		soundvolume: 0,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"MP5SD",
		model: b"./data/model/weapon/mp5sd.x",
		texture: b"./data/model/weapon/mp5sd.bmp",
		attacks: 37,
		penetration: 0,
		blazings: 4,
		speed: 9,
		nbsmax: 30,
		reloads: 50,
		reaction: 6,
		error_range_min: 2,
		error_range_max: 21,
		mx: 7.0,
		my: 8.0,
		mz: 35.0,
		flashx: 8.0,
		flashy: 12.0,
		flashz: 84.0,
		yakkyou_px: 8.0,
		yakkyou_py: 12.0,
		yakkyou_pz: 35.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 6.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 7.5,
		soundid: 13,
		soundvolume: 84,
		silencer: true,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"CASE", // must be set to the ID_WEAPON_CASE constant
		model: b"./data/model/weapon/case.x",
		texture: b"./data/model/weapon/case.bmp",
		attacks: 0,
		penetration: 0,
		blazings: 0,
		speed: 0,
		nbsmax: 0,
		reloads: 0,
		reaction: 0,
		error_range_min: 0,
		error_range_max: 0,
		mx: 15.0,
		my: -2.0,
		mz: 35.0,
		flashx: 0.0,
		flashy: 0.0,
		flashz: 0.0,
		yakkyou_px: 0.0,
		yakkyou_py: 0.0,
		yakkyou_pz: 0.0,
		yakkyou_sx: 0.0,
		yakkyou_sy: 0.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 5.0,
		soundid: 0,
		soundvolume: 0,
		silencer: false,
		weapon_p: 2,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"GLOCK18 FULL",
		model: b"./data/model/weapon/glock18.x",
		texture: b"./data/model/weapon/glock18.bmp",
		attacks: 38,
		penetration: 0,
		blazings: 3,
		speed: 9,
		nbsmax: 19,
		reloads: 38,
		reaction: 9,
		error_range_min: 4,
		error_range_max: 26,
		mx: 2.0,
		my: 8.0,
		mz: 56.0,
		flashx: 2.0,
		flashy: 13.0,
		flashz: 70.0,
		yakkyou_px: 2.0,
		yakkyou_py: 14.0,
		yakkyou_pz: 54.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 5.0,
		soundid: 0,
		soundvolume: 90,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: 4,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"M1911",
		model: b"./data/model/weapon/cg.x",
		texture: b"./data/model/weapon/cg.bmp",
		attacks: 46,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 7,
		reloads: 35,
		reaction: 11,
		error_range_min: 5,
		error_range_max: 26,
		mx: 2.0,
		my: 7.0,
		mz: 61.0,
		flashx: 2.0,
		flashy: 14.0,
		flashz: 74.0,
		yakkyou_px: 2.0,
		yakkyou_py: 16.0,
		yakkyou_pz: 57.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 3.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 6.8,
		soundid: 3,
		soundvolume: 94,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"GLOCK17",
		model: b"./data/model/weapon/glock18.x",
		texture: b"./data/model/weapon/glock17.bmp",
		attacks: 38,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 17,
		reloads: 37,
		reaction: 9,
		error_range_min: 4,
		error_range_max: 26,
		mx: 2.0,
		my: 8.0,
		mz: 56.0,
		flashx: 2.0,
		flashy: 13.0,
		flashz: 70.0,
		yakkyou_px: 2.0,
		yakkyou_py: 14.0,
		yakkyou_pz: 54.0,
		yakkyou_sx: 4.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 5.0,
		soundid: 0,
		soundvolume: 90,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"M1",
		model: b"./data/model/weapon/m1.x",
		texture: b"./data/model/weapon/m1.bmp",
		attacks: 90,
		penetration: 0,
		blazings: 8,
		speed: 9,
		nbsmax: 7,
		reloads: 100,
		reaction: 18,
		error_range_min: 5,
		error_range_max: 23,
		mx: 8.0,
		my: 7.0,
		mz: 45.0,
		flashx: 8.0,
		flashy: 11.0,
		flashz: 94.0,
		yakkyou_px: 8.0,
		yakkyou_py: 11.0,
		yakkyou_pz: 42.0,
		yakkyou_sx: 2.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 7.7,
		soundid: 2,
		soundvolume: 98,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 6,
	},
	WeaponParameter {
		name: b"FAMAS",
		model: b"./data/model/weapon/famas.x",
		texture: b"./data/model/weapon/famas.bmp",
		attacks: 55,
		penetration: 1,
		blazings: 3,
		speed: 11,
		nbsmax: 25,
		reloads: 55,
		reaction: 7,
		error_range_min: 3,
		error_range_max: 19,
		mx: 9.0,
		my: 10.0,
		mz: 32.0,
		flashx: 9.0,
		flashy: 12.0,
		flashz: 70.0,
		yakkyou_px: 9.0,
		yakkyou_py: 11.0,
		yakkyou_pz: 14.0,
		yakkyou_sx: 3.0,
		yakkyou_sy: 5.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 0,
		scopemode: 0,
		size: 8.0,
		soundid: 4,
		soundvolume: 96,
		silencer: false,
		weapon_p: 0,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"MK23",
		model: b"./data/model/weapon/mk23.x",
		texture: b"./data/model/weapon/mk23.bmp",
		attacks: 48,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 12,
		reloads: 42,
		reaction: 10,
		error_range_min: 2,
		error_range_max: 26,
		mx: 2.0,
		my: 11.0,
		mz: 60.0,
		flashx: 2.0,
		flashy: 15.0,
		flashz: 74.0,
		yakkyou_px: 1.0,
		yakkyou_py: 16.0,
		yakkyou_pz: 60.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 5.0,
		soundid: 4,
		soundvolume: 92,
		silencer: false,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
	WeaponParameter {
		name: b"MK23 SD",
		model: b"./data/model/weapon/mk23sd.x",
		texture: b"./data/model/weapon/mk23.bmp",
		attacks: 39,
		penetration: 0,
		blazings: 5,
		speed: 9,
		nbsmax: 12,
		reloads: 43,
		reaction: 9,
		error_range_min: 3,
		error_range_max: 26,
		mx: 2.0,
		my: 11.0,
		mz: 60.0,
		flashx: 2.0,
		flashy: 15.0,
		flashz: 96.0,
		yakkyou_px: 1.0,
		yakkyou_py: 16.0,
		yakkyou_pz: 60.0,
		yakkyou_sx: 5.0,
		yakkyou_sy: 4.0,
		yakkyou_size: -1.0,
		yakkyou_delay: 0,
		burstcnt: 1,
		scopemode: 0,
		size: 5.0,
		soundid: 13,
		soundvolume: 86,
		silencer: true,
		weapon_p: 1,
		change_weapon_id: -1,
		change_weapon_cnt: 0,
		pellet: 1,
	},
];

/// Upstream `BugWeapon`. 1 entries.
pub const BUG_WEAPON_PARAMETER: [WeaponParameter; 1] = [WeaponParameter {
	// data for the special bug weapons
	name: b"BugWeapon",
	model: b"",
	texture: b"",
	attacks: 0,
	penetration: 0,
	blazings: 0,
	speed: 0,
	nbsmax: 0,
	reloads: 0,
	reaction: 0,
	error_range_min: 0,
	error_range_max: 0,
	mx: 0.0,
	my: 0.0,
	mz: 0.0,
	flashx: 0.0,
	flashy: 0.0,
	flashz: 0.0,
	yakkyou_px: 0.0,
	yakkyou_py: 0.0,
	yakkyou_pz: 0.0,
	yakkyou_sx: 0.0,
	yakkyou_sy: 0.0,
	yakkyou_size: -1.0,
	yakkyou_delay: 0,
	burstcnt: 0,
	scopemode: 0,
	size: 9.0,
	soundid: 0,
	soundvolume: 0,
	silencer: false,
	weapon_p: 1,
	change_weapon_id: -1,
	change_weapon_cnt: 0,
	pellet: 0,
}];

/// Upstream `SmallObject`. 12 entries.
pub const SMALL_OBJECT_PARAMETER: [SmallObjectParameter; TOTAL_PARAMETERINFO_SMALLOBJECT] = [
	SmallObjectParameter {
		// can
		model: b"./data/article/can.x",
		texture: b"./data/article/can.bmp",
		decide: 10,
		hp: 6,
		sound: 0,
		jump: 10,
	},
	SmallObjectParameter {
		// personal computer
		model: b"./data/article/pc.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, keyboard on top
		model: b"./data/article/pc2.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, body inverted
		model: b"./data/article/pc3.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, wide
		model: b"./data/article/pc_w.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, wide, keyboard on top
		model: b"./data/article/pc2_w.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, wide, body inverted
		model: b"./data/article/pc3_w.x",
		texture: b"./data/article/pc.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// chair
		model: b"./data/article/isu.x",
		texture: b"./data/article/isu.bmp",
		decide: 56,
		hp: 50,
		sound: 1,
		jump: 6,
	},
	SmallObjectParameter {
		// cardboard box
		model: b"./data/article/dan.x",
		texture: b"./data/article/dan.bmp",
		decide: 28,
		hp: 35,
		sound: 1,
		jump: 8,
	},
	SmallObjectParameter {
		// personal computer, booting
		model: b"./data/article/pc.x",
		texture: b"./data/article/pc_sw.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// personal computer, booting, dark
		model: b"./data/article/pc.x",
		texture: b"./data/article/pc_d.bmp",
		decide: 29,
		hp: 60,
		sound: 0,
		jump: 5,
	},
	SmallObjectParameter {
		// pylon
		model: b"./data/article/cone.x",
		texture: b"./data/article/cone.bmp",
		decide: 35,
		hp: 30,
		sound: 1,
		jump: 7,
	},
];

/// Upstream `Bullet`. 2 entries.
pub const BULLET_PARAMETER: [BulletParameter; TOTAL_PARAMETERINFO_BULLET] = [
	BulletParameter {
		// bullet object
		model: b"./data/model/bullet.x",
		texture: b"./data/model/bullet.bmp",
		size: 1.0,
	},
	BulletParameter {
		// hand grenade object
		model: WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].model,
		texture: WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].texture,
		size: WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].size,
	},
];

/// Upstream `MissionData`. 58 entries.
pub const OFFICIAL_MISSION_PARAMETER: [OfficialMissionParameter; TOTAL_OFFICIALMISSION] = [
	OfficialMissionParameter {
		name: b"TRAINING YARD",
		fullname: b"TRAINING YARD training",
		directory: b"data\\map0\\",
		txt: b"tr",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"UNDERGROUND_EXT",
		fullname: b"UNDERGROUND extermination",
		directory: b"data\\map5\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"BUILDING_EXT",
		fullname: b"BUILDING extermination",
		directory: b"data\\map1\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SNOW BASE_EXT",
		fullname: b"SNOW BASE extermination",
		directory: b"data\\map2\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MBASE_EXT",
		fullname: b"MILITARY BASE extermination",
		directory: b"data\\map4\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"WAREHOUSE_EXT",
		fullname: b"WAREHOUSE AREA extermination",
		directory: b"data\\map7\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"DUEL_EXT",
		fullname: b"DUEL extermination",
		directory: b"data\\map9\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"STATION_EXT",
		fullname: b"SUBWAY STATION extermination",
		directory: b"data\\map6\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MAZE_EXT",
		fullname: b"MAZE extermination",
		directory: b"data\\map13\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RUINS_EXT",
		fullname: b"RUINS extermination",
		directory: b"data\\map14\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_EXT",
		fullname: b"URBAN extermination",
		directory: b"data\\map8\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"UNDERGROUND_EXT2",
		fullname: b"UNDERGROUND extermination2",
		directory: b"data\\map5\\",
		txt: b"ext2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"TUNNEL_EXT",
		fullname: b"TUNNEL extermination",
		directory: b"data\\map12\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_DEF",
		fullname: b"URBAN defend target",
		directory: b"data\\map8\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"DTOWN_EXT",
		fullname: b"DESERT TOWN extermination",
		directory: b"data\\map3\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_DEF2",
		fullname: b"URBAN defend target2",
		directory: b"data\\map8\\",
		txt: b"def2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"WAREHOUSE_KT",
		fullname: b"WAREHOUSE AREA kill the target",
		directory: b"data\\map7\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RUINS_RE",
		fullname: b"RUINS release",
		directory: b"data\\map14\\",
		txt: b"re",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RELIC_CAP",
		fullname: b"RELIC capture",
		directory: b"data\\map16\\",
		txt: b"cap",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MBASE_DE",
		fullname: b"MILITARY BASE destroy",
		directory: b"data\\map4\\",
		txt: b"de",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RUINS_CAP",
		fullname: b"RUINS capture",
		directory: b"data\\map14\\",
		txt: b"cap",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"DTOWN_KT",
		fullname: b"DESERT TOWN kill the target",
		directory: b"data\\map3\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SNOW BASE_RE",
		fullname: b"SNOW BASE release",
		directory: b"data\\map2\\",
		txt: b"re",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"UNDERGROUND_DEF",
		fullname: b"UNDERGROUND defend target",
		directory: b"data\\map5\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"TUNNEL_EXT2",
		fullname: b"TUNNEL extermination2",
		directory: b"data\\map12\\",
		txt: b"ext2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RELIC_DEF",
		fullname: b"RELIC defend target",
		directory: b"data\\map16\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_KT",
		fullname: b"URBAN kill the target",
		directory: b"data\\map8\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"ALLEY_KT",
		fullname: b"ALLEY kill the target",
		directory: b"data\\map10\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"STATION_KT",
		fullname: b"SUBWAY STATION kill the target",
		directory: b"data\\map6\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"WAREHOUSE_DEF",
		fullname: b"WAREHOUSE AREA defend target",
		directory: b"data\\map7\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_KT2",
		fullname: b"URBAN kill the target2",
		directory: b"data\\map8\\",
		txt: b"kt2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"OFFICE_DEF",
		fullname: b"OFFICE defend target",
		directory: b"data\\map15\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_CAP",
		fullname: b"URBAN capture",
		directory: b"data\\map8\\",
		txt: b"cap",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"ALLEY_EXT",
		fullname: b"ALLEY extermination",
		directory: b"data\\map10\\",
		txt: b"ext",
		collision: false,
		screen: true,
	},
	OfficialMissionParameter {
		name: b"TUNNEL_ESC",
		fullname: b"TUNNEL escape",
		directory: b"data\\map12\\",
		txt: b"esc",
		collision: false,
		screen: true,
	},
	OfficialMissionParameter {
		name: b"MAZE_CAP",
		fullname: b"MAZE capture",
		directory: b"data\\map13\\",
		txt: b"cap",
		collision: false,
		screen: true,
	},
	OfficialMissionParameter {
		name: b"WAREHOUSE_DEF2",
		fullname: b"WAREHOUSE AREA defend target2",
		directory: b"data\\map7\\",
		txt: b"def2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RUINS_DE",
		fullname: b"RUINS destroy",
		directory: b"data\\map14\\",
		txt: b"de",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_KT3",
		fullname: b"URBAN kill the target3",
		directory: b"data\\map8\\",
		txt: b"kt3",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"DUEL_EXT2",
		fullname: b"DUEL extermination2",
		directory: b"data\\map9\\",
		txt: b"ext2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"RELIC_EXT",
		fullname: b"RELIC extermination",
		directory: b"data\\map16\\",
		txt: b"ext",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SNOW BASE_DEF",
		fullname: b"SNOW BASE defend target",
		directory: b"data\\map2\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"BUILDING_KT",
		fullname: b"BUILDING kill the target",
		directory: b"data\\map1\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MAZE_ESC",
		fullname: b"MAZE escape",
		directory: b"data\\map13\\",
		txt: b"esc",
		collision: false,
		screen: true,
	},
	OfficialMissionParameter {
		name: b"ALLEY_EXT2",
		fullname: b"ALLEY extermination2",
		directory: b"data\\map10\\",
		txt: b"ext2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MBASE_ESC",
		fullname: b"MILITARY BASE escape",
		directory: b"data\\map4\\",
		txt: b"esc",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"DTOWN_DEF",
		fullname: b"DESERT TOWN defend target",
		directory: b"data\\map3\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"OFFICE_KT",
		fullname: b"OFFICE kill the target",
		directory: b"data\\map15\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"BUILDING_DEF",
		fullname: b"BUILDING defend target",
		directory: b"data\\map1\\",
		txt: b"def",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"OFFICE_KT2",
		fullname: b"OFFICE kill the target2",
		directory: b"data\\map15\\",
		txt: b"kt2",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"MBASE_CAP",
		fullname: b"MILITARY BASE capture",
		directory: b"data\\map4\\",
		txt: b"cap",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"TUNNEL_KT",
		fullname: b"TUNNEL kill the target",
		directory: b"data\\map12\\",
		txt: b"kt",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"OFFICE_RE",
		fullname: b"OFFICE release",
		directory: b"data\\map15\\",
		txt: b"re",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"URBAN_KT4",
		fullname: b"URBAN kill the target4",
		directory: b"data\\map8\\",
		txt: b"kt4",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"UNDERGROUND_EXT3",
		fullname: b"UNDERGROUND extermination3",
		directory: b"data\\map5\\",
		txt: b"ext3",
		collision: false,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SCHOOL_EXT",
		fullname: b"SCHOOL extermination",
		directory: b"data\\map11\\",
		txt: b"ext",
		collision: true,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SCHOOL_EXT2",
		fullname: b"SCHOOL extermination2",
		directory: b"data\\map11\\",
		txt: b"ext2",
		collision: true,
		screen: false,
	},
	OfficialMissionParameter {
		name: b"SCHOOL_DE",
		fullname: b"SCHOOL destroy",
		directory: b"data\\map11\\",
		txt: b"de",
		collision: true,
		screen: false,
	},
];

/// Upstream `AIlevel`. 6 entries.
pub const AI_PARAMETER: [AIParameter; TOTAL_PARAMETERINFO_AILEVEL] = [
	AIParameter {
		aiming: 1,
		attack: 99,
		search: 1,
		limitserror: 0,
	},
	AIParameter {
		aiming: 1,
		attack: 18,
		search: 2,
		limitserror: 2,
	},
	AIParameter {
		aiming: 2,
		attack: 16,
		search: 3,
		limitserror: 0,
	},
	AIParameter {
		aiming: 3,
		attack: 14,
		search: 4,
		limitserror: -1,
	},
	AIParameter {
		aiming: 4,
		attack: 12,
		search: 5,
		limitserror: -2,
	},
	AIParameter {
		aiming: 5,
		attack: 10,
		search: 6,
		limitserror: -3,
	},
];

/// Upstream `Scope`. 4 entries.
pub const SCOPE_PARAMETER: [ScopeParameter; TOTAL_PARAMETERINFO_SCOPE] = [
	SCOPE_PARAMETER_0,
	ScopeParameter {
		// simple scope
		view_angle: degree_to_radian(30.0),
		mouse_range: 0.0032,
		scope_gunsight: 1,
		no_scope_gunsight: true,
		weapon_no_scope_reaction: 0,
		weapon_motion_reaction_y: -1.0,
		weapon_recoil_scope_only: true,
		weapon_recoil_scope_rx: 0.4,
		weapon_recoil_scope_ry_min: 1.2,
		weapon_recoil_scope_ry_max: 2.0,
		ai_shot_angle: degree_to_radian(6.0),
		ai_shot_angle_long: degree_to_radian(3.0),
		ai_add_search_enemy_max_dist_normal: 25.0,
		ai_add_search_enemy_max_dist_caution: 40.0,
	},
	ScopeParameter {
		// precision scope
		view_angle: degree_to_radian(15.0),
		mouse_range: 0.0060,
		scope_gunsight: 2,
		no_scope_gunsight: false,
		weapon_no_scope_reaction: 0,
		weapon_motion_reaction_y: degree_to_radian(4.0),
		weapon_recoil_scope_only: false,
		weapon_recoil_scope_rx: 0.6,
		weapon_recoil_scope_ry_min: 2.4,
		weapon_recoil_scope_ry_max: 3.6,
		ai_shot_angle: degree_to_radian(4.0),
		ai_shot_angle_long: degree_to_radian(2.0),
		ai_add_search_enemy_max_dist_normal: 50.0,
		ai_add_search_enemy_max_dist_caution: 80.0,
	},
	SCOPE_PARAMETER_0,
];

// ---------------------------------------------------------------------------
// Accessors
// ---------------------------------------------------------------------------

/// Class that manages the configured values.
///
/// Upstream this owned the tables through pointers. Here the tables are `const` items,
/// so the class is empty and only carries the accessors, which keeps call sites reading
/// `parameter.get_weapon(id)` exactly as they read `Parameter.GetWeapon(id, &out)`.
///
/// Every upstream accessor reports failure as the return code `1` and success as `0`,
/// and leaves the caller's out parameter untouched on failure. The out parameter is the
/// return value here and the code is folded into it: `None` is the `1`, `Some` is the
/// `0`, and there is no third outcome to tell apart.
#[derive(Clone, Copy, Debug, Default)]
pub struct ParameterInfo;

impl ParameterInfo {
	/// Upstream `ParameterInfo()`, which only pointed its members at nothing.
	pub fn new() -> Self {
		ParameterInfo
	}

	/// Get the human settings. Upstream `GetHuman`.
	pub fn get_human(&self, id: i32) -> Option<HumanParameter> {
		if id < 0 || TOTAL_PARAMETERINFO_HUMAN as i32 <= id {
			return None;
		}
		Some(HUMAN_PARAMETER[id as usize])
	}

	/// Get the path of the human texture file. Upstream `GetHumanTexturePath`.
	///
	/// Upstream `strcpy`s into a caller buffer. The returned slice points at the same
	/// bytes, so the copy and the buffer size limit are not reproduced.
	pub fn get_human_texture_path(&self, id: i32) -> Option<&'static [u8]> {
		if id < 0 || TOTAL_HUMANTEXTURE as i32 <= id {
			return None;
		}
		Some(HUMAN_TEXTURE_PATH[id as usize])
	}

	/// Get the weapon settings. Upstream `GetWeapon`.
	///
	/// An id outside the weapon table falls through to [`Self::get_bug_weapon`], which is
	/// the upstream behaviour when `ENABLE_BUG_HUMANWEAPON` is defined.
	pub fn get_weapon(&self, id: i32) -> Option<WeaponParameter> {
		if id < 0 || TOTAL_PARAMETERINFO_WEAPON as i32 <= id {
			if ENABLE_BUG_HUMANWEAPON {
				return self.get_bug_weapon(id);
			}
			return None;
		}
		Some(WEAPON_PARAMETER[id as usize])
	}

	/// Get the weapon settings of a special bug weapon. Upstream `GetBugWeapon`.
	///
	/// Only the four ids the upstream lists are recognised, and each of them answers with
	/// the single `BugWeapon` entry. Use this only after [`Self::get_weapon`] has failed.
	pub fn get_bug_weapon(&self, id: i32) -> Option<WeaponParameter> {
		if id == 23 || id == 24 || id == 30 || id == 53 {
			return Some(BUG_WEAPON_PARAMETER[0]);
		}
		None
	}

	/// Get the small object settings. Upstream `GetSmallObject`.
	///
	/// Settings added by a mission cannot be retrieved here, the upstream warning says to
	/// ask the `MIFInterface` class for those.
	pub fn get_small_object(&self, id: i32) -> Option<SmallObjectParameter> {
		if id < 0 || TOTAL_PARAMETERINFO_SMALLOBJECT as i32 <= id {
			return None;
		}
		Some(SMALL_OBJECT_PARAMETER[id as usize])
	}

	/// Get the bullet object settings. Upstream `GetBullet`.
	pub fn get_bullet(&self, id: i32) -> Option<BulletParameter> {
		if id < 0 || TOTAL_PARAMETERINFO_BULLET as i32 <= id {
			return None;
		}
		Some(BULLET_PARAMETER[id as usize])
	}

	/// Get an official mission. Upstream `GetOfficialMission`.
	///
	/// Upstream takes six out parameters and each of them may be null, so a caller asks
	/// for only the fields it wants. The whole record is one `Copy` value here, so the
	/// caller takes the fields it wants from the result instead.
	pub fn get_official_mission(&self, id: i32) -> Option<OfficialMissionParameter> {
		if id < 0 || TOTAL_OFFICIALMISSION as i32 <= id {
			return None;
		}
		Some(OFFICIAL_MISSION_PARAMETER[id as usize])
	}

	/// Get the AI level settings (performance values). Upstream `GetAIlevel`.
	///
	/// Upstream hands back a pointer into its own table. The record is four integers, so
	/// this returns a copy, which keeps the caller free of a borrow of the table.
	pub fn get_ai_level(&self, level: i32) -> Option<AIParameter> {
		if level < 0 || TOTAL_PARAMETERINFO_AILEVEL as i32 <= level {
			return None;
		}
		Some(AI_PARAMETER[level as usize])
	}

	/// Get the scope settings. Upstream `GetScopeParam`.
	///
	/// A negative id fails, while an id of `TOTAL_PARAMETERINFO_SCOPE` or more returns the
	/// setting value at `TOTAL_PARAMETERINFO_SCOPE - 1`.
	pub fn get_scope_param(&self, id: i32) -> Option<ScopeParameter> {
		if id < 0 {
			return None;
		}
		let id = if TOTAL_PARAMETERINFO_SCOPE as i32 <= id {
			TOTAL_PARAMETERINFO_SCOPE as i32 - 1
		} else {
			id
		};
		Some(SCOPE_PARAMETER[id as usize])
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn table_sizes_match_the_constants() {
		assert_eq!(HUMAN_PARAMETER.len(), TOTAL_PARAMETERINFO_HUMAN);
		assert_eq!(HUMAN_TEXTURE_PATH.len(), TOTAL_HUMANTEXTURE);
		assert_eq!(WEAPON_PARAMETER.len(), TOTAL_PARAMETERINFO_WEAPON);
		assert_eq!(BUG_WEAPON_PARAMETER.len(), 1);
		assert_eq!(
			SMALL_OBJECT_PARAMETER.len(),
			TOTAL_PARAMETERINFO_SMALLOBJECT
		);
		assert_eq!(BULLET_PARAMETER.len(), TOTAL_PARAMETERINFO_BULLET);
		assert_eq!(OFFICIAL_MISSION_PARAMETER.len(), TOTAL_OFFICIALMISSION);
		assert_eq!(AI_PARAMETER.len(), TOTAL_PARAMETERINFO_AILEVEL);
		assert_eq!(SCOPE_PARAMETER.len(), TOTAL_PARAMETERINFO_SCOPE);
	}

	#[test]
	fn the_id_constants_name_the_entries_upstream_says_they_do() {
		assert_eq!(
			WEAPON_PARAMETER[ID_WEAPON_NONE as usize].name,
			b"NONE".as_slice()
		);
		assert_eq!(
			WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].name,
			b"GRENADE".as_slice()
		);
		assert_eq!(
			WEAPON_PARAMETER[ID_WEAPON_CASE as usize].name,
			b"CASE".as_slice()
		);
		assert_eq!(OFFICIALMISSION_BD1, b"temp.bd1".as_slice());
		assert_eq!(SMALLOBJECT_SCALE, 5.0);
	}

	#[test]
	fn specific_entries_match_the_upstream_literals() {
		// human: the combat robot has the one health value in the thousands
		let robot = &HUMAN_PARAMETER[10];
		assert_eq!(robot.texture, 13);
		assert_eq!(robot.model, 0);
		assert_eq!(robot.hp, 2500);
		assert_eq!(robot.ai_level, 4);
		assert_eq!(robot.ai_trackability, -1);
		assert_eq!(robot.weapon, [6, 12]);
		assert_eq!(robot.r#type, 1);
		// a zombie: trackability is set, type 2
		assert_eq!(HUMAN_PARAMETER[29].ai_trackability, 1);
		assert_eq!(HUMAN_PARAMETER[32].hp, 950);
		assert_eq!(HUMAN_PARAMETER[32].r#type, 2);
		assert_eq!(HUMAN_PARAMETER[42].ai_level, 4);
		assert_eq!(HUMAN_PARAMETER[42].weapon, [22, 0]);

		// weapon 1, MP5
		let mp5 = &WEAPON_PARAMETER[1];
		assert_eq!(mp5.name, b"MP5".as_slice());
		assert_eq!(mp5.model, b"./data/model/weapon/mp5.x".as_slice());
		assert_eq!(mp5.texture, b"./data/model/weapon/mp5.bmp".as_slice());
		assert_eq!(mp5.attacks, 44);
		assert_eq!(mp5.blazings, 4);
		assert_eq!(mp5.nbsmax, 30);
		assert_eq!(mp5.error_range_max, 21);
		assert_eq!(mp5.mz, 35.0);
		assert_eq!(mp5.flashz, 68.0);
		assert_eq!(mp5.yakkyou_size, -1.0);
		assert_eq!(mp5.burstcnt, 0);
		assert_eq!(mp5.weapon_p, 0);
		assert_eq!(mp5.change_weapon_id, -1);
		assert!(!mp5.silencer);
		assert_eq!(mp5.pellet, 1);

		// the last weapon, and the semi automatic glock that switches to 16
		assert_eq!(WEAPON_PARAMETER[22].name, b"MK23 SD".as_slice());
		assert!(WEAPON_PARAMETER[22].silencer);
		assert_eq!(WEAPON_PARAMETER[4].change_weapon_id, 16);
		assert_eq!(WEAPON_PARAMETER[13].name, b"GRENADE".as_slice());
		// weapon 0 is the empty weapon, and its cartridge size is still -1
		assert_eq!(WEAPON_PARAMETER[ID_WEAPON_NONE as usize].attacks, 0);
		assert_eq!(WEAPON_PARAMETER[ID_WEAPON_NONE as usize].yakkyou_size, -1.0);
		assert_eq!(WEAPON_PARAMETER[ID_WEAPON_NONE as usize].weapon_p, 2);

		// small objects
		assert_eq!(SMALL_OBJECT_PARAMETER[0].decide, 10);
		assert_eq!(SMALL_OBJECT_PARAMETER[0].hp, 6);
		assert_eq!(
			SMALL_OBJECT_PARAMETER[7].model,
			b"./data/article/isu.x".as_slice()
		);
		assert_eq!(SMALL_OBJECT_PARAMETER[7].decide, 56);
		assert_eq!(SMALL_OBJECT_PARAMETER[11].jump, 7);

		// bullet 1 is the weapon 13 model, texture and size
		assert_eq!(
			BULLET_PARAMETER[0].model,
			b"./data/model/bullet.x".as_slice()
		);
		assert_eq!(BULLET_PARAMETER[0].size, 1.0);
		assert_eq!(
			BULLET_PARAMETER[1].model,
			WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].model
		);
		assert_eq!(
			BULLET_PARAMETER[1].texture,
			WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].texture
		);
		assert_eq!(
			BULLET_PARAMETER[1].size,
			WEAPON_PARAMETER[ID_WEAPON_GRENADE as usize].size
		);

		// missions, including the directory separators that stay backslashes
		let training = &OFFICIAL_MISSION_PARAMETER[0];
		assert_eq!(training.name, b"TRAINING YARD".as_slice());
		assert_eq!(training.fullname, b"TRAINING YARD training".as_slice());
		assert_eq!(training.directory, b"data\\map0\\".as_slice());
		assert_eq!(training.txt, b"tr".as_slice());
		assert!(!training.collision);
		assert!(!training.screen);
		assert!(OFFICIAL_MISSION_PARAMETER[33].screen);
		assert!(OFFICIAL_MISSION_PARAMETER[43].screen);
		assert!(OFFICIAL_MISSION_PARAMETER[55].collision);
		assert_eq!(
			OFFICIAL_MISSION_PARAMETER[55].name,
			b"SCHOOL_EXT".as_slice()
		);
		assert!(OFFICIAL_MISSION_PARAMETER[56].collision);
		assert_eq!(OFFICIAL_MISSION_PARAMETER[57].name, b"SCHOOL_DE".as_slice());
		assert_eq!(OFFICIAL_MISSION_PARAMETER[57].txt, b"de".as_slice());
		assert!(OFFICIAL_MISSION_PARAMETER[57].collision);

		// AI levels: attack falls as the level rises, level 0 is the odd one at 99
		assert_eq!(
			AI_PARAMETER[0],
			AIParameter {
				aiming: 1,
				attack: 99,
				search: 1,
				limitserror: 0
			}
		);
		assert_eq!(
			AI_PARAMETER[3],
			AIParameter {
				aiming: 3,
				attack: 14,
				search: 4,
				limitserror: -1
			}
		);
		assert_eq!(
			AI_PARAMETER[5],
			AIParameter {
				aiming: 5,
				attack: 10,
				search: 6,
				limitserror: -3
			}
		);

		// scopes: the angles go through DegreeToRadian, scope 3 is a copy of scope 0
		assert_eq!(SCOPE_PARAMETER[0].view_angle, degree_to_radian(65.0));
		assert_eq!(SCOPE_PARAMETER[0].mouse_range, 0.0100);
		assert!(SCOPE_PARAMETER[0].no_scope_gunsight);
		assert_eq!(SCOPE_PARAMETER[0].weapon_motion_reaction_y, -1.0);
		assert_eq!(SCOPE_PARAMETER[1].view_angle, degree_to_radian(30.0));
		assert_eq!(SCOPE_PARAMETER[1].mouse_range, 0.0032);
		assert!(SCOPE_PARAMETER[1].weapon_recoil_scope_only);
		assert_eq!(SCOPE_PARAMETER[1].weapon_recoil_scope_ry_max, 2.0);
		assert_eq!(
			SCOPE_PARAMETER[1].ai_add_search_enemy_max_dist_caution,
			40.0
		);
		assert_eq!(SCOPE_PARAMETER[2].view_angle, degree_to_radian(15.0));
		assert_eq!(SCOPE_PARAMETER[2].scope_gunsight, 2);
		assert!(!SCOPE_PARAMETER[2].no_scope_gunsight);
		assert_eq!(
			SCOPE_PARAMETER[2].weapon_motion_reaction_y,
			degree_to_radian(4.0)
		);
		assert_eq!(SCOPE_PARAMETER[3], SCOPE_PARAMETER[0]);
	}

	#[test]
	fn an_out_of_range_id_fails_the_way_upstream_returns_one() {
		let p = ParameterInfo::new();
		// negative
		assert!(p.get_human(-1).is_none());
		assert!(p.get_human_texture_path(-1).is_none());
		assert!(p.get_small_object(-1).is_none());
		assert!(p.get_bullet(-1).is_none());
		assert!(p.get_official_mission(-1).is_none());
		assert!(p.get_ai_level(-1).is_none());
		assert!(p.get_scope_param(-1).is_none());
		// one past the end
		assert!(p.get_human(TOTAL_PARAMETERINFO_HUMAN as i32).is_none());
		assert!(p
			.get_human_texture_path(TOTAL_HUMANTEXTURE as i32)
			.is_none());
		assert!(p
			.get_small_object(TOTAL_PARAMETERINFO_SMALLOBJECT as i32)
			.is_none());
		assert!(p.get_bullet(TOTAL_PARAMETERINFO_BULLET as i32).is_none());
		assert!(p
			.get_official_mission(TOTAL_OFFICIALMISSION as i32)
			.is_none());
		assert!(p.get_ai_level(TOTAL_PARAMETERINFO_AILEVEL as i32).is_none());
		// the last valid id still works
		assert!(p.get_human(TOTAL_PARAMETERINFO_HUMAN as i32 - 1).is_some());
		assert!(p
			.get_ai_level(TOTAL_PARAMETERINFO_AILEVEL as i32 - 1)
			.is_some());
	}

	#[test]
	fn an_out_of_range_weapon_falls_through_to_the_bug_weapon() {
		let p = ParameterInfo::new();
		// the four ids the upstream GetBugWeapon answers
		for id in [23, 24, 30, 53] {
			let w = p.get_weapon(id).expect("bug weapon id");
			assert_eq!(w.name, b"BugWeapon".as_slice());
			assert_eq!(w.size, 9.0);
			assert_eq!(w.attacks, 0);
			assert_eq!(w.weapon_p, 1);
		}
		// 22 is the last real weapon, 23 is the first out of range id and one of the four
		// the bug weapon answers, 25 is in neither table
		assert_eq!(p.get_weapon(22).unwrap().name, b"MK23 SD".as_slice());
		assert_eq!(
			p.get_weapon(TOTAL_PARAMETERINFO_WEAPON as i32)
				.unwrap()
				.name,
			b"BugWeapon".as_slice()
		);
		assert!(p.get_weapon(25).is_none());
		assert!(p.get_weapon(-1).is_none());
		assert!(p.get_bug_weapon(23).is_some());
		assert!(p.get_bug_weapon(1).is_none());
	}

	#[test]
	fn a_scope_id_at_or_above_the_table_clamps_to_the_last_entry() {
		let p = ParameterInfo::new();
		assert_eq!(
			p.get_scope_param(TOTAL_PARAMETERINFO_SCOPE as i32),
			p.get_scope_param(3)
		);
		assert_eq!(p.get_scope_param(999), p.get_scope_param(3));
		assert_eq!(p.get_scope_param(3), p.get_scope_param(0));
		// the clamp is not a failure
		assert!(p.get_scope_param(i32::MAX).is_some());
	}

	#[test]
	fn the_accessors_hand_back_the_table_entry() {
		let p = ParameterInfo::new();
		assert_eq!(p.get_human(10), Some(HUMAN_PARAMETER[10]));
		assert_eq!(
			p.get_human_texture_path(0),
			Some(b"./data/model/civ1.bmp".as_slice())
		);
		assert_eq!(
			p.get_human_texture_path(29),
			Some(b"./data/model/zombie4.bmp".as_slice())
		);
		assert_eq!(p.get_weapon(1), Some(WEAPON_PARAMETER[1]));
		assert_eq!(p.get_small_object(7), Some(SMALL_OBJECT_PARAMETER[7]));
		assert_eq!(p.get_bullet(1), Some(BULLET_PARAMETER[1]));
		assert_eq!(
			p.get_official_mission(57),
			Some(OFFICIAL_MISSION_PARAMETER[57])
		);
		assert_eq!(p.get_ai_level(0), Some(AI_PARAMETER[0]));
		assert_eq!(p.get_scope_param(1), Some(SCOPE_PARAMETER[1]));
	}
}
