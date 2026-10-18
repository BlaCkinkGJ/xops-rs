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

use crate::gamemain::{ConsoleData, GameInfo, World, MAX_CONSOLELEN, MAX_CONSOLELINES, TOTAL_EVENTLINE};
use crate::gui_object::TextBoxObject;
use crate::scene::D3Dscene;

/// The main game screen.
pub struct MainGame<'a> {
	pub base: D3Dscene,
	/// The mission, shared with whichever screen is on screen.
	pub world: &'a mut World<'a>,

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
	/// The lines already printed, newest first.
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
	pub fn new(world: &'a mut World<'a>) -> MainGame<'a> {
		MainGame {
			base: D3Dscene::new(),
			world,
			sky_number: 0,
			dark_screen_flag: false,
			fog_flag: false,
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
			camera_blind: false,
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
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::resource::ResourceManager;

	/// A world to build the screen against. The resource has to outlive it, so it is
	/// declared first; nothing is leaked, the chain lives in the test's own scope.
	fn world_with_a_resource(resource: &mut ResourceManager) -> World<'_> {
		World::new(resource)
	}

	#[test]
	fn a_new_screen_starts_with_every_debug_switch_off() {
		let mut resource = ResourceManager::new();
		let mut world = world_with_a_resource(&mut resource);
		let screen = MainGame::new(&mut world);

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
	}

	#[test]
	fn the_debug_switches_can_be_turned_on() {
		let mut resource = ResourceManager::new();
		let mut world = world_with_a_resource(&mut resource);
		let mut screen = MainGame::new(&mut world);

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
}
