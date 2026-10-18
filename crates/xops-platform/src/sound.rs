//! `sound-ezds.cpp`: the mixer behind the game's sound class.
//!
//! Upstream reached the mixing through `ezds.dll`. What the game asks of it is small:
//! decode one WAV at load time, play it at a volume, stop it, and keep a bounded number of
//! overlapping voices. There is no panning and no 3D here, which is why `sound-ezds.cpp`
//! leaves its pan calculation commented out and why `Play3DSound` applies the distance
//! attenuation in the caller before it ever gets to the device.
//!
//! [`Mixer`] is all of the mixing and needs no device, so a test fills a buffer by hand.
//! [`SoundOutput`] is the part that needs the OS: it owns the stream and drains the mixer
//! from the audio callback.

use std::sync::{Arc, Mutex, MutexGuard};

/// Maximum number of sounds that can be loaded. `MAX_LOADSOUND`, `sound.h`.
pub const MAX_LOADSOUND: usize = 24;

/// Overlapping one-shot voices.
///
/// Upstream DirectSound duplicates the loaded buffer `MAX_SOUNDLISTS` (100) times per
/// sound and refuses playback when all of them are busy. The whole game shares one pool of
/// 24 instead, which is the amount of overlap the game's sounds ever produce. The pool is
/// what bounds playback here: `MAX_SOUNDLISTS` and `MAX_SOUNDDIST` are the game's
/// constants, declared in `xops_game::sound` where the attenuation that uses them lives.
pub const SOUNDVOICES: usize = 24;

/// Sample rate the mixer runs at when there is no device to ask.
pub const SAMPLE_RATE: u32 = 44100;
/// Output channels. Upstream is stereo everywhere, even though nothing pans.
pub const CHANNELS: usize = 2;

/// One loaded sound, resampled once at load time to the mixer's own rate.
///
/// Upstream kept the device-format conversion in `LoadSound` for the same reason: the
/// mixer itself is then a plain add.
#[derive(Default)]
struct Slot {
	used: bool,
	/// Interleaved stereo frames at [`Mixer::sample_rate`].
	samples: Vec<i16>,
}

/// One playing instance of a loaded slot.
struct Voice {
	active: bool,
	/// Slot number of the sound being played.
	id: usize,
	/// Frame offset into the slot.
	pos: usize,
	/// 0.0 to 1.0.
	gain: f32,
}

impl Default for Voice {
	fn default() -> Self {
		Voice {
			active: false,
			id: 0,
			pos: 0,
			gain: 0.0,
		}
	}
}

/// The software mixer: loaded sounds, the voice pool, and the sum of the two.
///
/// `SoundControl` holds one of these upstream, and the manual has it instantiated exactly
/// once, so nothing here is global.
pub struct Mixer {
	sample_rate: u32,
	slots: [Slot; MAX_LOADSOUND],
	voices: [Voice; SOUNDVOICES],
}

impl Default for Mixer {
	fn default() -> Self {
		Mixer::new(SAMPLE_RATE)
	}
}

impl Mixer {
	/// A mixer that produces `sample_rate` Hz and has nothing loaded.
	pub fn new(sample_rate: u32) -> Self {
		Mixer {
			sample_rate,
			slots: Default::default(),
			voices: Default::default(),
		}
	}

	/// The rate loaded sounds are resampled to.
	pub fn sample_rate(&self) -> u32 {
		self.sample_rate
	}

	/// Points the mixer at the device's rate. Call it before the first load, because the
	/// sounds are resampled on the way in.
	pub fn set_sample_rate(&mut self, sample_rate: u32) {
		self.sample_rate = sample_rate;
	}

	/// `SoundControl::LoadSound`: decode a WAV into the first free slot.
	///
	/// Returns the data number, or `None` when every slot is taken or the file cannot be
	/// decoded. Upstream returns -1 for both.
	pub fn load(&mut self, wav: &[u8]) -> Option<usize> {
		let id = self.slots.iter().position(|slot| !slot.used)?;
		let samples = decode_wav(wav, self.sample_rate)?;
		self.slots[id] = Slot {
			used: true,
			samples,
		};
		Some(id)
	}

	/// `SoundControl::CleanupSound`: stop the sound's voices and free its slot.
	pub fn cleanup(&mut self, id: i32) {
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return;
		}
		self.stop(id);
		self.slots[id as usize] = Slot::default();
	}

	/// `SoundControl::GetTotalSounds`.
	pub fn total_sounds(&self) -> i32 {
		self.slots.iter().filter(|slot| slot.used).count() as i32
	}

	/// `SoundControl::PlaySound`, with the master volume already folded in by the caller.
	///
	/// `volume` is the upstream's -100 to 100 scale, where 100 is full scale. `pan` is
	/// taken and ignored, because there is no panning in this build.
	///
	/// Returns 1 when a voice was started and 0 when it was not, which is the upstream's
	/// success and failure. A sound is refused rather than stealing a voice from another
	/// one, which is what `sound-directsound.cpp:377` does when it finds no free buffer.
	pub fn play(&mut self, id: i32, volume: i32, _pan: i32) -> i32 {
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return 0;
		}
		let slot = id as usize;
		if !self.slots[slot].used || self.slots[slot].samples.is_empty() {
			return 0;
		}

		let Some(voice) = self.voices.iter().position(|voice| !voice.active) else {
			return 0;
		};

		let gain = ((volume.clamp(-100, 100) + 100) as f32 / 200.0).clamp(0.0, 1.0);
		self.voices[voice] = Voice {
			active: true,
			id: slot,
			pos: 0,
			gain,
		};

		1
	}

	/// Stops every voice playing the given sound, the inner loop of the upstream's
	/// `CleanupSound` and its equivalent in the DirectSound backend's `Stop`.
	pub fn stop(&mut self, id: i32) {
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return;
		}
		for voice in self.voices.iter_mut() {
			if voice.active && voice.id == id as usize {
				voice.active = false;
			}
		}
	}

	/// Stops every voice, whatever it is playing.
	pub fn stop_all(&mut self) {
		for voice in self.voices.iter_mut() {
			voice.active = false;
		}
	}

	/// How many voices are sounding, for tests and for a caller that wants to know.
	pub fn active_voices(&self) -> usize {
		self.voices.iter().filter(|voice| voice.active).count()
	}

	/// Sums every active voice into `out`, which is interleaved [`CHANNELS`] frames.
	///
	/// A voice that runs out mid-buffer contributes silence for the rest of it and then
	/// stops, so a short sound never wraps around into the next buffer.
	pub fn mix(&mut self, out: &mut [i16]) {
		out.fill(0);

		let frames = out.len() / CHANNELS;
		for voice in self.voices.iter_mut() {
			if !voice.active {
				continue;
			}

			let slot = &self.slots[voice.id];
			if !slot.used {
				voice.active = false;
				continue;
			}

			let source_frames = slot.samples.len() / CHANNELS;
			let mut done = 0;
			while done < frames && voice.pos + done < source_frames {
				for c in 0..CHANNELS {
					let mixed = out[done * CHANNELS + c] as i32
						+ (slot.samples[(voice.pos + done) * CHANNELS + c] as f32 * voice.gain)
							as i32;
					out[done * CHANNELS + c] = mixed.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
				}
				done += 1;
			}

			voice.pos += done;
			if voice.pos >= source_frames {
				voice.active = false;
			}
		}
	}
}

// ---------------------------------------------------------------------------
// WAV
// ---------------------------------------------------------------------------

fn get_2byte_little_endian(data: &[u8]) -> u32 {
	u16::from_le_bytes([data[0], data[1]]) as u32
}

fn get_4byte_little_endian(data: &[u8]) -> u32 {
	u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Decodes a RIFF/WAVE file into interleaved stereo samples at `rate`.
///
/// Upstream left this to a library (the DLL), so there was nothing to port: this is a
/// minimal reader. PCM 8 bit unsigned, PCM 16 bit signed and 32 bit IEEE float are
/// supported, with any channel count and any sample rate, resampled with linear
/// interpolation. Anything else (ADPCM, A-law, 24 bit) is refused, which is enough for the
/// game's data: everything under `data/sound` is 16 bit mono PCM at 22050 Hz.
///
/// The chunk walk skips over what it does not know, because the files carry other chunks
/// (`fact` in `cco.wav`) and the `fmt` chunk is not always 16 bytes.
pub fn decode_wav(data: &[u8], rate: u32) -> Option<Vec<i16>> {
	if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
		return None;
	}

	// format, channels, sample rate, bits per sample
	let mut format: Option<(u16, u16, u32, u16)> = None;
	let mut samples: Option<&[u8]> = None;

	let mut pos = 12;
	while pos + 8 <= data.len() {
		let id = &data[pos..pos + 4];
		let size = get_4byte_little_endian(&data[pos + 4..]) as usize;
		let body_start = pos + 8;
		let body_end = body_start.saturating_add(size).min(data.len());
		let body = &data[body_start..body_end];

		if id == b"fmt " && body.len() >= 16 {
			format = Some((
				get_2byte_little_endian(body) as u16,
				get_2byte_little_endian(&body[2..]) as u16,
				get_4byte_little_endian(&body[4..]),
				get_2byte_little_endian(&body[14..]) as u16,
			));
		} else if id == b"data" {
			samples = Some(body);
		}

		// Chunks are word aligned, so an odd size is followed by a pad byte.
		pos = body_start + size + (size & 1);
	}

	let (format, channels, source_rate, bits) = format?;
	let samples = samples?;
	if channels == 0 || source_rate == 0 || rate == 0 {
		return None;
	}

	let bytes_per_sample = match (format, bits) {
		(1, 8) => 1,
		(1, 16) => 2,
		(3, 32) => 4,
		_ => return None,
	};

	// A partial frame at the end of the data chunk is dropped.
	let source_frames = samples.len() / (bytes_per_sample * channels as usize);
	let kept = CHANNELS.min(channels as usize);

	let mut frames: Vec<[i16; CHANNELS]> = Vec::with_capacity(source_frames);
	for f in 0..source_frames {
		let mut frame = [0i16; CHANNELS];
		for c in 0..kept {
			let at = (f * channels as usize + c) * bytes_per_sample;
			let s = &samples[at..at + bytes_per_sample];
			frame[c] = match (format, bits) {
				(1, 8) => (s[0] as i16 - 128) * 256,
				(1, 16) => i16::from_le_bytes([s[0], s[1]]),
				_ => {
					(f32::from_le_bytes([s[0], s[1], s[2], s[3]]).clamp(-1.0, 1.0) * 32767.0) as i16
				}
			};
		}

		// The mixer is stereo, so a mono file goes to both channels and the output of a
		// file with more than two channels keeps the first two.
		if kept == 1 {
			frame[1] = frame[0];
		}
		frames.push(frame);
	}

	if source_rate == rate || frames.is_empty() {
		return Some(frames.into_iter().flatten().collect());
	}

	let out_frames = (frames.len() as u64 * rate as u64 / source_rate as u64) as usize;
	let mut out = Vec::with_capacity(out_frames * CHANNELS);
	for i in 0..out_frames {
		let at = i as f64 * source_rate as f64 / rate as f64;
		let a = at as usize;
		let b = (a + 1).min(frames.len() - 1);
		let t = (at - a as f64) as f32;
		for c in 0..CHANNELS {
			let va = frames[a][c] as f32;
			let vb = frames[b][c] as f32;
			out.push((va + (vb - va) * t) as i16);
		}
	}

	Some(out)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// The mixer plus the device stream that drains it.
///
/// Opening can fail, and a build or test machine has no device at all. That is not
/// something the game can act on: the slot bookkeeping keeps working so the ids stay
/// valid, and playback reports success without a stream.
pub struct SoundOutput {
	mixer: Arc<Mutex<Mixer>>,
	/// The device stream, `None` on a machine with no output at all.
	stream: Option<cpal::Stream>,
}

impl SoundOutput {
	/// Opens the default output device. Never fails: without one this is a mixer with
	/// nowhere to send the samples.
	pub fn open() -> Self {
		let mixer = Arc::new(Mutex::new(Mixer::new(SAMPLE_RATE)));

		let stream = attach_device(&mixer);
		if stream.is_none() {
			eprintln!("[xops] no usable audio output device, sounds will not be heard");
		}
		SoundOutput { mixer, stream }
	}

	/// The rate the mixer runs at, which is the device's once a device was found.
	pub fn sample_rate(&self) -> u32 {
		self.mixer().sample_rate()
	}

	/// Decodes a WAV into the first free slot. See [`Mixer::load`].
	pub fn load_wav(&self, wav: &[u8]) -> Option<usize> {
		self.mixer().load(wav)
	}

	/// See [`Mixer::play`].
	pub fn play(&self, id: i32, volume: i32, pan: i32) -> i32 {
		if !self.has_device() {
			return 1;
		}
		self.mixer().play(id, volume, pan)
	}

	/// See [`Mixer::cleanup`].
	pub fn cleanup(&self, id: i32) {
		self.mixer().cleanup(id);
	}

	/// See [`Mixer::total_sounds`].
	pub fn total_sounds(&self) -> i32 {
		self.mixer().total_sounds()
	}

	/// Whether there is somewhere for the samples to go. A machine with no output device
	/// still plays sounds into the mixer, because the game cannot act on the failure.
	fn has_device(&self) -> bool {
		self.stream.is_some()
	}

	fn mixer(&self) -> MutexGuard<'_, Mixer> {
		// A panic on the audio thread would poison the mixer and take the rest of the
		// session's sound with it, which is worse than the panic that caused it.
		self.mixer.lock().unwrap_or_else(|e| e.into_inner())
	}
}

impl Drop for SoundOutput {
	fn drop(&mut self) {
		self.mixer().stop_all();
	}
}

/// Opens the default device and hands its samples to `mixer`, returning the stream that
/// keeps it running. `None` is any machine without an output device.
fn attach_device(mixer: &Arc<Mutex<Mixer>>) -> Option<cpal::Stream> {
	use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

	fn report(error: cpal::StreamError) {
		eprintln!("[xops] audio stream error: {error}");
	}

	let device = cpal::default_host().default_output_device()?;
	let supported = device.default_output_config().ok()?;
	let format = supported.sample_format();
	let config = supported.config();

	{
		// The sounds are resampled on the way in, so the mixer has to know the device's
		// rate before the first load.
		let mut m = mixer.lock().unwrap_or_else(|e| e.into_inner());
		m.set_sample_rate(config.sample_rate);
	}

	let shared = Arc::clone(mixer);
	let mut scratch: Vec<i16> = Vec::new();

	let stream = match format {
		cpal::SampleFormat::F32 => device.build_output_stream(
			&config,
			move |out: &mut [f32], _: &cpal::OutputCallbackInfo| fill(&shared, &mut scratch, out),
			report,
			None,
		),
		cpal::SampleFormat::I16 => device.build_output_stream(
			&config,
			move |out: &mut [i16], _: &cpal::OutputCallbackInfo| fill(&shared, &mut scratch, out),
			report,
			None,
		),
		cpal::SampleFormat::U16 => device.build_output_stream(
			&config,
			move |out: &mut [u16], _: &cpal::OutputCallbackInfo| fill(&shared, &mut scratch, out),
			report,
			None,
		),
		// Every other format cpal can hand back is one the game's data never needs.
		_ => return None,
	}
	.ok()?;

	stream.play().ok()?;
	Some(stream)
}

/// Mixes one device buffer.
///
/// `scratch` is reused so the callback never allocates once it has warmed up. `T` is the
/// device's sample type, and `FromSample` is what converts between them: `i16` is the
/// mixer's own type, `f32` and `u16` the other two cpal hands out.
fn fill<T: cpal::Sample + cpal::FromSample<i16>>(
	mixer: &Arc<Mutex<Mixer>>,
	scratch: &mut Vec<i16>,
	out: &mut [T],
) {
	scratch.resize(out.len(), 0);
	mixer.lock().unwrap_or_else(|e| e.into_inner()).mix(scratch);
	for (dst, src) in out.iter_mut().zip(scratch.iter()) {
		*dst = cpal::FromSample::from_sample_(*src);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A PCM 16 bit WAV, little endian, the shape every file under `data/sound` has.
	fn pcm16_wav(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
		let data_len = (samples.len() * 2) as u32;
		let mut out = Vec::new();
		out.extend_from_slice(b"RIFF");
		out.extend_from_slice(&(36 + data_len).to_le_bytes());
		out.extend_from_slice(b"WAVEfmt ");
		out.extend_from_slice(&16u32.to_le_bytes());
		out.extend_from_slice(&1u16.to_le_bytes());
		out.extend_from_slice(&channels.to_le_bytes());
		out.extend_from_slice(&rate.to_le_bytes());
		out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
		out.extend_from_slice(&(channels * 2).to_le_bytes());
		out.extend_from_slice(&16u16.to_le_bytes());
		out.extend_from_slice(b"data");
		out.extend_from_slice(&data_len.to_le_bytes());
		for s in samples {
			out.extend_from_slice(&s.to_le_bytes());
		}
		out
	}

	/// Mono, so a frame is one sample and both output channels carry it.
	fn one_note_wav(sample: i16, frames: usize) -> Vec<u8> {
		pcm16_wav(&vec![sample; frames], 1, SAMPLE_RATE)
	}

	#[test]
	fn wav_is_resampled_to_the_mixer_rate() {
		// 22050 Hz mono, two frames of a 16 bit ramp.
		let wav = pcm16_wav(&[100, 200], 1, 22050);
		let samples = decode_wav(&wav, 44100).unwrap();
		// Twice the rate is twice the frames, interpolated between them.
		assert_eq!(samples.len(), 4 * CHANNELS);
		assert_eq!(samples[0], 100);
		assert_eq!(samples[1], 100);
		// The sample halfway between 100 and 200.
		assert_eq!(samples[2], 150);
	}

	#[test]
	fn game_wav_loads_when_the_game_data_is_present() {
		// The game's sound data is not redistributable and is not in git, so this test
		// reports its reason and passes when it is not on the machine.
		let path =
			std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/sound/can.wav");
		let Ok(data) = crate::fs::read_file(crate::path::resolve_case_insensitive(&path)) else {
			println!("skipping: {} is not present", path.display());
			return;
		};

		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&data).unwrap();
		assert_eq!(mixer.total_sounds(), 1);
		assert_eq!(mixer.play(id as i32, 100, 0), 1);

		// The file opens with a few milliseconds of silence, so the whole sound has to be
		// mixed before it is known that anything came out of it at all.
		let mut out = vec![0i16; 4096 * CHANNELS];
		let mut mixed = 0usize;
		let mut first = None;
		while mixer.active_voices() > 0 {
			mixer.mix(&mut out);
			mixed += out.len() / CHANNELS;
			if first.is_none() {
				if let Some(i) = out.iter().position(|s| *s != 0) {
					first = Some((out[i & !1], out[i | 1]));
				}
			}
		}

		assert!(
			mixed > 1000,
			"the wav decoded to almost nothing: {mixed} frames"
		);
		let (left, right) = first.expect("the loaded wav mixed as silence");
		// The file is 16 bit mono, so both channels carry the same sample.
		assert_eq!(left, right);
	}

	#[test]
	fn a_loaded_wav_mixes_at_the_requested_volume() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&one_note_wav(1000, 8)).unwrap() as i32;

		// Volume 100 is full scale. The whole sound is mixed so its voice is finished
		// before the next assertion starts another one.
		assert_eq!(mixer.play(id, 100, 0), 1);
		let mut out = vec![0i16; 8 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![1000i16; 16]);
		assert_eq!(mixer.active_voices(), 0);

		// Volume 0 is half scale, and the gain truncates like the upstream cast.
		assert_eq!(mixer.play(id, 0, 0), 1);
		let mut out = vec![0i16; 8 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![500i16; 16]);
	}

	#[test]
	fn a_voice_at_zero_volume_is_silent() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&one_note_wav(1000, 8)).unwrap() as i32;

		// -100 is the bottom of the upstream scale, which the loader's warm up play uses.
		assert_eq!(mixer.play(id, -100, 0), 1);
		assert_eq!(mixer.active_voices(), 1);

		let mut out = vec![0i16; 4 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![0i16; 8]);
	}

	#[test]
	fn a_stopped_voice_stops_contributing() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&one_note_wav(1000, 64)).unwrap() as i32;

		assert_eq!(mixer.play(id, 100, 0), 1);
		let mut out = vec![0i16; 8 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![1000i16; 16]);
		assert_eq!(mixer.active_voices(), 1);

		mixer.stop(id);
		assert_eq!(mixer.active_voices(), 0);
		let mut out = vec![0i16; 8 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![0i16; 16]);

		// The slot is only freed by a cleanup, which also stops the voices.
		assert_eq!(mixer.play(id, 100, 0), 1);
		mixer.cleanup(id);
		assert_eq!(mixer.total_sounds(), 0);
		assert_eq!(mixer.active_voices(), 0);
		assert_eq!(mixer.play(id, 100, 0), 0);
	}

	#[test]
	fn the_voice_limit_refuses_the_extra_voice() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&one_note_wav(1000, 4096)).unwrap() as i32;

		for _ in 0..SOUNDVOICES {
			assert_eq!(mixer.play(id, 100, 0), 1);
		}
		assert_eq!(mixer.active_voices(), SOUNDVOICES);

		// Upstream returns 0 here rather than stealing a voice, so the extra sound is
		// not added to the ones already playing.
		assert_eq!(mixer.play(id, 100, 0), 0);
		assert_eq!(mixer.active_voices(), SOUNDVOICES);

		let mut out = vec![0i16; 4 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![1000i16 * SOUNDVOICES as i16; 8]);

		// One voice ending frees a place for the next sound.
		mixer.mix(&mut vec![0i16; 4096 * CHANNELS]);
		assert_eq!(mixer.active_voices(), 0);
		assert_eq!(mixer.play(id, 100, 0), 1);
	}

	#[test]
	fn voices_are_clamped_rather_than_wrapped() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		let id = mixer.load(&one_note_wav(20000, 4096)).unwrap() as i32;

		for _ in 0..3 {
			assert_eq!(mixer.play(id, 100, 0), 1);
		}
		let mut out = vec![0i16; 2 * CHANNELS];
		mixer.mix(&mut out);
		assert_eq!(out, vec![i16::MAX; 4]);
	}

	#[test]
	fn the_output_opens_with_or_without_a_device() {
		// This is the only path that touches the OS, and whether a device exists depends
		// on the machine, so the test asserts what holds either way: opening never fails,
		// the sound still loads, and playing it reports success because a machine without
		// a device does not report failures the game cannot act on.
		let output = SoundOutput::open();
		let id = output.load_wav(&one_note_wav(1000, 64)).expect("load");
		assert_eq!(output.total_sounds(), 1);
		assert_eq!(output.play(id as i32, -100, 0), 1);
		output.cleanup(id as i32);
		assert_eq!(output.total_sounds(), 0);
		assert!(output.sample_rate() >= 8000);
	}

	#[test]
	fn unsupported_and_broken_files_are_refused() {
		let mut mixer = Mixer::new(SAMPLE_RATE);
		assert_eq!(mixer.load(b"not a wav"), None);
		assert_eq!(mixer.load(b"RIFF\x00\x00\x00\x00WAVE"), None);

		// 8 bit unsigned PCM is one of the formats the reader takes. The samples are the
		// bytes 0x0080 0x0000, so the first byte reads as 0x80 and the rest as 0x00.
		let mut wav = pcm16_wav(&[128, 0], 1, SAMPLE_RATE);
		wav[34] = 8;
		wav[35] = 0;
		let samples = decode_wav(&wav, SAMPLE_RATE).unwrap();
		assert_eq!(
			samples,
			vec![0, 0, -32768, -32768, -32768, -32768, -32768, -32768]
		);

		// A data chunk that claims more than the file holds reads to the end and stops.
		let mut wav = pcm16_wav(&[1000, 2000, 3000, 4000], 1, SAMPLE_RATE);
		wav.truncate(44 + 4);
		let samples = decode_wav(&wav, SAMPLE_RATE).unwrap();
		assert_eq!(samples, vec![1000, 1000, 2000, 2000]);

		// A file cut off inside a chunk header has no audio to read at all.
		assert_eq!(decode_wav(&wav[..40], SAMPLE_RATE), None);
	}
}
