//! The one way the game reaches a file.
//!
//! Every caller goes through here rather than calling `std::fs` directly, so a path is
//! turned into bytes in one place and the loaders have one set of error kinds to match
//! on. The case-insensitive resolution in `path` walks directories through the same
//! module.

use std::io;
use std::path::Path;

/// Reads a whole file.
pub fn read_file(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
	std::fs::read(path)
}

/// Writes a whole file. The configuration file is the only caller.
pub fn write_file(path: impl AsRef<Path>, data: &[u8]) -> io::Result<()> {
	std::fs::write(path, data)
}

/// Whether a path exists.
pub fn exists(path: impl AsRef<Path>) -> bool {
	path.as_ref().exists()
}

/// The names of the entries in a directory, which is what the case-insensitive path
/// resolution walks.
pub fn read_dir_names(path: impl AsRef<Path>) -> io::Result<Vec<String>> {
	let mut names = Vec::new();
	for entry in std::fs::read_dir(path)? {
		names.push(entry?.file_name().to_string_lossy().into_owned());
	}
	Ok(names)
}
