//! Resource management, from the upstream `resource.cpp` and `resource.h`.
//!
//! This is the layer between the setting tables in [`crate::parameter`] and the loaded
//! assets: for each human, weapon, small object and bullet it loads the model and the
//! texture the table names, loads the shared sounds, and hands the data numbers back by
//! the table's own id. Nothing here draws anything.
//!
//! Upstream the class holds three pointers, to `ParameterInfo`, `D3DGraphics` and
//! `SoundControl`, and every method reaches through them. The tables carry no state, and
//! the other two classes own what gets loaded, so those arrive as arguments, the way
//! `object::Object::render` already takes the graphics. Only the parameter class is kept
//! as a field, because `SetParameterInfo` is a call the game makes at init and the call
//! sites read the same with it in place.
//!
//! Two upstream guards cannot exist here and are not missed: every method starts by
//! refusing a `NULL` member, and an argument can never be absent. The guards' outcome,
//! the failure return, is unreachable in each case and is not reproduced.
//!
//! `-1` is the empty data number, as upstream. A loader that cannot find a file keeps
//! going and leaves the id at `-1` rather than failing the whole call, which is what the
//! callers depend on: `objectmanager.cpp` adds the object anyway and draws it untextured,
//! and for a human it sets a refresh flag and asks again every frame.

use crate::core::ENABLE_BUG_HUMANWEAPON;
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::{path_from_bytes, MAX_ADDSMALLOBJECT};
use crate::parameter::{
	ParameterInfo, TOTAL_PARAMETERINFO_BULLET, TOTAL_PARAMETERINFO_HUMAN,
	TOTAL_PARAMETERINFO_SMALLOBJECT, TOTAL_PARAMETERINFO_WEAPON,
};
use crate::sound::SoundControl;

/// Total upper body models.
pub const TOTAL_UPMODE: usize = 6;
/// Total arm models.
pub const TOTAL_ARMMODE: usize = 3;
/// Total leg (walking) models.
pub const TOTAL_WALKMODE: usize = 8;
/// Total leg (running) models.
pub const TOTAL_RUNMODE: usize = 12;

/// Maximum number of human textures to load.
pub const MAX_LOADHUMANTEXTURE: usize = 10;

/// The models, textures and sounds the game loads once and hands out by data number.
#[derive(Debug)]
pub struct ResourceManager {
	/// The setting tables. Upstream the pointer set by `SetParameterInfo`.
	param_info: ParameterInfo,

	/// Upper body models.
	human_upmodel: [i32; TOTAL_UPMODE],
	/// Arm models.
	human_armmodel: [i32; TOTAL_ARMMODE],
	/// Leg (standing) model.
	human_legmodel: i32,
	/// Leg (walking) models.
	human_walkmodel: [i32; TOTAL_WALKMODE],
	/// Leg (running) models.
	human_runmodel: [i32; TOTAL_RUNMODE],
	/// Corresponding numbers of the human textures, which is the texture path id the
	/// human table named.
	human_texture_param: [i32; MAX_LOADHUMANTEXTURE],
	/// Data numbers of the human textures. Upstream keeps this array parallel to
	/// [`Self::human_texture_param`], and both are written together, so slot `i` of one
	/// belongs to slot `i` of the other. Any lookup that has the path id walks this array
	/// rather than searching the tables.
	human_texture_d3dg: [i32; MAX_LOADHUMANTEXTURE],
	/// Data numbers of the weapon models.
	weapon_model: [i32; TOTAL_PARAMETERINFO_WEAPON],
	/// Data numbers of the weapon textures.
	weapon_texture: [i32; TOTAL_PARAMETERINFO_WEAPON],
	/// Data numbers of the weapon sounds.
	weapon_sound: [i32; TOTAL_PARAMETERINFO_WEAPON],
	/// Data number of the weapon reload sound.
	weapon_reloadsound: i32,
	/// Data numbers of the small object models.
	smallobject_model: [i32; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
	/// Data numbers of the small object textures.
	smallobject_texture: [i32; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
	/// Data numbers of the small object sounds.
	smallobject_sound: [i32; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
	/// Bullet models.
	bullet_model: [i32; TOTAL_PARAMETERINFO_BULLET],
	/// Bullet textures.
	bullet_texture: [i32; TOTAL_PARAMETERINFO_BULLET],

	/// Scope texture.
	scopetexture: i32,
	/// Sky background model.
	skymodel: i32,
	/// Sky background texture.
	skytexture: i32,
	/// Bullet hit sound (A).
	bullet_hitsound_a: i32,
	/// Bullet hit sound (B).
	bullet_hitsound_b: i32,
	/// Bullet hit sound (human).
	bullet_humanhitsound: i32,
	/// Bullet passing sound.
	bullet_passingsound: i32,
	/// Grenade explosion sound.
	grenade_bang: i32,
	/// Grenade bounce sound.
	grenade_cco: i32,
	/// Effect blood texture.
	effecttexture_blood: i32,
	/// Effect flash texture.
	effecttexture_mflash: i32,
	/// Effect smoke texture.
	effecttexture_smoke: i32,
	/// Effect shell casing texture.
	effecttexture_yakkyou: i32,
}

impl Default for ResourceManager {
	fn default() -> Self {
		ResourceManager::new()
	}
}

impl ResourceManager {
	/// The upstream constructor, which marks every slot empty.
	pub fn new() -> ResourceManager {
		ResourceManager {
			param_info: ParameterInfo::new(),

			human_upmodel: [-1; TOTAL_UPMODE],
			human_armmodel: [-1; TOTAL_ARMMODE],
			human_legmodel: -1,
			human_walkmodel: [-1; TOTAL_WALKMODE],
			human_runmodel: [-1; TOTAL_RUNMODE],
			human_texture_param: [-1; MAX_LOADHUMANTEXTURE],
			human_texture_d3dg: [-1; MAX_LOADHUMANTEXTURE],
			weapon_model: [-1; TOTAL_PARAMETERINFO_WEAPON],
			weapon_texture: [-1; TOTAL_PARAMETERINFO_WEAPON],
			weapon_sound: [-1; TOTAL_PARAMETERINFO_WEAPON],
			weapon_reloadsound: -1,
			smallobject_model: [-1; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
			smallobject_texture: [-1; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
			smallobject_sound: [-1; TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT],
			bullet_model: [-1; TOTAL_PARAMETERINFO_BULLET],
			bullet_texture: [-1; TOTAL_PARAMETERINFO_BULLET],

			scopetexture: -1,
			skymodel: -1,
			skytexture: -1,
			bullet_hitsound_a: -1,
			bullet_hitsound_b: -1,
			bullet_humanhitsound: -1,
			bullet_passingsound: -1,
			grenade_bang: -1,
			grenade_cco: -1,
			effecttexture_blood: -1,
			effecttexture_mflash: -1,
			effecttexture_smoke: -1,
			effecttexture_yakkyou: -1,
		}
	}

	/// Sets the parameter tables to read. Upstream `SetParameterInfo`.
	///
	/// The tables are a `const` item and this value carries no state, so the upstream
	/// requirement that the class is configured before use cannot be violated here. The
	/// call is kept because it is what the game makes at init.
	pub fn set_parameter_info(&mut self, param_info: ParameterInfo) {
		self.param_info = param_info;
	}

	/// Releases everything. Upstream `DestroyResource`, which its destructor called.
	///
	/// The upstream destructor reaches the graphics and the sound class through its own
	/// members, so it can free the assets on the way out. Here those two are the caller's
	/// and a `Drop` impl could not name them, so the release is this explicit call, which
	/// is also what `CleanupGame` makes upstream.
	pub fn destroy_resource(&mut self, d3dg: &mut D3DGraphics, sound: &mut SoundControl) {
		self.cleanup_human_model(d3dg);
		self.cleanup_human_texture(d3dg);
		self.cleanup_weapon_model_texture(d3dg);
		self.cleanup_weapon_sound(sound);
		self.cleanup_small_object_model_texture(d3dg);
		self.cleanup_small_object_sound(sound);

		self.cleanup_scope_texture(d3dg);
		self.cleanup_sky_model_texture(d3dg);
		self.cleanup_bullet_model_texture(d3dg);
		self.cleanup_bullet_sound(sound);
		self.cleanup_effect_texture(d3dg);
	}

	// -- human models -------------------------------------------------------

	/// Loads the human models. Upstream `LoadHumanModel`.
	///
	/// Success: 0, failure: 1. The failure is the upstream `d3dg == NULL` guard, which
	/// cannot be reached, so this always reports success.
	pub fn load_human_model(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		self.human_upmodel[0] = d3dg.load_model(&path_from_bytes(b"data\\model\\up0.x"));
		self.human_upmodel[1] = d3dg.load_model(&path_from_bytes(b"data\\model\\up1.x"));
		self.human_upmodel[2] = d3dg.load_model(&path_from_bytes(b"data\\model\\up2.x"));
		self.human_upmodel[3] = d3dg.load_model(&path_from_bytes(b"data\\model\\up3.x"));
		self.human_upmodel[4] = d3dg.load_model(&path_from_bytes(b"data\\model\\up4.x"));
		self.human_upmodel[5] = d3dg.load_model(&path_from_bytes(b"data\\model\\up5.x"));
		self.human_armmodel[0] = d3dg.load_model(&path_from_bytes(b"data\\model\\arm00.x"));
		self.human_armmodel[1] = d3dg.load_model(&path_from_bytes(b"data\\model\\arm01.x"));
		self.human_armmodel[2] = d3dg.load_model(&path_from_bytes(b"data\\model\\arm02.x"));
		self.human_legmodel = d3dg.load_model(&path_from_bytes(b"data\\model\\leg.x"));
		self.human_walkmodel[0] = d3dg.load_model(&path_from_bytes(b"data\\model\\walk01.x"));
		self.human_walkmodel[2] = d3dg.load_model(&path_from_bytes(b"data\\model\\walk02.x"));
		self.human_walkmodel[4] = d3dg.load_model(&path_from_bytes(b"data\\model\\walk03.x"));
		self.human_walkmodel[6] = d3dg.load_model(&path_from_bytes(b"data\\model\\walk04.x"));
		self.human_runmodel[0] = d3dg.load_model(&path_from_bytes(b"data\\model\\run01.x"));
		self.human_runmodel[2] = d3dg.load_model(&path_from_bytes(b"data\\model\\run02.x"));
		self.human_runmodel[4] = d3dg.load_model(&path_from_bytes(b"data\\model\\run03.x"));
		self.human_runmodel[6] = d3dg.load_model(&path_from_bytes(b"data\\model\\run04.x"));
		self.human_runmodel[8] = d3dg.load_model(&path_from_bytes(b"data\\model\\run05.x"));
		self.human_runmodel[10] = d3dg.load_model(&path_from_bytes(b"data\\model\\run06.x"));

		// `ENABLE_HUMANMODEL_MORPHING` is defined upstream, so the morphing branch is the
		// one that runs. The odd slots are the poses halfway between their even
		// neighbours, and the last one wraps back to the first, which is what makes a
		// walk loop without a jump at the seam. The copy branch beside it in the upstream
		// is the disabled build and is not carried over.
		self.human_walkmodel[1] =
			d3dg.morphing_model(self.human_walkmodel[0], self.human_walkmodel[2]);
		self.human_walkmodel[3] =
			d3dg.morphing_model(self.human_walkmodel[2], self.human_walkmodel[4]);
		self.human_walkmodel[5] =
			d3dg.morphing_model(self.human_walkmodel[4], self.human_walkmodel[6]);
		self.human_walkmodel[7] =
			d3dg.morphing_model(self.human_walkmodel[6], self.human_walkmodel[0]);
		self.human_runmodel[1] =
			d3dg.morphing_model(self.human_runmodel[0], self.human_runmodel[2]);
		self.human_runmodel[3] =
			d3dg.morphing_model(self.human_runmodel[2], self.human_runmodel[4]);
		self.human_runmodel[5] =
			d3dg.morphing_model(self.human_runmodel[4], self.human_runmodel[6]);
		self.human_runmodel[7] =
			d3dg.morphing_model(self.human_runmodel[6], self.human_runmodel[8]);
		self.human_runmodel[9] =
			d3dg.morphing_model(self.human_runmodel[8], self.human_runmodel[10]);
		self.human_runmodel[11] =
			d3dg.morphing_model(self.human_runmodel[10], self.human_runmodel[0]);

		0
	}

	/// Gets the data numbers of the human models. Upstream `GetHumanModel`, whose five
	/// out-parameters become this tuple.
	pub fn get_human_model(
		&self,
	) -> (
		[i32; TOTAL_UPMODE],
		[i32; TOTAL_ARMMODE],
		i32,
		[i32; TOTAL_WALKMODE],
		[i32; TOTAL_RUNMODE],
	) {
		(
			self.human_upmodel,
			self.human_armmodel,
			self.human_legmodel,
			self.human_walkmodel,
			self.human_runmodel,
		)
	}

	/// Releases the human models. Upstream `CleanupHumanModel`.
	///
	/// Every slot is released and then emptied, which also happens for the slots that were
	/// never loaded, because `CleanupModel` ignores an id of `-1`.
	pub fn cleanup_human_model(&mut self, d3dg: &mut D3DGraphics) {
		for i in 0..TOTAL_UPMODE {
			d3dg.cleanup_model(self.human_upmodel[i]);
			self.human_upmodel[i] = -1;
		}
		for i in 0..TOTAL_ARMMODE {
			d3dg.cleanup_model(self.human_armmodel[i]);
			self.human_armmodel[i] = -1;
		}
		d3dg.cleanup_model(self.human_legmodel);
		self.human_legmodel = -1;
		for i in 0..TOTAL_WALKMODE {
			d3dg.cleanup_model(self.human_walkmodel[i]);
			self.human_walkmodel[i] = -1;
		}
		for i in 0..TOTAL_RUNMODE {
			d3dg.cleanup_model(self.human_runmodel[i]);
			self.human_runmodel[i] = -1;
		}
	}

	// -- human textures -----------------------------------------------------

	/// Loads the human texture of a human type. Upstream `AddHumanTexture`.
	///
	/// Success: the slot the texture was loaded into, failure: -1. A human type whose
	/// texture is already loaded is a -1 as well: the upstream leaves the existing load
	/// alone and the caller keeps using the id it already has.
	pub fn add_human_texture(&mut self, d3dg: &mut D3DGraphics, id: i32) -> i32 {
		if id < 0 || TOTAL_PARAMETERINFO_HUMAN as i32 <= id {
			return -1;
		}

		let Some(data) = self.param_info.get_human(id) else {
			return -1;
		};

		// Checks whether the texture has already been loaded
		for i in 0..MAX_LOADHUMANTEXTURE {
			if self.human_texture_param[i] == data.texture {
				return -1;
			}
		}

		// Loads the texture
		for i in 0..MAX_LOADHUMANTEXTURE {
			if self.human_texture_param[i] == -1 {
				let texture_path_id = data.texture;
				let Some(path) = self.param_info.get_human_texture_path(texture_path_id) else {
					return -1;
				};
				// The path id is recorded before the load, so a texture file that is
				// missing still occupies its slot. That is what keeps the two arrays in
				// step with the human table, and it is what the caller of
				// `GetHumanTexture` reads back as the -1 that makes it retry next frame.
				self.human_texture_param[i] = texture_path_id;
				self.human_texture_d3dg[i] = d3dg.load_texture(path, false, false);
				return i as i32;
			}
		}

		-1
	}

	/// Gets the number of loaded human textures. Upstream `GetHumanTextures`.
	pub fn get_human_textures(&self) -> i32 {
		self.human_texture_param
			.iter()
			.filter(|&&param| param != -1)
			.count() as i32
	}

	/// Gets the data number of a human type's texture. Upstream `GetHumanTexture`.
	///
	/// Success: the texture data number, failure: -1. The data number is the one the
	/// loader recorded, so it is -1 for a texture file that could not be found, and the
	/// caller treats that the same as a human type that named no texture at all.
	pub fn get_human_texture(&self, id: i32) -> i32 {
		if id < 0 || TOTAL_PARAMETERINFO_HUMAN as i32 <= id {
			return -1;
		}

		let Some(data) = self.param_info.get_human(id) else {
			return -1;
		};

		for i in 0..MAX_LOADHUMANTEXTURE {
			if self.human_texture_param[i] == data.texture {
				return self.human_texture_d3dg[i];
			}
		}

		-1
	}

	/// Gets information about a loaded human texture. Upstream `GetHumanTextureInfo`.
	///
	/// `Some((param_id, texture_id))` is a slot that holds a texture, `None` is a `dataid`
	/// outside the ten slots. Upstream both outputs may be `NULL` and either may be asked
	/// for alone, which is what a caller wanting only the path id does. Here the pair comes
	/// back whole, and the slot's two values are read from the same index of the two
	/// parallel arrays, which is the relationship itself rather than a search for it.
	///
	/// Upstream notes this is a debugging function, and the only caller is the debug
	/// console's human texture page.
	pub fn get_human_texture_info(&self, dataid: i32) -> Option<(i32, i32)> {
		if dataid < 0 || MAX_LOADHUMANTEXTURE as i32 <= dataid {
			return None;
		}

		let slot = dataid as usize;
		Some((
			self.human_texture_param[slot],
			self.human_texture_d3dg[slot],
		))
	}

	/// Releases all human textures at once. Upstream `CleanupHumanTexture`.
	pub fn cleanup_human_texture(&mut self, d3dg: &mut D3DGraphics) {
		for i in 0..MAX_LOADHUMANTEXTURE {
			if self.human_texture_param[i] != -1 {
				d3dg.cleanup_texture(self.human_texture_d3dg[i]);
				self.human_texture_param[i] = -1;
				self.human_texture_d3dg[i] = -1;
			}
		}
	}

	// -- weapons ------------------------------------------------------------

	/// Loads all weapon models and textures at once. Upstream `LoadWeaponModelTexture`.
	///
	/// Success: 0, failure: 1 or more, and the count is what it says: every model and every
	/// texture that could not be loaded adds one, and a weapon whose settings could not be
	/// read adds two. A missing file does not abort the loop, so one broken weapon leaves
	/// the other twenty-two usable, which is the point of the count.
	pub fn load_weapon_model_texture(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		let mut cnt = 0;

		for i in 0..TOTAL_PARAMETERINFO_WEAPON {
			match self.param_info.get_weapon(i as i32) {
				Some(data) => {
					// Loads the model and texture, and counts them if an error occurs.
					self.weapon_model[i] = d3dg.load_model(&path_from_bytes(data.model));
					self.weapon_texture[i] = d3dg.load_texture(data.texture, false, false);
					if self.weapon_model[i] == -1 {
						cnt += 1;
					}
					if self.weapon_texture[i] == -1 {
						cnt += 1;
					}
				}
				// If the setting data is invalid, both the model and the texture are errors.
				None => cnt += 2,
			}
		}

		cnt
	}

	/// Gets the weapon model and texture. Upstream `GetWeaponModelTexture`.
	///
	/// `None` is the upstream `1`, which covers an id outside the weapon table that is not
	/// one of the bug weapons. Upstream also reports the `NULL` out-parameters as `1`, and
	/// a tuple return cannot be asked for one alone, so that failure is gone.
	pub fn get_weapon_model_texture(&self, d3dg: &D3DGraphics, id: i32) -> Option<(i32, i32)> {
		if id < 0 || TOTAL_PARAMETERINFO_WEAPON as i32 <= id {
			if ENABLE_BUG_HUMANWEAPON {
				return self.get_bug_weapon_model_texture(d3dg, id);
			}
			return None;
		}

		Some((
			self.weapon_model[id as usize],
			self.weapon_texture[id as usize],
		))
	}

	/// Gets the model and texture of a bug weapon. Upstream `GetBugWeaponModelTexture`.
	///
	/// The four ids are the ones the upstream hard-codes, and each is the human upper body
	/// model drawn with one of the map's own textures, which is how a bug weapon gets
	/// something to show without a model of its own. The map texture is asked for on every
	/// call rather than recorded, so it follows a map reload.
	///
	/// Upstream warns to call this only after [`Self::get_weapon_model_texture`] failed,
	/// and that is the only way it is reached. The parameter class has the matching list
	/// in `ParameterInfo::get_bug_weapon`, and the two must be kept in step.
	pub fn get_bug_weapon_model_texture(&self, d3dg: &D3DGraphics, id: i32) -> Option<(i32, i32)> {
		let texture = match id {
			23 => d3dg.map_texture_id(0),
			24 => d3dg.map_texture_id(3),
			30 => d3dg.map_texture_id(2),
			53 => d3dg.map_texture_id(8),
			_ => return None,
		};

		Some((self.human_upmodel[0], texture))
	}

	/// Releases all weapon models and textures at once. Upstream
	/// `CleanupWeaponModelTexture`.
	pub fn cleanup_weapon_model_texture(&mut self, d3dg: &mut D3DGraphics) {
		for i in 0..TOTAL_PARAMETERINFO_WEAPON {
			d3dg.cleanup_model(self.weapon_model[i]);
			d3dg.cleanup_texture(self.weapon_texture[i]);
			self.weapon_model[i] = -1;
			self.weapon_texture[i] = -1;
		}
	}

	/// Loads the weapon sounds. Upstream `LoadWeaponSound`.
	///
	/// Success: 0, failure: 1. The six files are loaded once and shared, and each weapon
	/// points at one of them by the `soundid` in its settings, so twenty-two weapons cost
	/// six sounds.
	///
	/// The reload sound is disabled upstream, its load commented out and the id left at
	/// -1, and the check below it still asks for a loaded reload sound. The result is that
	/// this returns failure even when all six files loaded, and always has. It is kept
	/// exactly as it is, because the return value is the upstream behaviour and the caller
	/// ignores it: `InitGame` does not look at what this returns, and the per-weapon sound
	/// ids are already assigned by then.
	pub fn load_weapon_sound(&mut self, sound: &mut SoundControl) -> i32 {
		let mut soundid = [0i32; 6];

		// Loads the sound files all at once
		soundid[0] = sound.load_sound(&path_from_bytes(b"data\\sound\\bang1.wav"));
		soundid[1] = sound.load_sound(&path_from_bytes(b"data\\sound\\bang2.wav"));
		soundid[2] = sound.load_sound(&path_from_bytes(b"data\\sound\\bang3.wav"));
		soundid[3] = sound.load_sound(&path_from_bytes(b"data\\sound\\bang4.wav"));
		soundid[4] = sound.load_sound(&path_from_bytes(b"data\\sound\\bang5.wav"));
		soundid[5] = sound.load_sound(&path_from_bytes(b"data\\sound\\ka.wav"));
		//reloadid = SoundCtrl->LoadSound("reload.wav");
		let reloadid = -1;

		for i in 0..TOTAL_PARAMETERINFO_WEAPON {
			let Some(data) = self.param_info.get_weapon(i as i32) else {
				continue;
			};
			// Assigns sound effects according to the sound numbers.
			self.weapon_sound[i] = match data.soundid {
				0 => soundid[0],
				1 => soundid[1],
				2 => soundid[2],
				3 => soundid[3],
				4 => soundid[4],
				13 => soundid[5],
				_ => -1,
			};
		}
		self.weapon_reloadsound = reloadid;

		// Checks whether an error occurred while loading the file
		for id in soundid {
			if id == -1 {
				return 1;
			}
		}
		if reloadid == -1 {
			return 1;
		}
		0
	}

	/// Gets a weapon sound. Upstream `GetWeaponSound`.
	///
	/// `id` is a weapon data number, or -1 for the reload sound. Success: the data number,
	/// failure: -1.
	pub fn get_weapon_sound(&self, id: i32) -> i32 {
		if id == -1 {
			return self.weapon_reloadsound;
		}

		if id < 0 || TOTAL_PARAMETERINFO_WEAPON as i32 <= id {
			return -1;
		}
		self.weapon_sound[id as usize]
	}

	/// Releases all weapon sounds at once. Upstream `CleanupWeaponSound`.
	pub fn cleanup_weapon_sound(&mut self, sound: &mut SoundControl) {
		for i in 0..TOTAL_PARAMETERINFO_WEAPON {
			sound.cleanup_sound(self.weapon_sound[i]);
			self.weapon_sound[i] = -1;
		}
		sound.cleanup_sound(self.weapon_reloadsound);
		self.weapon_reloadsound = -1;
	}

	// -- small objects ------------------------------------------------------

	/// Loads all small object models and textures at once. Upstream
	/// `LoadSmallObjectModelTexture`.
	///
	/// Success: 0, failure: 1 or more, counted the same way as the weapons. Two entries
	/// name the same texture file, so the second load finds the file again and takes a
	/// second texture slot: the upstream has no sharing here, unlike the human textures.
	pub fn load_small_object_model_texture(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		let mut cnt = 0;

		for i in 0..TOTAL_PARAMETERINFO_SMALLOBJECT {
			match self.param_info.get_small_object(i as i32) {
				Some(data) => {
					// Loads the model and texture, and counts them if an error occurs.
					self.smallobject_model[i] = d3dg.load_model(&path_from_bytes(data.model));
					self.smallobject_texture[i] = d3dg.load_texture(data.texture, false, false);
					if self.smallobject_model[i] == -1 {
						cnt += 1;
					}
					if self.smallobject_texture[i] == -1 {
						cnt += 1;
					}
				}
				// If the setting data is invalid, both the model and the texture are errors.
				None => cnt += 2,
			}
		}

		cnt
	}

	/// Gets the small object model and texture. Upstream `GetSmallObjectModelTexture`.
	///
	/// The range covers the additional small object as well as the fixed table, because
	/// the two share one array and one data number space.
	pub fn get_small_object_model_texture(&self, id: i32) -> Option<(i32, i32)> {
		if id < 0 || (TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT) as i32 <= id {
			return None;
		}

		let slot = id as usize;
		Some((self.smallobject_model[slot], self.smallobject_texture[slot]))
	}

	/// Releases all small object models and textures at once. Upstream
	/// `CleanupSmallObjectModelTexture`.
	pub fn cleanup_small_object_model_texture(&mut self, d3dg: &mut D3DGraphics) {
		for i in 0..(TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT) {
			d3dg.cleanup_model(self.smallobject_model[i]);
			d3dg.cleanup_texture(self.smallobject_texture[i]);
			self.smallobject_model[i] = -1;
			self.smallobject_texture[i] = -1;
		}
	}

	/// Loads the small object sounds. Upstream `LoadSmallObjectSound`.
	///
	/// Success: 0, failure: 1. Only the twelve fixed entries are given a sound here. The
	/// additional small object names its own file in the mission and is loaded separately.
	pub fn load_small_object_sound(&mut self, sound: &mut SoundControl) -> i32 {
		let mut soundid = [0i32; 2];

		// Loads the sound files all at once
		soundid[0] = sound.load_sound(&path_from_bytes(b"data\\sound\\can.wav"));
		soundid[1] = sound.load_sound(&path_from_bytes(b"data\\sound\\dan.wav"));

		for i in 0..TOTAL_PARAMETERINFO_SMALLOBJECT {
			let Some(data) = self.param_info.get_small_object(i as i32) else {
				continue;
			};
			// Assigns sound effects according to the sound numbers.
			self.smallobject_sound[i] = match data.sound {
				0 => soundid[0],
				1 => soundid[1],
				_ => -1,
			};
		}

		// Checks whether an error occurred while loading the file
		for id in soundid {
			if id == -1 {
				return 1;
			}
		}
		0
	}

	/// Gets a small object sound. Upstream `GetSmallObjectSound`.
	///
	/// Success: the data number, failure: -1. The range covers the additional small object
	/// as well as the fixed table.
	pub fn get_small_object_sound(&self, id: i32) -> i32 {
		if id < 0 || (TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT) as i32 <= id {
			return -1;
		}
		self.smallobject_sound[id as usize]
	}

	/// Releases all small object sounds at once. Upstream `CleanupSmallObjectSound`.
	pub fn cleanup_small_object_sound(&mut self, sound: &mut SoundControl) {
		for i in 0..(TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT) {
			sound.cleanup_sound(self.smallobject_sound[i]);
			self.smallobject_sound[i] = -1;
		}
	}

	/// Gets the model, texture and sound of an additional small object. Upstream
	/// `LoadAddSmallObject`.
	///
	/// The three paths come from the mission's file, so they are named at run time and
	/// nothing in the parameter tables is involved. `id` is the additional slot, which is
	/// normally 0: the file format defines one. Success: 0, failure: 1 or more, the count
	/// keeping its meaning from the other loaders, one per asset that would not load.
	///
	/// Upstream also refuses a `NULL` path, and a slice cannot be one, so those three
	/// guards are gone.
	pub fn load_add_small_object(
		&mut self,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundControl,
		id: i32,
		modelpath: &[u8],
		texturepath: &[u8],
		soundpath: &[u8],
	) -> i32 {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return 1;
		}

		let dataid = TOTAL_PARAMETERINFO_SMALLOBJECT + id as usize;
		let mut cnt = 0;

		self.smallobject_model[dataid] = d3dg.load_model(&path_from_bytes(modelpath));
		self.smallobject_texture[dataid] = d3dg.load_texture(texturepath, false, false);
		if self.smallobject_model[dataid] == -1 {
			cnt += 1;
		}
		if self.smallobject_texture[dataid] == -1 {
			cnt += 1;
		}
		self.smallobject_sound[dataid] = sound.load_sound(&path_from_bytes(soundpath));
		if self.smallobject_sound[dataid] == -1 {
			cnt += 1;
		}

		cnt
	}

	/// Releases the model, texture and sound of an additional small object. Upstream
	/// `CleanupAddSmallObject`.
	///
	/// The three ids are left as they are, which is what the upstream does: only the
	/// assets are released. The next mission's load writes over them.
	pub fn cleanup_add_small_object(
		&mut self,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundControl,
		id: i32,
	) {
		if id < 0 || MAX_ADDSMALLOBJECT as i32 <= id {
			return;
		}

		let dataid = TOTAL_PARAMETERINFO_SMALLOBJECT + id as usize;

		d3dg.cleanup_model(self.smallobject_model[dataid]);
		d3dg.cleanup_texture(self.smallobject_texture[dataid]);
		sound.cleanup_sound(self.smallobject_sound[dataid]);
	}

	// -- bullets ------------------------------------------------------------

	/// Loads the bullet models and textures. Upstream `LoadBulletModelTexture`.
	///
	/// Success: 0, failure: 1 or more, counted like the weapons. The second bullet is the
	/// hand grenade, whose model and texture are the grenade weapon's own.
	pub fn load_bullet_model_texture(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		let mut cnt = 0;

		for i in 0..TOTAL_PARAMETERINFO_BULLET {
			match self.param_info.get_bullet(i as i32) {
				Some(data) => {
					// Loads the model and texture, and counts them if an error occurs.
					self.bullet_model[i] = d3dg.load_model(&path_from_bytes(data.model));
					self.bullet_texture[i] = d3dg.load_texture(data.texture, false, false);
					if self.bullet_model[i] == -1 {
						cnt += 1;
					}
					if self.bullet_texture[i] == -1 {
						cnt += 1;
					}
				}
				// If the setting data is invalid, both the model and the texture are errors.
				None => cnt += 2,
			}
		}

		cnt
	}

	/// Gets the bullet model and texture. Upstream `GetBulletModelTexture`.
	pub fn get_bullet_model_texture(&self, id: i32) -> Option<(i32, i32)> {
		if id < 0 || TOTAL_PARAMETERINFO_BULLET as i32 <= id {
			return None;
		}

		Some((
			self.bullet_model[id as usize],
			self.bullet_texture[id as usize],
		))
	}

	/// Releases the bullet models and textures. Upstream `CleanupBulletModelTexture`.
	pub fn cleanup_bullet_model_texture(&mut self, d3dg: &mut D3DGraphics) {
		for i in 0..TOTAL_PARAMETERINFO_BULLET {
			d3dg.cleanup_model(self.bullet_model[i]);
			self.bullet_model[i] = -1;

			d3dg.cleanup_texture(self.bullet_texture[i]);
			self.bullet_texture[i] = -1;
		}
	}

	// -- scope --------------------------------------------------------------

	/// Loads the scope texture. Upstream `LoadScopeTexture`.
	///
	/// Success: 0, failure: 1. The file is a fixed one beside the data directory, not
	/// per-weapon: the scope overlay is the same picture for every scoped weapon.
	pub fn load_scope_texture(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		self.scopetexture = d3dg.load_texture(b"data\\scope.dds", false, false);
		if self.scopetexture == -1 {
			return 1;
		}
		0
	}

	/// Gets the scope texture. Upstream `GetScopeTexture`.
	pub fn get_scope_texture(&self) -> i32 {
		self.scopetexture
	}

	/// Releases the scope texture. Upstream `CleanupScopeTexture`.
	///
	/// Unlike the effect textures below, this one is not released when it is already
	/// empty, and the guard is kept so that the two read the same as upstream.
	pub fn cleanup_scope_texture(&mut self, d3dg: &mut D3DGraphics) {
		if self.scopetexture == -1 {
			return;
		}

		d3dg.cleanup_texture(self.scopetexture);
		self.scopetexture = -1;
	}

	// -- sky ----------------------------------------------------------------

	/// Loads the sky background model and texture. Upstream `LoadSkyModelTexture`.
	///
	/// Success: 0, failure: 1. `id` is the sky number, 1 to 5, and it picks the texture
	/// while the model is the same one every time. One model slot is shared by all five
	/// skies, so switching skies without a cleanup leaks the previous model: the game
	/// calls the cleanup first, which is why the id is not checked here.
	pub fn load_sky_model_texture(&mut self, d3dg: &mut D3DGraphics, id: i32) -> i32 {
		if !(1..=5).contains(&id) {
			return 1;
		}

		// Loads the model (fixed)
		self.skymodel = d3dg.load_model(&path_from_bytes(b"data\\sky\\sky.x"));

		// Generates the texture name and loads it
		let mut path = b"data\\sky\\sky".to_vec();
		path.extend_from_slice(id.to_string().as_bytes());
		path.extend_from_slice(b".bmp");
		self.skytexture = d3dg.load_texture(&path, false, false);

		if self.skymodel == -1 {
			return 1;
		}
		if self.skytexture == -1 {
			return 1;
		}
		0
	}

	/// Gets the sky background model and texture. Upstream `GetSkyModelTexture`, whose two
	/// out-parameters become this tuple.
	pub fn get_sky_model_texture(&self) -> (i32, i32) {
		(self.skymodel, self.skytexture)
	}

	/// Releases the sky background model and texture. Upstream `CleanupSkyModelTexture`.
	pub fn cleanup_sky_model_texture(&mut self, d3dg: &mut D3DGraphics) {
		if self.skymodel != -1 {
			d3dg.cleanup_model(self.skymodel);
			self.skymodel = -1;
		}
		if self.skytexture != -1 {
			d3dg.cleanup_texture(self.skytexture);
			self.skytexture = -1;
		}
	}

	// -- bullet sounds ------------------------------------------------------

	/// Loads the bullet and grenade sounds. Upstream `LoadBulletSound`.
	///
	/// Success: 0, failure: 1. These six are shared by every bullet and grenade in the
	/// game, so they are loaded once here rather than per shot.
	pub fn load_bullet_sound(&mut self, sound: &mut SoundControl) -> i32 {
		self.bullet_hitsound_a = sound.load_sound(&path_from_bytes(b"data\\sound\\hit1.wav"));
		self.bullet_hitsound_b = sound.load_sound(&path_from_bytes(b"data\\sound\\hit3.wav"));
		self.bullet_humanhitsound = sound.load_sound(&path_from_bytes(b"data\\sound\\hit2.wav"));
		self.bullet_passingsound = sound.load_sound(&path_from_bytes(b"data\\sound\\hyu.wav"));
		self.grenade_bang = sound.load_sound(&path_from_bytes(b"data\\sound\\bang.wav"));
		self.grenade_cco = sound.load_sound(&path_from_bytes(b"data\\sound\\cco.wav"));

		if self.bullet_hitsound_a == -1 {
			return 1;
		}
		if self.bullet_hitsound_b == -1 {
			return 1;
		}
		if self.bullet_humanhitsound == -1 {
			return 1;
		}
		if self.bullet_passingsound == -1 {
			return 1;
		}
		if self.grenade_bang == -1 {
			return 1;
		}
		if self.grenade_cco == -1 {
			return 1;
		}
		0
	}

	/// Gets the bullet and grenade sounds. Upstream `GetBulletSound`, whose six optional
	/// out-parameters become this tuple, in the order they are named there: the two bullet
	/// hit sounds, the human hit sound, the passing sound, the grenade explosion and the
	/// grenade bounce.
	///
	/// Upstream sets only the ones asked for and returns early without a sound class. The
	/// class is not a member here, so all six always come back.
	pub fn get_bullet_sound(&self) -> (i32, i32, i32, i32, i32, i32) {
		(
			self.bullet_hitsound_a,
			self.bullet_hitsound_b,
			self.bullet_humanhitsound,
			self.bullet_passingsound,
			self.grenade_bang,
			self.grenade_cco,
		)
	}

	/// Releases all bullet and grenade sounds at once. Upstream `CleanupBulletSound`.
	pub fn cleanup_bullet_sound(&mut self, sound: &mut SoundControl) {
		sound.cleanup_sound(self.bullet_hitsound_a);
		sound.cleanup_sound(self.bullet_hitsound_b);
		sound.cleanup_sound(self.bullet_humanhitsound);
		sound.cleanup_sound(self.bullet_passingsound);
		sound.cleanup_sound(self.grenade_bang);
		sound.cleanup_sound(self.grenade_cco);

		self.bullet_hitsound_a = -1;
		self.bullet_hitsound_b = -1;
		self.bullet_humanhitsound = -1;
		self.bullet_passingsound = -1;
		self.grenade_bang = -1;
		self.grenade_cco = -1;
	}

	// -- effects ------------------------------------------------------------

	/// Loads the effect textures. Upstream `LoadEffectTexture`.
	///
	/// Success: 0, failure: 1. The four are the blood, the muzzle flash, the smoke and the
	/// shell casing, all of them billboards drawn by the effect objects.
	pub fn load_effect_texture(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		self.effecttexture_blood = d3dg.load_texture(b"data\\blood.dds", false, false);
		self.effecttexture_mflash = d3dg.load_texture(b"data\\mflash.dds", false, false);
		self.effecttexture_smoke = d3dg.load_texture(b"data\\smoke.dds", false, false);
		self.effecttexture_yakkyou = d3dg.load_texture(b"data\\yakkyou.dds", false, false);

		if self.effecttexture_blood == -1 {
			return 1;
		}
		if self.effecttexture_mflash == -1 {
			return 1;
		}
		if self.effecttexture_smoke == -1 {
			return 1;
		}
		if self.effecttexture_yakkyou == -1 {
			return 1;
		}
		0
	}

	/// Gets the blood texture. Upstream `GetEffectBloodTexture`.
	pub fn get_effect_blood_texture(&self) -> i32 {
		self.effecttexture_blood
	}

	/// Gets the flash texture. Upstream `GetEffectMflashTexture`.
	pub fn get_effect_mflash_texture(&self) -> i32 {
		self.effecttexture_mflash
	}

	/// Gets the smoke texture. Upstream `GetEffectSmokeTexture`.
	pub fn get_effect_smoke_texture(&self) -> i32 {
		self.effecttexture_smoke
	}

	/// Gets the shell casing texture. Upstream `GetEffectYakkyouTexture`.
	pub fn get_effect_yakkyou_texture(&self) -> i32 {
		self.effecttexture_yakkyou
	}

	/// Releases the effect textures. Upstream `CleanupEffectTexture`.
	pub fn cleanup_effect_texture(&mut self, d3dg: &mut D3DGraphics) {
		if self.effecttexture_blood != -1 {
			d3dg.cleanup_texture(self.effecttexture_blood);
			self.effecttexture_blood = -1;
		}
		if self.effecttexture_mflash != -1 {
			d3dg.cleanup_texture(self.effecttexture_mflash);
			self.effecttexture_mflash = -1;
		}
		if self.effecttexture_smoke != -1 {
			d3dg.cleanup_texture(self.effecttexture_smoke);
			self.effecttexture_smoke = -1;
		}
		if self.effecttexture_yakkyou != -1 {
			d3dg.cleanup_texture(self.effecttexture_yakkyou);
			self.effecttexture_yakkyou = -1;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use xops_platform::gfx::Gfx;

	/// The renderer's own test helper, copied: no window and no surface.
	fn headless() -> D3DGraphics {
		let instance =
			xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
				backends: xops_platform::wgpu::Backends::all(),
				..Default::default()
			});
		let gfx = Gfx::new(&instance, None, 64, 48, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	#[test]
	fn the_constructor_marks_every_slot_empty() {
		let graphics = headless();
		let resource = ResourceManager::new();

		assert_eq!(resource.get_human_textures(), 0);
		assert_eq!(
			resource.get_human_model(),
			(
				[-1; TOTAL_UPMODE],
				[-1; TOTAL_ARMMODE],
				-1,
				[-1; TOTAL_WALKMODE],
				[-1; TOTAL_RUNMODE]
			)
		);
		assert_eq!(resource.get_human_texture_info(0), Some((-1, -1)));
		assert_eq!(resource.get_human_texture(0), -1);
		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 0),
			Some((-1, -1))
		);
		assert_eq!(resource.get_small_object_model_texture(0), Some((-1, -1)));
		assert_eq!(resource.get_bullet_model_texture(0), Some((-1, -1)));
		assert_eq!(resource.get_scope_texture(), -1);
		assert_eq!(resource.get_sky_model_texture(), (-1, -1));
		assert_eq!(resource.get_effect_blood_texture(), -1);
		assert_eq!(resource.get_effect_mflash_texture(), -1);
		assert_eq!(resource.get_effect_smoke_texture(), -1);
		assert_eq!(resource.get_effect_yakkyou_texture(), -1);
		assert_eq!(resource.get_bullet_sound(), (-1, -1, -1, -1, -1, -1));
		// The reload sound is the id -1 upstream, and it is never loaded.
		assert_eq!(resource.get_weapon_sound(-1), -1);
		assert_eq!(resource.get_small_object_sound(0), -1);
	}

	#[test]
	fn the_human_texture_arrays_stay_parallel() {
		let mut graphics = headless();
		let mut resource = ResourceManager::new();
		let params = ParameterInfo::new();

		// Humans 0, 2 and 5 name three different texture files and human 1 shares human
		// 0's. The table is the reference for those numbers, so they are read rather than
		// written out here.
		let first = params.get_human(0).unwrap().texture;
		let second = params.get_human(2).unwrap().texture;
		let third = params.get_human(5).unwrap().texture;
		assert_ne!(first, second);
		assert_ne!(second, third);
		assert_eq!(params.get_human(1).unwrap().texture, first);

		assert_eq!(resource.add_human_texture(&mut graphics, 0), 0);
		assert_eq!(resource.add_human_texture(&mut graphics, 2), 1);
		assert_eq!(resource.add_human_texture(&mut graphics, 5), 2);
		assert_eq!(resource.get_human_textures(), 3);

		// A human whose texture is already loaded takes no new slot, which is what keeps
		// the ten slots for ten pictures rather than for ten human types.
		assert_eq!(resource.add_human_texture(&mut graphics, 1), -1);
		assert_eq!(resource.get_human_textures(), 3);

		// Slot i of the two arrays belongs to slot i of the other, and the first value is
		// the texture path id the human table named.
		assert_eq!(resource.get_human_texture_info(0).unwrap().0, first);
		assert_eq!(resource.get_human_texture_info(1).unwrap().0, second);
		assert_eq!(resource.get_human_texture_info(2).unwrap().0, third);
		// There is no slot ten, which is where the debug console's page stops.
		assert_eq!(
			resource.get_human_texture_info(MAX_LOADHUMANTEXTURE as i32),
			None
		);
		assert_eq!(resource.get_human_texture_info(-1), None);

		// The human lookup walks the same two arrays, so every human type that names a
		// loaded picture answers with that picture's data number, including the human
		// that shares one.
		for (human, path_id) in [(0, first), (1, first), (2, second), (5, third)] {
			let slot = match path_id {
				id if id == first => 0,
				id if id == second => 1,
				_ => 2,
			};
			assert_eq!(
				resource.get_human_texture(human),
				resource.get_human_texture_info(slot).unwrap().1
			);
		}
		// A human type outside the table has no texture.
		assert_eq!(resource.get_human_texture(-1), -1);
		assert_eq!(
			resource.get_human_texture(TOTAL_PARAMETERINFO_HUMAN as i32),
			-1
		);
		assert_eq!(
			resource.add_human_texture(&mut graphics, TOTAL_PARAMETERINFO_HUMAN as i32),
			-1
		);

		resource.cleanup_human_texture(&mut graphics);
		assert_eq!(resource.get_human_textures(), 0);
		assert_eq!(resource.get_human_texture_info(0), Some((-1, -1)));
	}

	#[test]
	fn a_missing_asset_is_counted_and_still_handed_out_as_minus_one() {
		let mut graphics = headless();
		let mut sound = SoundControl::new();
		let mut resource = ResourceManager::new();

		// The additional small object is the one loader that takes names from the mission,
		// so it is the one place a name that cannot exist can be asked for.
		let code = resource.load_add_small_object(
			&mut graphics,
			&mut sound,
			0,
			b"no_such_model.x",
			b"no_such_texture.bmp",
			b"no_such_sound.wav",
		);
		// One count per asset, which is what the caller reads as "something is missing".
		assert_eq!(code, 3);

		// The data number the game asks for is the one past the fixed table, and the
		// lookup succeeds with the empty ids. That is the behaviour the callers are
		// written around: the object is added anyway and draws untextured.
		let dataid = TOTAL_PARAMETERINFO_SMALLOBJECT as i32;
		assert_eq!(
			resource.get_small_object_model_texture(dataid),
			Some((-1, -1))
		);
		assert_eq!(resource.get_small_object_sound(dataid), -1);

		// Nothing reached the renderer, and cleaning the slot up is a no-op rather than a
		// release of something that was never loaded.
		assert_eq!(graphics.get_total_models(), 0);
		assert_eq!(graphics.get_total_textures(), 0);
		resource.cleanup_add_small_object(&mut graphics, &mut sound, 0);

		// An id outside the additional slots is refused, and the slot past them is out of
		// the array's reach.
		assert_eq!(
			resource.load_add_small_object(
				&mut graphics,
				&mut sound,
				MAX_ADDSMALLOBJECT as i32,
				b"m.x",
				b"t.bmp",
				b"s.wav"
			),
			1
		);
		assert_eq!(resource.get_small_object_model_texture(dataid + 1), None);
	}

	#[test]
	fn the_fixed_assets_report_failure_when_their_files_are_absent() {
		let mut graphics = headless();
		let mut sound = SoundControl::new();
		let mut resource = ResourceManager::new();

		// The suite runs beside the crate, where there is no `data` directory: the state
		// the game is in before its data files are installed. Every fixed loader has to
		// report failure and leave its id empty rather than hand out a slot.
		assert_eq!(resource.load_human_model(&mut graphics), 0);
		assert_eq!(resource.load_scope_texture(&mut graphics), 1);
		assert_eq!(resource.get_scope_texture(), -1);
		assert_eq!(resource.load_effect_texture(&mut graphics), 1);
		assert_eq!(resource.get_effect_blood_texture(), -1);
		assert_eq!(resource.load_sky_model_texture(&mut graphics, 1), 1);
		assert_eq!(resource.get_sky_model_texture(), (-1, -1));
		// A sky number outside 1 to 5 is refused before anything is loaded.
		assert_eq!(resource.load_sky_model_texture(&mut graphics, 0), 1);
		assert_eq!(resource.load_sky_model_texture(&mut graphics, 6), 1);

		// The counting loaders keep going and count every asset they could not load: two
		// per entry, model and texture.
		assert_eq!(
			resource.load_weapon_model_texture(&mut graphics),
			(TOTAL_PARAMETERINFO_WEAPON * 2) as i32
		);
		assert_eq!(
			resource.load_small_object_model_texture(&mut graphics),
			(TOTAL_PARAMETERINFO_SMALLOBJECT * 2) as i32
		);
		assert_eq!(
			resource.load_bullet_model_texture(&mut graphics),
			(TOTAL_PARAMETERINFO_BULLET * 2) as i32
		);

		// The sound loaders have no output stream here and still report failure. The
		// weapon one fails on the reload sound, which is disabled upstream and is checked
		// anyway.
		assert_eq!(resource.load_weapon_sound(&mut sound), 1);
		assert_eq!(resource.load_small_object_sound(&mut sound), 1);
		assert_eq!(resource.load_bullet_sound(&mut sound), 1);

		// Releasing what was never loaded leaves the ids where they were.
		resource.destroy_resource(&mut graphics, &mut sound);
		assert_eq!(resource.get_human_textures(), 0);
		assert_eq!(resource.get_scope_texture(), -1);
		assert_eq!(resource.get_sky_model_texture(), (-1, -1));
		assert_eq!(resource.get_weapon_sound(-1), -1);
	}

	#[test]
	fn the_bug_weapon_fallback_names_the_map_textures() {
		let mut graphics = headless();
		let resource = ResourceManager::new();

		// The four ids are the ones the upstream hard-codes, and each names one of the
		// map's own textures. Those are the half of the fallback that can be pinned
		// without the model files.
		graphics.set_map_texture_id(0, 11);
		graphics.set_map_texture_id(3, 13);
		graphics.set_map_texture_id(2, 12);
		graphics.set_map_texture_id(8, 18);

		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 23),
			Some((-1, 11))
		);
		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 24),
			Some((-1, 13))
		);
		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 30),
			Some((-1, 12))
		);
		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 53),
			Some((-1, 18))
		);

		// The model half is the first human upper body model, which is empty until the
		// human models load.
		assert_eq!(resource.get_human_model().0[0], -1);

		// An id that is not one of the four is not a bug weapon, and a weapon id inside
		// the table is answered from the table.
		assert_eq!(resource.get_weapon_model_texture(&graphics, 26), None);
		assert_eq!(resource.get_bug_weapon_model_texture(&graphics, 26), None);
		assert_eq!(
			resource.get_weapon_model_texture(&graphics, 22),
			Some((-1, -1))
		);
		assert_eq!(resource.get_weapon_model_texture(&graphics, -1), None);
	}

	#[test]
	fn the_table_lookups_keep_their_bounds() {
		let graphics = headless();
		let resource = ResourceManager::new();

		assert_eq!(
			resource.get_weapon_model_texture(&graphics, TOTAL_PARAMETERINFO_WEAPON as i32 + 3),
			None
		);
		assert_eq!(resource.get_weapon_sound(-2), -1);
		assert_eq!(
			resource.get_weapon_sound(TOTAL_PARAMETERINFO_WEAPON as i32),
			-1
		);

		// The small object tables are one longer than the fixed settings, for the
		// additional object the mission names.
		let last = (TOTAL_PARAMETERINFO_SMALLOBJECT + MAX_ADDSMALLOBJECT) as i32 - 1;
		assert_eq!(
			resource.get_small_object_model_texture(last),
			Some((-1, -1))
		);
		assert_eq!(resource.get_small_object_model_texture(last + 1), None);
		assert_eq!(resource.get_small_object_model_texture(-1), None);
		assert_eq!(resource.get_small_object_sound(last), -1);
		assert_eq!(resource.get_small_object_sound(last + 1), -1);

		assert_eq!(resource.get_bullet_model_texture(-1), None);
		assert_eq!(
			resource.get_bullet_model_texture(TOTAL_PARAMETERINFO_BULLET as i32),
			None
		);
	}
}
