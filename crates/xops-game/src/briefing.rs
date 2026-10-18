//! The briefing and result screens, from `gamemain.cpp`.
//!
//! Both are `D2Dscene`s upstream with no members of their own that carry the picture:
//! they read the loaded mission (the `MIFdata` global) and the outcome (the
//! `GameInfoData` global) live every frame. This port bans globals, so each screen
//! copies what it draws in `create`, which is safe because neither the loaded mission
//! nor the result changes while the screen is up.

use xops_platform::input::Input;

use crate::config::Config;
use crate::core::GAMEFPS;
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::MIFInterface;
use crate::debug::DebugLog;
use crate::gamemain::GameInfo;
use crate::input::InputControl;
use crate::parameter::ParameterInfo;
use crate::scene::D2Dscene;
use crate::statemachine::StateMachine;
use crate::window::WindowControl;

/// The bytes of a fixed C field up to its NUL. This module is the only place upstream
/// code compares a `MAX_PATH` picture field with `"!"`, so it keeps its own trim.
fn cstr_bytes(field: &[u8]) -> Vec<u8> {
	let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
	field[..end].to_vec()
}

/// The mission name both screens show above their content.
///
/// Upstream picks it live from the globals: a standard mission (`.txt`, `GetFiletype`
/// false) takes the official mission's full name from the parameter table, an addon
/// (`.mif`) carries its own. The parameter table indexes by `selectmission_id`.
pub fn mission_name(mif: &MIFInterface, params: &ParameterInfo, info: &GameInfo) -> Vec<u8> {
	if !mif.get_filetype() {
		params
			.get_official_mission(info.selectmission_id)
			.map(|mission| mission.fullname.to_vec())
			.unwrap_or_default()
	} else {
		cstr_bytes(mif.get_mission_fullname())
	}
}

/// The briefing body split into the lines the system font lays out.
///
/// The body is one field, up to 17 lines the loader appended with their line feeds kept,
/// and the rasteriser breaks it at each `\n` and stops at the NUL. This is the same
/// split, kept separate so the line count is testable without a renderer.
pub fn briefing_text_lines(text: &[u8]) -> Vec<&[u8]> {
	let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
	let body = &text[..end];
	if body.is_empty() {
		return Vec::new();
	}

	let mut lines = Vec::new();
	let mut start = 0;
	for (i, &byte) in body.iter().enumerate() {
		if byte == b'\n' {
			lines.push(&body[start..i]);
			start = i + 1;
		}
	}
	// A final line feed ends the last line rather than opening an empty one, which is
	// how the loader's line count and the font's break agree.
	if start < body.len() {
		lines.push(&body[start..]);
	}
	lines
}

/// The result screen's outcome line, from `missioncomplete`.
pub fn result_outcome(missioncomplete: bool) -> &'static [u8] {
	if missioncomplete {
		b"mission successful"
	} else {
		b"mission failure"
	}
}

/// The mission time as `(minutes, seconds)`.
///
/// `GAMEFPS` is a float, and upstream truncates it to an int before dividing, so the
/// frame count is divided by 33 and not by 33.33.
pub fn result_time(info: &GameInfo) -> (u32, u32) {
	let seconds = info.framecnt / GAMEFPS as u32;
	(seconds / 60, seconds % 60)
}

/// The hit rate, `ontarget` truncated before the division as upstream, and zero when
/// nothing was fired.
pub fn hit_rate(info: &GameInfo) -> f32 {
	if info.fire == 0 {
		return 0.0;
	}
	info.ontarget.floor() / info.fire as f32 * 100.0
}

/// Every value line the result screen prints, in the order it draws them.
///
/// The totals are the ones [`GameInfo`] holds. Nothing here depends on whether the
/// mission was completed, which only changes the outcome line.
pub fn result_lines(info: &GameInfo) -> Vec<Vec<u8>> {
	let (minutes, seconds) = result_time(info);
	let hits = info.ontarget.floor() as i32;
	vec![
		format!("Time  {minutes}min {seconds}sec").into_bytes(),
		format!("Rounds fired  {}", info.fire).into_bytes(),
		format!("Rounds on target  {hits}").into_bytes(),
		format!("Accuracy rate  {:.1}%", hit_rate(info)).into_bytes(),
		format!("Kill  {} / HeadShot  {}", info.kill, info.headshot).into_bytes(),
	]
}

/// The briefing screen, upstream `briefing`.
pub struct Briefing {
	base: D2Dscene,
	/// Whether the mission supplies a second briefing image, upstream `TwoTexture`.
	two_texture: bool,
	/// Briefing image A, or -1 when it did not load.
	texture_a: i32,
	/// Briefing image B, or -1.
	texture_b: i32,
	/// The mission name, resolved once in `create`, upstream read from `MIFdata`.
	mission_name: Vec<u8>,
	/// The briefing body, copied once in `create`.
	briefing_text: Vec<u8>,
}

impl Briefing {
	/// The upstream constructor, which starts with no images and one-image mode.
	pub fn new() -> Briefing {
		Briefing {
			base: D2Dscene::new(),
			two_texture: false,
			texture_a: -1,
			texture_b: -1,
			mission_name: Vec::new(),
			briefing_text: Vec::new(),
		}
	}

	/// The upstream `briefing::Create`.
	///
	/// It loads the title by hand rather than calling `D2Dscene::Create`, because it
	/// advances the state machine only after the mission file has loaded. [`D2Dscene::recovery`]
	/// is the same two lines of title loading, so it is reused here.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		window: &WindowControl,
		config: &Config,
		d3dg: &mut D3DGraphics,
		input_ctrl: &InputControl,
		log: &mut DebugLog,
		info: &GameInfo,
		params: &ParameterInfo,
		mif: &mut MIFInterface,
	) -> i32 {
		self.base.recovery(d3dg);

		// Official missions build the path from the parameter table's directory, file
		// stem and the `.txt` extension. An addon carries its own path in the info.
		let path = if info.selectmission_id >= 0 {
			match params.get_official_mission(info.selectmission_id) {
				Some(mission) => {
					let mut path = mission.directory.to_vec();
					path.extend_from_slice(mission.txt);
					path.extend_from_slice(b".txt");
					path
				}
				None => Vec::new(),
			}
		} else {
			cstr_bytes(&info.mifpath)
		};

		if mif.load_file_data(&path, true) != 0 {
			// Briefing data open failed.
			return 1;
		}

		self.load_images(d3dg, mif);
		self.mission_name = mission_name(mif, params, info);
		self.briefing_text = cstr_bytes(mif.get_briefing_text());

		// Move the mouse cursor to the centre.
		input_ctrl.move_mouse_center(window, config);
		self.base.scene.framecnt = 0;

		state.next_state(log);
		0
	}

	/// The upstream `briefing::Recovery`, which reloads the title and the images after
	/// the renderer was reset. The title failure is ignored, as upstream.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics, mif: &MIFInterface) -> i32 {
		self.base.recovery(d3dg);
		self.load_images(d3dg, mif);
		0
	}

	/// The upstream image block, shared by `create` and `recovery`.
	///
	/// Image B is `"!"` when the mission supplies only one image, and that is the exact
	/// key that decides whether the second is drawn at all.
	fn load_images(&mut self, d3dg: &mut D3DGraphics, mif: &MIFInterface) {
		let (picture_a, picture_b) = mif.get_picturefile_path();
		if cstr_bytes(picture_b) == *b"!" {
			self.two_texture = false;
			self.texture_a = d3dg.load_texture(picture_a, true, false);
			self.texture_b = -1;
		} else {
			self.two_texture = true;
			self.texture_a = d3dg.load_texture(picture_a, true, false);
			self.texture_b = d3dg.load_texture(picture_b, true, false);
		}
	}

	/// The upstream `briefing::Input`, which is `scene::Input`.
	pub fn input(
		&mut self,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		state: &mut StateMachine,
		log: &mut DebugLog,
	) {
		self.base.scene.input(input_ctrl, input, state, log);
	}

	/// The upstream `briefing::Process`, which is `scene::Process`.
	pub fn process(&mut self) {
		self.base.scene.process();
	}

	/// Forwards to [`D2Dscene::render_main`] with this screen's `render_2d`.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		D2Dscene::render_main(d3dg, config, self.base.gametitle, self, Self::render_2d)
	}

	/// The upstream `briefing::Render2D`.
	fn render_2d(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let framecnt = self.base.scene.framecnt;
		let effect_a = self.base.scene.get_effect_alpha_loop(framecnt, 0.8, 0.7, true);
		let effect_b = self.base.scene.get_effect_alpha_loop(framecnt, 0.8, 1.0, true);
		// The blinking prompt grows and shrinks once a second, `framecnt % GAMEFPS`.
		let cycle = framecnt % GAMEFPS as u32;
		let effect_b_w = (cycle as f32 * 0.2 + 18.0) as i32;
		let effect_b_h = (cycle as f32 * 1.0 + 26.0) as i32;

		// The background image is drawn automatically by D2Dscene::render_main.

		d3dg.draw_2d_texture_font_text_center_scaling(
			0,
			30,
			b"BRIEFING",
			D3DGraphics::get_color_code(1.0, 1.0, 0.0, effect_a),
			60,
			42,
		);
		let screen_w = config.get_screen_width();
		let screen_h = config.get_screen_height();
		d3dg.draw_2d_texture_font_text(
			screen_w - 210 - effect_b_w * 20 / 2,
			screen_h - 37 - effect_b_h / 2,
			b"LEFT CLICK TO BEGIN",
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_b),
			effect_b_w,
			effect_b_h,
		);
		d3dg.draw_2d_texture_font_text(
			screen_w - 210 - 18 * 20 / 2,
			screen_h - 37 - 26 / 2,
			b"LEFT CLICK TO BEGIN",
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			18,
			26,
		);

		// The briefing images. A missing image is a blank box, and the two-image layout
		// moves both up to make room.
		if !self.two_texture {
			if self.texture_a == -1 {
				d3dg.draw_2d_box_scaling(
					40,
					180,
					40 + 160,
					180 + 150,
					D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				);
			} else {
				d3dg.draw_2d_texture_scaling(40, 180, self.texture_a, 160, 150, 1.0);
			}
		} else {
			if self.texture_a == -1 {
				d3dg.draw_2d_box_scaling(
					40,
					130,
					40 + 160,
					130 + 150,
					D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				);
			} else {
				d3dg.draw_2d_texture_scaling(40, 130, self.texture_a, 160, 150, 1.0);
			}
			if self.texture_b == -1 {
				d3dg.draw_2d_box_scaling(
					40,
					300,
					40 + 160,
					300 + 150,
					D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				);
			} else {
				d3dg.draw_2d_texture_scaling(40, 300, self.texture_b, 160, 150, 1.0);
			}
		}

		d3dg.draw_2d_texture_font_text_center_scaling(
			0,
			90,
			&self.mission_name,
			D3DGraphics::get_color_code(1.0, 0.5, 0.0, 1.0),
			18,
			25,
		);

		// The body goes to the system font, which breaks the lines at their line feeds.
		d3dg.draw_2d_ms_font_text_scaling(
			230,
			180,
			&self.briefing_text,
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
		);
	}

	/// The upstream `briefing::Destroy`: the two images and the title go back to the
	/// renderer, and the state machine moves on.
	pub fn destroy(&mut self, d3dg: &mut D3DGraphics, state: &mut StateMachine, log: &mut DebugLog) {
		d3dg.cleanup_texture(self.texture_a);
		d3dg.cleanup_texture(self.texture_b);
		self.base.destroy(d3dg, state, log);
	}
}

impl Default for Briefing {
	fn default() -> Briefing {
		Briefing::new()
	}
}

/// The result screen, upstream `result`.
pub struct Result {
	base: D2Dscene,
	/// The outcome the mission reported, copied in `create`.
	info: GameInfo,
	/// The mission name, resolved once in `create`.
	mission_name: Vec<u8>,
}

impl Result {
	/// The upstream constructor, which only leaves the title to `D2Dscene::Create`.
	pub fn new() -> Result {
		Result {
			base: D2Dscene::new(),
			info: GameInfo::default(),
			mission_name: Vec::new(),
		}
	}

	/// Loads the screen and copies the outcome it will report.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		d3dg: &mut D3DGraphics,
		log: &mut DebugLog,
		info: &GameInfo,
		params: &ParameterInfo,
		mif: &MIFInterface,
	) -> i32 {
		let code = self.base.create(state, d3dg, log);
		self.info = *info;
		self.mission_name = mission_name(mif, params, info);
		code
	}

	/// The upstream `result` uses `D2Dscene::Recovery`.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		self.base.recovery(d3dg)
	}

	/// The upstream `result::Input`, which is `scene::Input`.
	pub fn input(
		&mut self,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		state: &mut StateMachine,
		log: &mut DebugLog,
	) {
		self.base.scene.input(input_ctrl, input, state, log);
	}

	/// The upstream `result::Process`, which is `scene::Process`.
	pub fn process(&mut self) {
		self.base.scene.process();
	}

	/// Forwards to [`D2Dscene::render_main`] with this screen's `render_2d`.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		D2Dscene::render_main(d3dg, config, self.base.gametitle, self, Self::render_2d)
	}

	/// The upstream `result::Render2D`.
	fn render_2d(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let _ = config;
		let effect_a = self
			.base
			.scene
			.get_effect_alpha_loop(self.base.scene.framecnt, 1.0, 0.8, true);

		// The background image is drawn automatically by D2Dscene::render_main.

		d3dg.draw_2d_texture_font_text_center_scaling(
			0,
			40,
			b"RESULT",
			D3DGraphics::get_color_code(1.0, 0.0, 1.0, effect_a),
			50,
			42,
		);
		d3dg.draw_2d_texture_font_text_center_scaling(
			0,
			100,
			&self.mission_name,
			D3DGraphics::get_color_code(0.5, 0.5, 1.0, 1.0),
			18,
			25,
		);

		let colour = if self.info.missioncomplete {
			D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0)
		} else {
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0)
		};
		d3dg.draw_2d_texture_font_text_center_scaling(
			0,
			150,
			result_outcome(self.info.missioncomplete),
			colour,
			24,
			32,
		);

		// Five rows, 50 pixels apart, starting at 210.
		for (i, line) in result_lines(&self.info).iter().enumerate() {
			d3dg.draw_2d_texture_font_text_center_scaling(
				0,
				210 + i as i32 * 50,
				line,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				20,
				32,
			);
		}
	}

	/// The upstream `result::Destroy`, which is `D2Dscene::Destroy`.
	pub fn destroy(&mut self, d3dg: &mut D3DGraphics, state: &mut StateMachine, log: &mut DebugLog) {
		self.base.destroy(d3dg, state, log);
	}
}

impl Default for Result {
	fn default() -> Result {
		Result::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Writes bytes to a temp file and returns the path as the byte path the loaders
	/// take.
	fn temp_write(name: &str, bytes: &[u8]) -> Vec<u8> {
		let path = std::env::temp_dir().join(format!(
			"xops_briefing_{}_{}",
			std::process::id(),
			name
		));
		xops_platform::fs::write_file(&path, bytes).unwrap();
		path.to_string_lossy().into_owned().into_bytes()
	}

	#[test]
	fn the_briefing_body_splits_on_its_line_feeds() {
		assert_eq!(
			briefing_text_lines(b"The text\nMore text\n"),
			vec![&b"The text"[..], &b"More text"[..]]
		);
		assert_eq!(briefing_text_lines(b"one"), vec![&b"one"[..]]);
		// The NUL ends the field, so anything past it is not a line.
		assert_eq!(briefing_text_lines(b"a\0b\nc"), vec![&b"a"[..]]);
		assert!(briefing_text_lines(b"").is_empty());
		assert!(briefing_text_lines(&[0u8; 8]).is_empty());
	}

	#[test]
	fn the_loader_stops_the_briefing_at_seventeen_lines() {
		// A standard mission file: image A, image B, sky, then the body.
		let mut seventeen = b"!\n!\n0\n".to_vec();
		for i in 1..=17 {
			seventeen.extend_from_slice(format!("line {i}\n").as_bytes());
		}
		let path = temp_write("brief17.txt", &seventeen);
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, true), 0);
		assert_eq!(mif.get_total_datas(), 17);
		assert_eq!(briefing_text_lines(mif.get_briefing_text()).len(), 17);
		assert_eq!(briefing_text_lines(mif.get_briefing_text())[16], b"line 17");
	}

	#[test]
	fn the_eighteenth_briefing_line_is_dropped() {
		let mut eighteen = b"!\n!\n0\n".to_vec();
		for i in 1..=18 {
			eighteen.extend_from_slice(format!("line {i}\n").as_bytes());
		}
		let path = temp_write("brief18.txt", &eighteen);
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, true), 0);
		assert_eq!(mif.get_total_datas(), 17);
		let lines = briefing_text_lines(mif.get_briefing_text());
		assert_eq!(lines.len(), 17);
		assert_eq!(lines[16], b"line 17");
	}

	#[test]
	fn the_mission_name_follows_the_file_type() {
		// A standard mission file takes the official mission's full name.
		let path = temp_write("official.txt", b"!\n!\n0\nbody\n");
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, true), 0);
		assert!(!mif.get_filetype());
		let mut info = GameInfo::default();
		info.selectmission_id = 0;
		assert_eq!(
			mission_name(&mif, &ParameterInfo::new(), &info),
			b"TRAINING YARD training"
		);
		let unknown = GameInfo {
			selectmission_id: -1,
			..GameInfo::default()
		};
		assert!(mission_name(&mif, &ParameterInfo::new(), &unknown).is_empty());

		// A `.mif` carries its own full name.
		let mif_bytes = b"MISSION1\nMission One\ntemp.bd1\ntemp.pd1\n1\n0\n!\n!\n!\nLine one\nLine two\n";
		let path = temp_write("addon.mif", mif_bytes);
		let mut mif = MIFInterface::new();
		assert_eq!(mif.load_file_data(&path, true), 0);
		assert!(mif.get_filetype());
		assert_eq!(
			mission_name(&mif, &ParameterInfo::new(), &GameInfo::default()),
			b"Mission One"
		);
	}

	#[test]
	fn the_result_outcome_is_the_mission_completion() {
		assert_eq!(result_outcome(true), b"mission successful");
		assert_eq!(result_outcome(false), b"mission failure");
	}

	#[test]
	fn the_result_lines_are_the_game_info_totals() {
		let info = GameInfo {
			framecnt: 2145,
			fire: 40,
			ontarget: 3.5,
			kill: 2,
			headshot: 1,
			..GameInfo::default()
		};
		assert_eq!(result_time(&info), (1, 5));
		let lines = result_lines(&info);
		assert_eq!(lines[0], b"Time  1min 5sec");
		assert_eq!(lines[1], b"Rounds fired  40");
		assert_eq!(lines[2], b"Rounds on target  3");
		assert_eq!(lines[3], b"Accuracy rate  7.5%");
		assert_eq!(lines[4], b"Kill  2 / HeadShot  1");
	}

	#[test]
	fn the_result_totals_are_the_same_on_both_outcomes() {
		let base = GameInfo {
			framecnt: 660,
			fire: 10,
			ontarget: 2.9,
			kill: 1,
			headshot: 0,
			..GameInfo::default()
		};
		let failed = GameInfo { missioncomplete: false, ..base };
		let completed = GameInfo { missioncomplete: true, ..base };

		// Only the outcome line differs, the totals are the ones GameInfo holds.
		assert_ne!(result_outcome(failed.missioncomplete), result_outcome(completed.missioncomplete));
		assert_eq!(result_lines(&failed), result_lines(&completed));
		assert_eq!(result_lines(&failed)[3], b"Accuracy rate  20.0%");
	}

	#[test]
	fn a_result_with_no_shots_has_no_rate() {
		let info = GameInfo {
			fire: 0,
			ontarget: 5.0,
			..GameInfo::default()
		};
		assert_eq!(hit_rate(&info), 0.0);
		assert_eq!(result_lines(&info)[3], b"Accuracy rate  0.0%");
	}

}
