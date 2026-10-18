//! The opening screen and the menu, from the upstream `gamemain.cpp`: `opening` at lines
//! 206 to 495 and `mainmenu` at 496 to 1607, with both classes declared in `gamemain.h`.
//!
//! Three things about the shape are worth knowing before reading the port.
//!
//! The screens reach their world through [`crate::gamemain::World`], which is where the
//! file scope objects upstream (`ObjMgr`, `BlockData`, `PointData`, `CollD`, `GameAddon`,
//! `GameInfoData`) live. They have to: `Render3D` and `Render2D` are hooks called through
//! a plain function pointer, so `D3Dscene::render_main` cannot hand them anything beyond
//! the screen itself, and the pointer type is what the screens are built around.
//!
//! The options screens behind `ENABLE_MENUOPTIONS` are live, not optional: the switch is
//! true in [`crate::core`], so page 1, the key settings page and the licence page are all
//! reachable, and the branches upstream keeps behind the `#ifdef` are kept as `if
//! ENABLE_MENUOPTIONS` so that flipping the constant reproduces the no-options build.
//!
//! The licence text is the `GRAPHIC_ENGINE == 8` branch, the OpenGL one, because that is
//! the core this port came from and `OPTIONS_LICENSELINE` is 6 to match it.

use xops_platform::input::Input;

use crate::config::Config;
use crate::core::{
	degree_to_radian, ADDONDIR, ENABLE_MENUOPTIONS, GAMEFPS, GAMENAME, GAMEVERSION, MAX_PATH,
};
use crate::d3dgraphics::D3DGraphics;
use crate::datafile::AddonList;
use crate::debug::DebugLog;
use crate::gamemain::{
	mainmenu_option_y, mainmenu_x, mainmenu_y, options_p1_x, options_p1_y, options_p2_x,
	options_p2_y, options_p3_x, options_p3_y, GameInfo, MenuLinkTextData, World, MAINMENU_H,
	MAINMENU_OPTION_X, OPTIONS_LICENSELINE, OPTIONS_P1_DATAS, OPTIONS_P1_H, OPTIONS_P1_W,
	OPTIONS_P2_DATAS, OPTIONS_P2_H, OPTIONS_P2_W, OPTIONS_P3_DATAS, OPTIONS_P3_H, OPTIONS_P3_W,
	TOTAL_MENUITEMS,
};
use crate::gui_object::ScrollbarObject;
use crate::input::{get_esc_keycode, originalkeycode_to_dinputdef, InputControl};
use crate::objectmanager::MAX_HUMAN;
use crate::parameter::{ParameterInfo, TOTAL_OFFICIALMISSION};
use crate::scene::D3Dscene;
use crate::soundmanager::SoundManager;
use crate::statemachine::StateMachine;
use crate::window::{get_rand, WindowControl};

/// The map `opening` shows, which is a fixed one rather than a mission.
const OPENING_DIR: &[u8] = b"data\\map10\\";
const OPENING_BLOCK: &[u8] = b"data\\map10\\temp.bd1";
const OPENING_POINT: &[u8] = b"data\\map10\\op.pd1";

/// The six demo maps `mainmenu` picks from, in the order of the upstream `switch`.
const MAINMENU_DEMOS: [&[u8]; 6] = [
	b"data\\map2\\",
	b"data\\map4\\",
	b"data\\map5\\",
	b"data\\map7\\",
	b"data\\map8\\",
	b"data\\map16\\",
];

/// The menu's own title image. The 2D screens use `scene.rs`'s `TITLE_TEXTURE` through
/// their base, and this is `mainmenu` loading the same file for itself, as upstream.
const TITLE_TEXTURE: &[u8] = b"data\\title.dds";

/// The licence text, `mainmenu::CreateOptions`'s `GRAPHIC_ENGINE == 8` branch. There are
/// two branches upstream, 26 lines naming the DirectX libraries and 6 for OpenGL, and the
/// count in [`OPTIONS_LICENSELINE`] is the one this table has to match.
const LICENSE_INFO: [&[u8]; OPTIONS_LICENSELINE] = [
	b"",
	b"OpenXOPS",
	b"Copyright (c) 2014-2023, OpenXOPS Project / [-_-;](mikan) All rights reserved.",
	b"",
	b" -- See the document.",
	b"",
];

/// The licence lines one screen shows, and 14 when the language is Korean, where the
/// upstream shortens the page for the taller glyphs.
const OPTIONS_LICENSEMAXLINE: i32 = 18;
const OPTIONS_LICENSEMAXLINE_KOREAN: i32 = 14;

/// The action names beside the 18 key binding rows, in the order
/// `Options_p2LinkTextData[14 + i]` lists them. Upstream repeats the same draw call 18
/// times with the label written out, and the rows are one table here.
const OPTIONS_KEY_NAMES: [&[u8]; 18] = [
	b"TURN UP",
	b"TURN DOWN",
	b"TURN LEFT",
	b"TURN RIGHT",
	b"MOVE FORWARD",
	b"MOVE BACKWARD",
	b"MOVE LEFT",
	b"MOVE RIGHT",
	b"WALK",
	b"JUMP",
	b"RELOAD",
	b"DROP WEAPON",
	b"ZOOM",
	b"FIRE MODE",
	b"SWITCH WEAPON",
	b"WEAPON 1",
	b"WEAPON 2",
	b"FIRE",
];

/// How many key binding rows the settings page has, which is the upstream loop's 18 and
/// the tail of `OPTIONS_P2_DATAS` after the 14 items before it.
const OPTIONS_KEY_DATAS: usize = 18;

/// `strlen` on a NUL terminated byte string.
fn cstr(bytes: &[u8]) -> &[u8] {
	&bytes[..bytes.iter().position(|&byte| byte == 0).unwrap_or(bytes.len())]
}

/// `strcpy` into one of the fixed buffers these screens hold. The upstream writes past
/// them when the source is longer, which is the one thing this cannot do.
fn write_cstr(dst: &mut [u8], src: &[u8]) {
	let len = src.len().min(dst.len() - 1);
	dst[..len].copy_from_slice(&src[..len]);
	dst[len] = 0;
}

/// `strcpy(buf, dir); strcat(buf, name);`, the way every path in this file is built.
fn joined(dir: &[u8], name: &[u8]) -> Vec<u8> {
	let mut path = Vec::with_capacity(dir.len() + name.len());
	path.extend_from_slice(dir);
	path.extend_from_slice(name);
	path
}

/// The colour set all three menu scrollbars are given, which upstream repeats verbatim
/// three times.
fn set_scrollbar_colors(scrollbar: &mut ScrollbarObject) {
	scrollbar.set_color(
		D3DGraphics::get_color_code(0.5, 0.5, 0.5, 0.5),
		D3DGraphics::get_color_code(0.6, 0.6, 0.25, 1.0),
		D3DGraphics::get_color_code(0.8, 0.8, 0.25, 1.0),
		D3DGraphics::get_color_code(0.4, 0.67, 0.57, 1.0),
		D3DGraphics::get_color_code(0.38, 0.77, 0.64, 1.0),
		D3DGraphics::get_color_code(0.6, 0.3, 0.25, 1.0),
		D3DGraphics::get_color_code(0.8, 0.3, 0.25, 1.0),
	);
}

/// One link text, as `CreateOptions` fills them in: the same six fields every entry sets
/// and the string.
fn link_text(
	pos_x: i32,
	pos_y: i32,
	text_w: i32,
	text_h: i32,
	default_color: u32,
	cursor_color: u32,
	text: &[u8],
) -> MenuLinkTextData {
	let mut link = MenuLinkTextData {
		enable: true,
		pos_x,
		pos_y,
		text_w,
		text_h,
		text_defaultcolor: default_color,
		text_cursorcolor: cursor_color,
		..MenuLinkTextData::default()
	};
	write_cstr(&mut link.text, text);
	link
}

/// The upstream inline test for one link text's rectangle: as wide as the text's
/// character count times `text_w`, and `text_h` tall from `pos_y`.
fn link_text_hit(
	link: &MenuLinkTextData,
	base_x: i32,
	base_y: i32,
	mouse_x: i32,
	mouse_y: i32,
) -> bool {
	let text_len = cstr(&link.text).len() as i32;
	base_x + link.pos_x < mouse_x
		&& mouse_x < base_x + link.pos_x + text_len * link.text_w
		&& base_y + link.pos_y < mouse_y
		&& mouse_y < base_y + link.pos_y + link.text_h
}

/// The upstream inline test for one mission row: 20 pixels per character, rows 30 pixels
/// apart starting 30 below the menu's top edge, and 26 pixels of text in each row.
fn mission_row_hit(
	name: &[u8],
	row: i32,
	mouse_x: i32,
	mouse_y: i32,
	config: &Config,
) -> bool {
	let menu_x = mainmenu_x(config);
	let menu_y = mainmenu_y(config);
	menu_x < mouse_x
		&& mouse_x < menu_x + name.len() as i32 * 20
		&& menu_y + 30 + row * 30 < mouse_y
		&& mouse_y < menu_y + 30 + row * 30 + 26
}

/// The upstream's plain rectangle test, which every clickable row of the menu uses. The
/// bounds are open at both ends, so the pixel on either edge is outside.
fn rect_hit(x1: i32, y1: i32, x2: i32, y2: i32, mouse_x: i32, mouse_y: i32) -> bool {
	x1 < mouse_x && mouse_x < x2 && y1 < mouse_y && mouse_y < y2
}

/// The same test as the menu's own text uses it, with the scrollbar drag suppressing the
/// highlight: a text that lights up under the cursor must not light up while the knob is
/// being dragged past it.
fn hovered(x1: i32, y1: i32, x2: i32, y2: i32, mouse_x: i32, mouse_y: i32, scrolling: bool) -> bool {
	!scrolling && rect_hit(x1, y1, x2, y2, mouse_x, mouse_y)
}

/// The name drawn on one mission row, which is where upstream branches on
/// `GameInfoData.selectmission_id >= 0`.
///
/// An id outside either list reads empty rather than being dropped, which is what the
/// upstream writes into its `char name[32]` before asking: the official table answers 0
/// without touching the buffer, and the addon table's unpublished entries are zeroed.
fn mission_name<'m>(
	index: i32,
	game_info: &GameInfo,
	addon: &'m AddonList,
	param: &ParameterInfo,
) -> &'m [u8] {
	if game_info.selectmission_id >= 0 {
		param
			.get_official_mission(index)
			.map_or(b"".as_slice(), |mission| mission.name)
	} else {
		addon
			.get_mission_name_of(index)
			.map_or(b"".as_slice(), |name| cstr(name))
	}
}

/// `strcpy(mifpath, ADDONDIR); strcat(mifpath, "\\"); strcat(mifpath, GetFileName(id))`.
fn set_mifpath(game_info: &mut GameInfo, id: i32, addon: &AddonList) {
	let name = addon.get_filename(id).map_or(b"".as_slice(), |name| cstr(name));
	let mut path = Vec::with_capacity(ADDONDIR.len() + 1 + name.len());
	path.extend_from_slice(ADDONDIR);
	path.push(b'\\');
	path.extend_from_slice(name);
	write_cstr(&mut game_info.mifpath, &path);
}

/// The upstream key binding capture, `InputOptions`'s first block on the settings page.
///
/// The first rebindable key *released* this frame becomes the binding, and the capture
/// ends on it: the id and the count are both cleared, which is what stops the countdown
/// from taking the next key. Returns whether a key was taken, which is the upstream's
/// early `return` from `InputOptions`: the frame it fires, no other link text may react.
///
/// `keycode` is what `CheckInputAll` reports, -1 when nothing usable came up.
fn capture_key(
	keycode: i32,
	key_config_id: &mut i32,
	key_config_cnt: &mut i32,
	config: &mut Config,
) -> bool {
	if *key_config_id == -1 || *key_config_cnt <= 0 {
		return false;
	}
	if keycode == -1 {
		return false;
	}

	config.set_keycode(*key_config_id, keycode);
	*key_config_id = -1;
	*key_config_cnt = 0;
	true
}

/// One link text of an options page: the black shadow one pixel down and right when the
/// entry is enabled, then the text itself, in the cursor colour while the mouse is over
/// it and grey when the entry is disabled.
fn draw_link_text(
	d3dg: &mut D3DGraphics,
	link: &MenuLinkTextData,
	base_x: i32,
	base_y: i32,
	mouse_x: i32,
	mouse_y: i32,
) {
	if !link.enable {
		d3dg.draw_2d_texture_font_text(
			base_x + link.pos_x,
			base_y + link.pos_y,
			cstr(&link.text),
			D3DGraphics::get_color_code(0.6, 0.6, 0.6, 1.0),
			link.text_w,
			link.text_h,
		);
		return;
	}

	let color = if link_text_hit(link, base_x, base_y, mouse_x, mouse_y) {
		link.text_cursorcolor
	} else {
		link.text_defaultcolor
	};

	d3dg.draw_2d_texture_font_text(
		base_x + link.pos_x + 1,
		base_y + link.pos_y + 1,
		cstr(&link.text),
		D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
		link.text_w,
		link.text_h,
	);
	d3dg.draw_2d_texture_font_text(
		base_x + link.pos_x,
		base_y + link.pos_y,
		cstr(&link.text),
		color,
		link.text_w,
		link.text_h,
	);
}

// ---------------------------------------------------------------------------
// The opening screen
// ---------------------------------------------------------------------------

/// `opening`: a slowly moving camera over `data\map10` with the title fading in.
///
/// The six `add_camera_*` fields are the animation: an amount added to the camera each
/// frame, which ramps up and then is either held or damped depending on the second the
/// screen is in.
pub struct Opening<'a> {
	base: D3Dscene,
	world: World<'a>,
	add_camera_x: f32,
	add_camera_y: f32,
	add_camera_z: f32,
	add_camera_rx: f32,
	add_camera_ry: f32,
}

impl<'a> Opening<'a> {
	/// The upstream constructor. The camera itself is placed in
	/// [`Opening::create`], which is where upstream places it too.
	/// Hands the world to the screen that comes next. The screens own the world rather than
	/// borrowing it: a screen lives for many frames, and a struct that holds a borrow of the
	/// world cannot be stored beside the world itself, which is exactly what a scheduler
	/// does. Owning it costs one move per screen change.
	pub fn into_world(self) -> World<'a> {
		self.world
	}

	pub fn new(world: World<'a>) -> Opening<'a> {
		Opening {
			base: D3Dscene::new(),
			world,
			add_camera_x: 0.0,
			add_camera_y: 0.0,
			add_camera_z: 0.0,
			add_camera_rx: 0.0,
			add_camera_ry: 0.0,
		}
	}

	/// The upstream `opening::Create`.
	///
	/// Upstream this is one of the few `Create`s that is real work: it loads the opening's
	/// own map, hands the collision its blocks, builds the demo objects and places the
	/// camera. The three things `scene::Create` would do are done at the end by calling it,
	/// which is the same three the upstream writes out by hand.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		window: &WindowControl,
		config: &Config,
		input_ctrl: &InputControl,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundManager<'_>,
	) -> i32 {
		// The world lends the resource exclusively here, which is what the map load, the
		// object build and the sky all need. Every other phase only reads it.
		let resource = &mut self.world.resource;

		let blockflag = self.world.block_data.load_file_data(OPENING_BLOCK);
		let pointflag = self.world.point_data.load_file_data(OPENING_POINT);

		// 2 bits: point data open failed, 1 bit: block data open failed.
		if blockflag != 0 || pointflag != 0 {
			return pointflag * 2 + blockflag;
		}

		self.world.block_data.calculation_blockdata(false);
		d3dg.load_mapdata(self.world.block_data.clone(), OPENING_DIR);
		self.world.collision.init_collision(Some(&self.world.block_data));

		self.world.objects.load_point_data(
			d3dg,
			resource,
			&self.world.point_data,
			&self.world.collision,
			&self.world.block_data,
			self.world.mif.as_ref(),
		);

		// A nonexistent person as the player, which is the upstream's countermeasure
		// against a gunshot sounding from nowhere in the demo.
		self.world.objects.set_player_id(MAX_HUMAN as i32 - 1);
		self.world.objects.set_add_collision_flag(false);

		// The AI of every human, each one pointed at its own person number and started on
		// that human's path. `SetClass` upstream also re-points the object manager, the
		// map and the settings, which are parameters here.
		for i in 0..MAX_HUMAN {
			self.world.ai[i].set_class(i as i32);
			self.world.ai[i].init(&self.world.objects, &self.world.point_data);
		}

		resource.load_sky_model_texture(d3dg, 1);
		sound.init_world_sound();

		// `inputCtrl->MoveMouseCenter(); framecnt = 0; GameState->NextState();`, which is
		// exactly `scene::Create`.
		self.base
			.scene
			.create(state, window, config, input_ctrl, log);

		self.base.camera_x = -5.0;
		self.base.camera_y = 58.0;
		self.base.camera_z = 29.0;
		self.base.camera_rx = degree_to_radian(206.0);
		self.base.camera_ry = degree_to_radian(12.0);
		self.add_camera_x = 0.0;
		self.add_camera_y = 0.0;
		self.add_camera_z = 0.0;
		self.add_camera_rx = 0.0;
		self.add_camera_ry = 0.0;

		0
	}

	/// The upstream `opening::Recovery`, which is called after the renderer was reset.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		let resource = &mut self.world.resource;

		d3dg.load_mapdata(self.world.block_data.clone(), OPENING_DIR);
		self.world.objects.recovery(d3dg, resource);

		resource.cleanup_sky_model_texture(d3dg);
		resource.load_sky_model_texture(d3dg, 1);

		0
	}

	/// The upstream `opening::Input`. Escape leaves the screen, and a left click or Enter
	/// counts as a mouse click, which is what carries the state machine to the menu.
	pub fn input(
		&mut self,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		state: &mut StateMachine,
		log: &mut DebugLog,
	) {
		input_ctrl.get_input_state(input, false);

		if input_ctrl.check_key_down(get_esc_keycode()) {
			state.push_back_space_key(log);
		} else if input_ctrl.check_mouse_button_up_l() {
			state.push_mouse_button(log);
		} else if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x0F)) {
			// [ENTER] also counts as a mouse click.
			state.push_mouse_button(log);
		}
	}

	/// The upstream `opening::Process`: the camera work, then the frame count.
	///
	/// The `framecnt` tests are the upstream's `(int)(seconds * GAMEFPS)` casts, computed
	/// the same way so the frame each stage starts on is the frame upstream starts on. The
	/// last test is not one of those: `16*((int)GAMEFPS)` truncates the frame rate first,
	/// so the screen ends at 528 frames and not at 533.
	pub fn process(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		config: &Config,
		sound: &mut SoundManager<'_>,
	) {
		self.world.objects.process(
			-1,
			false,
			false,
			d3dg,
			&*self.world.resource,
			&self.world.collision,
			&self.world.block_data,
			self.world.mif.as_ref(),
			sound,
			config,
		);

		// The AI moves the humans, which is the only thing that makes the opening's camera
		// drift have anything to watch.
		for i in 0..MAX_HUMAN {
			self.world.ai[i].process(
				&mut self.world.objects,
				&self.world.block_data,
				&self.world.point_data,
				&self.world.collision,
				sound,
				d3dg,
				&*self.world.resource,
				config,
			);
		}

		let framecnt = self.base.scene.framecnt;

		// Camera work (position): still for four seconds, then easing into a slow
		// sideways and downward drift over the fifth, then holding that speed.
		if framecnt < (4.0 * GAMEFPS) as u32 {
			self.add_camera_z = 0.0;
			self.add_camera_y = 0.0;
		} else if framecnt < (5.0 * GAMEFPS) as u32 {
			self.add_camera_z += (0.08 - self.add_camera_z) / 5.0;
			self.add_camera_y += (-0.05 - self.add_camera_y) / 5.0;
		} else {
			self.add_camera_z = 0.08;
			self.add_camera_y = -0.05;
		}
		self.base.camera_x += self.add_camera_x;
		self.base.camera_y += self.add_camera_y;
		self.base.camera_z += self.add_camera_z;

		// Camera work (rotation): still for 2.6 seconds, easing into a turn over the next
		// second, holding it for the rest of the fourth and fifth, then damping it to a
		// stop.
		if framecnt < (2.6 * GAMEFPS) as u32 {
			self.add_camera_rx = 0.0;
			self.add_camera_ry = 0.0;
		} else if framecnt < (3.6 * GAMEFPS) as u32 {
			self.add_camera_rx += (degree_to_radian(0.9) - self.add_camera_rx) / 5.0;
			self.add_camera_ry += (degree_to_radian(-0.6) - self.add_camera_ry) / 5.0;
		} else if framecnt < (5.0 * GAMEFPS) as u32 {
			self.add_camera_rx = degree_to_radian(0.9);
			self.add_camera_ry = degree_to_radian(-0.6);
		} else {
			self.add_camera_rx *= 0.8;
			self.add_camera_ry *= 0.8;
		}
		self.base.camera_rx += self.add_camera_rx;
		self.base.camera_ry += self.add_camera_ry;

		// End after 16 seconds.
		if framecnt >= 16 * (GAMEFPS as u32) {
			state.push_mouse_button(log);
		}

		self.base.scene.framecnt = self.base.scene.framecnt.wrapping_add(1);
	}

	/// The upstream `opening::Render3D`.
	fn render_3d(&mut self, d3dg: &mut D3DGraphics, _config: &Config) {
		d3dg.set_fog(true, 1);
		d3dg.set_camera(
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
			crate::gamemain::VIEWANGLE_NORMAL,
		);

		// The background sky is drawn at the camera position. Upstream also switches
		// backface culling off around this one model for the browser build, which is an
		// Emscripten only workaround and has no counterpart here.
		d3dg.set_world_transform(
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			0.0,
			0.0,
			2.0,
		);
		let (skymodel, skytexture) = self.world.resource.get_sky_model_texture();
		d3dg.render_model(skymodel, skytexture, false, false, false);

		// A mid-frame depth clear, so the sky cannot occlude the map.
		d3dg.reset_zbuffer();

		d3dg.reset_world_transform();
		d3dg.render_mapdata(false);

		self.world.objects.render(
			d3dg,
			&*self.world.resource,
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
			0,
			false,
		);
	}

	/// The upstream `opening::Render2D`: the title sequence over the darkened world.
	fn render_2d(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let framecnt = self.base.scene.framecnt;
		let screen_width = config.get_screen_width();
		let screen_height = config.get_screen_height();
		let scaling_y = screen_height as f32 / 480.0;
		let swidth = (screen_width as f32 / screen_height as f32 * 480.0) as i32;
		let mut effect = 0.0f32;

		// The fade in, the clear middle, and the fade out, each starting on the frame the
		// upstream's own cast says.
		if framecnt < (2.0 * GAMEFPS) as u32 {
			effect = self
				.base
				.scene
				.get_effect_alpha(framecnt, 1.0, 2.0, 0.0, true);
		}
		if (2.0 * GAMEFPS) as u32 <= framecnt && framecnt < (11.0 * GAMEFPS) as u32 {
			effect = 0.0;
		}
		if (11.0 * GAMEFPS) as u32 <= framecnt && framecnt < (15.0 * GAMEFPS) as u32 {
			effect = self
				.base
				.scene
				.get_effect_alpha(framecnt, 1.0, 4.0, 11.0, false);
		}
		if (15.0 * GAMEFPS) as u32 <= framecnt {
			effect = 1.0;
		}
		d3dg.draw_2d_box(
			0,
			0,
			screen_width,
			screen_height,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, effect),
		);

		// The letterbox bars.
		d3dg.draw_2d_box(
			0,
			0,
			screen_width,
			(scaling_y * 40.0) as i32,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
		);
		d3dg.draw_2d_box(
			0,
			screen_height - (scaling_y * 40.0) as i32,
			screen_width,
			screen_height,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
		);

		// The project name, from half a second in to four seconds.
		if ((0.5 * GAMEFPS) as u32) < framecnt && framecnt < (4.0 * GAMEFPS) as u32 {
			let mut name = GAMENAME.as_bytes().to_vec();
			name.extend_from_slice(b" project");
			let mut effect_a = 1.0;
			if framecnt < (1.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 0.5, false);
			}
			if framecnt > (3.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 3.0, true);
			}
			d3dg.draw_2d_texture_font_text_center_scaling(
				0,
				340,
				&name,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				22,
				22,
			);
		}

		// The first pair of names.
		if ((4.5 * GAMEFPS) as u32) < framecnt && framecnt < (8.5 * GAMEFPS) as u32 {
			let mut effect_a = 1.0;
			if framecnt < (5.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 4.5, false);
			}
			if framecnt > (7.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 7.5, true);
			}
			d3dg.draw_2d_texture_font_text_scaling(
				60,
				150,
				b"ORIGINAL",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				20,
				20,
			);
		}
		if ((5.0 * GAMEFPS) as u32) < framecnt && framecnt < (9.0 * GAMEFPS) as u32 {
			let mut effect_a = 1.0;
			if framecnt < (6.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 5.0, false);
			}
			if framecnt > (8.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 8.0, true);
			}
			d3dg.draw_2d_texture_font_text_scaling(
				100,
				180,
				b"nine-two",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				20,
				20,
			);
			d3dg.draw_2d_texture_font_text_scaling(
				100,
				210,
				b"TENNKUU",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				20,
				20,
			);
		}

		// The second pair.
		if ((7.0 * GAMEFPS) as u32) < framecnt && framecnt < (11.0 * GAMEFPS) as u32 {
			let mut effect_a = 1.0;
			if framecnt < (8.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 7.0, false);
			}
			if framecnt > (10.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 10.0, true);
			}
			d3dg.draw_2d_texture_font_text_scaling(
				swidth - 310,
				300,
				b"REMAKE",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				20,
				20,
			);
		}
		if ((7.5 * GAMEFPS) as u32) < framecnt && framecnt < (11.5 * GAMEFPS) as u32 {
			let mut effect_a = 1.0;
			if framecnt < (8.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 7.5, false);
			}
			if framecnt > (10.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 10.5, true);
			}
			d3dg.draw_2d_texture_font_text_scaling(
				swidth - 270,
				330,
				b"[-_-;](mikan)",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, effect_a),
				20,
				20,
			);
		}

		// The game name, in red, from twelve seconds until the screen ends.
		if ((12.0 * GAMEFPS) as u32) <= framecnt {
			let mut effect_a = 1.0;
			if framecnt < (13.0 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 1.0, 12.0, false);
			}
			if ((15.0 * GAMEFPS) as u32) < framecnt && framecnt < (15.5 * GAMEFPS) as u32 {
				effect_a = self
					.base
					.scene
					.get_effect_alpha(framecnt, 1.0, 0.5, 15.0, true);
			}
			if framecnt >= (15.5 * GAMEFPS) as u32 {
				effect_a = 0.0;
			}
			d3dg.draw_2d_texture_font_text_center_scaling(
				0,
				(480 - 11) / 2,
				GAMENAME.as_bytes(),
				D3DGraphics::get_color_code(1.0, 0.0, 0.0, effect_a),
				22,
				22,
			);
		}
	}

	/// [`D3Dscene::sound`] for this screen, the world sound positioned at the camera. The
	/// frame runs `Input`, `Process` and then this.
	pub fn sound(&mut self, game_sound: &mut SoundManager<'_>) {
		self.base.sound(game_sound);
	}

	/// [`D3Dscene::render_main`] for this screen.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		D3Dscene::render_main(d3dg, config, self, Self::render_3d, Self::render_2d)
	}

	/// The upstream `opening::Destroy`.
	pub fn destroy(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
	) {
		let resource = &mut self.world.resource;

		d3dg.cleanup_mapdata();
		self.world.objects.cleanup(d3dg, resource);
		resource.cleanup_sky_model_texture(d3dg);

		state.next_state(log);
	}
}

// ---------------------------------------------------------------------------
// The menu
// ---------------------------------------------------------------------------

/// `mainmenu`: the mission list, the add-ons behind it, and the options pages.
pub struct MainMenu<'a> {
	base: D3Dscene,
	world: World<'a>,

	/// The demo map loaded behind the menu.
	demopath: [u8; MAX_PATH],
	/// Which screen is showing: 0 the mission list, 1 the options page, 2 the key
	/// settings, 3 the licence text.
	modescreen: i32,
	mainmenu_mouse_x: i32,
	mainmenu_mouse_y: i32,
	/// The previous frame's `mainmenu_mouse_y`, which is the movement a scrollbar drag
	/// follows.
	mainmenu_mouse_y_old: i32,
	scrollbar_official: ScrollbarObject,
	scrollbar_addon: ScrollbarObject,
	/// The title image, or -1 when it did not load.
	gametitle: i32,

	options_p1_link_text_data: [MenuLinkTextData; OPTIONS_P1_DATAS],
	options_p2_link_text_data: [MenuLinkTextData; OPTIONS_P2_DATAS],
	options_p3_link_text_data: [MenuLinkTextData; OPTIONS_P3_DATAS],
	scrollbar_license: ScrollbarObject,
	/// The link text of page 2 that was picked this frame, or -1.
	options_p2_link_text_id: i32,
	/// The fullscreen flag the settings page started with, so a failed switch can be
	/// undone and an unchanged one skipped.
	options_fullscreen_flag_old: bool,
	/// The key setting being captured, or -1.
	options_key_config_id: i32,
	/// Frames left before that capture is dropped.
	options_key_config_cnt: i32,
	/// The licence lines the licence page last built. Upstream this is a `new char[100*18]`
	/// it `strcat`s into every frame.
	licenseinfobuf: Vec<u8>,
}

impl<'a> MainMenu<'a> {
	/// The upstream constructor.
	///
	/// Two fields are worth naming. `Options_FullscreenFlag_old` reads the setting out of
	/// the global config upstream, and `CreateOptions` sets it again before anything reads
	/// it, so it starts false here. `licenseinfobuf` is a heap buffer upstream and a `Vec`
	/// here.
	/// Hands the world to the screen that comes next. The screens own the world rather than
	/// borrowing it: a screen lives for many frames, and a struct that holds a borrow of the
	/// world cannot be stored beside the world itself, which is exactly what a scheduler
	/// does. Owning it costs one move per screen change.
	pub fn into_world(self) -> World<'a> {
		self.world
	}

	pub fn new(world: World<'a>) -> MainMenu<'a> {
		MainMenu {
			base: D3Dscene::new(),
			world,
			demopath: [0; MAX_PATH],
			modescreen: 0,
			mainmenu_mouse_x: 0,
			mainmenu_mouse_y: 0,
			mainmenu_mouse_y_old: 0,
			scrollbar_official: ScrollbarObject::new(),
			scrollbar_addon: ScrollbarObject::new(),
			gametitle: -1,
			options_p1_link_text_data: [MenuLinkTextData::default(); OPTIONS_P1_DATAS],
			options_p2_link_text_data: [MenuLinkTextData::default(); OPTIONS_P2_DATAS],
			options_p3_link_text_data: [MenuLinkTextData::default(); OPTIONS_P3_DATAS],
			scrollbar_license: ScrollbarObject::new(),
			options_p2_link_text_id: -1,
			options_fullscreen_flag_old: false,
			options_key_config_id: -1,
			options_key_config_cnt: 0,
			licenseinfobuf: Vec::new(),
		}
	}

	/// The upstream `mainmenu::Create`.
	pub fn create(
		&mut self,
		state: &mut StateMachine,
		window: &WindowControl,
		config: &Config,
		input_ctrl: &InputControl,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
		sound: &mut SoundManager<'_>,
	) -> i32 {
		let resource = &mut self.world.resource;

		// One of the six demo maps, at random.
		let demo = MAINMENU_DEMOS[get_rand(MAINMENU_DEMOS.len() as i32) as usize];
		write_cstr(&mut self.demopath, demo);

		let blockflag = self
			.world
			.block_data
			.load_file_data(&joined(&self.demopath, b"temp.bd1"));
		let pointflag = self
			.world
			.point_data
			.load_file_data(&joined(&self.demopath, b"demo.pd1"));

		// 2 bits: point data open failed, 1 bit: block data open failed.
		if blockflag != 0 || pointflag != 0 {
			return pointflag * 2 + blockflag;
		}

		self.world.block_data.calculation_blockdata(false);
		d3dg.load_mapdata(self.world.block_data.clone(), &self.demopath);
		self.world
			.collision
			.init_collision(Some(&self.world.block_data));

		self.world.objects.load_point_data(
			d3dg,
			resource,
			&self.world.point_data,
			&self.world.collision,
			&self.world.block_data,
			self.world.mif.as_ref(),
		);

		// Every human's AI, pointed at its own person number and started on its path, the
		// same as the opening's demo world.
		for i in 0..MAX_HUMAN {
			self.world.ai[i].set_class(i as i32);
			self.world.ai[i].init(&self.world.objects, &self.world.point_data);
		}

		self.modescreen = 0;
		self.gametitle = d3dg.load_texture(TITLE_TEXTURE, false, false);
		sound.init_world_sound();

		self.mainmenu_mouse_x = config.get_screen_width() / 2;
		self.mainmenu_mouse_y = config.get_screen_height() / 2;
		self.mainmenu_mouse_y_old = self.mainmenu_mouse_y;

		self.create_scrollbars(config);

		if ENABLE_MENUOPTIONS {
			self.create_options(config);
		}

		// `inputCtrl->MoveMouseCenter(); framecnt = 0; GameState->NextState();`.
		self.base
			.scene
			.create(state, window, config, input_ctrl, log);

		0
	}

	/// The two mission scrollbars, at the menu's right edge, from `mainmenu::Create`.
	///
	/// Both are handed the same colour set. The totals they cover are why the official
	/// bar has travel and the addon bar has none when there are no addons, and they are
	/// read off the two lists, so this is also what `Create` needs the addon list for.
	fn create_scrollbars(&mut self, config: &Config) {
		let menu_x = mainmenu_x(config);
		let menu_y = mainmenu_y(config);
		let x = menu_x + 341;
		let y = menu_y + 1;
		let width = 19;
		let height = MAINMENU_H - 1 - 24;

		let scrollitem = self.scrollbar_official.get_scroll_item();
		self.scrollbar_official.create(
			x,
			y,
			width,
			height,
			TOTAL_OFFICIALMISSION as i32,
			TOTAL_MENUITEMS,
			scrollitem,
		);
		set_scrollbar_colors(&mut self.scrollbar_official);

		let scrollitem = self.scrollbar_addon.get_scroll_item();
		self.scrollbar_addon.create(
			x,
			y,
			width,
			height,
			self.world.addon.get_total_datas(),
			TOTAL_MENUITEMS,
			scrollitem,
		);
		set_scrollbar_colors(&mut self.scrollbar_addon);
	}

	/// The upstream `mainmenu::Recovery`.
	pub fn recovery(&mut self, d3dg: &mut D3DGraphics) -> i32 {
		let resource = &mut self.world.resource;

		d3dg.load_mapdata(self.world.block_data.clone(), &self.demopath);
		self.world.objects.recovery(d3dg, resource);
		self.gametitle = d3dg.load_texture(TITLE_TEXTURE, false, false);

		0
	}

	/// The upstream `mainmenu::Input`, the mission list and the options screens.
	pub fn input(
		&mut self,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		state: &mut StateMachine,
		log: &mut DebugLog,
		config: &mut Config,
	) {
		input_ctrl.get_input_state(input, false);

		// The absolute cursor, which is the mode this screen reads: the menu is driven by
		// a cursor the player can see, and the movement it produces is kept separately for
		// the scrollbar drag.
		let (mouse_x, mouse_y) = input_ctrl.get_mouse_movement();
		self.mainmenu_mouse_x = mouse_x;
		self.mainmenu_mouse_y = mouse_y;

		if self.modescreen == 0 {
			self.input_missionlist(input_ctrl, state, log, config);
		} else if ENABLE_MENUOPTIONS {
			self.input_options(input_ctrl, config);
		}

		self.mainmenu_mouse_y_old = mouse_y;
	}

	/// The mission list half of [`MainMenu::input`].
	fn input_missionlist(
		&mut self,
		input_ctrl: &InputControl,
		state: &mut StateMachine,
		log: &mut DebugLog,
		config: &Config,
	) {
		let mouse_x = self.mainmenu_mouse_x;
		let mouse_y = self.mainmenu_mouse_y;
		let menu_x = mainmenu_x(config);
		let menu_y = mainmenu_y(config);

		// The scrollbar under the list, which is the one the selection is on.
		let scrollitems = if self.world.game_info.selectmission_id >= 0 {
			self.scrollbar_official.input(
				mouse_x,
				mouse_y,
				input_ctrl.check_mouse_button_now_l(),
				mouse_y - self.mainmenu_mouse_y_old,
			);
			self.scrollbar_official.get_scroll_item()
		} else {
			self.scrollbar_addon.input(
				mouse_x,
				mouse_y,
				input_ctrl.check_mouse_button_now_l(),
				mouse_y - self.mainmenu_mouse_y_old,
			);
			self.scrollbar_addon.get_scroll_item()
		};

		// A click on a mission row takes the mission. The official list stores its own
		// number and the addon list hands over a file name, which is what the mission
		// loader reads back later.
		if input_ctrl.check_mouse_button_up_l() {
			for i in 0..TOTAL_MENUITEMS {
				let selectmission_id = self.world.game_info.selectmission_id;
				let name = mission_name(
					scrollitems + i,
					&self.world.game_info,
					&self.world.addon,
					&self.world.param,
				);
				if !mission_row_hit(name, i, mouse_x, mouse_y, config) {
					continue;
				}

				if selectmission_id >= 0 {
					self.world.game_info.selectmission_id = scrollitems + i;
					self.world.game_info.mifpath = [0; MAX_PATH];
				} else {
					self.world.game_info.selectmission_id = -1;
					set_mifpath(
						&mut self.world.game_info,
						scrollitems + i,
						&self.world.addon,
					);
				}
				state.push_mouse_button(log);
			}
		}

		if input_ctrl.check_mouse_button_up_l() {
			// UP, which is the row above the list.
			if rect_hit(menu_x, menu_y, menu_x + 340, menu_y + 30, mouse_x, mouse_y) {
				if self.world.game_info.selectmission_id >= 0 {
					self.scrollbar_official.scroll_up();
				} else {
					self.scrollbar_addon.scroll_up();
				}
			}

			// DOWN, which is the row above the switch under it.
			if rect_hit(
				menu_x,
				menu_y + MAINMENU_H - 55,
				menu_x + 340,
				menu_y + MAINMENU_H - 55 + 30,
				mouse_x,
				mouse_y,
			) {
				if self.world.game_info.selectmission_id >= 0 {
					self.scrollbar_official.scroll_down();
				} else {
					self.scrollbar_addon.scroll_down();
				}
			}
		}

		// The switch between the standard missions and the add-ons. Back to the standard
		// list is always offered, and to the add-ons only when there are any.
		if input_ctrl.check_mouse_button_up_l()
			&& rect_hit(
				menu_x,
				menu_y + MAINMENU_H - 25,
				menu_x + 340,
				menu_y + MAINMENU_H - 2,
				mouse_x,
				mouse_y,
			)
		{
			if self.world.game_info.selectmission_id >= 0 {
				if self.world.addon.get_total_datas() > 0 {
					self.world.game_info.selectmission_id = -1;
				}
			} else {
				self.world.game_info.selectmission_id = 0;
			}
		}

		if ENABLE_MENUOPTIONS {
			let option_y = mainmenu_option_y(config);
			// The clickable area is 120 wide where the drawn box is 134, which is the
			// upstream as written: the right hand strip of the box is inert.
			if input_ctrl.check_mouse_button_up_l()
				&& rect_hit(
					MAINMENU_OPTION_X,
					option_y,
					MAINMENU_OPTION_X + 120,
					option_y + 16,
					mouse_x,
					mouse_y,
				)
			{
				self.modescreen = 1;
			}
		}

		if input_ctrl.check_key_down(get_esc_keycode()) {
			state.push_back_space_key(log);
		}
	}

	/// The upstream `mainmenu::InputOptions`, one branch per options page.
	fn input_options(&mut self, input_ctrl: &InputControl, config: &mut Config) {
		self.options_p2_link_text_id = -1;

		if self.modescreen == 1 {
			let base_x = options_p1_x(config);
			let base_y = options_p1_y(config);
			let mut button_id = -1;
			for i in 0..OPTIONS_P1_DATAS {
				if !self.options_p1_link_text_data[i].enable {
					continue;
				}
				if input_ctrl.check_mouse_button_up_l()
					&& link_text_hit(
						&self.options_p1_link_text_data[i],
						base_x,
						base_y,
						self.mainmenu_mouse_x,
						self.mainmenu_mouse_y,
					) {
					button_id = i as i32;
				}
			}

			if button_id == 0 {
				self.modescreen = 2;
			}
			if button_id == 1 {
				self.modescreen = 3;

				// The licence text starts at the top.
				self.scrollbar_license.set_scroll_item(0);
				self.scrollbar_license.input(0, 0, false, 0);
			}
			if button_id == 2 {
				self.modescreen = 0;
			}

			if input_ctrl.check_key_down(get_esc_keycode()) {
				self.modescreen = 0;
			}
		} else if self.modescreen == 2 {
			// The key capture, which takes the first key the player releases while it is
			// running and then ends.
			let keycode = if self.options_key_config_id != -1 && self.options_key_config_cnt > 0 {
				input_ctrl.check_input_all()
			} else {
				-1
			};
			if capture_key(
				keycode,
				&mut self.options_key_config_id,
				&mut self.options_key_config_cnt,
				config,
			) {
				// On a click, end processing so other link texts do not react.
				return;
			}

			let base_x = options_p2_x(config);
			let base_y = options_p2_y(config);
			for i in 0..OPTIONS_P2_DATAS {
				if !self.options_p2_link_text_data[i].enable {
					continue;
				}
				if input_ctrl.check_mouse_button_up_l()
					&& link_text_hit(
						&self.options_p2_link_text_data[i],
						base_x,
						base_y,
						self.mainmenu_mouse_x,
						self.mainmenu_mouse_y,
					) {
					self.options_p2_link_text_id = i as i32;
				}
			}

			if input_ctrl.check_key_down(get_esc_keycode()) {
				// Upstream leaves `modescreen = 1` commented out here and only picks the
				// cancel link, so Escape is the same as clicking Cancel.
				self.options_p2_link_text_id = 2;
			}
		} else if self.modescreen == 3 {
			self.scrollbar_license.input(
				self.mainmenu_mouse_x,
				self.mainmenu_mouse_y,
				input_ctrl.check_mouse_button_now_l(),
				self.mainmenu_mouse_y - self.mainmenu_mouse_y_old,
			);

			let base_x = options_p3_x(config);
			let base_y = options_p3_y(config);
			let mut button_id = -1;
			for i in 0..OPTIONS_P3_DATAS {
				if !self.options_p3_link_text_data[i].enable {
					continue;
				}
				if input_ctrl.check_mouse_button_up_l()
					&& link_text_hit(
						&self.options_p3_link_text_data[i],
						base_x,
						base_y,
						self.mainmenu_mouse_x,
						self.mainmenu_mouse_y,
					) {
					button_id = i as i32;
				}
			}
			if button_id == 0 {
				self.modescreen = 1;
			}

			if input_ctrl.check_key_down(get_esc_keycode()) {
				self.modescreen = 1;
			}
		}
	}

	/// The upstream `mainmenu::Process`: the demo world behind the menu, the camera it is
	/// watched from, and the options pages.
	///
	/// The camera is read off the player human rather than interpolated, so the menu looks
	/// at whatever the demo's player is doing.
	pub fn process(
		&mut self,
		d3dg: &mut D3DGraphics,
		config: &mut Config,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		log: &mut DebugLog,
		sound: &mut SoundManager<'_>,
	) {
		self.world.objects.process(
			-1,
			true,
			false,
			d3dg,
			&*self.world.resource,
			&self.world.collision,
			&self.world.block_data,
			self.world.mif.as_ref(),
			sound,
			config,
		);

		for i in 0..MAX_HUMAN {
			self.world.ai[i].process(
				&mut self.world.objects,
				&self.world.block_data,
				&self.world.point_data,
				&self.world.collision,
				sound,
				d3dg,
				&*self.world.resource,
				config,
			);
		}

		if let Some(human) = self.world.objects.get_player_human_object() {
			let (x, y, z, _) = human.pos_data();
			self.base.camera_x = x - 4.0;
			self.base.camera_y = y + 22.0;
			self.base.camera_z = z - 12.0;
		}
		self.base.camera_rx = degree_to_radian(45.0);
		self.base.camera_ry = degree_to_radian(-25.0);

		if self.modescreen != 0 && ENABLE_MENUOPTIONS {
			self.process_options(
				d3dg, config, window, input_ctrl, input, log, sound,
			);
		}

		self.base.scene.framecnt = self.base.scene.framecnt.wrapping_add(1);
	}

	/// The upstream `mainmenu::ProcessOptions`, which turns this frame's picked link text
	/// into a setting, and a press on a key row into a capture.
	fn process_options(
		&mut self,
		d3dg: &mut D3DGraphics,
		config: &mut Config,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		log: &mut DebugLog,
		sound: &mut SoundManager<'_>,
	) {
		if self.modescreen != 2 {
			return;
		}

		// Default: back to the shipped settings.
		if self.options_p2_link_text_id == 0 {
			config.set_default_config();
		}

		// Save: apply the display mode, apply the sound level and write the file.
		if self.options_p2_link_text_id == 1 {
			if self.options_fullscreen_flag_old != config.get_fullscreen_flag() {
				let fullscreen = config.get_fullscreen_flag();
				if self.change_window_mode(d3dg, window, input_ctrl, input, config, fullscreen) != 0
				{
					// Restore the setting if it fails.
					config.set_fullscreen_flag(self.options_fullscreen_flag_old);
				} else {
					// Apply it if it succeeds.
					self.options_fullscreen_flag_old = config.get_fullscreen_flag();
				}
			}

			let volume = if !config.get_sound_flag() { 0.0 } else { 1.0 };
			sound.set_volume(volume);

			// Write the settings file. Upstream reports a failure here by leaving the
			// commented out call to the error dialog, and carries on either way.
			let _ = config.save_file("config.dat", log);

			self.options_key_config_id = -1;
			self.options_key_config_cnt = 0;

			self.modescreen = 1;
		}

		// Cancel: reload the settings file and forget what was changed on this page.
		if self.options_p2_link_text_id == 2 {
			let _ = config.load_file("config.dat", log);

			self.options_fullscreen_flag_old = config.get_fullscreen_flag();
			self.options_key_config_id = -1;
			self.options_key_config_cnt = 0;

			self.modescreen = 1;
		}

		// The mouse sensitivity and the brightness, one step per press and clamped where
		// the config clamps them.
		if self.options_p2_link_text_id == 3 {
			let mut param = config.get_mouse_sensitivity() - 1;
			if param < 10 {
				param = 10;
			}
			config.set_mouse_sensitivity(param);
		}
		if self.options_p2_link_text_id == 4 {
			let mut param = config.get_mouse_sensitivity() + 1;
			if param > 50 {
				param = 50;
			}
			config.set_mouse_sensitivity(param);
		}
		if self.options_p2_link_text_id == 5 {
			let mut param = config.get_brightness() - 1;
			if param < 0 {
				param = 0;
			}
			config.set_brightness(param);
		}
		if self.options_p2_link_text_id == 6 {
			let mut param = config.get_brightness() + 1;
			if param > 10 {
				param = 10;
			}
			config.set_brightness(param);
		}

		if self.options_p2_link_text_id == 7 {
			config.set_fullscreen_flag(true);
		}
		if self.options_p2_link_text_id == 8 {
			config.set_fullscreen_flag(false);
		}
		if self.options_p2_link_text_id == 9 {
			let flag = config.get_sound_flag();
			config.set_sound_flag(!flag);
		}
		if self.options_p2_link_text_id == 10 {
			let flag = config.get_blood_flag();
			config.set_blood_flag(!flag);
		}
		if self.options_p2_link_text_id == 11 {
			let flag = config.get_invert_mouse_flag();
			config.set_invert_mouse_flag(!flag);
		}
		if self.options_p2_link_text_id == 12 {
			let flag = config.get_frameskip_flag();
			config.set_frameskip_flag(!flag);
		}
		if self.options_p2_link_text_id == 13 {
			let flag = config.get_another_gunsight_flag();
			config.set_another_gunsight_flag(!flag);
		}

		// A press on a key row starts the capture, which runs for five seconds.
		for i in 0..OPTIONS_KEY_DATAS {
			if self.options_p2_link_text_id == 14 + i as i32 {
				self.options_key_config_id = i as i32;
				self.options_key_config_cnt = (GAMEFPS * 5.0) as i32;
			}
		}

		// The countdown, and the capture is dropped when it reaches zero.
		if self.options_key_config_id != -1 && self.options_key_config_cnt > 0 {
			self.options_key_config_cnt -= 1;
		} else {
			self.options_key_config_id = -1;
			self.options_key_config_cnt = 0;
		}
	}

	/// The display mode switch, the upstream `ChangeWindowMode` in `gamemain.cpp`.
	///
	/// Upstream this is the window mode, a full renderer reset (`ResetGame`) and then the
	/// screen's own `Recovery`. The renderer reset belongs to the frame host, which owns
	/// the swapchain, so what is left here is the mode itself, the recovery, and the
	/// input state the reset invalidates: the mouse movement caused by the resolution
	/// change is read out and thrown away, and the cursor goes back to the middle.
	///
	/// Returns 0 on success and 1 on failure, the two codes the caller branches on.
	fn change_window_mode(
		&mut self,
		d3dg: &mut D3DGraphics,
		window: &mut WindowControl,
		input_ctrl: &mut InputControl,
		input: &mut Input,
		config: &Config,
		fullscreen: bool,
	) -> i32 {
		d3dg.set_fullscreen_flag(fullscreen);

		if !window.change_window_mode(fullscreen) {
			// A window that cannot switch at all. Upstream returns 2 here and exits the
			// game. The caller only tells 0 from non-zero, and the host owns the exit.
			return 1;
		}

		self.recovery(d3dg);

		input_ctrl.get_input_state(input, true);
		input_ctrl.move_mouse_center(window, config);

		0
	}

	/// The upstream `mainmenu::Render3D`, the demo world behind the menu.
	fn render_3d(&mut self, d3dg: &mut D3DGraphics, _config: &Config) {
		d3dg.set_fog(true, 0);
		d3dg.set_camera(
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
			crate::gamemain::VIEWANGLE_NORMAL,
		);

		d3dg.reset_zbuffer();
		d3dg.reset_world_transform();
		d3dg.render_mapdata(false);

		self.world.objects.render(
			d3dg,
			&*self.world.resource,
			self.base.camera_x,
			self.base.camera_y,
			self.base.camera_z,
			self.base.camera_rx,
			self.base.camera_ry,
			0,
			false,
		);
	}

	/// The upstream `mainmenu::Render2D`.
	fn render_2d(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let screen_width = config.get_screen_width();
		let screen_height = config.get_screen_height();
		let swidth = (screen_width as f32 / screen_height as f32 * 480.0) as i32;
		let mouse_x = self.mainmenu_mouse_x;
		let mouse_y = self.mainmenu_mouse_y;

		// The version, with a one pixel shadow.
		d3dg.draw_2d_texture_font_text_scaling(
			swidth - 118 + 1,
			75 + 1,
			GAMEVERSION.as_bytes(),
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
			18,
			22,
		);
		d3dg.draw_2d_texture_font_text_scaling(
			swidth - 118,
			75,
			GAMEVERSION.as_bytes(),
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			18,
			22,
		);

		if self.modescreen == 0 {
			self.render_missionlist(d3dg, config);
		} else if ENABLE_MENUOPTIONS {
			self.render_options(d3dg, config);
		}

		// The cursor, drawn as a red cross over everything.
		d3dg.draw_2d_box(
			0,
			mouse_y - 1,
			screen_width,
			mouse_y + 1,
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 0.5),
		);
		d3dg.draw_2d_box(
			mouse_x - 1,
			0,
			mouse_x + 1,
			screen_height,
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 0.5),
		);
		d3dg.draw_2d_line(
			0,
			mouse_y,
			screen_width,
			mouse_y,
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0),
		);
		d3dg.draw_2d_line(
			mouse_x,
			0,
			mouse_x,
			screen_height,
			D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0),
		);

		d3dg.draw_2d_texture_scaling(20, 25, self.gametitle, 480, 80, 1.0);

		// The menu fades in over two seconds, unlike the opening's own fade.
		let framecnt = self.base.scene.framecnt;
		let effect = if framecnt < (2.0 * GAMEFPS) as u32 {
			self.base
				.scene
				.get_effect_alpha(framecnt, 1.0, 2.0, 0.0, true)
		} else {
			0.0
		};
		d3dg.draw_2d_box(
			0,
			0,
			screen_width,
			screen_height,
			D3DGraphics::get_color_code(0.0, 0.0, 0.0, effect),
		);
	}

	/// The mission list half of [`MainMenu::render_2d`].
	fn render_missionlist(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let mouse_x = self.mainmenu_mouse_x;
		let mouse_y = self.mainmenu_mouse_y;
		let menu_x = mainmenu_x(config);
		let menu_y = mainmenu_y(config);
		let addon_total = self.world.addon.get_total_datas();

		let (scrollitems, totalmission, scrolling) = if self.world.game_info.selectmission_id >= 0 {
			(
				self.scrollbar_official.get_scroll_item(),
				TOTAL_OFFICIALMISSION as i32,
				self.scrollbar_official.check_scrolling(),
			)
		} else {
			(
				self.scrollbar_addon.get_scroll_item(),
				addon_total,
				self.scrollbar_addon.check_scrolling(),
			)
		};

		// The list's backing. With add-ons the box is 24 pixels taller, because the
		// switch row is only drawn when there is somewhere to switch to.
		if addon_total > 0 {
			d3dg.draw_2d_box(
				menu_x - 1,
				menu_y,
				menu_x + 360,
				menu_y + MAINMENU_H + 1,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);
		} else {
			d3dg.draw_2d_box(
				menu_x - 1,
				menu_y,
				menu_x + 360,
				menu_y + MAINMENU_H - 24,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);
		}

		if self.world.game_info.selectmission_id >= 0 {
			self.scrollbar_official.draw(d3dg);
		} else {
			self.scrollbar_addon.draw(d3dg);
		}

		// UP is only offered while there is something above the list, and is drawn as
		// plain text rather than as something to press when there is not.
		if scrollitems > 0 {
			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + 1,
				b"<  UP  >",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				25,
				26,
			);

			let color = if hovered(
				menu_x,
				menu_y,
				menu_x + 340,
				menu_y + 30,
				mouse_x,
				mouse_y,
				scrolling,
			) {
				D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
			} else {
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0)
			};

			d3dg.draw_2d_texture_font_text(menu_x, menu_y, b"<  UP  >", color, 25, 26);
		} else {
			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + 1,
				b"<  UP  >",
				D3DGraphics::get_color_code(0.6, 0.6, 0.6, 1.0),
				25,
				26,
			);
		}

		// DOWN, the same, on the other end of the list.
		if scrollitems < totalmission - TOTAL_MENUITEMS {
			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + MAINMENU_H - 55 + 1,
				b"< DOWN >",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				25,
				26,
			);

			let color = if hovered(
				menu_x,
				menu_y + MAINMENU_H - 55,
				menu_x + 340,
				menu_y + MAINMENU_H - 55 + 30,
				mouse_x,
				mouse_y,
				scrolling,
			) {
				D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
			} else {
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0)
			};

			d3dg.draw_2d_texture_font_text(
				menu_x,
				menu_y + MAINMENU_H - 55,
				b"< DOWN >",
				color,
				25,
				26,
			);
		} else {
			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + MAINMENU_H - 55 + 1,
				b"< DOWN >",
				D3DGraphics::get_color_code(0.6, 0.6, 0.6, 1.0),
				25,
				26,
			);
		}

		// The switch row. Upstream only draws it, on the official list, when there are
		// add-ons to switch to, and the addon branch's own guard is commented out, so the
		// way back is always drawn.
		let switch_hovered = hovered(
			menu_x,
			menu_y + MAINMENU_H - 25,
			menu_x + 340,
			menu_y + MAINMENU_H - 2,
			mouse_x,
			mouse_y,
			scrolling,
		);
		let switch_color = if switch_hovered {
			D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
		} else {
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0)
		};
		if self.world.game_info.selectmission_id >= 0 {
			if addon_total > 0 {
				d3dg.draw_2d_texture_font_text(
					menu_x + 1,
					menu_y + MAINMENU_H - 25 + 1,
					b"ADD-ON MISSIONS >>",
					D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
					17,
					22,
				);
				d3dg.draw_2d_texture_font_text(
					menu_x,
					menu_y + MAINMENU_H - 25,
					b"ADD-ON MISSIONS >>",
					switch_color,
					17,
					22,
				);
			}
		} else {
			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + MAINMENU_H - 25 + 1,
				b"<< STANDARD MISSIONS",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				17,
				22,
			);
			d3dg.draw_2d_texture_font_text(
				menu_x,
				menu_y + MAINMENU_H - 25,
				b"<< STANDARD MISSIONS",
				switch_color,
				17,
				22,
			);
		}

		// The missions themselves.
		for i in 0..TOTAL_MENUITEMS {
			let name = mission_name(
				scrollitems + i,
				&self.world.game_info,
				&self.world.addon,
				&self.world.param,
			);
			let color = if hovered(
				menu_x,
				menu_y + 30 + i * 30,
				menu_x + name.len() as i32 * 20,
				menu_y + 30 + i * 30 + 26,
				mouse_x,
				mouse_y,
				scrolling,
			) {
				D3DGraphics::get_color_code(1.0, 0.6, 0.6, 1.0)
			} else {
				D3DGraphics::get_color_code(0.6, 0.6, 1.0, 1.0)
			};

			d3dg.draw_2d_texture_font_text(
				menu_x + 1,
				menu_y + 30 + 1 + i * 30,
				name,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				20,
				26,
			);
			d3dg.draw_2d_texture_font_text(
				menu_x,
				menu_y + 30 + i * 30,
				name,
				color,
				20,
				26,
			);
		}

		if ENABLE_MENUOPTIONS {
			let option_x = MAINMENU_OPTION_X;
			let option_y = mainmenu_option_y(config);
			d3dg.draw_2d_box(
				option_x,
				option_y,
				option_x + 132 + 2,
				option_y + 16 + 2,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);

			let color = if hovered(
				option_x,
				option_y,
				option_x + 132 + 2,
				option_y + 16,
				mouse_x,
				mouse_y,
				scrolling,
			) {
				D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0)
			} else {
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0)
			};

			d3dg.draw_2d_texture_font_text(
				option_x + 1,
				option_y + 1,
				b"< OPTIONS >",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				12,
				16,
			);
			d3dg.draw_2d_texture_font_text(
				option_x,
				option_y,
				b"< OPTIONS >",
				color,
				12,
				16,
			);
		}
	}

	/// The upstream `mainmenu::RenderOptions`.
	fn render_options(&mut self, d3dg: &mut D3DGraphics, config: &Config) {
		let mouse_x = self.mainmenu_mouse_x;
		let mouse_y = self.mainmenu_mouse_y;

		if self.modescreen == 1 {
			let base_x = options_p1_x(config);
			let base_y = options_p1_y(config);

			d3dg.draw_2d_box(
				base_x,
				base_y,
				base_x + OPTIONS_P1_W,
				base_y + OPTIONS_P1_H,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);

			for i in 0..OPTIONS_P1_DATAS {
				draw_link_text(
					d3dg,
					&self.options_p1_link_text_data[i],
					base_x,
					base_y,
					mouse_x,
					mouse_y,
				);
			}
		}

		if self.modescreen == 2 {
			let base_x = options_p2_x(config);
			let base_y = options_p2_y(config);

			d3dg.draw_2d_box(
				base_x,
				base_y,
				base_x + OPTIONS_P2_W,
				base_y + OPTIONS_P2_H,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);

			// The two settings, with their numbers.
			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 1,
				base_y + 1,
				b"MOUSE SENSITIVITY",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				12,
				16,
			);
			d3dg.draw_2d_texture_font_text(
				base_x + 350,
				base_y,
				b"MOUSE SENSITIVITY",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				12,
				16,
			);
			let str = format!("{:02}", config.get_mouse_sensitivity()).into_bytes();
			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 85 + 1,
				base_y + 20 + 1,
				&str,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				12,
				16,
			);
			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 85,
				base_y + 20,
				&str,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				12,
				16,
			);

			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 1,
				base_y + 50 + 1,
				b"BRIGHTNESS",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				12,
				16,
			);
			d3dg.draw_2d_texture_font_text(
				base_x + 350,
				base_y + 50,
				b"BRIGHTNESS",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				12,
				16,
			);
			let str = format!("{:02}", config.get_brightness()).into_bytes();
			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 85 + 1,
				base_y + 70 + 1,
				&str,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				12,
				16,
			);
			d3dg.draw_2d_texture_font_text(
				base_x + 350 + 85,
				base_y + 70,
				&str,
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				12,
				16,
			);

			// The action names of the key rows, one per row.
			for (i, name) in OPTIONS_KEY_NAMES.iter().enumerate() {
				let y = base_y + i as i32 * 20;
				d3dg.draw_2d_texture_font_text(
					base_x + 1,
					y + 1,
					name,
					D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
					12,
					16,
				);
				d3dg.draw_2d_texture_font_text(
					base_x,
					y,
					name,
					D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
					12,
					16,
				);
			}

			// Each key row shows the count while it is capturing and the bound key's name
			// otherwise.
			for i in 0..OPTIONS_KEY_DATAS {
				let count = (self.options_key_config_cnt as f32 / GAMEFPS) as i32 + 1;
				let text = if self.options_key_config_id == i as i32 {
					format!("[({count})]").into_bytes()
				} else {
					let name = config.get_originalkeycode_string(config.get_keycode(i as i32));
					let mut text = Vec::with_capacity(name.len() + 2);
					text.push(b'[');
					text.extend_from_slice(name);
					text.push(b']');
					text
				};
				write_cstr(&mut self.options_p2_link_text_data[14 + i].text, &text);
			}

			// The two switches that are a pair: what the box shows is the setting, so the
			// tick is written into the shared text each frame.
			if config.get_fullscreen_flag() {
				self.options_p2_link_text_data[7].text[1] = b'+';
				self.options_p2_link_text_data[8].text[1] = b' ';
			} else {
				self.options_p2_link_text_data[7].text[1] = b' ';
				self.options_p2_link_text_data[8].text[1] = b'+';
			}
			let toggles = [
				(9, config.get_sound_flag()),
				(10, config.get_blood_flag()),
				(11, config.get_invert_mouse_flag()),
				(12, config.get_frameskip_flag()),
				(13, config.get_another_gunsight_flag()),
			];
			for (index, flag) in toggles {
				self.options_p2_link_text_data[index].text[1] = if flag { b'*' } else { b' ' };
			}

			for i in 0..OPTIONS_P2_DATAS {
				draw_link_text(
					d3dg,
					&self.options_p2_link_text_data[i],
					base_x,
					base_y,
					mouse_x,
					mouse_y,
				);
			}
		}

		if self.modescreen == 3 {
			let base_x = options_p3_x(config);
			let base_y = options_p3_y(config);
			let maxline = if config.get_language() == 1 {
				OPTIONS_LICENSEMAXLINE_KOREAN
			} else {
				OPTIONS_LICENSEMAXLINE
			};

			d3dg.draw_2d_box(
				base_x,
				base_y,
				base_x + OPTIONS_P3_W,
				base_y + OPTIONS_P3_H,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.5),
			);

			d3dg.draw_2d_texture_font_text(
				base_x + 1,
				base_y + 15 + 1,
				b"SOFTWARE LICENSE",
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 1.0),
				18,
				24,
			);
			d3dg.draw_2d_texture_font_text(
				base_x,
				base_y + 15,
				b"SOFTWARE LICENSE",
				D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
				18,
				24,
			);

			d3dg.draw_2d_box(
				base_x,
				base_y + 55,
				base_x + OPTIONS_P3_W - 20,
				base_y + 55 + 18 * 12,
				D3DGraphics::get_color_code(0.0, 0.0, 0.0, 0.7),
			);

			// The visible lines, joined with line feeds for the small font.
			let scrollline = self.scrollbar_license.get_scroll_item();
			self.licenseinfobuf.clear();
			let lines = if OPTIONS_LICENSELINE > maxline as usize {
				maxline
			} else {
				OPTIONS_LICENSELINE as i32
			};
			for i in 0..lines {
				// The scrollbar has no travel here, six lines being shorter than the
				// page, so this cannot walk off the end. It is read as empty rather than
				// panicking if a future change lets it.
				let line = LICENSE_INFO
					.get((scrollline + i) as usize)
					.copied()
					.unwrap_or(b"");
				self.licenseinfobuf.extend_from_slice(line);
				self.licenseinfobuf.push(b'\n');
			}
			d3dg.draw_2d_ms_small_font_text(
				base_x + 5,
				base_y + 55,
				&self.licenseinfobuf,
				D3DGraphics::get_color_code(0.8, 0.8, 0.8, 1.0),
			);

			self.scrollbar_license.draw(d3dg);

			for i in 0..OPTIONS_P3_DATAS {
				draw_link_text(
					d3dg,
					&self.options_p3_link_text_data[i],
					base_x,
					base_y,
					mouse_x,
					mouse_y,
				);
			}
		}
	}

	/// The upstream `mainmenu::CreateOptions`: the link texts of all three pages, the
	/// licence scrollbar, and where the licence text comes from.
	fn create_options(&mut self, config: &Config) {
		self.options_fullscreen_flag_old = config.get_fullscreen_flag();
		self.options_key_config_id = -1;
		self.options_key_config_cnt = 0;

		// Page 1: two entries that change the page and the way out.
		let blue = D3DGraphics::get_color_code(0.6, 0.6, 1.0, 1.0);
		let red = D3DGraphics::get_color_code(1.0, 0.6, 0.6, 1.0);
		self.options_p1_link_text_data[0] = link_text(0, 30, 20, 26, blue, red, b"CONFIG");
		self.options_p1_link_text_data[1] =
			link_text(0, 90, 20, 26, blue, red, b"SOFTWARE LICENSE");
		self.options_p1_link_text_data[2] = link_text(
			0,
			150,
			20,
			26,
			D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0),
			D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0),
			b"< EXIT >",
		);

		// Page 2: the three buttons, then the four stepper arrows, then the seven
		// switches and finally the eighteen key rows.
		let white = D3DGraphics::get_color_code(1.0, 1.0, 1.0, 1.0);
		let cyan = D3DGraphics::get_color_code(0.0, 1.0, 1.0, 1.0);
		let names = [b"< DEFAULT >".as_slice(), b"< SAVE >", b"< CANCEL >"];
		for (i, name) in names.iter().enumerate() {
			self.options_p2_link_text_data[i] =
				link_text(350, 280 + i as i32 * 30, 12, 16, white, cyan, name);
		}

		let steps = [
			(370, 20, b"[<<]".as_slice()),
			(470, 20, b"[>>]"),
			(370, 70, b"[<<]"),
			(470, 70, b"[>>]"),
		];
		for (i, (x, y, text)) in steps.iter().enumerate() {
			self.options_p2_link_text_data[3 + i] = link_text(*x, *y, 12, 16, blue, red, text);
		}

		let switches: [(i32, &[u8]); 7] = [
			(100, b"[ ] FULL SCREEN"),
			(120, b"[ ] WINDOW"),
			(160, b"[ ] ENABLE SOUND"),
			(180, b"[ ] ENABLE BLOOD"),
			(200, b"[ ] INVERT MOUSE"),
			(220, b"[ ] FRAME SKIP"),
			(240, b"[ ] ANOTHER GUNSIGHT"),
		];
		for (i, (y, text)) in switches.iter().enumerate() {
			self.options_p2_link_text_data[7 + i] = link_text(350, *y, 12, 16, blue, red, text);
		}

		for i in 0..OPTIONS_KEY_DATAS {
			self.options_p2_link_text_data[14 + i] = link_text(170, i as i32 * 20, 12, 16, blue, red, b"[ ]");
		}

		// Page 3: the way back to page 1.
		self.options_p3_link_text_data[0] = link_text(
			(OPTIONS_P3_W - 160) / 2,
			OPTIONS_P3_H - 35,
			20,
			26,
			white,
			cyan,
			b"< EXIT >",
		);

		// The licence scrollbar. The page shows 18 lines, so this bar never has travel:
		// `ScrollbarObject::create` pulls the shown count down to the total and the knob
		// covers the whole bar.
		let maxline = if config.get_language() == 1 {
			OPTIONS_LICENSEMAXLINE_KOREAN
		} else {
			OPTIONS_LICENSEMAXLINE
		};
		self.scrollbar_license.create(
			options_p3_x(config) + OPTIONS_P3_W - 20,
			options_p3_y(config) + 55,
			20,
			18 * 12,
			OPTIONS_LICENSELINE as i32,
			maxline,
			0,
		);
		set_scrollbar_colors(&mut self.scrollbar_license);

		// The licence text itself is the constant table, and the `GRAPHIC_ENGINE` choice
		// is already made in it.
	}

	/// [`D3Dscene::sound`] for this screen. The menu is watched from a camera moving over
	/// the demo world, so the sound is positioned there rather than at the player.
	pub fn sound(&mut self, game_sound: &mut SoundManager<'_>) {
		self.base.sound(game_sound);
	}

	/// [`D3Dscene::render_main`] for this screen.
	pub fn render_main(&mut self, d3dg: &mut D3DGraphics, config: &Config) -> bool {
		D3Dscene::render_main(d3dg, config, self, Self::render_3d, Self::render_2d)
	}

	/// The upstream `mainmenu::Destroy`.
	pub fn destroy(
		&mut self,
		state: &mut StateMachine,
		log: &mut DebugLog,
		d3dg: &mut D3DGraphics,
	) {
		let resource = &mut self.world.resource;

		d3dg.cleanup_mapdata();
		self.world.objects.cleanup(d3dg, resource);
		resource.cleanup_sky_model_texture(d3dg);
		d3dg.cleanup_texture(self.gametitle);

		state.next_state(log);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::debug::DebugLog;
	use crate::resource::ResourceManager;
	use crate::statemachine::State;
	use xops_platform::gfx::Gfx;
	use xops_platform::input::vk;
	use xops_platform::wgpu;
	use xops_platform::winit::dpi::PhysicalPosition;
	use xops_platform::winit::event::{ElementState, MouseButton, WindowEvent};

	/// A 640x480 renderer, which is the size every number below is worked out at.
	fn headless() -> D3DGraphics {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});
		let gfx = Gfx::new(&instance, None, 640, 480, None).expect("a headless renderer");
		D3DGraphics::new(gfx, false, 0)
	}

	/// The shipped settings at the default screen size, which is what the menu's layout
	/// constants read.
	fn config() -> Config {
		let mut config = Config::new();
		config.set_default_config();
		config.set_default_ext_config();
		config
	}

	/// A cursor move, which is the only way the platform's mouse position is set.
	fn move_cursor(input: &mut Input, x: f64, y: f64) {
		input.handle(&WindowEvent::CursorMoved {
			// Never routed anywhere, so the device it names is never read.
			device_id: unsafe { std::mem::zeroed() },
			position: PhysicalPosition::new(x, y),
		});
	}

	/// A left button press or release, which the menu reads as an edge.
	fn mouse_button(input: &mut Input, state: ElementState) {
		input.handle(&WindowEvent::MouseInput {
			device_id: unsafe { std::mem::zeroed() },
			state,
			button: MouseButton::Left,
		});
	}

	/// Everything one frame of the menu's input needs that is not the menu itself.
	///
	/// The sound manager is built with nothing attached, which is how every test in this
	/// crate builds one: the phases below only queue into it.
	struct Frame<'a> {
		input: Input,
		input_ctrl: InputControl,
		state: StateMachine,
		log: DebugLog,
		config: Config,
		sound: SoundManager<'a>,
	}

	impl<'a> Frame<'a> {
		fn new() -> Frame<'a> {
			Frame {
				input: Input::default(),
				input_ctrl: InputControl::new(),
				state: StateMachine::new(),
				log: DebugLog::new(),
				config: config(),
				sound: SoundManager::new(None, None, None),
			}
		}

		/// One frame with the cursor at `x`, `y`, and the left button either untouched or
		/// driven to `button`.
		fn tick(&mut self, menu: &mut MainMenu<'_>, x: f64, y: f64, button: Option<ElementState>) {
			move_cursor(&mut self.input, x, y);
			if let Some(state) = button {
				mouse_button(&mut self.input, state);
			}
			menu.input(
				&mut self.input_ctrl,
				&mut self.input,
				&mut self.state,
				&mut self.log,
				&mut self.config,
			);
		}

		/// A whole click: the frame the button goes down and the frame it comes up, which
		/// is the edge the menu acts on.
		fn click(&mut self, menu: &mut MainMenu<'_>, x: f64, y: f64) {
			self.tick(menu, x, y, Some(ElementState::Pressed));
			self.tick(menu, x, y, Some(ElementState::Released));
		}

		/// Steps the machine to where the host has it when this screen is running: the
		/// opening's three states, then the menu's create and run.
		fn enter_menu(&mut self) {
			for _ in 0..4 {
				self.state.next_state(&mut self.log);
			}
		}
	}

	/// A directory holding the two addon missions the tests below read.
	///
	/// The `.mif` files are the smallest ones the reader accepts, and what matters here is
	/// only that they are found and sorted, which is the same fixture `datafile.rs` uses.
	fn addon_dir(name: &str) -> std::path::PathBuf {
		let dir = std::env::temp_dir().join(format!("xops_gamemenu_{}_{}", std::process::id(), name));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).expect("create temp dir");
		std::fs::write(dir.join("b.mif"), b"BETA\nBeta\nb.bd1\np.pd1\n0\n0\n!\n!\n!\n").unwrap();
		std::fs::write(dir.join("a.MIF"), b"alpha\nAlpha\na.bd1\np.pd1\n0\n0\n!\n!\n!\n").unwrap();
		dir
	}

	/// The mission list, from the menu's own view of the world.
	fn name_of(menu: &MainMenu<'_>, index: i32) -> Vec<u8> {
		mission_name(
			index,
			&menu.world.game_info,
			&menu.world.addon,
			&menu.world.param,
		)
		.to_vec()
	}

	// --- the mission list ---------------------------------------------------

	/// Both lists load, and a row is named from the list the selection is on. The official
	/// list is the parameter table and the addon list is the directory, and an index past
	/// either reads empty, which is what the upstream leaves in its name buffer.
	#[test]
	fn the_official_missions_and_the_addons_are_both_read() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);

		// The official list: 58 missions, and the first and last are the table's.
		assert_eq!(TOTAL_OFFICIALMISSION, 58);
		assert_eq!(menu.world.game_info.selectmission_id, 0);
		assert_eq!(name_of(&menu, 0), b"TRAINING YARD");
		assert_eq!(name_of(&menu, 57), b"SCHOOL_DE");
		assert!(name_of(&menu, 58).is_empty());

		// The addon list, from a directory of two missions. The sort is on the lowercased
		// name, so "alpha" comes before "BETA" even though the file name does not.
		let dir = addon_dir("list");
		let dir_bytes = dir.to_string_lossy().into_owned().into_bytes();
		assert_eq!(menu.world.addon.load_file_data(&dir_bytes), 2);
		assert_eq!(menu.world.addon.get_total_datas(), 2);

		menu.world.game_info.selectmission_id = -1;
		assert_eq!(name_of(&menu, 0), b"alpha");
		assert_eq!(name_of(&menu, 1), b"BETA");
		// Past the end of the list, and this is the row the menu still draws.
		assert!(name_of(&menu, 2).is_empty());

		let _ = std::fs::remove_dir_all(&dir);
	}

	/// The official bar has 58 items over 8 rows, and its knob sits at the top of the bar
	/// on the first page and at the bottom on the last one. The positions are read back
	/// through the knob's own hit test, which is the only thing a caller can see: the
	/// knob is grabbable exactly where it is drawn.
	#[test]
	fn the_mission_scrollbar_knob_sits_at_the_top_and_at_the_bottom() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let frame = Frame::new();

		menu.create_scrollbars(&frame.config);

		// The bar is 19 wide and 300 tall at the menu's right edge: x 621 to 640, y 141
		// to 441. 300/58*8 is 41.379 pixels of knob and (300-41.379)/50 is 5.172 pixels of
		// travel per item, so on the first page the knob covers y 141 to 181 and on the
		// last it covers y 399 to 439.
		let bar_x = 630;
		assert_eq!(mainmenu_x(&frame.config), 280);
		assert_eq!(mainmenu_y(&frame.config), 140);
		assert_eq!(menu.scrollbar_official.get_scroll_item(), 0);

		// At the top: the point just under the bar's top edge is on the knob, and the
		// point near the bottom is not.
		menu.scrollbar_official.input(bar_x, 151, true, 0);
		assert!(menu.scrollbar_official.check_scrolling());
		menu.scrollbar_official.input(bar_x, 151, false, 0);
		assert!(!menu.scrollbar_official.check_scrolling());
		menu.scrollbar_official.input(bar_x, 420, true, 0);
		assert!(!menu.scrollbar_official.check_scrolling());

		// The last page, which is 58 items less the 8 on screen.
		for _ in 0..100 {
			menu.scrollbar_official.scroll_down();
		}
		assert_eq!(menu.scrollbar_official.get_scroll_item(), 50);
		// The knob follows the item when the drag is not on, which is where it is read
		// back from.
		menu.scrollbar_official.input(0, 0, false, 0);

		// At the bottom now: the point that was off the knob is on it, and the one that
		// was on it is off.
		menu.scrollbar_official.input(bar_x, 420, true, 0);
		assert!(menu.scrollbar_official.check_scrolling());
		// And it still reports the last page, so the grab landed where the knob is drawn.
		assert_eq!(menu.scrollbar_official.get_scroll_item(), 50);
		menu.scrollbar_official.input(bar_x, 420, false, 0);
		menu.scrollbar_official.input(bar_x, 151, true, 0);
		assert!(!menu.scrollbar_official.check_scrolling());
	}

	/// A list shorter than the screen has no travel: the knob covers the whole bar and
	/// dragging it anywhere leaves the list on its first item.
	#[test]
	fn an_addon_list_shorter_than_the_screen_does_not_scroll() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let frame = Frame::new();

		let dir = addon_dir("short");
		let dir_bytes = dir.to_string_lossy().into_owned().into_bytes();
		menu.world.addon.load_file_data(&dir_bytes);
		menu.create_scrollbars(&frame.config);

		// The knob is the bar, so the bottom of it is grabbable with two addons listed.
		menu.scrollbar_addon.input(630, 420, true, 0);
		assert!(menu.scrollbar_addon.check_scrolling());
		// And dragging it down by more than the bar does not move the list, because the
		// last page is the first page.
		menu.scrollbar_addon.input(630, 420, true, 1000);
		assert_eq!(menu.scrollbar_addon.get_scroll_item(), 0);

		let _ = std::fs::remove_dir_all(&dir);
	}

	/// A click on a mission row takes that mission and clears the addon path, which is
	/// what tells the mission loader this is an official mission. The click also carries
	/// the state machine on, which is how the menu is left.
	#[test]
	fn a_click_on_a_mission_row_takes_that_mission() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();

		frame.enter_menu();
		menu.create_scrollbars(&frame.config);
		// A path from an earlier addon pick, which an official mission has to clear.
		menu.world.game_info.mifpath[0] = b'x';

		// Row 1 of the official list. Rows start at `MAINMENU_Y + 30` and are 30 apart,
		// so row 1 spans y 200 to 226, and the name is 15 characters of 20 pixels.
		let row_y = (mainmenu_y(&frame.config) + 60 + 10) as f64;
		frame.click(&mut menu, 300.0, row_y);

		assert_eq!(menu.world.game_info.selectmission_id, 1);
		assert_eq!(menu.world.game_info.mifpath[0], 0);
		assert_eq!(frame.state.get_state(), State::STATE_DESTROY_MENU);
	}

	/// The DOWN and UP rows walk the list and stop at its ends: the last page is 50 and
	/// the first is 0, however many times they are pressed.
	#[test]
	fn the_up_and_down_rows_stop_at_the_ends_of_the_list() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();

		menu.create_scrollbars(&frame.config);
		let menu_y = mainmenu_y(&frame.config);
		// The DOWN row is 55 above the bottom edge of the menu and 30 tall.
		let down_y = (menu_y + MAINMENU_H - 55 + 20) as f64;
		let up_y = (menu_y + 10) as f64;

		for _ in 0..60 {
			frame.click(&mut menu, 300.0, down_y);
		}
		assert_eq!(menu.scrollbar_official.get_scroll_item(), 50);

		for _ in 0..60 {
			frame.click(&mut menu, 300.0, up_y);
		}
		assert_eq!(menu.scrollbar_official.get_scroll_item(), 0);
	}

	/// The switch row only offers the addon list when there is one, and coming back is
	/// always offered. Nothing here is a wrap: the row above the list does not jump to
	/// the bottom.
	#[test]
	fn the_switch_row_moves_between_the_two_lists() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();

		menu.create_scrollbars(&frame.config);
		let switch_y = (mainmenu_y(&frame.config) + MAINMENU_H - 12) as f64;

		// No addons: the row is drawn as the way back to the standard missions and the
		// press does nothing, because the official list is already showing.
		frame.click(&mut menu, 300.0, switch_y);
		assert_eq!(menu.world.game_info.selectmission_id, 0);

		// With addons it switches, and switches back.
		let dir = addon_dir("switch");
		let dir_bytes = dir.to_string_lossy().into_owned().into_bytes();
		menu.world.addon.load_file_data(&dir_bytes);
		menu.create_scrollbars(&frame.config);

		frame.click(&mut menu, 300.0, switch_y);
		assert_eq!(menu.world.game_info.selectmission_id, -1);
		frame.click(&mut menu, 300.0, switch_y);
		assert_eq!(menu.world.game_info.selectmission_id, 0);

		let _ = std::fs::remove_dir_all(&dir);
	}

	/// An addon row writes the addon's directory and file name into the mission path,
	/// which is what the loader opens later.
	#[test]
	fn an_addon_row_writes_the_mission_path() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();

		frame.enter_menu();
		let dir = addon_dir("path");
		let dir_bytes = dir.to_string_lossy().into_owned().into_bytes();
		menu.world.addon.load_file_data(&dir_bytes);
		menu.create_scrollbars(&frame.config);
		menu.world.game_info.selectmission_id = -1;

		// Row 0: "alpha" is five characters, so the row is 100 pixels wide from
		// MAINMENU_X, and it spans y 170 to 196.
		frame.click(&mut menu, 300.0, (mainmenu_y(&frame.config) + 40) as f64);

		assert_eq!(menu.world.game_info.selectmission_id, -1);
		assert_eq!(
			cstr(&menu.world.game_info.mifpath),
			b"addon\\a.MIF".as_slice()
		);
		assert_eq!(frame.state.get_state(), State::STATE_DESTROY_MENU);

		let _ = std::fs::remove_dir_all(&dir);
	}

	// --- the link rectangles and the options pages --------------------------

	/// A link text is as wide as its characters and as tall as its height, and the bounds
	/// are open, so the exact edges are misses.
	#[test]
	fn a_link_text_is_hit_across_its_characters() {
		// CONFIG: six characters of 20 pixels, 26 tall, drawn at the page's own corner.
		let link = link_text(0, 30, 20, 26, 0, 0, b"CONFIG");

		assert!(link_text_hit(&link, 280, 261, 300, 300));
		assert!(link_text_hit(&link, 280, 261, 399, 316));
		assert!(!link_text_hit(&link, 280, 261, 280, 300));
		assert!(!link_text_hit(&link, 280, 261, 400, 300));
		assert!(!link_text_hit(&link, 280, 261, 300, 291));
		assert!(!link_text_hit(&link, 280, 261, 300, 317));

		// A disabled entry is still a rectangle, which is what `InputOptions` skips.
		let mut disabled = link;
		disabled.enable = false;
		assert!(link_text_hit(&disabled, 280, 261, 300, 300));
	}

	/// A mission row is hit over the name's own width, which is why a short name is a
	/// short target and an empty one is no target at all.
	#[test]
	fn a_mission_row_is_hit_by_its_name_width() {
		let config = config();
		assert_eq!(mainmenu_x(&config), 280);
		assert_eq!(mainmenu_y(&config), 140);

		// "TRAINING YARD" is 13 characters, so 260 pixels from x 280, and the row spans
		// y 170 to 196.
		assert!(mission_row_hit(b"TRAINING YARD", 0, 300, 180, &config));
		assert!(mission_row_hit(b"TRAINING YARD", 0, 539, 195, &config));
		assert!(!mission_row_hit(b"TRAINING YARD", 0, 540, 180, &config));
		assert!(!mission_row_hit(b"TRAINING YARD", 0, 300, 196, &config));

		// Row 7 is the last one the menu shows, 210 pixels further down.
		assert!(mission_row_hit(b"TRAINING YARD", 7, 300, 390, &config));

		// The empty rows an addon list leaves at the end are not clickable.
		assert!(!mission_row_hit(b"", 0, 300, 180, &config));
	}

	/// The three pages are built with the upstream rectangles and texts.
	#[test]
	fn the_options_pages_are_built_with_the_upstream_entries() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let config = config();

		menu.create_options(&config);

		assert_eq!(cstr(&menu.options_p1_link_text_data[0].text), b"CONFIG");
		assert_eq!(
			cstr(&menu.options_p1_link_text_data[1].text),
			b"SOFTWARE LICENSE"
		);
		assert_eq!(cstr(&menu.options_p1_link_text_data[2].text), b"< EXIT >");
		assert_eq!(menu.options_p1_link_text_data[0].pos_y, 30);
		assert_eq!(menu.options_p1_link_text_data[0].text_w, 20);
		assert_eq!(menu.options_p1_link_text_data[0].text_h, 26);

		assert_eq!(cstr(&menu.options_p2_link_text_data[0].text), b"< DEFAULT >");
		assert_eq!(cstr(&menu.options_p2_link_text_data[3].text), b"[<<]");
		assert_eq!(
			cstr(&menu.options_p2_link_text_data[13].text),
			b"[ ] ANOTHER GUNSIGHT"
		);
		// The eighteen key rows are the tail of the page.
		assert_eq!(OPTIONS_P2_DATAS, 32);
		assert_eq!(cstr(&menu.options_p2_link_text_data[14].text), b"[ ]");
		assert_eq!(cstr(&menu.options_p2_link_text_data[31].text), b"[ ]");

		assert_eq!(cstr(&menu.options_p3_link_text_data[0].text), b"< EXIT >");
		// The licence is six lines, the OpenGL branch, and they all fit on the page.
		assert_eq!(LICENSE_INFO.len(), OPTIONS_LICENSELINE);
		assert_eq!(OPTIONS_LICENSELINE, 6);
		assert_eq!(menu.scrollbar_license.get_scroll_item(), 0);
	}

	/// The licence page joins the six lines it shows, one line feed each, and its
	/// scrollbar has no travel because six lines fit in the eighteen the page holds.
	#[test]
	fn the_licence_page_shows_the_six_lines_of_this_build() {
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let config = config();

		menu.create_options(&config);
		menu.modescreen = 3;
		d3dg.start_render();
		menu.render_options(&mut d3dg, &config);
		assert!(!d3dg.end_render(), "frame encoding failed");

		assert_eq!(
			menu.licenseinfobuf,
			b"\nOpenXOPS\nCopyright (c) 2014-2023, OpenXOPS Project / [-_-;](mikan) All rights reserved.\n\n -- See the document.\n\n".to_vec()
		);

		// The knob covers the bar, because six lines fit in the eighteen the page holds,
		// and dragging it off the end cannot scroll the page.
		menu.scrollbar_license.input(610, 350, true, 0);
		assert!(menu.scrollbar_license.check_scrolling());
		menu.scrollbar_license.input(610, 350, true, 1000);
		assert_eq!(menu.scrollbar_license.get_scroll_item(), 0);
	}

	/// The OPTIONS box opens page 1, the page's own entries move between the pages, and
	/// a press outside every entry does nothing.
	#[test]
	fn the_options_pages_are_reached_by_clicking_their_entries() {
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();

		menu.create_options(&frame.config);

		// The OPTIONS box. The clickable area is 120 wide where the box is 134, so x 60
		// is inside it and the far right of the box is not.
		let option_y = mainmenu_option_y(&frame.config);
		assert_eq!(MAINMENU_OPTION_X, 10);
		assert_eq!(option_y, 449);
		frame.click(&mut menu, 60.0, 457.0);
		assert_eq!(menu.modescreen, 1);

		// A press that is on the page but on no entry leaves it where it is.
		frame.click(&mut menu, 60.0, 200.0);
		assert_eq!(menu.modescreen, 1);
		// And the inert strip at the right hand end of the box does too.
		frame.click(&mut menu, 140.0, 457.0);
		assert_eq!(menu.modescreen, 1);

		// CONFIG, the first entry of page 1.
		frame.click(&mut menu, 300.0, 300.0);
		assert_eq!(menu.modescreen, 2);

		// Back out of page 1 is not possible from page 2 without a press on Cancel, so
		// reload the page 1 entries and go to the licence instead.
		menu.modescreen = 1;
		frame.click(&mut menu, 300.0, 360.0);
		assert_eq!(menu.modescreen, 3);
		// The licence starts at the top whenever it is entered.
		assert_eq!(menu.scrollbar_license.get_scroll_item(), 0);

		// The way out of the licence page, which is drawn 220 across and 285 down the
		// page at x 20, y 132, so it spans x 240 to 400 and y 417 to 443.
		frame.click(&mut menu, 300.0, 430.0);
		assert_eq!(menu.modescreen, 1);
	}

	/// A press on a key row starts the capture for the frame count the settings page
	/// holds it for, and the capture ends when the count runs out.
	#[test]
	fn the_key_capture_runs_for_five_seconds_and_then_ends() {
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut menu = MainMenu::new(world);
		let mut frame = Frame::new();
		let mut window = WindowControl::new();

		menu.create_options(&frame.config);
		menu.modescreen = 2;
		// The eighth key row, which is `Options_p2LinkTextData[14 + 7]`.
		menu.options_p2_link_text_id = 21;

		menu.process_options(
			&mut d3dg,
			&mut frame.config,
			&mut window,
			&mut frame.input_ctrl,
			&mut frame.input,
			&mut frame.log,
			&mut frame.sound,
		);

		// Five seconds of frames, and the frame that started it has already counted.
		assert_eq!((GAMEFPS * 5.0) as i32, 166);
		assert_eq!(menu.options_key_config_id, 7);
		assert_eq!(menu.options_key_config_cnt, 165);

		// Nothing is picked, so the count runs out and the capture is dropped.
		menu.options_p2_link_text_id = -1;
		let mut frames = 0;
		while menu.options_key_config_id != -1 && frames < 400 {
			menu.process_options(
				&mut d3dg,
				&mut frame.config,
				&mut window,
				&mut frame.input_ctrl,
				&mut frame.input,
				&mut frame.log,
				&mut frame.sound,
			);
			frames += 1;
		}

		// The count runs out on the 166th frame after the one that started it, and the
		// row is released on the frame after that, because the countdown is only stepped
		// while the count is above zero.
		assert_eq!(frames, 166);
		assert_eq!(menu.options_key_config_id, -1);
		assert_eq!(menu.options_key_config_cnt, 0);
	}

	/// A key the player releases becomes the binding, in the original keycode the config
	/// stores, and the capture ends on it.
	///
	/// This is `capture_key` on its own because the platform layer has no way to press a
	/// key: `Input` is filled from window events, and winit's `KeyEvent` cannot be built
	/// outside winit. What the capture is handed is `CheckInputAll`'s answer, so that is
	/// what this drives.
	#[test]
	fn a_released_key_becomes_the_binding() {
		let mut config = config();
		let mut id = 7;
		let mut cnt = (GAMEFPS * 5.0) as i32;

		// A key that is only held is not an answer: `CheckInputAll` reports -1 until one
		// comes up.
		assert!(!capture_key(-1, &mut id, &mut cnt, &mut config));
		assert_eq!(id, 7);
		assert_eq!(cnt, 166);
		// The shipped binding of `KEY_MOVERIGHT`, which is the row being captured.
		assert_eq!(config.get_keycode(7), 35);

		// The first rebindable key released this frame. Code 0 is the up arrow, and it is
		// the first code `CheckInputAll` can return, so it is the key a capture of a
		// freshly started row ends on when the player presses nothing else.
		assert!(capture_key(0x00, &mut id, &mut cnt, &mut config));
		assert_eq!(config.get_keycode(7), 0x00);
		assert_eq!(
			originalkeycode_to_dinputdef(config.get_keycode(7)),
			vk::UP as i32
		);
		assert_eq!(id, -1);
		assert_eq!(cnt, 0);

		// And it stays ended: the row is no longer capturing, so the next key does nothing
		// even before the count would have run out.
		assert!(!capture_key(0x01, &mut id, &mut cnt, &mut config));
		assert_eq!(config.get_keycode(7), 0x00);
	}

	/// A capture that never started, and one whose count has run out, take nothing.
	#[test]
	fn a_key_is_only_taken_while_a_row_is_capturing() {
		let mut config = config();

		let mut id = -1;
		let mut cnt = 0;
		assert!(!capture_key(0x00, &mut id, &mut cnt, &mut config));
		assert_eq!(config.get_keycode(7), 35);

		// The row is capturing but the five seconds are up, which is the state `Process`
		// leaves behind on the frame the count reaches zero.
		let mut id = 7;
		let mut cnt = 0;
		assert!(!capture_key(0x00, &mut id, &mut cnt, &mut config));
		assert_eq!(config.get_keycode(7), 35);
		assert_eq!(id, 7);
	}

	// --- the opening --------------------------------------------------------

	/// The opening cannot load its map without the game data, and reports the upstream's
	/// two bit code: 2 for the point data plus 1 for the block data.
	#[test]
	fn the_opening_reports_the_map_it_could_not_load() {
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut opening = Opening::new(world);
		let mut state = StateMachine::new();
		let mut log = DebugLog::new();
		let mut sound = SoundManager::new(None, None, None);
		let config = config();
		let input_ctrl = InputControl::new();
		let window = WindowControl::new();

		// The data folder is not part of this repository, so both reads fail. A machine
		// that does have the data would reach the camera below instead.
		assert_eq!(
			opening.create(
				&mut state,
				&window,
				&config,
				&input_ctrl,
				&mut log,
				&mut d3dg,
				&mut sound,
			),
			3
		);
		// A failed load leaves the frame count where it was, because the three things
		// `scene::Create` does are behind the load.
		assert_eq!(opening.base.scene.framecnt, 0);
	}

	/// The camera is still for four seconds, eased over the fifth, and then drifts at a
	/// fixed speed and a fixed angle for the rest of the screen.
	#[test]
	fn the_opening_camera_holds_still_then_drifts() {
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut opening = Opening::new(world);
		let mut state = StateMachine::new();
		let mut log = DebugLog::new();
		let mut sound = SoundManager::new(None, None, None);
		let config = config();

		// Where `Create` puts the camera and the count it starts from, which is the state
		// the screen is in once the map has loaded.
		opening.base.camera_x = -5.0;
		opening.base.camera_y = 58.0;
		opening.base.camera_z = 29.0;
		opening.base.camera_rx = degree_to_radian(206.0);
		opening.base.camera_ry = degree_to_radian(12.0);

		// The four thresholds, computed the way the screen computes them: a float multiply
		// truncated to a frame count.
		assert_eq!((2.6 * GAMEFPS) as u32, 86);
		assert_eq!((3.6 * GAMEFPS) as u32, 119);
		assert_eq!((4.0 * GAMEFPS) as u32, 133);
		assert_eq!((5.0 * GAMEFPS) as u32, 166);

		// The rotation holds still for 2.6 seconds, so at frame 86 nothing has moved.
		for _ in 0..86 {
			opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		}
		assert_eq!(opening.add_camera_rx, 0.0);
		assert_eq!(opening.add_camera_ry, 0.0);
		assert_eq!(opening.add_camera_z, 0.0);
		assert_eq!(opening.base.camera_rx, degree_to_radian(206.0));

		// Then it eases for one second and holds: the ramp runs over frames 86 to 118, and
		// frame 119 is the first one handed the exact angle, while the drift is still
		// waiting on its own threshold at 133.
		for _ in 0..34 {
			opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		}
		assert_eq!(opening.add_camera_rx, degree_to_radian(0.9));
		assert_eq!(opening.add_camera_ry, degree_to_radian(-0.6));
		assert_eq!(opening.add_camera_z, 0.0);
		assert!(opening.base.camera_rx > degree_to_radian(206.0));

		// The drift eases over the fifth second, a fifth of the way each frame, starting
		// with frame 133.
		for _ in 0..13 {
			opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		}
		assert_eq!(opening.add_camera_z, 0.0);
		opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		assert!((opening.add_camera_z - 0.016).abs() < 1e-6);
		assert!((opening.add_camera_y + 0.01).abs() < 1e-6);
		assert!(opening.base.camera_z > 29.0);

		// 5.0 * 33.333 is 166.66, so from frame 167 the drift is the fixed step and the
		// turn is damping to a stop instead of holding.
		for _ in 0..80 {
			opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		}
		assert_eq!(opening.add_camera_z, 0.08);
		assert_eq!(opening.add_camera_y, -0.05);
		assert!(opening.add_camera_rx < degree_to_radian(0.9));
		assert!(opening.add_camera_rx > 0.0);
		assert!(opening.add_camera_ry > degree_to_radian(-0.6));
		assert!(opening.add_camera_ry < 0.0);
	}

	/// The screen ends on the frame the upstream counts to: `16*((int)GAMEFPS)`, which
	/// truncates the frame rate to 33 first and so is 528 rather than 533.
	#[test]
	fn the_opening_ends_after_sixteen_seconds() {
		let mut d3dg = headless();
		let mut resource = ResourceManager::new();
		let mut world = World::new(&mut resource);
		let mut opening = Opening::new(world);
		let mut state = StateMachine::new();
		let mut log = DebugLog::new();
		let mut sound = SoundManager::new(None, None, None);
		let config = config();

		assert_eq!(16 * (GAMEFPS as u32), 528);
		assert_eq!((16.0 * GAMEFPS) as u32, 533);

		// The frame before the end passes without a click, the end frame itself pushes
		// one, which is what carries the state machine to the menu.
		opening.base.scene.framecnt = 527;
		opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		assert_eq!(state.get_state(), State::STATE_CREATE_OPENING);
		assert_eq!(opening.base.scene.framecnt, 528);

		opening.process(&mut state, &mut log, &mut d3dg, &config, &mut sound);
		assert_eq!(state.get_state(), State::STATE_NOW_OPENING);
		assert_eq!(opening.base.scene.framecnt, 529);
	}
}
