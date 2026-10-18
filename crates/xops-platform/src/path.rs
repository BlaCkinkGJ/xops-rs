//! Finding a file whose name differs from the path only in case.
//!
//! The game was written for Windows, which does not distinguish case, and its data is
//! authored accordingly. Addons are known to reference a texture as `KABE.BMP` when the
//! file on disk is `KABE.bmp`, which works on the platform the game came from and does
//! not work anywhere that a filesystem is case sensitive. The repository's own notes
//! record the case, so this is not a hypothetical.
//!
//! The browser port did not handle this at all. It mapped the backslash separators and
//! stopped there, which means an addon with mixed case would have failed there too.

use std::path::{Component, Path, PathBuf};

/// Returns the path to the file, matching it case insensitively when the exact path does
/// not exist.
///
/// Each component is resolved in turn against its directory, so `data/MAP0/Kabe.BMP`
/// finds `data/map0/kabe.bmp` and also finds a file named `dAtA/mAp0/KaBe.bmp`.
///
/// Returns the original path when nothing matches, so the caller's own error handling and
/// its error message stay the ones it would have produced. An unmatched component is not
/// an error here: this only ever turns a missing path into an existing one.
pub fn resolve_case_insensitive(path: &Path) -> PathBuf {
	if crate::fs::exists(path) {
		return path.to_path_buf();
	}

	let mut resolved = PathBuf::new();
	for component in path.components() {
		match component {
			// A root or a Windows prefix is not a name to match, so it is taken as given.
			Component::Prefix(_) | Component::RootDir | Component::CurDir => {
				resolved.push(component.as_os_str());
				continue;
			}
			Component::ParentDir => {
				resolved.push("..");
				continue;
			}
			Component::Normal(name) => {
				let candidate = resolved.join(name);
				if crate::fs::exists(&candidate) {
					resolved = candidate;
					continue;
				}
				// The exact name is missing, so look for one that differs only in case.
				// `resolved` holds every component before this one, so it *is* the
				// directory the entry should be in. It is empty for the first component of
				// a relative path, which `read_dir` rejects, so that case reads the
				// current directory instead.
				let directory = if resolved.as_os_str().is_empty() {
					PathBuf::from(".")
				} else {
					resolved.clone()
				};
				let wanted = name.to_string_lossy().to_lowercase();
				let found = crate::fs::read_dir_names(&directory)
					.ok()
					.and_then(|names| {
						names
							.into_iter()
							.find(|entry| entry.to_lowercase() == wanted)
					});
				match found {
					Some(actual) => resolved.push(actual),
					// Nothing matches, so the rest of the path cannot be resolved either.
					// The caller gets the path it passed and reports its own failure.
					None => return path.to_path_buf(),
				}
			}
		}
	}

	resolved
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A directory of our own, removed when the test ends.
	struct TempDir(PathBuf);

	impl TempDir {
		fn new(tag: &str) -> TempDir {
			let path = std::env::temp_dir().join(format!("xops-path-{tag}-{}", std::process::id()));
			let _ = std::fs::remove_dir_all(&path);
			std::fs::create_dir_all(&path).expect("create the temp directory");
			TempDir(path)
		}

		fn path(&self) -> &Path {
			&self.0
		}
	}

	impl Drop for TempDir {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	/// Whether the filesystem under `dir` distinguishes case.
	///
	/// This matters because the whole point of the resolver is a filesystem that does, and
	/// the machine this runs on usually does not: macOS ships APFS case insensitive by
	/// default, and Windows never distinguishes. On such a machine `KABE.BMP` really does
	/// open `kabe.bmp`, so there is nothing for the resolver to do and the matching tests
	/// would be asserting against a filesystem that already agreed with them.
	fn filesystem_is_case_sensitive(dir: &Path) -> bool {
		let lower = dir.join("case-probe");
		crate::fs::write_file(&lower, b"x").expect("write the probe");
		let sensitive = !crate::fs::exists(dir.join("CASE-PROBE"));
		let _ = std::fs::remove_file(&lower);
		sensitive
	}

	#[test]
	fn an_exact_path_is_returned_untouched() {
		let dir = TempDir::new("exact");
		let file = dir.path().join("kabe.bmp");
		crate::fs::write_file(&file, b"x").expect("write");
		assert_eq!(resolve_case_insensitive(&file), file);
	}

	#[test]
	fn the_file_name_is_matched_regardless_of_case() {
		// This is the case the repository's own notes record: the data asks for the upper
		// case name and the file is lower case.
		let dir = TempDir::new("name");
		if !filesystem_is_case_sensitive(dir.path()) {
			eprintln!(
				"skipping: this filesystem already ignores case, so there is nothing to resolve"
			);
			return;
		}
		crate::fs::write_file(dir.path().join("kabe.bmp"), b"x").expect("write");

		let resolved = resolve_case_insensitive(&dir.path().join("KABE.BMP"));
		assert_eq!(resolved, dir.path().join("kabe.bmp"));
		assert!(resolved.is_file());
	}

	#[test]
	fn every_component_of_a_path_is_matched() {
		let dir = TempDir::new("components");
		if !filesystem_is_case_sensitive(dir.path()) {
			eprintln!(
				"skipping: this filesystem already ignores case, so there is nothing to resolve"
			);
			return;
		}
		std::fs::create_dir_all(dir.path().join("data/map0")).expect("mkdir");
		crate::fs::write_file(dir.path().join("data/map0/kabe.bmp"), b"x").expect("write");

		let asked = dir.path().join("DATA/Map0/Kabe.BMP");
		let resolved = resolve_case_insensitive(&asked);
		assert_eq!(resolved, dir.path().join("data/map0/kabe.bmp"));
		assert!(resolved.is_file());
	}

	#[test]
	fn the_original_path_comes_back_when_nothing_matches() {
		// The caller has to see its own failure, with the path it asked for in it.
		let dir = TempDir::new("missing");
		let asked = dir.path().join("NOTHING/Here.txt");
		assert_eq!(resolve_case_insensitive(&asked), asked);
	}

	#[test]
	fn a_missing_directory_stops_the_walk_rather_than_panicking() {
		let dir = TempDir::new("halfway");
		std::fs::create_dir_all(dir.path().join("data")).expect("mkdir");
		// `data` resolves, the next component does not.
		let asked = dir.path().join("Nope/File.bmp");
		assert_eq!(resolve_case_insensitive(&asked), asked);
	}

	#[test]
	fn a_relative_path_is_matched_from_the_current_directory() {
		// `data` on its own has an empty parent, which is not a directory that can be
		// read, so this is the case that catches a walk starting in the wrong place. It
		// needs a case-sensitive filesystem and a file whose name it can mismatch.
		if !filesystem_is_case_sensitive(Path::new(".")) {
			eprintln!(
				"skipping: this filesystem already ignores case, so there is nothing to resolve"
			);
			return;
		}
		if !Path::new("Cargo.toml").is_file() {
			eprintln!("skipping: the working directory is not the crate");
			return;
		}
		assert_eq!(
			resolve_case_insensitive(Path::new("CARGO.TOML")),
			PathBuf::from("Cargo.toml"),
			"a relative path resolves against the working directory"
		);
	}

	#[test]
	fn a_case_insensitive_filesystem_is_left_alone() {
		// The complement of the matching tests, and the assertion that holds on the machine
		// this was written on: where case does not matter, the path is already right and
		// the resolver must not rewrite it into a different name.
		let dir = TempDir::new("insensitive");
		if filesystem_is_case_sensitive(dir.path()) {
			eprintln!("skipping: this filesystem distinguishes case, so the other tests cover it");
			return;
		}
		crate::fs::write_file(dir.path().join("kabe.bmp"), b"x").expect("write");

		let asked = dir.path().join("KABE.BMP");
		assert!(
			asked.is_file(),
			"the premise: the filesystem matched it already"
		);
		assert_eq!(resolve_case_insensitive(&asked), asked);
	}
}
