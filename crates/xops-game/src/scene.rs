//! The screen base classes, from `scene.cpp` and `scene.h`.
//!
//! Upstream this is a three level inheritance chain. `scene` holds the frame count and the
//! phases every screen has, `D2Dscene` adds the title image, `D3Dscene` adds the camera and
//! the sound manager. Rust has no inheritance, so each level is a struct the next one
//! embeds by value, the way [`crate::object::Object`] is embedded in the concrete objects:
//!
//! * a screen is a struct with a field named `base` holding its base, so `opening` holds a
//!   [`D3Dscene`] and `briefing` a [`D2Dscene`];
//! * a phase the base does real work in is a method on the base struct and the screen calls
//!   it, as in `self.base.scene.input(input, state, log)`. `D3Dscene::Process` and
//!   `D3Dscene::Render3D` are empty upstream, which is why a 3D screen's own `process`
//!   advances `self.base.scene.framecnt` itself rather than calling the base;
//! * the drawing hooks (`Render3D`, `Render2D`) are not on the base at all: every screen
//!   upstream overrides them and none calls the base version, so a screen simply has its own
//!   methods. `render_main` still needs them to run in the right order, so it is an
//!   associated function taking the screen and its hooks. It cannot be a method on the base
//!   because the screen that implements the hooks also contains the base, and one `&mut`
//!   cannot reach both.
//!
//! The classes the base holds pointers to upstream (`GameState`, `WindowControl`,
//! `D3DGraphics`, `InputControl`, `SoundManager`) are singletons there. This port stores
//! none of them: every method that needs one takes it as an argument, which is the port's
//! rule for globals and the only way to keep the borrows disjoint. `scene::SetClass` has
//! nothing left to do as a result, and `scene::Create` takes the window that
//! `InputControl::MoveMouseCenter` used to reach through its own handle.

use xops_platform::host::HostWindow;
use xops_platform::input::{vk, Input};

use crate::config::Config;
use crate::datafile::path_from_bytes;
use crate::d3dgraphics::D3DGraphics;
use crate::debug::DebugLog;
use crate::soundmanager::SoundManager;
use crate::statemachine::StateMachine;

/// The title image both 2D screens and the menu load, upstream a literal at each call site.
const TITLE_TEXTURE: &[u8] = b"data\\title.dds";

/// The screen base, the upstream `scene`.
pub struct Scene {
	/// Frames this screen has been running for, upstream `framecnt`. Each screen has its
	/// own, because each screen is its own object upstream.
	pub framecnt: u32,
}

impl Scene {
	/// The upstream constructor, which also nulls its five class pointers. There are no
	/// pointers to null here.
	pub fn new() -> Scene {
		Scene { framecnt: 0 }
	}

	/// The upstream `scene::Create`: the cursor goes to the middle of the screen, the frame
	/// count restarts, and the state machine moves on.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		window: &HostWindow,
		log: &mut DebugLog,
	) -> i32 {
		// Upstream `InputControl::MoveMouseCenter` centres on `GameConfig.GetScreenWidth()
		// / 2`, which is the client area the window was created at. The window is asked for
		// it directly, which is the same number and stays right when the mode changes.
		if let Some((width, height)) = window.client_size() {
			window.set_cursor_position(width as f64 / 2.0, height as f64 / 2.0);
		}

		self.framecnt = 0;

		state.next_state(log);
		0
	}

	/// The upstream `scene::Recovery`, which is called after the renderer was reset and has
	/// nothing to do at this level.
	pub fn recovery(&mut self) -> i32 {
		0
	}

	/// The upstream `scene::Input`: Escape leaves the screen, releasing the left mouse
	/// button picks.
	///
	/// Upstream this starts with `GetInputState(false)`, which samples the keyboard into a
	/// table and keeps the previous frame's copy to compare against. The platform input
	/// layer is fed by the event loop and counts those transitions as they arrive, so there
	/// is nothing to sample here and the two checks below read the edges directly.
	pub fn input(&mut self, input: &mut Input, state: &mut StateMachine, log: &mut DebugLog) {
		if input.take_key_down(vk::ESCAPE) {
			state.push_back_space_key(log);
		} else if input.take_button_up(0) {
			state.push_mouse_button(log);
		}
	}

	/// The upstream `scene::Process`.
	pub fn process(&mut self) {
		// The counter is unsigned upstream, so it wraps rather than aborting the game after
		// four years of continuous play at 30 ms a frame.
		self.framecnt = self.framecnt.wrapping_add(1);
	}

	/// The upstream `scene::RenderMain`: a black screen with the two test strings on it,
	/// which is what a screen that draws nothing of its own shows.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		if d3dg.start_render() != 0 {
			return true;
		}

		d3dg.draw_2d_box(
			0,
			0,
			config.get_screen_width(),
			config.get_screen_height(),
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
		);
		d3dg.draw_2d_texture_font_text(
			10,
			10,
			b"hello world !",
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			20,
			32,
		);

		d3dg.screen_brightness(
			config.get_screen_width(),
			config.get_screen_height(),
			config.get_brightness(),
		);

		d3dg.end_render()
	}

	/// The upstream `scene::Destroy`: the screen is finished with, so the state machine
	/// moves on from whatever state this screen was showing.
	pub fn destroy(&mut self, state: &mut StateMachine, log: &mut DebugLog) {
		state.next_state(log);
	}

	/// The upstream `scene::GetEffectAlpha`, the fade a screen draws itself with.
	///
	/// `tframecnt - frameoffset` is unsigned upstream, so counting from before the offset
	/// wraps to a huge number rather than going negative, and the clamp turns that into a
	/// fully faded in screen. No call site reaches that: every one of them has the frame
	/// count past the offset by the time it asks.
	pub fn get_effect_alpha(
		&self,
		tframecnt: u32,
		max_alpha: f32,
		timingsec: f32,
		offsetsec: f32,
		reversal: bool,
	) -> f32 {
		let frametiming = (timingsec * crate::core::GAMEFPS) as u32;
		let frameoffset = (offsetsec * crate::core::GAMEFPS) as u32;
		// A `frametiming` of zero divides by zero into an infinity, which is what the
		// upstream float division does, and the clamps below bring it back.
		let mut alpha = max_alpha / frametiming as f32 * tframecnt.wrapping_sub(frameoffset) as f32;

		if alpha < 0.0 {
			alpha = 0.0;
		}
		if alpha > 1.0 {
			alpha = 1.0;
		}

		if reversal {
			return 1.0 - alpha;
		}
		alpha
	}

	/// The upstream `scene::GetEffectAlphaLoop`, the same fade on a repeating cycle.
	///
	/// There is no clamp here, so the result is `max_alpha` at its peak and can in principle
	/// be anything in between. A `timingsec` of zero is a modulo by zero upstream and aborts
	/// here, which is the closest defined answer to undefined behaviour.
	pub fn get_effect_alpha_loop(
		&self,
		tframecnt: u32,
		max_alpha: f32,
		timingsec: f32,
		reversal: bool,
	) -> f32 {
		let frametiming = (timingsec * crate::core::GAMEFPS) as u32;
		let alpha = max_alpha / frametiming as f32 * (tframecnt % frametiming) as f32;

		if reversal {
			return 1.0 - alpha;
		}
		alpha
	}
}

impl Default for Scene {
	fn default() -> Scene {
		Scene::new()
	}
}

/// The 2D screen base, the upstream `D2Dscene`: the state of [`Scene`] plus the title image
/// every 2D screen is drawn over.
pub struct D2Dscene {
	/// The state and the frame count.
	pub scene: Scene,
	/// The title image, or -1 when it did not load.
	pub gametitle: i32,
}

impl D2Dscene {
	/// The upstream constructor, which starts with no title loaded.
	pub fn new() -> D2Dscene {
		D2Dscene {
			scene: Scene::new(),
			gametitle: -1,
		}
	}

	/// The upstream `D2Dscene::Create`. It does not call `scene::Create`: loading the title
	/// replaces the cursor centring and the frame count restart, and a screen that wants
	/// those does them itself.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		d3dg: &mut D3DGraphics,
		log: &mut DebugLog,
	) -> i32 {
		self.gametitle = d3dg.load_texture(TITLE_TEXTURE, false, false);
		state.next_state(log);
		if self.gametitle == -1 {
			return 1;
		}
		0
	}

	/// The upstream `D2Dscene::Recovery`: the screen is entered again after the renderer was
	/// reset, so the title is loaded again and the failure is reported before the caller
	/// tries to draw it.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		self.gametitle = d3dg.load_texture(TITLE_TEXTURE, false, false);
		if self.gametitle == -1 {
			return 1;
		}
		0
	}

	/// The upstream `D2Dscene::RenderMain`, with the derived `Render2D` called in place.
	///
	/// `gametitle` is the caller's `base.gametitle` passed by value: this cannot be a method
	/// on the base, because `screen` borrows the whole screen and the base lives inside it.
	/// A screen calls this as
	/// `D2Dscene::render_main(d3dg, config, self.base.gametitle, self, Self::render_2d)`.
	pub fn render_main<S>(
		d3dg: &mut D3DGraphics,
		config: &Config,
		gametitle: i32,
		screen: &mut S,
		render_2d: fn(&mut S, &mut D3DGraphics, &Config),
	) -> bool {
		if d3dg.start_render() != 0 {
			return true;
		}

		d3dg.draw_2d_texture(
			0,
			0,
			gametitle,
			config.get_screen_width(),
			config.get_screen_height(),
			0.4,
		);
		d3dg.draw_2d_box(
			11,
			11,
			config.get_screen_width() - 10,
			config.get_screen_height() - 10,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.75),
		);

		render_2d(screen, d3dg, config);

		d3dg.screen_brightness(
			config.get_screen_width(),
			config.get_screen_height(),
			config.get_brightness(),
		);

		d3dg.end_render()
	}

	/// The upstream `D2Dscene::Destroy`: the title image goes back to the renderer and the
	/// state machine moves on.
	pub fn destroy(&mut self, d3dg: &mut D3DGraphics, state: &mut StateMachine, log: &mut DebugLog) {
		d3dg.cleanup_texture(self.gametitle);
		state.next_state(log);
	}
}

impl Default for D2Dscene {
	fn default() -> D2Dscene {
		D2Dscene::new()
	}
}

/// The 3D screen base, the upstream `D3Dscene`: the state of [`Scene`] plus the camera every
/// 3D screen draws from.
///
/// The sound manager is a pointer upstream and is passed to [`D3Dscene::sound`] here, so it
/// is not a field.
pub struct D3Dscene {
	/// The state and the frame count.
	pub scene: Scene,
	pub camera_x: f32,
	pub camera_y: f32,
	pub camera_z: f32,
	pub camera_rx: f32,
	pub camera_ry: f32,
}

impl D3Dscene {
	/// The upstream constructor, which starts the camera at the origin looking down +Z.
	/// A screen moves it in its own `create`, which is where the upstream does it too.
	pub fn new() -> D3Dscene {
		D3Dscene {
			scene: Scene::new(),
			camera_x: 0.0,
			camera_y: 0.0,
			camera_z: 0.0,
			camera_rx: 0.0,
			camera_ry: 0.0,
		}
	}

	/// The upstream `D3Dscene::Sound`, the sound the whole world makes, positioned at the
	/// camera. Upstream this is the base's `Sound`, so the screens that do not override it
	/// get this, and the ones that do call the manager themselves.
	pub fn sound(&mut self, game_sound: &mut SoundManager) {
		game_sound.play_world_sound(
			self.camera_x,
			self.camera_y,
			self.camera_z,
			self.camera_rx,
			1,
		);
	}

	/// The upstream `D3Dscene::RenderMain`, with the derived `Render3D` and `Render2D`
	/// called in place.
	///
	/// A screen calls this as
	/// `D3Dscene::render_main(d3dg, config, self, Self::render_3d, Self::render_2d)`. The
	/// two hooks are the screen's own methods, because the base's are empty upstream and no
	/// screen calls them.
	pub fn render_main<S>(
		d3dg: &mut D3DGraphics,
		config: &Config,
		screen: &mut S,
		render_3d: fn(&mut S, &mut D3DGraphics, &Config),
		render_2d: fn(&mut S, &mut D3DGraphics, &Config),
	) -> bool {
		if d3dg.start_render() != 0 {
			return true;
		}

		render_3d(screen, d3dg, config);
		render_2d(screen, d3dg, config);

		d3dg.screen_brightness(
			config.get_screen_width(),
			config.get_screen_height(),
			config.get_brightness(),
		);

		d3dg.end_render()
	}
}

impl Default for D3Dscene {
	fn default() -> D3Dscene {
		D3Dscene::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::GAMEFPS;
	use crate::statemachine::State;

	/// The alpha is a float division, so it lands near the expected value rather than on it.
	fn close(actual: f32, expected: f32) {
		assert!(
			(actual - expected).abs() < 1e-6,
			"{actual} is not {expected}"
		);
	}

	/// A fade that starts at the offset and takes the whole timing to reach full alpha.
	///
	/// Two seconds is 66 frames at 30 ms a frame, not 67, so the last frame of the ramp is
	/// the one that reaches one.
	#[test]
	fn effect_alpha_runs_from_transparent_to_opaque() {
		let scene = Scene::new();

		close(scene.get_effect_alpha(0, 1.0, 2.0, 0.0, false), 0.0);
		close(scene.get_effect_alpha(33, 1.0, 2.0, 0.0, false), 0.5);
		close(scene.get_effect_alpha(66, 1.0, 2.0, 0.0, false), 1.0);
	}

	/// The frame count cannot make it overshoot in either direction.
	#[test]
	fn effect_alpha_is_clamped_at_both_ends() {
		let scene = Scene::new();

		close(scene.get_effect_alpha(500, 1.0, 2.0, 0.0, false), 1.0);
		close(scene.get_effect_alpha(0, 0.0, 2.0, 0.0, false), 0.0);
	}

	/// `reversal` is the fade out, which is the same ramp inverted, and it inverts after the
	/// clamp rather than before, so the ends stay at the ends.
	#[test]
	fn effect_alpha_reversal_inverts_the_ramp() {
		let scene = Scene::new();

		close(scene.get_effect_alpha(0, 1.0, 2.0, 0.0, true), 1.0);
		close(scene.get_effect_alpha(33, 1.0, 2.0, 0.0, true), 0.5);
		close(scene.get_effect_alpha(66, 1.0, 2.0, 0.0, true), 0.0);
	}

	/// The offset delays the start, and a frame count before the offset wraps as unsigned
	/// upstream and reads as fully faded in.
	#[test]
	fn effect_alpha_counts_the_offset_first() {
		let scene = Scene::new();
		let offset = 11.0;
		// 11 seconds at 30 ms a frame, which is where `opening` starts its fade.
		let first = (offset * GAMEFPS) as u32;

		close(scene.get_effect_alpha(first, 1.0, 4.0, offset, false), 0.0);
		close(
			scene.get_effect_alpha(first + (4.0 * GAMEFPS) as u32, 1.0, 4.0, offset, false),
			1.0,
		);
		// Before the offset the subtraction wraps, and the clamp turns that into one.
		close(scene.get_effect_alpha(0, 1.0, 4.0, offset, false), 1.0);
	}

	/// The loop version has no clamp, so it repeats the ramp instead of flattening.
	#[test]
	fn effect_alpha_loop_repeats() {
		let scene = Scene::new();

		close(scene.get_effect_alpha_loop(0, 1.0, 2.0, false), 0.0);
		close(scene.get_effect_alpha_loop(33, 1.0, 2.0, false), 0.5);
		close(scene.get_effect_alpha_loop(66, 1.0, 2.0, false), 0.0);
		close(scene.get_effect_alpha_loop(66, 1.0, 2.0, true), 1.0);
	}

	/// The base's own `Process` is the only place the frame count advances. A 3D screen
	/// overrides it without calling this, which is what the upstream empty
	/// `D3Dscene::Process` amounts to.
	#[test]
	fn process_advances_the_frame_count() {
		let mut scene = Scene::new();

		scene.process();
		scene.process();
		assert_eq!(scene.framecnt, 2);
	}

	/// `Create` restarts the frame count and moves the state machine on, `Destroy` only
	/// moves it on.
	#[test]
	fn create_restarts_the_count_and_advances_the_state() {
		let mut scene = Scene::new();
		let mut state = StateMachine::new();
		let mut log = DebugLog::new();
		// A window that does not exist yet has no client area to centre the cursor in,
		// which is the upstream `hWnd == NULL` case.
		let window = HostWindow::default();

		scene.framecnt = 120;
		assert_eq!(scene.create(&mut state, &window, &mut log), 0);
		assert_eq!(scene.framecnt, 0);
		// One step from the create state, which is where the frame loop then runs the screen.
		assert_eq!(state.get_state(), State::STATE_NOW_OPENING);

		scene.destroy(&mut state, &mut log);
		assert_eq!(state.get_state(), State::STATE_DESTROY_OPENING);
	}

	/// The base `Recovery` succeeds without doing anything, which is why every screen that
	/// has something to reload overrides it.
	#[test]
	fn recovery_has_nothing_to_do_at_this_level() {
		let mut scene = Scene::new();
		assert_eq!(scene.recovery(), 0);
	}

	/// A 2D screen starts with no title, and its `Destroy` has a texture to hand back.
	#[test]
	fn two_d_scene_starts_without_a_title() {
		let scene = D2Dscene::new();
		assert_eq!(scene.gametitle, -1);
		assert_eq!(scene.scene.framecnt, 0);
	}

	/// A 3D screen starts with the camera at the origin, and the screen is what moves it.
	#[test]
	fn three_d_scene_starts_at_the_origin() {
		let scene = D3Dscene::new();
		assert_eq!(scene.camera_x, 0.0);
		assert_eq!(scene.camera_y, 0.0);
		assert_eq!(scene.camera_z, 0.0);
		assert_eq!(scene.camera_rx, 0.0);
		assert_eq!(scene.camera_ry, 0.0);
	}
}
