//! `ai.cpp` / `ai.h`: the AI that drives the humans the object manager owns.
//!
//! One [`AIcontrol`] speaks for one human slot, and the game screen holds one per slot
//! indexed by the human's data number, which is what the upstream's file scope
//! `AIcontrol HumanAI[MAX_HUMAN]` amounts to. The class stores only that number: every
//! collaborator upstream kept as a pointer (the object manager, the block data, the point
//! data, the settings, the collision, the sound) is a parameter of the method that uses
//! it, the same shape [`ObjectManager`] itself uses. That is also what keeps the borrow
//! checker out of the way: the AI reads a value out of a human into a local, lets the
//! borrow end, and then asks the manager to move or fire, which is the precedent
//! `objectmanager.rs` set for the same problem.
//!
//! **The random draws are behaviour.** [`get_rand`] is the shared Microsoft C runtime
//! sequence, so a draw that is added, dropped or reordered shifts every later value in
//! the frame and the game plays differently. Every draw is kept where the upstream has
//! it, including the ones that only happen for some inputs because upstream put them
//! inside a `&&`, an `||` or a branch. Those are called out in comments.

use std::f32::consts::PI;

use crate::collision::{check_target_angle, Collision};
use crate::config::Config;
use crate::core::{
	angle_normalization, degree_to_radian, del_flag, get_flag, set_flag, GAMEFPS, TOTAL_HAVEWEAPON,
};
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::{
	BlockDataInterface, PointDataInterface, Pointdata, POINT_P1TYPE_AIPATH,
	POINT_P1TYPE_RAND_AIPATH,
};
use crate::object::{HUMAN_HEIGHT, HUMAN_MAPCOLLISION_HEIGHT, HUMAN_MAPCOLLISION_R};
use crate::objectmanager::{ObjectManager, MAX_HUMAN, VIEW_HEIGHT};
use crate::parameter::{
	AIParameter, HumanParameter, ParameterInfo, ScopeParameter, ID_WEAPON_CASE,
	ID_WEAPON_GRENADE, ID_WEAPON_NONE, SCOPE_PARAMETER_0, TOTAL_PARAMETERINFO_AILEVEL,
};
use crate::resource::ResourceManager;
use crate::soundmanager::{SoundManager, Soundlist, MAX_SOUNDMGR_LIST};
use crate::window::get_rand;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// `AI_TOTALHUMAN_SCALE`: multiplier for the maximum number per 24 people.
///
/// The integer division is upstream's, and the header warns `MAX_HUMAN` has to stay a
/// multiple of 24 for it to mean anything.
pub const AI_TOTALHUMAN_SCALE: i32 = MAX_HUMAN as i32 / 24;

/// `AI_ADDTURNRAD`: turning (rotation) capability of the AI.
pub const AI_ADDTURNRAD: f32 = degree_to_radian(0.8);
/// `AI_SEARCH_RX`: horizontal axis angle for detecting enemies.
pub const AI_SEARCH_RX: f32 = degree_to_radian(110.0);
/// `AI_SEARCH_RY`: vertical axis angle for detecting enemies.
pub const AI_SEARCH_RY: f32 = degree_to_radian(60.0);
/// `AI_ZOMBIEATTACK_ARMRY`: arm angle when a zombie attacks.
pub const AI_ZOMBIEATTACK_ARMRY: f32 = degree_to_radian(-15.0);

/// `AI_ARRIVALDIST_PATH`: distance at which arrival at the path is judged.
pub const AI_ARRIVALDIST_PATH: f32 = 5.0;
/// `AI_ARRIVALDIST_TRACKING`: distance at which arrival at the tracking target is judged.
pub const AI_ARRIVALDIST_TRACKING: f32 = 18.0;
/// `AI_ARRIVALDIST_WALKTRACKING`: distance to approach the tracking target by walking.
pub const AI_ARRIVALDIST_WALKTRACKING: f32 = 24.0;
/// `AI_CHECKJUMP_HEIGHT`: height for judging whether there is no block ahead.
pub const AI_CHECKJUMP_HEIGHT: f32 = 0.3;
/// `AI_CHECKJUMP_DIST`: distance for judging whether there is no block ahead.
pub const AI_CHECKJUMP_DIST: f32 = 2.0;
/// `AI_CHECKJUMP2_DIST`: distance for judging whether there is no block ahead, with
/// priority running. Nothing in `ai.cpp` reads it, and the header keeps it.
pub const AI_CHECKJUMP2_DIST: f32 = 10.0;
/// `AI_CHECKBULLET`: distance for judging that a bullet passed nearby. Unread in
/// `ai.cpp`, kept from the header.
pub const AI_CHECKBULLET: f32 = 20.0;
/// `AI_CHECKSHORTATTACK_DIST`: distance for judging a short-range attack.
pub const AI_CHECKSHORTATTACK_DIST: f32 = 200.0;

/// The battle mode constants, upstream `AImode`.
///
/// The discriminants are what the code compares and what the debug overlay prints, so
/// they are exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMode {
	/// `AI_DEAD`: a person who is dead.
	AiDead = 0,
	/// `AI_ACTION`: a person in combat.
	AiAction = 1,
	/// `AI_CAUTION`: a person on caution.
	AiCaution = 2,
	/// `AI_NORMAL`: normal mode.
	AiNormal = 3,
	/// `AI_WALK`: a person walking along a movement path.
	AiWalk = 4,
	/// `AI_RUN`: a person running along a movement path.
	AiRun = 5,
	/// `AI_WAIT`: a person waiting according to a path.
	AiWait = 6,
	/// `AI_STOP_5SEC`: a person waiting for a time according to a path (5 seconds).
	AiStop5Sec = 7,
	/// `AI_TRACKING`: tracking a specific person.
	AiTracking = 8,
	/// `AI_GRENADE`: throwing a grenade.
	AiGrenade = 9,
	/// `AI_RUN2`: priority running.
	AiRun2 = 10,
	/// `AI_RANDOM`: processing a random path.
	AiRandom = 11,
	/// `AI_NULL`: no path.
	AiNull = 12,
}

/// The movement mode constants, upstream `AIMoveNaviFlag`.
///
/// The first five are what the navigation reports as a movement mode and the last five as
/// a point mode, which is why they live in one enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiMoveNaviFlag {
	/// `AI_NAVI_MOVE_NULL`.
	AiNaviMoveNull = 0,
	/// `AI_NAVI_MOVE_WALK`.
	AiNaviMoveWalk = 1,
	/// `AI_NAVI_MOVE_RUN`.
	AiNaviMoveRun = 2,
	/// `AI_NAVI_MOVE_RUN2`.
	AiNaviMoveRun2 = 3,
	/// `AI_NAVI_MOVE_TRACKING`.
	AiNaviMoveTracking = 4,
	/// `AI_NAVI_POINT_NULL`.
	AiNaviPointNull = 5,
	/// `AI_NAVI_POINT_WAIT`.
	AiNaviPointWait = 6,
	/// `AI_NAVI_POINT_STOP_5SEC`.
	AiNaviPointStop5Sec = 7,
	/// `AI_NAVI_POINT_TRACKING`.
	AiNaviPointTracking = 8,
	/// `AI_NAVI_POINT_GRENADE`.
	AiNaviPointGrenade = 9,
}

/// `AI_CTRL_MOVEFORWARD`.
pub const AI_CTRL_MOVEFORWARD: u32 = 0x0001;
/// `AI_CTRL_MOVEBACKWARD`.
pub const AI_CTRL_MOVEBACKWARD: u32 = 0x0002;
/// `AI_CTRL_MOVELEFT`.
pub const AI_CTRL_MOVELEFT: u32 = 0x0004;
/// `AI_CTRL_MOVERIGHT`.
pub const AI_CTRL_MOVERIGHT: u32 = 0x0008;
/// `AI_CTRL_MOVEWALK`.
pub const AI_CTRL_MOVEWALK: u32 = 0x0010;
/// `AI_CTRL_TURNUP`.
pub const AI_CTRL_TURNUP: u32 = 0x0100;
/// `AI_CTRL_TURNDOWN`.
pub const AI_CTRL_TURNDOWN: u32 = 0x0200;
/// `AI_CTRL_TURNLEFT`.
pub const AI_CTRL_TURNLEFT: u32 = 0x0400;
/// `AI_CTRL_TURNRIGHT`.
pub const AI_CTRL_TURNRIGHT: u32 = 0x0800;

/// The four movement flags, which `MoveRandom` tests in one go.
const AI_CTRL_MOVE_ANY: u32 =
	AI_CTRL_MOVEFORWARD | AI_CTRL_MOVEBACKWARD | AI_CTRL_MOVELEFT | AI_CTRL_MOVERIGHT;

/// The level settings an instance holds before `Init` has read them.
///
/// Upstream `LevelParam` starts as a null pointer and every read of it would fault, so
/// the zero record stands in: a search count of zero finds nothing rather than crashing.
const NO_LEVEL_PARAM: AIParameter = AIParameter {
	aiming: 0,
	attack: 0,
	search: 0,
	limitserror: 0,
};

// ---------------------------------------------------------------------------
// AIcontrol
// ---------------------------------------------------------------------------

/// The AI of one human, upstream `AIcontrol`.
pub struct AIcontrol {
	/// The settings tables. Upstream a pointer set once, and a zero sized handle here.
	param: ParameterInfo,
	/// The AI performance values of the current level, upstream `LevelParam`.
	level_param: AIParameter,
	/// Person number of the AI itself (the controlled target).
	ctrlid: i32,
	/// AI level.
	ai_level: i32,
	/// Battle mode.
	battlemode: AiMode,
	/// Non-combat flag.
	no_fight: bool,
	/// X coordinate.
	posx: f32,
	/// Y coordinate.
	posy: f32,
	/// Z coordinate.
	posz: f32,
	/// X axis rotation angle.
	rx: f32,
	/// Y axis rotation angle.
	ry: f32,
	/// X coordinate to return to after caution.
	cautionback_posx: f32,
	/// Z coordinate to return to after caution.
	cautionback_posz: f32,
	/// Flag to face a specific report during caution.
	face_caution_flag: bool,
	/// Direction to face during caution.
	face_caution_rx: f32,
	/// Total movement amount.
	total_move: f32,
	/// Wait time count.
	waitcnt: i32,
	/// Caution count.
	cautioncnt: i32,
	/// Attacking count.
	actioncnt: i32,
	/// Short-range / long-range flag.
	longattack: bool,
	/// Flag for throwing a grenade via an event.
	event_weapon_shot: bool,
	/// The attack target, which is the upstream's `enemyhuman` and its null.
	enemy_human_id: Option<i32>,
	/// The destination logic.
	move_navi: AIMoveNavi,
	/// The movement and rotation control.
	obj_driver: AIObjectDriver,
}

impl AIcontrol {
	/// Constructor. `human_id` is the person number the instance speaks for.
	pub fn new(human_id: i32) -> AIcontrol {
		AIcontrol {
			param: ParameterInfo::new(),
			level_param: NO_LEVEL_PARAM,
			ctrlid: human_id,
			ai_level: 0,
			battlemode: AiMode::AiNormal,
			no_fight: false,
			posx: 0.0,
			posy: 0.0,
			posz: 0.0,
			rx: 0.0,
			ry: 0.0,
			cautionback_posx: 0.0,
			cautionback_posz: 0.0,
			face_caution_flag: false,
			face_caution_rx: 0.0,
			total_move: 0.0,
			waitcnt: 0,
			cautioncnt: 0,
			actioncnt: 0,
			longattack: false,
			event_weapon_shot: false,
			enemy_human_id: None,
			move_navi: AIMoveNavi::new(human_id),
			obj_driver: AIObjectDriver::new(human_id),
		}
	}

	/// `SetClass`, reduced to the one thing the port still stores.
	///
	/// Upstream also re-points the object manager, the block data, the point data, the
	/// settings, the collision and the sound, and passes them on to the two sub-objects.
	/// All of those are method parameters here, so the person number is what is left to
	/// set, and it is what the game screen hands over when it builds its array.
	///
	/// The movement state is not reset: upstream only stores the new classes here, and
	/// [`Self::init`] is what starts a path.
	pub fn set_class(&mut self, human_id: i32) {
		self.ctrlid = human_id;
		self.move_navi.set_class(human_id);
		self.obj_driver.set_class(human_id);
	}

	/// `Init`: starts the instance on the human's own path.
	pub fn init(&mut self, obj_mgr: &ObjectManager, points: &PointDataInterface) {
		let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) else {
			return;
		};
		// Do nothing if the person is not in use.
		if !ctrlhuman.enable_flag() {
			return;
		}

		self.no_fight = false;
		self.battlemode = AiMode::AiNormal;
		self.enemy_human_id = None;
		self.face_caution_flag = false;
		self.face_caution_rx = 0.0;
		self.total_move = 0.0;
		self.waitcnt = 0;
		self.cautioncnt = 0;
		self.actioncnt = 0;
		self.longattack = false;
		self.event_weapon_shot = false;

		self.move_navi.init(obj_mgr, points);
		self.obj_driver.init();

		// Get the AI level and its setting values. A human whose type number is not in the
		// table keeps the level it had, where upstream would read the record the failed
		// lookup left on the stack.
		let (paramid, _, _, _) = ctrlhuman.get_param_data();
		if let Some(paramdata) = self.param.get_human(paramid) {
			self.ai_level = paramdata.ai_level;
		}
		self.set_level_param();

		// Search for the next point.
		self.move_navi.move_path_now_state(obj_mgr, points);
	}

	/// `SetAIlevel`: sets the AI level, and -1 asks for the human's own setting.
	pub fn set_ai_level(&mut self, obj_mgr: &ObjectManager, level: i32) {
		if level < -1 || TOTAL_PARAMETERINFO_AILEVEL as i32 <= level {
			return;
		}

		if level == -1 {
			if let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) {
				let (paramid, _, _, _) = ctrlhuman.get_param_data();
				if let Some(paramdata) = self.param.get_human(paramid) {
					self.ai_level = paramdata.ai_level;
				}
			}
		} else {
			self.ai_level = level;
		}

		self.set_level_param();
	}

	/// The performance values of the current level, upstream `Param->GetAIlevel(...)`.
	fn set_level_param(&mut self) {
		if let Some(param) = self.param.get_ai_level(self.ai_level) {
			self.level_param = param;
		}
	}

	/// `GetAIlevel`.
	pub fn get_ai_level(&self) -> i32 {
		self.ai_level
	}

	/// `SetHoldWait`: waits at a location whatever the path says.
	pub fn set_hold_wait(&mut self, px: f32, pz: f32, rx: f32) {
		self.move_navi.set_hold_wait(px, pz, rx);
	}

	/// `SetHoldTracking`: tracks a person whatever the path says.
	pub fn set_hold_tracking(&mut self, id: i32) {
		if (0..MAX_HUMAN as i32).contains(&id) {
			self.move_navi.set_hold_tracking(id);
		}
	}

	/// `SetCautionMode`: forces caution, and does nothing while priority running.
	pub fn set_caution_mode(&mut self) {
		if self.move_navi.get_run2() {
			return;
		}

		if self.battlemode == AiMode::AiNormal {
			self.cautionback_posx = self.posx;
			self.cautionback_posz = self.posz;
		}
		self.battlemode = AiMode::AiCaution;
		self.cautioncnt = 160;
		self.face_caution_flag = false;
		self.face_caution_rx = 0.0;
	}

	/// `SetNoFightFlag`: a non-combatant stops recognising enemies.
	pub fn set_no_fight_flag(&mut self, flag: bool) {
		self.no_fight = flag;
	}

	/// `GetBattleMode`: the mode number and the name the debug overlay prints.
	///
	/// Upstream copies the name into a caller buffer, and only the four battle modes have
	/// one: any other value answers with the empty string.
	pub fn get_battle_mode(&self) -> (i32, &'static [u8]) {
		let name: &'static [u8] = match self.battlemode {
			AiMode::AiDead => b"AI_DEAD",
			AiMode::AiAction => b"AI_ACTION",
			AiMode::AiCaution => b"AI_CAUTION",
			AiMode::AiNormal => b"AI_NORMAL",
			_ => b"",
		};
		(self.battlemode as i32, name)
	}

	/// `GetEnemyHumanID`: the person number of the attack target, -1 for none.
	pub fn get_enemy_human_id(&self) -> i32 {
		self.enemy_human_id.unwrap_or(-1)
	}

	/// `GetMoveTargetPos`: where the AI is heading, which during an attack or caution is
	/// the place it will return to.
	pub fn get_move_target_pos(&self) -> (f32, f32, AiMoveNaviFlag) {
		let target = self.move_navi.get_target_pos();

		if !self.move_navi.get_run2()
			&& (self.battlemode == AiMode::AiAction || self.battlemode == AiMode::AiCaution)
		{
			return (self.cautionback_posx, self.cautionback_posz, target.movemode);
		}

		(target.posx, target.posz, target.movemode)
	}

	/// `GetPathPointData`: the point data the AI is currently standing on.
	pub fn get_path_point_data(&self, points: &PointDataInterface) -> Option<Pointdata> {
		self.move_navi.get_path_point_data(points)
	}

	/// `Process`: one frame of the AI.
	///
	/// `d3dg` and `resource` are only reached when a weapon type has to be switched, and
	/// `config` only when a zombie's claw lands, which is what upstream gets from its
	/// globals.
	#[allow(clippy::too_many_arguments)]
	pub fn process(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
		sound: &mut SoundManager,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
		config: &Config,
	) {
		let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) else {
			return;
		};
		// Do nothing if the person is not in use.
		if !ctrlhuman.enable_flag() {
			return;
		}
		// If dead.
		if ctrlhuman.hp() <= 0 {
			self.battlemode = AiMode::AiDead;
			return;
		}

		// Marked as dead even though the HP is not 0 ... revived?
		if self.battlemode == AiMode::AiDead {
			self.battlemode = AiMode::AiNormal;
			self.move_navi.move_path_now_state(obj_mgr, points);
		}

		self.update_pos(obj_mgr);

		// Randomly stop actions.
		self.cancel_move_turn();

		// If attacking or on caution.
		if self.battlemode == AiMode::AiAction || self.battlemode == AiMode::AiCaution {
			self.have_weapon(obj_mgr);
		}

		// Execute the main calculation.
		if self.battlemode == AiMode::AiAction {
			self.action_main(obj_mgr, blocks, points, coll_d, sound, resource, config);
		} else if self.battlemode == AiMode::AiCaution {
			self.caution_main(obj_mgr, blocks, points, coll_d, sound);
		} else {
			self.normal_main(obj_mgr, blocks, points, coll_d, sound);
		}

		// Movement and turning processing.
		self.obj_driver.control_object(obj_mgr);

		// Handle the weapon.
		self.control_weapon(obj_mgr, sound, d3dg, resource);
	}

	/// The coordinates and angles of the person it controls, which upstream reads once per
	/// `Process` and every decision then uses.
	fn update_pos(&mut self, obj_mgr: &ObjectManager) {
		let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) else {
			return;
		};
		let (posx, posy, posz, _) = ctrlhuman.pos_data();
		self.posx = posx;
		self.posy = posy;
		self.posz = posz;
		let (rx, ry) = ctrlhuman.rx_ry();
		self.rx = rx;
		self.ry = ry;
	}

	/// `CheckTargetPos`: whether the AI has arrived at its target point.
	fn check_target_pos(&self, back: bool) -> bool {
		let (target_posx, target_posz, movemode) = if !back {
			let target = self.move_navi.get_target_pos();
			(target.posx, target.posz, target.movemode)
		} else {
			(
				self.cautionback_posx,
				self.cautionback_posz,
				AiMoveNaviFlag::AiNaviMoveRun,
			)
		};

		let x = self.posx - target_posx;
		let z = self.posz - target_posz;
		let r = x * x + z * z;

		if movemode == AiMoveNaviFlag::AiNaviMoveTracking {
			r < AI_ARRIVALDIST_TRACKING * AI_ARRIVALDIST_TRACKING
		} else {
			r < AI_ARRIVALDIST_PATH * AI_ARRIVALDIST_PATH
		}
	}

	/// `MoveTarget`: walks toward the target point.
	fn move_target(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
		back: bool,
	) {
		let (target_posx, target_posz, movemode) = if !back {
			let target = self.move_navi.get_target_pos();
			(target.posx, target.posz, target.movemode)
		} else {
			(
				self.cautionback_posx,
				self.cautionback_posz,
				AiMoveNaviFlag::AiNaviMoveRun,
			)
		};

		let zombie = self.is_zombie(obj_mgr, self.ctrlid);

		// Stop all movement once.
		self.obj_driver.reset_mode();

		// The angle toward the target point, which only needs the flat coordinates.
		let Some((atan, _, r)) = check_target_angle(
			self.posx,
			0.0,
			self.posz,
			self.rx * -1.0 + PI / 2.0,
			0.0,
			target_posx,
			0.0,
			target_posz,
			0.0,
		) else {
			return;
		};

		// Turn.
		if atan > degree_to_radian(0.5) {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
		}
		if atan < degree_to_radian(-0.5) {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
		}

		// Move forward.
		if zombie {
			if atan.abs() < degree_to_radian(20.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_MOVEWALK);
			}
		} else if movemode == AiMoveNaviFlag::AiNaviMoveRun
			|| movemode == AiMoveNaviFlag::AiNaviMoveRun2
		{
			if atan.abs() < degree_to_radian(50.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
			}
		} else if movemode == AiMoveNaviFlag::AiNaviMoveWalk {
			if atan.abs() < degree_to_radian(6.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_MOVEWALK);
			}
		} else if movemode == AiMoveNaviFlag::AiNaviMoveTracking {
			if atan.abs() < degree_to_radian(20.0) {
				if r < AI_ARRIVALDIST_WALKTRACKING * AI_ARRIVALDIST_WALKTRACKING {
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEWALK);
				} else {
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
				}
			}
		}

		// Jump. A 1 in 16 draw every frame, and `MoveJump` draws nothing of its own.
		if get_rand(16) == 0 {
			self.move_jump(obj_mgr, blocks, coll_d);
		}

		// If stuck, randomly rotate left and right. The third draw only happens when the
		// person is moving but has not moved this frame.
		if get_rand(28) == 0 {
			let movemode_now = obj_mgr
				.get_human_object(self.ctrlid)
				.map_or(0, |human| human.get_movemode(true));
			if movemode_now != 0 {
				let total = obj_mgr
					.get_human_object(self.ctrlid)
					.map_or(0.0, |human| human.total_move());
				if total - self.total_move < 0.1 {
					if get_rand(2) == 0 {
						self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
					} else {
						self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
					}
				}
			}
		}
		self.total_move = obj_mgr
			.get_human_object(self.ctrlid)
			.map_or(0.0, |human| human.total_move());
	}

	/// `MoveTarget2`: walks toward the target point while priority running, which needs no
	/// turning because it may move sideways.
	fn move_target2(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
		back: bool,
	) {
		let (target_posx, target_posz) = if !back {
			let target = self.move_navi.get_target_pos();
			(target.posx, target.posz)
		} else {
			(self.cautionback_posx, self.cautionback_posz)
		};

		let Some((atan, _, _)) = check_target_angle(
			self.posx,
			0.0,
			self.posz,
			self.rx * -1.0 + PI / 2.0,
			0.0,
			target_posx,
			0.0,
			target_posz,
			0.0,
		) else {
			return;
		};

		// Handle forward and backward movement.
		if atan.abs() < degree_to_radian(56.0) {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
		}
		if atan.abs() > degree_to_radian(123.5) {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
		}

		// Handle left and right movement.
		if degree_to_radian(-146.0) < atan && atan < degree_to_radian(-33.0) {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVERIGHT);
		}
		if degree_to_radian(33.0) < atan && atan < degree_to_radian(146.0) {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVELEFT);
		}

		// Jump.
		if get_rand(16) == 0 {
			self.move_jump(obj_mgr, blocks, coll_d);
		}

		// If stuck, randomly rotate left and right.
		if get_rand(28) == 0 {
			let movemode_now = obj_mgr
				.get_human_object(self.ctrlid)
				.map_or(0, |human| human.get_movemode(true));
			if movemode_now != 0 {
				let total = obj_mgr
					.get_human_object(self.ctrlid)
					.map_or(0.0, |human| human.total_move());
				if total - self.total_move < 0.1 {
					if get_rand(2) == 0 {
						self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
					} else {
						self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
					}
				}
			}
		}
		self.total_move = obj_mgr
			.get_human_object(self.ctrlid)
			.map_or(0.0, |human| human.total_move());
	}

	/// `MoveRandom`: moves randomly forward, backward, left and right during an attack.
	fn move_random(
		&mut self,
		obj_mgr: &ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
	) {
		let (forwardstart, backstart, sidestart) = if !self.longattack {
			(80, 90, 70)
		} else {
			(120, 150, 130)
		};

		// Start moving randomly: four draws, in this order, every frame.
		if get_rand(forwardstart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
		}
		if get_rand(backstart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
		}
		if get_rand(sidestart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVELEFT);
		}
		if get_rand(sidestart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVERIGHT);
		}

		// If the AI owns no weapon and is empty-handed.
		if self.main_weapon_type_no(obj_mgr, self.ctrlid) == ID_WEAPON_NONE {
			// Start backing up with a 1/80 probability.
			if get_rand(80) == 0 {
				self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
			}
		}

		// With a 1/3 probability, or if a movement flag is set. The draw is always taken,
		// and the flag test only decides whether the draw mattered.
		if get_rand(3) == 0 || self.obj_driver.get_mode_flag(AI_CTRL_MOVE_ANY) {
			if get_rand(2) == 0 {
				// Calculate the forward direction vector.
				let angle = self.rx * -1.0 + PI / 2.0;
				if blocked_ahead(
					blocks,
					coll_d,
					self.posx,
					self.posy,
					self.posz,
					angle.cos(),
					angle.sin(),
				) {
					// Clear the forward flag and set the backward flag.
					self.obj_driver.del_mode_flag(AI_CTRL_MOVEFORWARD);
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
				}

				// Calculate the backward direction vector.
				let angle = self.rx * -1.0 + PI / 2.0 + PI;
				if blocked_ahead(
					blocks,
					coll_d,
					self.posx,
					self.posy,
					self.posz,
					angle.cos(),
					angle.sin(),
				) {
					// Clear the backward flag and set the forward flag.
					self.obj_driver.del_mode_flag(AI_CTRL_MOVEBACKWARD);
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
				}
			} else {
				// The right direction vector.
				let angle = self.rx * -1.0;
				if blocked_ahead(
					blocks,
					coll_d,
					self.posx,
					self.posy,
					self.posz,
					angle.cos(),
					angle.sin(),
				) {
					// Clear the right movement flag and set the left movement flag.
					self.obj_driver.del_mode_flag(AI_CTRL_MOVERIGHT);
					self.obj_driver.set_mode_flag(AI_CTRL_MOVELEFT);
				}

				let angle = self.rx * -1.0 + PI;
				if blocked_ahead(
					blocks,
					coll_d,
					self.posx,
					self.posy,
					self.posz,
					angle.cos(),
					angle.sin(),
				) {
					// Clear the left movement flag and set the right movement flag.
					self.obj_driver.del_mode_flag(AI_CTRL_MOVELEFT);
					self.obj_driver.set_mode_flag(AI_CTRL_MOVERIGHT);
				}
			}
		}

		// If there is an attack target.
		if let Some(enemy_id) = self.enemy_human_id {
			let (tx, ty, tz) = match obj_mgr.get_human_object(enemy_id) {
				Some(human) => {
					let (x, y, z, _) = human.pos_data();
					(x, y, z)
				}
				None => (0.0, 0.0, 0.0),
			};

			let x = self.posx - tx;
			let y = self.posy - ty;
			let z = self.posz - tz;
			let r = x * x + y * y + z * z;

			// If too close to the enemy, back away.
			if r < 20.0 * 20.0 {
				self.obj_driver.del_mode_flag(AI_CTRL_MOVEFORWARD);
				if get_rand(70) == 0 {
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
				}
			}
		}
	}

	/// `TurnSeen`: looks around in place.
	fn turn_seen(&mut self) {
		let target = self.move_navi.get_target_pos();

		// If alerted by being shot, face the direction of the shot.
		if self.battlemode == AiMode::AiCaution && self.face_caution_flag {
			let tr = angle_normalization(self.face_caution_rx - self.rx);

			// Turn.
			if tr > degree_to_radian(2.5) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
			}
			if tr < degree_to_radian(-2.5) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
			}

			// Once facing the specific direction, switch to normal caution.
			if tr.abs() <= degree_to_radian(2.5) {
				self.face_caution_flag = false;
			}

			return;
		}

		// Set the start and stop probabilities for rotation.
		if self.battlemode == AiMode::AiAction {
			return;
		}
		let (turnstart, turnstop) = if self.battlemode == AiMode::AiCaution {
			(20, 20)
		} else {
			let start = if target.pointmode == AiMoveNaviFlag::AiNaviPointTracking {
				65
			} else {
				85
			};
			(start, 18)
		};

		// Start rotating randomly: two draws.
		if get_rand(turnstart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
		}
		if get_rand(turnstart) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
		}

		if self.battlemode == AiMode::AiNormal && target.pointmode == AiMoveNaviFlag::AiNaviPointWait
		{
			// Randomly try to face toward the point, which reproduces "slightly prefer the
			// direction of the point". A 1 in 80 draw, only while waiting on a path.
			if get_rand(80) == 0 {
				let tr = angle_normalization(target.rx - self.rx);

				if tr > 0.0 {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
				}
				if tr < 0.0 {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
				}
			}
		}

		// Stop rotating randomly: two more draws.
		if get_rand(turnstop) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNRIGHT);
		}
		if get_rand(turnstop) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNLEFT);
		}
	}

	/// `StopSeen`: keeps looking in the direction the path point asks for.
	///
	/// Returns true once it faces that direction.
	fn stop_seen(&mut self) -> bool {
		let target = self.move_navi.get_target_pos();

		let tr = angle_normalization(target.rx - self.rx);
		let mut returnflag = false;

		self.obj_driver.del_mode_flag(AI_CTRL_TURNRIGHT);
		self.obj_driver.del_mode_flag(AI_CTRL_TURNLEFT);

		// Turn.
		if tr > degree_to_radian(2.5) {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
			returnflag = false;
		}
		if tr < degree_to_radian(-2.5) {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
			returnflag = false;
		}

		// Check whether it faces the specific direction.
		if tr.abs() <= degree_to_radian(2.5) {
			returnflag = true;
		}

		returnflag
	}

	/// `MoveJump`: jumps when a block is in the way of travel.
	fn move_jump(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
	) -> bool {
		// Do nothing if standing still.
		let movemode = obj_mgr
			.get_human_object(self.ctrlid)
			.map_or(0, |human| human.get_movemode(false));
		if movemode == 0 {
			return false;
		}

		// Calculate the direction of travel. The flags are tested in this order and each
		// overwrites the last, so right beats left beats backward beats forward.
		let mut check_rx = 0.0;
		if self.obj_driver.get_mode_flag(AI_CTRL_MOVEWALK) {
			check_rx = degree_to_radian(0.0);
		}
		if self.obj_driver.get_mode_flag(AI_CTRL_MOVEFORWARD) {
			check_rx = degree_to_radian(0.0);
		}
		if self.obj_driver.get_mode_flag(AI_CTRL_MOVEBACKWARD) {
			check_rx = degree_to_radian(180.0);
		}
		if self.obj_driver.get_mode_flag(AI_CTRL_MOVELEFT) {
			check_rx = degree_to_radian(90.0);
		}
		if self.obj_driver.get_mode_flag(AI_CTRL_MOVERIGHT) {
			check_rx = degree_to_radian(-90.0);
		}

		// Collision check around the waist.
		let angle = self.rx * -1.0 + PI / 2.0 + check_rx;
		let new_posx = self.posx + angle.cos() * (AI_CHECKJUMP_DIST + HUMAN_MAPCOLLISION_R);
		let new_posy = self.posy + HUMAN_MAPCOLLISION_HEIGHT;
		let new_posz = self.posz + angle.sin() * (AI_CHECKJUMP_DIST + HUMAN_MAPCOLLISION_R);
		if coll_d.check_all_block_inside(blocks, new_posx, new_posy, new_posz) {
			obj_mgr.move_jump(self.ctrlid);
			return true;
		}

		// Collision check for the whole body.
		let new_posx = self.posx + angle.cos() * AI_CHECKJUMP_DIST;
		let new_posy = self.posy + AI_CHECKJUMP_HEIGHT;
		let new_posz = self.posz + angle.sin() * AI_CHECKJUMP_DIST;
		if coll_d.check_all_block_inside(blocks, new_posx, new_posy, new_posz) {
			obj_mgr.move_jump(self.ctrlid);
			return true;
		}
		if coll_d
			.check_all_block_intersect_ray(
				blocks,
				new_posx,
				new_posy,
				new_posz,
				0.0,
				1.0,
				0.0,
				HUMAN_HEIGHT - AI_CHECKJUMP_HEIGHT,
			)
			.is_some()
		{
			obj_mgr.move_jump(self.ctrlid);
			return true;
		}

		false
	}

	/// `Action`: the attack itself, once an enemy is in hand.
	#[allow(clippy::too_many_arguments)]
	fn action(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
		sound: &mut SoundManager,
		resource: &ResourceManager,
		config: &Config,
	) {
		// Upstream dereferences `enemyhuman` here without a null check, because the battle
		// mode is only AI_ACTION with a target found. A missing target ends the attack.
		let Some(enemy_id) = self.enemy_human_id else {
			return;
		};

		// Get the coordinates.
		let Some(enemy) = obj_mgr.get_human_object(enemy_id) else {
			return;
		};
		let (enemy_x, enemy_y, enemy_z, _) = enemy.pos_data();
		let (mut tx, mut ty, mut tz) = (enemy_x, enemy_y + VIEW_HEIGHT, enemy_z);
		let posy2 = self.posy + VIEW_HEIGHT;

		let zombie = self.is_zombie(obj_mgr, self.ctrlid);

		// Get the type of the owned weapon.
		let weaponid = self.main_weapon_type_no(obj_mgr, self.ctrlid);

		// Correct the position of the target.
		if zombie {
			// If a zombie, lead the target by its own movement, scaled by how well this
			// kind of zombie tracks. The flat distance is capped at 180, and the comment
			// upstream says the cap may have been meant to be 1800.
			let x = self.posx - tx;
			let z = self.posz - tz;
			let mut r = (x * x + z * z).sqrt();
			if r > 180.0 {
				r = 180.0;
			}

			let scale = match self
				.human_param(obj_mgr, self.ctrlid)
				.map_or(-1, |param| param.ai_trackability)
			{
				0 => 0.05 * 2.0,
				1 => 0.10 * 2.0,
				2 => 0.13 * 2.0,
				3 => 0.17 * 2.0,
				_ => 0.0,
			};

			let (mx, _, mz) = obj_mgr
				.get_human_object(enemy_id)
				.map_or((0.0, 0.0, 0.0), |human| human.move_pos());
			tx += mx * r * scale;
			tz += mz * r * scale;
		} else if weaponid == ID_WEAPON_GRENADE {
			// If the AI itself has a grenade, the throw is lobbed: the aim point rises
			// with the distance, and further when it is a long attack.
			let x = self.posx - tx;
			let z = self.posz - tz;
			let r = (x * x + z * z).sqrt();
			let scale = if !self.longattack { 0.12 } else { 0.4 };

			ty += (r - AI_CHECKSHORTATTACK_DIST) * scale;
		} else {
			let (mx, _, mz) = obj_mgr
				.get_human_object(enemy_id)
				.map_or((0.0, 0.0, 0.0), |human| human.move_pos());

			// Predict the enemy's movement. A short attack leads by a fixed amount, a long
			// one by the distance to the target.
			let scale = if !self.longattack {
				1.5
			} else {
				let x = self.posx - tx;
				let z = self.posz - tz;
				(x * x + z * z).sqrt() * 0.12
			};

			tx += mx * scale;
			tz += mz * scale;
		}

		// The angle toward the target point.
		let Some((atanx, atany, r)) = check_target_angle(
			self.posx,
			posy2,
			self.posz,
			self.rx * -1.0 + PI / 2.0,
			self.ry,
			tx,
			ty,
			tz,
			0.0,
		) else {
			return;
		};

		// Decide the timing to change direction.
		let mut randr = self.level_param.aiming;
		if !self.longattack {
			randr += 1;
		} else {
			randr += 2;
		}

		if randr != 0 {
			// Turn.
			if atanx > 0.0 {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNRIGHT);
			}
			if atanx < 0.0 {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNLEFT);
			}

			// Arm angle.
			if zombie {
				// Turn.
				if self.ry < AI_ZOMBIEATTACK_ARMRY {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);
				}
				if self.ry > AI_ZOMBIEATTACK_ARMRY {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
				}
			} else if weaponid == ID_WEAPON_NONE {
				// If the AI itself is empty-handed.
				if self.main_weapon_type_no(obj_mgr, enemy_id) == ID_WEAPON_NONE {
					// If the enemy is also empty-handed, keep aiming down.
					self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
				} else {
					// If the enemy has a weapon, keep aiming up.
					self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);
				}
			} else {
				// Turn.
				if atany > 0.0 {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);
				}
				if atany < 0.0 {
					self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
					self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
				}
			}
		}

		// If not a zombie and empty-handed, back away with a fixed probability.
		if !zombie && weaponid == ID_WEAPON_NONE && get_rand(80) == 0 {
			self.obj_driver.set_mode_flag(AI_CTRL_MOVEBACKWARD);
		}

		if zombie {
			// A zombie attacks with its arm.
			let y = posy2 - ty;

			// If running, switch to walking once.
			if self.obj_driver.get_mode_flag(AI_CTRL_MOVEFORWARD) {
				self.obj_driver.del_mode_flag(AI_CTRL_MOVEFORWARD);
				self.obj_driver.set_mode_flag(AI_CTRL_MOVEWALK);
			}

			// Advance toward the enemy.
			if atanx.abs() <= degree_to_radian(25.0) {
				if atanx.abs() <= degree_to_radian(15.0)
					&& r < 24.0 * 24.0
					&& self.actioncnt % 50 > 20
				{
					// Cancel walking and run.
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEFORWARD);
					self.obj_driver.del_mode_flag(AI_CTRL_MOVEWALK);
				} else {
					self.obj_driver.set_mode_flag(AI_CTRL_MOVEWALK);
				}
			}

			if r < 9.0 * 9.0 && y.abs() < 10.0 {
				let x = self.posx - tx;
				let z = self.posz - tz;

				// Grab, which pulls the enemy in.
				if let Some(enemy) = obj_mgr.get_human_object_mut(enemy_id) {
					enemy.add_pos_order(z.atan2(x), 0.0, 0.5);
				}

				// Randomly move the enemy's viewpoint: two draws, one per axis, and both
				// are always taken.
				let (erx, ery) = obj_mgr
					.get_human_object(enemy_id)
					.map_or((0.0, 0.0), |human| human.rx_ry());
				let erx = erx
					+ match get_rand(3) {
						0 => -degree_to_radian(2.0),
						1 => degree_to_radian(2.0),
						_ => 0.0,
					};
				let ery = ery
					+ match get_rand(3) {
						0 => -degree_to_radian(2.0),
						1 => degree_to_radian(2.0),
						_ => 0.0,
					};
				if let Some(enemy) = obj_mgr.get_human_object_mut(enemy_id) {
					enemy.set_rx_ry(erx, ery);
				}
			}

			if self.actioncnt % 50 == 0 {
				if obj_mgr.check_zombie_attack(self.ctrlid as usize, enemy_id as usize) {
					obj_mgr.hit_zombie_attack(
						self.ctrlid as usize,
						enemy_id as usize,
						resource,
						sound,
						config,
					);
				}
			}
		} else {
			// Fire.
			let mut scopemode = 0;

			// Get the scope setting of the weapon.
			if weaponid != ID_WEAPON_NONE {
				scopemode = self
					.param
					.get_weapon(weaponid)
					.map_or(0, |weapon| weapon.scopemode);
			}

			// Calculate the tolerance for judging that the enemy is captured, adjusted by
			// the level's allowed error.
			let sparam = self.param.get_scope_param(scopemode).unwrap_or(SCOPE_PARAMETER_0);
			let shot_angle = self.shot_angle(&sparam, self.move_navi.get_run2());

			// If the enemy is captured, fire. The draw only happens when it is.
			if Self::captured(atanx, atany, shot_angle) {
				let mut rand = self.level_param.attack;
				if self.longattack {
					rand += 1;
				}

				if get_rand(rand) == 0 {
					obj_mgr.shot_weapon(self.ctrlid, sound);
				}
			}
		}

		// Switch between short-range and long-range according to distance. The set value
		// is kept exactly when the distance equals AI_CHECKSHORTATTACK_DIST.
		if r < AI_CHECKSHORTATTACK_DIST * AI_CHECKSHORTATTACK_DIST {
			self.longattack = false;
		}
		if r > AI_CHECKSHORTATTACK_DIST * AI_CHECKSHORTATTACK_DIST
			&& !self.move_navi.get_run2()
		{
			self.longattack = true;
		}

		if !zombie {
			// Move randomly.
			self.move_random(obj_mgr, blocks, coll_d);
		}

		self.actioncnt += 1;
	}

	/// The tolerance within which an enemy counts as captured, `ShotAngle` in `Action`.
	///
	/// The level's allowed error is added to the scope's own angle, with a different
	/// multiplier for a short and a long attack, and a priority runner's short attack is
	/// widened by half again.
	fn shot_angle(&self, sparam: &ScopeParameter, run2: bool) -> f32 {
		if !self.longattack {
			let mut angle =
				sparam.ai_shot_angle + degree_to_radian(0.5) * self.level_param.limitserror as f32;
			if run2 {
				angle *= 1.5;
			}
			angle
		} else {
			sparam.ai_shot_angle_long
				+ degree_to_radian(0.2) * self.level_param.limitserror as f32
		}
	}

	/// Whether the enemy is captured: the two angles added together against the tolerance,
	/// which is `atanxy < ShotAngle` in `Action`.
	fn captured(atanx: f32, atany: f32, shot_angle: f32) -> bool {
		atanx.abs() + atany.abs() < shot_angle
	}

	/// `ActionCancel`: whether the attack should end.
	fn action_cancel(
		&mut self,
		obj_mgr: &ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
	) -> bool {
		// End if the non-combat flag is enabled.
		if self.no_fight {
			return true;
		}

		// End when the enemy dies. Upstream reads the pointer without a check.
		let Some(enemy_id) = self.enemy_human_id else {
			return true;
		};
		let dead = obj_mgr
			.get_human_object(enemy_id)
			.map_or(true, |human| human.dead_flag());
		if dead {
			return true;
		}

		// Get the distance.
		let (tx, ty, tz) = match obj_mgr.get_human_object(enemy_id) {
			Some(human) => {
				let (x, y, z, _) = human.pos_data();
				(x, y, z)
			}
			None => (0.0, 0.0, 0.0),
		};
		let x = self.posx - tx;
		let y = self.posy - ty;
		let z = self.posz - tz;
		let r = x * x + y * y + z * z;

		// End if the distance has become too large.
		if r > 620.0 * 620.0 {
			return true;
		}

		if !self.longattack {
			// Occasionally check whether the enemy is visible: a 1 in 40 draw, and the
			// visibility check only runs when that draw lands.
			if get_rand(40) == 0
				&& self
					.check_look_enemy(
						obj_mgr,
						blocks,
						coll_d,
						enemy_id,
						AI_SEARCH_RX,
						AI_SEARCH_RY,
						620.0,
					)
					.is_none()
			{
				return true;
			}

			// Force termination.
			if get_rand(550) == 0 {
				return true;
			}
		} else {
			if get_rand(30) == 0
				&& self
					.check_look_enemy(
						obj_mgr,
						blocks,
						coll_d,
						enemy_id,
						AI_SEARCH_RX,
						AI_SEARCH_RY,
						620.0,
					)
					.is_none()
			{
				return true;
			}

			// Force termination.
			if get_rand(450) == 0 {
				return true;
			}
		}

		false
	}

	/// `HaveWeapon`: holds a weapon, which is switching to one that still has ammunition.
	fn have_weapon(&self, obj_mgr: &mut ObjectManager) -> i32 {
		let Some((selectweapon, weapon, _, nbslist)) =
			obj_mgr.get_human_object(self.ctrlid).map(|h| h.get_weapon())
		else {
			return 0;
		};
		let index = weapon[selectweapon as usize];

		// If the AI has no weapon but has ammunition, do not switch.
		if index.is_none() && nbslist[selectweapon as usize] > 0 {
			return 0;
		}

		// If the AI has a weapon, get the total ammunition count.
		let nbs = index
			.and_then(|i| obj_mgr.get_weapon_object(i as i32))
			.map_or(0, |weapon| weapon.param_data().2);

		// If the AI has no weapon or has 0 ammunition.
		if index.is_none() || nbs == 0 {
			// Specify the next weapon.
			let mut notselectweapon = selectweapon + 1;
			if notselectweapon == TOTAL_HAVEWEAPON as i32 {
				notselectweapon = 0;
			}

			// If the AI has the next weapon and its ammunition is not 0, switch.
			if let Some(other) = weapon[notselectweapon as usize] {
				let nbs = obj_mgr
					.get_weapon_object(other as i32)
					.map_or(0, |weapon| weapon.param_data().2);
				if nbs > 0 {
					obj_mgr.change_have_weapon(self.ctrlid, -1);
					return 1;
				}
			}
		}

		0
	}

	/// `CancelMoveTurn`: randomly stops movement and turning.
	fn cancel_move_turn(&mut self) {
		if self.battlemode == AiMode::AiAction {
			// During attack, while priority running, everything stops and only the walk
			// flag is dropped at a 1 in 3 chance.
			if self.move_navi.get_run2() {
				self.obj_driver.del_mode_flag(AI_CTRL_MOVEFORWARD);
				self.obj_driver.del_mode_flag(AI_CTRL_MOVEBACKWARD);
				self.obj_driver.del_mode_flag(AI_CTRL_MOVELEFT);
				self.obj_driver.del_mode_flag(AI_CTRL_MOVERIGHT);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNLEFT);
				self.obj_driver.del_mode_flag(AI_CTRL_TURNRIGHT);
				if get_rand(3) == 0 {
					self.obj_driver.del_mode_flag(AI_CTRL_MOVEWALK);
				}
				return;
			}

			if !self.longattack {
				self.stop_move_turn(6, 6, 7, 5, 6);
			} else {
				self.stop_move_turn(5, 4, 5, 3, 3);
			}
		} else if self.battlemode == AiMode::AiCaution {
			self.stop_move_turn(10, 10, 10, 14, 20);
		} else {
			self.stop_move_turn(12, 12, 12, 15, 18);
		}
	}

	/// The nine draws at the end of `CancelMoveTurn`, in the upstream order: forward, back,
	/// left, right, walk, up, down, left turn, right turn.
	fn stop_move_turn(&mut self, forward: i32, back: i32, side: i32, updown: i32, rightleft: i32) {
		// Stop movement randomly.
		if get_rand(forward) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_MOVEFORWARD);
		}
		if get_rand(back) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_MOVEBACKWARD);
		}
		if get_rand(side) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_MOVELEFT);
		}
		if get_rand(side) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_MOVERIGHT);
		}
		if get_rand(3) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_MOVEWALK);
		}

		// Stop rotating randomly.
		if get_rand(updown) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
		}
		if get_rand(updown) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);
		}
		if get_rand(rightleft) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNLEFT);
		}
		if get_rand(rightleft) == 0 {
			self.obj_driver.del_mode_flag(AI_CTRL_TURNRIGHT);
		}
	}

	/// `ControlWeapon`: reloads or discards a weapon.
	///
	/// Discard: 1, reload: 2, switch weapon: 3, FULL/SEMI switch: 4, do nothing: 0.
	fn control_weapon(
		&mut self,
		obj_mgr: &mut ObjectManager,
		sound: &mut SoundManager,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
	) -> i32 {
		let Some((selectweapon, weapon, _, _)) =
			obj_mgr.get_human_object(self.ctrlid).map(|h| h.get_weapon())
		else {
			return 0;
		};
		let Some(index) = weapon[selectweapon as usize] else {
			return 0;
		};
		let Some((weaponid, lnbs, nbs)) = obj_mgr
			.get_weapon_object(index as i32)
			.map(|weapon| weapon.param_data())
		else {
			return 0;
		};

		// Get weapon performance. A weapon type with no settings is left alone.
		let Some(paramdata) = self.param.get_weapon(weaponid) else {
			return 0;
		};

		// Cancel the scope.
		if let Some(ctrlhuman) = obj_mgr.get_human_object_mut(self.ctrlid) {
			ctrlhuman.set_disable_scope();
		}

		// Do nothing for the "case".
		if weaponid == ID_WEAPON_CASE {
			return 0;
		}

		// If the AI has a grenade during combat, it may switch away from it.
		if self.battlemode == AiMode::AiAction && weaponid == ID_WEAPON_GRENADE {
			let mut nextselectweapon = selectweapon + 1;
			if nextselectweapon == TOTAL_HAVEWEAPON as i32 {
				nextselectweapon = 0;
			}
			let nextnds = weapon[nextselectweapon as usize]
				.and_then(|i| obj_mgr.get_weapon_object(i as i32))
				.map_or(0, |weapon| weapon.param_data().2);

			// The draw comes first, so it happens whether or not there is ammunition in
			// the other slot.
			if !self.longattack {
				if get_rand(100) == 0 && nextnds > 0 {
					obj_mgr.change_have_weapon(self.ctrlid, -1);
					return 3;
				}
			} else if get_rand(66) == 0 && nextnds > 0 {
				obj_mgr.change_have_weapon(self.ctrlid, -1);
				return 3;
			}
		}

		// If there is no remaining ammunition.
		if lnbs == 0 {
			// Decide the processing probability.
			let ways = match self.battlemode {
				AiMode::AiNormal => 1,
				AiMode::AiCaution => 10,
				_ => 8,
			};

			// Process with a 1/ways probability.
			if get_rand(ways) == 0 {
				// Reload probability.
				let (ways, under) = match self.battlemode {
					AiMode::AiNormal => (1, 0),
					AiMode::AiCaution => (5, 3),
					_ => {
						if !self.longattack {
							// The probability is 3/4.
							(4, 2)
						} else {
							// The probability is 2/3.
							(3, 1)
						}
					}
				};

				// Discard if there is no ammunition.
				if nbs == 0 {
					obj_mgr.dump_weapon(self.ctrlid);
					return 1;
				}

				// Randomly reload or switch weapons.
				if get_rand(ways) <= under {
					obj_mgr.reload_weapon(self.ctrlid, sound);
					return 2;
				}
				obj_mgr.change_have_weapon(self.ctrlid, -1);
				return 3;
			}
		}

		// If the weapon supports burst mode switching.
		if paramdata.change_weapon_id != -1 {
			// Get the current weapon's burst setting.
			let burstcnt_s = paramdata.burstcnt;

			// Get the new weapon's burst setting.
			let Some(next) = self.param.get_weapon(paramdata.change_weapon_id) else {
				return 0;
			};
			let burstcnt_n = next.burstcnt;

			if !self.longattack {
				// If attacking at short range and switching from SEMI to FULL.
				if burstcnt_s == 1 && burstcnt_n != 1 {
					obj_mgr.change_weapon_id(self.ctrlid, d3dg, resource);
					return 4;
				}
			} else {
				// If attacking at long range and switching from FULL to SEMI.
				if burstcnt_s != 1 && burstcnt_n == 1 {
					obj_mgr.change_weapon_id(self.ctrlid, d3dg, resource);
					return 4;
				}
			}
		}

		0
	}

	/// `ThrowGrenade`: the grenade throwing path.
	///
	/// In progress: 0, throw finished: 1, no grenade: 2.
	fn throw_grenade(
		&mut self,
		obj_mgr: &mut ObjectManager,
		points: &PointDataInterface,
		sound: &mut SoundManager,
	) -> i32 {
		// Check whether the AI has a grenade.
		let (selectweapon, weapon) = match obj_mgr.get_human_object(self.ctrlid) {
			Some(human) => {
				let (selectweapon, weapon, _, _) = human.get_weapon();
				(selectweapon, weapon)
			}
			None => return 2,
		};

		let mut i = 0;
		while i < TOTAL_HAVEWEAPON {
			if let Some(index) = weapon[i] {
				let weaponid = obj_mgr
					.get_weapon_object(index as i32)
					.map_or(ID_WEAPON_NONE, |weapon| weapon.param_data().0);
				if weaponid == ID_WEAPON_GRENADE {
					break;
				}
			}
			i += 1;
		}

		// If the AI can switch to a grenade, switch.
		if i != TOTAL_HAVEWEAPON && i as i32 != selectweapon {
			obj_mgr.change_have_weapon(self.ctrlid, i as i32);
		}

		// Get the path and the person's height. Upstream reads the point data
		// unconditionally and aims at whatever the stack held when the point is missing,
		// so a missing point aims at zero here.
		let pdata = self
			.move_navi
			.get_path_point_data(points)
			.unwrap_or_default();
		let posy2 = self.posy + VIEW_HEIGHT;

		// Stop all movement once.
		self.obj_driver.reset_mode();

		// The angle toward the target point.
		let Some((atan_rx, atan_ry, _)) = check_target_angle(
			self.posx,
			posy2,
			self.posz,
			self.rx * -1.0 + PI / 2.0,
			self.ry,
			pdata.x,
			pdata.y,
			pdata.z,
			0.0,
		) else {
			return 0;
		};

		// If the AI has no grenade it still turns toward the throw, which is the original
		// XOPS behaviour: a person with no grenade on a grenade path keeps facing it.
		if i == TOTAL_HAVEWEAPON {
			if atan_rx > 0.0 {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
			}
			if atan_rx < 0.0 {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
			}
			self.arm_angle(obj_mgr);

			return 2;
		}

		// Turn.
		if atan_rx > 0.0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNLEFT);
		}
		if atan_rx < 0.0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNRIGHT);
		}
		if atan_ry > 0.0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
		}
		if atan_ry < 0.0 {
			self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
		}

		// Check whether the aim is settled.
		if atan_rx.abs() < degree_to_radian(1.5) && atan_ry.abs() < degree_to_radian(1.5) {
			// Do not allow continuous fire.
			if !self.weapon_blazing_mode(obj_mgr, self.ctrlid) && self.event_weapon_shot {
				return 0;
			}

			// Set the angle.
			let (rx, ry) = (self.rx, self.ry);
			if let Some(ctrlhuman) = obj_mgr.get_human_object_mut(self.ctrlid) {
				ctrlhuman.set_rx_ry(rx, ry);
			}

			// Throw.
			if obj_mgr.shot_weapon(self.ctrlid, sound) != 0 {
				self.event_weapon_shot = true;
				return 1;
			}
		}

		0
	}

	/// `ArmAngle`: sets the arm angle for the current state.
	fn arm_angle(&mut self, obj_mgr: &ObjectManager) {
		self.obj_driver.del_mode_flag(AI_CTRL_TURNUP);
		self.obj_driver.del_mode_flag(AI_CTRL_TURNDOWN);

		if self.main_weapon_type_no(obj_mgr, self.ctrlid) == ID_WEAPON_NONE {
			// Empty-handed, so keep aiming down.
			self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
		} else if self.battlemode == AiMode::AiCaution && self.cautioncnt > 0 {
			// During caution, level the arm out.
			let addry2 = 0.0 - self.ry;

			if addry2 > degree_to_radian(1.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
			}
			if addry2 < degree_to_radian(-1.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
			}
		} else {
			// Normal state with a weapon owned, so hold it at about -30 degrees.
			if self.ry < degree_to_radian(-32.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNUP);
			}
			if self.ry > degree_to_radian(-28.0) {
				self.obj_driver.set_mode_flag(AI_CTRL_TURNDOWN);
			}
		}
	}

	/// `SearchEnemy`: looks for an enemy in the field of view.
	///
	/// Found at short range: 1, found at long range: 2, nothing: 0.
	fn search_enemy(
		&mut self,
		obj_mgr: &ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
	) -> i32 {
		// Do not find enemies if the non-combat flag is enabled.
		if self.no_fight {
			return 0;
		}
		if self.battlemode == AiMode::AiAction {
			return 0;
		}

		// If the AI has a weapon, get the scope mode.
		let weaponid = self.main_weapon_type_no(obj_mgr, self.ctrlid);
		let mut weaponscope = 0;
		if weaponid != ID_WEAPON_NONE {
			weaponscope = self
				.param
				.get_weapon(weaponid)
				.map_or(0, |weapon| weapon.scopemode);
		}

		// Set the enemy search count and the search range, which is a distance and two
		// pairs of angles.
		let (searchloops, max_dist, a_rx, a_ry, b_rx, b_ry) =
			if self.battlemode == AiMode::AiNormal {
				let searchloops = self.level_param.search * AI_TOTALHUMAN_SCALE;
				let sparam = self.param.get_scope_param(weaponscope).unwrap_or(SCOPE_PARAMETER_0);
				let max_dist = sparam.ai_add_search_enemy_max_dist_normal
					+ 12.0 * (self.level_param.search - 2) as f32
					+ 350.0;
				(
					searchloops,
					max_dist,
					degree_to_radian(110.0),
					degree_to_radian(60.0),
					degree_to_radian(60.0),
					degree_to_radian(40.0),
				)
			} else {
				let searchloops = self.level_param.search * AI_TOTALHUMAN_SCALE + 4;
				let sparam = self.param.get_scope_param(weaponscope).unwrap_or(SCOPE_PARAMETER_0);
				let max_dist = sparam.ai_add_search_enemy_max_dist_caution
					+ 15.0 * (self.level_param.search - 2) as f32
					+ 420.0;
				(
					searchloops,
					max_dist,
					degree_to_radian(130.0),
					degree_to_radian(80.0),
					degree_to_radian(80.0),
					degree_to_radian(50.0),
				)
			};

		// Search for enemies the specified number of times. Each pass draws the person to
		// look at, and a second draw decides whether a long range sighting is taken.
		for _ in 0..searchloops {
			let targetid = get_rand(MAX_HUMAN as i32);

			if self
				.check_look_enemy(
					obj_mgr,
					blocks,
					coll_d,
					targetid,
					a_rx,
					a_ry,
					AI_CHECKSHORTATTACK_DIST,
				)
				.is_some()
			{
				self.longattack = false;
				return 1;
			}

			if self
				.check_look_enemy(obj_mgr, blocks, coll_d, targetid, b_rx, b_ry, max_dist)
				.is_some()
				&& get_rand(4) == 0
			{
				self.longattack = !self.move_navi.get_run2();
				return 2;
			}
		}

		0
	}

	/// `SearchShortEnemy`: looks for a close target during a long-range attack.
	fn search_short_enemy(
		&mut self,
		obj_mgr: &ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
	) -> i32 {
		let a_rx = degree_to_radian(100.0);
		let a_ry = degree_to_radian(52.0);

		for _ in 0..3 {
			let targetid = get_rand(MAX_HUMAN as i32);

			if self
				.check_look_enemy(
					obj_mgr,
					blocks,
					coll_d,
					targetid,
					a_rx,
					a_ry,
					AI_CHECKSHORTATTACK_DIST,
				)
				.is_some()
			{
				self.longattack = false;
				return 1;
			}
		}

		0
	}

	/// `CheckLookEnemy`: whether a person is an enemy in sight, and how far away.
	///
	/// The distance comes back as the upstream's `out_minDist`, and a sighting is what
	/// sets the attack target.
	fn check_look_enemy(
		&mut self,
		obj_mgr: &ObjectManager,
		blocks: &BlockDataInterface,
		coll_d: &Collision,
		id: i32,
		search_rx: f32,
		search_ry: f32,
		max_dist: f32,
	) -> Option<f32> {
		// Exit if the data is invalid.
		let ctrlteam = obj_mgr.get_human_object(self.ctrlid)?.get_param_data().3;
		let thuman = obj_mgr.get_human_object(id)?;
		if !thuman.enable_flag() {
			return None;
		}
		if thuman.dead_flag() {
			return None;
		}

		// Get the coordinates of the enemy.
		let targetteam = thuman.get_param_data().3;
		let (tx, ty, tz, _) = thuman.pos_data();

		// If the AI itself and the enemy are not the same team.
		if ctrlteam == targetteam {
			return None;
		}

		// Determine the distance and also get the angle.
		let (mrx, mry, dist2) = check_target_angle(
			self.posx,
			self.posy,
			self.posz,
			self.rx * -1.0 + PI / 2.0,
			0.0,
			tx,
			ty,
			tz,
			max_dist,
		)?;

		// If it is within the field of view in terms of angle, which is the half angle on
		// each axis.
		if !(mrx.abs() < search_rx / 2.0 && mry.abs() < search_ry / 2.0) {
			return None;
		}

		let dist = dist2.sqrt();

		// Get the vector.
		let vx = (tx - self.posx) / dist;
		let vy = (ty - self.posy) / dist;
		let vz = (tz - self.posz) / dist;

		// If no block is blocking, which is when the ray check does not hit.
		if coll_d
			.check_all_block_intersect_ray(
				blocks,
				self.posx,
				self.posy + VIEW_HEIGHT,
				self.posz,
				vx,
				vy,
				vz,
				dist,
			)
			.is_none()
		{
			self.enemy_human_id = Some(id);
			return Some(dist);
		}

		None
	}

	/// `CheckCorpse`: whether a person is a dead ally nearby.
	fn check_corpse(&self, obj_mgr: &ObjectManager, id: i32) -> bool {
		let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) else {
			return false;
		};
		if !(0..MAX_HUMAN as i32).contains(&id) {
			return false;
		}

		// Get the target's class.
		let Some(thuman) = obj_mgr.get_human_object(id) else {
			return false;
		};
		if !thuman.enable_flag() {
			return false;
		}

		// If dead, and an ally.
		if thuman.dead_flag() {
			let ctrlteam = ctrlhuman.get_param_data().3;
			let targetteam = thuman.get_param_data().3;

			if ctrlteam == targetteam {
				let (tposx, tposy, tposz, _) = thuman.pos_data();

				// Calculate the distance and angle.
				if let Some((atan, _, _)) = check_target_angle(
					self.posx,
					self.posy,
					self.posz,
					self.rx * -1.0 + PI / 2.0,
					0.0,
					tposx,
					tposy,
					tposz,
					22.0,
				) {
					if atan.abs() < degree_to_radian(40.0) {
						return true;
					}
				}
			}
		}

		false
	}

	/// `MovePath`: follows the movement path.
	fn move_path(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
	) {
		let target = self.move_navi.get_target_pos();

		// An abnormal path.
		if target.movemode == AiMoveNaviFlag::AiNaviMoveNull {
			return;
		}

		if !self.check_target_pos(false) {
			self.move_target(obj_mgr, blocks, coll_d, false);
		} else if target.pointmode == AiMoveNaviFlag::AiNaviPointWait
			|| target.pointmode == AiMoveNaviFlag::AiNaviPointTracking
		{
			self.turn_seen();
		} else if target.pointmode == AiMoveNaviFlag::AiNaviPointStop5Sec {
			if self.waitcnt < GAMEFPS as i32 * 5 {
				if self.stop_seen() {
					self.waitcnt += 1;
				}
			} else {
				self.waitcnt = 0;
				self.move_navi.move_path_next_state(points);
				self.move_navi.move_path_now_state(obj_mgr, points);
			}
		} else {
			self.waitcnt = 0;
			self.move_navi.move_path_next_state(points);
			self.move_navi.move_path_now_state(obj_mgr, points);
		}
	}

	/// `ActionMain`: the attack processing of one frame. Changed: true.
	#[allow(clippy::too_many_arguments)]
	fn action_main(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
		sound: &mut SoundManager,
		resource: &ResourceManager,
		config: &Config,
	) -> bool {
		let mut newbattlemode = AiMode::AiAction;

		// Attack processing.
		self.action(obj_mgr, blocks, coll_d, sound, resource, config);

		// Execute processing for a random path. It assumes "priority running".
		if self.move_navi.get_move_mode() == AiMode::AiRandom {
			self.move_navi.move_path_next_state(points);
			self.move_navi.move_path_now_state(obj_mgr, points);
		}

		if self.move_navi.get_run2() {
			// Priority running: move to the target point.
			if self.check_target_pos(false) {
				// Set this target point as the position to return to when caution is
				// released.
				let target = self.move_navi.get_target_pos();
				self.cautionback_posx = target.posx;
				self.cautionback_posz = target.posz;

				self.move_navi.move_path_next_state(points);
				self.move_navi.move_path_now_state(obj_mgr, points);
			} else {
				self.move_target2(obj_mgr, blocks, coll_d, false);
			}
		} else if self.longattack {
			// If attacking at long range, search for a nearby enemy.
			self.search_short_enemy(obj_mgr, blocks, coll_d);
		}

		// Attack end judgment.
		if self.action_cancel(obj_mgr, blocks, coll_d) {
			self.enemy_human_id = None;

			if self.move_navi.get_run2() {
				newbattlemode = AiMode::AiNormal;
			} else {
				newbattlemode = AiMode::AiCaution;
				self.cautioncnt = 160;
				self.face_caution_flag = false;
				self.face_caution_rx = 0.0;
			}
		}

		// Judge the setting.
		if self.battlemode != newbattlemode {
			self.battlemode = newbattlemode;
			return true;
		}
		false
	}

	/// `CautionMain`: the caution processing of one frame. Changed: true.
	fn caution_main(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
		sound: &SoundManager,
	) -> bool {
		let mut newbattlemode = AiMode::AiCaution;

		// Get the coordinates and team number.
		let teamid = obj_mgr
			.get_human_object(self.ctrlid)
			.map_or(0, |human| human.get_param_data().3);

		// Get the hit and sound situation. The hit flag is consumed here.
		let (hit_flag, caution_rx) = match obj_mgr.get_human_object_mut(self.ctrlid) {
			Some(ctrlhuman) => ctrlhuman.check_hit(),
			None => (false, 0.0),
		};
		let mut soundlist = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		let mut soundlists = sound.get_world_sound(
			self.posx,
			self.posy + VIEW_HEIGHT,
			self.posz,
			teamid,
			&self.param,
			&mut soundlist,
		);

		// If the non-combat flag is enabled, treat sounds as inaudible.
		if self.no_fight {
			soundlists = 0;
		}

		// Main processing.
		if self.enemy_human_id.is_some() {
			// An enemy has already been found.
			newbattlemode = AiMode::AiAction;
			self.actioncnt = 0;
		} else if self.search_enemy(obj_mgr, blocks, coll_d) != 0 {
			newbattlemode = AiMode::AiAction;
			self.actioncnt = 0;
		} else if hit_flag {
			// Was hit: resume caution and face the direction the hit came from.
			self.cautioncnt = 160;
			self.face_caution_flag = true;
			self.face_caution_rx = caution_rx;
		} else if soundlists > 0 {
			// Heard a sound.
			self.cautioncnt = 160;
		} else if self.cautioncnt == 0 {
			// Caution is ending.
			if !self.check_target_pos(true) {
				// Far from the caution start position, so approach it.
				self.move_target(obj_mgr, blocks, coll_d, true);
			} else {
				newbattlemode = AiMode::AiNormal;
				self.face_caution_flag = false;
				self.face_caution_rx = 0.0;

				// If it is a caution wait path, proceed to the next.
				if let Some(pdata) = self.move_navi.get_path_point_data(points) {
					if pdata.p1 == POINT_P1TYPE_AIPATH && pdata.p2 == 4 {
						self.move_navi.move_path_next_state(points);
						self.move_navi.move_path_now_state(obj_mgr, points);
					}
				}
			}
		} else if self.cautioncnt < 100 && !self.face_caution_flag {
			// When under 100 frames, randomly end caution by counting to 0.
			if get_rand(50) == 0 {
				self.cautioncnt = 0;
			}
		} else {
			self.cautioncnt -= 1;
		}

		// If tracking and too far from the target, randomly end caution. The draw only
		// happens while tracking.
		if self.move_navi.get_move_mode() == AiMode::AiTracking && get_rand(3) == 0 {
			let target_id = self.move_navi.get_target_human_id();
			let (tx, tz) = match obj_mgr.get_human_object(target_id) {
				Some(human) => {
					let (x, _, z, _) = human.pos_data();
					(x, z)
				}
				None => (0.0, 0.0),
			};
			let x = self.posx - tx;
			let z = self.posz - tz;
			if x * x + z * z > 25.0 * 25.0 {
				self.cautioncnt = 0;
			}
		}

		// Rotation and arm angle.
		self.turn_seen();
		self.arm_angle(obj_mgr);

		// Judge the setting.
		if self.battlemode != newbattlemode {
			self.battlemode = newbattlemode;
			return true;
		}
		false
	}

	/// `NormalMain`: the normal processing of one frame. Changed: true.
	fn normal_main(
		&mut self,
		obj_mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
		sound: &mut SoundManager,
	) -> bool {
		let mut newbattlemode = AiMode::AiNormal;
		let mut event_throw_grenade = 0;

		self.move_navi.move_path_now_state(obj_mgr, points);
		self.enemy_human_id = None;

		// Get the coordinates and team number.
		let teamid = obj_mgr
			.get_human_object(self.ctrlid)
			.map_or(0, |human| human.get_param_data().3);

		// Get the hit and sound situation. The hit flag is consumed here.
		let (hit_flag, caution_rx) = match obj_mgr.get_human_object_mut(self.ctrlid) {
			Some(ctrlhuman) => ctrlhuman.check_hit(),
			None => (false, 0.0),
		};
		let mut soundlist = [Soundlist::default(); MAX_SOUNDMGR_LIST];
		let mut soundlists = sound.get_world_sound(
			self.posx,
			self.posy + VIEW_HEIGHT,
			self.posz,
			teamid,
			&self.param,
			&mut soundlist,
		);

		// If the non-combat flag is enabled, treat sounds as inaudible.
		if self.no_fight {
			soundlists = 0;
		}

		// Execute processing if it is a random path.
		if self.move_navi.get_move_mode() == AiMode::AiRandom {
			self.move_navi.move_path_next_state(points);
			self.move_navi.move_path_now_state(obj_mgr, points);
		}

		// Grenade path.
		if self.move_navi.get_move_mode() == AiMode::AiGrenade {
			event_throw_grenade = self.throw_grenade(obj_mgr, points, sound);
			if event_throw_grenade != 0 {
				self.move_navi.move_path_next_state(points);
				self.move_navi.move_path_now_state(obj_mgr, points);
			}
		}
		if event_throw_grenade != 1 {
			self.event_weapon_shot = false;
		}

		if self.move_navi.get_move_mode() == AiMode::AiRun2 {
			// Priority running processing: enter the attack when an enemy is found.
			if self.search_enemy(obj_mgr, blocks, coll_d) != 0 {
				newbattlemode = AiMode::AiAction;
			} else {
				// Execute movement.
				self.move_path(obj_mgr, blocks, points, coll_d);
			}
		} else {
			// Processing to enter the caution judgment. A hit is decided first, so the
			// search does not run at all on the frame the AI was hit, and the corpse draw
			// is the last term of the chain, so it only happens when nothing else was
			// found or heard.
			if hit_flag {
				// Was hit.
				newbattlemode = AiMode::AiCaution;
				self.cautioncnt = 160;
				self.cautionback_posx = self.posx;
				self.cautionback_posz = self.posz;
				self.face_caution_flag = true;
				self.face_caution_rx = caution_rx;
			} else {
				let found = self.search_enemy(obj_mgr, blocks, coll_d) != 0;
				let heard = soundlists > 0;
				let corpse = if !found && !heard {
					self.check_corpse(obj_mgr, get_rand(MAX_HUMAN as i32))
				} else {
					false
				};

				if found || heard || corpse {
					newbattlemode = AiMode::AiCaution;
					self.cautioncnt = 160;
					self.cautionback_posx = self.posx;
					self.cautionback_posz = self.posz;
					self.face_caution_flag = false;
					self.face_caution_rx = 0.0;
				} else {
					// Execute movement.
					self.move_path(obj_mgr, blocks, points, coll_d);
				}
			}
		}

		// Set the arm angle.
		if self.move_navi.get_move_mode() != AiMode::AiGrenade {
			self.arm_angle(obj_mgr);
		}

		// Judge the setting.
		if self.battlemode != newbattlemode {
			self.battlemode = newbattlemode;
			return true;
		}
		false
	}

	/// The human settings of a person, upstream `Param->GetHuman(paramid, ...)`.
	///
	/// A type number outside the table answers nothing, where upstream reads whatever the
	/// failed lookup left on the stack.
	fn human_param(&self, obj_mgr: &ObjectManager, human_id: i32) -> Option<HumanParameter> {
		let human = obj_mgr.get_human_object(human_id)?;
		let (paramid, _, _, _) = human.get_param_data();
		self.param.get_human(paramid)
	}

	/// Whether a person is a zombie, which is the `Paraminfo.type == 2` test upstream
	/// repeats in `MoveTarget` and `Action`.
	fn is_zombie(&self, obj_mgr: &ObjectManager, human_id: i32) -> bool {
		self.human_param(obj_mgr, human_id)
			.map_or(false, |param| param.r#type == 2)
	}

	/// `human::GetMainWeaponTypeNO`.
	///
	/// [`crate::human::Human::get_main_weapon_type_no`] wants the whole weapon pool as a
	/// slice and the manager hands out one weapon at a time, so this walks the equipped
	/// slot instead.
	fn main_weapon_type_no(&self, obj_mgr: &ObjectManager, human_id: i32) -> i32 {
		let Some(human) = obj_mgr.get_human_object(human_id) else {
			return ID_WEAPON_NONE;
		};
		let (selectweapon, weapon, _, _) = human.get_weapon();

		match weapon[selectweapon as usize] {
			None => ID_WEAPON_NONE,
			Some(index) => obj_mgr
				.get_weapon_object(index as i32)
				.map_or(ID_WEAPON_NONE, |weapon| weapon.param_data().0),
		}
	}

	/// `human::GetWeaponBlazingmode`, through the equipped slot for the same reason.
	fn weapon_blazing_mode(&self, obj_mgr: &ObjectManager, human_id: i32) -> bool {
		let Some(human) = obj_mgr.get_human_object(human_id) else {
			return false;
		};
		let (selectweapon, weapon, _, _) = human.get_weapon();
		let Some(index) = weapon[selectweapon as usize] else {
			return false;
		};
		let Some(weaponid) = obj_mgr
			.get_weapon_object(index as i32)
			.map(|weapon| weapon.param_data().0)
		else {
			return false;
		};

		self.param
			.get_weapon(weaponid)
			.map_or(false, |data| data.burstcnt != 1)
	}
}

/// One leg of `MoveRandom`'s obstacle test: a block at waist height, or no block under the
/// feet, which is what stops a person from walking into a wall or off a ledge.
///
/// The second ray is only cast when the first misses, which is the upstream short circuit.
fn blocked_ahead(
	blocks: &BlockDataInterface,
	coll_d: &Collision,
	posx: f32,
	posy: f32,
	posz: f32,
	vx: f32,
	vz: f32,
) -> bool {
	if coll_d
		.check_all_block_intersect_dummy_ray(
			blocks,
			posx,
			posy + HUMAN_MAPCOLLISION_HEIGHT,
			posz,
			vx,
			0.0,
			vz,
			HUMAN_MAPCOLLISION_R,
		)
		.is_some()
	{
		return true;
	}

	coll_d
		.check_all_block_intersect_dummy_ray(
			blocks,
			posx,
			posy - 1.0,
			posz,
			vx,
			0.0,
			vz,
			HUMAN_MAPCOLLISION_R,
		)
		.is_none()
}

// ---------------------------------------------------------------------------
// AIMoveNavi
// ---------------------------------------------------------------------------

/// The target a person is moving to, which upstream hands back through five pointers, any
/// of which the caller may leave null.
#[derive(Clone, Copy, Debug)]
pub struct TargetPos {
	/// X coordinate of the target.
	pub posx: f32,
	/// Z coordinate of the target.
	pub posz: f32,
	/// Horizontal angle of the target.
	pub rx: f32,
	/// Movement mode toward the target.
	pub movemode: AiMoveNaviFlag,
	/// Point mode of the target point.
	pub pointmode: AiMoveNaviFlag,
}

/// The person movement management class, upstream `AIMoveNavi`.
///
/// It decides where a person in normal mode is heading: along the point data's AI path,
/// or to a held position or person.
pub struct AIMoveNavi {
	/// Person number of the AI itself (the controlled target).
	ctrlid: i32,
	/// Movement mode.
	movemode: AiMode,
	/// Does not read the movement path.
	hold: bool,
	/// Point data number of the path.
	path_pointid: i32,
	/// Data number of the person to use as the target.
	target_humanid: i32,
	/// X coordinate of the target.
	target_posx: f32,
	/// Z coordinate of the target.
	target_posz: f32,
	/// Horizontal angle of the target.
	target_rx: f32,
}

impl AIMoveNavi {
	/// Constructor.
	pub fn new(ctrlid: i32) -> AIMoveNavi {
		AIMoveNavi {
			ctrlid,
			movemode: AiMode::AiWait,
			hold: false,
			path_pointid: 0,
			target_humanid: -1,
			target_posx: 0.0,
			target_posz: 0.0,
			target_rx: 0.0,
		}
	}

	/// `SetClass`, reduced to the person number for the same reason as [`AIcontrol`].
	pub fn set_class(&mut self, ctrlid: i32) {
		self.ctrlid = ctrlid;
	}

	/// `Init`: starts at the path the person's own point data names.
	pub fn init(&mut self, obj_mgr: &ObjectManager, points: &PointDataInterface) {
		self.movemode = AiMode::AiNull;
		self.hold = false;
		self.path_pointid = 0;
		self.target_humanid = -1;

		if let Some(ctrlhuman) = obj_mgr.get_human_object(self.ctrlid) {
			let (_, point_dataid, _, _) = ctrlhuman.get_param_data();
			self.path_pointid = point_dataid;
		}

		self.move_path_next_state(points);
	}

	/// `MovePathNowState`: applies the target of the point the path is standing on.
	///
	/// Must be called every frame while tracking, because it is what refreshes the target
	/// person's coordinates.
	pub fn move_path_now_state(
		&mut self,
		obj_mgr: &ObjectManager,
		points: &PointDataInterface,
	) -> bool {
		if self.hold {
			if self.movemode == AiMode::AiTracking {
				if let Some(targethuman) = obj_mgr.get_human_object(self.target_humanid) {
					let (posx, _, posz, rx) = targethuman.pos_data();
					self.target_posx = posx;
					self.target_posz = posz;
					self.target_rx = rx;
				}
			}
			return true;
		}

		// Get the point information.
		let Some(pdata) = points.getdata(self.path_pointid) else {
			self.movemode = AiMode::AiNull;
			return false;
		};

		// If it is a movement path.
		if pdata.p1 == POINT_P1TYPE_AIPATH {
			// Set the movement state. An unknown p2 leaves the mode as it was, which is
			// upstream's `default: break`.
			match pdata.p2 {
				0 => self.movemode = AiMode::AiWalk,
				1 => self.movemode = AiMode::AiRun,
				2 => self.movemode = AiMode::AiWait,
				3 => self.movemode = AiMode::AiTracking,
				4 => self.movemode = AiMode::AiWait,
				5 => self.movemode = AiMode::AiStop5Sec,
				6 => self.movemode = AiMode::AiGrenade,
				7 => self.movemode = AiMode::AiRun2,
				_ => {}
			}

			if self.movemode == AiMode::AiTracking {
				if self.target_humanid == -1 {
					let nextpointp4 = pdata.p3;

					// Get the point (person) information.
					let (total, first) = points.search_pointdata(0x08, 0, 0, 0, nextpointp4, 0);
					if total == 0 {
						return false;
					}
					let Some(found) = first else {
						return false;
					};

					// Search for the person.
					let Some(id) = obj_mgr.search_human(found.p4) else {
						return false;
					};
					self.target_humanid = id as i32;
				}

				if let Some(targethuman) = obj_mgr.get_human_object(self.target_humanid) {
					let (posx, _, posz, rx) = targethuman.pos_data();
					self.target_posx = posx;
					self.target_posz = posz;
					self.target_rx = rx;
				}
			} else {
				// Apply the information.
				self.target_posx = pdata.x;
				self.target_posz = pdata.z;
				self.target_rx = pdata.r;
			}

			return true;
		}

		// If it is a random path.
		if pdata.p1 == POINT_P1TYPE_RAND_AIPATH {
			self.movemode = AiMode::AiRandom;
			return false;
		}

		self.movemode = AiMode::AiNull;
		false
	}

	/// `MovePathNextState`: searches for the next target point.
	///
	/// This is where a random path draws its one number, to pick between the two points it
	/// names.
	pub fn move_path_next_state(&mut self, points: &PointDataInterface) -> bool {
		// Get the point information.
		let Some(pdata) = points.getdata(self.path_pointid) else {
			self.movemode = AiMode::AiNull;
			return false;
		};

		let mut nextpointp4 = pdata.p3;

		// Random path processing.
		if pdata.p1 == POINT_P1TYPE_RAND_AIPATH {
			nextpointp4 = if get_rand(2) == 0 { pdata.p2 } else { pdata.p3 };
			self.movemode = AiMode::AiRandom;
		}

		// Search for the point.
		let (total, first) = points.search_pointdata(0x08, 0, 0, 0, nextpointp4, 0);
		if total == 0 {
			return false;
		}
		let Some(found) = first else {
			return false;
		};

		self.path_pointid = found.id;
		true
	}

	/// `SetHoldWait`: waits at a location whatever the path says.
	pub fn set_hold_wait(&mut self, px: f32, pz: f32, rx: f32) {
		self.movemode = AiMode::AiWait;
		self.hold = true;
		self.target_posx = px;
		self.target_posz = pz;
		self.target_rx = rx;
	}

	/// `SetHoldTracking`: tracks a person whatever the path says.
	pub fn set_hold_tracking(&mut self, id: i32) {
		if !(0..MAX_HUMAN as i32).contains(&id) {
			return;
		}

		self.movemode = AiMode::AiTracking;
		self.hold = true;
		self.target_humanid = id;
	}

	/// `GetMoveMode`.
	pub fn get_move_mode(&self) -> AiMode {
		self.movemode
	}

	/// `GetRun2`: whether it is priority running.
	pub fn get_run2(&self) -> bool {
		self.movemode == AiMode::AiRun2
	}

	/// `GetTargetHumanID`.
	pub fn get_target_human_id(&self) -> i32 {
		self.target_humanid
	}

	/// `GetPathPointData`: the point data the path is standing on.
	pub fn get_path_point_data(&self, points: &PointDataInterface) -> Option<Pointdata> {
		points.getdata(self.path_pointid)
	}

	/// `GetTargetPos`: the target to move to.
	///
	/// The coordinates are not updated here: call [`Self::move_path_now_state`] first.
	pub fn get_target_pos(&self) -> TargetPos {
		let movemode = match self.movemode {
			AiMode::AiWalk | AiMode::AiWait | AiMode::AiStop5Sec => {
				AiMoveNaviFlag::AiNaviMoveWalk
			}
			AiMode::AiRun => AiMoveNaviFlag::AiNaviMoveRun,
			AiMode::AiRun2 => AiMoveNaviFlag::AiNaviMoveRun2,
			AiMode::AiTracking => AiMoveNaviFlag::AiNaviMoveTracking,
			_ => AiMoveNaviFlag::AiNaviMoveNull,
		};

		let pointmode = match self.movemode {
			AiMode::AiWait => AiMoveNaviFlag::AiNaviPointWait,
			AiMode::AiStop5Sec => AiMoveNaviFlag::AiNaviPointStop5Sec,
			AiMode::AiTracking => AiMoveNaviFlag::AiNaviPointTracking,
			AiMode::AiGrenade => AiMoveNaviFlag::AiNaviPointGrenade,
			_ => AiMoveNaviFlag::AiNaviPointNull,
		};

		TargetPos {
			posx: self.target_posx,
			posz: self.target_posz,
			rx: self.target_rx,
			movemode,
			pointmode,
		}
	}
}

// ---------------------------------------------------------------------------
// AIObjectDriver
// ---------------------------------------------------------------------------

/// The person movement and rotation control class, upstream `AIObjectDriver`.
///
/// It holds the flags one frame's decisions set and turns them into movement and rotation
/// on the object manager.
pub struct AIObjectDriver {
	/// Person number of the AI itself (the controlled target).
	ctrlid: i32,
	/// Flags for the movement and rotation direction.
	moveturn_mode: u32,
	/// X axis rotation angular acceleration.
	addrx: f32,
	/// Y axis rotation angular acceleration.
	addry: f32,
}

impl AIObjectDriver {
	/// Constructor.
	pub fn new(ctrlid: i32) -> AIObjectDriver {
		AIObjectDriver {
			ctrlid,
			moveturn_mode: 0,
			addrx: 0.0,
			addry: 0.0,
		}
	}

	/// `SetClass`, reduced to the person number for the same reason as [`AIcontrol`].
	pub fn set_class(&mut self, ctrlid: i32) {
		self.ctrlid = ctrlid;
	}

	/// `Init`.
	pub fn init(&mut self) {
		self.addrx = 0.0;
		self.addry = 0.0;

		self.reset_mode();
	}

	/// `ResetMode`: clears the control flags, which stops all movement once.
	pub fn reset_mode(&mut self) {
		self.moveturn_mode = 0;
	}

	/// `SetModeFlag`.
	pub fn set_mode_flag(&mut self, flag: u32) {
		self.moveturn_mode = set_flag(self.moveturn_mode, flag);
	}

	/// `DelModeFlag`.
	pub fn del_mode_flag(&mut self, flag: u32) {
		self.moveturn_mode = del_flag(self.moveturn_mode, flag);
	}

	/// `GetModeFlag`.
	pub fn get_mode_flag(&self, flag: u32) -> bool {
		get_flag(self.moveturn_mode, flag)
	}

	/// `ControlObject`: executes the movement and rotation the flags ask for.
	///
	/// Call this only once per frame.
	pub fn control_object(&mut self, obj_mgr: &mut ObjectManager) {
		// Execute movement.
		if self.get_mode_flag(AI_CTRL_MOVEFORWARD) {
			obj_mgr.move_forward(self.ctrlid);
		}
		if self.get_mode_flag(AI_CTRL_MOVEBACKWARD) {
			obj_mgr.move_back(self.ctrlid);
		}
		if self.get_mode_flag(AI_CTRL_MOVELEFT) {
			obj_mgr.move_left(self.ctrlid);
		}
		if self.get_mode_flag(AI_CTRL_MOVERIGHT) {
			obj_mgr.move_right(self.ctrlid);
		}
		if self.get_mode_flag(AI_CTRL_MOVEWALK) {
			obj_mgr.move_walk(self.ctrlid);
		}

		// Execute turning, which adds to the rotation speed.
		if self.get_mode_flag(AI_CTRL_TURNUP) {
			self.addry += AI_ADDTURNRAD;
		}
		if self.get_mode_flag(AI_CTRL_TURNDOWN) {
			self.addry -= AI_ADDTURNRAD;
		}
		if self.get_mode_flag(AI_CTRL_TURNLEFT) {
			self.addrx -= AI_ADDTURNRAD;
		}
		if self.get_mode_flag(AI_CTRL_TURNRIGHT) {
			self.addrx += AI_ADDTURNRAD;
		}

		// Get the angle.
		let Some(ctrlhuman) = obj_mgr.get_human_object_mut(self.ctrlid) else {
			return;
		};
		let (rx, ry) = ctrlhuman.rx_ry();

		// Add to the angle.
		let rx = rx + self.addrx;
		let mut ry = ry + self.addry;

		// Clamp the vertical rotation range.
		if ry > degree_to_radian(70.0) {
			ry = degree_to_radian(70.0);
		}
		if ry < degree_to_radian(-70.0) {
			ry = degree_to_radian(-70.0);
		}

		// Set the angle.
		ctrlhuman.set_rx_ry(rx, ry);

		// Decay the rotation speed.
		self.addrx *= 0.8;
		self.addry *= 0.8;

		// Correct toward 0.0.
		if self.addrx.abs() < degree_to_radian(0.2) {
			self.addrx = 0.0;
		}
		if self.addry.abs() < degree_to_radian(0.2) {
			self.addry = 0.0;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::atomic::{AtomicU64, Ordering};
	use crate::datafile::TOTAL_BLOCKTEXTURE;
	use std::fs;
	use std::path::PathBuf;

	// --- helpers ---------------------------------------------------------

	/// A path in the temp directory that this test can write to.
	fn temp_path(name: &str) -> PathBuf {
		// Unique per call, not merely per name. Several tests build the same fixture, and
		// they run at once, so a path derived only from the name would have them writing the
		// same file: `fs::write` truncates before it writes, and a reader that arrives in
		// between sees a half written file and loads a truncated fixture. That reads as a
		// path test failing for no reason, about one run in ten.
		static NEXT: AtomicU64 = AtomicU64::new(0);
		temp_dir().join(format!(
			"xops_ai_{}_{}_{}",
			std::process::id(),
			NEXT.fetch_add(1, Ordering::Relaxed),
			name
		))
	}

	/// The temporary directory, `std::env::temp_dir` under a name that also reads well here.
	fn temp_dir() -> PathBuf {
		std::env::temp_dir()
	}

	/// Places an enabled, living human of type 0 at a position, on a team, with the point
	/// identifier a path point can name it by.
	fn place_human(
		mgr: &mut ObjectManager,
		id: i32,
		x: f32,
		y: f32,
		z: f32,
		team: i32,
		rx: f32,
		ry: f32,
		p4: i16,
	) {
		let human = mgr.get_human_object_mut(id).expect("the pool holds every slot");
		human.set_pos_data(x, y, z, rx);
		// `init` resets the arm angle, so the angles go in after the settings.
		human.set_param_data(0, 0, p4, team, true);
		human.set_rx_ry(rx, ry);
		human.object.set_enable_flag(true);
	}

	/// A collision that has never been initialised, so every query answers no: an empty
	/// map, with nothing in the way.
	fn empty_world() -> (BlockDataInterface, Collision) {
		(BlockDataInterface::new(), Collision::new())
	}

	fn write_f32_le(out: &mut Vec<u8>, value: f32) {
		out.extend_from_slice(&value.to_bits().to_le_bytes());
	}

	/// A .bd1 image holding one unit cube whose minimum corner is at `(ox, oy, oz)`.
	fn cube_image(ox: f32, oy: f32, oz: f32) -> Vec<u8> {
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
					0 => corner.0 + ox,
					1 => corner.1 + oy,
					_ => corner.2 + oz,
				};
				write_f32_le(&mut out, value);
			}
		}
		for i in 24..72 {
			write_f32_le(&mut out, i as f32);
		}
		for j in 0..6 {
			out.extend_from_slice(&[j as u8, 0, 0, 0]);
		}
		out.extend_from_slice(&[0; 4]);
		out
	}

	/// A map holding one unit cube, loaded and prepared the way a mission load leaves it.
	fn cube_map(name: &str, ox: f32, oy: f32, oz: f32) -> (BlockDataInterface, Collision) {
		let path = temp_path(name);
		fs::write(&path, cube_image(ox, oy, oz)).expect("write the temp map");

		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(path.to_string_lossy().as_bytes()), 0);
		blocks.calculation_blockdata(false);

		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(Some(&blocks)), 0);
		(blocks, coll)
	}

	/// A .pd1 image: `points` as (x, y, z, r, p1, p2, p3, p4).
	fn pd1_image(points: &[(f32, f32, f32, f32, i8, i8, i8, i8)]) -> Vec<u8> {
		let mut out = Vec::new();
		out.extend_from_slice(&(points.len() as u16).to_le_bytes());
		for &(x, y, z, r, p1, p2, p3, p4) in points {
			write_f32_le(&mut out, x);
			write_f32_le(&mut out, y);
			write_f32_le(&mut out, z);
			write_f32_le(&mut out, r);
			out.extend_from_slice(&[p1 as u8, p2 as u8, p3 as u8, p4 as u8]);
		}
		out
	}

	/// Loads a point data file the way a mission load does.
	fn load_points(name: &str, points: &[(f32, f32, f32, f32, i8, i8, i8, i8)]) -> PointDataInterface {
		let path = temp_path(name);
		fs::write(&path, pd1_image(points)).expect("write the temp points");

		let mut data = PointDataInterface::new();
		assert_eq!(data.load_file_data(path.to_string_lossy().as_bytes()), 0);
		// The file has done its work, and these names are unique so they would otherwise
		// pile up in the temporary directory.
		let _ = fs::remove_file(&path);
		data
	}

	/// A sound manager with no sound class, so every call just queues.
	fn sound() -> SoundManager {
		SoundManager::new()
	}

	/// A renderer with no window and no surface, the same shape the object manager's own
	/// tests use. Only the paths that have to touch the renderer get one.
	fn headless() -> D3DGraphics {
		let instance = xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
			backends: xops_platform::wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = xops_platform::gfx::Gfx::new(&instance, None, 64, 48, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// One frame of the AI, with the classes a test does not care about standing in.
	fn frame(
		ai: &mut AIcontrol,
		mgr: &mut ObjectManager,
		blocks: &BlockDataInterface,
		points: &PointDataInterface,
		coll_d: &Collision,
		sound: &mut SoundManager,
		d3dg: &D3DGraphics,
		resource: &ResourceManager,
		config: &Config,
	) {
		ai.process(mgr, blocks, points, coll_d, sound, d3dg, resource, config);
	}

	// --- perception ------------------------------------------------------

	/// The field of view is the half angle on each axis, and the boundary is where the
	/// half ends.
	#[test]
	fn the_field_of_view_takes_just_inside_and_refuses_just_outside() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		let (blocks, coll_d) = empty_world();

		let mut ai = AIcontrol::new(0);
		ai.update_pos(&mgr);
		assert_eq!((ai.posx, ai.posy, ai.posz, ai.rx), (0.0, 0.0, 0.0, 0.0));
		// The horizontal half angle is AI_SEARCH_RX / 2, which is 55 degrees. A target at
		// (100 sin t, 0, 100 cos t) reads as the angle t from the +Z the AI faces.
		for (angle, expected) in [(54.9f32, true), (55.1, false)] {
			let t = degree_to_radian(angle);
			place_human(&mut mgr, 1, 100.0 * t.sin(), 0.0, 100.0 * t.cos(), 2, 0.0, 0.0, 1);

			let seen = ai.check_look_enemy(
				&mgr,
				&blocks,
				&coll_d,
				1,
				AI_SEARCH_RX,
				AI_SEARCH_RY,
				620.0,
			);
			assert_eq!(seen.is_some(), expected, "horizontal angle {angle}");
			if expected {
				// The distance comes back as the upstream's out_minDist.
				assert!((seen.unwrap() - 100.0).abs() < 1e-3);
				assert_eq!(ai.get_enemy_human_id(), 1);
			}
		}

		// The vertical half angle is AI_SEARCH_RY / 2, which is 30 degrees. A target at
		// (0, 100 tan t, 100) reads as the angle t above the horizon.
		for (angle, expected) in [(29.9f32, true), (30.1, false)] {
			let h = 100.0 * degree_to_radian(angle).tan();
			place_human(&mut mgr, 1, 0.0, h, 100.0, 2, 0.0, 0.0, 1);

			let seen = ai.check_look_enemy(
				&mgr,
				&blocks,
				&coll_d,
				1,
				AI_SEARCH_RX,
				AI_SEARCH_RY,
				620.0,
			);
			assert_eq!(seen.is_some(), expected, "vertical angle {angle}");
		}

		// An ally is never an enemy, whatever the angle.
		place_human(&mut mgr, 1, 0.0, 0.0, 100.0, 1, 0.0, 0.0, 1);
		assert!(ai
			.check_look_enemy(&mgr, &blocks, &coll_d, 1, AI_SEARCH_RX, AI_SEARCH_RY, 620.0)
			.is_none());

		// A person out of use is not looked at either.
		place_human(&mut mgr, 1, 0.0, 0.0, 100.0, 2, 0.0, 0.0, 1);
		mgr.get_human_object_mut(1).unwrap().object.set_enable_flag(false);
		assert!(ai
			.check_look_enemy(&mgr, &blocks, &coll_d, 1, AI_SEARCH_RX, AI_SEARCH_RY, 620.0)
			.is_none());
	}

	/// The detection distance is the maximum the search passes in, inclusive at the edge.
	#[test]
	fn the_detection_distance_stops_at_its_boundary() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		let (blocks, coll_d) = empty_world();

		let mut ai = AIcontrol::new(0);
		ai.update_pos(&mgr);

		// Straight ahead, so only the distance is in question.
		for (distance, expected) in [(99.9f32, true), (100.0, true), (100.1, false)] {
			place_human(&mut mgr, 1, 0.0, 0.0, distance, 2, 0.0, 0.0, 1);

			let seen = ai.check_look_enemy(
				&mgr,
				&blocks,
				&coll_d,
				1,
				AI_SEARCH_RX,
				AI_SEARCH_RY,
				100.0,
			);
			assert_eq!(seen.is_some(), expected, "distance {distance}");
		}

		// A maximum of zero or less turns the distance check off, which is what upstream
		// does with a check distance of zero.
		place_human(&mut mgr, 1, 0.0, 0.0, 5000.0, 2, 0.0, 0.0, 1);
		let seen = ai.check_look_enemy(&mgr, &blocks, &coll_d, 1, AI_SEARCH_RX, AI_SEARCH_RY, 0.0);
		assert!(seen.is_some());
		assert!((seen.unwrap() - 5000.0).abs() < 0.1);
	}

	/// A block between the two is what stops a sighting, which is the difference between
	/// an enemy that tracks through a wall and one that does not.
	#[test]
	fn a_block_in_the_way_blocks_the_sight() {
		// The AI stands ten units out on -X facing +X, so the reference direction is
		// +X and the target straight ahead is at +X. The eye is at y = 19.
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, -10.0, 0.0, 0.0, 1, PI / 2.0, 0.0, 0);
		place_human(&mut mgr, 1, 10.0, 0.0, 0.0, 2, 0.0, 0.0, 1);

		let (blocks, coll_d) = cube_map("los.bd1", -1.0, 18.5, -0.5);

		let mut ai = AIcontrol::new(0);
		ai.update_pos(&mgr);
		assert_eq!(ai.rx, PI / 2.0);

		// The cube spans x -1..0, y 18.5..19.5 and z -0.5..0.5, so the ray from the eye
		// at (-10, 19, 0) to the target at (10, 19, 0) goes straight through it.
		assert!(ai
			.check_look_enemy(&mgr, &blocks, &coll_d, 1, AI_SEARCH_RX, AI_SEARCH_RY, 620.0)
			.is_none());

		// The same map and the same distance, with the target off to the side so the ray
		// passes beside the cube, is a sighting.
		place_human(&mut mgr, 1, 10.0, 0.0, 5.0, 2, 0.0, 0.0, 1);
		let seen = ai.check_look_enemy(&mgr, &blocks, &coll_d, 1, AI_SEARCH_RX, AI_SEARCH_RY, 620.0);
		assert!(seen.is_some());
		assert!((seen.unwrap() - (425.0f32).sqrt()).abs() < 1e-3);
	}

	// --- the aim ---------------------------------------------------------

	/// The allowed error of the AI level moves the tolerance within which an enemy counts
	/// as captured, and the extremes are the two ends of the level table.
	#[test]
	fn the_allowed_error_spreads_the_aim_at_both_ends() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &PointDataInterface::new());

		// Level 1 allows the most error, level 5 the least.
		ai.set_ai_level(&mgr, 1);
		assert_eq!(ai.level_param.limitserror, 2);
		ai.set_ai_level(&mgr, 5);
		assert_eq!(ai.level_param.limitserror, -3);

		// No scope: 8 degrees at short range, 4 at long range, plus 0.5 and 0.2 degrees
		// per point of allowed error.
		ai.set_ai_level(&mgr, 1);
		ai.longattack = false;
		assert!((ai.shot_angle(&SCOPE_PARAMETER_0, false) - 0.1570796).abs() < 1e-6);
		ai.set_ai_level(&mgr, 5);
		assert!((ai.shot_angle(&SCOPE_PARAMETER_0, false) - 0.1134464).abs() < 1e-6);

		ai.longattack = true;
		assert!((ai.shot_angle(&SCOPE_PARAMETER_0, false) - 0.0593412).abs() < 1e-6);
		ai.set_ai_level(&mgr, 1);
		assert!((ai.shot_angle(&SCOPE_PARAMETER_0, false) - 0.0767945).abs() < 1e-6);

		// A priority runner's short attack is half again as wide.
		ai.longattack = false;
		assert!((ai.shot_angle(&SCOPE_PARAMETER_0, true) - 0.2356194).abs() < 1e-6);

		// The capture test is the two angles added, against the tolerance.
		let eight = degree_to_radian(8.0);
		assert!(AIcontrol::captured(degree_to_radian(7.9), 0.0, eight));
		assert!(!AIcontrol::captured(degree_to_radian(8.1), 0.0, eight));
		assert!(AIcontrol::captured(degree_to_radian(4.0), degree_to_radian(3.9), eight));
		assert!(!AIcontrol::captured(degree_to_radian(4.0), degree_to_radian(4.1), eight));
	}

	// --- the reaction delay ----------------------------------------------

	/// Caution is the reaction delay: a hundred and sixty frames, one a frame, with a
	/// chance of an early end under a hundred, and then back to normal.
	#[test]
	fn caution_counts_down_to_zero_and_returns_to_normal() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		let (blocks, coll_d) = empty_world();
		let points = PointDataInterface::new();
		let mut sound = sound();
		let d3dg = headless();
		let resource = ResourceManager::new();
		let config = Config::new();

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &points);
		ai.set_caution_mode();

		assert_eq!(ai.battlemode, AiMode::AiCaution);
		assert_eq!(ai.cautioncnt, 160);

		// Down to a hundred there is no early end, so it is exactly one a frame.
		for _ in 0..60 {
			frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		}
		assert_eq!(ai.cautioncnt, 100);
		assert_eq!(ai.battlemode, AiMode::AiCaution);

		// Under a hundred it either drops one or ends at once, and it never rises. The
		// count only moves down, so it reaches zero within a hundred more frames.
		let mut previous = ai.cautioncnt;
		let mut frames = 0;
		while ai.battlemode == AiMode::AiCaution && frames < 400 {
			frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
			assert!(
				ai.cautioncnt <= previous,
				"caution rose from {previous} to {}",
				ai.cautioncnt
			);
			previous = ai.cautioncnt;
			frames += 1;
		}

		assert!(frames <= 101, "the delay ran for {frames} frames");
		assert_eq!(ai.cautioncnt, 0);
		assert_eq!(ai.battlemode, AiMode::AiNormal);
		assert!(!ai.face_caution_flag);
	}

	// --- the path --------------------------------------------------------

	/// A path of movement points, a wait, a stop and a random fork, which is what the AI
	/// walks in normal mode.
	fn path_points() -> PointDataInterface {
		// p1: 3 is an AI path, 8 a random AI path, 4 a human info point.
		// p2: 0 walk, 1 run, 2 wait, 3 tracking, 5 stop for five seconds, 7 priority run.
		// p3: the identifier of the next point. p4: this point's own identifier.
		load_points(
			"path.pd1",
			&[
				(0.0, 0.0, 0.0, 0.0, 4, 0, 1, 0),
				(10.0, 0.0, 0.0, 1.0, 3, 0, 2, 1),
				(20.0, 0.0, 0.0, 2.0, 3, 1, 3, 2),
				(30.0, 0.0, 0.0, 3.0, 3, 2, 4, 3),
				(40.0, 0.0, 0.0, 4.0, 3, 5, 5, 4),
				(50.0, 0.0, 0.0, 5.0, 3, 7, 6, 5),
				(0.0, 0.0, 0.0, 0.0, 8, 7, 8, 6),
				(70.0, 0.0, 0.0, 7.0, 3, 0, 7, 7),
				(80.0, 0.0, 0.0, 8.0, 3, 0, 8, 8),
				(0.0, 0.0, 0.0, 0.0, 3, 3, 10, 9),
				(0.0, 0.0, 0.0, 0.0, 4, 0, 0, 10),
			],
		)
	}

	/// The path starts at the point the person's own point names and walks on through the
	/// points it names, in the mode each of them carries.
	#[test]
	fn a_path_walks_through_its_points() {
		let mut mgr = ObjectManager::new();
		// The person's own point is 0, whose p3 names the first movement point.
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		let points = path_points();

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &points);

		// The person's own point is a human info point, so the path moved on to point 1.
		assert_eq!(ai.move_navi.path_pointid, 1);
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (10.0, 0.0, 1.0));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveWalk);
		assert_eq!(target.pointmode, AiMoveNaviFlag::AiNaviPointNull);
		assert_eq!(ai.move_navi.get_move_mode(), AiMode::AiWalk);

		// Point 2 runs, and its own angle is 2.
		ai.move_navi.move_path_next_state(&points);
		assert!(ai.move_navi.move_path_now_state(&mgr, &points));
		assert_eq!(ai.move_navi.path_pointid, 2);
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (20.0, 0.0, 2.0));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveRun);

		// Point 3 waits, which is a walk mode and a wait point mode.
		ai.move_navi.move_path_next_state(&points);
		ai.move_navi.move_path_now_state(&mgr, &points);
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (30.0, 0.0, 3.0));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveWalk);
		assert_eq!(target.pointmode, AiMoveNaviFlag::AiNaviPointWait);
		assert_eq!(ai.get_path_point_data(&points).unwrap().p2, 2);

		// Point 4 stops for five seconds, and point 5 is priority running.
		ai.move_navi.move_path_next_state(&points);
		ai.move_navi.move_path_now_state(&mgr, &points);
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (40.0, 0.0, 4.0));
		assert_eq!(target.pointmode, AiMoveNaviFlag::AiNaviPointStop5Sec);

		ai.move_navi.move_path_next_state(&points);
		ai.move_navi.move_path_now_state(&mgr, &points);
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (50.0, 0.0, 5.0));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveRun2);
		assert!(ai.move_navi.get_run2());

		// Point 6 is a random fork, which draws one number and lands on one of the two
		// points it names.
		ai.move_navi.move_path_next_state(&points);
		assert!(!ai.move_navi.move_path_now_state(&mgr, &points));
		assert_eq!(ai.move_navi.get_move_mode(), AiMode::AiRandom);

		let mut saw_left = false;
		let mut saw_right = false;
		for _ in 0..32 {
			ai.move_navi.path_pointid = 6;
			assert!(ai.move_navi.move_path_next_state(&points));
			ai.move_navi.move_path_now_state(&mgr, &points);

			match ai.move_navi.get_target_pos().posx {
				70.0 => saw_left = true,
				80.0 => saw_right = true,
				other => panic!("the fork landed on {other}"),
			}
		}
		assert!(saw_left && saw_right, "both branches of the fork were taken");
	}

	/// A tracking point follows the person it names, refreshing every frame.
	#[test]
	fn a_tracking_point_follows_its_person() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		// The person to track carries the point identifier the info point names.
		place_human(&mut mgr, 2, 5.0, 0.0, 5.0, 2, 0.0, 0.0, 10);
		let points = path_points();

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &points);
		ai.move_navi.path_pointid = 9;
		ai.move_navi.target_humanid = -1;

		assert!(ai.move_navi.move_path_now_state(&mgr, &points));
		assert_eq!(ai.move_navi.get_move_mode(), AiMode::AiTracking);
		assert_eq!(ai.move_navi.get_target_human_id(), 2);

		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz), (5.0, 5.0));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveTracking);
		assert_eq!(target.pointmode, AiMoveNaviFlag::AiNaviPointTracking);

		// The person walks, and the next frame picks the new position up.
		mgr.get_human_object_mut(2).unwrap().set_pos_data(7.0, 0.0, 9.0, 0.25);
		assert!(ai.move_navi.move_path_now_state(&mgr, &points));
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (7.0, 9.0, 0.25));

		// A held wait ignores the path and stays put.
		ai.set_hold_wait(3.0, 4.0, 0.5);
		assert!(ai.move_navi.move_path_now_state(&mgr, &points));
		let target = ai.move_navi.get_target_pos();
		assert_eq!((target.posx, target.posz, target.rx), (3.0, 4.0, 0.5));
		assert_eq!(target.movemode, AiMoveNaviFlag::AiNaviMoveWalk);
		assert_eq!(target.pointmode, AiMoveNaviFlag::AiNaviPointWait);
	}

	// --- the states ------------------------------------------------------

	/// Each battle state decides one thing on one input, and the decision is what the
	/// mode and its counters say.
	#[test]
	fn each_battle_state_decides_on_its_input() {
		let mut mgr = ObjectManager::new();
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		place_human(&mut mgr, 1, 10.0, 0.0, 10.0, 2, 0.0, 0.0, 1);
		let (blocks, coll_d) = empty_world();
		let points = PointDataInterface::new();
		let mut sound = sound();
		let d3dg = headless();
		let resource = ResourceManager::new();
		let config = Config::new();

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &points);
		assert_eq!(ai.battlemode, AiMode::AiNormal);

		// A person with no health is dead, and reports it by number and by name.
		mgr.get_human_object_mut(0)
			.unwrap()
			.set_param_data(-1, 0, 0, 1, true);
		assert!(mgr.get_human_object(0).unwrap().hp() <= 0);
		frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		assert_eq!(ai.battlemode, AiMode::AiDead);
		assert_eq!(ai.get_battle_mode(), (0, &b"AI_DEAD"[..]));

		// A person marked dead whose health is back is revived into normal.
		mgr.get_human_object_mut(0)
			.unwrap()
			.set_param_data(0, 0, 0, 1, true);
		assert!(mgr.get_human_object(0).unwrap().hp() > 0);
		frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		assert_eq!(ai.battlemode, AiMode::AiNormal);

		// Being hit in normal mode starts caution, and points the AI at the shot.
		mgr.get_human_object_mut(0).unwrap().set_hit_flag(1.25);
		frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		assert_eq!(ai.battlemode, AiMode::AiCaution);
		assert_eq!(ai.cautioncnt, 160);
		assert!(ai.face_caution_flag);
		assert!((ai.face_caution_rx - 1.25).abs() < 1e-6);
		assert_eq!(ai.get_battle_mode(), (2, &b"AI_CAUTION"[..]));

		// A target already in hand turns caution into an attack, with the attack count
		// restarted.
		ai.enemy_human_id = Some(1);
		ai.actioncnt = 999;
		frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		assert_eq!(ai.battlemode, AiMode::AiAction);
		assert_eq!(ai.actioncnt, 0);
		assert_eq!(ai.get_enemy_human_id(), 1);

		// The non-combat switch ends an attack at once, and an attack that ends without
		// priority running falls back to caution with a fresh count.
		ai.set_no_fight_flag(true);
		frame(&mut ai, &mut mgr, &blocks, &points, &coll_d, &mut sound, &d3dg, &resource, &config);
		assert_eq!(ai.battlemode, AiMode::AiCaution);
		assert_eq!(ai.cautioncnt, 160);
		assert_eq!(ai.get_enemy_human_id(), -1);
		assert!(!ai.face_caution_flag);

		// The forced caution does nothing while priority running.
		ai.move_navi.movemode = AiMode::AiRun2;
		ai.battlemode = AiMode::AiNormal;
		ai.set_caution_mode();
		assert_eq!(ai.battlemode, AiMode::AiNormal);
		ai.move_navi.movemode = AiMode::AiNull;
		ai.set_caution_mode();
		assert_eq!(ai.battlemode, AiMode::AiCaution);
		assert_eq!(ai.cautioncnt, 160);
	}

	/// The attack itself decides which way to turn and which way to hold the arm, from the
	/// angle to the target and the weapons on both sides.
	#[test]
	fn the_attack_turns_toward_its_target() {
		let mut mgr = ObjectManager::new();
		// Facing +Z, with the target to the right of that.
		place_human(&mut mgr, 0, 0.0, 0.0, 0.0, 1, 0.0, 0.0, 0);
		place_human(&mut mgr, 1, 10.0, 0.0, 10.0, 2, 0.0, 0.0, 1);
		let (blocks, coll_d) = empty_world();
		let mut sound = sound();
		let resource = ResourceManager::new();
		let config = Config::new();

		let mut ai = AIcontrol::new(0);
		ai.init(&mgr, &PointDataInterface::new());
		ai.update_pos(&mgr);
		ai.enemy_human_id = Some(1);

		ai.action(&mut mgr, &blocks, &coll_d, &mut sound, &resource, &config);

		// The angle to the target is -45 degrees, which is to the right, and the target is
		// at 14.1 units, which is a short attack with no weapon on either side.
		assert!(ai.obj_driver.get_mode_flag(AI_CTRL_TURNRIGHT));
		assert!(!ai.obj_driver.get_mode_flag(AI_CTRL_TURNLEFT));
		assert!(ai.obj_driver.get_mode_flag(AI_CTRL_TURNDOWN));
		assert!(!ai.obj_driver.get_mode_flag(AI_CTRL_TURNUP));
		assert!(!ai.longattack);
		assert_eq!(ai.actioncnt, 1);
	}
}
