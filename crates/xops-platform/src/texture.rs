//! Texture decoding, ported from `d3dgraphics-opengl.cpp` lines 973-1560.
//!
//! The upstream functions are
//!
//! * `D3DGraphics::CheckTextureFileExtension` (line 973) - format from the file extension
//! * `D3DGraphics::CheckTextureFileTypeFlag`    (line 1014) - format from the first bytes
//! * `D3DGraphics::LoadBMPTexture`              (line 1190)
//! * `D3DGraphics::LoadDDSTexture`              (line 1370)
//! * `D3DGraphics::LoadJPEGTexture`             (line 1443, behind `ENABLE_OPENGL_LIBJPEGPNG`)
//! * `D3DGraphics::LoadPNGTexture`              (line 1512, behind `ENABLE_OPENGL_LIBJPEGPNG`)
//!
//! `LoadTexture` (line 1052) is the caller that picks the loader from the format number and
//! hands over `BlackTransparent`. It lives with the GPU state, so it is not part of this
//! module. Where upstream fills a `TEXTUREDATA { useflag, width, height, data }` out
//! parameter, [`decode`] returns a [`DecodedTexture`]: the `useflag` is the caller's bookkeeping.
//!
//! # Channel order
//!
//! Upstream feeds `data` to `glTexImage2D(GL_RGBA, GL_UNSIGNED_BYTE)` and every loader
//! stores bytes as `[0] = red, [1] = green, [2] = blue, [3] = alpha` (a 24-bit BMP pixel is
//! read from the file as B,G,R and stored as `data[0] = pixel[2]`, i.e. red first).
//! [`DecodedTexture::rgba`] keeps exactly that: **R, G, B, A** interleaved, row major, and
//! the top row of the picture first. `gfx::upload_texture` takes the same layout
//! (`wgpu::TextureFormat::Rgba8Unorm`, `bytes_per_row = width * 4`).

use std::path::Path;

/// Upstream `format == 0` in `LoadTexture`: unsupported, the texture is treated as missing.
pub const FORMAT_UNKNOWN: i32 = 0;
pub const FORMAT_BMP: i32 = 1;
pub const FORMAT_DDS: i32 = 2;
pub const FORMAT_JPEG: i32 = 3;
pub const FORMAT_PNG: i32 = 4;

/// The decoded form of upstream's `TEXTUREDATA`, minus the `useflag`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8, row major, top row first (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

/// Determine the texture format from the file extension.
///
/// Upstream: `int D3DGraphics::CheckTextureFileExtension(const char* filename, int nowformat)`
/// (line 973). The name is lowercased, then scanned backwards for the last `.` whose
/// remainder is exactly one of the known extensions. `nowformat` comes back when the
/// extension says nothing.
pub fn check_texture_file_extension(filename: &[u8], nowformat: i32) -> i32 {
    // Upstream copies through a NUL-terminated buffer with strlen(), so anything after an
    // embedded NUL is invisible.
    let name = match filename.iter().position(|&b| b == 0) {
        Some(end) => &filename[..end],
        None => filename,
    };
    // C's tolower() in the "C" locale: only A-Z change.
    let lower: Vec<u8> = name.iter().map(|b| b.to_ascii_lowercase()).collect();

    // Upstream's loop is `for(i = strlen-1; i > 0; i--)`, so a dot at index 0 never matches
    // and the suffix is compared from that dot to the end of the name.
    for i in (1..lower.len()).rev() {
        if lower[i] == b'.' {
            match &lower[i..] {
                b".bmp" => return FORMAT_BMP,
                b".dds" => return FORMAT_DDS,
                b".jpeg" | b".jpg" | b".jpe" => return FORMAT_JPEG,
                b".png" => return FORMAT_PNG,
                _ => {}
            }
        }
    }

    nowformat
}

/// Determine the texture format from the file header.
///
/// Upstream: `int D3DGraphics::CheckTextureFileTypeFlag(const char* filename, int nowformat)`
/// (line 1014). Upstream opens the file itself and returns `false` (0) when the open fails.
/// This takes the head of the file instead, so the caller is the one that reports an
/// unreadable file (see [`decode`]); `nowformat` comes back when the bytes say nothing.
pub fn check_texture_file_type_flag(data: &[u8], nowformat: i32) -> i32 {
    // The order of these tests is upstream's, and BMP comes first.
    if data.len() >= 2 && data[0] == b'B' && data[1] == b'M' {
        return FORMAT_BMP; // .bmp
    }
    if data.len() >= 3 && data[0] == b'D' && data[1] == b'D' && data[2] == b'S' {
        return FORMAT_DDS; // .dds
    }
    if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
        return FORMAT_JPEG; // .jpg
    }
    if data.len() >= 4 && data[0] == 0x89 && data[1] == b'P' && data[2] == b'N' && data[3] == b'G' {
        return FORMAT_PNG; // .png (the signature is really 8 bytes)
    }

    nowformat
}

/// Decode a texture file of an already detected `format`.
///
/// This is the body of the `format` dispatch in `LoadTexture` (lines 1081-1102): a failure
/// here is upstream's `return false` from a loader, which makes `LoadTexture` give up and
/// the texture count as missing.
pub fn decode(path: &Path, format: i32, black_transparent: bool) -> Result<DecodedTexture, String> {
	let buf = crate::fs::read_file(crate::path::resolve_case_insensitive(path))
		.map_err(|e| format!("{}: {e}", path.display()))?;

    match format {
        FORMAT_BMP => load_bmp_texture(&buf, black_transparent).map_err(|e| format!("{}: {e}", path.display())),
        FORMAT_DDS => load_dds_texture(&buf, black_transparent).map_err(|e| format!("{}: {e}", path.display())),
        FORMAT_JPEG => load_jpeg_texture(&buf, black_transparent).map_err(|e| format!("{}: {e}", path.display())),
        FORMAT_PNG => load_png_texture(&buf, black_transparent).map_err(|e| format!("{}: {e}", path.display())),
        other => Err(format!("{}: unsupported texture format {other}", path.display())),
    }
}

/// `*(int*)&header[at]` -- upstream reads every header field as a little endian machine int.
fn le_u32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

/// A bounds checked `fread`. Upstream trusts the file to be long enough, which turns a
/// truncated texture into a crash or garbage. Here the read fails instead.
fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], String> {
    let end = pos.checked_add(n).ok_or("truncated file")?;
    if end > buf.len() {
        return Err("truncated file".to_string());
    }
    let out = &buf[*pos..end];
    *pos = end;
    Ok(out)
}

/// The `BlackTransparent` snippet, which every loader repeats inline: a pixel whose *RGB*
/// is exactly black becomes fully transparent. The incoming alpha is not consulted.
fn black_to_transparent(rgba: &mut [u8], i: usize) {
    if rgba[i] == 0 && rgba[i + 1] == 0 && rgba[i + 2] == 0 {
        rgba[i + 3] = 0;
    }
}

/// The pixel count of `width * height`, checked, as upstream's `new unsigned char[w*h*4]`.
fn rgba_len(width: u32, height: u32) -> Result<usize, String> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| format!("image too large: {width}x{height}"))
}

/// Rows of a BMP are padded to a multiple of 4 bytes.
fn bmp_row_padding(row_bytes: usize) -> usize {
    (4 - (row_bytes % 4)) % 4
}

/// Load a BMP file.
///
/// Upstream: `bool D3DGraphics::LoadBMPTexture(const char*, bool, TEXTUREDATA*)` (line 1190).
///
/// Deviations, all of them places where upstream reads uninitialised memory or mis-strides:
///
/// * A negative height means top-down rows in a BMP. Upstream stores the height in an
///   `unsigned int` and then loops `for(int h = height-1; h >= 0; h--)`, which never
///   allocates and never fills anything for a top-down file. The top-down order is
///   implemented here instead of returning a blank texture.
/// * Upstream pads 24-bit rows by `4 - (width % 4)` rather than by the row length, which
///   mis-strides odd widths. The padding here is the truth about the row: `4` byte rows.
/// * A bit depth outside 4/8/24/32 leaves upstream's buffer uninitialised and still returns
///   success. This fails instead.
fn load_bmp_texture(buf: &[u8], black_transparent: bool) -> Result<DecodedTexture, String> {
    if buf.len() < 54 {
        return Err("not a .bmp: file shorter than the 54 byte header".to_string());
    }
    if buf[0] != b'B' || buf[1] != b'M' {
        return Err("not a .bmp".to_string());
    }

    // `dataPos = *(int*)&(header[0x0E]) + 14` -- 0x0E is biSize, so this is the palette and
    // pixel data that follows a header of that size.
    let data_pos = le_u32(buf, 0x0E) as usize + 14;
    let width = le_u32(buf, 0x12);
    let height_raw = le_u32(buf, 0x16) as i32;
    let bits = le_u32(buf, 0x1C);

    let top_down = height_raw < 0;
    let height = height_raw.unsigned_abs();
    let mut rgba = vec![0u8; rgba_len(width, height)?];
    let mut pos = data_pos;

    let row_at = |row: u32| -> usize {
        if top_down {
            row as usize
        } else {
            (height - 1 - row) as usize
        }
    };
    let width_usize = width as usize;

    match bits {
        // 4 bits per pixel: a 16 colour palette, two pixels per byte, high nibble first.
        4 => {
            let pallet = take(buf, &mut pos, 16 * 4)?.to_vec();
            let row_bytes = width_usize / 2;
            let pad = bmp_row_padding(row_bytes);
            for row in 0..height {
                let h = row_at(row);
                for w in 0..row_bytes {
                    let byte = take(buf, &mut pos, 1)?[0];
                    for (half, index) in [(0, byte >> 4), (1, byte & 0x0F)] {
                        let p = h * width_usize + w * 2 + half;
                        let pal = index as usize * 4;
                        rgba[p * 4] = pallet[pal + 2];
                        rgba[p * 4 + 1] = pallet[pal + 1];
                        rgba[p * 4 + 2] = pallet[pal];
                        rgba[p * 4 + 3] = 255;
                        if black_transparent {
                            black_to_transparent(&mut rgba, p * 4);
                        }
                    }
                }
                take(buf, &mut pos, pad)?;
            }
        }
        // 8 bits per pixel: a 256 colour palette, one byte per pixel.
        8 => {
            let pallet = take(buf, &mut pos, 256 * 4)?.to_vec();
            let row_bytes = width_usize;
            let pad = bmp_row_padding(row_bytes);
            for row in 0..height {
                let h = row_at(row);
                for w in 0..row_bytes {
                    let index = take(buf, &mut pos, 1)?[0] as usize * 4;
                    let i = (h * width_usize + w) * 4;
                    rgba[i] = pallet[index + 2];
                    rgba[i + 1] = pallet[index + 1];
                    rgba[i + 2] = pallet[index];
                    rgba[i + 3] = 255;
                    if black_transparent {
                        black_to_transparent(&mut rgba, i);
                    }
                }
                take(buf, &mut pos, pad)?;
            }
        }
        // 24 bits per pixel: full colour, B G R in the file.
        24 => {
            let row_bytes = width_usize * 3;
            let pad = bmp_row_padding(row_bytes);
            for row in 0..height {
                let h = row_at(row);
                for w in 0..width_usize {
                    let pixel = take(buf, &mut pos, 3)?;
                    let i = (h * width_usize + w) * 4;
                    rgba[i] = pixel[2];
                    rgba[i + 1] = pixel[1];
                    rgba[i + 2] = pixel[0];
                    rgba[i + 3] = 255;
                    if black_transparent {
                        black_to_transparent(&mut rgba, i);
                    }
                }
                take(buf, &mut pos, pad)?;
            }
        }
        // 32 bits per pixel: full colour, the file's fourth byte is dropped for a solid alpha.
        32 => {
            for row in 0..height {
                let h = row_at(row);
                for w in 0..width_usize {
                    let pixel = take(buf, &mut pos, 4)?;
                    let i = (h * width_usize + w) * 4;
                    rgba[i] = pixel[2];
                    rgba[i + 1] = pixel[1];
                    rgba[i + 2] = pixel[0];
                    rgba[i + 3] = 255;
                    if black_transparent {
                        black_to_transparent(&mut rgba, i);
                    }
                }
                // With 4 bytes per pixel no padding is needed.
            }
        }
        other => return Err(format!("BMP: unsupported {other} bits per pixel")),
    }

    Ok(DecodedTexture { width, height, rgba })
}

/// Load a DDS file.
///
/// Upstream: `bool D3DGraphics::LoadDDSTexture(const char*, bool, TEXTUREDATA*)` (line 1370).
///
/// Only two pixel formats are accepted, chosen by `dwRGBBitCount` at 0x58:
///
/// * `16` -- ARGB4444. Upstream ignores the header's channel masks and always expands the
///   nibbles as `R = high byte << 4`, `G = low byte`, `B = low byte << 4`, `A = high byte`.
///   The shipped textures (`data\char.dds` has masks R 0x0F00, G 0x00F0, B 0x000F,
///   A 0xF000) match exactly that.
/// * `32` -- ARGB8888, stored B G R A and rearranged to R G B A. The masks are ignored too.
///
/// Anything else fails, which is upstream's `return false`, and the texture then counts as
/// missing. That includes the compressed formats: a DXT1/DXT3/DXT5 DDS has `dwFourCC` set
/// and `dwRGBBitCount == 0`, so it is rejected here, exactly as upstream rejects it. No
/// decompression is invented.
fn load_dds_texture(buf: &[u8], black_transparent: bool) -> Result<DecodedTexture, String> {
    if buf.len() < 124 + 4 {
        return Err("not a .dds: file shorter than the 128 byte header".to_string());
    }
    if &buf[0..4] != b"DDS " {
        return Err("not a .dds".to_string());
    }

    let width = le_u32(buf, 0x10);
    let height = le_u32(buf, 0x0C);
    let index = le_u32(buf, 0x58);
    let flag = le_u32(buf, 0x08);
    // Upstream reads the capability flags at 0x70, which is dwCaps2 in the DDS header and not
    // the dwCaps at 0x6C the flag lives in. Kept as written.
    let caps = le_u32(buf, 0x70);

    if index != 16 && index != 32 {
        return Err(format!(
            "DDS: unsupported format, {index} bits per pixel, fourCC {:?}",
            String::from_utf8_lossy(&buf[0x54..0x58])
        ));
    }

    let mut pos = 124 + 4;
    if (flag & 0x0002_0000) != 0 && (caps & 0x0040_0000) != 0 {
        // DDSD_MIPMAPCOUNT plus the mipmap capability flag: "load mipmap information (dummy)",
        // one more 128 byte header's worth of file is skipped.
        pos += 128;
    }

    load_dds_surface(buf, pos, width, height, index, black_transparent)
}

/// The pixel loop of `load_dds_texture`, from a given file position.
fn load_dds_surface(
    buf: &[u8],
    pos: usize,
    width: u32,
    height: u32,
    index: u32,
    black_transparent: bool,
) -> Result<DecodedTexture, String> {
    let mut pos = pos;
    let mut rgba = vec![0u8; rgba_len(width, height)?];
    let bytes_per_pixel = (index / 8) as usize;

    for h in 0..height as usize {
        for w in 0..width as usize {
            let pixel = take(buf, &mut pos, bytes_per_pixel)?;
            let i = (h * width as usize + w) * 4;

            if index == 16 {
                rgba[i] = (pixel[1] << 4) & 0xF0;
                rgba[i + 1] = pixel[0] & 0xF0;
                rgba[i + 2] = (pixel[0] << 4) & 0xF0;
                rgba[i + 3] = pixel[1] & 0xF0;
            } else {
                rgba[i] = pixel[2];
                rgba[i + 1] = pixel[1];
                rgba[i + 2] = pixel[0];
                rgba[i + 3] = pixel[3];
            }

            if black_transparent {
                black_to_transparent(&mut rgba, i);
            }
        }
    }

    Ok(DecodedTexture { width, height, rgba })
}

/// Load a JPEG file.
///
/// Upstream: `bool D3DGraphics::LoadJPEGTexture(const char*, bool, TEXTUREDATA*)` (line 1443,
/// the libjpeg body). libjpeg is asked for its defaults, three colour components, and the
/// bytes are taken as R G B with a solid alpha.
///
/// A grayscale JPEG has one component upstream and the pixel loop reads three bytes per
/// pixel out of a one-byte-per-pixel row, i.e. out of bounds. Here it is widened to grey
/// RGB. CMYK and 16-bit grayscale are rejected rather than misread.
fn load_jpeg_texture(buf: &[u8], black_transparent: bool) -> Result<DecodedTexture, String> {
    let mut decoder = jpeg_decoder::Decoder::new(buf);
    let pixels = decoder.decode().map_err(|e| format!("JPEG: {e}"))?;
    let info = decoder.info().ok_or("JPEG: no image info")?;

    let width = info.width as u32;
    let height = info.height as u32;
    let components = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => 3,
        jpeg_decoder::PixelFormat::L8 => 1,
        other => return Err(format!("JPEG: unsupported pixel format {other:?}")),
    };

    let mut rgba = vec![0u8; rgba_len(width, height)?];
    if pixels.len() < width as usize * height as usize * components {
        return Err("JPEG: decode returned too little pixel data".to_string());
    }

    for h in 0..height as usize {
        for w in 0..width as usize {
            let i = (h * width as usize + w) * 4;
            if components == 3 {
                let s = (h * width as usize + w) * 3;
                rgba[i] = pixels[s];
                rgba[i + 1] = pixels[s + 1];
                rgba[i + 2] = pixels[s + 2];
            } else {
                let v = pixels[h * width as usize + w];
                rgba[i] = v;
                rgba[i + 1] = v;
                rgba[i + 2] = v;
            }
            rgba[i + 3] = 255;

            if black_transparent {
                black_to_transparent(&mut rgba, i);
            }
        }
    }

    Ok(DecodedTexture { width, height, rgba })
}

/// Load a PNG file.
///
/// Upstream: `bool D3DGraphics::LoadPNGTexture(const char*, bool, TEXTUREDATA*)` (line 1512,
/// the libpng body). It rejects interlacing, any bit depth other than 8 and both grayscale
/// colour types, and it takes the palette route for indexed images (the PLTE colours with a
/// solid alpha, the tRNS chunk is not consulted there) and the RGBA route otherwise
/// (`png_set_add_alpha` plus `png_set_tRNS_to_alpha`).
fn load_png_texture(buf: &[u8], black_transparent: bool) -> Result<DecodedTexture, String> {
    let mut decoder = png::Decoder::new(buf);

    let (interlaced, bit_depth, color_type) = {
        let info = decoder.read_header_info().map_err(|e| format!("PNG: {e}"))?;
        (info.interlaced, info.bit_depth, info.color_type)
    };

    if interlaced {
        return Err("PNG: interlacing is not supported".to_string());
    }
    if bit_depth != png::BitDepth::Eight {
        return Err("PNG: bit depth other than 8 is not supported".to_string());
    }
    if color_type == png::ColorType::Grayscale || color_type == png::ColorType::GrayscaleAlpha {
        return Err("PNG: grayscale is not supported".to_string());
    }

    let indexed = color_type == png::ColorType::Indexed;
    if indexed {
        // Read the raw palette indices, as upstream's palette branch does.
        decoder.set_transformations(png::Transformations::IDENTITY);
    } else {
        // RGB and RGBA both come out as 8 bit RGBA, with tRNS resolved to alpha.
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    }

    let mut reader = decoder.read_info().map_err(|e| format!("PNG: {e}"))?;
    let mut raw = vec![0u8; reader.output_buffer_size()];
    let out = reader.next_frame(&mut raw).map_err(|e| format!("PNG: {e}"))?;

    let width = out.width;
    let height = out.height;
    let mut rgba = vec![0u8; rgba_len(width, height)?];
    if raw.len() < out.line_size * height as usize {
        return Err("PNG: decode returned too little pixel data".to_string());
    }

    if indexed {
        let pallet = reader
            .info()
            .palette
            .clone()
            .ok_or("PNG: indexed image without a palette")?;
        for h in 0..height as usize {
            for w in 0..width as usize {
                let index = raw[h * out.line_size + w] as usize * 3;
                let i = (h * width as usize + w) * 4;
                if index + 2 >= pallet.len() {
                    return Err("PNG: palette index out of range".to_string());
                }
                rgba[i] = pallet[index];
                rgba[i + 1] = pallet[index + 1];
                rgba[i + 2] = pallet[index + 2];
                rgba[i + 3] = 255;
                if black_transparent {
                    black_to_transparent(&mut rgba, i);
                }
            }
        }
    } else {
        if out.color_type != png::ColorType::Rgba {
            return Err(format!("PNG: unexpected output colour type {:?}", out.color_type));
        }
        for h in 0..height as usize {
            for w in 0..width as usize {
                let s = h * out.line_size + w * 4;
                let i = (h * width as usize + w) * 4;
                rgba[i] = raw[s];
                rgba[i + 1] = raw[s + 1];
                rgba[i + 2] = raw[s + 2];
                rgba[i + 3] = raw[s + 3];
                if black_transparent {
                    black_to_transparent(&mut rgba, i);
                }
            }
        }
    }

    Ok(DecodedTexture { width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(t: &DecodedTexture, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * t.width + x) * 4) as usize;
        [t.rgba[i], t.rgba[i + 1], t.rgba[i + 2], t.rgba[i + 3]]
    }

    // ---- format detection -------------------------------------------------------------

    #[test]
    fn format_from_extension() {
        assert_eq!(check_texture_file_extension(b"data\\title.dds", 0), FORMAT_DDS);
        assert_eq!(check_texture_file_extension(b"DATA\\TITLE.DDS", 0), FORMAT_DDS);
        assert_eq!(check_texture_file_extension(b"a.bmp", 0), FORMAT_BMP);
        assert_eq!(check_texture_file_extension(b"a.png", 0), FORMAT_PNG);
        assert_eq!(check_texture_file_extension(b"a.jpg", 0), FORMAT_JPEG);
        assert_eq!(check_texture_file_extension(b"a.jpeg", 0), FORMAT_JPEG);
        assert_eq!(check_texture_file_extension(b"a.jpe", 0), FORMAT_JPEG);
        // The last dot wins, and a suffix that is not a known extension falls through to it.
        assert_eq!(check_texture_file_extension(b"a.tar.bmp", 0), FORMAT_BMP);
        // Upstream scans back to index 1, so a leading dot is not an extension.
        assert_eq!(check_texture_file_extension(b".bmp", 7), 7);
        // Unknown suffix: the incoming format survives.
        assert_eq!(check_texture_file_extension(b"a.dds", 3), FORMAT_DDS);
        assert_eq!(check_texture_file_extension(b"a.gif", 3), 3);
        assert_eq!(check_texture_file_extension(b"noext", 5), 5);
        assert_eq!(check_texture_file_extension(b"", 5), 5);
        // Upstream lowercases into a NUL terminated buffer built with strlen().
        assert_eq!(check_texture_file_extension(b"a.dds\0.png", 0), FORMAT_DDS);
    }

    #[test]
    fn format_from_file_header() {
        assert_eq!(check_texture_file_type_flag(b"BM\x00\x00stuff", 0), FORMAT_BMP);
        assert_eq!(check_texture_file_type_flag(b"DDS \x00", 0), FORMAT_DDS);
        assert_eq!(check_texture_file_type_flag(b"\xFF\xD8\xFF\xE0", 0), FORMAT_JPEG);
        assert_eq!(check_texture_file_type_flag(b"\x89PNG\r\n", 0), FORMAT_PNG);
        // Nothing recognised: the incoming format survives.
        assert_eq!(check_texture_file_type_flag(b"XXXX", 2), 2);
        assert_eq!(check_texture_file_type_flag(b"", 4), 4);
        // Shorter than the signature is not a match (upstream reads whatever it can and
        // compares uninitialised bytes; nothing sane can be reproduced from that).
        assert_eq!(check_texture_file_type_flag(b"\x89P", 0), 0);
        assert_eq!(check_texture_file_type_flag(b"D", 0), 0);
    }

    // ---- BMP --------------------------------------------------------------------------

    /// A BITMAPINFOHEADER BMP with a palette of `entries` colours, entry k being (k, k, k).
    /// `rows` is in picture order, top row first, and its rows carry no padding.
    fn bmp_paletted(width: u32, height: i32, bits: u32, rows: &[Vec<u8>]) -> Vec<u8> {
        let entries: u32 = if bits == 4 { 16 } else { 256 };
        let mut f = vec![0u8; 54];
        f[0] = b'B';
        f[1] = b'M';
        let offset = 54 + entries * 4;
        f[2..6].copy_from_slice(&offset.to_le_bytes());
        f[0x0A..0x0E].copy_from_slice(&offset.to_le_bytes());
        f[0x0E..0x12].copy_from_slice(&40u32.to_le_bytes());
        f[0x12..0x16].copy_from_slice(&width.to_le_bytes());
        f[0x16..0x1A].copy_from_slice(&(height as u32).to_le_bytes());
        f[0x1C..0x1E].copy_from_slice(&(bits as u16).to_le_bytes());
        for k in 0..entries {
            f.extend([k as u8, k as u8, k as u8, 0]);
        }

        let row_bytes = if bits == 4 { (width / 2) as usize } else { width as usize };
        let pad = (4 - (row_bytes % 4)) % 4;
        let order: Vec<usize> = if height > 0 {
            (0..height as usize).rev().collect()
        } else {
            (0..height.unsigned_abs() as usize).collect()
        };
        for r in order {
            f.extend(&rows[r]);
            f.extend(std::iter::repeat(0u8).take(pad));
        }
        f
    }

    /// A 24 bit BMP. `rows` is in picture order, top row first, pixels are R G B.
    fn bmp24(width: u32, height: i32, rows: &[Vec<[u8; 3]>]) -> Vec<u8> {
        let mut f = vec![0u8; 54];
        f[0] = b'B';
        f[1] = b'M';
        f[2..6].copy_from_slice(&54u32.to_le_bytes());
        f[0x0A..0x0E].copy_from_slice(&54u32.to_le_bytes());
        f[0x0E..0x12].copy_from_slice(&40u32.to_le_bytes());
        f[0x12..0x16].copy_from_slice(&width.to_le_bytes());
        f[0x16..0x1A].copy_from_slice(&(height as u32).to_le_bytes());
        f[0x1C..0x1E].copy_from_slice(&24u16.to_le_bytes());

        let pad = (4 - (width as usize * 3 % 4)) % 4;
        let order: Vec<usize> = if height > 0 {
            (0..height as usize).rev().collect()
        } else {
            (0..height.unsigned_abs() as usize).collect()
        };
        for r in order {
            for px in &rows[r] {
                f.extend([px[2], px[1], px[0]]);
            }
            f.extend(std::iter::repeat(0u8).take(pad));
        }
        f
    }

    #[test]
    fn bmp8_bottom_up_rows_are_flipped_and_padded() {
        // 5 wide: 5 index bytes plus 3 bytes of padding per row.
        let rows = vec![vec![10, 11, 12, 13, 14], vec![0, 1, 2, 3, 4]];
        let f = bmp_paletted(5, 2, 8, &rows);
        assert_eq!(f.len(), 54 + 256 * 4 + 2 * 8);

        let t = load_bmp_texture(&f, false).unwrap();
        assert_eq!((t.width, t.height), (5, 2));
        // The file stored the bottom row first.
        for (x, index) in [10u8, 11, 12, 13, 14].into_iter().enumerate() {
            assert_eq!(pixel(&t, x as u32, 0), [index, index, index, 255]);
        }
        for (x, index) in [0u8, 1, 2, 3, 4].into_iter().enumerate() {
            assert_eq!(pixel(&t, x as u32, 1), [index, index, index, 255]);
        }
    }

    #[test]
    fn bmp8_top_down_height_is_honoured() {
        // Same picture, written top down: the rows are no longer reversed.
        let rows = vec![vec![10, 11, 12, 13, 14], vec![0, 1, 2, 3, 4]];
        let f = bmp_paletted(5, -2, 8, &rows);
        let t = load_bmp_texture(&f, false).unwrap();
        assert_eq!((t.width, t.height), (5, 2));
        for (x, index) in [10u8, 11, 12, 13, 14].into_iter().enumerate() {
            assert_eq!(pixel(&t, x as u32, 0), [index, index, index, 255]);
        }
        assert_eq!(pixel(&t, 0, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn bmp4_packs_two_pixels_per_byte() {
        // 2 wide: one byte per row, 3 bytes of padding.
        let rows = vec![vec![0x12]];
        let f = bmp_paletted(2, 1, 4, &rows);
        let t = load_bmp_texture(&f, false).unwrap();
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(pixel(&t, 0, 0), [1, 1, 1, 255]);
        assert_eq!(pixel(&t, 1, 0), [2, 2, 2, 255]);
    }

    #[test]
    fn bmp24_bottom_up_with_padding_and_top_down() {
        let rows = vec![
            vec![[1, 2, 3], [4, 5, 6], [7, 8, 9]],
            vec![[10, 11, 12], [13, 14, 15], [16, 17, 18]],
        ];
        // 3 wide: 9 bytes per row plus 3 of padding.
        let bottom_up = bmp24(3, 2, &rows);
        assert_eq!(bottom_up.len(), 54 + 2 * 12);
        let top_down = bmp24(3, -2, &rows);

        for f in [bottom_up, top_down] {
            let t = load_bmp_texture(&f, false).unwrap();
            assert_eq!((t.width, t.height), (3, 2));
            assert_eq!(pixel(&t, 0, 0), [1, 2, 3, 255]);
            assert_eq!(pixel(&t, 1, 0), [4, 5, 6, 255]);
            assert_eq!(pixel(&t, 2, 0), [7, 8, 9, 255]);
            assert_eq!(pixel(&t, 0, 1), [10, 11, 12, 255]);
            assert_eq!(pixel(&t, 1, 1), [13, 14, 15, 255]);
            assert_eq!(pixel(&t, 2, 1), [16, 17, 18, 255]);
        }
    }

    #[test]
    fn bmp_rejects_a_non_bmp_and_an_unknown_depth() {
        assert!(load_bmp_texture(b"not a bmp at all, not even 54 bytes", false).is_err());
        let mut f = bmp24(2, 1, &[vec![[1, 2, 3], [4, 5, 6]]]);
        f[0x1C] = 7; // 7 bits per pixel
        assert!(load_bmp_texture(&f, false).unwrap_err().contains("7 bits"));
    }

    #[test]
    fn black_transparent_only_touches_black_pixels() {
        let rows = vec![vec![0, 1], vec![0, 1]];
        let f = bmp_paletted(2, 2, 8, &rows);

        let plain = load_bmp_texture(&f, false).unwrap();
        assert_eq!(pixel(&plain, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&plain, 1, 0), [1, 1, 1, 255]);

        let keyed = load_bmp_texture(&f, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 1, 0), [1, 1, 1, 255]);
        assert_eq!(pixel(&keyed, 0, 1), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 1, 1), [1, 1, 1, 255]);
    }

    // ---- DDS --------------------------------------------------------------------------

    fn dds_header(width: u32, height: u32, bit_count: u32, four_cc: &[u8; 4]) -> Vec<u8> {
        let mut f = vec![0u8; 128];
        f[0..4].copy_from_slice(b"DDS ");
        f[4..8].copy_from_slice(&124u32.to_le_bytes());
        f[8..12].copy_from_slice(&0x0000_1007u32.to_le_bytes());
        f[12..16].copy_from_slice(&height.to_le_bytes());
        f[16..20].copy_from_slice(&width.to_le_bytes());
        f[76..80].copy_from_slice(&32u32.to_le_bytes()); // ddspf.dwSize
        f[80..84].copy_from_slice(&0x41u32.to_le_bytes()); // DDPF_RGB | DDPF_ALPHAPIXELS
        f[84..88].copy_from_slice(four_cc);
        f[88..92].copy_from_slice(&bit_count.to_le_bytes());
        f
    }

    #[test]
    fn dds32_is_stored_bgra_and_comes_out_rgba() {
        let mut f = dds_header(2, 1, 32, &[0, 0, 0, 0]);
        // R 0x00FF0000, G 0x0000FF00, B 0x000000FF, A 0xFF000000, as the shipped textures
        // have them. The loader ignores these and uses the byte order.
        f[92..96].copy_from_slice(&0x00FF_0000u32.to_le_bytes());
        f[96..100].copy_from_slice(&0x0000_FF00u32.to_le_bytes());
        f[100..104].copy_from_slice(&0x0000_00FFu32.to_le_bytes());
        f[104..108].copy_from_slice(&0xFF00_0000u32.to_le_bytes());
        f.extend([30, 20, 10, 255]); // B G R A
        f.extend([0, 0, 0, 200]);

        let t = load_dds_texture(&f, false).unwrap();
        assert_eq!((t.width, t.height), (2, 1));
        assert_eq!(pixel(&t, 0, 0), [10, 20, 30, 255]);
        assert_eq!(pixel(&t, 1, 0), [0, 0, 0, 200]);

        let keyed = load_dds_texture(&f, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [10, 20, 30, 255]);
        assert_eq!(pixel(&keyed, 1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn dds16_expands_argb4444_by_nibbles() {
        let mut f = dds_header(2, 1, 16, &[0, 0, 0, 0]);
        // char.dds: R 0x0F00, G 0x00F0, B 0x000F, A 0xF000.
        f[92..96].copy_from_slice(&0x0000_0F00u32.to_le_bytes());
        f[96..100].copy_from_slice(&0x0000_00F0u32.to_le_bytes());
        f[100..104].copy_from_slice(&0x0000_000Fu32.to_le_bytes());
        f[104..108].copy_from_slice(&0x0000_F000u32.to_le_bytes());
        f.extend([0x21, 0xF3]); // low byte, high byte
        f.extend([0x00, 0x00]);

        let t = load_dds_texture(&f, false).unwrap();
        // R = high byte << 4, G = low byte, B = low byte << 4, A = high byte.
        assert_eq!(pixel(&t, 0, 0), [0x30, 0x20, 0x10, 0xF0]);
        assert_eq!(pixel(&t, 1, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn dds_rejects_compressed_and_odd_formats() {
        // A DXT1 file: dwFourCC is set and dwRGBBitCount is 0. Upstream only looks at the bit
        // count, so 0 is "unsupported format" and the texture counts as missing.
        let mut dxt1 = dds_header(8, 8, 0, b"DXT1");
        dxt1.extend([0u8; 8]); // one 8 byte DXT1 block
        let e = load_dds_texture(&dxt1, false).unwrap_err();
        assert!(e.contains("0 bits per pixel"), "{e}");

        // DXT5, same story.
        let mut dxt5 = dds_header(8, 8, 0, b"DXT5");
        dxt5.extend([0u8; 16]); // one 16 byte DXT5 block
        assert!(load_dds_texture(&dxt5, false).is_err());

        // 24 bit and 8 bit DDS are equally unsupported.
        assert!(load_dds_texture(&dds_header(2, 2, 24, &[0, 0, 0, 0]), false).is_err());
        assert!(load_dds_texture(&dds_header(2, 2, 8, &[0, 0, 0, 0]), false).is_err());

        // And something that is not a DDS at all.
        assert!(load_dds_texture(b"BMxxxxxxxxxx", false).is_err());
    }

    // ---- PNG --------------------------------------------------------------------------

    fn encode_png(
        width: u32,
        height: u32,
        color: png::ColorType,
        depth: png::BitDepth,
        data: &[u8],
        palette: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(depth);
            if let Some(palette) = palette {
                encoder.set_palette(palette);
            }
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    #[test]
    fn png_rgba_keeps_its_own_alpha() {
        let f = encode_png(2, 1, png::ColorType::Rgba, png::BitDepth::Eight, &[0, 0, 0, 128, 10, 20, 30, 255], None);

        let plain = load_png_texture(&f, false).unwrap();
        assert_eq!((plain.width, plain.height), (2, 1));
        assert_eq!(pixel(&plain, 0, 0), [0, 0, 0, 128]);
        assert_eq!(pixel(&plain, 1, 0), [10, 20, 30, 255]);

        let keyed = load_png_texture(&f, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 1, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn png_rgb_gets_a_solid_alpha() {
        let f = encode_png(2, 1, png::ColorType::Rgb, png::BitDepth::Eight, &[0, 0, 0, 1, 2, 3], None);

        let plain = load_png_texture(&f, false).unwrap();
        assert_eq!(pixel(&plain, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&plain, 1, 0), [1, 2, 3, 255]);

        let keyed = load_png_texture(&f, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 1, 0), [1, 2, 3, 255]);
    }

    #[test]
    fn png_indexed_uses_the_palette() {
        let palette = vec![0, 0, 0, 255, 255, 255, 255, 0, 0];
        let f = encode_png(
            2,
            2,
            png::ColorType::Indexed,
            png::BitDepth::Eight,
            &[0, 1, 2, 0],
            Some(palette),
        );

        let plain = load_png_texture(&f, false).unwrap();
        assert_eq!((plain.width, plain.height), (2, 2));
        assert_eq!(pixel(&plain, 0, 0), [0, 0, 0, 255]);
        assert_eq!(pixel(&plain, 1, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&plain, 0, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&plain, 1, 1), [0, 0, 0, 255]);

        let keyed = load_png_texture(&f, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 1, 0), [255, 255, 255, 255]);
        assert_eq!(pixel(&keyed, 0, 1), [255, 0, 0, 255]);
        assert_eq!(pixel(&keyed, 1, 1), [0, 0, 0, 0]);
    }

    #[test]
    fn png_rejects_grayscale_and_other_bit_depths() {
        let gray = encode_png(2, 1, png::ColorType::Grayscale, png::BitDepth::Eight, &[0, 0], None);
        assert!(load_png_texture(&gray, false).unwrap_err().contains("grayscale"));

        let one_bit = encode_png(2, 1, png::ColorType::Grayscale, png::BitDepth::One, &[0], None);
        assert!(load_png_texture(&one_bit, false)
            .unwrap_err()
            .contains("bit depth"));
    }

    // ---- JPEG -------------------------------------------------------------------------
    //
    // A real encoder is not available in the test build, so the two fixtures are tiny JPEGs
    // made elsewhere. Their pixels were checked against an independent decoder: 8x8, the left
    // half black and the right half white (or mid grey in the grayscale file).

    const JPEG_RGB8: &[u8] = include_bytes!("../tests/fixtures/jpeg_rgb8.jpg");
    const JPEG_GRAY8: &[u8] = include_bytes!("../tests/fixtures/jpeg_gray8.jpg");

    #[test]
    fn jpeg_rgb_keeps_colour_and_gets_a_solid_alpha() {
        let t = load_jpeg_texture(JPEG_RGB8, false).unwrap();
        assert_eq!((t.width, t.height), (8, 8));
        for y in 0..8 {
            for x in 0..8 {
                let level = if x < 4 { 0 } else { 255 };
                assert_eq!(pixel(&t, x, y), [level, level, level, 255]);
            }
        }

        let keyed = load_jpeg_texture(JPEG_RGB8, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 4, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn jpeg_grayscale_is_widened_to_rgb() {
        let t = load_jpeg_texture(JPEG_GRAY8, false).unwrap();
        assert_eq!((t.width, t.height), (8, 8));
        for y in 0..8 {
            for x in 0..8 {
                let level = if x < 4 { 0 } else { 128 };
                assert_eq!(pixel(&t, x, y), [level, level, level, 255]);
            }
        }

        let keyed = load_jpeg_texture(JPEG_GRAY8, true).unwrap();
        assert_eq!(pixel(&keyed, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&keyed, 7, 7), [128, 128, 128, 255]);
    }

    // ---- dispatch ---------------------------------------------------------------------

    #[test]
    fn decode_reads_a_file_and_reports_unknown_formats() {
        let path = std::env::temp_dir().join("xops_texture_decode_test.bmp");
        crate::fs::write_file(&path, &bmp_paletted(1, 1, 8, &[vec![9]])).unwrap();

        let t = decode(&path, FORMAT_BMP, false).unwrap();
        assert_eq!(pixel(&t, 0, 0), [9, 9, 9, 255]);

        assert!(decode(&path, FORMAT_UNKNOWN, false).is_err());
        assert!(decode(&path.join("nope"), FORMAT_BMP, false).is_err());

        std::fs::remove_file(&path).unwrap();
    }
}
