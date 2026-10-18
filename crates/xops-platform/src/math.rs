//! Vector and matrix arithmetic for the fixed-function pipeline.
//!
//! Storage is OpenGL's: column-major, so element (row r, column c) lives at `m[c * 4 + r]`.
//! The game reads its matrices back through `glGetFloatv` and feeds them to
//! `gluUnProject`, so the shape has to stay GL-shaped or the HUD weapon lands in the
//! wrong place.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vec3 {
	pub x: f32,
	pub y: f32,
	pub z: f32,
}

impl Vec3 {
	pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

	pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
		Vec3 { x, y, z }
	}

	pub fn dot(self, o: Vec3) -> f32 {
		self.x * o.x + self.y * o.y + self.z * o.z
	}

	pub fn cross(self, o: Vec3) -> Vec3 {
		Vec3::new(
			self.y * o.z - self.z * o.y,
			self.z * o.x - self.x * o.z,
			self.x * o.y - self.y * o.x,
		)
	}

	pub fn length(self) -> f32 {
		self.dot(self).sqrt()
	}

	/// Returns the vector unchanged when it has no length, which is what `gluLookAt`
	/// does rather than producing a NaN basis.
	pub fn normalized(self) -> Vec3 {
		let len = self.length();
		if len == 0.0 {
			return self;
		}
		self * (1.0 / len)
	}

	pub fn as_array(self) -> [f32; 3] {
		[self.x, self.y, self.z]
	}
}

impl Add for Vec3 {
	type Output = Vec3;
	fn add(self, o: Vec3) -> Vec3 {
		Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
	}
}

impl Sub for Vec3 {
	type Output = Vec3;
	fn sub(self, o: Vec3) -> Vec3 {
		Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
	}
}

impl Neg for Vec3 {
	type Output = Vec3;
	fn neg(self) -> Vec3 {
		Vec3::new(-self.x, -self.y, -self.z)
	}
}

impl Mul<f32> for Vec3 {
	type Output = Vec3;
	fn mul(self, s: f32) -> Vec3 {
		Vec3::new(self.x * s, self.y * s, self.z * s)
	}
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Vec4 {
	pub x: f32,
	pub y: f32,
	pub z: f32,
	pub w: f32,
}

impl Vec4 {
	pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Vec4 {
		Vec4 { x, y, z, w }
	}

	pub fn as_array(self) -> [f32; 4] {
		[self.x, self.y, self.z, self.w]
	}
}

/// A 4x4 matrix in OpenGL's column-major layout.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Mat4 {
	pub m: [f32; 16],
}

impl Default for Mat4 {
	fn default() -> Self {
		Mat4::IDENTITY
	}
}

impl Mat4 {
	pub const IDENTITY: Mat4 = Mat4 {
		m: [
			1.0, 0.0, 0.0, 0.0, //
			0.0, 1.0, 0.0, 0.0, //
			0.0, 0.0, 1.0, 0.0, //
			0.0, 0.0, 0.0, 1.0, //
		],
	};

	/// The matrix product `self * rhs`, which is what `glMultMatrix` computes when it
	/// post-multiplies onto the current matrix.
	pub fn mul(&self, rhs: &Mat4) -> Mat4 {
		let mut out = [0.0f32; 16];
		for col in 0..4 {
			for row in 0..4 {
				let mut sum = 0.0;
				for k in 0..4 {
					sum += self.m[k * 4 + row] * rhs.m[col * 4 + k];
				}
				out[col * 4 + row] = sum;
			}
		}
		Mat4 { m: out }
	}

	pub fn post_mul(&mut self, rhs: &Mat4) {
		*self = self.mul(rhs);
	}

	pub fn translation(x: f32, y: f32, z: f32) -> Mat4 {
		let mut out = Mat4::IDENTITY;
		out.m[12] = x;
		out.m[13] = y;
		out.m[14] = z;
		out
	}

	pub fn scaling(x: f32, y: f32, z: f32) -> Mat4 {
		let mut out = Mat4::IDENTITY;
		out.m[0] = x;
		out.m[5] = y;
		out.m[10] = z;
		out
	}

	pub fn rotation_x(radians: f32) -> Mat4 {
		let (s, c) = radians.sin_cos();
		let mut out = Mat4::IDENTITY;
		out.m[5] = c;
		out.m[6] = s;
		out.m[9] = -s;
		out.m[10] = c;
		out
	}

	pub fn rotation_y(radians: f32) -> Mat4 {
		let (s, c) = radians.sin_cos();
		let mut out = Mat4::IDENTITY;
		out.m[0] = c;
		out.m[2] = -s;
		out.m[8] = s;
		out.m[10] = c;
		out
	}

	pub fn rotation_z(radians: f32) -> Mat4 {
		let (s, c) = radians.sin_cos();
		let mut out = Mat4::IDENTITY;
		out.m[0] = c;
		out.m[1] = s;
		out.m[4] = -s;
		out.m[5] = c;
		out
	}

	/// Transforms a position, treating `w` as 1 and discarding the result's `w`. The
	/// matrices the game builds are affine, so the division would be by 1 anyway.
	pub fn transform_point(&self, v: Vec3) -> Vec3 {
		let out = self.transform_vec4(Vec4::new(v.x, v.y, v.z, 1.0));
		Vec3::new(out.x, out.y, out.z)
	}

	pub fn transform_vec4(&self, v: Vec4) -> Vec4 {
		let a = v.as_array();
		let mut out = [0.0f32; 4];
		for row in 0..4 {
			let mut sum = 0.0;
			for col in 0..4 {
				sum += self.m[col * 4 + row] * a[col];
			}
			out[row] = sum;
		}
		Vec4::new(out[0], out[1], out[2], out[3])
	}

	/// The `glFrustum` matrix.
	pub fn frustum(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
		let mut out = [0.0f32; 16];
		out[0] = 2.0 * near / (right - left);
		out[5] = 2.0 * near / (top - bottom);
		out[8] = (right + left) / (right - left);
		out[9] = (top + bottom) / (top - bottom);
		out[10] = -(far + near) / (far - near);
		out[11] = -1.0;
		out[14] = -2.0 * far * near / (far - near);
		Mat4 { m: out }
	}

	/// `gluPerspective`, which is `glFrustum` with the half-extents worked out from the
	/// vertical field of view in degrees.
	pub fn perspective(fovy_degrees: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
		let fh = (fovy_degrees as f64 / 360.0 * std::f64::consts::PI).tan() as f32 * near;
		let fw = fh * aspect;
		Mat4::frustum(-fw, fw, -fh, fh, near, far)
	}

	/// The rotation and translation part of `gluLookAt` only. The upstream helper
	/// post-multiplies this and then translates by the negated eye, so callers get the
	/// same two steps through [`Mat4::mult_look_at`].
	pub fn look_at_basis(eye: Vec3, center: Vec3, up: Vec3) -> Mat4 {
		let forward = (center - eye).normalized();
		if forward.length() == 0.0 {
			return Mat4::IDENTITY;
		}
		let side = forward.cross(up).normalized();
		if side.length() == 0.0 {
			return Mat4::IDENTITY;
		}
		let up = side.cross(forward);

		// Written out the way glu.cpp builds it: rows are side, up and -forward.
		Mat4 {
			m: [
				side.x, up.x, -forward.x, 0.0, //
				side.y, up.y, -forward.y, 0.0, //
				side.z, up.z, -forward.z, 0.0, //
				0.0, 0.0, 0.0, 1.0, //
			],
		}
	}

	/// `gluLookAt`'s whole effect on the modelview stack: the basis, then the eye
	/// translation.
	pub fn mult_look_at(&mut self, eye: Vec3, center: Vec3, up: Vec3) {
		self.post_mul(&Mat4::look_at_basis(eye, center, up));
		self.post_mul(&Mat4::translation(-eye.x, -eye.y, -eye.z));
	}

	/// The inverse, or `None` when the matrix is singular. Only `gluUnProject` needs
	/// it, which is why it may fail rather than returning a garbage matrix.
	pub fn invert(&self) -> Option<Mat4> {
		let m = &self.m;
		// Treat the storage as row-major here, which transposes it, and transpose back
		// at the end. The cofactor pattern below is written for row-major input.
		let a = [
			m[0], m[4], m[8], m[12], //
			m[1], m[5], m[9], m[13], //
			m[2], m[6], m[10], m[14], //
			m[3], m[7], m[11], m[15], //
		];

		let mut inv = [0.0f32; 16];
		inv[0] = a[5] * a[10] * a[15] - a[5] * a[11] * a[14] - a[9] * a[6] * a[15]
			+ a[9] * a[7] * a[14]
			+ a[13] * a[6] * a[11]
			- a[13] * a[7] * a[10];
		inv[4] = -a[4] * a[10] * a[15] + a[4] * a[11] * a[14] + a[8] * a[6] * a[15]
			- a[8] * a[7] * a[14]
			- a[12] * a[6] * a[11]
			+ a[12] * a[7] * a[10];
		inv[8] = a[4] * a[9] * a[15] - a[4] * a[11] * a[13] - a[8] * a[5] * a[15]
			+ a[8] * a[7] * a[13]
			+ a[12] * a[5] * a[11]
			- a[12] * a[7] * a[9];
		inv[12] = -a[4] * a[9] * a[14] + a[4] * a[10] * a[13] + a[8] * a[5] * a[14]
			- a[8] * a[6] * a[13]
			- a[12] * a[5] * a[10]
			+ a[12] * a[6] * a[9];
		inv[1] = -a[1] * a[10] * a[15] + a[1] * a[11] * a[14] + a[9] * a[2] * a[15]
			- a[9] * a[3] * a[14]
			- a[13] * a[2] * a[11]
			+ a[13] * a[3] * a[10];
		inv[5] = a[0] * a[10] * a[15] - a[0] * a[11] * a[14] - a[8] * a[2] * a[15]
			+ a[8] * a[3] * a[14]
			+ a[12] * a[2] * a[11]
			- a[12] * a[3] * a[10];
		inv[9] = -a[0] * a[9] * a[15] + a[0] * a[11] * a[13] + a[8] * a[1] * a[15]
			- a[8] * a[3] * a[13]
			- a[12] * a[1] * a[11]
			+ a[12] * a[3] * a[9];
		inv[13] = a[0] * a[9] * a[14] - a[0] * a[10] * a[13] - a[8] * a[1] * a[14]
			+ a[8] * a[2] * a[13]
			+ a[12] * a[1] * a[10]
			- a[12] * a[2] * a[9];
		inv[2] = a[1] * a[6] * a[15] - a[1] * a[7] * a[14] - a[5] * a[2] * a[15]
			+ a[5] * a[3] * a[14]
			+ a[13] * a[2] * a[7]
			- a[13] * a[3] * a[6];
		inv[6] = -a[0] * a[6] * a[15] + a[0] * a[7] * a[14] + a[4] * a[2] * a[15]
			- a[4] * a[3] * a[14]
			- a[12] * a[2] * a[7]
			+ a[12] * a[3] * a[6];
		inv[10] = a[0] * a[5] * a[15] - a[0] * a[7] * a[13] - a[4] * a[1] * a[15]
			+ a[4] * a[3] * a[13]
			+ a[12] * a[1] * a[7]
			- a[12] * a[3] * a[5];
		inv[14] = -a[0] * a[5] * a[14] + a[0] * a[6] * a[13] + a[4] * a[1] * a[14]
			- a[4] * a[2] * a[13]
			- a[12] * a[1] * a[6]
			+ a[12] * a[2] * a[5];
		inv[3] = -a[1] * a[6] * a[11] + a[1] * a[7] * a[10] + a[5] * a[2] * a[11]
			- a[5] * a[3] * a[10]
			- a[9] * a[2] * a[7]
			+ a[9] * a[3] * a[6];
		inv[7] = a[0] * a[6] * a[11] - a[0] * a[7] * a[10] - a[4] * a[2] * a[11]
			+ a[4] * a[3] * a[10]
			+ a[8] * a[2] * a[7]
			- a[8] * a[3] * a[6];
		inv[11] = -a[0] * a[5] * a[11] + a[0] * a[7] * a[9] + a[4] * a[1] * a[11]
			- a[4] * a[3] * a[9]
			- a[8] * a[1] * a[7]
			+ a[8] * a[3] * a[5];
		inv[15] = a[0] * a[5] * a[10] - a[0] * a[6] * a[9] - a[4] * a[1] * a[10]
			+ a[4] * a[2] * a[9]
			+ a[8] * a[1] * a[6]
			- a[8] * a[2] * a[5];

		let det = a[0] * inv[0] + a[1] * inv[4] + a[2] * inv[8] + a[3] * inv[12];
		if det == 0.0 {
			return None;
		}
		let scale = 1.0 / det;

		let mut out = [0.0f32; 16];
		for row in 0..4 {
			for col in 0..4 {
				out[col * 4 + row] = inv[row * 4 + col] * scale;
			}
		}
		Some(Mat4 { m: out })
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn identity_multiplies_nothing_away() {
		let t = Mat4::translation(1.0, 2.0, 3.0);
		assert_eq!(t.mul(&Mat4::IDENTITY), t);
		assert_eq!(Mat4::IDENTITY.mul(&t), t);
	}

	#[test]
	fn translation_moves_a_point() {
		let p = Mat4::translation(1.0, 2.0, 3.0).transform_point(Vec3::new(10.0, 0.0, -10.0));
		assert_eq!(p, Vec3::new(11.0, 2.0, -7.0));
	}

	#[test]
	fn post_mul_applies_after() {
		// Scaling then translating must not equal translating then scaling: this is the
		// order GL's stack gives us, and getting it backwards is invisible until a model
		// lands somewhere else.
		let mut s = Mat4::scaling(2.0, 2.0, 2.0);
		s.post_mul(&Mat4::translation(1.0, 0.0, 0.0));
		let moved_then_scaled = s.transform_point(Vec3::new(0.0, 0.0, 0.0));

		let mut t = Mat4::translation(1.0, 0.0, 0.0);
		t.post_mul(&Mat4::scaling(2.0, 2.0, 2.0));
		let scaled_then_moved = t.transform_point(Vec3::new(0.0, 0.0, 0.0));

		assert_eq!(moved_then_scaled, Vec3::new(2.0, 0.0, 0.0));
		assert_eq!(scaled_then_moved, Vec3::new(1.0, 0.0, 0.0));
	}

	#[test]
	fn inverse_round_trips_a_point() {
		let mut m = Mat4::translation(3.0, -4.0, 5.0);
		m.post_mul(&Mat4::rotation_y(0.7));
		m.post_mul(&Mat4::scaling(2.0, 1.0, 0.5));

		let p = Vec3::new(1.5, -2.5, 0.25);
		let there = m.transform_point(p);
		let back = m.invert().expect("invertible").transform_point(there);
		assert!((back.x - p.x).abs() < 1e-4);
		assert!((back.y - p.y).abs() < 1e-4);
		assert!((back.z - p.z).abs() < 1e-4);
	}

	#[test]
	fn singular_has_no_inverse() {
		assert!(Mat4::scaling(1.0, 0.0, 1.0).invert().is_none());
	}

	#[test]
	fn perspective_maps_the_near_plane_to_minus_one() {
		// GL clip space, which the shader remaps on z. This pins the convention: if the
		// matrix ever starts returning zero-to-one the remap becomes a double remap.
		let p = Mat4::perspective(90.0, 1.0, 1.0, 100.0);
		let at_near = p.transform_vec4(Vec4::new(0.0, 0.0, -1.0, 1.0));
		assert!((at_near.z / at_near.w - -1.0).abs() < 1e-5);
	}

	#[test]
	fn look_at_puts_the_eye_at_the_origin_facing_forward() {
		let mut mv = Mat4::IDENTITY;
		mv.mult_look_at(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));
		// The origin should land ten units in front of the camera, which GL writes as
		// negative z in view space.
		let origin = mv.transform_point(Vec3::ZERO);
		assert!((origin.x).abs() < 1e-5);
		assert!((origin.y).abs() < 1e-5);
		assert!((origin.z - -10.0).abs() < 1e-5);
	}

	#[test]
	fn look_at_is_not_transposed() {
		// An oblique camera, because an axis-aligned one cannot tell a look-at matrix from
		// its transpose: every basis vector is of the form (k, 0, 0), so swapping rows for
		// columns changes nothing and a transposed implementation passes. This camera can.
		let eye = Vec3::new(3.0, 4.0, 5.0);
		let center = Vec3::new(-1.0, 0.0, 2.0);
		let up = Vec3::new(0.0, 1.0, 0.0);

		let mut mv = Mat4::IDENTITY;
		mv.mult_look_at(eye, center, up);

		// The eye lands on the origin, which a transposed matrix does not manage.
		let at_eye = mv.transform_point(eye);
		assert!(at_eye.x.abs() < 1e-4, "{at_eye:?}");
		assert!(at_eye.y.abs() < 1e-4, "{at_eye:?}");
		assert!(at_eye.z.abs() < 1e-4, "{at_eye:?}");

		// The target lands straight ahead, down negative z.
		let ahead = mv.transform_point(center);
		assert!(ahead.x.abs() < 1e-4, "{ahead:?}");
		assert!(ahead.y.abs() < 1e-4, "{ahead:?}");
		assert!(ahead.z < 0.0, "the target is in front of the camera: {ahead:?}");

		// And it is a rigid rotation, so nothing is scaled or sheared on the way.
		let origin = mv.transform_point(Vec3::ZERO);
		let x_axis = mv.transform_point(Vec3::new(1.0, 0.0, 0.0)) - origin;
		let y_axis = mv.transform_point(Vec3::new(0.0, 1.0, 0.0)) - origin;
		assert!((x_axis.length() - 1.0).abs() < 1e-4, "{x_axis:?}");
		assert!((y_axis.length() - 1.0).abs() < 1e-4, "{y_axis:?}");
		assert!(x_axis.dot(y_axis).abs() < 1e-4, "the axes stayed perpendicular");
	}
}
