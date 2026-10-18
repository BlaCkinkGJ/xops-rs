//! The screen scheduler: the upstream `InitScreen` and `ProcessScreen` from
//! `gamemain.cpp`, which switch on the state machine and drive the six screens.
//!
//! Upstream there is one object per screen and a world at file scope, and `ProcessScreen`
//! reads `GameState.GetState()` to run the phase the screen asked for. The shape is the
//! same here, with one difference the types force: a screen is alive for many frames and
//! works on the world, so the world cannot be borrowed by the screen and also live beside
//! it. Each screen owns it while it runs, and it moves between them.
//!
//! The opening, the menu and the game screen reach into the world throughout, so they take
//! it when they are built. The briefing and the result screen read it only while being
//! created, so they hold it beside them.

use crate::briefing::{Briefing, Result};
use crate::config::Config;
use crate::datafile::MIFInterface;
use crate::debug::DebugLog;
use crate::d3dgraphics::D3DGraphics;
use crate::gamemain::World;
use crate::gamemenu::{MainMenu, Opening};
use crate::input::InputControl;
use crate::maingame::MainGame;
use crate::sound::SoundControl;
use crate::soundmanager::SoundManager;
use crate::statemachine::{State, StateMachine};
use crate::window::WindowControl;
use xops_platform::input::Input;

/// The screen that is up, holding the world it is working on.
///
/// Exactly one of these holds the world at any time, and a screen change moves it rather
/// than copying it. [`Stage::Idle`] is where it sits between screens, which is also where
/// it starts.
pub enum Stage<'a> {
	/// No screen up. A destroy state leaves the world here for the create state that
	/// follows, which is what makes the state machine's two steps safe.
	Idle(Box<World<'a>>),
	Opening(Opening<'a>),
	Menu(MainMenu<'a>),
	Briefing(Briefing, Box<World<'a>>),
	MainGame(MainGame<'a>),
	Result(Result, Box<World<'a>>),
}

impl<'a> Stage<'a> {
	/// The world back, so the next screen can have it. Consumes the stage, since the world
	/// was its only reason to exist.
	pub fn into_world(self) -> Box<World<'a>> {
		match self {
			Stage::Idle(world) => world,
			Stage::Opening(screen) => screen.into_world(),
			Stage::Menu(screen) => screen.into_world(),
			Stage::Briefing(_, world) => world,
			Stage::MainGame(screen) => screen.into_world(),
			Stage::Result(_, world) => world,
		}
	}
}

/// Not reachable: the caller hands a stage in, and every state above leaves one behind.
const STAGE: &str = "the scheduler always holds a stage";

/// The world out of the stage, for a state that is about to build the next screen from it.
fn take_world<'a>(stage: &mut Option<Stage<'a>>) -> Box<World<'a>> {
	stage.take().expect(STAGE).into_world()
}

/// Whether this frame draws, which is the upstream frame skip: an odd frame is skipped when
/// the setting is on.
fn draws_this_frame(config: &Config, framecnt: u32) -> bool {
	!config.get_frameskip_flag() || framecnt % 2 == 0
}

/// Upstream `ResetGame`: the renderer has been rebuilt and everything on it is gone, so the
/// textures are reloaded.
///
/// Upstream reports three codes, recovered, busy and failed, because losing a Direct3D
/// device is routine. There is no device to lose here: the surface is reconfigured by
/// [`D3DGraphics::set_size`] when the window changes, and a swap chain the driver has
/// dropped comes back from the same call. So this is called and always succeeds, which is
/// the answer that makes the caller reload, and the reload is the part that matters.
///
/// The order matters: the human texture list is released first because it holds texture ids
/// that unloading below would otherwise release twice.
pub fn reset_game(world: &mut World, d3dg: &mut D3DGraphics) {
	world.resource.cleanup_human_texture(d3dg);
	world.resource.load_human_model(d3dg);
	world.resource.load_weapon_model_texture(d3dg);
	world.resource.load_small_object_model_texture(d3dg);
	world.resource.load_scope_texture(d3dg);
	world.resource.load_bullet_model_texture(d3dg);
	world.resource.load_effect_texture(d3dg);
}

/// Upstream `ProcessScreen`: run the phase of the screen that the state machine is asking
/// for.
///
/// `stage` is an argument rather than a local because the world has to outlive the call.
///
/// The state machine guarantees the pairing, that a destroy state follows the now state of
/// the same screen, which is the same assurance upstream gets from its six separate
/// pointers. A mismatched stage is dropped rather than handled, since there is no sane way
/// to continue from it.
#[allow(clippy::too_many_arguments)]
pub fn process_screen<'a>(
	stage: &mut Option<Stage<'a>>,
	state: &mut StateMachine,
	window: &mut WindowControl,
	config: &mut Config,
	input_ctrl: &mut InputControl,
	input: &mut Input,
	d3dg: &mut D3DGraphics,
	sound: &mut SoundManager,
	sound_ctrl: &mut SoundControl,
	log: &mut DebugLog,
	framecnt: u32,
) {
	match state.get_state() {
		// -- the opening screen -------------------------------------------------

		State::STATE_CREATE_OPENING => {
			let mut screen = Opening::new(take_world(stage));
			let error = screen.create(state, window, config, input_ctrl, log, d3dg, sound);
			if error != 0 {
				// Bit 0 is the map, bit 1 is the objects, the same two bits upstream reads.
				if error & 0x0001 != 0 {
					window.error_info(b"block data open failed");
				}
				if error & 0x0002 != 0 {
					window.error_info(b"point data open failed");
				}
				// Upstream has the close commented out here and steps the state back
				// instead, so a failed opening returns where it came from.
				state.push_back_space_key(log);
			}
			*stage = Some(Stage::Opening(screen));
		}
		State::STATE_NOW_OPENING => {
			if let Some(Stage::Opening(screen)) = stage {
				screen.input(input_ctrl, input, state, log);
				screen.process(state, log, d3dg, config, sound);
				screen.sound(sound, sound_ctrl);
				if draws_this_frame(config, framecnt) && screen.render_main(d3dg, config) {
					reset_game(&mut screen.world, d3dg);
					screen.recovery(d3dg);
				}
			}
		}
		State::STATE_DESTROY_OPENING => {
			if let Some(Stage::Opening(mut screen)) = stage.take() {
				screen.destroy(state, log, d3dg);
				*stage = Some(Stage::Idle(screen.into_world()));
			}
		}

		// -- the menu screen ----------------------------------------------------

		State::STATE_CREATE_MENU => {
			let mut screen = MainMenu::new(take_world(stage));
			let error = screen.create(state, window, config, input_ctrl, log, d3dg, sound);
			if error != 0 {
				if error & 0x0001 != 0 {
					window.error_info(b"block data open failed");
				}
				if error & 0x0002 != 0 {
					window.error_info(b"point data open failed");
				}
				// The menu closes where the opening steps back, which is upstream's
				// difference between the two rather than an oversight.
				window.close_window();
			}
			*stage = Some(Stage::Menu(screen));
		}
		State::STATE_NOW_MENU => {
			if let Some(Stage::Menu(screen)) = stage {
				screen.input(input_ctrl, input, state, log, config);
				screen.process(d3dg, config, window, input_ctrl, input, log, sound, sound_ctrl);
				screen.sound(sound, sound_ctrl);
				if draws_this_frame(config, framecnt) && screen.render_main(d3dg, config) {
					reset_game(&mut screen.world, d3dg);
					screen.recovery(d3dg);
				}
			}
		}
		State::STATE_DESTROY_MENU => {
			if let Some(Stage::Menu(mut screen)) = stage.take() {
				screen.destroy(state, log, d3dg);
				*stage = Some(Stage::Idle(screen.into_world()));
			}
		}

		// -- the briefing screen ------------------------------------------------

		State::STATE_CREATE_BRIEFING => {
			// The world is read here and then kept beside the screen, because the screen
			// does not reach into it again.
			let mut world = take_world(stage);
			let mut screen = Briefing::new();
			// The briefing reads the mission itself, from the file the menu chose or from
			// the parameter tables for an official one, so a world that has not read a
			// mission yet is not a precondition: it is the normal state coming from the menu.
			//
			// This used to skip the create in that case, which skipped the state machine
			// step the create ends with, so the state stayed on create and the screen was
			// rebuilt every frame. The window kept showing the last frame it had drawn and
			// answered nothing, which is what a player sees as the game freezing on the
			// mission they picked.
			let mif = world.mif.get_or_insert_with(MIFInterface::new);
			let error = screen.create(
				state,
				window,
				config,
				d3dg,
				input_ctrl,
				log,
				&world.game_info,
				&world.param,
				mif,
			);
			*stage = Some(Stage::Briefing(screen, world));
			if error == 1 {
				window.error_info(b"briefing data open failed");
				window.close_window();
			}
		}
		State::STATE_NOW_BRIEFING => {
			if let Some(Stage::Briefing(screen, world)) = stage {
				screen.input(input_ctrl, input, state, log);
				screen.process();
				if draws_this_frame(config, framecnt) && screen.render_main(d3dg, config) {
					reset_game(world, d3dg);
					// The briefing's recovery reads the mission file, so it needs the one
					// the world is holding.
					if let Some(mif) = world.mif.as_ref() {
						screen.recovery(d3dg, mif);
					}
				}
			}
		}
		State::STATE_DESTROY_BRIEFING => {
			if let Some(Stage::Briefing(mut screen, world)) = stage.take() {
				screen.destroy(d3dg, state, log);
				*stage = Some(Stage::Idle(world));
			}
		}

		// -- the game screen ----------------------------------------------------

		State::STATE_CREATE_MAINGAME => {
			let mut screen = MainGame::new(take_world(stage));
			let error = screen.create(state, window, config, input_ctrl, log, d3dg, sound, sound_ctrl);
			if error != 0 {
				if error & 0x0001 != 0 {
					window.error_info(b"block data open failed");
				}
				if error & 0x0002 != 0 {
					window.error_info(b"point data open failed");
				}
				window.close_window();
			}
			*stage = Some(Stage::MainGame(screen));
		}
		State::STATE_NOW_MAINGAME => {
			if let Some(Stage::MainGame(screen)) = stage {
				// The game speed repeats the three phases inside one drawn frame, which
				// is where the speed console command takes effect.
				for _ in 0..screen.get_game_speed() {
					screen.input(input_ctrl, input, state, log, window, config, d3dg, sound);
					screen.process(
						state, log, d3dg, config, window, input_ctrl, input, sound, sound_ctrl,
					);
					screen.sound(sound, sound_ctrl);
				}
				if draws_this_frame(config, framecnt) && screen.render_main(d3dg, config) {
					reset_game(&mut screen.world, d3dg);
					screen.recovery(d3dg, sound_ctrl);
				}
			}
		}
		State::STATE_DESTROY_MAINGAME => {
			if let Some(Stage::MainGame(mut screen)) = stage.take() {
				screen.destroy(state, log, d3dg, sound_ctrl);
				*stage = Some(Stage::Idle(screen.into_world()));
			}
		}

		// -- the result screen --------------------------------------------------

		State::STATE_CREATE_RESULT => {
			// No error handling upstream either: the result screen has nothing to open.
			let mut world = take_world(stage);
			let mut screen = Result::new();
			// The same as the briefing: the mission is read here, so one that has not been
			// read yet is not a reason to skip, and skipping would skip its state step.
			let mif = world.mif.get_or_insert_with(MIFInterface::new);
			screen.create(state, d3dg, log, &world.game_info, &world.param, mif);
			*stage = Some(Stage::Result(screen, world));
		}
		State::STATE_NOW_RESULT => {
			if let Some(Stage::Result(screen, world)) = stage {
				screen.input(input_ctrl, input, state, log);
				screen.process();
				if draws_this_frame(config, framecnt) && screen.render_main(d3dg, config) {
					reset_game(world, d3dg);
					screen.recovery(d3dg);
				}
			}
		}
		State::STATE_DESTROY_RESULT => {
			if let Some(Stage::Result(mut screen, world)) = stage.take() {
				screen.destroy(d3dg, state, log);
				*stage = Some(Stage::Idle(world));
			}
		}

		// -- exit ---------------------------------------------------------------

		State::STATE_EXIT => {
			window.close_window();
		}
		State::STATE_NULL => {}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::resource::ResourceManager;

	/// The upstream frame skip: with the setting on, an odd frame is not drawn at all. The
	/// predicate is the only arithmetic in this file, and getting it backwards would make
	/// the game run at double rate rather than skip, which is worth one check.
	#[test]
	fn the_frame_skip_skips_odd_frames_only_when_it_is_on() {
		let mut config = Config::new();

		config.set_frameskip_flag(false);
		for framecnt in 0..4 {
			assert!(
				draws_this_frame(&config, framecnt),
				"frame {framecnt} drawn with the skip off"
			);
		}

		config.set_frameskip_flag(true);
		assert!(draws_this_frame(&config, 0));
		assert!(!draws_this_frame(&config, 1));
		assert!(draws_this_frame(&config, 2));
		assert!(!draws_this_frame(&config, 3));
	}

	/// The briefing is created with no mission file loaded, which is the state the menu
	/// leaves the world in.
	///
	/// The create is what steps the state machine, so a screen whose create was skipped
	/// could never be left: the state stayed on create, the screen was rebuilt every frame,
	/// and the window showed the last frame it had drawn while answering nothing at all.
	/// A player sees that as the game freezing on the mission they picked, and it is what
	/// picking the top row of the mission list did.
	#[test]
	fn the_briefing_is_created_with_no_mission_file_loaded() {
		let mut state = StateMachine::new();
		let mut window = WindowControl::new();
		let mut config = Config::new();
		let mut input_ctrl = InputControl::new();
		let mut input = Input::default();
		let instance =
			xops_platform::wgpu::Instance::new(&xops_platform::wgpu::InstanceDescriptor {
				backends: xops_platform::wgpu::Backends::all(),
				..Default::default()
			});
		let gfx = xops_platform::gfx::Gfx::new(&instance, None, 64, 48, None)
			.expect("a headless renderer");
		let mut d3dg = D3DGraphics::new(gfx, false, 0);
		let mut sound = SoundManager::new();
		let mut sound_ctrl = SoundControl::new();
		let mut log = DebugLog::new();

		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		world.game_info.selectmission_id = 0;
		assert!(world.mif.is_none(), "the menu leaves no mission file behind");

		// The state the menu's selection leads to.
		state.set_state_mode(1);
		let mut stage = Some(Stage::Idle(world));

		process_screen(
			&mut stage,
			&mut state,
			&mut window,
			&mut config,
			&mut input_ctrl,
			&mut input,
			&mut d3dg,
			&mut sound,
			&mut sound_ctrl,
			&mut log,
			0,
		);

		// The briefing read its mission, which is the thing that only happens when its
		// create ran.
		let world = stage.expect("a stage is always left behind").into_world();
		assert!(
			world.mif.is_some(),
			"the briefing loaded the mission it was asked for"
		);
	}
}
