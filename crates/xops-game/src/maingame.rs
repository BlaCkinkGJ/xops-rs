//! The main game screen, from the upstream `gamemain.cpp`'s `maingame` class.
//!
//! This is the FPS itself: the camera modes, the player input, the HUD, the radar, the
//! debug console with its command table, the event lines and the frame ordering that runs
//! the object manager, the AI and the world per frame.
//!
//! The class holds its own copy of the mission result (`main_game_info`), which is the
//! upstream `MainGameInfo`. That is a working copy of the shared `World::game_info`: it is
//! taken at `Create`, updated as the mission runs, and written back during `Process`,
//! which is where the result screen reads it from afterwards. The two are not kept in
//! step, so the copy back is what carries the result.
//!
//! The three event lines are a file scope `EventControl Event[TOTAL_EVENTLINE]` upstream,
//! read only from here, so they are a field. `ENABLE_DEBUGCONSOLE` is on, so the console,
//! its command table, its input line and its panel are live and ported in full rather than
//! behind a switch.
//!
//! Upstream `maingame` reaches three file scope classes that this port does not have as
//! fields: the sound manager (whose asset loads it drives through the resource class), the
//! window (which the console's display mode switch needs) and the input and state machine
//! (which the console's reset command needs). Each is a parameter of the phase that uses
//! it, which is the port's rule for globals.

use xops_platform::input::Input;

use crate::collision::get_2d_line_in_box;
use crate::config::{Config, ControlKey, MAX_PLAYERNAME, TOTAL_ControlKey};
use crate::core::{
	angle_normalization, degree_to_radian, GAMEFPS, GAMENAME, GAMEVERSION, MAX_PATH,
	TOTAL_HAVEWEAPON,
};
use crate::d3dgraphics::{D3DGraphics, MAX_TEXTURE};
use crate::datafile::{
	blockdataface, get_file_directory, MAX_ADDSMALLOBJECT, MAX_BLOCKS, MAX_POINTS,
	MAX_POINTMESSAGEBYTE, POINT_P1TYPE_AIPATH, POINT_P1TYPE_HUMAN, POINT_P1TYPE_HUMAN2,
	POINT_P1TYPE_HUMANINFO, POINT_P1TYPE_RAND_AIPATH, POINT_P1TYPE_RAND_WEAPON,
	POINT_P1TYPE_SMALLOBJ, POINT_P1TYPE_WEAPON, TOTAL_BLOCKTEXTURE,
};
use crate::debug::DebugLog;
use crate::event::{EventParameter, DISTANCE_CHECKPOINT};
use crate::gamemain::{
	huda_weapon_posx, huda_weapon_posy, ConsoleData, GameInfo, World, CONSOLE_PROMPT,
	HUDA_WEAPON_SIZEH, HUDA_WEAPON_SIZEW, INPUT_ARROWKEYS_ANGLE, INPUT_F1NUMKEYS_ANGLE,
	MAX_CONSOLELEN, MAX_CONSOLELINES, TOTAL_EVENTENT_SHOWMESSEC, TOTAL_EVENTENTRYPOINT_0,
	TOTAL_EVENTENTRYPOINT_1, TOTAL_EVENTENTRYPOINT_2, TOTAL_EVENTLINE, VIEW_DIST,
	VIEW_F1MODE_ANGLE, VIEW_F1MODE_DIST, VIEW_FREECAMERA_SCALE, VIEW_HEIGHT,
};
use crate::gui_object::TextBoxObject;
use crate::input::{
	get_esc_keycode, get_function_keycode, get_home_keycode, originalkeycode_to_dinputdef,
	InputControl,
};
use crate::model::MAX_MODEL;
use crate::object::{HUMAN_HEIGHT, HUMAN_MAPCOLLISION_SLOPEANGLE};
use crate::objectmanager::{
	MAX_BULLET, MAX_EFFECT, MAX_GRENADE, MAX_HUMAN, MAX_SMALLOBJECT, MAX_WEAPON,
};
use crate::parameter::{
	ScopeParameter, WeaponParameter, OFFICIALMISSION_BD1, SCOPE_PARAMETER_0,
	TOTAL_PARAMETERINFO_AILEVEL, TOTAL_PARAMETERINFO_SMALLOBJECT, TOTAL_PARAMETERINFO_WEAPON,
	WEAPON_PARAMETER,
};
use crate::resource::MAX_LOADHUMANTEXTURE;
use crate::scene::D3Dscene;
use crate::sound::MAX_LOADSOUND;
use crate::sound::SoundControl;
use crate::soundmanager::{SoundManager, MAX_SOUNDMGR_LIST};
use crate::statemachine::StateMachine;
use crate::window::{get_fps, get_time_ms, get_time_name, WindowControl};

/// `GRAPHICS_CORE`, the graphics line of the version display. This port replaces
/// `d3dgraphics-opengl.cpp`, which is the branch the licence line count and the field of
/// view were taken from, so it carries that branch's string.
const GRAPHICS_CORE: &str = "OpenGL 1.1";
/// `SOUND_CORE`.
const SOUND_CORE: &str = "ezds";
/// `INPUT_CORE`. The port reads the platform's key table the way the Raw Input back end
/// does, which is the interface upstream selects too.
const INPUT_CORE: &str = "RawInput";

/// The point data extension the two mission file loaders append.
const PD1_EXT: &[u8] = b".pd1";

/// One cell of a mode A HUD panel. Both the status boxes and the weapon panel are drawn
/// from 32 pixel tiles, which is why their edges meet.
const HUDA_CELL: i32 = 32;

/// The lower left status box's top edge, the upstream's half-width katakana string.
const HUDA_LEFT_EDGE: &[u8] = &[0xB3, 0xB4, 0xB4, 0xB4, 0xB4, 0xB4, 0xB4, 0xB5];
/// The lower right status box's top edge, which is a different string: its middle glyphs
/// step up so that the box closes against the weapon panel.
const HUDA_RIGHT_EDGE: &[u8] = &[0xB3, 0xB4, 0xB4, 0xB6, 0xB7, 0xB7, 0xB7, 0xB8, 0xB9];
/// The weapon panel's edge: this glyph, a run of [`HUDA_WEAPON_EDGE_MIDDLE`], then the
/// closing one.
const HUDA_WEAPON_EDGE_FIRST: u8 = 0xB0;
const HUDA_WEAPON_EDGE_MIDDLE: u8 = 0xB1;
const HUDA_WEAPON_EDGE_LAST: u8 = 0xB2;

/// The length the upstream `strlen` would see: everything before the first NUL.
fn c_str_len(bytes: &[u8]) -> usize {
	bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len())
}

/// `strcpy` into a fixed C buffer. Upstream overruns the buffer here, so the copy stops at
/// its last byte and the rest is cleared, which is the defined reading of the same call.
fn write_cstr(out: &mut [u8], text: &[u8]) {
	let text = &text[..c_str_len(text)];
	let count = text.len().min(out.len().saturating_sub(1));
	out[..count].copy_from_slice(&text[..count]);
	for byte in &mut out[count..] {
		*byte = 0;
	}
}

/// The bytes of the parts joined, which is the upstream's `strcpy`/`strcat` chain into a
/// `MAX_PATH` buffer.
fn joined(parts: &[&[u8]]) -> Vec<u8> {
	let mut out = Vec::new();
	for part in parts {
		out.extend_from_slice(&part[..c_str_len(part)]);
	}
	out
}

/// The first `max` bytes of a C string, which is the upstream's `str[n] = '\0'` before a
/// `strcat`.
fn clipped(text: &[u8], max: usize) -> &[u8] {
	&text[..c_str_len(text).min(max)]
}

/// A line the console shows, built from the parts the upstream `sprintf`s together.
fn console_line(parts: &[&[u8]]) -> Vec<u8> {
	let mut out = joined(parts);
	out.truncate(MAX_CONSOLELEN);
	out
}

/// The `+0x10` the upstream adds to a half-width katakana string to move its glyphs down
/// one row of the texture font: the other edge of the same box.
fn huda_next_row(edge: &mut [u8]) {
	for byte in edge.iter_mut() {
		*byte += 0x10;
	}
}

/// The weapon panel's edge, one cell per [`HUDA_WEAPON_SIZEW`].
fn huda_weapon_edge() -> Vec<u8> {
	let mut edge = vec![HUDA_WEAPON_EDGE_MIDDLE; HUDA_WEAPON_SIZEW as usize];
	edge[0] = HUDA_WEAPON_EDGE_FIRST;
	edge[HUDA_WEAPON_SIZEW as usize - 1] = HUDA_WEAPON_EDGE_LAST;
	edge
}

/// Where the mode A HUD draws its four panels, from the screen size.
///
/// Upstream writes this arithmetic out at each draw, so the panels only line up because
/// they all count in the same 32 pixel cell. Kept in one place so the geometry is one
/// number each and can be checked.
struct HudaLayout {
	/// The lower left status box and the lower right one, at their top left cells.
	left_x: i32,
	left_y: i32,
	right_x: i32,
	right_y: i32,
	/// The weapon panel's top left cell.
	weapon_x: i32,
	weapon_y: i32,
	/// The ammo readout, which sits inside the lower left box.
	ammo_x: i32,
	ammo_y: i32,
	/// The state readout, on the lower right box's bottom row.
	state_x: i32,
	state_y: i32,
}

impl HudaLayout {
	fn new(config: &Config) -> HudaLayout {
		let height = config.get_screen_height();
		HudaLayout {
			left_x: 15,
			left_y: height - 105,
			right_x: 15,
			right_y: height - 55,
			weapon_x: huda_weapon_posx(config),
			weapon_y: huda_weapon_posy(config),
			ammo_x: 25,
			ammo_y: height - 96,
			state_x: 23,
			state_y: height - 45,
		}
	}
}

/// The main game screen.
pub struct MainGame<'a> {
	pub base: D3Dscene,
	/// The mission, shared with whichever screen is on screen.
	pub world: Box<World<'a>>,

	// -- the map and the atmosphere
	/// `SkyNumber`: which sky the mission set, which also picks the fog colour.
	pub sky_number: i32,
	/// Darken the whole screen, which is a mission setting the debug console toggles.
	pub dark_screen_flag: bool,
	pub fog_flag: bool,

	// -- the camera
	/// The angle the mouse has asked for.
	pub mouse_rx: f32,
	pub mouse_ry: f32,
	/// How far the camera has to catch up to it.
	pub view_rx: f32,
	pub view_ry: f32,
	/// Per frame camera turn from a key, added to the mouse.
	pub add_camera_rx: f32,
	pub add_camera_ry: f32,

	// -- the debug view switches
	pub show_info_debugmode: bool,
	pub camera_debugmode: bool,
	/// Draw the object tags.
	pub tag: bool,
	pub radar: bool,
	pub wireframe: bool,
	/// Do not draw models, which is how the map is inspected on its own.
	pub nomodel: bool,
	pub center_line: bool,
	pub camera_blind: bool,
	pub camera_f1mode: bool,
	/// F2 cycles the camera through its modes.
	pub camera_f2mode: i32,
	pub camera_home_mode: bool,
	/// The F5 cheat.
	pub cmd_f5: bool,
	pub invincible_id: i32,
	/// Hand the player's body to the AI.
	pub player_ai: bool,
	pub ai_stop: bool,
	pub ai_no_fight: bool,
	pub ai_debuginfo_id: i32,

	// -- the frame
	pub start_framecnt: i32,
	pub end_framecnt: i32,
	/// How many simulation steps run per drawn frame.
	pub game_speed: i32,

	// -- events
	pub event_stop: bool,
	pub event_skip: [bool; TOTAL_EVENTLINE],
	pub message_id: i32,
	pub message_cnt: i32,
	pub redflash_flag: bool,

	// -- where the frame's time went, which the debug overlay shows
	pub time: i32,
	pub time_input: i32,
	pub time_process_object: i32,
	pub time_process_ai: i32,
	pub time_process_event: i32,
	pub time_sound: i32,
	pub time_render: i32,

	/// The mission result as it stands. See this module's header for how it relates to the
	/// shared one.
	pub main_game_info: GameInfo,

	// -- the debug console, live because ENABLE_DEBUGCONSOLE is true
	pub show_console: bool,
	/// The lines already printed, oldest first.
	pub info_console_data: [ConsoleData; MAX_CONSOLELINES],
	/// The line being typed.
	pub console_text_box_control: TextBoxObject,
	/// The command as typed, unprocessed.
	pub new_command: [u8; MAX_CONSOLELEN],
	pub screen_shot: i32,
}

impl<'a> MainGame<'a> {
	/// Constructor. Upstream sets the debug view switches to their off state and leaves the
	/// rest to `Create`.
	///
	/// Two of them are not off: the fog and the blindfold both start on, which is the state
	/// the game starts in. `Camera_HOMEmode` comes from the setting tables upstream, which
	/// the constructor cannot reach here, so `Create` reads it, which is before the first
	/// frame either way.
	/// Hands the world to the screen that comes next. The screens own the world rather than
	/// borrowing it: a screen lives for many frames, and a struct that holds a borrow of the
	/// world cannot be stored beside the world itself, which is exactly what a scheduler
	/// does. Owning it costs one move per screen change.
	pub fn into_world(self) -> Box<World<'a>> {
		self.world
	}

	pub fn new(world: Box<World<'a>>) -> MainGame<'a> {
		MainGame {
			base: D3Dscene::new(),
			world,
			sky_number: 0,
			dark_screen_flag: false,
			fog_flag: true,
			mouse_rx: 0.0,
			mouse_ry: 0.0,
			view_rx: 0.0,
			view_ry: 0.0,
			add_camera_rx: 0.0,
			add_camera_ry: 0.0,
			show_info_debugmode: false,
			camera_debugmode: false,
			tag: false,
			radar: false,
			wireframe: false,
			nomodel: false,
			center_line: false,
			camera_blind: true,
			camera_f1mode: false,
			camera_f2mode: 0,
			camera_home_mode: false,
			cmd_f5: false,
			invincible_id: -1,
			player_ai: false,
			ai_stop: false,
			ai_no_fight: false,
			ai_debuginfo_id: -1,
			start_framecnt: 0,
			end_framecnt: 0,
			// One step per frame unless the console changes it.
			game_speed: 1,
			event_stop: false,
			event_skip: [false; TOTAL_EVENTLINE],
			message_id: -1,
			message_cnt: 0,
			redflash_flag: false,
			time: 0,
			time_input: 0,
			time_process_object: 0,
			time_process_ai: 0,
			time_process_event: 0,
			time_sound: 0,
			time_render: 0,
			main_game_info: GameInfo::default(),
			show_console: false,
			info_console_data: [ConsoleData::default(); MAX_CONSOLELINES],
			console_text_box_control: TextBoxObject::default(),
			new_command: [0; MAX_CONSOLELEN],
			screen_shot: 0,
		}
	}

	/// `SetShowInfoFlag`: the debug overlay, which the console toggles.
	pub fn set_show_info_flag(&mut self, flag: bool) {
		self.show_info_debugmode = flag;
	}

	/// `GetGameSpeed`.
	pub fn get_game_speed(&self) -> i32 {
		self.game_speed
	}

	// -- the setting tables ------------------------------------------------

	/// `GameParamInfo.GetScopeParam`, with a negative id answered by the first entry.
	/// Upstream reads whatever the caller's record held there, which is uninitialised
	/// stack, and the only ids that reach this come from a human's scope mode.
	fn scope_param(&self, id: i32) -> ScopeParameter {
		self.world.param.get_scope_param(id).unwrap_or(SCOPE_PARAMETER_0)
	}

	/// `GameParamInfo.GetWeapon`, with an id the weapon table refuses answered by the first
	/// entry, which is what `ID_WEAPON_NONE` names.
	fn weapon_param(&self, id: i32) -> WeaponParameter {
		self.world
			.param
			.get_weapon(id)
			.unwrap_or(WEAPON_PARAMETER[0])
	}

	/// The official mission's data directory and point file name.
	///
	/// An id outside the table answers with two empty slices. Upstream writes neither and
	/// leaves the caller's `MAX_PATH` buffers as they were, which is uninitialised stack,
	/// and the empty answer turns that into a failed file read.
	fn official_paths(&self, id: i32) -> (&'static [u8], &'static [u8]) {
		match self.world.param.get_official_mission(id) {
			Some(mission) => (mission.directory, mission.txt),
			None => (&[], &[]),
		}
	}

	/// The mission's data directory, which the map load and the console's dark screen
	/// command both need. A standard mission reads it from the parameter table and an addon
	/// from the directory its `.bd1` sits in.
	fn mission_directory(&self, selectmission_id: i32) -> Vec<u8> {
		let filetype = self
			.world
			.mif
			.as_ref()
			.map(|m| m.get_filetype())
			.unwrap_or(false);

		if filetype {
			let bd1 = self
				.world
				.mif
				.as_ref()
				.map(|m| m.get_datafile_path().0.to_vec())
				.unwrap_or_default();
			let mut directory = [0u8; MAX_PATH];
			get_file_directory(&bd1, &mut directory);
			directory.to_vec()
		} else {
			let (directory, _) = self.official_paths(selectmission_id);
			joined(&[directory])
		}
	}

	/// The field of view the camera is drawn with.
	///
	/// Upstream this is the `if` at the top of `Render3D`: the third person camera and the
	/// free camera both take the scope table's no-scope entry, and only the player view
	/// takes the current scope's.
	fn view_angle(&self, scopemode: i32) -> f32 {
		if !self.camera_f1mode && !self.camera_debugmode {
			self.scope_param(scopemode).view_angle
		} else {
			self.scope_param(0).view_angle
		}
	}

	// -- the mission -------------------------------------------------------

	/// The upstream `maingame::Create`.
	///
	/// Loads the mission the menu selected into the world, hands the map to the renderer
	/// and starts the three event lines. The return code is the two file read results, point
	/// data in the high bit, which is what the frame host branches on.
	#[allow(clippy::too_many_arguments)]
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		window: &WindowControl,
		config: &Config,
		input_ctrl: &InputControl,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundManager,
		sound_ctrl: &mut SoundControl,
	) -> i32 {
		self.main_game_info = self.world.game_info;

		let selectmission_id = self.main_game_info.selectmission_id;
		let filetype = self
			.world
			.mif
			.as_ref()
			.map(|m| m.get_filetype())
			.unwrap_or(false);

		// Find the file paths of `.bd1` and `.pd1`.
		let (bdata, pdata, directory, collisionflag);
		if !filetype {
			let (dir, txt) = self.official_paths(selectmission_id);
			let mission = self.world.param.get_official_mission(selectmission_id);
			collisionflag = mission.map(|m| m.collision).unwrap_or(false);
			self.dark_screen_flag = mission.map(|m| m.screen).unwrap_or(false);
			directory = joined(&[dir]);
			bdata = joined(&[dir, OFFICIALMISSION_BD1]);
			pdata = joined(&[dir, txt, PD1_EXT]);
		} else {
			let (bd1, pd1) = self
				.world
				.mif
				.as_ref()
				.map(|m| {
					let (bd1, pd1) = m.get_datafile_path();
					(bd1.to_vec(), pd1.to_vec())
				})
				.unwrap_or_default();
			collisionflag = self
				.world
				.mif
				.as_ref()
				.map(|m| m.get_collision_flag())
				.unwrap_or(false);
			self.dark_screen_flag = self
				.world
				.mif
				.as_ref()
				.map(|m| m.get_screen_flag())
				.unwrap_or(false);
			let mut dir = [0u8; MAX_PATH];
			get_file_directory(&bd1, &mut dir);
			directory = dir.to_vec();
			bdata = bd1;
			pdata = pd1;
		}

		{
			let resource = &mut self.world.resource;

			// Load the additional items. The sound is loaded through the resource class,
			// which needs the control rather than the manager.
			for i in 0..MAX_ADDSMALLOBJECT as i32 {
				let model = self
					.world
					.mif
					.as_ref()
					.and_then(|m| m.get_add_smallobject_model_path(i))
					.map(|p| p.to_vec())
					.unwrap_or_default();
				let texture = self
					.world
					.mif
					.as_ref()
					.and_then(|m| m.get_add_smallobject_texture_path(i))
					.map(|p| p.to_vec())
					.unwrap_or_default();
				let soundpath = self
					.world
					.mif
					.as_ref()
					.and_then(|m| m.get_add_smallobject_sound_path(i))
					.map(|p| p.to_vec())
					.unwrap_or_default();

				resource.load_add_small_object(
					d3dg,
					sound_ctrl,
					i,
					&model,
					&texture,
					&soundpath,
				);
			}

			// Load the block data and the point data. Point data is the high bit.
			let blockflag = self.world.block_data.load_file_data(&bdata);
			let pointflag = self.world.point_data.load_file_data(&pdata);
			if blockflag != 0 || pointflag != 0 {
				return pointflag * 2 + blockflag;
			}

			// Initialize the block data.
			self.world
				.block_data
				.calculation_blockdata(self.dark_screen_flag);
			d3dg.load_mapdata(self.world.block_data.clone(), &directory);
			self.world
				.collision
				.init_collision(Some(&self.world.block_data));

			// Initialize the point data.
			self.world.objects.load_point_data(
				d3dg,
				resource,
				&self.world.point_data,
				&self.world.collision,
				&self.world.block_data,
				self.world.mif.as_ref(),
			);
		}

		// The additional collision detection setup.
		self.world.objects.set_add_collision_flag(collisionflag);

		// The AI setup. `SetClass` upstream also points each AI at the object manager, the
		// map and the setting tables, which are parameters here.
		for i in 0..MAX_HUMAN {
			self.world.ai[i].set_class(i as i32);
			self.world.ai[i].init(&self.world.objects, &self.world.point_data);
		}

		// Load the background sky.
		self.sky_number = self
			.world
			.mif
			.as_ref()
			.map(|m| m.get_skynumber())
			.unwrap_or(0);
		self.world
			.resource
			.load_sky_model_texture(d3dg, self.sky_number);

		// Sound initialization, which forgets everything queued.
		sound.init_world_sound();

		// Event initialization. Upstream also points each line at the point data and the
		// object manager through `SetClass`, which are parameters of the process call here.
		self.world.events[0].reset(TOTAL_EVENTENTRYPOINT_0 as i16);
		self.world.events[1].reset(TOTAL_EVENTENTRYPOINT_1 as i16);
		self.world.events[2].reset(TOTAL_EVENTENTRYPOINT_2 as i16);

		// Get the player orientation.
		self.refresh_player_orientation();

		self.fog_flag = true;
		self.view_rx = 0.0;
		self.view_ry = 0.0;
		self.add_camera_rx = 0.0;
		self.add_camera_ry = 0.0;
		self.camera_debugmode = false;
		self.tag = false;
		self.radar = false;
		self.wireframe = false;
		self.nomodel = false;
		self.center_line = false;
		self.camera_blind = true;
		self.camera_f1mode = false;
		// `Camera_HOMEmode = GameConfig.GetShowArmFlag()`, read here because the
		// constructor has no config.
		self.camera_home_mode = config.get_show_arm_flag();
		self.invincible_id = -1;
		self.player_ai = false;
		self.ai_stop = false;
		self.ai_no_fight = false;
		self.ai_debuginfo_id = -1;
		self.start_framecnt = 0;
		self.end_framecnt = 0;
		self.event_stop = false;
		for i in 0..TOTAL_EVENTLINE {
			self.event_skip[i] = false;
		}
		self.game_speed = 1;
		self.message_id = -1;
		self.message_cnt = 0;
		self.redflash_flag = false;
		self.main_game_info.missioncomplete = false;
		self.main_game_info.fire = 0;
		self.main_game_info.ontarget = 0.0;
		self.main_game_info.kill = 0;
		self.main_game_info.headshot = 0;

		self.show_console = false;
		self.screen_shot = 0;

		// Console initialization.
		let white = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
		for i in 0..MAX_CONSOLELINES {
			self.info_console_data[i].color = white;
			self.info_console_data[i].textdata[0] = 0;
		}
		self.add_info_console(white, b"Game Debug Console.");
		self.add_info_console(white, b"        Command list >help");
		self.console_text_box_control.init_text_box(
			b"",
			(MAX_CONSOLELEN - CONSOLE_PROMPT.len()) as i32,
			0x01 | 0x04 | 0x20,
		);

		// `inputCtrl->MoveMouseCenter(); framecnt = 0; GameState->NextState();`.
		self.base
			.scene
			.create(state, window, config, input_ctrl, log);

		0
	}

	/// The upstream `maingame::Recovery`, called after the renderer was reset.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics, sound_ctrl: &mut SoundControl) -> i32 {
		let directory = self.mission_directory(self.main_game_info.selectmission_id);
		let resource = &mut self.world.resource;

		// Release and load the additional items.
		for i in 0..MAX_ADDSMALLOBJECT as i32 {
			resource.cleanup_add_small_object(d3dg, sound_ctrl, i);
		}
		for i in 0..MAX_ADDSMALLOBJECT as i32 {
			let model = self
				.world
				.mif
				.as_ref()
				.and_then(|m| m.get_add_smallobject_model_path(i))
				.map(|p| p.to_vec())
				.unwrap_or_default();
			let texture = self
				.world
				.mif
				.as_ref()
				.and_then(|m| m.get_add_smallobject_texture_path(i))
				.map(|p| p.to_vec())
				.unwrap_or_default();
			let soundpath = self
				.world
				.mif
				.as_ref()
				.and_then(|m| m.get_add_smallobject_sound_path(i))
				.map(|p| p.to_vec())
				.unwrap_or_default();

			resource.load_add_small_object(d3dg, sound_ctrl, i, &model, &texture, &soundpath);
		}

		// Initialize the block data, the point data and the background sky.
		d3dg.load_mapdata(self.world.block_data.clone(), &directory);
		self.world.objects.recovery(d3dg, resource);

		resource.cleanup_sky_model_texture(d3dg);
		resource.load_sky_model_texture(d3dg, self.sky_number);

		0
	}

	/// The upstream `Destroy`. The sound control is here for the same reason `Recovery`
	/// takes it: releasing an additional small object's sound needs the sound class.
	pub fn destroy(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		sound_ctrl: &mut SoundControl,
	) {
		let resource = &mut self.world.resource;

		// Release the block data and the object manager.
		d3dg.cleanup_mapdata();
		self.world.objects.cleanup(d3dg, resource);

		// Release the additional items and the background sky.
		for i in 0..MAX_ADDSMALLOBJECT as i32 {
			resource.cleanup_add_small_object(d3dg, sound_ctrl, i);
		}
		resource.cleanup_sky_model_texture(d3dg);

		self.console_text_box_control.destroy_text_box();

		state.next_state(log);
	}

	// -- the input ---------------------------------------------------------

	/// `CheckInputControl`: whether the key bound to `check_key` is in the asked-for state.
	///
	/// `mode` is 0 for "down now", 1 for "went down this frame" and 2 for "came up this
	/// frame", which is the order of the upstream's three return statements.
	///
	/// The upstream also answers for the split shift and control keycodes, and those two
	/// branches are not here for the same reason [`InputControl::check_input_all`] has
	/// none: [`originalkeycode_to_dinputdef`] never returns their marker values, so no
	/// binding can reach them.
	pub fn check_input_control(
		&self,
		input_ctrl: &InputControl,
		config: &Config,
		check_key: ControlKey,
		mode: i32,
	) -> bool {
		let keycode = originalkeycode_to_dinputdef(config.get_keycode(check_key as i32));

		if keycode == -1 {
			return match mode {
				0 => input_ctrl.check_mouse_button_now_l(),
				1 => input_ctrl.check_mouse_button_down_l(),
				_ => input_ctrl.check_mouse_button_up_l(),
			};
		}
		if keycode == -2 {
			return match mode {
				0 => input_ctrl.check_mouse_button_now_r(),
				1 => input_ctrl.check_mouse_button_down_r(),
				_ => input_ctrl.check_mouse_button_up_r(),
			};
		}

		if mode == 0 {
			return input_ctrl.check_key_now(keycode);
		}
		if mode == 1 {
			return input_ctrl.check_key_down(keycode);
		}
		input_ctrl.check_key_up(keycode)
	}

	/// The upstream `maingame::Input`.
	#[allow(clippy::too_many_arguments)]
	pub fn input(
		&mut self,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		state: &mut StateMachine,
		log: &mut DebugLog,
		window: &WindowControl,
		config: &Config,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundManager,
	) {
		self.time = get_time_ms() as i32;

		// Get the key input, and put the cursor back in the middle.
		input_ctrl.get_input_state(input, true);
		input_ctrl.move_mouse_center(window, config);

		// Get the mouse movement.
		let (mouse_x, mouse_y) = input_ctrl.get_mouse_movement();

		// Calculate the view movement. The scope the player is wearing scales it.
		let scopemode = self
			.world
			.objects
			.get_player_human_object()
			.map(|h| h.get_scope_mode())
			.unwrap_or(0);
		let sparam = self.scope_param(scopemode);
		let mouse_sensitivity =
			degree_to_radian(1.0) * sparam.mouse_range * config.get_mouse_sensitivity() as f32;

		// Invert it when the mouse inversion setting is on.
		let mouse_y = if config.get_invert_mouse_flag() {
			-mouse_y
		} else {
			mouse_y
		};

		if input_ctrl.check_key_down(get_esc_keycode()) {
			// The game exit operation.
			state.push_back_space_key(log);
			self.game_speed = 1;
		} else if input_ctrl.check_key_down(get_function_keycode(12)) {
			// The reset operation.
			state.push_f12_key(log);
			self.game_speed = 1;
		}

		// The screen UI change operation.
		if input_ctrl.check_key_down(get_function_keycode(2)) {
			if self.camera_f2mode == 2 {
				self.camera_f2mode = 0;
			} else {
				self.camera_f2mode += 1;
			}
		}

		if !self.show_console {
			if !self.camera_debugmode {
				// Normal mode: the player drives, unless the AI has the body.
				if !self.player_ai {
					self.input_player(
						mouse_x,
						mouse_y,
						mouse_sensitivity,
						input_ctrl,
						config,
						d3dg,
						sound,
					);
				}
			} else {
				// Debug mode: the free camera drives.
				self.input_view_camera(mouse_x, mouse_y, mouse_sensitivity, input_ctrl, config);
			}
		}

		// The debug console display operation.
		if input_ctrl.check_key_down(get_function_keycode(11)) {
			if !self.show_console {
				self.show_console = true;
				self.console_text_box_control.set_text_box_str(b"");
			} else {
				self.show_console = false;
			}
		}

		if self.show_console {
			self.input_console(input_ctrl);
		}

		self.time_input = get_time_ms() as i32 - self.time;
	}

	/// The upstream `InputPlayer`: the player's own control.
	#[allow(clippy::too_many_arguments)]
	fn input_player(
		&mut self,
		mouse_x: i32,
		mouse_y: i32,
		mouse_sensitivity: f32,
		input_ctrl: &InputControl,
		config: &Config,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundManager,
	) {
		let Some(player_id) = self
			.world
			.objects
			.get_player_human_object()
			.map(|_| self.world.objects.get_player_id())
		else {
			return;
		};
		let hp = self
			.world
			.objects
			.get_player_human_object()
			.map(|h| h.hp())
			.unwrap_or(0);

		if hp > 0 {
			// The direction from the mouse.
			self.mouse_rx += mouse_x as f32 * mouse_sensitivity;
			self.mouse_ry -= mouse_y as f32 * mouse_sensitivity;
			xops_platform::trace::aim(self.mouse_rx, self.mouse_ry);

			// The direction from the keys, eased towards the arrow key angle.
			let up = self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNUP, 0);
			let down = self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNDOWN, 0);
			if up && !down {
				self.add_camera_ry += (INPUT_ARROWKEYS_ANGLE - self.add_camera_ry) * 0.2;
			} else if down && !up {
				self.add_camera_ry += (INPUT_ARROWKEYS_ANGLE * -1.0 - self.add_camera_ry) * 0.2;
			} else {
				self.add_camera_ry = 0.0;
			}

			let left = self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNLEFT, 0);
			let right = self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNRIGHT, 0);
			if left && !right {
				self.add_camera_rx += (INPUT_ARROWKEYS_ANGLE * -1.0 - self.add_camera_rx) * 0.2;
			} else if right && !left {
				self.add_camera_rx += (INPUT_ARROWKEYS_ANGLE - self.add_camera_rx) * 0.2;
			} else {
				self.add_camera_rx = 0.0;
			}

			self.mouse_rx += self.add_camera_rx;
			self.mouse_ry += self.add_camera_ry;

			if self.mouse_ry > degree_to_radian(70.0) {
				self.mouse_ry = degree_to_radian(70.0);
			}
			if self.mouse_ry < degree_to_radian(-70.0) {
				self.mouse_ry = degree_to_radian(-70.0);
			}

			// Set the player orientation.
			if let Some(human) = self.world.objects.get_player_human_object_mut() {
				human.set_rx_ry(self.mouse_rx, self.mouse_ry);
			}

			// Forward, back, left and right movement.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVEFORWARD, 0) {
				self.world.objects.move_forward(player_id);
			} else if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVEBACKWARD, 0)
			{
				self.world.objects.move_back(player_id);
			}
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVELEFT, 0) {
				self.world.objects.move_left(player_id);
			} else if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVERIGHT, 0) {
				self.world.objects.move_right(player_id);
			}

			// Walking.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_WALK, 0) {
				self.world.objects.move_walk(player_id);
			}
			// Jumping.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_JUMP, 1) {
				self.world.objects.move_jump(player_id);
			}

			// Whether it is a zombie, and the firing mode: a zombie always swings, and a
			// weapon without burst mode fires on the press.
			let human_param_id = self
				.world
				.objects
				.get_player_human_object()
				.map(|h| h.get_param_data().0)
				.unwrap_or(-1);
			let zombie = self
				.world
				.param
				.get_human(human_param_id)
				.map(|h| h.r#type == 2)
				.unwrap_or(false);

			let keymode = if zombie {
				1
			} else {
				let blazing = self
					.world
					.objects
					.get_player_human_object()
					.map(|h| h.get_weapon_blazingmode(self.world.objects.weapons()))
					.unwrap_or(false);
				if blazing {
					0
				} else {
					1
				}
			};

			if self.check_input_control(input_ctrl, config, ControlKey::KEY_SHOT, keymode) {
				if !zombie {
					// If the bullet was fired successfully.
					if self.world.objects.shot_weapon(player_id, sound) == 1 {
						self.main_game_info.fire += 1;

						// Get the player orientation back from the arm.
						self.refresh_player_orientation();
					}
				} else {
					for i in 0..MAX_HUMAN {
						if !self.world.objects.check_zombie_attack(player_id as usize, i) {
							continue;
						}
						self.world.objects.hit_zombie_attack(
							player_id as usize,
							i,
							self.world.resource,
							sound,
							config,
						);
					}
				}
			}

			// Reloading.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_RELOAD, 1) {
				self.world.objects.reload_weapon(player_id, sound);
			}

			// Weapon switching.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_SWITCHWEAPON, 1) {
				self.world.objects.change_have_weapon(player_id, -1);
			}
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_WEAPON1, 1) {
				self.world.objects.change_have_weapon(player_id, 0);
			}
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_WEAPON2, 1) {
				self.world.objects.change_have_weapon(player_id, 1);
			}

			// The firing mode change.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_SHOTMODE, 1) {
				self.world
					.objects
					.change_weapon_id(player_id, d3dg, self.world.resource);
			}

			// Discarding the weapon.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_DROPWEAPON, 1) {
				self.world.objects.dump_weapon(player_id);
			}

			// The scope.
			if self.check_input_control(input_ctrl, config, ControlKey::KEY_ZOOMSCOPE, 1) {
				self.world.objects.change_scope_mode(player_id);
			}
		}

		// The camera view mode change.
		if input_ctrl.check_key_down(get_function_keycode(1)) {
			if !self.camera_f1mode {
				self.camera_f1mode = true;
				self.view_rx = 0.0;
				self.view_ry = VIEW_F1MODE_ANGLE;
			} else {
				self.camera_f1mode = false;
				self.view_rx = 0.0;
				self.view_ry = 0.0;
			}
		}

		// The third person camera's numeric keypad.
		if self.camera_f1mode {
			if input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x0C)) {
				// NUM8
				self.view_ry -= INPUT_F1NUMKEYS_ANGLE;
			}
			if input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x09)) {
				// NUM5
				self.view_ry += INPUT_F1NUMKEYS_ANGLE;
			}
			if input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x08)) {
				// NUM4
				self.view_rx -= INPUT_F1NUMKEYS_ANGLE;
			}
			if input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x0A)) {
				// NUM6
				self.view_rx += INPUT_F1NUMKEYS_ANGLE;
			}
		}

		// -- the cheats -----------------------------------------------------

		// The rise cheat: F5 with Enter held.
		self.cmd_f5 = input_ctrl.check_key_now(get_function_keycode(5))
			&& input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x0F));

		if hp > 0 {
			// Add ammo: F6 with Enter held.
			if input_ctrl.check_key_now(get_function_keycode(6))
				&& input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x0F))
			{
				self.world
					.objects
					.cheat_add_bullet(player_id, d3dg, self.world.resource);
			}

			// Change weapon: F7 with left or right held.
			if input_ctrl.check_key_now(get_function_keycode(7)) {
				if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x02)) {
					let mut id_param = self.weapon_type_no();
					if id_param >= TOTAL_PARAMETERINFO_WEAPON as i32 - 1 {
						id_param = 0;
					} else {
						id_param += 1;
					}
					self.world
						.objects
						.cheat_new_weapon(player_id, id_param, d3dg, self.world.resource);
				}
				if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x03)) {
					let mut id_param = self.weapon_type_no();
					if id_param <= 0 {
						id_param = TOTAL_PARAMETERINFO_WEAPON as i32 - 1;
					} else {
						id_param -= 1;
					}
					self.world
						.objects
						.cheat_new_weapon(player_id, id_param, d3dg, self.world.resource);
				}
			}
		}

		// Change person: F8 with left or right held.
		if input_ctrl.check_key_now(get_function_keycode(8)) {
			if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x02)) {
				let mut player_human_id = self.world.objects.get_player_id() + 1;
				if player_human_id >= MAX_HUMAN as i32 {
					player_human_id = 0;
				}
				self.world.objects.set_player_id(player_human_id);
				self.refresh_player_orientation();
			}
			if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x03)) {
				let mut player_human_id = self.world.objects.get_player_id() - 1;
				if player_human_id < 0 {
					player_human_id = MAX_HUMAN as i32 - 1;
				}
				self.world.objects.set_player_id(player_human_id);
				self.refresh_player_orientation();
			}
		}

		// Add person: F9 with up or down held.
		if input_ctrl.check_key_now(get_function_keycode(9))
			&& (input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x00))
				|| input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x01)))
		{
			self.cheat_add_human(player_id, input_ctrl, d3dg);
		}

		// The arm drawing cheat.
		if input_ctrl.check_key_down(get_home_keycode()) {
			self.camera_home_mode = !self.camera_home_mode;
		}
	}

	/// `GetMainWeaponTypeNO` off the player human, which the weapon cheat counts around.
	fn weapon_type_no(&self) -> i32 {
		self.world
			.objects
			.get_player_human_object()
			.map(|h| h.get_main_weapon_type_no(self.world.objects.weapons()))
			.unwrap_or(0)
	}

	/// The upstream's add-person cheat: a copy of the player, in front of them, with the
	/// same weapons. Up hands it to the AI tracking the player and down parks it.
	fn cheat_add_human(
		&mut self,
		player_id: i32,
		input_ctrl: &InputControl,
		d3dg: &mut D3DGraphics,
	) {
		let mut weapon_paramid = [0i32; TOTAL_HAVEWEAPON];
		let (x, y, z, r, param, team, selectweapon, held) =
			match self.world.objects.get_player_human_object() {
				Some(human) => {
					let (x, y, z, r) = human.pos_data();
					let (param, _dataid, _p4, team) = human.get_param_data();
					let (selectweapon, held, _lnbs, _nbs) = human.get_weapon();
					(x, y, z, r, param, team, selectweapon, held)
				}
				None => return,
			};
		for i in 0..TOTAL_HAVEWEAPON {
			if let Some(index) = held[i] {
				if let Some(weapon) = self.world.objects.get_weapon_object(index as i32) {
					weapon_paramid[i] = weapon.param_data().0;
				}
			}
		}

		// The place in front of the player.
		let (sin, cos) = (r * -1.0 + std::f32::consts::PI / 2.0).sin_cos();
		let (px, py, pz) = (x + cos * 10.0, y + 5.0, z + sin * 10.0);

		let id = self.world.objects.add_human_index(
			px,
			py,
			pz,
			r,
			param,
			team,
			weapon_paramid,
			0,
			false,
			d3dg,
			self.world.resource,
		);
		if id >= 0 {
			self.world.objects.change_have_weapon(id, selectweapon);

			// Set the AI.
			let id = id as usize;
			self.world.ai[id].init(&self.world.objects, &self.world.point_data);
			if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x00)) {
				self.world.ai[id].set_hold_tracking(player_id);
			}
			if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x01)) {
				self.world.ai[id].set_hold_wait(px, pz, r);
			}
		}
	}

	/// `GetRxRy` off the player human into the mouse angles, and the third person camera's
	/// reset that the cheat operations do alongside it.
	fn refresh_player_orientation(&mut self) {
		if let Some(human) = self.world.objects.get_player_human_object() {
			let (rx, ry) = human.rx_ry();
			self.mouse_rx = rx;
			self.mouse_ry = ry;
		}

		if self.camera_f1mode {
			self.base.camera_rx = degree_to_radian(90.0);
		}
	}

	/// The upstream `InputViewCamera`: the free camera.
	fn input_view_camera(
		&mut self,
		mouse_x: i32,
		mouse_y: i32,
		mouse_sensitivity: f32,
		input_ctrl: &InputControl,
		config: &Config,
	) {
		// The mouse movement is the camera orientation.
		self.base.camera_rx -= mouse_x as f32 * mouse_sensitivity;
		self.base.camera_ry -= mouse_y as f32 * mouse_sensitivity;

		if self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNUP, 0) {
			self.base.camera_ry += INPUT_ARROWKEYS_ANGLE;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNDOWN, 0) {
			self.base.camera_ry -= INPUT_ARROWKEYS_ANGLE;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNLEFT, 0) {
			self.base.camera_rx += INPUT_ARROWKEYS_ANGLE;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_TURNRIGHT, 0) {
			self.base.camera_rx -= INPUT_ARROWKEYS_ANGLE;
		}

		if self.base.camera_ry > degree_to_radian(70.0) {
			self.base.camera_ry = degree_to_radian(70.0);
		}
		if self.base.camera_ry < degree_to_radian(-70.0) {
			self.base.camera_ry = degree_to_radian(-70.0);
		}

		// The movement amount, doubled while the trigger is held.
		let mut dist = VIEW_FREECAMERA_SCALE;
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_SHOT, 0) {
			dist *= 2.0;
		}

		let (crx_sin, crx_cos) = self.base.camera_rx.sin_cos();
		let (cry_sin, cry_cos) = self.base.camera_ry.sin_cos();

		if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVEFORWARD, 0) {
			self.base.camera_x += crx_cos * cry_cos * dist;
			self.base.camera_y += cry_sin * dist;
			self.base.camera_z += crx_sin * cry_cos * dist;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVEBACKWARD, 0) {
			self.base.camera_x -= crx_cos * cry_cos * dist;
			self.base.camera_y -= cry_sin * dist;
			self.base.camera_z -= crx_sin * cry_cos * dist;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVELEFT, 0) {
			let (sin, cos) = (self.base.camera_rx + std::f32::consts::PI / 2.0).sin_cos();
			self.base.camera_x += cos * dist;
			self.base.camera_z += sin * dist;
		}
		if self.check_input_control(input_ctrl, config, ControlKey::KEY_MOVERIGHT, 0) {
			let (sin, cos) = (self.base.camera_rx - std::f32::consts::PI / 2.0).sin_cos();
			self.base.camera_x += cos * dist;
			self.base.camera_z += sin * dist;
		}
	}

	// -- the frame ---------------------------------------------------------

	/// The upstream `maingame::Process`.
	///
	/// The frame runs the object manager, then the AI, then the events, in that order, and
	/// each step's wall clock time is measured around it for the debug overlay. The frame
	/// count, the mission end count and the result copy back are at the end.
	#[allow(clippy::too_many_arguments)]
	pub fn process(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		config: &Config,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		sound: &mut SoundManager,
		sound_ctrl: &mut SoundControl,
	) {
		self.time = get_time_ms() as i32;

		// Run the object manager. The F5 cheat hands the player's body to the AI for the
		// frame, which is what makes it rise.
		let cmd_f5id = if self.cmd_f5 {
			self.world.objects.get_player_id()
		} else {
			-1
		};
		{
			let dark = self.dark_screen_flag;
			let World {
				objects,
				collision,
				block_data,
				mif,
				resource,
				..
			} = &mut *self.world;
			objects.process(
				cmd_f5id,
				false,
				dark,
				d3dg,
				resource,
				collision,
				block_data,
				mif.as_ref(),
				sound,
				config,
			);
		}

		// Add the player's battle record.
		if let Some((ontarget, kill, headshot)) = self
			.world
			.objects
			.get_human_shot_info(self.world.objects.get_player_id())
		{
			self.main_game_info.ontarget += ontarget;
			self.main_game_info.kill += kill;
			self.main_game_info.headshot += headshot;
		}

		self.time_process_object = get_time_ms() as i32 - self.time;

		// Run the AI, which skips the player's own slot unless the cheat handed the body
		// over.
		self.time = get_time_ms() as i32;
		if !self.ai_stop {
			let player_id = self.world.objects.get_player_id();
			let player_ai = self.player_ai;
			let World {
				ai,
				objects,
				block_data,
				point_data,
				collision,
				resource,
				..
			} = &mut *self.world;
			for i in 0..MAX_HUMAN {
				if !player_ai && i as i32 == player_id {
					continue;
				}
				ai[i].process(
					objects,
					block_data,
					point_data,
					collision,
					sound,
					d3dg,
					resource,
					config,
				);
			}
		}
		self.time_process_ai = get_time_ms() as i32 - self.time;

		// The mission end check, which only starts once and only while it has not.
		if self.end_framecnt == 0 {
			let check = self.world.objects.check_game_overor_complete();
			if check == 1 {
				self.end_framecnt += 1;
				self.main_game_info.missioncomplete = true;
			}
			if check == 2 {
				self.end_framecnt += 1;
				self.main_game_info.missioncomplete = false;
			}
		}

		// Run the events.
		self.time = get_time_ms() as i32;
		if !self.event_stop {
			for i in 0..TOTAL_EVENTLINE {
				if self.end_framecnt != 0 {
					continue;
				}

				// The event execution status, which the interpreter writes back.
				let mut param = EventParameter {
					endcnt: self.end_framecnt,
					complete: self.main_game_info.missioncomplete,
					message_id: self.message_id,
					set_message_id: false,
				};

				self.world.events[i].process_event_point(
					&mut self.world.point_data,
					&mut self.world.objects,
					self.event_skip[i],
					&mut param,
				);

				self.end_framecnt = param.endcnt;
				self.main_game_info.missioncomplete = param.complete;
				self.message_id = param.message_id;

				// The skip flag lasts one frame.
				self.event_skip[i] = false;

				// A reset message restarts the count.
				if param.set_message_id {
					self.message_cnt = 0;
				}
			}
		}

		if self.message_id != -1 && self.message_cnt < (TOTAL_EVENTENT_SHOWMESSEC * GAMEFPS) as i32 {
			self.message_cnt += 1;
		} else {
			self.message_id = -1;
			self.message_cnt = 0;
		}
		self.time_process_event = get_time_ms() as i32 - self.time;

		// The camera work.
		let Some((human_x, human_y, human_z, dead, dead_motion)) = self
			.world
			.objects
			.get_player_human_object()
			.map(|human| {
				let (x, y, z, _) = human.pos_data();
				(x, y, z, human.dead_flag(), human.dead_motion_flag())
			})
		else {
			return;
		};

		if self.player_ai {
			// The AI has the body, so the camera follows what it did with it.
			self.refresh_player_orientation();
		}

		self.update_camera(human_x, human_y, human_z, dead, dead_motion);

		// A red flash while the player is taking damage.
		if let Some(human) = self.world.objects.get_player_human_object_mut() {
			self.redflash_flag = human.check_hit().0;
		}

		// The console's main process, while it is up and the mission is still running.
		if self.show_console && self.end_framecnt == 0 {
			self.process_console(state, log, d3dg, config, sound, sound_ctrl, window, input_ctrl, input);
		}

		self.base.scene.framecnt = self.base.scene.framecnt.wrapping_add(1);
		if self.start_framecnt < (2.0 * GAMEFPS) as i32 {
			// The mission is starting.
			self.start_framecnt += 1;
		}
		if self.end_framecnt == 1 {
			// Just after the mission ended: the result is frozen and handed over.
			self.main_game_info.framecnt = self.base.scene.framecnt;
			self.world.game_info = self.main_game_info;
			self.end_framecnt += 1;
		} else if self.end_framecnt > 0 {
			// The mission is ending, and the screen is handed over once the fade is done.
			if self.end_framecnt < (4.0 * GAMEFPS) as i32 {
				self.end_framecnt += 1;
			} else {
				self.world.game_info.fire = self.main_game_info.fire;
				self.world.game_info.ontarget = self.main_game_info.ontarget;
				self.world.game_info.kill = self.main_game_info.kill;
				self.world.game_info.headshot = self.main_game_info.headshot;

				state.push_mouse_button(log);
				self.game_speed = 1;
			}
		}
	}

	/// The camera placement, which upstream is the block of `if`s in the middle of
	/// `Process`.
	///
	/// The live branches are the free camera's own, the death fall, the still frame of the
	/// death motion, the third person camera and the player view. Split out so the eye
	/// position and the angle each of them produces can be checked without a mission.
	fn update_camera(
		&mut self,
		human_x: f32,
		human_y: f32,
		human_z: f32,
		dead: bool,
		dead_motion: bool,
	) {
		if self.camera_debugmode {
			// The free camera drives itself, so the frame leaves it alone.
		} else if dead {
			// Falling over: the camera watches from a little way around and 33 above.
			let camera_ry = angle_normalization(self.base.camera_ry);

			let ry = camera_ry * 0.95 + degree_to_radian(-89.0) * 0.05;
			let r = 3.12;

			self.base.camera_rx += degree_to_radian(1.0);
			self.base.camera_ry = ry;
			self.base.camera_x = human_x + self.base.camera_rx.cos() * r;
			self.base.camera_y = human_y + 33.3;
			self.base.camera_z = human_z + self.base.camera_rx.sin() * r;
		} else if dead_motion {
			// The death motion is playing, so the camera holds still.
		} else if self.camera_f1mode {
			// The third person camera: it trails the look-at point and is pulled in by the
			// first block between the two.
			let crx = self.base.camera_rx * 0.8
				+ (self.view_rx + self.mouse_rx * -1.0 + std::f32::consts::PI / 2.0) * 0.2;
			let mut cry = self.base.camera_ry * 0.8
				+ (self.view_ry + self.mouse_ry) * 0.2
				- std::f32::consts::PI / 2.0;
			let ccx = human_x - crx.cos() * cry.cos() * 3.0;
			let ccy = human_y + HUMAN_HEIGHT - 0.5 + (-cry).sin() * 2.5;
			let ccz = human_z - crx.sin() * cry.cos() * 3.0;

			cry += std::f32::consts::PI / 2.0;

			let hit = self.world.collision.check_all_block_intersect_ray(
				&self.world.block_data,
				ccx,
				ccy,
				ccz,
				crx.cos() * cry.cos() * -1.0,
				(-cry).sin(),
				crx.sin() * cry.cos() * -1.0,
				VIEW_F1MODE_DIST,
			);
			let dist = match hit {
				Some((_, _, dist)) => dist - 1.0,
				None => VIEW_F1MODE_DIST,
			};

			self.base.camera_x = ccx - crx.cos() * cry.cos() * dist;
			self.base.camera_y = ccy + (-cry).sin() * dist;
			self.base.camera_z = ccz - crx.sin() * cry.cos() * dist;
			self.base.camera_rx = crx;
			self.base.camera_ry = cry;
		} else {
			let crx = self.view_rx + self.mouse_rx * -1.0 + std::f32::consts::PI / 2.0;
			let cry = self.view_ry + self.mouse_ry;

			self.base.camera_x = human_x + crx.cos() * cry.cos() * VIEW_DIST;
			self.base.camera_y = human_y + VIEW_HEIGHT + cry.sin() * VIEW_DIST;
			self.base.camera_z = human_z + crx.sin() * cry.cos() * VIEW_DIST;
			self.base.camera_rx = crx;
			self.base.camera_ry = cry;
		}
	}

	/// The upstream `maingame::Sound`: the world's sound, from where the player's camera is
	/// and for the team the player is on.
	pub fn sound(&mut self, sound: &mut SoundManager, sound_ctrl: &mut SoundControl) {
		self.time = get_time_ms() as i32;

		let teamid = self
			.world
			.objects
			.get_player_human_object()
			.map(|h| h.get_param_data().3)
			.unwrap_or(0);

		sound.play_world_sound(
			sound_ctrl,
			self.world.resource,
			&self.world.param,
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			teamid,
		);

		self.time_sound = get_time_ms() as i32 - self.time;
	}

	// -- the drawing -------------------------------------------------------

	/// The upstream `maingame::Render3D`.
	fn render_3d(&mut self, d3dg: &mut D3DGraphics, _config: &Config) {
		self.time = get_time_ms() as i32;

		let (hp, scopemode) = match self.world.objects.get_player_human_object() {
			Some(human) => (human.hp(), human.get_scope_mode()),
			None => (0, 0),
		};

		// Set the fog and the camera.
		d3dg.set_fog(self.fog_flag, self.sky_number);
		let viewangle = self.view_angle(scopemode);
		d3dg.set_camera(
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
			viewangle,
		);

		// The background sky, at the camera.
		d3dg.set_world_transform(
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			0.0,
			0.0,
			2.0,
		);
		let (skymodel, skytexture) = self.world.resource.get_sky_model_texture();
		d3dg.render_model(skymodel, skytexture, self.dark_screen_flag, false, false);

		// Clear Z buffer.
		d3dg.reset_zbuffer();

		if self.center_line {
			// The center line, for debug.
			d3dg.render_centerline();
		}

		// The map.
		d3dg.reset_world_transform();
		d3dg.render_mapdata(self.wireframe);

		// Whether to draw the player: not in the third person camera, the free camera or
		// when dead, and the arms only when the arm setting is off.
		let mut draw_player = 0;
		if !self.camera_f1mode && !self.camera_debugmode && hp > 0 {
			if !self.camera_home_mode {
				draw_player = 1;
			} else {
				draw_player = 2;
			}
		}

		let (cx, cy, cz, crx, cry) = (
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
		);
		let nomodel = self.nomodel;
		self.world.objects.render(
			d3dg,
			self.world.resource,
			cx,
			cy,
			cz,
			crx,
			cry,
			draw_player,
			nomodel,
		);

		// The AI debug view, drawn in the world.
		if self.ai_debuginfo_id != -1 && (0..MAX_HUMAN as i32).contains(&self.ai_debuginfo_id) {
			self.render_ai_debug_3d(d3dg);
		}
	}

	/// The AI debug view's 3D half: a cross on the AI, one on where it is heading and a box
	/// on its target.
	fn render_ai_debug_3d(&mut self, d3dg: &mut D3DGraphics) {
		let id = self.ai_debuginfo_id;

		let (posx, posy, posz) = match self.world.objects.get_human_object(id) {
			Some(human) => {
				let (x, y, z, _) = human.pos_data();
				(x, y, z)
			}
			None => return,
		};
		let enemy_id = self.world.ai[id as usize].get_enemy_human_id();
		let (mposx, mposz, _) = self.world.ai[id as usize].get_move_target_pos();

		let blue = D3DGraphics::get_color_code(0.0, 0.0, 1.0, 1.0);
		let yellow = D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0);
		let red = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);

		d3dg.reset_world_transform();

		// Itself.
		d3dg.render_line(posx + 10.0, posy, posz, posx - 10.0, posy, posz, blue);
		d3dg.render_line(posx, posy + 10.0, posz, posx, posy - 10.0, posz, blue);
		d3dg.render_line(posx, posy, posz + 10.0, posx, posy, posz - 10.0, blue);

		// The destination.
		d3dg.render_line(mposx + 10.0, posy, mposz, mposx - 10.0, posy, mposz, yellow);
		d3dg.render_line(mposx, 5000.0, mposz, mposx, -500.0, mposz, yellow);
		d3dg.render_line(mposx, posy, mposz + 10.0, mposx, posy, mposz - 10.0, yellow);

		if enemy_id != -1 {
			let (posx, posy, posz) = match self.world.objects.get_human_object(enemy_id) {
				Some(human) => {
					let (x, y, z, _) = human.pos_data();
					(x, y, z)
				}
				None => return,
			};

			// The attack target, a box from the feet to the head.
			let y2 = posy + HUMAN_HEIGHT;
			let lines = [
				(posx + 3.0, posy, posz + 3.0, posx + 3.0, posy, posz - 3.0),
				(posx + 3.0, posy, posz - 3.0, posx - 3.0, posy, posz - 3.0),
				(posx - 3.0, posy, posz - 3.0, posx - 3.0, posy, posz + 3.0),
				(posx - 3.0, posy, posz + 3.0, posx + 3.0, posy, posz + 3.0),
				(posx + 3.0, y2, posz + 3.0, posx + 3.0, y2, posz - 3.0),
				(posx + 3.0, y2, posz - 3.0, posx - 3.0, y2, posz - 3.0),
				(posx - 3.0, y2, posz - 3.0, posx - 3.0, y2, posz + 3.0),
				(posx - 3.0, y2, posz + 3.0, posx + 3.0, y2, posz + 3.0),
				(posx + 3.0, posy, posz + 3.0, posx + 3.0, y2, posz + 3.0),
				(posx + 3.0, posy, posz - 3.0, posx + 3.0, y2, posz - 3.0),
				(posx - 3.0, posy, posz - 3.0, posx - 3.0, y2, posz - 3.0),
				(posx - 3.0, posy, posz + 3.0, posx - 3.0, y2, posz + 3.0),
			];
			for (x1, y1, z1, x2, y2, z2) in lines {
				d3dg.render_line(x1, y1, z1, x2, y2, z2, red);
			}
		}
	}

	/// [`D3Dscene::render_main`] for this screen.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		D3Dscene::render_main(d3dg, config, self, Self::render_3d, Self::render_2d)
	}

	/// One edge cell string of a mode A panel, at the panel's own 32 pixel cell.
	fn draw_huda_edge(&self, d3dg: &mut D3DGraphics, x: i32, y: i32, edge: &[u8]) {
		d3dg.draw_2d_texture_font_text(
			x,
			y,
			edge,
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 0.5),
			HUDA_CELL,
			HUDA_CELL,
		);
	}

	/// The upstream `maingame::Render2D`, which is the whole HUD.
	fn render_2d(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let (width, height) = (config.get_screen_width(), config.get_screen_height());
		let fps = get_fps(10);

		// -- the player's weapon state --------------------------------------

		let (
			selectweapon,
			held,
			lnbslist,
			nbslist,
			hp,
			reloadcnt,
			selectweaponcnt,
			error_range,
			human_pos,
			scopemode,
		) = match self.world.objects.get_player_human_object() {
			Some(human) => {
				let (selectweapon, held, lnbslist, nbslist) = human.get_weapon();
				(
					selectweapon,
					held,
					lnbslist,
					nbslist,
					human.hp(),
					human.get_weapon_reload_cnt(),
					human.get_change_have_weapon_cnt() + human.get_change_weapon_id_cnt(),
					human.gunsight_error_range(),
					human.pos_data(),
					human.get_scope_mode(),
				)
			}
			None => return,
		};

		let mut weapon_paramid = [0i32; TOTAL_HAVEWEAPON];
		let mut lnbs = 0;
		let mut nbs = 0;
		for i in 0..TOTAL_HAVEWEAPON {
			if let Some(index) = held[i] {
				if let Some(weapon) = self.world.objects.get_weapon_object(index as i32) {
					let (param_id, loadbullets, bullets) = weapon.param_data();
					weapon_paramid[i] = param_id;
					if selectweapon == i as i32 {
						lnbs = loadbullets;
						nbs = bullets;
					}
				}
			} else if selectweapon == i as i32 {
				lnbs = lnbslist[i];
				nbs = nbslist[i];
			}
		}

		let weapon_paramdata = self.weapon_param(weapon_paramid[selectweapon as usize]);
		// `str[sizeof(weaponname) - 1] = '\0'`, so a long name is cut to 24 bytes.
		let weaponname = clipped(weapon_paramdata.name, 24).to_vec();
		let param_scopemode = weapon_paramdata.scopemode;
		let param_weapon_p = weapon_paramdata.weapon_p;
		let param_pellet = weapon_paramdata.pellet;

		let (human_x, human_y, human_z, human_rx) = human_pos;

		// The color by HP.
		let statecolor = if hp >= 100 {
			D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0)
		} else if hp >= 50 {
			D3DGraphics::get_color_code(1.0 / 50.0 * (100 - hp) as f32, 1.0, 0.0, 1.0)
		} else if hp > 0 {
			D3DGraphics::get_color_code(1.0, 1.0 / 50.0 * hp as f32, 0.0, 1.0)
		} else {
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0)
		};

		// The red flash, which lasts one frame.
		if self.redflash_flag && !self.camera_debugmode {
			d3dg.draw_2d_box(
				0,
				0,
				width,
				height,
				D3DGraphics::get_color_code(1.0, 0.0, 0.0, 0.5),
			);
			self.redflash_flag = false;
		}

		// The scope overlay.
		if !self.camera_f1mode && !self.camera_debugmode && scopemode != 0 {
			let sparam = self.scope_param(scopemode);
			let scope_texture = self.world.resource.get_scope_texture();
			let wide = width as f32 / height as f32 > 1.5;
			let swidth = if wide {
				(height as f32 * 1.333) as i32 + 1
			} else {
				0
			};

			if wide {
				d3dg.draw_2d_texture((width - swidth) / 2, 0, scope_texture, swidth, height, 1.0);
				let black = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0);
				d3dg.draw_2d_box(0, 0, (width - swidth) / 2, height, black);
				d3dg.draw_2d_box(width - (width - swidth) / 2, 0, width, height, black);
			} else {
				d3dg.draw_2d_texture(0, 0, scope_texture, width, height, 1.0);
			}

			if sparam.scope_gunsight == 1 {
				let black = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0);
				let half = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5);
				let (cx, cy) = (width / 2, height / 2);
				d3dg.draw_2d_line(cx - 49, cy, cx - 4, cy, black);
				d3dg.draw_2d_line(cx + 4, cy, cx + 49, cy, black);
				d3dg.draw_2d_line(cx, cy - 49, cx, cy - 4, black);
				d3dg.draw_2d_line(cx, cy + 4, cx, cy + 49, black);
				d3dg.draw_2d_box(cx - 50, cy - 1, cx + 50, cy + 1, half);
				d3dg.draw_2d_box(cx - 1, cy - 50, cx + 1, cy + 50, half);
			}
			if sparam.scope_gunsight == 2 {
				let w = if wide {
					140 + (width - swidth) / 2
				} else {
					140
				};
				let black = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0);
				let half = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5);
				let (cx, cy) = (width / 2, height / 2);
				d3dg.draw_2d_line(w, cy, width - w, cy, black);
				d3dg.draw_2d_line(cx, 60, cx, height - 60, black);
				d3dg.draw_2d_box(w, cy - 1, width - w, cy + 1, half);
				d3dg.draw_2d_box(cx - 1, 60, cx + 1, height - 60, half);
			}
		}

		// The blindfold: a black half of the screen wherever the wall is close enough for
		// the camera to be inside it.
		if self.camera_blind && !self.camera_debugmode && hp > 0 {
			let adddist = 1.2f32;
			let addang = self.scope_param(scopemode).view_angle / 4.0;
			let black = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0);
			let (crx, cry) = (self.base.camera_rx, self.base.camera_ry);
			let (cx, cy, cz) = (self.base.camera_x, self.base.camera_y, self.base.camera_z);

			let inside = |rx: f32, ry: f32| {
				self.world.collision.check_all_block_inside(
					&self.world.block_data,
					cx + rx.cos() * ry.cos() * adddist,
					cy + ry.sin() * adddist,
					cz + rx.sin() * ry.cos() * adddist,
				)
			};

			// Top.
			if inside(crx, cry + addang) {
				d3dg.draw_2d_box(0, 0, width, height / 2, black);
			}
			// Bottom.
			if inside(crx, cry - addang) {
				d3dg.draw_2d_box(0, height / 2, width, height, black);
			}
			// Left.
			if inside(crx + addang, cry) {
				d3dg.draw_2d_box(0, 0, width / 2, height, black);
			}
			// Right.
			if inside(crx - addang, cry) {
				d3dg.draw_2d_box(width / 2, 0, width, height, black);
			}
		}

		// The debug overlay, in the texture font so it stays legible at any size.
		if self.show_info_debugmode {
			let (move_x, move_y, move_z) = self
				.world
				.objects
				.get_player_human_object()
				.map(|h| h.move_pos())
				.unwrap_or((0.0, 0.0, 0.0));
			let keyflag = self
				.world
				.objects
				.get_player_human_object()
				.map(|h| h.get_move_flag(false))
				.unwrap_or(0);
			let block_id = self
				.world
				.objects
				.get_player_human_object()
				.map(|h| h.get_under_block().0)
				.unwrap_or(-1);

			let frames = self.base.scene.framecnt;
			let fps_int = GAMEFPS as u32;
			let fps_int = if fps_int == 0 { 1 } else { fps_int };

			let dark = D3DGraphics::get_color_code(0.1, 0.1, 0.1, 1.0);
			let bright = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
			let player_id = self.world.objects.get_player_id();
			let (cx, cy, cz, crx, cry) = (
				self.base.camera_x,
				self.base.camera_y,
				self.base.camera_z,
				self.base.camera_rx,
				self.base.camera_ry,
			);
			let (time_input, time_object, time_ai, time_event, time_sound) = (
				self.time_input,
				self.time_process_object,
				self.time_process_ai,
				self.time_process_event,
				self.time_sound,
			);

			let lines = [
				format!(
					"frame:{}   time {:02}:{:02}",
					frames,
					frames / fps_int / 60,
					frames / fps_int % 60
				),
				format!(
					"camera x:{cx:.2} y:{cy:.2} z:{cz:.2} rx:{crx:.2} ry:{cry:.2}"
				),
				format!(
					"human[{}] x:{human_x:.2} y:{human_y:.2} z:{human_z:.2} rx:{human_rx:.2} HP:{} MoveKey:0x{:02X}",
					player_id,
					hp,
					keyflag
				),
				format!(
					"          move_x:{move_x:.2} move_y:{move_y:.2} move_z:{move_z:.2} BlockID:{block_id}"
				),
				format!(
					"Input:{:02}ms Object:{:02}ms AI:{:02}ms Event:{:02}ms Sound:{:02}ms Render:{:02}ms",
					time_input, time_object, time_ai, time_event, time_sound, self.time_render
				),
			];

			for (i, text) in lines.iter().enumerate() {
				let y = 10 + 20 * i as i32;
				d3dg.draw_2d_texture_debug_font_text(10 + 1, y + 1, text.as_bytes(), dark);
				d3dg.draw_2d_texture_debug_font_text(10, y, text.as_bytes(), bright);
			}
		}

		// The fps readout, in the texture font at the top right.
		let fps_text = format!("fps:{fps:.2}");
		let fps_x = width - fps_text.len() as i32 * 10 - 5;
		d3dg.draw_2d_texture_font_text(
			fps_x + 1,
			5 + 1,
			fps_text.as_bytes(),
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
			10,
			14,
		);
		d3dg.draw_2d_texture_font_text(
			fps_x,
			5,
			fps_text.as_bytes(),
			D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0),
			10,
			14,
		);

		let layout = HudaLayout::new(config);

		// -- HUD mode A -----------------------------------------------------

		if self.camera_f2mode == 0 {
			let mut edge = HUDA_LEFT_EDGE.to_vec();
			self.draw_huda_edge(d3dg, layout.left_x, layout.left_y, &edge);
			huda_next_row(&mut edge);
			self.draw_huda_edge(d3dg, layout.left_x, layout.left_y + HUDA_CELL, &edge);

			let mut edge = HUDA_RIGHT_EDGE.to_vec();
			self.draw_huda_edge(d3dg, layout.right_x, layout.right_y, &edge);
			huda_next_row(&mut edge);
			self.draw_huda_edge(d3dg, layout.right_x, layout.right_y + HUDA_CELL, &edge);

			let mut edge = huda_weapon_edge();
			self.draw_huda_edge(d3dg, layout.weapon_x, layout.weapon_y, &edge);
			huda_next_row(&mut edge);
			for i in 1..HUDA_WEAPON_SIZEH - 1 {
				self.draw_huda_edge(d3dg, layout.weapon_x, layout.weapon_y + HUDA_CELL * i, &edge);
			}
			huda_next_row(&mut edge);
			self.draw_huda_edge(
				d3dg,
				layout.weapon_x,
				layout.weapon_y + HUDA_CELL * (HUDA_WEAPON_SIZEH - 1),
				&edge,
			);

			// The ammo count, spelled in the same katakana: `A` is the magazine and `B` the
			// reserve after it.
			let mut ammo = format!("A{} B{}", lnbs, nbs - lnbs).into_bytes();
			for byte in ammo.iter_mut() {
				if *byte == b'A' {
					*byte = 0xBB;
				}
				if *byte == b'B' {
					*byte = 0xBA;
				}
			}
			d3dg.draw_2d_texture_font_text(
				layout.ammo_x,
				layout.ammo_y,
				&ammo,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				23,
				24,
			);

			// The state word, which sits to the right of its label.
			let (state_word, state_word_x) = if hp >= 80 {
				(&b"FINE"[..], 155)
			} else if hp >= 40 {
				(&b"CAUTION"[..], 135)
			} else if hp > 0 {
				(&b"DANGER"[..], 140)
			} else {
				(&b"DEAD"[..], 155)
			};
			d3dg.draw_2d_texture_font_text(
				layout.state_x,
				layout.state_y,
				b"STATE",
				statecolor,
				18,
				24,
			);
			d3dg.draw_2d_texture_font_text(state_word_x, layout.state_y, state_word, statecolor, 18, 24);

			// The weapon name, shrunk when it is longer than the panel's label width.
			let (w, h) = D3DGraphics::get_font_text_size(&weaponname, 16, 20, 14);
			d3dg.draw_2d_texture_font_text(
				layout.weapon_x + 9,
				layout.weapon_y + 4 + (20 - h) / 2,
				&weaponname,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				w,
				h,
			);
		}

		// -- HUD mode B -----------------------------------------------------

		if self.camera_f2mode == 1 {
			// A frame around the screen in the state color.
			d3dg.draw_2d_line(0, 0, width - 1, 0, statecolor);
			d3dg.draw_2d_line(width - 1, 0, width - 1, height - 1, statecolor);
			d3dg.draw_2d_line(0, 0, 0, height - 1, statecolor);
			d3dg.draw_2d_line(0, height - 1, width - 1, height - 1, statecolor);

			let (w, h) = D3DGraphics::get_font_text_size(&weaponname, 16, 20, 14);
			d3dg.draw_2d_box(
				8,
				height - 32,
				227,
				height - 7,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.3),
			);
			d3dg.draw_2d_texture_font_text(
				10,
				height - 30 + (20 - h) / 2,
				&weaponname,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				w,
				h,
			);
		}

		// The radar.
		if self.radar {
			self.render_radar(d3dg, config);
		}

		// The event message, which fades in and out at the ends of its five seconds.
		if self.message_id != -1 && self.message_cnt < (TOTAL_EVENTENT_SHOWMESSEC * GAMEFPS) as i32 {
			let mut messtr = [0u8; MAX_POINTMESSAGEBYTE];
			if let Some(text) = self.world.point_data.get_message_text(self.message_id) {
				messtr.copy_from_slice(text);
			}

			let mut effect_a = 1.0f32;
			if self.message_cnt < (0.2 * GAMEFPS) as i32 {
				effect_a =
					self.base
						.scene
						.get_effect_alpha(self.message_cnt as u32, 1.0, 0.2, 0.0, false);
			}
			if (((TOTAL_EVENTENT_SHOWMESSEC - 0.2) * GAMEFPS) as i32) < self.message_cnt {
				effect_a = self.base.scene.get_effect_alpha(
					self.message_cnt as u32,
					1.0,
					0.2,
					TOTAL_EVENTENT_SHOWMESSEC - 0.2,
					true,
				);
			}

			d3dg.draw_2d_ms_font_text_center_scaling(
				1,
				340 + 1,
				width,
				140,
				&messtr,
				D3DGraphics::get_color_code(0.1, 0.1, 0.1, effect_a),
			);
			d3dg.draw_2d_ms_font_text_center_scaling(
				0,
				340,
				width,
				140,
				&messtr,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
			);
		}

		if self.show_info_debugmode {
			self.world
				.objects
				.render_log(d3dg, layout.weapon_x, layout.weapon_y - 60);
		}

		// The reload and weapon switch notices.
		if reloadcnt > 0 {
			let dark = D3DGraphics::get_color_code(0.2, 0.2, 0.2, 1.0);
			let bright = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
			d3dg.draw_2d_texture_font_text_center_scaling(3, 300 + 3, b"RELOADING", dark, 32, 34);
			d3dg.draw_2d_texture_font_text_center_scaling(0, 300, b"RELOADING", bright, 32, 34);
		}
		if selectweaponcnt > 0 {
			let dark = D3DGraphics::get_color_code(0.2, 0.2, 0.2, 1.0);
			let bright = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
			d3dg.draw_2d_texture_font_text_center_scaling(3, 300 + 3, b"CHANGING", dark, 32, 34);
			d3dg.draw_2d_texture_font_text_center_scaling(0, 300, b"CHANGING", bright, 32, 34);
		}

		// The crosshair.
		if !self.camera_f1mode && param_weapon_p != 2 && param_pellet > 0 {
			if held[selectweapon as usize].is_some() {
				let sparam = self.scope_param(param_scopemode);
				let shown = (scopemode == 0 && sparam.no_scope_gunsight)
					|| (scopemode != 0 && sparam.scope_gunsight == 0);
				if shown {
					let (cx, cy) = (width / 2, height / 2);
					if config.get_another_gunsight_flag() {
						// The alternate sight, whose arms spread with the weapon's error.
						let mut alpha = 1.0 - error_range as f32 / 40.0;
						if alpha < 0.0 {
							alpha = 0.0;
						}
						let half = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 0.5);
						let full = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);
						let arm = D3DGraphics::get_color_code(1.0, 0.0, 0.0, alpha);
						d3dg.draw_2d_line(cx, cy, cx, cy + 4, half);
						d3dg.draw_2d_line(cx - 15, cy + 15, cx - 19, cy + 19, half);
						d3dg.draw_2d_line(cx + 15, cy + 15, cx + 19, cy + 19, half);
						d3dg.draw_2d_line(cx - 4, cy + 4, cx + 4, cy + 4, full);

						d3dg.draw_2d_line(
							cx - 4 - error_range,
							cy - 4 - error_range / 2,
							cx - 4 - error_range,
							cy + 4 + error_range / 2,
							arm,
						);
						d3dg.draw_2d_line(
							cx + 4 + error_range,
							cy - 4 - error_range / 2,
							cx + 4 + error_range,
							cy + 4 + error_range / 2,
							arm,
						);
					} else {
						// The standard sight, with the two marks that show the spread.
						let full = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);
						d3dg.draw_2d_line(cx - 13, cy, cx - 3, cy, full);
						d3dg.draw_2d_line(cx + 13, cy, cx + 3, cy, full);
						d3dg.draw_2d_line(cx, cy - 13, cx, cy - 3, full);
						d3dg.draw_2d_line(cx, cy + 13, cx, cy + 3, full);

						let half = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 0.5);
						d3dg.draw_2d_texture_font_text_center(
							error_range * -1,
							cy - 16,
							&[0xBD],
							half,
							32,
							32,
						);
						d3dg.draw_2d_texture_font_text_center(error_range, cy - 16, &[0xBE], half, 32, 32);
					}
				}
			}
		}

		if self.tag {
			if let Some((color, info)) = self.world.objects.get_object_info_tag(
				self.base.camera_x,
				self.base.camera_y,
				self.base.camera_z,
				self.base.camera_rx,
				self.base.camera_ry,
			) {
				d3dg.draw_2d_texture_font_text_center_scaling(
					1,
					270 + 1,
					&info,
					D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
					14,
					18,
				);
				d3dg.draw_2d_texture_font_text_center_scaling(0, 270, &info, color, 14, 18);
			}
		}

		// The AI debug view's text half.
		if self.ai_debuginfo_id != -1 && (0..MAX_HUMAN as i32).contains(&self.ai_debuginfo_id) {
			self.render_ai_debug_2d(d3dg);
		}

		// Clear Z buffer, which the held weapon models below need.
		d3dg.reset_zbuffer();

		// The held weapon models, which are 3D draws inside the 2D pass.
		if self.camera_f2mode == 0 {
			let mut notselectweapon = selectweapon + 1;
			if notselectweapon == TOTAL_HAVEWEAPON as i32 {
				notselectweapon = 0;
			}

			// The camera is set again from the scope table's no-scope entry, so the weapon
			// keeps its size whatever scope the player is wearing.
			let sparam = self.scope_param(0);
			d3dg.set_camera(
				self.base.camera_x,
				self.base.camera_y,
				self.base.camera_z,
				self.base.camera_rx,
				self.base.camera_ry,
				sparam.view_angle,
			);

			let frames = self.base.scene.framecnt;
			let (crx, cry) = (self.base.camera_rx, self.base.camera_ry);
			for (slot, rotation) in [(selectweapon, true), (notselectweapon, false)] {
				let param_id = weapon_paramid[slot as usize];
				let size = self.weapon_param(param_id).size;
				let Some((model, texture)) =
					self.world.resource.get_weapon_model_texture(d3dg, param_id)
				else {
					continue;
				};

				let spin = if rotation {
					degree_to_radian((frames * 2) as f32)
				} else {
					0.0
				};
				d3dg.set_world_transform_player_weapon(rotation, crx, cry, spin, size);
				d3dg.render_model(model, texture, false, true, self.nomodel);
			}
		}

		// The console panel, or the two frames the screenshot waits out so that the panel
		// is not in the picture.
		if self.show_console {
			if self.screen_shot > 0 {
				self.screen_shot += 1;
			} else {
				self.render_console(d3dg, config);
			}
		}

		// The blackout at the start and the end of a mission.
		let effect = if self.start_framecnt < (2.0 * GAMEFPS) as i32 {
			self.base
				.scene
				.get_effect_alpha(self.start_framecnt as u32, 1.0, 2.0, 0.0, true)
		} else if self.end_framecnt > 0 {
			self.base
				.scene
				.get_effect_alpha(self.end_framecnt as u32, 1.0, 3.5, 0.0, false)
		} else {
			0.0
		};
		d3dg.draw_2d_box(
			0,
			0,
			width,
			height,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, effect),
		);

		// The result line at the end of a mission.
		if self.end_framecnt > 0 {
			let effect = if self.end_framecnt < (1.0 * GAMEFPS) as i32 {
				self.base
					.scene
					.get_effect_alpha(self.end_framecnt as u32, 1.0, 1.0, 0.0, false)
			} else if self.end_framecnt > (3.0 * GAMEFPS) as i32 {
				self.base
					.scene
					.get_effect_alpha(self.end_framecnt as u32, 1.0, 1.0, 3.0, true)
			} else {
				1.0
			};

			if self.main_game_info.missioncomplete {
				d3dg.draw_2d_texture_font_text_center_scaling(
					0,
					240 + 10,
					b"objective complete",
					D3DGraphics::get_color_code(1.0, 0.5, 0.0, effect),
					28,
					32,
				);
			} else {
				d3dg.draw_2d_texture_font_text_center_scaling(
					0,
					240 + 10,
					b"mission failure",
					D3DGraphics::get_color_code(1.0, 0.0, 0.0, effect),
					28,
					32,
				);
			}
		}

		self.time_render = get_time_ms() as i32 - self.time;
	}

	/// The AI debug view's text half.
	fn render_ai_debug_2d(&mut self, d3dg: &mut D3DGraphics) {
		let id = self.ai_debuginfo_id;

		let (posx, posy, posz, rx, hp) = match self.world.objects.get_human_object(id) {
			Some(human) => {
				let (x, y, z, rx) = human.pos_data();
				(x, y, z, rx, human.hp())
			}
			None => return,
		};
		let (_mode, mode_str) = self.world.ai[id as usize].get_battle_mode();
		let enemy_id = self.world.ai[id as usize].get_enemy_human_id();
		let ai_level = self.world.ai[id as usize].get_ai_level();
		let (mposx, mposz, movemode) = self.world.ai[id as usize].get_move_target_pos();
		let path_point = self.world.ai[id as usize].get_path_point_data(&self.world.point_data);

		let (p1, p2, p3, p4) = match path_point {
			Some(data) => (data.p1, data.p2, data.p3, data.p4),
			None => (0, 0, 0, 0),
		};

		let yellow = D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0);
		let black = D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0);

		let lines = [
			format!("AI debug info [ID:{id}]"),
			format!("(X:{posx:.2} Y:{posy:.2} Z:{posz:.2} RX:{rx:.2} HP:{hp})"),
			format!(
				"Mode:{}  TargetEnemyID:{}  AIlevel:{}",
				String::from_utf8_lossy(mode_str),
				enemy_id,
				ai_level
			),
			format!("PointPath:[{p1}][{p2}][{p3}][{p4}]"),
			format!(
				"MovePosX:{mposx:.2}  MovePosZ:{mposz:.2}  (MoveMode:{})",
				movemode as i32
			),
		];

		for (i, text) in lines.iter().enumerate() {
			let y = 130 + 20 * i as i32;
			d3dg.draw_2d_texture_debug_font_text(20 + 1, y + 1, text.as_bytes(), black);
			d3dg.draw_2d_texture_debug_font_text(20, y, text.as_bytes(), yellow);
		}
	}

	// -- the radar ---------------------------------------------------------

	/// The upstream `GetRadarPos`: a world point to radar coordinates.
	///
	/// `radar_pos_x` and `radar_pos_y` are the radar's top left corner, `radar_size` its
	/// width and height, and `radar_world_r` the world radius it covers. Returns the
	/// drawing point and the point's height relative to the camera.
	///
	/// `None` is both "off the radar" and "the rough check ruled it out early", which is
	/// why the check is only run when asked for: the map's own lines are drawn without it.
	#[allow(clippy::too_many_arguments)]
	pub fn get_radar_pos(
		&self,
		in_x: f32,
		in_y: f32,
		in_z: f32,
		radar_pos_x: i32,
		radar_pos_y: i32,
		radar_size: i32,
		radar_world_r: f32,
		check: bool,
	) -> Option<(i32, i32, f32)> {
		// The distance to the camera.
		let x = in_x - self.base.camera_x;
		let y = in_y - self.base.camera_y;
		let z = in_z - self.base.camera_z;

		// Only process it when it is near enough.
		if !check
			|| (x.abs() < radar_world_r * 2.0 && z.abs() < radar_world_r * 2.0 && y.abs() < 80.0)
		{
			// The angle and the distance.
			let rx = z.atan2(x);
			let r = (x * x + z * z).sqrt();

			// Camera based coordinates.
			let rx2 = (rx - self.base.camera_rx) * -1.0 - std::f32::consts::PI / 2.0;
			let (x2, z2) = (rx2.cos() * r, rx2.sin() * r);

			// Whether it fits.
			if !check || (x2.abs() < radar_world_r && z2.abs() < radar_world_r) {
				let r2 = r / radar_world_r * (radar_size / 2) as f32;
				let out_x = (radar_pos_x + radar_size / 2) as f32 + rx2.cos() * r2;
				let out_y = (radar_pos_y + radar_size / 2) as f32 + rx2.sin() * r2;
				return Some((out_x as i32, out_y as i32, y));
			}
		}

		None
	}

	/// The upstream `RenderRadar`: the map around the player, the event arrival points and
	/// the people.
	fn render_radar(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let radar_size = 200;
		let radar_pos_x = config.get_screen_width() - radar_size - 10;
		let radar_pos_y = 110;
		let radar_world_r = 300.0f32;

		let ecr = DISTANCE_CHECKPOINT / radar_world_r * (radar_size / 2) as f32;

		// The base and the enclosure.
		d3dg.draw_2d_box(
			radar_pos_x,
			radar_pos_y,
			radar_pos_x + radar_size,
			radar_pos_y + radar_size,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.6),
		);
		let green = D3DGraphics::get_color_code(0.0, 0.8, 0.0, 1.0);
		d3dg.draw_2d_line(radar_pos_x, radar_pos_y, radar_pos_x + radar_size, radar_pos_y, green);
		d3dg.draw_2d_line(
			radar_pos_x + radar_size,
			radar_pos_y,
			radar_pos_x + radar_size,
			radar_pos_y + radar_size,
			green,
		);
		d3dg.draw_2d_line(
			radar_pos_x + radar_size,
			radar_pos_y + radar_size,
			radar_pos_x,
			radar_pos_y + radar_size,
			green,
		);
		d3dg.draw_2d_line(radar_pos_x, radar_pos_y + radar_size, radar_pos_x, radar_pos_y, green);

		// The map: the faces that are neither the ground nor a step.
		let grey = D3DGraphics::get_color_code(0.8, 0.8, 0.8, 1.0);
		let (cam_x, cam_y, cam_z) = (self.base.camera_x, self.base.camera_y, self.base.camera_z);
		let total = self.world.block_data.get_total_datas();
		for i in 0..total {
			let Some(bdata) = self.world.block_data.getdata(i) else {
				continue;
			};

			// A rough candidate check first.
			if !self.world.collision.check_block_aabb(
				&self.world.block_data,
				i,
				cam_x - radar_world_r * 2.0,
				cam_y - 1.0,
				cam_z - radar_world_r * 2.0,
				cam_x + radar_world_r * 2.0,
				cam_y + 1.0,
				cam_z + radar_world_r * 2.0,
			) {
				continue;
			}

			for j in 0..6 {
				// A climbable slope only, which leaves the ground and the stairs out.
				let angle = bdata.material[j as usize].vy.acos();
				if !(HUMAN_MAPCOLLISION_SLOPEANGLE < angle && angle < degree_to_radian(120.0)) {
					continue;
				}

				let mut vid = [0i32; 4];
				blockdataface(j, Some(&mut vid), None);

				let mut points = [(0i32, 0i32); 4];
				for k in 0..4 {
					let v = vid[k] as usize;
					if let Some((x, y, _)) = self.get_radar_pos(
						bdata.x[v],
						bdata.y[v],
						bdata.z[v],
						radar_pos_x,
						radar_pos_y,
						radar_size,
						radar_world_r,
						false,
					) {
						points[k] = (x, y);
					}
				}

				// Each of the four edges, clipped to the radar's rectangle.
				for k in 0..4 {
					let (x1, y1) = points[k];
					let (x2, y2) = points[(k + 1) % 4];
					if let Some((lx1, ly1, lx2, ly2)) = get_2d_line_in_box(
						x1,
						y1,
						x2,
						y2,
						radar_pos_x,
						radar_pos_y,
						radar_pos_x + radar_size,
						radar_pos_y + radar_size,
					) {
						d3dg.draw_2d_line(lx1, ly1, lx2, ly2, grey);
					}
				}
			}
		}

		// The event arrival points.
		if !self.event_stop {
			for i in 0..TOTAL_EVENTLINE {
				let p4 = self.world.events[i].get_next_p4();
				let Some(mut data) = self.world.point_data.search_pointdata(0x08, 0, 0, 0, p4, 0).1
				else {
					continue;
				};
				if data.p1 != 13 && data.p1 != 16 {
					continue;
				}

				data.y += VIEW_HEIGHT;
				if let Some((x_2d, y_2d, y)) = self.get_radar_pos(
					data.x,
					data.y,
					data.z,
					radar_pos_x,
					radar_pos_y,
					radar_size,
					radar_world_r,
					true,
				) {
					// The transparency by height.
					let alpha = if y.abs() < 40.0 { 1.0 } else { 0.5 };
					d3dg.draw_2d_cycle(
						x_2d,
						y_2d,
						ecr as i32,
						D3DGraphics::get_color_code(1.0, 0.5, 0.0, alpha),
					);
				}
			}
		}

		// The people: the player, their team and everyone else.
		let player_id = self.world.objects.get_player_id();
		let my_teamid = self
			.world
			.objects
			.get_human_object(player_id)
			.map(|h| h.get_param_data().3)
			.unwrap_or(0);

		for i in 0..MAX_HUMAN as i32 {
			let Some(thuman) = self.world.objects.get_human_object(i) else {
				continue;
			};

			// Unused people and corpses are not drawn.
			if !thuman.enable_flag() || thuman.dead_flag() {
				continue;
			}

			let (tx, ty, tz, _) = thuman.pos_data();
			let tteamid = thuman.get_param_data().3;
			let ty = ty + VIEW_HEIGHT;

			if let Some((x_2d, y_2d, y)) = self.get_radar_pos(
				tx,
				ty,
				tz,
				radar_pos_x,
				radar_pos_y,
				radar_size,
				radar_world_r,
				true,
			) {
				let alpha = if y.abs() < 40.0 { 1.0 } else { 0.5 };

				let color = if player_id == i {
					D3DGraphics::get_color_code(1.0, 1.0, 0.0, alpha)
				} else if tteamid == my_teamid {
					D3DGraphics::get_color_code(0.0, 0.5, 1.0, alpha)
				} else {
					D3DGraphics::get_color_code(1.0, 0.0, 0.5, alpha)
				};

				d3dg.draw_2d_box(x_2d - 3, y_2d - 3, x_2d + 3, y_2d + 3, color);
			}
		}
	}

	/// The display mode switch, the upstream `ChangeWindowMode` in `gamemain.cpp`.
	///
	/// Upstream this is the window mode, a full renderer reset and then the screen's own
	/// `Recovery`. The renderer reset belongs to the frame host, which owns the swapchain,
	/// so what is left here is the mode, the recovery, and the input state the reset
	/// invalidates: the movement the resolution change produced is read out and thrown
	/// away, and the cursor goes back to the middle.
	///
	/// Returns 0 on success and 1 on failure, the two codes the console tells apart.
	#[allow(clippy::too_many_arguments)]
	fn change_window_mode(
		&mut self,
		d3dg: &mut D3DGraphics,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		config: &Config,
		sound_ctrl: &mut SoundControl,
		fullscreen: bool,
	) -> i32 {
		d3dg.set_fullscreen_flag(fullscreen);

		if !window.change_window_mode(fullscreen) {
			// A window that cannot switch at all. Upstream returns 2 here and exits the
			// game; the caller only tells 0 from non-zero, and the host owns the exit.
			return 1;
		}

		self.recovery(d3dg, sound_ctrl);

		input_ctrl.get_input_state(input, true);
		input_ctrl.move_mouse_center(window, config);

		0
	}

	// -- the debug console -------------------------------------------------

	/// The upstream `AddInfoConsole`: add a line to the console.
	///
	/// New lines go at the bottom, and once the displayable lines are full the oldest is
	/// dropped and everything shifts up.
	fn add_info_console(&mut self, color: u32, text: &[u8]) {
		for i in 0..MAX_CONSOLELINES {
			if self.info_console_data[i].textdata[0] == 0 {
				self.info_console_data[i].color = color;
				write_cstr(&mut self.info_console_data[i].textdata, text);
				return;
			}
		}

		for i in 1..MAX_CONSOLELINES {
			self.info_console_data[i - 1] = self.info_console_data[i];
		}
		let last = MAX_CONSOLELINES - 1;
		self.info_console_data[last].color = color;
		write_cstr(&mut self.info_console_data[last].textdata, text);
	}

	/// `strcmp(NewCommand, cmd) == 0`, the test the console's command table branches on.
	fn command_is(&self, cmd: &[u8]) -> bool {
		&self.new_command[..c_str_len(&self.new_command)] == cmd
	}

	/// The upstream `GetCommandNum`: a command with one integer argument.
	///
	/// The command name must be followed by a space, and everything after it must be digits
	/// with an optional leading sign, so `"skip 2"` answers 2 while `"skip"`, `"skipx 1"` and
	/// `"skip 2x"` answer nothing.
	pub fn get_command_num(&self, cmd: &[u8]) -> Option<i32> {
		let line = &self.new_command[..c_str_len(&self.new_command)];

		if line.len() < cmd.len() + 2 || &line[..cmd.len()] != cmd {
			return None;
		}
		if line[cmd.len()] != b' ' {
			return None;
		}

		let digits = &line[cmd.len() + 1..];
		if !digits
			.iter()
			.all(|&b| b.is_ascii_digit() || b == b'+' || b == b'-')
		{
			return None;
		}

		// `atoi` of the checked text. A sign with no digits behind it is a zero, and a run
		// too long for the type saturates rather than wrapping, which the upstream's own
		// overflow leaves undefined.
		let (negative, digits) = match digits.first() {
			Some(b'-') => (true, &digits[1..]),
			Some(b'+') => (false, &digits[1..]),
			_ => (false, digits),
		};
		let mut num: i32 = 0;
		for &byte in digits {
			num = num.saturating_mul(10).saturating_add((byte - b'0') as i32);
		}

		Some(if negative { -num } else { num })
	}

	/// The upstream `InputConsole`: the console's own line editor.
	fn input_console(&mut self, input_ctrl: &InputControl) {
		self.new_command[0] = 0;

		if self.console_text_box_control.process_text_box(input_ctrl) {
			// [ENTER]: echo the line and register it as an unprocessed command.
			let mut line = CONSOLE_PROMPT.to_vec();
			line.extend_from_slice(self.console_text_box_control.get_text_box_str());
			self.add_info_console(D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0), &line);

			let command = self.console_text_box_control.get_text_box_str().to_vec();
			write_cstr(&mut self.new_command, &command);

			self.console_text_box_control.set_text_box_str(b"");
		}
	}

	/// The upstream `ProcessConsole`: the command table.
	#[allow(clippy::too_many_arguments)]
	fn process_console(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		config: &Config,
		sound: &mut SoundManager,
		sound_ctrl: &mut SoundControl,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
	) {
		let white = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
		let mission_id = self.world.game_info.selectmission_id;
		let filetype = self
			.world
			.mif
			.as_ref()
			.map(|m| m.get_filetype())
			.unwrap_or(false);

		// The command list.
		if self.command_is(b"help") {
			for line in [
				&b"help          human        result          event            skip <NUM>"[..],
				&b"config        mif          bd1    pd1      resinfo          info"[..],
				&b"view          center       map    model    aiinfo <NUM>     tag"[..],
				&b"radar         inmap        sky <NUM>       dark     fog     teleport <NUM>"[..],
				&b"player <NUM>  revive       kill <NUM>      treat <NUM>      nodamage <NUM>"[..],
				&b"newobj <NUM>  break <NUM>  delhuman <NUM>  delweapon <NUM>  delobj <NUM>"[..],
				&b"nofight       caution      ailevel <NUM>   ff       bot     stop      estop"[..],
				&b"speed         window       ss              clear            ver       exit"[..],
			] {
				self.add_info_console(white, line);
			}
		}

		// The statistics for people.
		if self.command_is(b"human") {
			let mut alivemyfriend = 0;
			let mut myfriend = 0;
			let mut aliveenemy = 0;
			let mut enemy = 0;

			let myteamid = self
				.world
				.objects
				.get_player_human_object()
				.map(|h| h.get_param_data().3)
				.unwrap_or(0);

			for i in 0..MAX_HUMAN as i32 {
				let Some(thuman) = self.world.objects.get_human_object(i) else {
					continue;
				};
				if !thuman.enable_flag() {
					continue;
				}

				let deadflag = thuman.dead_flag();
				let teamid = thuman.get_param_data().3;

				if teamid == myteamid {
					myfriend += 1;
					if !deadflag {
						alivemyfriend += 1;
					}
				} else {
					enemy += 1;
					if !deadflag {
						aliveenemy += 1;
					}
				}
			}

			let line = format!(
				"Friend:{}/{}  Enemy:{}/{}  Total:{}/{}",
				alivemyfriend,
				myfriend,
				aliveenemy,
				enemy,
				alivemyfriend + aliveenemy,
				myfriend + enemy
			);
			self.add_info_console(white, line.as_bytes());
		}

		// The provisional result.
		if self.command_is(b"result") {
			let intontarget = self.main_game_info.ontarget.floor() as i32;
			let rate = if self.main_game_info.fire == 0 {
				0.0
			} else {
				intontarget as f32 / self.main_game_info.fire as f32 * 100.0
			};

			let frames = self.base.scene.framecnt;
			let fps = GAMEFPS as u32;
			let line = format!(
				"Time {:02}:{:02}  /  Fired {}  /  On target {}({:.2})",
				frames / fps / 60,
				frames / fps % 60,
				self.main_game_info.fire,
				intontarget,
				self.main_game_info.ontarget
			);
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				"AR rate {:.1}%  /  Kill {}  /  HS {}",
				rate, self.main_game_info.kill, self.main_game_info.headshot
			);
			self.add_info_console(white, line.as_bytes());
		}

		// The event tasks.
		if self.command_is(b"event") {
			for i in 0..TOTAL_EVENTLINE {
				let p4 = self.world.events[i].get_next_p4();
				let found = self.world.point_data.search_pointdata(0x08, 0, 0, 0, p4, 0).1;
				let line = match found {
					None => format!("Event {i}   No task."),
					Some(data) => {
						format!("Event {i}   [{}][{}][{}][{}]", data.p1, data.p2, data.p3, data.p4)
					}
				};
				self.add_info_console(white, line.as_bytes());
			}
		}

		// Skipping an event task.
		if let Some(id) = self.get_command_num(b"skip") {
			if (0..TOTAL_EVENTLINE as i32).contains(&id) {
				let p4 = self.world.events[id as usize].get_next_p4();
				let found = self.world.point_data.search_pointdata(0x08, 0, 0, 0, p4, 0).1;
				match found {
					None => {
						let line = format!("Event {id}   No task.");
						self.add_info_console(white, line.as_bytes());
					}
					Some(data) => {
						self.event_skip[id as usize] = true;

						let line = format!("Skipped event [{id}].");
						self.add_info_console(white, line.as_bytes());
						let line =
							format!(" ([{}][{}][{}][{}])", data.p1, data.p2, data.p3, data.p4);
						self.add_info_console(white, line.as_bytes());
					}
				}
			}
		}

		// The settings.
		if self.command_is(b"config") {
			self.add_info_console(white, b"[Basic Config]");

			// The key codes, ten to a line.
			self.add_info_console(white, b" Keycode : ");
			let mut i = 0;
			while i * 10 < TOTAL_ControlKey as i32 {
				let mut line = String::from(" ");
				for j in 0..10 {
					if i * 10 + j >= TOTAL_ControlKey as i32 {
						break;
					}
					line.push_str(&format!(" 0x{:02X}", config.get_keycode(i * 10 + j)));
				}
				self.add_info_console(white, line.as_bytes());
				i += 1;
			}

			let mut line = String::from(" ");
			line.push_str(&format!(
				"MouseSensitivity : {:2}           ",
				config.get_mouse_sensitivity()
			));
			if config.get_fullscreen_flag() {
				line.push_str("FullscreenFlag : true (fullscreen)");
			} else {
				line.push_str("FullscreenFlag : false (window)");
			}
			self.add_info_console(white, line.as_bytes());

			let mut line = String::from(" ");
			if config.get_sound_flag() {
				line.push_str("SoundFlag : true (ON)           ");
			} else {
				line.push_str("SoundFlag : false (OFF)         ");
			}
			if config.get_blood_flag() {
				line.push_str("BloodFlag : true (ON)");
			} else {
				line.push_str("BloodFlag : false (OFF)");
			}
			self.add_info_console(white, line.as_bytes());

			let mut line = String::from(" ");
			line.push_str(&format!(
				"Brightness : {:2}                 ",
				config.get_brightness()
			));
			if config.get_invert_mouse_flag() {
				line.push_str("InvertMouseFlag : true (ON)");
			} else {
				line.push_str("InvertMouseFlag : false (OFF)");
			}
			self.add_info_console(white, line.as_bytes());

			let mut line = String::from(" ");
			if config.get_frameskip_flag() {
				line.push_str("FrameskipFlag : true (ON)       ");
			} else {
				line.push_str("FrameskipFlag : false (OFF)     ");
			}
			if config.get_another_gunsight_flag() {
				line.push_str("AnotherGunsightFlag : true (ON)");
			} else {
				line.push_str("AnotherGunsightFlag : false (OFF)");
			}
			self.add_info_console(white, line.as_bytes());

			let mut name = [0u8; MAX_PLAYERNAME + 1];
			config.get_player_name(&mut name);
			let mut line = String::from(" PlayerName : ");
			line.push_str(&String::from_utf8_lossy(&name[..c_str_len(&name)]));
			self.add_info_console(white, line.as_bytes());

			self.add_info_console(white, b"[Add Config]");

			let mut line = String::from(" ");
			line.push_str(&format!("Language : {}     ", config.get_language()));
			line.push_str(&format!(
				"ScreenSize : {}x{}     ",
				config.get_screen_width(),
				config.get_screen_height()
			));
			line.push_str(&format!(
				"MasterVolume : {}%",
				(config.get_volume() * 100.0) as i32
			));
			self.add_info_console(white, line.as_bytes());
		}

		// The MIF information.
		if self.command_is(b"mif") {
			let header = if filetype {
				format!("[Addon Mission]   (MissionID:{mission_id})")
			} else {
				format!("[Standard Mission]   (MissionID:{mission_id})")
			};
			self.add_info_console(white, header.as_bytes());

			let mifpath = self.world.game_info.mifpath;
			let line = console_line(&[
				b"MIFpath : ",
				clipped(&mifpath, MAX_CONSOLELEN - 10 - 1),
			]);
			self.add_info_console(white, &line);

			let name = if filetype {
				self.world
					.mif
					.as_ref()
					.map(|m| m.get_mission_name().to_vec())
					.unwrap_or_default()
			} else {
				self.world
					.param
					.get_official_mission(mission_id)
					.map(|m| m.name.to_vec())
					.unwrap_or_default()
			};
			let line = console_line(&[b"Name : ", clipped(&name, MAX_CONSOLELEN - 7 - 1)]);
			self.add_info_console(white, &line);

			let fullname = if filetype {
				self.world
					.mif
					.as_ref()
					.map(|m| m.get_mission_fullname().to_vec())
					.unwrap_or_default()
			} else {
				self.world
					.param
					.get_official_mission(mission_id)
					.map(|m| m.fullname.to_vec())
					.unwrap_or_default()
			};
			let line = console_line(&[b"FullName : ", clipped(&fullname, MAX_CONSOLELEN - 11 - 1)]);
			self.add_info_console(white, &line);

			let line = if filetype {
				let bd1 = self
					.world
					.mif
					.as_ref()
					.map(|m| m.get_datafile_path().0.to_vec())
					.unwrap_or_default();
				console_line(&[b"BD1file : ", clipped(&bd1, MAX_CONSOLELEN - 10 - 1)])
			} else {
				// 8 is the length of the file name appended after it.
				let (dir, _) = self.official_paths(mission_id);
				console_line(&[
					b"BD1file : ",
					clipped(dir, MAX_CONSOLELEN - 10 - 8 - 1),
					OFFICIALMISSION_BD1,
				])
			};
			self.add_info_console(white, &line);

			let line = if filetype {
				let pd1 = self
					.world
					.mif
					.as_ref()
					.map(|m| m.get_datafile_path().1.to_vec())
					.unwrap_or_default();
				console_line(&[b"PD1file : ", clipped(&pd1, MAX_CONSOLELEN - 10 - 1)])
			} else {
				// 4 is the length of the extension appended after it.
				let (dir, txt) = self.official_paths(mission_id);
				console_line(&[
					b"PD1file : ",
					clipped(&joined(&[dir, txt]), MAX_CONSOLELEN - 10 - 4 - 1),
					PD1_EXT,
				])
			};
			self.add_info_console(white, &line);

			let addobj = self
				.world
				.mif
				.as_ref()
				.map(|m| m.get_add_smallobject_file().to_vec())
				.unwrap_or_default();
			let line = console_line(&[b"AddOBJfile : ", clipped(&addobj, MAX_CONSOLELEN - 13 - 1)]);
			self.add_info_console(white, &line);

			// The sky number always comes from the loaded mission, the two flags from
			// whichever source owns them.
			let skynumber = self
				.world
				.mif
				.as_ref()
				.map(|m| m.get_skynumber())
				.unwrap_or(0);
			let (collisionflag, screenflag) = if filetype {
				let mif = self.world.mif.as_ref();
				(
					mif.map(|m| m.get_collision_flag()).unwrap_or(false),
					mif.map(|m| m.get_screen_flag()).unwrap_or(false),
				)
			} else {
				self.world
					.param
					.get_official_mission(mission_id)
					.map(|m| (m.collision, m.screen))
					.unwrap_or((false, false))
			};
			let line = format!(
				"Sky:{}    CollisionFlag:{}    DarkScreenFlag:{}",
				skynumber, collisionflag as i32, screenflag as i32
			);
			self.add_info_console(white, line.as_bytes());
		}

		// The block data information.
		if self.command_is(b"bd1") {
			let line = if filetype {
				let bd1 = self
					.world
					.mif
					.as_ref()
					.map(|m| m.get_datafile_path().0.to_vec())
					.unwrap_or_default();
				console_line(&[b"Filename : ", clipped(&bd1, MAX_CONSOLELEN - 11 - 1)])
			} else {
				let (dir, _) = self.official_paths(mission_id);
				console_line(&[
					b"Filename : ",
					clipped(dir, MAX_CONSOLELEN - 11 - 8 - 1),
					OFFICIALMISSION_BD1,
				])
			};
			self.add_info_console(white, &line);

			let line = format!("TotalBlocks : {}", self.world.block_data.get_total_datas());
			self.add_info_console(white, line.as_bytes());

			self.add_info_console(white, b"Texture : ");
			for i in 0..(TOTAL_BLOCKTEXTURE / 2) as i32 {
				let fname1 = self
					.world
					.block_data
					.get_texture(i)
					.map(|f| f.to_vec())
					.unwrap_or_default();
				let fname2 = self
					.world
					.block_data
					.get_texture(i + (TOTAL_BLOCKTEXTURE / 2) as i32)
					.map(|f| f.to_vec())
					.unwrap_or_default();

				let flagstr1 = if d3dg.map_texture_id(i as usize) == -1 {
					"NG"
				} else {
					"OK"
				};
				let flagstr2 = if d3dg.map_texture_id(i as usize + TOTAL_BLOCKTEXTURE / 2) == -1 {
					"NG"
				} else {
					"OK"
				};

				let line = format!(
					" {:02}_{} {:<31} {:02}_{} {}",
					i,
					flagstr1,
					String::from_utf8_lossy(&fname1[..c_str_len(&fname1)]),
					i + (TOTAL_BLOCKTEXTURE / 2) as i32,
					flagstr2,
					String::from_utf8_lossy(&fname2[..c_str_len(&fname2)])
				);
				self.add_info_console(white, line.as_bytes());
			}
		}

		// The point data information.
		if self.command_is(b"pd1") {
			let total_points = self.world.point_data.get_total_datas();
			let mut human_points = 0;
			let mut weapon_points = 0;
			let mut object_points = 0;
			let mut humaninfo_points = 0;
			let mut path_points = 0;
			let mut event_points = 0;

			for i in 0..total_points {
				let Some(pdata) = self.world.point_data.getdata(i) else {
					continue;
				};

				if pdata.p1 == POINT_P1TYPE_HUMAN || pdata.p1 == POINT_P1TYPE_HUMAN2 {
					human_points += 1;
				}
				if pdata.p1 == POINT_P1TYPE_WEAPON || pdata.p1 == POINT_P1TYPE_RAND_WEAPON {
					weapon_points += 1;
				}
				if pdata.p1 == POINT_P1TYPE_SMALLOBJ {
					object_points += 1;
				}
				if pdata.p1 == POINT_P1TYPE_HUMANINFO {
					humaninfo_points += 1;
				}
				if pdata.p1 == POINT_P1TYPE_AIPATH || pdata.p1 == POINT_P1TYPE_RAND_AIPATH {
					path_points += 1;
				}
				if (10..=19).contains(&pdata.p1) {
					event_points += 1;
				}
			}

			let line = if filetype {
				let pd1 = self
					.world
					.mif
					.as_ref()
					.map(|m| m.get_datafile_path().1.to_vec())
					.unwrap_or_default();
				console_line(&[b"Filename : ", clipped(&pd1, MAX_CONSOLELEN - 11 - 1)])
			} else {
				let (dir, txt) = self.official_paths(mission_id);
				console_line(&[
					b"Filename : ",
					clipped(&joined(&[dir, txt]), MAX_CONSOLELEN - 11 - 4 - 1),
					PD1_EXT,
				])
			};
			self.add_info_console(white, &line);

			let line = format!("TotalPoints : {total_points}");
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				"HumanPoints : {:<3}       WeaponPoints : {:<3}    SmallOjectPoints : {:<3}",
				human_points, weapon_points, object_points
			);
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				"HumaninfoPoints : {:<3}   AIpathPoints : {:<3}    EventPoints : {:<3}",
				humaninfo_points, path_points, event_points
			);
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				"InvalidPoints : {}",
				total_points
					- (human_points
						+ weapon_points
						+ object_points
						+ humaninfo_points
						+ path_points
						+ event_points)
			);
			self.add_info_console(white, line.as_bytes());

			let line = format!(
				"HumanTextures : {}/{}",
				self.world.resource.get_human_textures(),
				MAX_LOADHUMANTEXTURE
			);
			self.add_info_console(white, line.as_bytes());

			for i in 0..(MAX_LOADHUMANTEXTURE / 2) as i32 {
				let info1 = self.world.resource.get_human_texture_info(i);
				let info2 = self
					.world
					.resource
					.get_human_texture_info(i + (MAX_LOADHUMANTEXTURE / 2) as i32);

				// A texture whose parameter number has no path prints as an empty name.
				let fname1 = info1
					.and_then(|(param_id, _)| self.world.param.get_human_texture_path(param_id))
					.unwrap_or(b"");
				let fname2 = info2
					.and_then(|(param_id, _)| self.world.param.get_human_texture_path(param_id))
					.unwrap_or(b"");

				let flagstr1 = if info1.map(|(_, texture)| texture).unwrap_or(-1) == -1 {
					"NG"
				} else {
					"OK"
				};
				let flagstr2 = if info2.map(|(_, texture)| texture).unwrap_or(-1) == -1 {
					"NG"
				} else {
					"OK"
				};

				let line = format!(
					" {:02}_{} {:<31} {:02}_{} {}",
					i,
					flagstr1,
					String::from_utf8_lossy(&fname1[..c_str_len(fname1)]),
					i + (MAX_LOADHUMANTEXTURE / 2) as i32,
					flagstr2,
					String::from_utf8_lossy(&fname2[..c_str_len(fname2)])
				);
				self.add_info_console(white, line.as_bytes());
			}
		}

		// The resource information.
		if self.command_is(b"resinfo") {
			self.add_info_console(white, b"[Graphics]");
			let line = format!(
				" model : {:2}/{:2}        texture : {:2}/{:2}",
				d3dg.get_total_models(),
				MAX_MODEL,
				d3dg.get_total_textures(),
				MAX_TEXTURE
			);
			self.add_info_console(white, line.as_bytes());

			self.add_info_console(white, b"[Sound]");
			let total_sounds = sound_ctrl.get_total_sounds();
			let line = format!(" sound : {:2}/{:2}", total_sounds, MAX_LOADSOUND);
			self.add_info_console(white, line.as_bytes());

			self.add_info_console(white, b"[Datafile]");
			let line = format!(
				" blocks : {:3}/{:3}     points : {:3}/{:3}",
				self.world.block_data.get_total_datas(),
				MAX_BLOCKS,
				self.world.point_data.get_total_datas(),
				MAX_POINTS
			);
			self.add_info_console(white, line.as_bytes());

			let (human, weapon, smallobject, bullet, grenade, effect) =
				self.world.objects.get_total_objects();
			self.add_info_console(white, b"[Object]");
			let line = format!(
				" human : {:3}/{:3}      weapon : {:3}/{:3}      smallobject : {:2}/{:2}",
				human, MAX_HUMAN, weapon, MAX_WEAPON, smallobject, MAX_SMALLOBJECT
			);
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				" bullet : {:3}/{:3}     grenade : {:3}/{:3}     effect : {:3}/{:3}",
				bullet, MAX_BULLET, grenade, MAX_GRENADE, effect, MAX_EFFECT
			);
			self.add_info_console(white, line.as_bytes());
			let line = format!(
				" sound : {:3}/{:3}",
				sound.get_total_sound_list(),
				MAX_SOUNDMGR_LIST
			);
			self.add_info_console(white, line.as_bytes());
		}

		// The debug text.
		if self.command_is(b"info") {
			if !self.show_info_debugmode {
				self.show_info_debugmode = true;
				self.add_info_console(white, b"Show Debug information.");
			} else {
				self.show_info_debugmode = false;
				self.add_info_console(white, b"Hide Debug information.");
			}
		}

		// The free camera.
		if self.command_is(b"view") {
			if !self.camera_debugmode {
				self.camera_debugmode = true;
				self.add_info_console(white, b"Change FreeView mode.");

				// Where the free camera starts, which is nowhere near the map.
				self.base.camera_x = 100.0;
				self.base.camera_y = 100.0;
				self.base.camera_z = -100.0;
				self.base.camera_rx = degree_to_radian(135.0);
				self.base.camera_ry = degree_to_radian(-40.0);
			} else {
				self.camera_debugmode = false;
				self.add_info_console(white, b"Change PlayerView mode.");
			}
		}

		// The world center line.
		if self.command_is(b"center") {
			if !self.center_line {
				self.center_line = true;
				self.add_info_console(white, b"Show World center position.");
			} else {
				self.center_line = false;
				self.add_info_console(white, b"Hide World center position.");
			}
		}

		// The map as a wireframe.
		if self.command_is(b"map") {
			if !self.wireframe {
				self.wireframe = true;
				self.add_info_console(white, b"Draw map on the Wire frame.");
			} else {
				self.wireframe = false;
				self.add_info_console(white, b"Draw map on the Normal.");
			}
		}

		// The model drawing switch.
		if self.command_is(b"model") {
			if !self.nomodel {
				self.nomodel = true;
				self.add_info_console(white, b"Disable render model.");
			} else {
				self.nomodel = false;
				self.add_info_console(white, b"Enable render model.");
			}
		}

		// The AI information.
		if let Some(id) = self.get_command_num(b"aiinfo") {
			if (0..MAX_HUMAN as i32).contains(&id) {
				if self.ai_debuginfo_id == id {
					// The same number twice turns it off.
					self.ai_debuginfo_id = -1;
					self.add_info_console(white, b"Disable AI Debug information.");
				} else {
					self.ai_debuginfo_id = id;
					let line = format!("Enable AI Debug information. Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		} else if self.command_is(b"aiinfo") && self.ai_debuginfo_id != -1 {
			self.ai_debuginfo_id = -1;
			self.add_info_console(white, b"Disable AI Debug information.");
		}

		// The object tags.
		if self.command_is(b"tag") {
			if !self.tag {
				self.tag = true;
				self.add_info_console(white, b"Show object information.");
			} else {
				self.tag = false;
				self.add_info_console(white, b"Hide object information.");
			}
		}

		// The radar.
		if self.command_is(b"radar") {
			if !self.radar {
				self.radar = true;
				self.add_info_console(white, b"Show Radar.");
			} else {
				self.radar = false;
				self.add_info_console(white, b"Hide Radar.");
			}
		}

		// The blindfold.
		if self.command_is(b"inmap") {
			if !self.camera_blind {
				self.camera_blind = true;
				self.add_info_console(white, b"Blindfold in map.");
			} else {
				self.camera_blind = false;
				self.add_info_console(white, b"Not blindfold in map.");
			}
		}

		// The background sky.
		if let Some(id) = self.get_command_num(b"sky") {
			if (0..=5).contains(&id) {
				self.sky_number = id;

				self.world.resource.cleanup_sky_model_texture(d3dg);
				self.world.resource.load_sky_model_texture(d3dg, id);

				let line = format!("Select SkyNumber {id}.");
				self.add_info_console(white, line.as_bytes());
			}
		}

		// The dark screen, which needs the map rebuilt.
		if self.command_is(b"dark") {
			if !self.dark_screen_flag {
				self.dark_screen_flag = true;
				self.add_info_console(white, b"Enable DarkScreen Flag.");
			} else {
				self.dark_screen_flag = false;
				self.add_info_console(white, b"Disable DarkScreen Flag.");
			}

			let directory = self.mission_directory(self.main_game_info.selectmission_id);
			self.world
				.block_data
				.calculation_blockdata(self.dark_screen_flag);
			d3dg.cleanup_mapdata();
			d3dg.load_mapdata(self.world.block_data.clone(), &directory);
		}

		// The fog.
		if self.command_is(b"fog") {
			if !self.fog_flag {
				self.fog_flag = true;
				self.add_info_console(white, b"Enable Fog Flag.");
			} else {
				self.fog_flag = false;
				self.add_info_console(white, b"Disable Fog Flag.");
			}
		}

		// Moving the player.
		if let Some(id) = self.get_command_num(b"teleport") {
			if (0..MAX_HUMAN as i32).contains(&id) && id != self.world.objects.get_player_id() {
				if let Some((x, y, z, _)) = self
					.world
					.objects
					.get_human_object(id)
					.map(|h| h.pos_data())
				{
					let rx = self
						.world
						.objects
						.get_player_human_object()
						.map(|h| h.pos_data().3)
						.unwrap_or(0.0);
					if let Some(player) = self.world.objects.get_player_human_object_mut() {
						player.set_pos_data(x + 5.0, y + 5.0, z + 5.0, rx);
					}

					let line = format!("Teleported player to Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Switching the player.
		if let Some(id) = self.get_command_num(b"player") {
			if (0..MAX_HUMAN as i32).contains(&id) && id != self.world.objects.get_player_id() {
				self.world.objects.set_player_id(id);
				self.refresh_player_orientation();

				let line = format!("Changed player to Human[{id}].");
				self.add_info_console(white, line.as_bytes());
			}
		}

		// Reviving everyone.
		if self.command_is(b"revive") {
			for i in 0..MAX_HUMAN as i32 {
				self.world.objects.human_resuscitation(i);
			}
			self.add_info_console(white, b"All human has resuscitation.");
		}

		// Healing.
		if let Some(id) = self.get_command_num(b"treat") {
			if (0..MAX_HUMAN as i32).contains(&id) {
				let param = self
					.world
					.objects
					.get_human_object(id)
					.map(|h| h.get_param_data().0)
					.unwrap_or(-1);
				let hp = self
					.world
					.param
					.get_human(param)
					.map(|data| data.hp)
					.unwrap_or(0);

				let healed = self
					.world
					.objects
					.get_human_object_mut(id)
					.map(|human| human.set_hp(hp))
					.unwrap_or(false);
				if healed {
					let line = format!("Set the HP:{hp} to Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Invincibility.
		if let Some(id) = self.get_command_num(b"nodamage") {
			if (0..MAX_HUMAN as i32).contains(&id) {
				if self.invincible_id == id {
					// The same number twice turns it off.
					self.invincible_id = -1;
					if let Some(human) = self.world.objects.get_human_object_mut(id) {
						human.set_invincible_flag(false);
					}
					let line = format!("Not invincible Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				} else {
					// Whoever was specified before is turned off first.
					if self.invincible_id != -1 {
						let previous = self.invincible_id;
						if let Some(human) = self.world.objects.get_human_object_mut(previous) {
							human.set_invincible_flag(false);
						}
						let line = format!("Not invincible Human[{previous}].");
						self.add_info_console(white, line.as_bytes());
					}

					self.invincible_id = id;
					if let Some(human) = self.world.objects.get_human_object_mut(id) {
						human.set_invincible_flag(true);
					}
					let line = format!("Invincible Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		} else if self.command_is(b"nodamage") && self.invincible_id != -1 {
			let id = self.invincible_id;
			if let Some(human) = self.world.objects.get_human_object_mut(id) {
				human.set_invincible_flag(false);
			}
			let line = format!("Not invincible Human[{id}].");
			self.add_info_console(white, line.as_bytes());
			self.invincible_id = -1;
		}

		// Killing.
		if let Some(id) = self.get_command_num(b"kill") {
			if (0..MAX_HUMAN as i32).contains(&id) {
				let enabled = self
					.world
					.objects
					.get_human_object(id)
					.map(|h| h.enable_flag())
					.unwrap_or(false);
				let killed = enabled
					&& self
						.world
						.objects
						.get_human_object_mut(id)
						.map(|h| h.set_hp(0))
						.unwrap_or(false);
				if killed {
					let line = format!("Killed Human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Placing a new item.
		if let Some(id) = self.get_command_num(b"newobj") {
			if (0..TOTAL_PARAMETERINFO_SMALLOBJECT as i32).contains(&id) {
				let (sin, cos) = self.base.camera_rx.sin_cos();
				let (cx, cy, cz) = (self.base.camera_x, self.base.camera_y, self.base.camera_z);
				let dataid = self.world.objects.add_small_object_index(
					cx + cos * 20.0,
					cy,
					cz + sin * 20.0,
					self.base.camera_rx * -1.0,
					id,
					true,
					&self.world.collision,
					&self.world.block_data,
					self.world.mif.as_ref(),
					self.world.resource,
				);
				if dataid != -1 {
					let line = format!("Add SmallObject[{dataid}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Breaking an item.
		if let Some(id) = self.get_command_num(b"break") {
			if (0..MAX_SMALLOBJECT as i32).contains(&id) {
				let enabled = self
					.world
					.objects
					.get_small_object(id)
					.map(|o| o.object.enable_flag())
					.unwrap_or(false);
				if enabled {
					if let Some(object) = self.world.objects.get_small_object_mut(id) {
						object.destruction(self.world.mif.as_ref());
					}

					let line = format!("Broke SmallObject[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Deleting a person, and the weapons they were holding with them.
		if let Some(id) = self.get_command_num(b"delhuman") {
			if (0..MAX_HUMAN as i32).contains(&id) {
				let enabled = self
					.world
					.objects
					.get_human_object(id)
					.map(|h| h.enable_flag())
					.unwrap_or(false);
				if enabled {
					{
						let (humans, weapons) = self.world.objects.humans_and_weapons();
						let (_, held, _, _) = humans[id as usize].get_weapon();
						for slot in held.iter().flatten() {
							weapons[*slot].object.set_enable_flag(false);
						}
						humans[id as usize].set_weapon(
							[None; TOTAL_HAVEWEAPON],
							[0; TOTAL_HAVEWEAPON],
							[0; TOTAL_HAVEWEAPON],
							weapons,
						);
						humans[id as usize].object.set_enable_flag(false);
					}

					let line = format!("Delete human[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Deleting a weapon, which also takes it out of everyone's hands.
		if let Some(id) = self.get_command_num(b"delweapon") {
			if (0..MAX_WEAPON as i32).contains(&id) {
				let enabled = self
					.world
					.objects
					.get_weapon_object(id)
					.map(|w| w.object.enable_flag())
					.unwrap_or(false);
				if enabled {
					{
						let (humans, weapons) = self.world.objects.humans_and_weapons();
						for human in humans.iter_mut() {
							if !human.enable_flag() {
								continue;
							}
							let (_, mut held, lnbs, nbs) = human.get_weapon();
							for slot in held.iter_mut() {
								if *slot == Some(id as usize) {
									*slot = None;
								}
							}
							human.set_weapon(held, lnbs, nbs, weapons);
						}
						weapons[id as usize].object.set_enable_flag(false);
					}

					let line = format!("Delete weapon[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Deleting an item.
		if let Some(id) = self.get_command_num(b"delobj") {
			if (0..MAX_SMALLOBJECT as i32).contains(&id) {
				let enabled = self
					.world
					.objects
					.get_small_object(id)
					.map(|o| o.object.enable_flag())
					.unwrap_or(false);
				if enabled {
					if let Some(object) = self.world.objects.get_small_object_mut(id) {
						object.object.set_enable_flag(false);
					}

					let line = format!("Delete SmallObject[{id}].");
					self.add_info_console(white, line.as_bytes());
				}
			}
		}

		// Friendly fire. Upstream judges the current state from person 0 alone.
		if self.command_is(b"ff") {
			if self.world.objects.get_friendly_fire_flag(0) {
				for i in 0..MAX_HUMAN as i32 {
					self.world.objects.set_friendly_fire_flag(i, false);
				}
				self.add_info_console(white, b"Disable Friendly Fire.");
			} else {
				for i in 0..MAX_HUMAN as i32 {
					self.world.objects.set_friendly_fire_flag(i, true);
				}
				self.add_info_console(white, b"Enable Friendly Fire.");
			}
		}

		// Handing the player's body to the AI.
		if self.command_is(b"bot") {
			if !self.player_ai {
				self.player_ai = true;
				self.add_info_console(white, b"Player is AI control. (bot)");
			} else {
				self.player_ai = false;
				self.add_info_console(white, b"Player is User control.");
			}
		}

		// Making the AI non-combatant.
		if self.command_is(b"nofight") {
			if !self.ai_no_fight {
				self.ai_no_fight = true;
				for i in 0..MAX_HUMAN {
					self.world.ai[i].set_no_fight_flag(true);
				}
				self.add_info_console(white, b"AI had no Fight.");
			} else {
				self.ai_no_fight = false;
				for i in 0..MAX_HUMAN {
					self.world.ai[i].set_no_fight_flag(false);
				}
				self.add_info_console(white, b"AI had Fight.");
			}
		}

		// Making the AI cautious.
		if self.command_is(b"caution") {
			for i in 0..MAX_HUMAN {
				self.world.ai[i].set_caution_mode();
			}
			self.add_info_console(white, b"Set cautious AI.");
		}

		// The AI level. -1 puts every human back on its own setting.
		if let Some(id) = self.get_command_num(b"ailevel") {
			if id == -1 {
				for i in 0..MAX_HUMAN {
					self.world.ai[i].set_ai_level(&self.world.objects, -1);
				}
				self.add_info_console(white, b"Set all human AI-level to default.");
			}
			if (0..TOTAL_PARAMETERINFO_AILEVEL as i32).contains(&id) {
				for i in 0..MAX_HUMAN {
					self.world.ai[i].set_ai_level(&self.world.objects, id);
				}
				let line = format!("Set all human to AI-level [{id}].");
				self.add_info_console(white, line.as_bytes());
			}
		} else if self.command_is(b"ailevel") {
			for i in 0..MAX_HUMAN {
				self.world.ai[i].set_ai_level(&self.world.objects, -1);
			}
			self.add_info_console(white, b"Set all human AI-level to default.");
		}

		// Stopping the AI.
		if self.command_is(b"stop") {
			if !self.ai_stop {
				self.ai_stop = true;
				self.add_info_console(white, b"Stopped AI control.");
			} else {
				self.ai_stop = false;
				self.add_info_console(white, b"Started AI control.");
			}
		}

		// Stopping the event points.
		if self.command_is(b"estop") {
			if !self.event_stop {
				self.event_stop = true;
				self.add_info_console(white, b"Stopped Event chains.");
			} else {
				self.event_stop = false;
				self.add_info_console(white, b"Started Event chains.");
			}
		}

		// The version.
		if self.command_is(b"ver") {
			let line = format!("{}   Version:{}", GAMENAME, GAMEVERSION);
			self.add_info_console(white, line.as_bytes());
			let line = format!(" Graphics : {GRAPHICS_CORE}");
			self.add_info_console(white, line.as_bytes());
			let line = format!(" Sound : {SOUND_CORE}");
			self.add_info_console(white, line.as_bytes());
			let line = format!(" Input : {INPUT_CORE}");
			self.add_info_console(white, line.as_bytes());
		}

		// The game execution speed.
		if self.command_is(b"speed") {
			self.game_speed = if self.game_speed == 1 {
				2
			} else if self.game_speed == 2 {
				4
			} else {
				1
			};

			let line = format!("Set GameSpeed x{}", self.game_speed);
			self.add_info_console(white, line.as_bytes());
		}

		// The window and fullscreen switch.
		if self.command_is(b"window") {
			let flag = !d3dg.get_fullscreen_flag();

			if self.change_window_mode(d3dg, window, input_ctrl, input, config, sound_ctrl, flag) != 0 {
				self.add_info_console(white, b"[Error] Change failed.");
			} else if flag {
				self.add_info_console(white, b"Changed FullScreen mode.");
			} else {
				self.add_info_console(white, b"Changed Window mode.");
			}
		}

		// The screenshot. Upstream delays the capture by two frames so that the console
		// panel is not in the picture, which is the count `render_2d` shares.
		if self.screen_shot == 3 {
			let mut name = [0u8; 256];
			get_time_name(&mut name);
			let mut fname = name[..c_str_len(&name)].to_vec();
			fname.extend_from_slice(b".bmp");

			let path = crate::datafile::path_from_bytes(&fname);
			if d3dg.save_screenshot(&path) {
				let line = format!("Saved Screenshot  (File:{})", String::from_utf8_lossy(&fname));
				self.add_info_console(white, line.as_bytes());
			} else {
				self.add_info_console(white, b"[Error] Save failed.");
			}

			self.screen_shot = 0;
		} else if self.command_is(b"ss") {
			self.screen_shot = 1;
		}

		// Clearing the console.
		if self.command_is(b"clear") {
			for i in 0..MAX_CONSOLELINES {
				self.info_console_data[i].color = white;
				self.info_console_data[i].textdata[0] = 0;
			}
		}

		// Closing the console.
		if self.command_is(b"exit") {
			self.add_info_console(white, b"Closed debug console.");
			self.show_console = false;
		}

		// The reset operation. Upstream guards this one with `_DEBUG`, which the port has
		// no switch for. It is kept because the console exists for debugging and this is
		// the same key F12 reaches from the input phase.
		if self.command_is(b"f12") {
			state.push_f12_key(log);
			self.game_speed = 1;
		}

		if self.command_is(b"mikan") {
			let orange = D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0);
			self.add_info_console(orange, b"Hello! I'm [-_-;](mikan).");
			self.add_info_console(orange, b"Thank you for playing OpenXOPS!");
		}
	}

	/// The upstream `RenderConsole`: the panel and the line being typed.
	fn render_console(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		// The base.
		d3dg.draw_2d_box(
			0,
			0,
			config.get_screen_width(),
			((MAX_CONSOLELINES + 1) * 17 + 5 + 5) as i32,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.75),
		);

		// The lines already printed, each with a drop shadow.
		for i in 0..MAX_CONSOLELINES {
			if self.info_console_data[i].textdata[0] == 0 {
				continue;
			}
			let text = self.info_console_data[i].textdata;
			let color = self.info_console_data[i].color;
			let y = i as i32 * 17 + 5;
			d3dg.draw_2d_texture_debug_font_text(
				5 + 1,
				y + 1,
				&text,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
			);
			d3dg.draw_2d_texture_debug_font_text(5, y, &text, color);
		}

		// The line being typed, on the first free row.
		let mut row = MAX_CONSOLELINES;
		for i in 0..MAX_CONSOLELINES {
			if self.info_console_data[i].textdata[0] == 0 {
				row = i;
				break;
			}
		}

		let mut line = CONSOLE_PROMPT.to_vec();
		line.extend_from_slice(self.console_text_box_control.get_text_box_str());
		let y = row as i32 * 17 + 5;
		d3dg.draw_2d_texture_debug_font_text(
			5 + 1,
			y + 1,
			&line,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
		);
		d3dg.draw_2d_texture_debug_font_text(
			5,
			y,
			&line,
			D3DGraphics::get_color_code(1.0, 1.0, 0.0, 1.0),
		);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::resource::ResourceManager;
	use xops_platform::gfx::Gfx;
	use xops_platform::winit::event::{ElementState, WindowEvent};
	use xops_platform::wgpu;

	/// A 640x480 renderer, which is the size every number below is worked out at.
	fn headless() -> D3DGraphics {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = Gfx::new(&instance, None, 640, 480, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// A world to build the screen against. The resource has to outlive it, so it is
	/// declared first; nothing is leaked, the chain lives in the test's own scope.
	fn world_with_a_resource(resource: &mut ResourceManager) -> Box<World<'_>> {
		World::new(resource)
	}

	/// The shipped settings at the default screen size, which is what the HUD's layout
	/// constants read.
	fn config() -> Config {
		let mut config = Config::new();
		config.set_default_config();
		config.set_default_ext_config();
		// The layout is what is being tested, so the screen it is laid out on is stated
		// rather than inherited from whatever the default happens to be today.
		config.set_screen_size(640, 480);
		config
	}

	/// Holds a control key down in the input table.
	///
	/// A keyboard event cannot be built from here: `winit::event::KeyEvent` has a
	/// `pub(crate)` field, so only winit can make one. `InputControl::set_key_state` is the
	/// test build's door into the table, and it takes the retrieval keycode the game asks
	/// the config for.
	fn press(input_ctrl: &mut InputControl, config: &Config, key: ControlKey) {
		input_ctrl.set_key_state(originalkeycode_to_dinputdef(config.get_keycode(key as i32)), true);
	}

	/// Releases it again, on the frame after the one it was held down on.
	fn release(input_ctrl: &mut InputControl, config: &Config, key: ControlKey) {
		input_ctrl.advance_frame();
		input_ctrl
			.set_key_state(originalkeycode_to_dinputdef(config.get_keycode(key as i32)), false);
	}

	/// Steps to the next frame, which turns what was held into the previous frame's table.
	fn advance(input_ctrl: &mut InputControl) {
		input_ctrl.advance_frame();
	}

	/// Two floats are close enough, which the camera's trigonometry needs.
	fn close(actual: f32, expected: f32) {
		assert!(
			(actual - expected).abs() < 1e-3,
			"{actual} is not {expected}"
		);
	}

	#[test]
	fn a_new_screen_starts_with_every_debug_switch_off() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);

		assert!(!screen.show_info_debugmode);
		assert!(!screen.wireframe);
		assert!(!screen.nomodel);
		assert!(!screen.tag);
		assert!(!screen.radar);
		assert!(!screen.show_console);
		assert!(!screen.event_stop);
		assert_eq!(screen.get_game_speed(), 1);
		assert_eq!(screen.sky_number, 0);
		assert_eq!(screen.message_id, -1);

		// The two that start on: the game begins fogged and blindfolded.
		assert!(screen.fog_flag);
		assert!(screen.camera_blind);
		assert_eq!(screen.world.events[0].get_next_p4(), 0);
	}

	#[test]
	fn the_debug_switches_can_be_turned_on() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen.set_show_info_flag(true);
		screen.wireframe = true;
		screen.nomodel = true;
		assert!(screen.show_info_debugmode);
		assert!(screen.wireframe);
		assert!(screen.nomodel);
	}

	#[test]
	fn the_world_is_built_with_every_human_slot_ready() {
		// Upstream allocates the AI array up front rather than growing it, and the slots
		// are indexed by human id, so the count has to match the manager's.
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		assert_eq!(world.ai.len(), crate::objectmanager::MAX_HUMAN);
		assert_eq!(world.objects.get_total_objects().0, 0);
	}

	// --- the camera ---------------------------------------------------------

	/// The player view puts the eye a little in front of and 19 above the feet, and it
	/// looks along the mouse angle.
	#[test]
	fn the_player_view_sits_on_the_players_head() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen.update_camera(10.0, 5.0, -3.0, false, false);

		// crx = 0 + 0 + PI/2 and cry = 0, so the eye is one tenth along positive Z.
		close(screen.base.camera_rx, std::f32::consts::PI / 2.0);
		close(screen.base.camera_ry, 0.0);
		close(screen.base.camera_x, 10.0);
		close(screen.base.camera_y, 24.0);
		close(screen.base.camera_z, -2.9);
	}

	/// The third person camera trails the look-at point, and with nothing in the way it
	/// sits the whole 14 units back.
	#[test]
	fn the_third_person_camera_trails_the_player() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen.camera_f1mode = true;
		screen.update_camera(10.0, 5.0, -3.0, false, false);

		close(screen.base.camera_rx, std::f32::consts::PI / 10.0);
		close(screen.base.camera_ry, 0.0);
		close(
			screen.base.camera_x,
			10.0 - (std::f32::consts::PI / 10.0).cos() * VIEW_F1MODE_DIST,
		);
		close(screen.base.camera_y, 27.0);
		close(
			screen.base.camera_z,
			-3.0 - (std::f32::consts::PI / 10.0).sin() * VIEW_F1MODE_DIST,
		);
	}

	/// The first person camera's angle is the one the F1 key asks for, and toggling back
	/// clears it.
	#[test]
	fn the_third_person_angle_starts_at_the_upstream_default() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		assert_eq!(screen.view_ry, 0.0);
		screen.view_ry = VIEW_F1MODE_ANGLE;
		assert_eq!(screen.view_ry, degree_to_radian(-22.5));
	}

	/// The death fall fixes the eye above the body and turns it one degree a frame, which
	/// is what makes the body sweep past.
	#[test]
	fn the_death_camera_sweeps_from_above() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen.update_camera(10.0, 5.0, -3.0, true, false);

		close(screen.base.camera_rx, degree_to_radian(1.0));
		close(screen.base.camera_ry, degree_to_radian(-89.0) * 0.05);
		close(screen.base.camera_x, 10.0 + degree_to_radian(1.0).cos() * 3.12);
		close(screen.base.camera_y, 38.3);
		close(screen.base.camera_z, -3.0 + degree_to_radian(1.0).sin() * 3.12);
	}

	/// While the death motion plays the camera holds where it is, and the free camera does
	/// the same for the whole mission.
	#[test]
	fn the_death_motion_and_the_free_camera_leave_the_eye_alone() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen.base.camera_x = 1.0;
		screen.base.camera_y = 2.0;
		screen.base.camera_z = 3.0;
		screen.base.camera_rx = 0.5;
		screen.base.camera_ry = 0.25;

		screen.update_camera(10.0, 5.0, -3.0, false, true);
		assert_eq!(screen.base.camera_x, 1.0);
		assert_eq!(screen.base.camera_y, 2.0);
		assert_eq!(screen.base.camera_z, 3.0);
		assert_eq!(screen.base.camera_rx, 0.5);
		assert_eq!(screen.base.camera_ry, 0.25);

		screen.camera_debugmode = true;
		screen.update_camera(10.0, 5.0, -3.0, false, false);
		assert_eq!(screen.base.camera_x, 1.0);
		assert_eq!(screen.base.camera_rx, 0.5);
	}

	/// The field of view is the scope table's, but the third person and the free camera
	/// both ignore what the player is wearing.
	#[test]
	fn the_third_person_camera_ignores_the_scope() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		// The table: no scope is 65 degrees, the simple scope 30, the precision one 15.
		close(screen.view_angle(0), degree_to_radian(65.0));
		close(screen.view_angle(1), degree_to_radian(30.0));
		close(screen.view_angle(2), degree_to_radian(15.0));
		// An id past the table reads the last entry, which is the no-scope one again.
		close(screen.view_angle(3), degree_to_radian(65.0));

		screen.camera_f1mode = true;
		close(screen.view_angle(2), degree_to_radian(65.0));

		screen.camera_f1mode = false;
		screen.camera_debugmode = true;
		close(screen.view_angle(2), degree_to_radian(65.0));
	}

	// --- the input ----------------------------------------------------------

	/// The movement key the config binds reaches the movement call, which is the whole
	/// point of the control layer.
	#[test]
	fn the_forward_key_is_the_one_the_config_binds() {
		let config = config();
		let mut input_ctrl = InputControl::new();

		// The shipped binding for forward is `0x36`, which is the W key: virtual-key 0x57.
		assert_eq!(config.get_keycode(ControlKey::KEY_MOVEFORWARD as i32), 54);
		assert_eq!(
			originalkeycode_to_dinputdef(config.get_keycode(ControlKey::KEY_MOVEFORWARD as i32)),
			0x57
		);

		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);

		// Nothing is held yet, on a frame the table has been through once.
		advance(&mut input_ctrl);
		assert!(!screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 0));

		// On the frame the key goes down, and only that frame, "went down" is true.
		press(&mut input_ctrl, &config, ControlKey::KEY_MOVEFORWARD);
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 0));
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 1));
		assert!(!screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 2));

		// Held on the next frame: still down, but no longer an edge.
		advance(&mut input_ctrl);
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 0));
		assert!(!screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 1));

		// Released, which is the third mode's edge.
		release(&mut input_ctrl, &config, ControlKey::KEY_MOVEFORWARD);
		assert!(!screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 0));
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 2));
	}

	/// A key the config does not bind to a control has no effect on it.
	#[test]
	fn another_key_does_not_reach_the_control() {
		let config = config();
		let mut input_ctrl = InputControl::new();

		// Jump is on the space bar, so holding W must not reach it.
		assert_eq!(
			originalkeycode_to_dinputdef(config.get_keycode(ControlKey::KEY_JUMP as i32)),
			originalkeycode_to_dinputdef(17)
		);
		press(&mut input_ctrl, &config, ControlKey::KEY_MOVEFORWARD);

		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_MOVEFORWARD, 0));
		assert!(!screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_JUMP, 0));
	}

	/// The mouse's own buttons are the two negative codes, and the left one is what a
	/// binding of `0x12` reaches.
	#[test]
	fn the_left_mouse_button_is_a_binding_like_any_other() {
		let mut config = config();
		let mut input = Input::default();
		let mut input_ctrl = InputControl::new();

		config.set_keycode(ControlKey::KEY_SHOT as i32, 0x12);
		input.handle(&WindowEvent::MouseInput {
			device_id: unsafe { std::mem::zeroed() },
			state: ElementState::Pressed,
			button: xops_platform::winit::event::MouseButton::Left,
		});
		input_ctrl.get_input_state(&mut input, true);

		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_SHOT, 0));
		assert!(screen.check_input_control(&input_ctrl, &config, ControlKey::KEY_SHOT, 1));
	}

	/// The free camera moves along the axis it is looking down, which is the sign the two
	/// forward and back branches have to get right.
	#[test]
	fn the_free_camera_moves_along_its_own_axis() {
		let config = config();
		let mut input_ctrl = InputControl::new();
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		press(&mut input_ctrl, &config, ControlKey::KEY_MOVEFORWARD);
		screen.input_view_camera(0, 0, 0.0, &input_ctrl, &config);

		// The yaw is the camera's own angle, and zero points down positive X: forward is
		// one and a half along +X and nothing moves in Y or Z.
		close(screen.base.camera_x, VIEW_FREECAMERA_SCALE);
		close(screen.base.camera_y, 0.0);
		close(screen.base.camera_z, 0.0);

		// The trigger doubles the step, which is the high speed mode. The trigger is the
		// left mouse button by default, which is its own binding code rather than a key.
		let mut resource2 = ResourceManager::new();
		let world2 = world_with_a_resource(&mut resource2);
		let mut screen2 = MainGame::new(world2);
		let mut input = Input::default();
		input.handle(&WindowEvent::MouseInput {
			device_id: unsafe { std::mem::zeroed() },
			state: ElementState::Pressed,
			button: xops_platform::winit::event::MouseButton::Left,
		});
		let mut input_ctrl2 = InputControl::new();
		input_ctrl2.get_input_state(&mut input, true);
		press(&mut input_ctrl2, &config, ControlKey::KEY_MOVEFORWARD);

		screen2.input_view_camera(0, 0, 0.0, &input_ctrl2, &config);
		close(screen2.base.camera_x, VIEW_FREECAMERA_SCALE * 2.0);
	}

	// --- the console --------------------------------------------------------

	/// The command parser: a name, a space and an integer, and nothing else.
	#[test]
	fn the_console_parser_reads_a_command_and_its_number() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		write_cstr(&mut screen.new_command, b"skip 2");
		assert_eq!(screen.get_command_num(b"skip"), Some(2));

		write_cstr(&mut screen.new_command, b"sky -1");
		assert_eq!(screen.get_command_num(b"sky"), Some(-1));

		write_cstr(&mut screen.new_command, b"kill 0");
		assert_eq!(screen.get_command_num(b"kill"), Some(0));

		write_cstr(&mut screen.new_command, b"aiinfo 5");
		assert_eq!(screen.get_command_num(b"aiinfo"), Some(5));

		// A sign with nothing behind it is `atoi`'s zero.
		write_cstr(&mut screen.new_command, b"ailevel -");
		assert_eq!(screen.get_command_num(b"ailevel"), Some(0));

		// No space, no argument, or the wrong name at all.
		write_cstr(&mut screen.new_command, b"skip");
		assert_eq!(screen.get_command_num(b"skip"), None);

		write_cstr(&mut screen.new_command, b"skipx 1");
		assert_eq!(screen.get_command_num(b"skip"), None);

		write_cstr(&mut screen.new_command, b"skip 2x");
		assert_eq!(screen.get_command_num(b"skip"), None);

		write_cstr(&mut screen.new_command, b"skipped 1");
		assert_eq!(screen.get_command_num(b"skip"), None);

		write_cstr(&mut screen.new_command, b" skiper 1");
		assert_eq!(screen.get_command_num(b"skip"), None);
	}

	/// The dispatch itself: a command that is recognised takes effect and says so, and one
	/// that is not changes nothing and prints nothing.
	#[test]
	fn a_command_that_is_not_recognised_does_nothing() {
		let mut d3dg = headless();
		let config = config();
		let mut sound = SoundManager::new();
		let mut sound_ctrl = SoundControl::new();
		let mut window = WindowControl::new();
		let mut input_ctrl = InputControl::new();
		let mut input = Input::default();
		let mut state = StateMachine::new();
		let mut log = DebugLog::new();

		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);
		screen.show_console = true;

		// Nothing typed yet: the console is empty and every switch is untouched.
		assert_eq!(screen.info_console_data[0].textdata[0], 0);
		write_cstr(&mut screen.new_command, b"nonsense");
		screen.process_console(
			&mut state,
			&mut log,
			&mut d3dg,
			&config,
			&mut sound,
			&mut sound_ctrl,
			&mut window,
			&mut input_ctrl,
			&mut input,
		);
		assert_eq!(screen.info_console_data[0].textdata[0], 0);
		assert!(!screen.radar);
		assert!(!screen.tag);
		assert_eq!(screen.game_speed, 1);

		// `radar` is recognised, flips the switch and prints its line.
		write_cstr(&mut screen.new_command, b"radar");
		screen.process_console(
			&mut state,
			&mut log,
			&mut d3dg,
			&config,
			&mut sound,
			&mut sound_ctrl,
			&mut window,
			&mut input_ctrl,
			&mut input,
		);
		assert!(screen.radar);
		assert_eq!(&screen.info_console_data[0].textdata[..5], b"Show ");
		assert_eq!(&screen.info_console_data[0].textdata[..10], b"Show Radar");

		// A command with a bad argument is as inert as one that is not there at all.
		write_cstr(&mut screen.new_command, b"aiinfo 999");
		screen.process_console(
			&mut state,
			&mut log,
			&mut d3dg,
			&config,
			&mut sound,
			&mut sound_ctrl,
			&mut window,
			&mut input_ctrl,
			&mut input,
		);
		assert_eq!(screen.ai_debuginfo_id, -1);
		assert_eq!(screen.info_console_data[1].textdata[0], 0);
	}

	/// The console's line editor puts what was typed into `new_command`, which is what the
	/// command table reads on the same frame.
	#[test]
	fn the_console_takes_the_typed_line_as_the_command() {
		let mut input_ctrl = InputControl::new();
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(world);

		screen
			.console_text_box_control
			.init_text_box(b"", (MAX_CONSOLELEN - 1) as i32, 0x25);
		screen.console_text_box_control.set_text_box_str(b"radar");

		// [ENTER], which the text box reads as the retrieval keycode of `0x0F`.
		input_ctrl.set_key_state(originalkeycode_to_dinputdef(0x0F), true);

		screen.input_console(&input_ctrl);
		assert!(screen.command_is(b"radar"));
		assert_eq!(&screen.new_command[..5], b"radar");

		// The line was echoed to the console behind its prompt.
		assert_eq!(&screen.info_console_data[0].textdata[..6], b">radar");
		// And the box was emptied for the next one.
		assert!(screen.console_text_box_control.get_text_box_str().is_empty());
	}

	// --- the radar ----------------------------------------------------------

	/// A point on the radar's edge lands on the radar's edge, and one past it is refused.
	#[test]
	fn the_radar_maps_its_own_radius_to_its_own_edge() {
		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);

		// The radar is 200 across and covers 300 world units, so half of it is 100 pixels
		// and the camera is at the origin facing down positive X.
		let radar = |x, z, check| screen.get_radar_pos(x, 0.0, z, 0, 0, 200, 300.0, check);

		// Straight ahead: the point is on the radar's middle row and half way right.
		assert_eq!(radar(150.0, 0.0, true), Some((100, 50, 0.0)));
		// At the coverage radius: the plotted distance lands on the edge.
		assert_eq!(radar(200.0, 0.0, true), Some((100, 33, 0.0)));
		// Past it, which the fit test refuses.
		assert_eq!(radar(400.0, 0.0, true), None);
		// The same point without the test is still plotted, which is how the map's own
		// lines are drawn.
		assert!(radar(400.0, 0.0, false).is_some());

		// Too high above the camera, which the first test refuses.
		assert_eq!(
			screen.get_radar_pos(0.0, 100.0, 0.0, 0, 0, 200, 300.0, true),
			None
		);
	}

	/// The radar's corner and size come from the screen size, and the corner is where the
	/// map's own clipping starts.
	#[test]
	fn the_radar_sits_at_the_top_right_corner() {
		let config = config();
		let radar_size = 200;
		let radar_pos_x = config.get_screen_width() - radar_size - 10;
		assert_eq!(radar_pos_x, 430);
		assert_eq!(radar_pos_x + radar_size, 630);

		let mut resource = ResourceManager::new();
		let world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(world);

		// A point the radar covers, plotted from the radar's own corner.
		assert!(screen
			.get_radar_pos(0.0, 0.0, 0.0, radar_pos_x, 110, radar_size, 300.0, true)
			.is_some());
		// Well outside it.
		assert_eq!(
			screen.get_radar_pos(1000.0, 0.0, 0.0, radar_pos_x, 110, radar_size, 300.0, true),
			None
		);
	}

	// --- the HUD ------------------------------------------------------------

	/// The mode A panels' geometry at the default screen size, which is what the weapon
	/// panel and the two status boxes line up on.
	#[test]
	fn the_hud_layout_is_worked_out_from_the_screen_size() {
		let config = config();
		let layout = HudaLayout::new(&config);

		// 640x480.
		assert_eq!(layout.left_x, 15);
		assert_eq!(layout.left_y, 375);
		assert_eq!(layout.right_x, 15);
		assert_eq!(layout.right_y, 425);
		assert_eq!(layout.weapon_x, 385);
		assert_eq!(layout.weapon_y, 382);
		assert_eq!(layout.ammo_x, 25);
		assert_eq!(layout.ammo_y, 384);
		assert_eq!(layout.state_x, 23);
		assert_eq!(layout.state_y, 435);

		// The lower left box is two rows of one 32 pixel cell, and the lower right box
		// starts exactly one cell too low for the weapon panel's height.
		assert_eq!(layout.left_y + 2 * HUDA_CELL, 439);
		assert_eq!(layout.right_y + 2 * HUDA_CELL, 489);
		// The panel is eight cells wide and three tall, so its bottom is two pixels above
		// the screen's own edge.
		assert_eq!(layout.weapon_y + HUDA_WEAPON_SIZEH * HUDA_CELL, 478);
		// Its right edge is one pixel past a 640 wide screen, which is what the upstream
		// tiles add up to.
		assert_eq!(layout.weapon_x + HUDA_WEAPON_SIZEW * HUDA_CELL, 641);
	}

	/// The panel edges are the upstream's own half-width katakana, with the same glyphs on
	/// the row below.
	#[test]
	fn the_hud_panel_edges_are_the_upstream_glyphs() {
		assert_eq!(HUDA_LEFT_EDGE.len(), 8);
		assert_eq!(HUDA_LEFT_EDGE[0], 0xB3);
		assert_eq!(HUDA_LEFT_EDGE[7], 0xB5);

		let mut edge = HUDA_LEFT_EDGE.to_vec();
		huda_next_row(&mut edge);
		assert_eq!(edge[0], 0xC3);
		assert_eq!(edge[7], 0xC5);

		// The weapon panel is `0xB0`, a run of `0xB1`, then `0xB2`.
		let edge = huda_weapon_edge();
		assert_eq!(edge.len(), HUDA_WEAPON_SIZEW as usize);
		assert_eq!(edge[0], 0xB0);
		assert_eq!(edge[1], 0xB1);
		assert_eq!(edge[6], 0xB1);
		assert_eq!(edge[7], 0xB2);

		// The ammo readout spells its two letters in the same katakana.
		let mut ammo = b"A3 B12".to_vec();
		for byte in ammo.iter_mut() {
			if *byte == b'A' {
				*byte = 0xBB;
			}
			if *byte == b'B' {
				*byte = 0xBA;
			}
		}
		assert_eq!(ammo, vec![0xBB, b'3', b' ', 0xBA, b'1', b'2']);
	}

}
