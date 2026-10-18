//! The GUI widgets, from `gui-object.cpp` and `gui-object.h`.
//!
//! Two classes with no base and no inheritance, so each is a plain struct. `gamemain` embeds
//! them by value, which is why the constructors and the phase methods are the shape they are.
//!
//! What the classes carry between frames:
//!
//! * `ScrollbarObject` computes the knob's size and scale once, in `create`, and keeps them
//!   because every frame's hit test needs them. It also keeps `knob_y` and `mouse_mode`,
//!   which are the drag: while the drag is on, `knob_y` is where the knob is and `input`
//!   reads the scrolled item back out of it, and while it is off, `knob_y` is recomputed
//!   from the scrolled item. A `totalitems` that changes needs another `create`, because the
//!   scale would otherwise be stale.
//! * `TextBoxObject` carries only its bytes, its limit and its mode. Nothing is cached and
//!   nothing is drawn here.

use crate::d3dgraphics::D3DGraphics;
use crate::input::{originalkeycode_to_dinputdef, InputControl};
use crate::window::round;

/// `MAX_INPUTBOXLEN`: the longest string the input box can hold.
pub const MAX_INPUTBOXLEN: usize = 127;

/// The scrollbar, the upstream `ScrollbarObject`. Vertical only.
///
/// The colours are upstream `int` and hold the result of `D3DGraphics::get_color_code`, which
/// is a `u32`, so they are stored as one: a colour is never arithmetic, only passed on, and
/// the bits are what matter.
pub struct ScrollbarObject {
	pos_x: i32,
	pos_y: i32,
	width: i32,
	height: i32,
	/// Total number of items the bar covers.
	totalitems: i32,
	/// Number of items one screen shows, upstream's misspelled `showtems`.
	showtems: i32,
	/// Number of items scrolled past, which is the item drawn at the top.
	scrollitem: i32,
	/// Height of the knob in pixels, computed by [`ScrollbarObject::create`].
	knob_height: f32,
	/// How many pixels of knob travel one item is worth, computed by
	/// [`ScrollbarObject::create`]. It is the layout: a `create` recomputes it and nothing
	/// else does.
	knob_scale: f32,
	/// Where the knob is, in pixels from the top of the bar.
	knob_y: i32,
	/// 0: the cursor is away from the bar, 1: it is over the knob, 2: it is dragging it.
	mouse_mode: i32,
	color_body: u32,
	color1_default: u32,
	color2_default: u32,
	color1_cursor: u32,
	color2_cursor: u32,
	color1_select: u32,
	color2_select: u32,
}

impl ScrollbarObject {
	/// The upstream constructor, which is an empty bar of no size.
	pub fn new() -> ScrollbarObject {
		ScrollbarObject {
			pos_x: 0,
			pos_y: 0,
			width: 0,
			height: 0,
			totalitems: 0,
			showtems: 0,
			scrollitem: 0,
			knob_height: 0.0,
			knob_scale: 0.0,
			knob_y: 0,
			mouse_mode: 0,
			color_body: 0x00000000,
			color1_default: 0x00000000,
			color2_default: 0x00000000,
			color1_cursor: 0x00000000,
			color2_cursor: 0x00000000,
			color1_select: 0x00000000,
			color2_select: 0x00000000,
		}
	}

	/// The upstream `ScrollbarObject::Create`, which is also what computes the layout.
	///
	/// The upstream asks for a width and a height of at least 8 and does not enforce it.
	pub fn create(
		&mut self,
		x: i32,
		y: i32,
		w: i32,
		h: i32,
		in_totalitems: i32,
		in_showtems: i32,
		in_scrollitem: i32,
	) {
		self.pos_x = x;
		self.pos_y = y;
		self.width = w;
		self.height = h;
		self.totalitems = in_totalitems;
		self.showtems = in_showtems;
		self.scrollitem = in_scrollitem;

		if self.totalitems < self.showtems {
			// Everything fits, so the knob covers the whole bar and one item is worth no
			// travel.
			self.showtems = self.totalitems;
			self.knob_height = h as f32;
			self.knob_scale = 1.0;
		} else {
			self.knob_height = h as f32 / self.totalitems as f32 * self.showtems as f32;
			// A bar that shows exactly its total has no travel to divide over, so this is
			// 0.0/0.0 and the scale is NaN. Nothing then reaches the knob: it is not drawn
			// because the totals are equal, and the hit test below compares against it
			// rather than through it. The upstream is the same expression, and its `(int)`
			// cast of a NaN is undefined rather than zero, which is the one place this port
			// is allowed to differ.
			self.knob_scale =
				(h as f32 - self.knob_height) / (self.totalitems - self.showtems) as f32;
		}

		self.knob_y = round(self.knob_scale * self.scrollitem as f32) as i32;
		self.mouse_mode = 0;
	}

	/// The upstream `ScrollbarObject::SetColor`. Callers pass
	/// [`D3DGraphics::get_color_code`] results.
	pub fn set_color(
		&mut self,
		body: u32,
		default1: u32,
		default2: u32,
		cursor1: u32,
		cursor2: u32,
		select1: u32,
		select2: u32,
	) {
		self.color_body = body;
		self.color1_default = default1;
		self.color2_default = default2;
		self.color1_cursor = cursor1;
		self.color2_cursor = cursor2;
		self.color1_select = select1;
		self.color2_select = select2;
	}

	/// The upstream `ScrollbarObject::SetScrollItem`, which refuses an item outside the
	/// list rather than clamping it. Note the bound: it is the total, not the last page, so
	/// an item past the last screen of the list is accepted.
	pub fn set_scroll_item(&mut self, id: i32) {
		if id < 0 || self.totalitems <= id {
			return;
		}

		self.scrollitem = id;
	}

	/// The upstream `ScrollbarObject::GetScrollItem`.
	pub fn get_scroll_item(&self) -> i32 {
		self.scrollitem
	}

	/// The upstream `ScrollbarObject::ScrollUP`, one item, stopping at the first.
	pub fn scroll_up(&mut self) {
		if self.scrollitem > 0 {
			self.scrollitem -= 1;
		}
	}

	/// The upstream `ScrollbarObject::ScrollDOWN`, one item, stopping at the last page. The
	/// last page is `totalitems - showtems`, so the list can be scrolled to its end and no
	/// further.
	pub fn scroll_down(&mut self) {
		if self.scrollitem < self.totalitems - self.showtems {
			self.scrollitem += 1;
		}
	}

	/// The upstream `ScrollbarObject::CheckScrolling`: whether the knob is being dragged.
	/// A caller uses it to leave the list it is scrolling alone for the frame.
	pub fn check_scrolling(&self) -> bool {
		self.mouse_mode == 2
	}

	/// The upstream `ScrollbarObject::Input`.
	///
	/// `mouse_x` and `mouse_y` are the cursor, `mouse_click` whether the button is held, and
	/// `scroll_y` the movement to apply to the knob this frame.
	pub fn input(&mut self, mouse_x: i32, mouse_y: i32, mouse_click: bool, scroll_y: i32) {
		// A drag that is no longer held ends, and a drag that is held is not re-tested
		// against the knob's rectangle, which is what lets the knob follow the cursor out of
		// the bar.
		if self.mouse_mode == 2 && !mouse_click {
			self.mouse_mode = 0;
		}
		if self.mouse_mode != 2 {
			// The upstream truncates these two bounds where it rounds `knob_y` below.
			let knob_top = self.pos_y + (self.knob_scale * self.scrollitem as f32) as i32;
			let knob_bottom =
				self.pos_y + (self.knob_scale * self.scrollitem as f32 + self.knob_height) as i32;

			if self.pos_x < mouse_x
				&& mouse_x < self.pos_x + self.width
				&& knob_top < mouse_y
				&& mouse_y < knob_bottom
			{
				self.mouse_mode = if mouse_click { 2 } else { 1 };
			} else {
				self.mouse_mode = 0;
			}
		}

		if self.mouse_mode == 2 {
			self.knob_y += scroll_y;
			if self.knob_y < 0 {
				self.knob_y = 0;
			}
			if self.knob_y > self.height - self.knob_height as i32 {
				self.knob_y = self.height - self.knob_height as i32;
			}

			self.scrollitem = round(self.knob_y as f32 / self.knob_scale) as i32;
			if self.scrollitem < 0 {
				self.scrollitem = 0;
			}
			if self.scrollitem > self.totalitems - self.showtems {
				self.scrollitem = self.totalitems - self.showtems;
			}
		} else {
			self.knob_y = round(self.knob_scale * self.scrollitem as f32) as i32;
		}
	}

	/// The upstream `ScrollbarObject::Draw`. The knob is not drawn when the whole list fits,
	/// so a bar that has nothing to scroll is only its body. The upstream checks its
	/// renderer pointer for NULL first, which a `&mut` cannot be.
	pub fn draw(&mut self, d3dg: &mut D3DGraphics) {
		d3dg.draw_2d_box(
			self.pos_x,
			self.pos_y,
			self.pos_x + self.width,
			self.pos_y + self.height,
			self.color_body,
		);

		if self.totalitems > self.showtems {
			let (color, color2) = if self.mouse_mode == 2 {
				(self.color1_select, self.color2_select)
			} else if self.mouse_mode == 1 {
				(self.color1_cursor, self.color2_cursor)
			} else {
				(self.color1_default, self.color2_default)
			};

			// The inner box is three pixels in from the outer one, which is what draws the
			// knob as a frame rather than a block.
			d3dg.draw_2d_box(
				self.pos_x,
				self.pos_y + self.knob_y,
				self.pos_x + self.width,
				self.pos_y + self.knob_y + self.knob_height as i32,
				color,
			);
			d3dg.draw_2d_box(
				self.pos_x + 3,
				self.pos_y + self.knob_y + 3,
				self.pos_x + self.width - 3,
				self.pos_y + self.knob_y + self.knob_height as i32 - 3,
				color2,
			);
		}
	}
}

impl Default for ScrollbarObject {
	fn default() -> ScrollbarObject {
		ScrollbarObject::new()
	}
}

/// The input box, the upstream `TextBoxObject`.
///
/// This is the string and the keys that type into it. Drawing the box, its border and its
/// text is the caller's, which is what the upstream's own note says.
pub struct TextBoxObject {
	/// Input mode, a combination of the flags `init_text_box` documents.
	mode: i32,
	/// The string, one byte per character and NUL terminated. Shift-JIS bytes, like every
	/// other game string.
	inputstr: [u8; MAX_INPUTBOXLEN + 1],
	/// Longest string this box accepts.
	maxlen: i32,
}

impl TextBoxObject {
	/// The upstream constructor: an empty box that accepts the full length.
	pub fn new() -> TextBoxObject {
		TextBoxObject {
			mode: 0,
			inputstr: [0; MAX_INPUTBOXLEN + 1],
			maxlen: MAX_INPUTBOXLEN as i32,
		}
	}

	/// The upstream `TextBoxObject::InitTextBox`.
	///
	/// The mode flags, which the header repeats:
	/// 0x01: digits, 0x02: digits / `-` / `.`, 0x04: lower case alphabet, 0x08: upper case
	/// alphabet, 0x20: space.
	pub fn init_text_box(&mut self, str: &[u8], in_maxlen: i32, in_mode: i32) {
		let mut maxlen = in_maxlen;
		if maxlen < 1 {
			maxlen = 1;
		}
		if maxlen > MAX_INPUTBOXLEN as i32 {
			maxlen = MAX_INPUTBOXLEN as i32;
		}

		self.mode = in_mode;
		// Upstream sets the string before the new length, so the length check inside reads
		// the length that was there before. On a fresh box that is the constructor's 127, so
		// the first default string is checked against 127 however small `in_maxlen` is.
		self.set_text_box_str(str);
		self.maxlen = maxlen;
	}

	/// The upstream `TextBoxObject::DestroyTextBox`, which has nothing to release: the string
	/// is a fixed array inside the object. The game calls it when the console closes.
	pub fn destroy_text_box(&mut self) {}

	/// The upstream `TextBoxObject::SetTextBoxStr`. A string longer than the box is refused
	/// and the box keeps what it had. The upstream checks its argument for NULL and uses an
	/// empty string, which a slice covers by being empty.
	pub fn set_text_box_str(&mut self, str: &[u8]) {
		if str.len() > self.maxlen as usize {
			return;
		}

		self.inputstr[..str.len()].copy_from_slice(str);
		// The bytes past the terminator are left as they were, which is what the upstream
		// `strcpy` leaves behind and what every reader stops before.
		self.inputstr[str.len()] = 0;
	}

	/// The upstream `TextBoxObject::GetTextBoxStr`, as the bytes up to the terminator rather
	/// than the fixed buffer. A caller appends it to a command line or copies it out, and
	/// neither wants the bytes the object happens to hold past the end.
	pub fn get_text_box_str(&self) -> &[u8] {
		&self.inputstr[..self.str_len()]
	}

	/// The length the upstream `strlen` would see on the fixed buffer.
	fn str_len(&self) -> usize {
		self.inputstr
			.iter()
			.position(|&byte| byte == 0)
			.unwrap_or(self.inputstr.len())
	}

	/// The upstream `TextBoxObject::InputChar`: appends one character at the end, and does
	/// nothing once the box is full.
	///
	/// Upstream the loop stops at `maxlen` rather than at the buffer, so a box holding
	/// exactly `maxlen` characters keeps its terminator outside the loop and refuses more
	/// input. There is no cursor to move: the character always goes at the end.
	fn input_char(&mut self, inchar: u8) {
		for i in 0..self.maxlen as usize {
			if self.inputstr[i] == 0 {
				self.inputstr[i] = inchar;
				self.inputstr[i + 1] = 0;
				return;
			}
		}
	}

	/// The upstream `TextBoxObject::DeleteChar`: removes the last character, and an empty box
	/// has none to remove.
	fn delete_char(&mut self) {
		let length = self.str_len();
		if length == 0 {
			return;
		}

		self.inputstr[length - 1] = 0;
	}

	/// The upstream `TextBoxObject::ProcessTextBox`. True means Enter was pressed with
	/// something in the box, which is the caller's cue to take the string and do something
	/// with it. The upstream answers false for a null input pointer, which a `&InputControl`
	/// cannot be.
	///
	/// The caller has to have called [`InputControl::get_input_state`] for this frame, which
	/// is what the upstream note says too: every check below is a frame edge.
	pub fn process_text_box(&mut self, input_ctrl: &InputControl) -> bool {
		if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x0F)) {
			// [ENTER]. An empty box is not an answer, and it does not delete anything
			// either, because the whole edit path below is in the else.
			if self.inputstr[0] != 0 {
				return true;
			}
		} else {
			// delete one character
			if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x0E)) {
				// [BACK]
				self.delete_char();
			}

			if self.mode & 0x01 != 0 || self.mode & 0x02 != 0 {
				// digits, 0x16 to 0x1F on the number row
				for key in 0x16..=0x1F {
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
						self.input_char((key + 0x1A) as u8);
					}
				}
				// the same digits on the numeric keypad, 0x04 to 0x0D
				for key in 0x04..=0x0D {
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
						self.input_char((key + 0x2C) as u8);
					}
				}

				if self.mode & 0x02 != 0 {
					// Upstream leaves the `+` of 0x48 commented out, so only `-` and `.` are
					// accepted, each from the row key and the keypad key.
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x3D)) {
						self.input_char(b'-');
					}
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x49)) {
						self.input_char(b'-');
					}
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x44)) {
						self.input_char(b'.');
					}
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x4A)) {
						self.input_char(b'.');
					}
				}
			}

			if self.mode & 0x04 != 0 && self.mode & 0x08 != 0 {
				// Both cases are accepted, so shift chooses between them, and it is read as
				// a level because the shift key is held while the letters are typed.
				if input_ctrl.check_key_now(originalkeycode_to_dinputdef(0x14)) {
					// [SHIFT], upper case alphabet
					for key in 0x20..=0x39 {
						if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
							self.input_char((key + 0x21) as u8);
						}
					}
				} else {
					for key in 0x20..=0x39 {
						if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
							self.input_char((key + 0x41) as u8);
						}
					}
				}
			} else if self.mode & 0x04 != 0 {
				for key in 0x20..=0x39 {
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
						self.input_char((key + 0x41) as u8);
					}
				}
			} else if self.mode & 0x08 != 0 {
				for key in 0x20..=0x39 {
					if input_ctrl.check_key_down(originalkeycode_to_dinputdef(key)) {
						self.input_char((key + 0x21) as u8);
					}
				}
			}

			// The comma of mode 0x10 is commented out upstream as well.

			if self.mode & 0x20 != 0 {
				// space
				if input_ctrl.check_key_down(originalkeycode_to_dinputdef(0x11)) {
					self.input_char(b' ');
				}
			}
		}

		false
	}
}

impl Default for TextBoxObject {
	fn default() -> TextBoxObject {
		TextBoxObject::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use xops_platform::gfx::Gfx;
	use xops_platform::wgpu;

	const BLUE: [u8; 4] = [0, 0, 255, 255];
	const RED: [u8; 4] = [255, 0, 0, 255];
	const GREEN: [u8; 4] = [0, 255, 0, 255];

	fn headless(width: u32, height: u32) -> Gfx {
		let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
			backends: wgpu::Backends::all(),
			..Default::default()
		});
		Gfx::new(&instance, None, width, height, None).expect("a headless renderer")
	}

	/// Runs `body` inside a frame and returns the read-back pixels, top row first.
	fn render(graphics: &mut D3DGraphics, body: impl FnOnce(&mut D3DGraphics)) -> Vec<u8> {
		graphics.start_render();
		body(graphics);
		assert!(!graphics.end_render(), "frame encoding failed");
		let (_, _, pixels) = graphics.gfx_ref().read_frame().expect("read frame");
		pixels
	}

	fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
		let index = ((y * width + x) * 4) as usize;
		[
			pixels[index],
			pixels[index + 1],
			pixels[index + 2],
			pixels[index + 3],
		]
	}

	/// A blue body, a red knob and a green inner, which is what the render tests sample.
	fn paint(scrollbar: &mut ScrollbarObject) {
		let blue = D3DGraphics::get_color_code(0.0, 0.0, 1.0, 1.0);
		let red = D3DGraphics::get_color_code(1.0, 0.0, 0.0, 1.0);
		let green = D3DGraphics::get_color_code(0.0, 1.0, 0.0, 1.0);
		scrollbar.set_color(blue, red, green, red, green, red, green);
	}

	/// A bar of 100 pixels holding 20 items, 5 of them on screen: 15 items of travel and 25
	/// pixels of it per item.
	fn bar() -> ScrollbarObject {
		let mut scrollbar = ScrollbarObject::new();
		scrollbar.create(10, 20, 8, 100, 20, 5, 0);
		scrollbar
	}

	#[test]
	fn create_computes_the_knob_layout() {
		let scrollbar = bar();

		// 100 / 20 * 5
		assert_eq!(scrollbar.knob_height, 25.0);
		// (100 - 25) / (20 - 5)
		assert_eq!(scrollbar.knob_scale, 5.0);
		assert_eq!(scrollbar.knob_y, 0);
		assert_eq!(scrollbar.get_scroll_item(), 0);
	}

	/// A list that fits entirely has a knob as tall as the bar and one scale unit per item,
	/// so nothing moves.
	#[test]
	fn a_list_that_fits_has_a_knob_that_covers_it() {
		let mut scrollbar = ScrollbarObject::new();
		scrollbar.create(0, 0, 8, 100, 3, 8, 0);

		// The shown count is pulled down to the total, which is how the upstream reports a
		// list that fits.
		assert_eq!(scrollbar.showtems, 3);
		assert_eq!(scrollbar.knob_height, 100.0);
		assert_eq!(scrollbar.knob_scale, 1.0);
	}

	/// Scrolling stops at the first item and at the last page, and one item is one step.
	#[test]
	fn scrolling_stops_at_both_ends() {
		let mut scrollbar = bar();

		scrollbar.scroll_up();
		assert_eq!(scrollbar.get_scroll_item(), 0);

		for _ in 0..100 {
			scrollbar.scroll_down();
		}
		// 20 items, 5 on screen: 15 is the last page.
		assert_eq!(scrollbar.get_scroll_item(), 15);

		scrollbar.scroll_up();
		assert_eq!(scrollbar.get_scroll_item(), 14);
	}

	/// `SetScrollItem` refuses anything outside the list, and it bounds by the total rather
	/// than by the last page.
	#[test]
	fn set_scroll_item_refuses_what_is_not_in_the_list() {
		let mut scrollbar = bar();

		scrollbar.set_scroll_item(-1);
		assert_eq!(scrollbar.get_scroll_item(), 0);
		scrollbar.set_scroll_item(20);
		assert_eq!(scrollbar.get_scroll_item(), 0);

		scrollbar.set_scroll_item(19);
		assert_eq!(scrollbar.get_scroll_item(), 19);
	}

	/// The hit test: away from the bar, over the knob, and dragging.
	#[test]
	fn the_knob_is_only_grabbed_where_it_is() {
		let mut scrollbar = bar();

		// Left of the bar, above it, and below it.
		scrollbar.input(5, 60, false, 0);
		assert!(!scrollbar.check_scrolling());
		scrollbar.input(15, 10, false, 0);
		assert!(!scrollbar.check_scrolling());
		scrollbar.input(15, 90, false, 0);
		assert!(!scrollbar.check_scrolling());

		// Over the knob without the button: the cursor is on it but nothing is dragged.
		scrollbar.input(15, 30, false, 0);
		assert_eq!(scrollbar.mouse_mode, 1);
		assert!(!scrollbar.check_scrolling());

		// With the button: the drag starts.
		scrollbar.input(15, 30, true, 0);
		assert_eq!(scrollbar.mouse_mode, 2);
		assert!(scrollbar.check_scrolling());

		// Releasing it ends the drag. The cursor is still on the knob, so the same frame
		// picks the knob up again without the button, which is the upstream's re-test.
		scrollbar.input(15, 30, false, 0);
		assert_eq!(scrollbar.mouse_mode, 1);
		assert!(!scrollbar.check_scrolling());
	}

	/// Dragging the knob scrolls the list, and the item cannot go past either end even when
	/// the movement is far larger than the bar.
	#[test]
	fn dragging_scrolls_the_list_between_its_ends() {
		let mut scrollbar = bar();

		scrollbar.input(15, 30, true, 0);
		assert_eq!(scrollbar.mouse_mode, 2);

		scrollbar.input(15, 30, true, 1000);
		assert_eq!(scrollbar.knob_y, 75);
		assert_eq!(scrollbar.get_scroll_item(), 15);

		scrollbar.input(15, 30, true, -1000);
		assert_eq!(scrollbar.knob_y, 0);
		assert_eq!(scrollbar.get_scroll_item(), 0);
	}

	/// A drag that has not started can still move the knob, because the upstream re-reads
	/// the knob position from the item whenever it is not dragging. Nothing scrolls.
	#[test]
	fn the_knob_follows_the_item_when_it_is_not_being_dragged() {
		let mut scrollbar = bar();

		scrollbar.set_scroll_item(15);
		scrollbar.input(0, 0, false, 0);
		assert_eq!(scrollbar.knob_y, 75);
	}

	/// The box takes characters at the end and refuses them once it is full.
	#[test]
	fn characters_are_appended_at_the_end() {
		let mut textbox = TextBoxObject::new();
		textbox.init_text_box(b"", 4, 0x01);

		textbox.input_char(b'1');
		textbox.input_char(b'2');
		assert_eq!(textbox.get_text_box_str(), b"12");

		textbox.input_char(b'3');
		textbox.input_char(b'4');
		assert_eq!(textbox.get_text_box_str(), b"1234");

		// Full: the terminator sits outside the loop that looks for it, so this is refused.
		textbox.input_char(b'5');
		assert_eq!(textbox.get_text_box_str(), b"1234");
	}

	/// Delete takes the last character, and an empty box has none.
	#[test]
	fn delete_removes_the_last_character_and_stops_there() {
		let mut textbox = TextBoxObject::new();
		textbox.init_text_box(b"hello", 10, 0x04);

		for _ in 0..3 {
			textbox.delete_char();
		}
		assert_eq!(textbox.get_text_box_str(), b"he");

		textbox.delete_char();
		textbox.delete_char();
		assert_eq!(textbox.get_text_box_str(), b"");

		// The empty case again, which must not underflow or panic.
		textbox.delete_char();
		assert_eq!(textbox.get_text_box_str(), b"");
	}

	/// A string longer than the box is refused, and the box keeps what it had.
	#[test]
	fn a_string_longer_than_the_box_is_refused() {
		let mut textbox = TextBoxObject::new();
		textbox.init_text_box(b"abc", 3, 0x04);
		assert_eq!(textbox.get_text_box_str(), b"abc");

		textbox.set_text_box_str(b"abcd");
		assert_eq!(textbox.get_text_box_str(), b"abc");

		textbox.set_text_box_str(b"ab");
		assert_eq!(textbox.get_text_box_str(), b"ab");
	}

	/// The default string of `InitTextBox` is checked against the length the box had before,
	/// because the upstream sets the string first and the new length second.
	#[test]
	fn the_default_string_is_checked_against_the_old_length() {
		let mut textbox = TextBoxObject::new();

		// A fresh box holds the constructor's 127, so this six byte default is accepted
		// even though the box is being limited to four.
		textbox.init_text_box(b"abcdef", 4, 0x04);
		assert_eq!(textbox.get_text_box_str(), b"abcdef");
		assert_eq!(textbox.maxlen, 4);

		// The next default is checked against four, so five bytes are refused. Four would
		// not be: the check is longer than, not longer than or equal.
		textbox.init_text_box(b"ghijk", 4, 0x04);
		assert_eq!(textbox.get_text_box_str(), b"abcdef");
	}

	/// The length is clamped to the box: a limit of zero becomes one and a limit past the
	/// buffer becomes the buffer's length.
	#[test]
	fn the_length_is_clamped() {
		let mut textbox = TextBoxObject::new();

		textbox.init_text_box(b"", 0, 0x01);
		assert_eq!(textbox.maxlen, 1);

		textbox.init_text_box(b"", 1000, 0x01);
		assert_eq!(textbox.maxlen, MAX_INPUTBOXLEN as i32);
	}

	/// With no key down, the box is left alone and Enter is not reported.
	#[test]
	fn no_keys_down_does_not_report_enter() {
		let mut textbox = TextBoxObject::new();
		textbox.init_text_box(b"abc", 8, 0x04);
		let input_ctrl = InputControl::new();

		assert!(!textbox.process_text_box(&input_ctrl));
		assert_eq!(textbox.get_text_box_str(), b"abc");
	}

	/// The knob lands on the pixels the scrolled item puts it on, and it moves by its whole
	/// travel between the first and the last page.
	#[test]
	fn the_knob_is_drawn_where_the_scrolled_item_puts_it() {
		let mut graphics = D3DGraphics::new(headless(32, 32), false, 0);
		let mut scrollbar = ScrollbarObject::new();
		scrollbar.create(4, 4, 8, 20, 10, 5, 0);
		paint(&mut scrollbar);

		// At the first item the knob covers y 4 to 14 of the bar's y 4 to 24.
		let pixels = render(&mut graphics, |graphics| scrollbar.draw(graphics));
		assert_eq!(pixel(&pixels, 32, 5, 20), BLUE);
		assert_eq!(pixel(&pixels, 32, 5, 8), RED);
		// The inner box is drawn three pixels in from the knob's edge.
		assert_eq!(pixel(&pixels, 32, 8, 9), GREEN);

		// The last page is five items further, and one item is two pixels of travel.
		scrollbar.set_scroll_item(5);
		scrollbar.input(0, 0, false, 0);
		assert_eq!(scrollbar.knob_y, 10);

		let pixels = render(&mut graphics, |graphics| scrollbar.draw(graphics));
		assert_eq!(pixel(&pixels, 32, 5, 5), BLUE);
		assert_eq!(pixel(&pixels, 32, 5, 20), RED);
	}

	/// A list that fits has no knob at all, so nothing but the body reaches the target.
	#[test]
	fn a_list_that_fits_draws_only_the_body() {
		let mut graphics = D3DGraphics::new(headless(32, 32), false, 0);
		let mut scrollbar = ScrollbarObject::new();
		scrollbar.create(4, 4, 8, 20, 3, 5, 0);
		paint(&mut scrollbar);

		let pixels = render(&mut graphics, |graphics| scrollbar.draw(graphics));
		assert_eq!(pixel(&pixels, 32, 5, 8), BLUE);
		assert_eq!(pixel(&pixels, 32, 8, 9), BLUE);
	}
}
