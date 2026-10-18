//! `sound.h` and `sound-ezds.cpp`: the class the game calls to play sounds.
//!
//! Upstream this class loaded `ezds.dll` at run time and forwarded every call into it, so
//! the interface is the whole of the port and the mixing lives in
//! [`xops_platform::sound`]. The header also had a DirectSound path (`SOUND_ENGINE 1` and
//! `2`), but the default build and the browser port both take the `ezds` path, so that is
//! the behaviour here: one-shot playback, a volume on the -100 to 100 scale where 100 is
//! full scale, and no panning (the pan calculation in `Play3DSound` is commented out
//! upstream).
//!
//! What is left in this file is what the game looks at: which ids are in use, the camera
//! the distance attenuation is measured from, the master volume, and the two pieces of
//! arithmetic that turn a distance into a volume.

use std::path::Path;

use xops_platform::sound::SoundOutput;

/// Maximum number of sounds that can be loaded, `sound.h`.
pub const MAX_LOADSOUND: usize = 24;
/// Maximum number of sounds that can be played, `sound.h`. The DirectSound backend
/// duplicates each loaded sound this many times, so it is the limit the game was built
/// against even though the mixer keeps one pool for all of them.
pub const MAX_SOUNDLISTS: usize = 100;
/// Maximum distance at which a sound can be played, `sound.h`.
pub const MAX_SOUNDDIST: i32 = 335;
/// Distance limit used for a sound the caller marks as nearby, `CheckSourceDist`.
pub const SOUNDDIST_NEAR: i32 = 30;

/// The sound class.
///
/// The manual has it instantiated exactly once, which is why the browser port put the
/// mixer in file scope statics. Here the mixer is a member and there are no statics.
pub struct SoundControl {
	/// The mixer and the device, absent until [`SoundControl::init_sound`].
	sound: Option<SoundOutput>,
	/// The upstream's `useflag`: which of the `MAX_LOADSOUND` data numbers are in use.
	useflag: [bool; MAX_LOADSOUND],

	/// Camera position and angle, from [`SoundControl::set_camera`].
	camera_x: f32,
	camera_y: f32,
	camera_z: f32,
	camera_rx: f32,
	/// Master volume, 0.0 to 1.0.
	mastervolume: f32,
	/// The master volume on the device's -100 to 100 scale, which is what actually
	/// reaches the mixer.
	volume_max: i32,
}

impl Default for SoundControl {
	fn default() -> Self {
		SoundControl::new()
	}
}

impl SoundControl {
	/// Constructor: nothing is loaded, the camera is at the origin and the volume is full.
	pub fn new() -> SoundControl {
		SoundControl {
			sound: None,
			useflag: [false; MAX_LOADSOUND],
			camera_x: 0.0,
			camera_y: 0.0,
			camera_z: 0.0,
			camera_rx: 0.0,
			mastervolume: 1.0,
			volume_max: 100,
		}
	}

	/// `SoundControl::InitSound`: opens the output.
	///
	/// The upstream takes the window because DirectSound needs it for its cooperative
	/// level. An output stream does not, so there is no parameter.
	///
	/// Success: 0, failure: 1. A machine without an audio device still succeeds, exactly
	/// as the upstream does when the DLL loads but its `DSinit` fails: it returns 0 there
	/// too, with the `return 1` commented out (`sound-ezds.cpp:112`).
	pub fn init_sound(&mut self) -> i32 {
		if self.sound.is_some() {
			return 1;
		}

		self.sound = Some(SoundOutput::open());
		0
	}

	/// `SoundControl::DestroySound`: release every loaded sound and close the output.
	pub fn destroy_sound(&mut self) {
		if self.sound.is_none() {
			return;
		}

		for id in 0..MAX_LOADSOUND as i32 {
			if self.useflag[id as usize] {
				self.cleanup_sound(id);
			}
		}

		self.sound = None;
	}

	/// `SoundControl::SetVolume`, which also derives the device scale.
	///
	/// Anything under 1% is silence, which the upstream does because the derived value
	/// would otherwise be -infinity.
	pub fn set_volume(&mut self, volume: f32) {
		let mut mastervolume = volume;
		if mastervolume < 0.01 {
			mastervolume = 0.0;
		}
		if mastervolume > 1.00 {
			mastervolume = 1.0;
		}
		self.mastervolume = mastervolume;

		// The maximum volume with the decibel curve taken into account.
		if mastervolume == 0.0 {
			self.volume_max = -100;
			return;
		}
		if mastervolume == 1.0 {
			self.volume_max = 100;
			return;
		}
		self.volume_max = 100 - (mastervolume.log10() * -0.2 * 200.0) as i32;
	}

	/// `SoundControl::SetCamera`: the position the distance is measured from. Every frame
	/// it has to be set again, because the attenuation uses the latest one.
	pub fn set_camera(&mut self, x: f32, y: f32, z: f32, rx: f32) {
		self.camera_x = x;
		self.camera_y = y;
		self.camera_z = z;
		self.camera_rx = rx;
	}

	/// `SoundControl::LoadSound`: decode a WAV into the first free data number.
	///
	/// Success: the data number, failure: -1.
	pub fn load_sound(&mut self, path: &Path) -> i32 {
		let Some(sound) = self.sound.as_ref() else {
			return -1;
		};

		let Ok(data) = xops_platform::fs::read_file(path) else {
			return -1;
		};

		// The mixer hands out the same first free number, from the same table size, so
		// the two stay in step without this side picking a slot as well.
		let Some(id) = sound.load_wav(&data) else {
			return -1;
		};

		// Upstream plays the sound once at the lowest volume straight after loading, to
		// prime the DLL's buffer (`sound-ezds.cpp:200`). It is kept: it spends the same
		// one voice, and at -100 the gain is zero.
		sound.play(id as i32, -100, 0);

		self.useflag[id] = true;
		id as i32
	}

	/// `SoundControl::PlaySound`: play a loaded sound with no distance in it.
	///
	/// Used for the sounds the player makes themselves and for anything heard everywhere.
	/// `pan` is taken and ignored: there is no panning in this build.
	///
	/// Success: 1, failure: 0.
	pub fn play_sound(&mut self, id: i32, volume: i32, pan: i32) -> i32 {
		if self.sound.is_none() {
			return 0;
		}
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return 0;
		}
		if !self.useflag[id as usize] {
			return 0;
		}

		let volume = volume.clamp(-100, 100);
		let volume2 = self.scale_volume(volume);

		match self.sound.as_ref() {
			Some(sound) => sound.play(id, volume2, pan),
			None => 0,
		}
	}

	/// `SoundControl::Play3DSound`: play a loaded sound heard from the camera.
	///
	/// Returns 0 without playing when the source is further away than [`MAX_SOUNDDIST`].
	pub fn play_3d_sound(&mut self, id: i32, x: f32, y: f32, z: f32, volume: i32) -> i32 {
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return 0;
		}
		if !self.useflag[id as usize] {
			return 0;
		}

		let volume = volume.clamp(-100, 100);

		let Some(dist) = self.check_source_dist(x, y, z, false) else {
			return 0;
		};
		let playvolume = self.calculation_volume(volume, dist, false);

		self.play_sound(id, playvolume, 0)
	}

	/// `SoundControl::GetTotalSounds`.
	pub fn get_total_sounds(&self) -> i32 {
		self.useflag.iter().filter(|used| **used).count() as i32
	}

	/// `SoundControl::CleanupSound`: stop this sound and release its id.
	pub fn cleanup_sound(&mut self, id: i32) {
		if self.sound.is_none() {
			return;
		}
		if id < 0 || MAX_LOADSOUND as i32 <= id {
			return;
		}
		if !self.useflag[id as usize] {
			return;
		}

		if let Some(sound) = self.sound.as_ref() {
			sound.cleanup(id);
		}
		self.useflag[id as usize] = false;
	}

	/// `SoundControl::CheckSourceDist`: how far the sound is from the camera.
	///
	/// `None` is the upstream's false: the source is out of range, and its `out_dist`
	/// would have been left at 0.0. `snear` measures against the nearby limit instead.
	pub fn check_source_dist(&self, x: f32, y: f32, z: f32, snear: bool) -> Option<f32> {
		let max_dist = if snear { SOUNDDIST_NEAR } else { MAX_SOUNDDIST };

		let dx = self.camera_x - x;
		let dy = self.camera_y - y;
		let dz = self.camera_z - z;
		let dist = dx * dx + dy * dy + dz * dz;
		if dist > (max_dist * max_dist) as f32 {
			return None;
		}

		Some(dist.sqrt())
	}

	/// `SoundControl::CalculationVolume`: linear attenuation from the source to the limit.
	///
	/// Full volume at the camera, zero at `max_dist` and the same line in between, which
	/// is the whole of the game's distance mixing. The caller has already rejected
	/// anything past `max_dist`, so the result only reaches zero at the limit itself.
	pub fn calculation_volume(&self, max_volume: i32, dist: f32, snear: bool) -> i32 {
		let max_dist = if snear { SOUNDDIST_NEAR } else { MAX_SOUNDDIST };

		(max_volume as f32 / max_dist as f32 * dist * -1.0 + max_volume as f32) as i32
	}

	/// The master volume folded into the device's scale, the expression inlined in
	/// `sound-ezds.cpp:236` and again in the browser port (`port/sound-sdl2.cpp:279`).
	///
	/// `volume` is the caller's -100 to 100 and the result is what reaches the mixer. A
	/// separate function only so that it can be checked without a device.
	pub fn scale_volume(&self, volume: i32) -> i32 {
		(1.0 / 200.0 * (volume + 100) as f32 * (self.volume_max + 100) as f32 - 100.0) as i32
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_new_control_has_nothing_loaded_and_full_volume() {
		let sound = SoundControl::new();
		assert_eq!(sound.get_total_sounds(), 0);
		assert_eq!(sound.mastervolume, 1.0);
		assert_eq!(sound.volume_max, 100);
		assert!(sound.sound.is_none());
	}

	#[test]
	fn calls_before_init_sound_fail_like_the_upstream() {
		let mut sound = SoundControl::new();
		// No library, so the upstream returns -1 from LoadSound and 0 from PlaySound.
		assert_eq!(sound.load_sound(Path::new("no-such-file.wav")), -1);
		assert_eq!(sound.play_sound(0, 100, 0), 0);
		assert_eq!(sound.play_3d_sound(0, 0.0, 0.0, 0.0, 100), 0);
		// CleanupSound and DestroySound are no-ops rather than failures.
		sound.cleanup_sound(0);
		sound.destroy_sound();
	}

	#[test]
	fn the_master_volume_derives_the_device_scale() {
		let mut sound = SoundControl::new();

		sound.set_volume(1.0);
		assert_eq!(sound.volume_max, 100);
		sound.set_volume(0.0);
		assert_eq!(sound.volume_max, -100);
		// Anything under 1% is silence, because the curve has no answer for it.
		sound.set_volume(0.009);
		assert_eq!(sound.mastervolume, 0.0);
		assert_eq!(sound.volume_max, -100);
		sound.set_volume(2.0);
		assert_eq!(sound.mastervolume, 1.0);
		assert_eq!(sound.volume_max, 100);

		// log10(0.5) * -0.2 * 200 = 12.04, so half volume is 88 on the device scale.
		sound.set_volume(0.5);
		assert_eq!(sound.mastervolume, 0.5);
		assert_eq!(sound.volume_max, 88);
		// The curve is steep at the bottom: 1% is 20, and 0.8 is 97.
		sound.set_volume(0.01);
		assert_eq!(sound.volume_max, 20);
		sound.set_volume(0.8);
		assert_eq!(sound.volume_max, 97);
	}

	#[test]
	fn a_volume_reaches_the_mixer_through_the_master() {
		let mut sound = SoundControl::new();

		// Full master: the caller's scale passes straight through.
		assert_eq!(sound.scale_volume(100), 100);
		assert_eq!(sound.scale_volume(0), 0);
		assert_eq!(sound.scale_volume(-100), -100);
		// Half the master is 88 on the device scale, so the caller's own volume is scaled
		// by that and not by the 1.0 they asked for.
		sound.set_volume(0.5);
		assert_eq!(sound.scale_volume(100), 88);
		assert_eq!(sound.scale_volume(0), -6);
		// Silent master: nothing gets through.
		sound.set_volume(0.0);
		assert_eq!(sound.scale_volume(100), -100);
	}

	#[test]
	fn distance_decides_whether_a_sound_is_heard_at_all() {
		let mut sound = SoundControl::new();
		sound.set_camera(0.0, 0.0, 0.0, 0.0);

		// Inside the limit, and the distance is the plain 3D distance.
		assert_eq!(sound.check_source_dist(3.0, 4.0, 0.0, false), Some(5.0));
		assert_eq!(sound.check_source_dist(335.0, 0.0, 0.0, false), Some(335.0));
		// One unit past it is out, which is where the sound stops being played.
		assert_eq!(sound.check_source_dist(336.0, 0.0, 0.0, false), None);
		assert_eq!(sound.check_source_dist(0.0, 400.0, 0.0, false), None);

		// A nearby source is measured against 30 instead.
		assert_eq!(sound.check_source_dist(30.0, 0.0, 0.0, true), Some(30.0));
		assert_eq!(sound.check_source_dist(31.0, 0.0, 0.0, true), None);

		// The camera moves, and the distance is from where it moved to.
		sound.set_camera(100.0, 0.0, 0.0, 0.0);
		assert_eq!(sound.check_source_dist(0.0, 0.0, 0.0, false), Some(100.0));
	}

	#[test]
	fn volume_falls_off_linearly_with_distance() {
		let sound = SoundControl::new();

		// At the camera, and at the far end of the range where it reaches zero.
		assert_eq!(sound.calculation_volume(100, 0.0, false), 100);
		assert_eq!(sound.calculation_volume(100, 335.0, false), 0);
		// Halfway along the 335 unit range.
		assert_eq!(sound.calculation_volume(100, 167.5, false), 50);
		// A louder source keeps its proportion, and is still zero at the limit.
		assert_eq!(sound.calculation_volume(200, 167.5, false), 100);
		assert_eq!(sound.calculation_volume(200, 335.0, false), 0);
		assert_eq!(sound.calculation_volume(120, 0.0, false), 120);
		// Past the limit the rule would go negative, which is why the caller rejects the
		// source before asking.
		assert_eq!(sound.calculation_volume(100, 400.0, false), -19);

		// The nearby limit is 30, so a nearby source falls off over 30 units.
		assert_eq!(sound.calculation_volume(100, 15.0, true), 50);
		assert_eq!(sound.calculation_volume(100, 30.0, true), 0);
	}
}
