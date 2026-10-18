//! The game's constants and shared structures, from the upstream `gamemain.h`.
//!
//! The screens themselves are in `gamemenu`, `briefing` and `maingame`. This module holds
//! what they share: the layout constants, the mission result structure, and the two small
//! structures the menu and the debug console use. They are here rather than beside each
//! screen because the header declares them once and every screen reads them.

use crate::collision::Collision;
use crate::d3dgraphics::D3DGraphics;
use crate::input::InputControl;
use crate::config::Config;
use crate::core::degree_to_radian;
use crate::datafile::{AddonList, BlockDataInterface, MIFInterface, PointDataInterface};
use crate::parameter::ParameterInfo;
use crate::resource::ResourceManager;
use crate::sound::SoundControl;
use crate::statemachine::StateMachine;

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// `TOTAL_MENUITEMS`: how many missions fit on one menu screen.
pub const TOTAL_MENUITEMS: i32 = 8;

/// `MAINMENU_X`: the menu's left edge, from the screen width. Upstream this is a macro
/// evaluated at each use, so it follows a screen size change.
pub fn mainmenu_x(config: &Config) -> i32 {
	config.get_screen_width() - 360
}

/// `MAINMENU_Y`.
pub fn mainmenu_y(config: &Config) -> i32 {
	config.get_screen_height() - 340
}

/// `MAINMENU_H`: the menu's height, which depends only on the item count.
pub const MAINMENU_H: i32 = (TOTAL_MENUITEMS + 2) * 30 + 25;

/// `MAINMENU_OPTION_X`.
pub const MAINMENU_OPTION_X: i32 = 10;

/// `MAINMENU_OPTION_Y`.
pub fn mainmenu_option_y(config: &Config) -> i32 {
	mainmenu_y(config) + MAINMENU_H - 16
}

/// `HUDA_WEAPON_POSX`: the weapon panel's left edge.
pub fn huda_weapon_posx(config: &Config) -> i32 {
	config.get_screen_width() - 255
}

/// `HUDA_WEAPON_POSY`.
pub fn huda_weapon_posy(config: &Config) -> i32 {
	config.get_screen_height() - 98
}

/// `HUDA_WEAPON_SIZEW`: the weapon panel's width in 32 pixel tiles.
pub const HUDA_WEAPON_SIZEW: i32 = 8;
/// `HUDA_WEAPON_SIZEH`: its height in 32 pixel tiles.
pub const HUDA_WEAPON_SIZEH: i32 = 3;

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

/// `VIEW_HEIGHT`: how far the eye sits above the player's feet.
pub const VIEW_HEIGHT: f32 = 19.0;
/// `VIEW_DIST`: how far the viewpoint sits in front of the player.
pub const VIEW_DIST: f32 = 0.1;
/// `VIEW_F1MODE_ANGLE`: the third person camera's initial angle.
pub const VIEW_F1MODE_ANGLE: f32 = degree_to_radian(-22.5);
/// `VIEW_F1MODE_DIST`: its maximum distance.
pub const VIEW_F1MODE_DIST: f32 = 14.0;
/// `VIEW_FREECAMERA_SCALE`: free camera movement speed, doubled in high speed mode.
pub const VIEW_FREECAMERA_SCALE: f32 = 1.5;
/// `VIEWANGLE_NORMAL`: the standard field of view.
pub const VIEWANGLE_NORMAL: f32 = degree_to_radian(65.0);

/// `INPUT_ARROWKEYS_ANGLE`: how far a direction key turns the view.
pub const INPUT_ARROWKEYS_ANGLE: f32 = degree_to_radian(4.0);
/// `INPUT_F1NUMKEYS_ANGLE`: how far the numeric keypad turns it in third person.
pub const INPUT_F1NUMKEYS_ANGLE: f32 = degree_to_radian(2.0);

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// `TOTAL_EVENTLINE`: how many event lines run at once.
pub const TOTAL_EVENTLINE: usize = 3;
pub const TOTAL_EVENTENTRYPOINT_0: i32 = -100;
pub const TOTAL_EVENTENTRYPOINT_1: i32 = -110;
pub const TOTAL_EVENTENTRYPOINT_2: i32 = -120;
/// `TOTAL_EVENTENT_SHOWMESSEC`: how long an event message stays up.
pub const TOTAL_EVENTENT_SHOWMESSEC: f32 = 5.0;

// ---------------------------------------------------------------------------
// Options screens
// ---------------------------------------------------------------------------

/// `OPTIONS_P1_*`: the first options page, a six row list.
pub const OPTIONS_P1_W: i32 = 340;
pub const OPTIONS_P1_H: i32 = 30 * 6;
pub const OPTIONS_P1_DATAS: usize = 3;
pub fn options_p1_x(config: &Config) -> i32 {
	config.get_screen_width() - 360
}
pub fn options_p1_y(config: &Config) -> i32 {
	mainmenu_y(config) + MAINMENU_H - OPTIONS_P1_H - 24
}

/// `OPTIONS_P2_*`: the second page, the key bindings.
pub const OPTIONS_P2_W: i32 = 600;
pub const OPTIONS_P2_H: i32 = 360;
pub const OPTIONS_P2_DATAS: usize = 32;
pub fn options_p2_x(config: &Config) -> i32 {
	(config.get_screen_width() - OPTIONS_P2_W) / 2
}
pub fn options_p2_y(config: &Config) -> i32 {
	105 + (config.get_screen_height() - 105 - OPTIONS_P2_H) / 2
}

/// `OPTIONS_P3_*`: the third page, the licence text.
pub const OPTIONS_P3_W: i32 = 600;
pub const OPTIONS_P3_H: i32 = 320;
pub const OPTIONS_P3_DATAS: usize = 1;
pub fn options_p3_x(config: &Config) -> i32 {
	(config.get_screen_width() - OPTIONS_P3_W) / 2
}
pub fn options_p3_y(config: &Config) -> i32 {
	105 + (config.get_screen_height() - 105 - OPTIONS_P3_H) / 2
}

/// `OPTIONS_LICENSELINE`: how many lines the licence text occupies.
///
/// Upstream this differs by graphics core, 26 for DirectX and 6 for OpenGL, because the
/// licence text names the libraries the build links. This port is the OpenGL side of that
/// branch, so it is 6.
pub const OPTIONS_LICENSELINE: usize = 6;

// ---------------------------------------------------------------------------
// Debug console
// ---------------------------------------------------------------------------

/// `MAX_CONSOLELEN`: characters per console line.
pub const MAX_CONSOLELEN: usize = 78;
/// `MAX_CONSOLELINES`: lines the console keeps.
pub const MAX_CONSOLELINES: usize = 12;
/// `CONSOLE_PROMPT`.
pub const CONSOLE_PROMPT: &[u8] = b">";

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// The texture font the HUD, the menus and the console draw with. Upstream
/// `InitGame` passes this to the renderer's initialisation.
pub const TEXTURE_FONT: &[u8] = b"data\\char.dds";

/// `InitGame`: loads everything a session needs and returns the world the screens share.
///
/// Upstream this also creates the renderer, the window, the input class and the sound
/// class. The window has to exist before a wgpu surface can, and the device before the
/// renderer, so the host makes them and hands them in. What is left is what the game
/// itself decides: the resource load, the input and sound setup, and either the mission it
/// was asked to start or the list of addons.
///
/// The sound class is taken rather than the sound manager, because the manager borrows the
/// resource and the world holds that mutably. The scheduler builds the manager for the
/// frame and passes it to the screen.
pub fn init_game<'a>(
	config: &Config,
	d3dg: &mut D3DGraphics,
	input: &mut InputControl,
	sound_ctrl: &mut SoundControl,
	resource: &'a mut ResourceManager,
	state: &mut StateMachine,
	mode: i32,
	mif_path: Option<&[u8]>,
) -> Result<World<'a>, String> {
	if d3dg.init_d3d(TEXTURE_FONT, config.get_language()) != 0 {
		return Err("the texture font did not load".to_string());
	}

	// The setting tables are constants, so the resource only needs the handle. Everything
	// else is loaded once here and lives for the session, which is why the world borrows the
	// resource rather than each screen reloading it.
	resource.set_parameter_info(ParameterInfo::new());
	resource.load_human_model(d3dg);
	resource.load_weapon_model_texture(d3dg);
	resource.load_weapon_sound(sound_ctrl);
	resource.load_small_object_model_texture(d3dg);
	resource.load_small_object_sound(sound_ctrl);
	resource.load_scope_texture(d3dg);
	resource.load_bullet_model_texture(d3dg);
	resource.load_bullet_sound(sound_ctrl);
	resource.load_effect_texture(d3dg);

	input.set_mouse_limit_flag(config.get_mouse_limit_flag());

	// `ENABLE_ADDCONFIG` is on, so the saved volume is used rather than a constant. A sound
	// flag of false means silent whatever the volume says.
	let volume = if config.get_sound_flag() {
		config.get_volume()
	} else {
		0.0
	};
	sound_ctrl.set_volume(volume);

	let mut world = World::new(resource);

	match (mode, mif_path) {
		// A mission was named on the command line, so the addon list is not needed: nothing
		// will show the menu it feeds.
		(mode, Some(path)) if mode != 0 => {
			world.game_info.selectmission_id = -1;
			let length = path.len().min(crate::core::MAX_PATH - 1);
			world.game_info.mifpath[..length].copy_from_slice(&path[..length]);
			world.game_info.mifpath[length] = 0;
			state.set_state_mode(mode);

			// Mode 2 starts in the mission, so the mif is read here. Mode 1 starts at the
			// briefing, which reads it itself.
			if mode == 2 {
				let mut mif = MIFInterface::new();
				if mif.load_file_data(path, true) != 0 {
					return Err("the briefing data did not load".to_string());
				}
				world.mif = Some(mif);
			}
		}
		// Otherwise the menu starts, and the addon list is what it lists beside the official
		// missions.
		_ => {
			world.addon.load_file_data(b"addon\\");
			world.game_info.selectmission_id = 0;
			world.game_info.mifpath = [0; crate::core::MAX_PATH];
		}
	}

	Ok(world)
}

/// The state that lives for a whole game session, shared by whichever screen is on screen.
///
/// Upstream these are file scope objects that `InitGame` fills in and `CleanupGame`
/// empties, and the screens reach them through pointers. Here they are one structure that
/// the scheduler owns and lends to the current screen, which is the faithful translation:
/// a screen holds `&mut World` and reaches the mission through it, exactly as it reached
/// the globals.
///
/// Only one screen exists at a time, which is what makes the borrow workable: the
/// scheduler builds a screen when it enters `STATE_CREATE_*` and drops it at
/// `STATE_DESTROY_*`, so there is never a second one holding the same borrow.
///
/// The collaborators that live longer than the world are held by reference. The resource
/// manager is loaded once at startup and is read only from here on, and the sound manager
/// borrows the sound class plus the resource and the setting tables, so bundling them here
/// keeps a screen down to a single lifetime parameter.
pub struct World<'a> {
	/// The humans, weapons, small objects, bullets, grenades and effects.
	pub objects: crate::objectmanager::ObjectManager,
	/// One AI per human slot, indexed by the human's id. Upstream a file scope array,
	/// `HumanAI[MAX_HUMAN]`, which the game screen alone drives.
	pub ai: Vec<crate::ai::AIcontrol>,
	pub block_data: BlockDataInterface,
	pub point_data: PointDataInterface,
	pub mif: Option<MIFInterface>,
	pub collision: Collision,
	/// The setting tables. A zero-sized handle, so a copy here costs nothing.
	pub param: ParameterInfo,
	/// The mission selection and its result. Upstream `GameInfoData`, which the menu fills
	/// in, the game screen updates and the result screen reads after the game screen is
	/// gone. That last part is why it belongs here rather than on the game screen.
	pub game_info: GameInfo,
	/// The addon mission list. Upstream `GameAddon`, a file scope object loaded once at
	/// startup and read only by the menu, which reaches it from a `render_2d` hook whose
	/// signature is fixed, so it has to be reachable from the screen.
	pub addon: AddonList,
	/// Mutable because loading a mission mutates it: the models, textures and sounds are
	/// cached inside it, and a screen loads and releases the sky through it on entry and
	/// exit. The sound manager borrows the same value, which is why the manager is not held
	/// here: a structure cannot lend one field both mutably and immutably. The scheduler
	/// builds the manager and passes it to the screen instead.
	pub resource: &'a mut ResourceManager,
}

impl<'a> World<'a> {
	/// Builds the mission state. The resource arrives as a mutable borrow because a screen
	/// loads and unloads mission assets through it.
	pub fn new(resource: &'a mut ResourceManager) -> World<'a> {
		World {
			objects: crate::objectmanager::ObjectManager::new(),
			// One per human slot, which is what the upstream array holds from the start
			// rather than growing as humans are added.
			ai: (0..crate::objectmanager::MAX_HUMAN as i32)
				.map(crate::ai::AIcontrol::new)
				.collect(),
			block_data: BlockDataInterface::new(),
			point_data: PointDataInterface::new(),
			mif: None,
			collision: Collision::new(),
			param: ParameterInfo::new(),
			game_info: GameInfo::default(),
			addon: AddonList::new(),
			resource,
		}
	}
}

/// `GameInfo`: what a mission reports when it ends.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GameInfo {
	pub selectmission_id: i32,
	/// The path of the `.mif` that was played. A fixed buffer upstream.
	pub mifpath: [u8; crate::core::MAX_PATH],
	pub missioncomplete: bool,
	pub framecnt: u32,
	/// Shots fired.
	pub fire: i32,
	/// Shots that hit.
	pub ontarget: f32,
	/// Enemies defeated.
	pub kill: i32,
	/// Head hits.
	pub headshot: i32,
}

impl Default for GameInfo {
	fn default() -> Self {
		GameInfo {
			selectmission_id: 0,
			mifpath: [0; crate::core::MAX_PATH],
			missioncomplete: false,
			framecnt: 0,
			fire: 0,
			ontarget: 0.0,
			kill: 0,
			headshot: 0,
		}
	}
}

/// `MenuLinkTextData`: one selectable line on an options page.
#[derive(Copy, Clone, Debug)]
pub struct MenuLinkTextData {
	pub enable: bool,
	pub pos_x: i32,
	pub pos_y: i32,
	pub text_w: i32,
	pub text_h: i32,
	pub text_defaultcolor: u32,
	pub text_cursorcolor: u32,
	pub text: [u8; 32],
}

impl Default for MenuLinkTextData {
	fn default() -> Self {
		MenuLinkTextData {
			enable: false,
			pos_x: 0,
			pos_y: 0,
			text_w: 0,
			text_h: 0,
			text_defaultcolor: 0,
			text_cursorcolor: 0,
			text: [0; 32],
		}
	}
}

/// `ConsoleData`: one line of the debug console.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConsoleData {
	pub color: u32,
	/// The line's text, NUL padded.
	pub textdata: [u8; MAX_CONSOLELEN + 1],
}

impl Default for ConsoleData {
	fn default() -> Self {
		ConsoleData {
			color: 0,
			textdata: [0; MAX_CONSOLELEN + 1],
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_menu_height_follows_the_item_count() {
		// (TOTAL_MENUITEMS + 2) * 30 + 25, which is the height the menu border is drawn at.
		assert_eq!(MAINMENU_H, (8 + 2) * 30 + 25);
		assert_eq!(MAINMENU_H, 325);
	}

	#[test]
	fn the_view_angles_are_the_upstream_degrees_in_radians() {
		assert!((VIEWANGLE_NORMAL - 65.0f32.to_radians()).abs() < 1e-6);
		assert!((VIEW_F1MODE_ANGLE - (-22.5f32).to_radians()).abs() < 1e-6);
		assert!((INPUT_ARROWKEYS_ANGLE - 4.0f32.to_radians()).abs() < 1e-6);
		assert!((INPUT_F1NUMKEYS_ANGLE - 2.0f32.to_radians()).abs() < 1e-6);
	}

	#[test]
	fn the_event_entry_points_are_the_upstream_identifiers() {
		// The point data uses these three numbers to start an event line, so a wrong value
		// silently drops every event in a mission.
		assert_eq!(TOTAL_EVENTENTRYPOINT_0, -100);
		assert_eq!(TOTAL_EVENTENTRYPOINT_1, -110);
		assert_eq!(TOTAL_EVENTENTRYPOINT_2, -120);
	}

	#[test]
	fn the_game_info_starts_empty() {
		let info = GameInfo::default();
		assert_eq!(info.selectmission_id, 0);
		assert!(!info.missioncomplete);
		assert_eq!(info.framecnt, 0);
		assert_eq!(info.fire, 0);
		assert_eq!(info.kill, 0);
		assert_eq!(info.headshot, 0);
		assert_eq!(info.ontarget, 0.0);
	}

	#[test]
	fn the_licence_line_count_is_the_open_gl_branch() {
		// Upstream has two values, 26 for DirectX and 6 for OpenGL, chosen by the graphics
		// core this port came from. Picking the other one clips the licence screen.
		assert_eq!(OPTIONS_LICENSELINE, 6);
	}
}
