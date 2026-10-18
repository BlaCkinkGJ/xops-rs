//! `soundmanager.cpp` and `soundmanager.h`: the sound source table.
//!
//! Nothing plays a sound directly. A shot, a hit or a footstep is queued into the list for
//! the frame, and at the end of the frame [`SoundManager::play_world_sound`] swaps the two
//! lists and decides, from the camera position and the sound's own type, which of the
//! queued sources are close enough to be played and at what volume. The same table is what
//! the AI reads through [`SoundManager::get_world_sound`] to decide what it can hear, so
//! where a sound is heard is gameplay and not just mixing.
//!
//! The volumes here are the source's maximum. The distance attenuation itself is
//! `SoundControl::calculation_volume`, which this file is careful not to duplicate: the
//! constants below are what it is asked to attenuate.

use crate::collision::distance_pos_ray;
use crate::core::*;
use crate::parameter::ParameterInfo;
use crate::resource::ResourceManager;
use crate::sound::{SoundControl, MAX_SOUNDDIST};
use crate::window::get_rand;

/// Maximum number of entries in the sound list, `soundmanager.h`.
pub const MAX_SOUNDMGR_LIST: usize = 100;

/// Maximum volume of the map impact sound.
pub const MAX_SOUNDHITMAP: i32 = 95;
/// Maximum volume of the small object impact sound.
pub const MAX_SOUNDHITSMALLOBJ: i32 = 110;
/// Maximum volume of the human impact (taking a hit) sound.
pub const MAX_SOUNDHITHUMAN: i32 = 75;
/// Maximum volume of the bullet passing sound.
pub const MAX_SOUNDPASSING: i32 = 80;
/// Maximum volume of the grenade bound and bounce sound.
pub const MAX_SOUNDCCOGRENADE: i32 = 100;
/// Maximum volume of the grenade explosion sound.
pub const MAX_SOUNDHITGRENADE: i32 = 120;

/// Constant for the sound list member `paramid`.
///
/// The variants are the upstream `SoundmgrID` constants, and each keeps the upstream's
/// spelling in its doc comment because the name is what the game's own sources and the
/// manual call it by. The discriminants are what the game compares against, so they stay
/// exactly as they are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum SoundmgrID {
	/// `SHOT_WEAPON`: gunshot sound
	#[default]
	ShotWeapon = 0,
	/// `SHOT_WEAPON_PLAYER`: the player's own gunshot sound
	ShotWeaponPlayer,
	/// `HIT_MAP`: map impact sound
	HitMap,
	/// `HIT_HUMAN_HEAD`: hit sound (head)
	HitHumanHead,
	/// `HIT_HUMAN_UP`: hit sound (upper body)
	HitHumanUp,
	/// `HIT_HUMAN_LEG`: hit sound (lower body)
	HitHumanLeg,
	/// `HIT_HUMAN_ZOMBIE`: zombie attack sound
	HitHumanZombie,
	/// `HIT_SMALLOBJECT`: small object destruction sound
	HitSmallobject,
	/// `BULLET`: bullet sound, passing sound
	Bullet,
	/// `GRE_BOUND`: grenade bound sound
	GreBound,
	/// `GRE_EXPLOSION`: grenade explosion sound
	GreExplosion,
	/// `FOOTSTEPS_WALK`: footsteps, walking
	FootstepsWalk,
	/// `FOOTSTEPS_FORWARD`: footsteps, running sound (forward)
	FootstepsForward,
	/// `FOOTSTEPS_BACK`: footsteps, running sound (backward)
	FootstepsBack,
	/// `FOOTSTEPS_SIDE`: footsteps, running sound (sideways)
	FootstepsSide,
	/// `FOOTSTEPS_JUMP`: footsteps, jumping
	FootstepsJump,
	/// `FOOTSTEPS_LANDING`: footsteps, landing sound
	FootstepsLanding,
	/// `WEAPON_RELOAD`: reload sound
	WeaponReload,
}

/// Structure for the sound list.
///
/// Upstream leaves the members a queueing call does not set as whatever the allocation
/// held, and no reader looks at one: the movement is only read for a bullet and the count
/// only for footsteps. They start at zero here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Soundlist {
	/// sound source type number, a [`SoundmgrID`] constant
	pub paramid: SoundmgrID,
	/// data number
	pub dataid: i32,
	/// X coordinate
	pub x: f32,
	/// Y coordinate
	pub y: f32,
	/// Z coordinate
	pub z: f32,
	/// X coordinate movement over one frame
	pub move_x: f32,
	/// Y coordinate movement over one frame
	pub move_y: f32,
	/// Z coordinate movement over one frame
	pub move_z: f32,
	/// team number
	pub teamid: i32,
	/// count value
	pub cnt: i32,
}

/// Sound management class.
///
/// Manages each sound, including its initialization, calculation and playback.
pub struct SoundManager<'a> {
	/// sound playback class
	sound_ctrl: Option<&'a mut SoundControl>,
	/// resource management class
	resource: Option<&'a ResourceManager>,
	/// class that manages the settings values
	param: Option<&'a ParameterInfo>,

	/// sound list A
	sound_list_a: [Soundlist; MAX_SOUNDMGR_LIST],
	/// sound list B
	sound_list_b: [Soundlist; MAX_SOUNDMGR_LIST],
	/// swapping of the sound lists
	change_ab: bool,
	/// number of entries in sound list A
	list_a_datas: i32,
	/// number of entries in sound list B
	list_b_datas: i32,
}

impl<'a> SoundManager<'a> {
	/// Constructor. Upstream the three pointers default to NULL and `SetClass` fills them
	/// in later.
	pub fn new(
		in_sound_ctrl: Option<&'a mut SoundControl>,
		in_resource: Option<&'a ResourceManager>,
		in_param: Option<&'a ParameterInfo>,
	) -> SoundManager<'a> {
		SoundManager {
			sound_ctrl: in_sound_ctrl,
			resource: in_resource,
			param: in_param,
			sound_list_a: [Soundlist::default(); MAX_SOUNDMGR_LIST],
			sound_list_b: [Soundlist::default(); MAX_SOUNDMGR_LIST],
			change_ab: false,
			list_a_datas: 0,
			list_b_datas: 0,
		}
	}

	/// `SoundManager::SetClass`.
	pub fn set_class(
		&mut self,
		in_sound_ctrl: Option<&'a mut SoundControl>,
		in_resource: Option<&'a ResourceManager>,
		in_param: Option<&'a ParameterInfo>,
	) {
		self.sound_ctrl = in_sound_ctrl;
		self.resource = in_resource;
		self.param = in_param;
	}

	/// The sound class the manager was built over.
	///
	/// Upstream the asset loaders and the debug console reach the file scope `SoundCtrl`
	/// directly, and this is that reach: the game screen loads and releases an additional
	/// small object's sound through the resource class, which needs the control rather
	/// than the manager, and reports the loaded sound count from it.
	pub fn sound_ctrl(&mut self) -> Option<&mut SoundControl> {
		self.sound_ctrl.as_deref_mut()
	}

	/// `SoundManager::InitWorldSound`: forget everything queued.
	pub fn init_world_sound(&mut self) {
		self.change_ab = false;
		self.list_a_datas = 0;
		self.list_b_datas = 0;
	}

	/// `SoundManager::ShotWeapon`: add a gunshot sound to the world.
	pub fn shot_weapon(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		id: i32,
		team_id: i32,
		player: bool,
	) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = if player {
			SoundmgrID::ShotWeaponPlayer
		} else {
			SoundmgrID::ShotWeapon
		};
		plist.dataid = id;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::HitMap`: add a map impact sound to the world.
	pub fn hit_map(&mut self, x: f32, y: f32, z: f32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::HitMap;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::HitHuman`: add a hit sound to the world.
	///
	/// `hit_id` is the hit location: head 0, body 1, leg 2, zombie attack 3.
	pub fn hit_human(&mut self, x: f32, y: f32, z: f32, hit_id: i32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		if !(0..=3).contains(&hit_id) {
			return false;
		}

		plist.paramid = match hit_id {
			0 => SoundmgrID::HitHumanHead,
			1 => SoundmgrID::HitHumanUp,
			2 => SoundmgrID::HitHumanLeg,
			_ => SoundmgrID::HitHumanZombie,
		};
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::HitSmallObject`: add a small object impact sound to the world.
	pub fn hit_small_object(&mut self, x: f32, y: f32, z: f32, id: i32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::HitSmallobject;
		plist.dataid = id;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::PassingBullet`: add a bullet passing or crossing sound.
	///
	/// The movement is the amount for one frame, and the coordinates are not moved by it.
	pub fn passing_bullet(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		move_x: f32,
		move_y: f32,
		move_z: f32,
		team_id: i32,
	) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::Bullet;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.move_x = move_x;
		plist.move_y = move_y;
		plist.move_z = move_z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::GrenadeBound`: add a grenade bound and bounce sound to the world.
	pub fn grenade_bound(&mut self, x: f32, y: f32, z: f32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::GreBound;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::GrenadeExplosion`: add a grenade explosion sound to the world.
	pub fn grenade_explosion(&mut self, x: f32, y: f32, z: f32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::GreExplosion;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::SetFootsteps`: add a footstep sound to the world.
	///
	/// `move_mode` is the movement mode: walking 0, forward 1, backward 2, sideways 3,
	/// jump 4, landing 5. `cnt` is the movement motion count value.
	pub fn set_footsteps(
		&mut self,
		x: f32,
		y: f32,
		z: f32,
		id: i32,
		move_mode: i32,
		team_id: i32,
		cnt: i32,
	) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = match move_mode {
			0 => SoundmgrID::FootstepsWalk,
			1 => SoundmgrID::FootstepsForward,
			2 => SoundmgrID::FootstepsBack,
			3 => SoundmgrID::FootstepsSide,
			4 => SoundmgrID::FootstepsJump,
			5 => SoundmgrID::FootstepsLanding,
			_ => return false,
		};
		plist.dataid = id;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;
		plist.cnt = cnt;

		true
	}

	/// `SoundManager::ReloadWeapon`: add a weapon reload sound to the world.
	pub fn reload_weapon(&mut self, x: f32, y: f32, z: f32, team_id: i32) -> bool {
		let Some(plist) = self.get_new_list() else {
			return false;
		};

		plist.paramid = SoundmgrID::WeaponReload;
		plist.x = x;
		plist.y = y;
		plist.z = z;
		plist.teamid = team_id;

		true
	}

	/// `SoundManager::GetTotalSoundList`: how many sources the list being processed holds.
	pub fn get_total_sound_list(&self) -> i32 {
		if self.change_ab == false {
			self.list_b_datas
		} else {
			self.list_a_datas
		}
	}

	/// `SoundManager::GetWorldSound`: the sources around a position that can be heard
	/// from it.
	///
	/// Writes them into `psoundlist` and returns how many there are. The upstream caller
	/// passes an array of [`MAX_SOUNDMGR_LIST`], and no more than that many can be written
	/// because queueing more than 99 in a frame is refused.
	pub fn get_world_sound(
		&self,
		pos_x: f32,
		pos_y: f32,
		pos_z: f32,
		team_id: i32,
		psoundlist: &mut [Soundlist],
	) -> i32 {
		let getlist = self.get_target_list();
		let mut newlists = 0usize;

		for i in 0..getlist.len() {
			// Standard value for the detection distance, changed per sound source type.
			let mut maxdist = 20.0f32;

			match getlist[i].paramid {
				SoundmgrID::ShotWeapon | SoundmgrID::ShotWeaponPlayer => {
					// An unknown weapon id answers with no silencer, where the upstream
					// would leave `WParam` as the stack held it.
					let silencer = self
						.param
						.and_then(|param| param.get_weapon(getlist[i].dataid))
						.map_or(false, |weapon| weapon.silencer);

					if getlist[i].teamid == team_id {
						maxdist = 20.0;
					} else if silencer {
						maxdist = 30.0;
					} else {
						maxdist = 120.0;
					}
				}
				SoundmgrID::HitMap => maxdist = 30.0,
				SoundmgrID::HitHumanHead => maxdist = 60.0,
				SoundmgrID::HitHumanUp => maxdist = 50.0,
				SoundmgrID::HitHumanLeg => maxdist = 40.0,
				SoundmgrID::HitHumanZombie => maxdist = 40.0,
				SoundmgrID::FootstepsWalk => continue,
				SoundmgrID::FootstepsForward => {
					if getlist[i].teamid == team_id {
						continue;
					}
					maxdist = 40.0;
				}
				SoundmgrID::FootstepsBack => {
					if getlist[i].teamid == team_id {
						continue;
					}
					maxdist = 30.0;
				}
				SoundmgrID::FootstepsSide => {
					if getlist[i].teamid == team_id {
						continue;
					}
					maxdist = 35.0;
				}
				SoundmgrID::FootstepsJump | SoundmgrID::FootstepsLanding => continue,
				SoundmgrID::GreBound => continue,
				SoundmgrID::GreExplosion => maxdist = 80.0,
				SoundmgrID::WeaponReload => continue,
				SoundmgrID::HitSmallobject | SoundmgrID::Bullet => {}
			}

			if ENABLE_BUG_TEAMID {
				// If the team number is negative and the sound source team number is
				// greater, ignore the sound source.
				if team_id < 0 && getlist[i].teamid < 0 && team_id < getlist[i].teamid {
					continue;
				}
			}

			if getlist[i].paramid == SoundmgrID::Bullet {
				// Where the bullet came closest to the listener.
				let Some((min_x, min_y, min_z)) =
					check_approach(&getlist[i], pos_x, pos_y, pos_z)
				else {
					continue;
				};

				let x = min_x - pos_x;
				let y = min_y - pos_y;
				let z = min_z - pos_z;
				if x * x + y * y + z * z < maxdist * maxdist {
					psoundlist[newlists] = Soundlist {
						paramid: getlist[i].paramid,
						dataid: getlist[i].dataid,
						x: min_x,
						y: min_y,
						z: min_z,
						..Default::default()
					};
					newlists += 1;
				}
			} else {
				let x = getlist[i].x - pos_x;
				let y = getlist[i].y - pos_y;
				let z = getlist[i].z - pos_z;
				if x * x + y * y + z * z < maxdist * maxdist {
					psoundlist[newlists] = Soundlist {
						paramid: getlist[i].paramid,
						dataid: getlist[i].dataid,
						x: getlist[i].x,
						y: getlist[i].y,
						z: getlist[i].z,
						..Default::default()
					};
					newlists += 1;
				}
			}
		}

		newlists as i32
	}

	/// Reaches the device, which is the upstream `SoundCtrl->SetVolume(volume)` as called
	/// from the options screen when the settings are saved.
	///
	/// The manager holds the only `&mut SoundControl`, so a screen that has the manager in
	/// hand routes through it. Upstream the volume is set to zero when the sound flag is
	/// off and to one when it is on, and the control clamps what it is given.
	pub fn set_volume(&mut self, volume: f32) {
		if let Some(sound_ctrl) = self.sound_ctrl.as_mut() {
			sound_ctrl.set_volume(volume);
		}
	}

	/// `SoundManager::PlayWorldSound`: play the sounds queued for the frame that just
	/// ended. Called once per frame.
	pub fn play_world_sound(
		&mut self,
		camera_x: f32,
		camera_y: f32,
		camera_z: f32,
		camera_rx: f32,
		team_id: i32,
	) {
		// Swap the lists. The one being replaced is the one the frame after this will
		// queue into, so it is cleared before it can be read again.
		if self.change_ab == false {
			self.list_b_datas = 0;
			self.change_ab = true;
		} else {
			self.list_a_datas = 0;
			self.change_ab = false;
		}

		if let Some(sound_ctrl) = self.sound_ctrl.as_mut() {
			sound_ctrl.set_camera(camera_x, camera_y, camera_z, camera_rx);
		}

		let lists = self.get_target_list().len();
		for i in 0..lists {
			// A copy, so that playing one source can borrow the sound class for the
			// duration without holding the list borrowed as well.
			let plist = self.get_target_list()[i];

			let x = plist.x - camera_x;
			let y = plist.y - camera_y;
			let z = plist.z - camera_z;

			// If the sound source is within range, try to play it. The upstream mixes the
			// int constant into this float comparison, which is the same value.
			if x * x + y * y + z * z < (MAX_SOUNDDIST * MAX_SOUNDDIST) as f32 {
				self.play_sound(&plist, camera_x, camera_y, camera_z, team_id);
			}
		}
	}

	/// `SoundManager::GetNewList`: the next free entry of the list being queued into.
	fn get_new_list(&mut self) -> Option<&mut Soundlist> {
		if self.change_ab == false {
			if self.list_a_datas + 1 >= MAX_SOUNDMGR_LIST as i32 {
				return None;
			}
			let index = self.list_a_datas as usize;
			self.list_a_datas += 1;
			Some(&mut self.sound_list_a[index])
		} else {
			if self.list_b_datas + 1 >= MAX_SOUNDMGR_LIST as i32 {
				return None;
			}
			let index = self.list_b_datas as usize;
			self.list_b_datas += 1;
			Some(&mut self.sound_list_b[index])
		}
	}

	/// `SoundManager::GetTargetList`: the list to process this frame.
	fn get_target_list(&self) -> &[Soundlist] {
		if self.change_ab == false {
			&self.sound_list_b[..self.list_b_datas as usize]
		} else {
			&self.sound_list_a[..self.list_a_datas as usize]
		}
	}

	/// `SoundManager::PlaySound`: decide the conditions for one queued source and play it.
	fn play_sound(
		&mut self,
		plist: &Soundlist,
		camera_x: f32,
		camera_y: f32,
		camera_z: f32,
		team_id: i32,
	) {
		// Every arm below either sets both of these or returns, so the upstream's
		// initialisers are not needed and the compiler checks the claim.
		let id;
		let volume;

		// Set the playback conditions.
		match plist.paramid {
			// gunshot sound
			SoundmgrID::ShotWeapon => {
				let Some(weapon) = self.param.and_then(|param| param.get_weapon(plist.dataid))
				else {
					return;
				};
				if weapon.soundvolume == 0 {
					return;
				}

				let Some(resource) = self.resource else {
					return;
				};
				id = resource.get_weapon_sound(plist.dataid);
				volume = weapon.soundvolume;
			}

			// the player's own gunshot sound
			SoundmgrID::ShotWeaponPlayer => {
				let Some(weapon) = self.param.and_then(|param| param.get_weapon(plist.dataid))
				else {
					return;
				};
				if weapon.soundvolume == 0 {
					return;
				}

				// Play as is without making it a 3D sound source, then end.
				if let (Some(sound_ctrl), Some(resource)) = (self.sound_ctrl.as_mut(), self.resource)
				{
					sound_ctrl.play_sound(
						resource.get_weapon_sound(plist.dataid),
						weapon.soundvolume,
						0,
					);
				}
				return;
			}

			// map impact sound
			SoundmgrID::HitMap => {
				let Some(resource) = self.resource else {
					return;
				};
				// The upstream passes NULL for the four sounds this one does not use.
				let (hitsound_a, hitsound_b, ..) = resource.get_bullet_sound();
				id = if get_rand(3) == 0 { hitsound_b } else { hitsound_a };
				volume = MAX_SOUNDHITMAP;
			}

			// hit sound, for every hit location
			SoundmgrID::HitHumanHead
			| SoundmgrID::HitHumanUp
			| SoundmgrID::HitHumanLeg
			| SoundmgrID::HitHumanZombie => {
				let Some(resource) = self.resource else {
					return;
				};
				let (_, _, human_hitsound, ..) = resource.get_bullet_sound();
				id = human_hitsound;
				volume = MAX_SOUNDHITHUMAN;
			}

			// small object destruction sound
			SoundmgrID::HitSmallobject => {
				let Some(resource) = self.resource else {
					return;
				};
				id = resource.get_small_object_sound(plist.dataid);
				volume = MAX_SOUNDHITSMALLOBJ;
			}

			// bullet sound, passing sound
			SoundmgrID::Bullet => {
				let Some((new_x, new_y, new_z)) = check_approach(plist, camera_x, camera_y, camera_z)
				else {
					return;
				};

				// If it is a friendly bullet, do nothing and end.
				if plist.teamid == team_id {
					return;
				}

				let Some(resource) = self.resource else {
					return;
				};
				let (_, _, _, passing_sound, ..) = resource.get_bullet_sound();
				if let Some(sound_ctrl) = self.sound_ctrl.as_mut() {
					sound_ctrl.play_3d_sound(
						passing_sound,
						new_x,
						new_y,
						new_z,
						MAX_SOUNDPASSING,
					);
				}
				return;
			}

			// grenade bound sound
			SoundmgrID::GreBound => {
				let Some(resource) = self.resource else {
					return;
				};
				let (.., grenade_cco) = resource.get_bullet_sound();
				id = grenade_cco;
				volume = MAX_SOUNDCCOGRENADE;
			}

			// grenade explosion sound
			SoundmgrID::GreExplosion => {
				let Some(resource) = self.resource else {
					return;
				};
				let (_, _, _, _, grenade_bang, _) = resource.get_bullet_sound();
				id = grenade_bang;
				volume = MAX_SOUNDHITGRENADE;
			}

			// footsteps, walking, forward, backward and sideways
			SoundmgrID::FootstepsWalk
			| SoundmgrID::FootstepsForward
			| SoundmgrID::FootstepsBack
			| SoundmgrID::FootstepsSide => {
				// The footstep sounds are not played upstream either: this is where the
				// processing would load the footstep data and play from `dataid` at fixed
				// intervals taken from `cnt`.
				return;
			}

			// the jump and landing sounds
			SoundmgrID::FootstepsJump | SoundmgrID::FootstepsLanding => {
				// Not played upstream either, and `dataid` is what the processing would
				// use to pick between them.
				return;
			}

			// reload sound
			SoundmgrID::WeaponReload => {
				let Some(resource) = self.resource else {
					return;
				};
				id = resource.get_weapon_sound(-1);
				volume = 100;
			}
		}

		// Play.
		if let Some(sound_ctrl) = self.sound_ctrl.as_mut() {
			sound_ctrl.play_3d_sound(id, plist.x, plist.y, plist.z, volume);
		}
	}
}

/// `SoundManager::CheckApproach`: whether the source comes closest to the camera this
/// frame, and where.
///
/// A bullet is only heard as it passes, so this is the test for that and the returned
/// point is the one played at.
fn check_approach(
	plist: &Soundlist,
	camera_x: f32,
	camera_y: f32,
	camera_z: f32,
) -> Option<(f32, f32, f32)> {
	// Distance one frame before.
	let x = camera_x - (plist.x - plist.move_x);
	let y = camera_y - (plist.y - plist.move_y);
	let z = camera_z - (plist.z - plist.move_z);
	let dist1 = x * x + y * y + z * z;

	// Distance at the current position.
	let x = camera_x - plist.x;
	let y = camera_y - plist.y;
	let z = camera_z - plist.z;
	let dist2 = x * x + y * y + z * z;

	// Distance one frame after.
	let x = camera_x - (plist.x + plist.move_x);
	let y = camera_y - (plist.y + plist.move_y);
	let z = camera_z - (plist.z + plist.move_z);
	let dist3 = x * x + y * y + z * z;

	// Only when the current position is the closest of the three.
	if dist1 > dist2 && dist2 < dist3 {
		// Find the movement speed.
		let speed =
			(plist.move_x * plist.move_x + plist.move_y * plist.move_y + plist.move_z * plist.move_z)
				.sqrt();

		// Find the coordinate of the shortest distance.
		let min_dist = distance_pos_ray(
			camera_x,
			camera_y,
			camera_z,
			plist.x,
			plist.y,
			plist.z,
			plist.move_x / speed,
			plist.move_y / speed,
			plist.move_z / speed,
		);

		let dist = (dist2 - min_dist * min_dist).sqrt();
		return Some((
			plist.x + plist.move_x / speed * dist,
			plist.y + plist.move_y / speed * dist,
			plist.z + plist.move_z / speed * dist,
		));
	}

	None
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Weapon 1, `MP5`: a gunshot with the standard volume and no silencer.
	const WEAPON_MP5: i32 = 1;
	/// Weapon 14, `MP5SD`: the silenced gunshot.
	const WEAPON_MP5SD: i32 = 14;
	/// Weapon 0, `NONE`: a gunshot with a volume of zero, so it never plays.
	const WEAPON_NONE: i32 = 0;

	static PARAM: ParameterInfo = ParameterInfo;

	/// A sound manager with the parameter table but no device, so the arithmetic can be
	/// checked without anything being played.
	fn manager() -> SoundManager<'static> {
		SoundManager::new(None, None, Some(&PARAM))
	}

	/// Swaps the lists the way the end of a frame does, so that what was just queued is
	/// the list `get_world_sound` reads.
	fn swap(sound: &mut SoundManager) {
		sound.play_world_sound(0.0, 0.0, 0.0, 0.0, 1);
	}

	/// How many of the queued sources a listener at `distance` along X can hear.
	fn heard(sound: &SoundManager, distance: f32, team_id: i32) -> i32 {
		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		sound.get_world_sound(distance, 0.0, 0.0, team_id, &mut out)
	}

	#[test]
	fn the_source_volumes_are_the_upstream_numbers() {
		assert_eq!(MAX_SOUNDHITMAP, 95);
		assert_eq!(MAX_SOUNDHITSMALLOBJ, 110);
		assert_eq!(MAX_SOUNDHITHUMAN, 75);
		assert_eq!(MAX_SOUNDPASSING, 80);
		assert_eq!(MAX_SOUNDCCOGRENADE, 100);
		assert_eq!(MAX_SOUNDHITGRENADE, 120);
		assert_eq!(MAX_SOUNDMGR_LIST, 100);
	}

	#[test]
	fn a_gunshot_carries_by_team_and_silencer() {
		// The player's own team hears a shot out to 20 units, whatever the weapon.
		let mut sound = manager();
		assert!(sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5, 1, false));
		swap(&mut sound);
		assert_eq!(heard(&sound, 19.0, 1), 1);
		assert_eq!(heard(&sound, 21.0, 1), 0);

		// Another team hears an unsilenced shot out to 120.
		let mut sound = manager();
		assert!(sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5, 1, false));
		swap(&mut sound);
		assert_eq!(heard(&sound, 119.0, 2), 1);
		assert_eq!(heard(&sound, 121.0, 2), 0);

		// The silenced MP5SD only carries 30.
		let mut sound = manager();
		assert!(sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5SD, 1, false));
		swap(&mut sound);
		assert_eq!(heard(&sound, 29.0, 2), 1);
		assert_eq!(heard(&sound, 31.0, 2), 0);
	}

	#[test]
	fn the_player_gunshot_is_the_same_distance_as_any_other() {
		let mut sound = manager();
		assert!(sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5, 1, true));
		swap(&mut sound);

		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		assert_eq!(sound.get_world_sound(0.0, 0.0, 0.0, 1, &mut out), 1);
		assert_eq!(out[0].paramid, SoundmgrID::ShotWeaponPlayer);
		assert_eq!(out[0].dataid, WEAPON_MP5);
	}

	#[test]
	fn each_hit_type_has_its_own_distance() {
		let cases = [
			(SoundmgrID::HitMap, 30.0),
			(SoundmgrID::HitHumanHead, 60.0),
			(SoundmgrID::HitHumanUp, 50.0),
			(SoundmgrID::HitHumanLeg, 40.0),
			(SoundmgrID::HitHumanZombie, 40.0),
			(SoundmgrID::HitSmallobject, 20.0),
			(SoundmgrID::GreExplosion, 80.0),
		];

		for (paramid, maxdist) in cases {
			let mut sound = manager();
			let queued = match paramid {
				SoundmgrID::HitMap => sound.hit_map(0.0, 0.0, 0.0, 1),
				SoundmgrID::HitHumanHead => sound.hit_human(0.0, 0.0, 0.0, 0, 1),
				SoundmgrID::HitHumanUp => sound.hit_human(0.0, 0.0, 0.0, 1, 1),
				SoundmgrID::HitHumanLeg => sound.hit_human(0.0, 0.0, 0.0, 2, 1),
				SoundmgrID::HitHumanZombie => sound.hit_human(0.0, 0.0, 0.0, 3, 1),
				SoundmgrID::HitSmallobject => sound.hit_small_object(0.0, 0.0, 0.0, 0, 1),
				_ => sound.grenade_explosion(0.0, 0.0, 0.0, 1),
			};
			assert!(queued, "{paramid:?} was not queued");
			swap(&mut sound);

			// Inside the distance is heard, and the limit itself is not: the upstream
			// compares the squared distance with a strict `<`.
			assert_eq!(heard(&sound, maxdist - 1.0, 1), 1, "{paramid:?} inside");
			assert_eq!(heard(&sound, maxdist, 1), 0, "{paramid:?} at the limit");
			assert_eq!(heard(&sound, maxdist + 1.0, 1), 0, "{paramid:?} outside");
		}
	}

	#[test]
	fn the_sounds_the_upstream_never_plays_are_never_heard() {
		// Queueing a footstep, a grenade bounce or a reload is not an error, it just
		// never reaches the listener.
		let mut sound = manager();
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 0, 1, 0));
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 4, 1, 0));
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 5, 1, 0));
		assert!(sound.grenade_bound(0.0, 0.0, 0.0, 1));
		assert!(sound.reload_weapon(0.0, 0.0, 0.0, 1));
		swap(&mut sound);

		assert_eq!(sound.get_total_sound_list(), 5);
		assert_eq!(heard(&sound, 0.0, 1), 0);
		assert_eq!(heard(&sound, 0.0, 2), 0);

		// The footsteps that another team can hear are the running ones.
		let mut sound = manager();
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 1, 2, 0)); // forward, same team
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 1, 3, 0)); // forward, other team
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 2, 3, 0)); // backward, other team
		assert!(sound.set_footsteps(0.0, 0.0, 0.0, 0, 3, 3, 0)); // sideways, other team
		swap(&mut sound);

		// At 34 units: forward carries 40 and sideways 35, but backward only 30, and the
		// same team's copy is skipped outright.
		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		assert_eq!(sound.get_world_sound(34.0, 0.0, 0.0, 2, &mut out), 2);
		assert_eq!(out[0].paramid, SoundmgrID::FootstepsForward);
		assert_eq!(out[1].paramid, SoundmgrID::FootstepsSide);

		// And at 29 units the backward one joins them.
		assert_eq!(sound.get_world_sound(29.0, 0.0, 0.0, 2, &mut out), 3);
		assert_eq!(out[1].paramid, SoundmgrID::FootstepsBack);
	}

	#[test]
	fn a_bad_footstep_mode_or_hit_location_is_refused() {
		let mut sound = manager();
		assert!(!sound.set_footsteps(0.0, 0.0, 0.0, 0, 6, 1, 0));
		assert!(!sound.hit_human(0.0, 0.0, 0.0, 4, 1));
		assert!(!sound.hit_human(0.0, 0.0, 0.0, -1, 1));
		assert_eq!(sound.get_total_sound_list(), 0);
	}

	#[test]
	fn a_negative_team_number_ignores_the_louder_company() {
		// With ENABLE_BUG_TEAMID on, a negative listener team ignores a negative source
		// team that is greater than it.
		assert!(ENABLE_BUG_TEAMID);

		let mut sound = manager();
		assert!(sound.hit_map(0.0, 0.0, 0.0, -1));
		swap(&mut sound);

		assert_eq!(heard(&sound, 0.0, -2), 0);
		assert_eq!(heard(&sound, 0.0, -1), 1);
	}

	#[test]
	fn a_bullet_is_heard_at_its_closest_point() {
		let mut sound = manager();
		// The bullet is at the camera on this frame and a step past it either way, which
		// is the frame the upstream plays its passing sound.
		assert!(sound.passing_bullet(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1));
		swap(&mut sound);

		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		assert_eq!(sound.get_world_sound(0.0, 0.0, 0.0, 2, &mut out), 1);
		assert_eq!(out[0].paramid, SoundmgrID::Bullet);
		assert_eq!((out[0].x, out[0].y, out[0].z), (0.0, 0.0, 0.0));

		// A bullet running away from the listener is never reported, so the AI hears
		// nothing of a shot fired in the other direction.
		let mut sound = manager();
		assert!(sound.passing_bullet(10.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1));
		swap(&mut sound);
		assert_eq!(heard(&sound, 0.0, 2), 0);
	}

	#[test]
	fn the_lists_swap_each_frame() {
		let mut sound = manager();
		assert_eq!(sound.get_total_sound_list(), 0);

		// Queued during the frame, and visible to the listener only after the swap.
		assert!(sound.hit_map(0.0, 0.0, 0.0, 1));
		assert_eq!(sound.get_total_sound_list(), 0);
		swap(&mut sound);
		assert_eq!(sound.get_total_sound_list(), 1);

		// The next frame queues into the other list and the processed one is cleared.
		assert!(sound.hit_map(1.0, 0.0, 0.0, 1));
		assert_eq!(sound.get_total_sound_list(), 1);
		swap(&mut sound);
		assert_eq!(sound.get_total_sound_list(), 1);

		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		assert_eq!(sound.get_world_sound(1.0, 0.0, 0.0, 1, &mut out), 1);
		assert_eq!(out[0].x, 1.0);

		sound.init_world_sound();
		assert_eq!(sound.get_total_sound_list(), 0);
	}

	#[test]
	fn the_list_refuses_to_overflow() {
		let mut sound = manager();
		// Upstream stops one short of the array size, so the hundredth is refused.
		for _ in 0..MAX_SOUNDMGR_LIST - 1 {
			assert!(sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5, 1, false));
		}
		assert!(!sound.shot_weapon(0.0, 0.0, 0.0, WEAPON_MP5, 1, false));
		assert_eq!(sound.list_a_datas, MAX_SOUNDMGR_LIST as i32 - 1);
	}

	#[test]
	fn a_hit_location_picks_its_own_sound() {
		let mut sound = manager();
		assert!(sound.hit_human(0.0, 0.0, 0.0, 0, 1));
		assert!(sound.hit_human(0.0, 0.0, 0.0, 1, 1));
		assert!(sound.hit_human(0.0, 0.0, 0.0, 2, 1));
		assert!(sound.hit_human(0.0, 0.0, 0.0, 3, 1));
		swap(&mut sound);

		let mut out = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		assert_eq!(sound.get_world_sound(0.0, 0.0, 0.0, 1, &mut out), 4);
		assert_eq!(out[0].paramid, SoundmgrID::HitHumanHead);
		assert_eq!(out[1].paramid, SoundmgrID::HitHumanUp);
		assert_eq!(out[2].paramid, SoundmgrID::HitHumanLeg);
		assert_eq!(out[3].paramid, SoundmgrID::HitHumanZombie);
	}

	#[test]
	fn the_weapon_table_answers_what_the_manager_expects() {
		// The two rules the distance switch reads, so a table change that broke them
		// fails here rather than silently changing how far a shot carries.
		assert!(PARAM.get_weapon(WEAPON_MP5).unwrap().soundvolume > 0);
		assert!(!PARAM.get_weapon(WEAPON_MP5).unwrap().silencer);
		assert!(PARAM.get_weapon(WEAPON_MP5SD).unwrap().silencer);
		assert_eq!(PARAM.get_weapon(WEAPON_NONE).unwrap().soundvolume, 0);
	}
}
