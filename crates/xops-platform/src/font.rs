//! System-font text: the menus, the briefing and the console.
//!
//! The Win32 build draws this text with GDI display lists and the browser build with a
//! canvas, neither of which exists here. Both do the same thing though: rasterise a
//! whole string into one RGBA bitmap, upload it once and let the vertex colour tint it.
//! This does the same, with fontdue as the rasteriser.
//!
//! The upstream draws at two sizes, 18px for `SystemFont` (menus, briefing, console)
//! and 12px for `SystemSmallFont`, and both arrive here as `pixel_size`. The 18 is a
//! 480 line reference size that the callers scale by the screen height themselves, so
//! the scaling stays with them. What the upstream calls `limitlen` stays there too: it
//! is a character count the texture-font path turns into a scale factor, and a
//! rasteriser has no opinion on it.
//!
//! The font is found at runtime instead of bundled. The game's data files are not
//! redistributable and this repository ships no third-party binaries, so a system font
//! with Japanese coverage is the only option, and [`TextRenderer::new`] names every path
//! it tried when a machine has none.

use std::collections::HashMap;

use fontdue::{Font, FontSettings};

/// The blank pixels kept around the text on every side.
///
/// The draw samples this texture with linear filtering, so a glyph that touched the
/// edge would blend with whatever the neighbouring texture slot holds. Inset by one.
const PAD: u32 = 1;

/// How many bitmaps the cache holds before it is emptied wholesale.
///
/// The working set is HUD labels and menu lines: a few dozen strings at two sizes.
/// 128 entries cover that with room to spare, and a briefing body at 18px is about
/// 60 KB of RGBA, which puts the ceiling near 8 MB. Filling the cache means a screen
/// drew more distinct strings than that, and there a wholesale clear costs one
/// re-rasterise of what is on screen, which is cheaper than the bookkeeping an LRU
/// would need to avoid it.
pub const CACHE_MAX: usize = 128;

/// A rasterised run of text, ready to upload as an RGBA texture.
///
/// Top row first, four bytes per pixel, the layout `Gfx::upload_texture` wants.
#[derive(Clone, Debug, PartialEq)]
pub struct TextBitmap {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

/// One glyph, placed relative to the bitmap's top left corner.
struct Placement {
	ch: char,
	/// Left edge of the glyph's cell.
	x: i32,
	/// The line's baseline, measured down from the bitmap's top edge.
	baseline: i32,
}

/// Where every glyph of a string sits, and the bitmap that holds them.
struct Layout {
	width: u32,
	height: u32,
	glyphs: Vec<Placement>,
}

/// A system font and the bitmaps already built from it.
pub struct TextRenderer {
	font: Font,
	/// The path the font was loaded from, for the log line at startup.
	path: String,
	/// The game draws the same labels every frame, and rasterising is milliseconds
	/// against a memcpy for a cache hit, so every bitmap is kept here.
	cache: HashMap<(Vec<u8>, u32), TextBitmap>,
	rasters: u64,
}

impl TextRenderer {
	/// Finds and loads a system font.
	///
	/// Returns an error naming every path tried, rather than silently drawing nothing.
	pub fn new() -> Result<TextRenderer, String> {
		let mut tried = Vec::new();

		for path in CANDIDATES {
			let bytes = match std::fs::read(path) {
				Ok(bytes) => bytes,
				Err(e) => {
					tried.push(format!("{path} ({e})"));
					continue;
				}
			};

			// Single characters are rasterised one at a time, never runs, so there is
			// nothing for a substituted glyph to depend on and the gsub walk is wasted.
			let settings = FontSettings {
				load_substitutions: false,
				..FontSettings::default()
			};

			match Font::from_bytes(bytes, settings) {
				Ok(font) => {
					return Ok(TextRenderer {
						font,
						path: (*path).to_string(),
						cache: HashMap::new(),
						rasters: 0,
					})
				}
				Err(e) => tried.push(format!("{path} ({e})")),
			}
		}

		Err(format!(
			"no usable system font found, tried: {}",
			tried.join(", ")
		))
	}

	/// The path of the font in use.
	pub fn path(&self) -> &str {
		&self.path
	}

	/// How many bitmaps have been built, as opposed to served from the cache.
	pub fn rasters(&self) -> u64 {
		self.rasters
	}

	/// Rasterises a line of Shift-JIS bytes at the given pixel size.
	///
	/// A `\n` in the text starts a new line, as in the GDI original, which breaks the
	/// string itself while drawing it.
	pub fn rasterize(&mut self, text: &[u8], pixel_size: f32) -> Result<TextBitmap, String> {
		if !(pixel_size > 0.0) {
			return Err(format!("font: pixel size {pixel_size} is not positive"));
		}

		let key = cache_key(text, pixel_size);
		if let Some(hit) = self.cache.get(&key) {
			return Ok(hit.clone());
		}

		let layout = self.layout(text, pixel_size);
		let pixels = layout.width as usize * layout.height as usize;

		// White with the glyph's coverage in alpha, so that a vertex colour multiplies
		// straight through and the text takes the colour it is drawn with. A tint needs
		// the texture's RGB to be neutral: any colour baked in here would multiply with
		// the draw's colour and come out wrong.
		let mut rgba = Vec::with_capacity(pixels * 4);
		for _ in 0..pixels {
			rgba.extend_from_slice(&[255, 255, 255, 0]);
		}

		for glyph in &layout.glyphs {
			let (metrics, coverage) = self.font.rasterize(glyph.ch, pixel_size);
			// The coverage buffer starts at the glyph bitmap's top left. `ymin` counts up
			// from the baseline to the bitmap's bottom edge, so in these y-down
			// coordinates the top edge is that far above the baseline, less the height.
			let top = glyph.baseline - metrics.ymin - metrics.height as i32;

			for row in 0..metrics.height as i32 {
				let y = top + row;
				if y < 0 || y >= layout.height as i32 {
					continue;
				}
				for col in 0..metrics.width as i32 {
					let x = glyph.x + metrics.xmin + col;
					if x < 0 || x >= layout.width as i32 {
						continue;
					}
					let cov = coverage[row as usize * metrics.width + col as usize];
					let alpha = (y as usize * layout.width as usize + x as usize) * 4 + 3;
					// Overhanging neighbours overlap by a pixel, and coverage only ever
					// adds ink.
					if cov > rgba[alpha] {
						rgba[alpha] = cov;
					}
				}
			}
		}

		let bitmap = TextBitmap {
			width: layout.width,
			height: layout.height,
			rgba,
		};

		self.rasters += 1;
		if self.cache.len() >= CACHE_MAX {
			self.cache.clear();
		}
		self.cache.insert(key, bitmap.clone());

		Ok(bitmap)
	}

	/// Width and height in pixels the text would occupy, without rasterising it.
	pub fn measure(&mut self, text: &[u8], pixel_size: f32) -> (u32, u32) {
		if !(pixel_size > 0.0) {
			return (0, 0);
		}
		// A string already drawn is measured from its bitmap, which is both cheaper than
		// laying it out again and guaranteed to be the size the draw will use.
		if let Some(hit) = self.cache.get(&cache_key(text, pixel_size)) {
			return (hit.width, hit.height);
		}
		let layout = self.layout(text, pixel_size);
		(layout.width, layout.height)
	}

	/// Places every glyph of the decoded string on a baseline, a line advance apart.
	fn layout(&self, text: &[u8], pixel_size: f32) -> Layout {
		let (ascent, advance) = match self.font.horizontal_line_metrics(pixel_size) {
			Some(m) => (m.ascent, m.new_line_size.max(pixel_size)),
			// A font without the horizontal metrics is one fontdue could not read the
			// hhea table of. Keep the em box as the line rather than stacking lines.
			None => (pixel_size, pixel_size * 1.25),
		};

		let baseline = ascent.ceil() as i32 + PAD as i32;
		let advance = advance.ceil() as i32;

		let mut glyphs = Vec::new();
		let mut widest = 0.0f32;
		let mut pen = 0.0f32;
		let mut line = 0i32;

		for ch in decode(text).chars() {
			if ch == '\n' {
				widest = widest.max(pen);
				pen = 0.0;
				line += 1;
				continue;
			}
			let metrics = self.font.metrics(ch, pixel_size);
			glyphs.push(Placement {
				ch,
				x: PAD as i32 + pen.round() as i32,
				baseline: baseline + line * advance,
			});
			pen += metrics.advance_width;
		}
		widest = widest.max(pen);

		Layout {
			width: widest.ceil() as u32 + PAD * 2,
			height: (line as u32 + 1) * advance as u32 + PAD * 2,
			glyphs,
		}
	}
}

/// The cache key: the bytes that were drawn and the size they were drawn at.
fn cache_key(text: &[u8], pixel_size: f32) -> (Vec<u8>, u32) {
	(text[..text_end(text)].to_vec(), pixel_size.to_bits())
}

/// Decodes Shift-JIS into the text the rasteriser lays out.
///
/// This is the one place in the port that decodes it: everywhere else the game's strings
/// stay bytes, because the texture font indexes glyphs by byte value and the browsers'
/// strings are Shift-JIS too. Bytes after a NUL are dropped, since the game keeps its
/// strings as NUL-terminated C buffers and the canvas build read them the same way.
/// Undecodable bytes become U+FFFD rather than failing the draw.
fn decode(text: &[u8]) -> String {
	encoding_rs::SHIFT_JIS
		.decode(&text[..text_end(text)])
		.0
		.into_owned()
}

/// Where the string ends: at its NUL terminator, or at the end of the slice.
fn text_end(text: &[u8]) -> usize {
	text.iter().position(|&b| b == 0).unwrap_or(text.len())
}

/// Where a system font is looked for, in the order they are tried.
///
/// The CJK fonts come first on every platform: the menus, briefing bodies and weapon
/// names are Japanese, and a latin-only font would draw them as boxes. The latin fonts
/// at the end are there so a machine with no CJK font at all can still run the game.
#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &[
	// The default Japanese face since El Capitan.
	"/System/Library/Fonts/PingFang.ttc",
	"/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
	"/System/Library/Fonts/Hiragino Sans GB.ttc",
	"/System/Library/Fonts/Supplemental/Songti.ttc",
	"/Library/Fonts/Arial Unicode.ttf",
	"/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
	"/System/Library/Fonts/Helvetica.ttc",
];

#[cfg(target_os = "windows")]
const CANDIDATES: &[&str] = &[
	"\\Windows\\Fonts\\meiryo.ttc",
	"\\Windows\\Fonts\\msgothic.ttc",
	"\\Windows\\Fonts\\YuGothM.ttc",
	"\\Windows\\Fonts\\msmincho.ttc",
	"C:\\Windows\\Fonts\\meiryo.ttc",
	"C:\\Windows\\Fonts\\msgothic.ttc",
	"C:\\Windows\\Fonts\\segoeui.ttf",
];

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CANDIDATES: &[&str] = &[
	"/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
	"/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
	"/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
	"/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
	"/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttf",
	"/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
	"/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
	"/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
	"/usr/share/fonts/TTF/DejaVuSans.ttf",
];

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::{Duration, Instant};

	/// The machine's font, or a printed note and `None` when there is none. The tests
	/// skip on `None` so the suite stays useful on a bare machine, and none of them
	/// asserts anything less when a font did load.
	fn renderer() -> Option<TextRenderer> {
		match TextRenderer::new() {
			Ok(renderer) => {
				println!("font test font: {}", renderer.path());
				Some(renderer)
			}
			Err(e) => {
				println!("skipping font test: {e}");
				None
			}
		}
	}

	#[test]
	fn rasterizes_ascii_with_coverage() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let bitmap = renderer.rasterize(b"Hello", 16.0).expect("ascii draws");

		assert!(bitmap.width > 0, "no width for 'Hello'");
		assert!(bitmap.height > 0, "no height for 'Hello'");
		assert_eq!(
			bitmap.rgba.len(),
			bitmap.width as usize * bitmap.height as usize * 4
		);

		let covered = bitmap.rgba.chunks(4).filter(|p| p[3] > 0).count();
		assert!(covered > 0, "'Hello' rasterised to nothing");
	}

	#[test]
	fn measure_matches_the_bitmap() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let bitmap = renderer.rasterize(b"Mission", 18.0).unwrap();
		let measured = renderer.measure(b"Mission", 18.0);

		assert_eq!(measured, (bitmap.width, bitmap.height));
	}

	#[test]
	fn longer_text_measures_wider() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let (short, short_height) = renderer.measure(b"XOPS", 16.0);
		let (long, long_height) = renderer.measure(b"XOPSXOPSXOPS", 16.0);

		assert!(long > short, "{long} is not wider than {short}");
		assert_eq!(short_height, long_height, "one line either way");
	}

	#[test]
	fn cache_serves_the_second_draw() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let first = renderer.rasterize(b"HUD 100", 16.0).unwrap();
		let rasters = renderer.rasters();
		let second = renderer.rasterize(b"HUD 100", 16.0).unwrap();

		assert_eq!(first, second);
		assert_eq!(
			renderer.rasters(),
			rasters,
			"the second draw rasterised again instead of hitting the cache"
		);
	}

	#[test]
	fn pixels_are_white_with_varying_alpha() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let bitmap = renderer.rasterize(b"Ag", 24.0).unwrap();
		let mut alphas = std::collections::BTreeSet::new();

		for pixel in bitmap.rgba.chunks(4) {
			assert_eq!(
				&pixel[..3],
				&[255, 255, 255],
				"glyph pixels must be white or the draw's colour tints them wrong"
			);
			alphas.insert(pixel[3]);
		}

		assert!(alphas.len() > 1, "coverage is flat, so nothing was antialiased");
		assert_eq!(alphas.iter().next(), Some(&0), "no blank pixel in the bitmap");
		assert!(
			*alphas.iter().next_back().unwrap() > 0,
			"no covered pixel in the bitmap"
		);
	}

	#[test]
	fn newline_starts_a_new_line() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let one = renderer.rasterize(b"line", 16.0).unwrap();
		let two = renderer.rasterize(b"line\nline", 16.0).unwrap();

		assert_eq!(two.width, one.width);
		assert!(two.height > one.height, "the line break added no height");
	}

	/// The Japanese path end to end: Shift-JIS bytes in, glyphs out, at the size the
	/// briefing uses. Also reports what a fresh 40 character string costs, which is the
	/// number to check a frame against when a caller rasterises per frame instead of
	/// drawing a cached label.
	#[test]
	fn rasterizes_forty_shift_jis_characters() {
		let Some(mut renderer) = renderer() else {
			return;
		};

		let base: Vec<char> =
			"敵勢力の排除および目標地点への移動を完了せよ。味方部隊との連携を維持し、市民の被害を最小限に抑えること。"
				.chars()
				.collect();
		assert!(base.len() >= 40);

		const RUNS: usize = 20;
		let mut total = Duration::ZERO;

		for run in 0..RUNS {
			// A different 40 characters every run, so every one of them misses the cache.
			let text: String = (0..40).map(|i| base[(run + i) % base.len()]).collect();
			let bytes = encoding_rs::SHIFT_JIS.encode(&text).0;
			// Full width characters, two Shift-JIS bytes each: the byte strings really
			// are the data and this is where they turn into text.
			assert_eq!(bytes.len(), 80);

			let start = Instant::now();
			let bitmap = renderer.rasterize(&bytes, 18.0).expect("japanese draws");
			total += start.elapsed();

			let covered = bitmap.rgba.chunks(4).filter(|p| p[3] > 0).count();
			assert!(covered > 0, "no coverage, the font has no Japanese glyphs");
		}

		println!(
			"fresh 40 character Japanese rasterise at 18px: {:?} each over {RUNS} runs",
			total / RUNS as u32
		);
	}
}
