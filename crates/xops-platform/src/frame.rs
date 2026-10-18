//! Writing rendered frames to disk.
//!
//! The parity harness compares the browser build's frames against this one, so a frame
//! has to be reachable as bytes as well as on screen. This writes RGBA8 straight out,
//! which is the layout [`crate::gfx::Gfx::read_frame`] hands back.

use std::fs;
use std::path::Path;

/// Writes one PNG. Any existing file is replaced.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
	let expected = (width as usize) * (height as usize) * 4;
	if rgba.len() < expected {
		return Err(format!(
			"{}: wanted {expected} bytes of pixels, got {}",
			path.display(),
			rgba.len()
		));
	}

	let file = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
	let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
	encoder.set_color(png::ColorType::Rgba);
	encoder.set_depth(png::BitDepth::Eight);
	let mut writer = encoder
		.write_header()
		.map_err(|e| format!("{}: {e}", path.display()))?;
	writer
		.write_image_data(&rgba[..expected])
		.map_err(|e| format!("{}: {e}", path.display()))?;
	Ok(())
}

/// Writes `frames` numbered PNGs into `dir`, creating it when missing.
pub fn write_frame_sequence(
	dir: &Path,
	stem: &str,
	width: u32,
	height: u32,
	frames: &[Vec<u8>],
) -> Result<(), String> {
	fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
	for (index, pixels) in frames.iter().enumerate() {
		let path = dir.join(format!("{stem}_{index:03}.png"));
		write_png(&path, width, height, pixels)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_short_buffer_is_rejected_rather_than_written_short() {
		let path = std::env::temp_dir().join("xops_frame_test.png");
		let error = write_png(&path, 4, 4, &[0u8; 8]).expect_err("short buffer");
		assert!(error.contains("wanted 64 bytes"), "{error}");
	}

	#[test]
	fn a_frame_round_trips_through_the_file() {
		let path = std::env::temp_dir().join("xops_frame_roundtrip.png");
		let mut pixels = vec![0u8; 3 * 2 * 4];
		// A red pixel, a green one and a blue one, so a channel swap would show.
		pixels[0..4].copy_from_slice(&[255, 0, 0, 255]);
		pixels[4..8].copy_from_slice(&[0, 255, 0, 255]);
		pixels[8..12].copy_from_slice(&[0, 0, 255, 255]);
		write_png(&path, 3, 2, &pixels).expect("write");

		let file = fs::File::open(&path).expect("open");
		let decoder = png::Decoder::new(std::io::BufReader::new(file));
		let mut reader = decoder.read_info().expect("header");
		let mut buffer = vec![0u8; reader.output_buffer_size()];
		let info = reader.next_frame(&mut buffer).expect("frame");
		assert_eq!(info.width, 3);
		assert_eq!(info.height, 2);
		assert_eq!(&buffer[0..4], &[255, 0, 0, 255]);
		assert_eq!(&buffer[4..8], &[0, 255, 0, 255]);
		assert_eq!(&buffer[8..12], &[0, 0, 255, 255]);

		let _ = fs::remove_file(&path);
	}
}
