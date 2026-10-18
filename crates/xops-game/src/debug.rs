//! The debug log, from the upstream `debug.cpp` and `debug.h`.
//!
//! Upstream this is a file scope `DebugLog OutputLog` that every translation unit reaches
//! for. Here the log is a value the caller owns and passes, so the write calls name it.
//!
//! The output is an HTML file because that is what the upstream writes, and it is read by
//! people rather than by the game, so the byte layout of a line is kept as it is.

use crate::core::{GAMENAME, GAMEVERSION};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// A plain note. `LOG_CHECK` upstream.
pub const LOG_CHECK: i32 = 0;
/// The start of something. `LOG_INIT` upstream.
pub const LOG_INIT: i32 = 1;
/// Reading a file. `LOG_LOAD` upstream.
pub const LOG_LOAD: i32 = 2;
/// Releasing something. `LOG_CLEANUP` upstream.
pub const LOG_CLEANUP: i32 = 3;
/// The end of something that started. `LOG_COMPLETE` upstream.
pub const LOG_COMPLETE: i32 = 4;
/// A failure. `LOG_ERROR` upstream.
pub const LOG_ERROR: i32 = 5;

/// The full width space the timestamp cell is padded with, in the Shift-JIS the upstream
/// wrote it in. The log file is bytes, so the bytes are kept rather than decoded.
const SJIS_SPACE: &[u8] = b"\x81\x40";

/// The full width colon in the `ID` line, Shift-JIS again.
const SJIS_COLON: &[u8] = b"\x81\x46";

/// Writes the debug information to a file.
pub struct DebugLog {
	/// Whether output is enabled.
	output_flag: bool,
	/// The file name. The upstream holds it in a `char[MAX_PATH]`.
	fname: PathBuf,
	/// An error is owed to the file on the next write.
	error: bool,
}

impl DebugLog {
	/// The upstream constructor, which names the file from the clock.
	pub fn new() -> Self {
		Self {
			output_flag: false,
			fname: PathBuf::from(format!("{}_log-{}.html", GAMENAME, get_time_name())),
			error: false,
		}
	}

	/// The file name the log writes to.
	///
	/// The upstream has no reader for it, but the caller has to be able to say where the
	/// log went, and the tests check the name the constructor builds.
	pub fn get_log_name(&self) -> &Path {
		&self.fname
	}

	/// Enables output to the log file.
	///
	/// Nothing is written at all until this is called.
	pub fn enable_output_log(&mut self) {
		self.output_flag = true;
	}

	/// Changes the file name of the log file.
	pub fn set_log_name(&mut self, name: impl AsRef<Path>) {
		self.fname = name.as_ref().to_path_buf();
	}

	/// Creates a new log file.
	///
	/// Returns false on success and true on failure, which is the way the upstream reports
	/// it and the way its call sites read it.
	pub fn make_log(&mut self) -> bool {
		if !self.output_flag {
			return true;
		}

		let Ok(mut fp) = File::create(&self.fname) else {
			return true;
		};

		let (year, month, day, hour, minute, second) = civil_from_unix(now_unix());

		let header = format!(
			"<html>\n\
			<head>\n\
			<title>{GAMENAME} {GAMEVERSION} Debug Log</title>\n\
			<style type=text/css>\n\
			<!--//\n\
			body{{ font-size : 12px; }}\n\
			table {{ font-size : 12px; }}\n\
			//-->\n\
			</style>\n\
			</head>\n\
			<body>\n\
			<p>\n\
			<b>{GAMENAME} {GAMEVERSION} Debug Log</b><br>\n\
			Date and time : {year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}<br>\n\
			</p>\n\
			<table>\n\
			<tr><td><b><font size=-2>System time (ms)</font></b></td><td><b>Title</b></td><td colspan=3><b>Message</b></td></tr>\n"
		);

		if fp.write_all(header.as_bytes()).is_err() {
			return true;
		}

		false
	}

	/// Appends an ID to the log file, as the upstream's `WriteLog(int, const char*, int)`.
	///
	/// Returns false on success and true on failure.
	pub fn write_log_id(&mut self, tag: i32, title: &[u8], id: i32) -> bool {
		// The upstream formats into a `char str[16]`, which a long ID overruns. Nothing
		// here is bounded, so the line is always whole.
		let mut text = Vec::new();
		text.extend_from_slice(b"ID");
		text.extend_from_slice(SJIS_COLON);
		text.extend_from_slice(id.to_string().as_bytes());

		self.write_log(tag, title, &text)
	}

	/// Appends to the log file, as the upstream's `WriteLog(int, const char*, const char*)`.
	///
	/// Returns false on success and true on failure.
	///
	/// A completion or a reported error clears the pending error flag. An initialization or
	/// a load sets it, so that the next write reports the error the upstream assumes when
	/// nothing completed the initialization or the load first.
	pub fn write_log(&mut self, tag: i32, title: &[u8], text: &[u8]) -> bool {
		// The upstream checks its title and text pointers for NULL first. A slice cannot
		// be NULL, so nothing is left of those checks.
		if !self.output_flag {
			return true;
		}

		if tag == LOG_COMPLETE || tag == LOG_ERROR {
			self.error = false;
		}

		if self.error {
			self.error = false;
			self.write_log(LOG_ERROR, b"", b"");
		}

		let Ok(mut fp) = OpenOptions::new().append(true).open(&self.fname) else {
			return true;
		};

		if fp.write_all(&log_line(tag, title, text, get_time_ms())).is_err() {
			return true;
		}

		if tag == LOG_INIT || tag == LOG_LOAD {
			self.error = true;
		}

		false
	}
}

impl Drop for DebugLog {
	/// Closes the file, as the upstream destructor does.
	fn drop(&mut self) {
		if !self.output_flag {
			return;
		}

		if let Ok(mut fp) = OpenOptions::new().append(true).open(&self.fname) {
			let _ = fp.write_all(b"</table>\n</body>\n</html>\n");
		}
	}
}

/// One table row, the way the upstream formats it.
///
/// A completion or an error is a row of its own with its title and message in the last two
/// cells, and every other tag puts its title and message in the three columns after it.
fn log_line(tag: i32, title: &[u8], text: &[u8], time_ms: u32) -> Vec<u8> {
	let (color, tagstr): (&[u8], &[u8]) = match tag {
		LOG_CHECK => (b"008000", b"Information"),
		LOG_INIT => (b"FF0080", b"Initialize"),
		LOG_LOAD => (b"0000FF", b"Open-Load"),
		LOG_CLEANUP => (b"FF8000", b"Close-Release"),
		LOG_COMPLETE => (b"008000", b"Success"),
		LOG_ERROR => (b"FF0000", b"Failed"),
		_ => (b"000000", b""),
	};

	let mut line = Vec::new();
	line.extend_from_slice(b"<tr><td>");
	line.extend_from_slice(SJIS_SPACE);
	line.extend_from_slice(time_ms.to_string().as_bytes());
	line.extend_from_slice(SJIS_SPACE);
	line.extend_from_slice(b"</td>");

	let tag_cell = |line: &mut Vec<u8>| {
		line.extend_from_slice(b"<td width=100><b><font color=#");
		line.extend_from_slice(color);
		line.extend_from_slice(b">");
		line.extend_from_slice(tagstr);
		line.extend_from_slice(b"</font></b></td>");
	};

	if tag != LOG_COMPLETE && tag != LOG_ERROR {
		tag_cell(&mut line);
		line.extend_from_slice(b"<td width=100>");
		line.extend_from_slice(title);
		line.extend_from_slice(b"</td><td colspan=3>");
		line.extend_from_slice(text);
		line.extend_from_slice(b"</td>");
	} else {
		line.extend_from_slice(b"<td colspan=2></td>");
		tag_cell(&mut line);
		line.extend_from_slice(b"<td>");
		line.extend_from_slice(title);
		line.extend_from_slice(b"</td><td>");
		line.extend_from_slice(text);
		line.extend_from_slice(b"</td>");
	}

	line.extend_from_slice(b"</tr>\n");
	line
}

/// Milliseconds since the first call, which is what the ported `timeGetTime` counts from.
///
/// The upstream returns `unsigned int`, so the count wraps the same way.
fn get_time_ms() -> u32 {
	static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);
	BASE.elapsed().as_millis() as u32
}

/// Seconds from the epoch, `time(NULL)` upstream.
fn now_unix() -> i64 {
	match SystemTime::now().duration_since(UNIX_EPOCH) {
		Ok(since) => since.as_secs() as i64,
		Err(_) => 0,
	}
}

/// The date and time in the `%04d%02d%02d%02d%02d%02d` the upstream `GetTimeName` writes.
///
/// The upstream takes it from `localtime`, and the standard library has no local time, so
/// this is UTC. The name is only ever a file name, and the digits are the same shape.
fn get_time_name() -> String {
	let (year, month, day, hour, minute, second) = civil_from_unix(now_unix());
	format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
}

/// Breaks seconds from the epoch into a civil date and time, in UTC.
fn civil_from_unix(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
	let days = secs.div_euclid(86_400);
	let rem = secs.rem_euclid(86_400);

	// Days from 1970-01-01 to 0000-03-01, then the 400 year era the day falls in.
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let doe = z.rem_euclid(146_097);
	let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
	let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
	// A year starting in March, so the leap day lands at the end of it.
	let mp = (5 * doy + 2) / 153;
	let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
	let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
	let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };

	(
		year as i32,
		month,
		day,
		(rem / 3600) as u32,
		(rem / 60 % 60) as u32,
		(rem % 60) as u32,
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A path under the temporary directory, unique to the test that asks for it.
	fn temp_path(name: &str) -> String {
		let mut path = std::env::temp_dir();
		path.push(format!("xops-debug-{}-{}.html", name, std::process::id()));
		path.to_string_lossy().into_owned()
	}

	/// An enabled log writing to a temporary file.
	fn temp_log(name: &str) -> DebugLog {
		let path = temp_path(name);
		let _ = std::fs::remove_file(&path);

		let mut log = DebugLog::new();
		log.set_log_name(&path);
		log.enable_output_log();
		assert!(!log.make_log());
		log
	}

	fn contains(haystack: &[u8], needle: &[u8]) -> bool {
		haystack.windows(needle.len()).any(|window| window == needle)
	}

	fn index_of(haystack: &[u8], needle: &[u8]) -> usize {
		haystack
			.windows(needle.len())
			.position(|window| window == needle)
			.unwrap_or_else(|| panic!("{:?} not in the log", String::from_utf8_lossy(needle)))
	}

	#[test]
	fn the_level_constants_keep_their_upstream_values() {
		assert_eq!(LOG_CHECK, 0);
		assert_eq!(LOG_INIT, 1);
		assert_eq!(LOG_LOAD, 2);
		assert_eq!(LOG_CLEANUP, 3);
		assert_eq!(LOG_COMPLETE, 4);
		assert_eq!(LOG_ERROR, 5);
	}

	#[test]
	fn the_constructor_names_the_file_from_the_clock() {
		let name = DebugLog::new().get_log_name().to_string_lossy().into_owned();

		assert!(name.starts_with("OpenXOPS_log-"));
		assert!(name.ends_with(".html"));
		let stamp = &name["OpenXOPS_log-".len()..name.len() - ".html".len()];
		assert_eq!(stamp.len(), 14);
		assert!(stamp.bytes().all(|byte| byte.is_ascii_digit()), "{stamp}");
	}

	#[test]
	fn a_disabled_log_writes_nothing() {
		let path = temp_path("disabled");
		let _ = std::fs::remove_file(&path);

		let mut log = DebugLog::new();
		log.set_log_name(&path);

		// `make_log` reports failure, as the upstream does when the output flag is off.
		assert!(log.make_log());
		assert!(log.write_log(LOG_CHECK, b"Input", b"nothing here"));
		assert!(!std::path::Path::new(&path).exists());
	}

	#[test]
	fn a_written_line_carries_its_tag_title_and_message() {
		let path = temp_path("line");

		{
			let mut log = temp_log("line");
			assert!(!log.write_log(LOG_CHECK, b"BD1", b"data/block.dat"));
		}

		let file = xops_platform::fs::read_file(&path).unwrap();
		let text = String::from_utf8_lossy(&file);

		assert!(text.contains("<b>OpenXOPS 1.080 Debug Log</b><br>\n"));
		let stamp_at = index_of(&file, b"Date and time : ") + "Date and time : ".len();
		let stamp = &file[stamp_at..stamp_at + 19];
		assert!(stamp.starts_with(b"2"), "{}", String::from_utf8_lossy(stamp));
		assert!(stamp[4] == b'-' && stamp[7] == b'-' && stamp[10] == b' ');
		assert!(stamp[13] == b':' && stamp[16] == b':');
		assert!(stamp
			.iter()
			.enumerate()
			.all(|(i, byte)| [4, 7, 10, 13, 16].contains(&i) || byte.is_ascii_digit()));
		assert!(file.starts_with(b"<html>\n<head>\n"));
		assert!(contains(
			&file,
			b"<tr><td><b><font size=-2>System time (ms)</font></b></td>"
		));

		// The row: a full width space, the milliseconds, another full width space.
		let row = index_of(&file, b"<tr><td>\x81\x40");
		let after = &file[row + b"<tr><td>\x81\x40".len()..];
		let digits = after
			.iter()
			.take_while(|byte| byte.is_ascii_digit())
			.count();
		assert!(digits > 0);
		assert_eq!(&after[digits..digits + 7], b"\x81\x40</td>");

		assert!(contains(
			&file,
			b"<td width=100><b><font color=#008000>Information</font></b></td>"
		));
		assert!(contains(
			&file,
			b"<td width=100>BD1</td><td colspan=3>data/block.dat</td>"
		));
		assert!(file.ends_with(b"</tr>\n</table>\n</body>\n</html>\n"));

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn every_tag_has_its_own_colour_and_name() {
		let cases: [(i32, &[u8], &[u8]); 6] = [
			(LOG_CHECK, b"008000", b"Information"),
			(LOG_INIT, b"FF0080", b"Initialize"),
			(LOG_LOAD, b"0000FF", b"Open-Load"),
			(LOG_CLEANUP, b"FF8000", b"Close-Release"),
			(LOG_COMPLETE, b"008000", b"Success"),
			(LOG_ERROR, b"FF0000", b"Failed"),
		];

		for (tag, color, name) in cases {
			let line = log_line(tag, b"T", b"M", 1234);

			let mut expected = Vec::new();
			expected.extend_from_slice(b"<font color=#");
			expected.extend_from_slice(color);
			expected.extend_from_slice(b">");
			expected.extend_from_slice(name);
			expected.extend_from_slice(b"</font>");
			assert!(contains(&line, &expected), "tag {tag}");
		}

		// Anything else the switch does not know about is unlabelled and black.
		let line = log_line(99, b"T", b"M", 1234);
		assert!(contains(&line, b"<font color=#000000></font>"));
	}

	#[test]
	fn the_completion_row_has_its_title_and_message_in_the_last_two_cells() {
		let line = log_line(LOG_COMPLETE, b"", b"", 7);
		assert_eq!(
			line,
			b"<tr><td>\x81\x407\x81\x40</td><td colspan=2></td>\
			<td width=100><b><font color=#008000>Success</font></b></td><td></td><td></td></tr>\n"
				.as_slice()
		);

		let line = log_line(LOG_ERROR, b"", b"", 7);
		assert_eq!(
			line,
			b"<tr><td>\x81\x407\x81\x40</td><td colspan=2></td>\
			<td width=100><b><font color=#FF0000>Failed</font></b></td><td></td><td></td></tr>\n"
				.as_slice()
		);
	}

	#[test]
	fn the_id_overload_writes_the_shift_jis_colon() {
		let path = temp_path("id");

		{
			let mut log = temp_log("id");
			assert!(!log.write_log_id(LOG_COMPLETE, b"Model", 42));
		}

		let file = xops_platform::fs::read_file(&path).unwrap();
		assert!(contains(&file, b"<td>Model</td><td>ID\x81\x4642</td>"));

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn an_unfinished_initialization_is_reported_as_an_error_first() {
		let path = temp_path("pending");

		{
			let mut log = temp_log("pending");
			assert!(!log.write_log(LOG_LOAD, b"BD1", b"data/block.dat"));
			assert!(!log.write_log(LOG_CHECK, b"Directory", b"data"));
		}

		let file = xops_platform::fs::read_file(&path).unwrap();

		// The owed error lands before the write that came after it.
		let load = index_of(&file, b"<td width=100>BD1</td>");
		let failed = index_of(&file, b"Failed");
		let check = index_of(&file, b"<td width=100>Directory</td>");
		assert!(load < failed && failed < check);
		assert!(contains(&file, b"<td></td><td></td></tr>"));

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn a_completion_clears_the_error_that_was_owed() {
		let path = temp_path("cleared");

		{
			let mut log = temp_log("cleared");
			assert!(!log.write_log(LOG_LOAD, b"BD1", b"data/block.dat"));
			assert!(!log.write_log(LOG_COMPLETE, b"", b""));
			assert!(!log.write_log(LOG_CHECK, b"Directory", b"data"));
		}

		let file = xops_platform::fs::read_file(&path).unwrap();
		assert!(!contains(&file, b"Failed"));

		let _ = std::fs::remove_file(&path);
	}

	#[test]
	fn a_log_that_cannot_be_created_reports_failure() {
		let mut log = DebugLog::new();
		log.set_log_name("/xops-does-not-exist/log.html");
		log.enable_output_log();

		assert!(log.make_log());
		// The upstream reports a failed append the same way, as true.
		assert!(log.write_log(LOG_CHECK, b"Input", b"unwritable"));
	}

	#[test]
	fn the_civil_date_matches_the_epoch() {
		assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
		assert_eq!(civil_from_unix(1_700_000_000), (2023, 11, 14, 22, 13, 20));
		// A leap day, and the day before it.
		assert_eq!(civil_from_unix(951_782_400), (2000, 2, 29, 0, 0, 0));
		assert_eq!(civil_from_unix(951_868_800), (2000, 3, 1, 0, 0, 0));
		assert_eq!(civil_from_unix(86_399), (1970, 1, 1, 23, 59, 59));
	}

	#[test]
	fn the_clock_counts_milliseconds_forward() {
		let first = get_time_ms();
		std::thread::sleep(std::time::Duration::from_millis(5));
		let second = get_time_ms();

		assert!(second >= first);
	}
}
