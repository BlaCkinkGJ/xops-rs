//! Collision detection against the map, from the upstream `collision.cpp`.
//!
//! The class answers the geometry questions the objects ask every frame: whether a box
//! or a ray meets the map and where, whether a point is buried inside a block, and the
//! angle between a movement vector and a face, which is what decides whether a slope can
//! be walked. The map is a list of deformed boxes, so every test here is against the
//! block's own faces rather than against its bounding box.
//!
//! One deliberate difference from the upstream: `Collision` does not hold the block data.
//! Upstream keeps a pointer to the interface the game owns, but here the renderer owns
//! it, and holding a borrow of the renderer inside this struct would keep the renderer
//! borrowed for a whole mission. The block data therefore arrives as the first argument
//! of every method that reads it, which is all of them except the ones that only look at
//! the prepared data in `cbdata`. Upstream's `BlockDataIF == NULL` guard becomes the
//! `initialized` flag, set by [`Collision::init_collision`].

use crate::core::angle_normalization;
use crate::datafile::{blockdataface, BlockDataInterface, Blockdata, MAX_BLOCKS};

/// `COLLISION_ADDSIZE`: amount added to the collision detection range.
///
/// Every block is grown by this much along the direction of each vertex, and the AABBs
/// by this much on each side. The objects pass it around as well, so it is public.
pub const COLLISION_ADDSIZE: f32 = 0.01;

// ---------------------------------------------------------------------------
// Human versus map collision constants
// ---------------------------------------------------------------------------

// These live in `object.h` upstream, not in `collision.h`, and every one of them tunes the
// map queries in this module. They are re-exported from the object module rather than
// repeated here, so the tuned values have exactly one definition. Several are documented
// as tuned against the standard SCHOOL map, which is why the odd looking ones stay odd.

pub use crate::object::{
	HUMAN_MAPCOLLISION_ADD_HEIGHT_A, HUMAN_MAPCOLLISION_ADD_HEIGHT_B,
	HUMAN_MAPCOLLISION_CHECK_HEIGHT, HUMAN_MAPCOLLISION_CHECK_MAXDIST,
	HUMAN_MAPCOLLISION_CLIMBADDY, HUMAN_MAPCOLLISION_CLIMBHEIGHT,
	HUMAN_MAPCOLLISION_GROUND_HEIGHT, HUMAN_MAPCOLLISION_GROUND_R1,
	HUMAN_MAPCOLLISION_GROUND_R2, HUMAN_MAPCOLLISION_GROUND_R3, HUMAN_MAPCOLLISION_HEIGHT,
	HUMAN_MAPCOLLISION_R, HUMAN_MAPCOLLISION_SLOPEANGLE,
};

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// `Coll_Blockdata`: the block data the collision detection works on.
///
/// The vertices are the map's own vertices grown outwards from the block center, and
/// the face centers are the average of the four vertices of each face. Both are built by
/// [`Collision::init_collision`], not by the block data loader.
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coll_Blockdata {
	/// Block X coordinate.
	pub x: [f32; 8],
	/// Block Y coordinate.
	pub y: [f32; 8],
	/// Block Z coordinate.
	pub z: [f32; 8],
	/// X coordinate of the center of the block face.
	pub polygon_center_x: [f32; 6],
	/// Y coordinate of the center of the block face.
	pub polygon_center_y: [f32; 6],
	/// Z coordinate of the center of the block face.
	pub polygon_center_z: [f32; 6],
	/// Minimum value of the block X coordinate.
	pub min_x: f32,
	/// Minimum value of the block Y coordinate.
	pub min_y: f32,
	/// Minimum value of the block Z coordinate.
	pub min_z: f32,
	/// Maximum value of the block X coordinate.
	pub max_x: f32,
	/// Maximum value of the block Y coordinate.
	pub max_y: f32,
	/// Maximum value of the block Z coordinate.
	pub max_z: f32,
	/// Whether the block is a flat plane with thickness 0.
	pub board_block: bool,
	/// Spatial subdivision group.
	pub worldgroup: i32,
}

/// `Collision`: the collision detection against the block data used as the map.
pub struct Collision {
	/// Upstream's `BlockDataIF != NULL`. Until `init_collision` runs, every check
	/// answers "no hit", which is what the upstream's null pointer does.
	initialized: bool,
	cbdata: Box<[Coll_Blockdata; MAX_BLOCKS]>,
}

impl Default for Collision {
	fn default() -> Self {
		Self::new()
	}
}

impl Collision {
	/// Constructor.
	pub fn new() -> Collision {
		Collision {
			initialized: false,
			cbdata: Box::new([Coll_Blockdata::default(); MAX_BLOCKS]),
		}
	}

	/// `InitCollision`: prepares the collision data of every block of the map.
	///
	/// Returns 0 on success and 1 when handed no block data, as upstream. The caller
	/// gives the same interface every other method takes.
	pub fn init_collision(&mut self, block_data_if: Option<&BlockDataInterface>) -> i32 {
		let Some(block_data_if) = block_data_if else {
			// Upstream returns here without touching its pointer, and so does this.
			return 1;
		};

		// The loader already caps the count at MAX_BLOCKS, so the clamp only keeps the
		// indexing below in range.
		let bs = block_data_if.get_total_datas().clamp(0, MAX_BLOCKS as i32) as usize;

		for i in 0..bs {
			self.cbdata[i].board_block = false;
		}

		for i in 0..bs {
			let Some(data) = block_data_if.getdata(i as i32) else {
				continue;
			};

			// Find the center.
			let mut cx = 0.0f32;
			let mut cy = 0.0f32;
			let mut cz = 0.0f32;
			for j in 0..8 {
				cx += data.x[j];
				cy += data.y[j];
				cz += data.z[j];
			}
			cx /= 8.0;
			cy /= 8.0;
			cz /= 8.0;

			// Recalculate the coordinates of each vertex.
			for j in 0..8 {
				// Compute the positional relation from the center.
				let x = data.x[j] - cx;
				let y = data.y[j] - cy;
				let z = data.z[j] - cz;
				let rx = z.atan2(x);
				let ry = y.atan2((x * x + z * z).sqrt());
				let mut r = (x * x + y * y + z * z).sqrt();

				// Enlarge.
				r += COLLISION_ADDSIZE;

				// Recalculate the coordinates.
				self.cbdata[i].x[j] = cx + rx.cos() * ry.cos() * r;
				self.cbdata[i].y[j] = cy + ry.sin() * r;
				self.cbdata[i].z[j] = cz + rx.sin() * ry.cos() * r;
			}
		}

		// Compute the center points of the collision planes.
		for i in 0..bs {
			for j in 0..6 {
				let mut v_id = [0i32; 4];
				blockdataface(j as i32, Some(&mut v_id), None);

				let mut x = 0.0f32;
				let mut y = 0.0f32;
				let mut z = 0.0f32;

				// Compute the center point of the 4 vertices.
				for k in 0..4 {
					x += self.cbdata[i].x[v_id[k] as usize];
					y += self.cbdata[i].y[v_id[k] as usize];
					z += self.cbdata[i].z[v_id[k] as usize];
				}
				self.cbdata[i].polygon_center_x[j] = x / 4.0;
				self.cbdata[i].polygon_center_y[j] = y / 4.0;
				self.cbdata[i].polygon_center_z[j] = z / 4.0;
			}
		}

		// Detect plate-like blocks.
		for i in 0..bs {
			self.cbdata[i].board_block = false;

			// If even one pair of vertices is identical, judge the block as plate-like.
			'pairs: for j in 0..8 {
				for k in (j + 1)..8 {
					if (self.cbdata[i].x[j] == self.cbdata[i].x[k])
						&& (self.cbdata[i].y[j] == self.cbdata[i].y[k])
						&& (self.cbdata[i].z[j] == self.cbdata[i].z[k])
					{
						self.cbdata[i].board_block = true;
						break 'pairs;
					}
				}
			}

			// Compute the center of the block.
			let mut mx = 0.0f32;
			let mut my = 0.0f32;
			let mut mz = 0.0f32;
			for j in 0..8 {
				mx += self.cbdata[i].x[j];
				my += self.cbdata[i].y[j];
				mz += self.cbdata[i].z[j];
			}
			mx /= 8.0;
			my /= 8.0;
			mz /= 8.0;

			// If even one of the 6 faces is front-facing relative to the block center,
			// judge the block as plate-like.
			for j in 0..6 {
				if self.check_polygon_front(block_data_if, i as i32, j, mx, my, mz) {
					self.cbdata[i].board_block = true;
					break;
				}
			}
		}

		// Create block AABBs.
		for i in 0..bs {
			let (min_x, min_y, min_z, max_x, max_y, max_z) = block_pos_minmax(&self.cbdata[i]);
			let block = &mut self.cbdata[i];
			block.min_x = min_x;
			block.min_y = min_y;
			block.min_z = min_z;
			block.max_x = max_x;
			block.max_y = max_y;
			block.max_z = max_z;
		}

		// Compute the spatial subdivision group of the block.
		for i in 0..bs {
			let (min_x, min_z, max_x, max_z) = {
				let block = &self.cbdata[i];
				(block.min_x, block.min_z, block.max_x, block.max_z)
			};
			let group = self.get_world_group(min_x, min_z);
			self.cbdata[i].worldgroup = group;
			if self.get_world_group(max_x, max_z) != group {
				self.cbdata[i].worldgroup = 0;
			}
		}

		self.initialized = true;
		0
	}

	/// `GetBlockPosMINMAX`: the minimum and maximum coordinates of the block, as
	/// `(min_x, min_y, min_z, max_x, max_y, max_z)`.
	pub fn get_block_pos_minmax(&self, id: i32) -> Option<(f32, f32, f32, f32, f32, f32)> {
		let id = usize::try_from(id).ok()?;
		Some(block_pos_minmax(self.cbdata.get(id)?))
	}

	/// `GetWorldGroup`: the spatial subdivision group of a coordinate, which is the
	/// quadrant it is in, or 0 on an axis.
	pub fn get_world_group(&self, x: f32, z: f32) -> i32 {
		if (x > 0.0) && (z > 0.0) {
			return 1;
		}
		if (x < 0.0) && (z > 0.0) {
			return 2;
		}
		if (x < 0.0) && (z < 0.0) {
			return 3;
		}
		if (x > 0.0) && (z < 0.0) {
			return 4;
		}
		0
	}

	/// `CheckBlockAABB`: an AABB collision check against one block.
	///
	/// Plate-like blocks are never hit, which is what keeps zero thickness walls out of
	/// the way of the moving objects.
	pub fn check_block_aabb(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		min_x: f32,
		min_y: f32,
		min_z: f32,
		max_x: f32,
		max_y: f32,
		max_z: f32,
	) -> bool {
		if !self.initialized {
			return false;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return false;
		}

		// Plate-like blocks are excluded from the calculation.
		let block = &self.cbdata[id as usize];
		if block.board_block {
			return false;
		}

		collide_box_aabb(
			min_x, min_y, min_z, max_x, max_y, max_z, block.min_x, block.min_y, block.min_z,
			block.max_x, block.max_y, block.max_z,
		)
	}

	/// `CheckALLBlockAABB`: an AABB collision check against all blocks.
	pub fn check_all_block_aabb(
		&self,
		block_data_if: &BlockDataInterface,
		min_x: f32,
		min_y: f32,
		min_z: f32,
		max_x: f32,
		max_y: f32,
		max_z: f32,
	) -> bool {
		if !self.initialized {
			return false;
		}

		for i in 0..block_data_if.get_total_datas() {
			if self.check_block_aabb(block_data_if, i, min_x, min_y, min_z, max_x, max_y, max_z) {
				return true;
			}
		}

		false
	}

	/// `CheckPolygonFront`: whether a coordinate is on the front side of a block face.
	///
	/// The coordinate is absolute, not a vector. The test is the inner product of the
	/// face normal with the vector from the coordinate to the face center, so the answer
	/// is true when that product is not positive.
	pub fn check_polygon_front(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		x: f32,
		y: f32,
		z: f32,
	) -> bool {
		if !self.initialized {
			return false;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return false;
		}
		// Upstream's guard lets face 6 through and then reads past both the face centers
		// and the material, so the length of the face table is the limit here instead.
		if (face < 0) || (6 <= face) {
			return false;
		}

		let bdata = blockdata_or_default(block_data_if, id);
		let block = &self.cbdata[id as usize];
		let face = face as usize;
		let material = &bdata.material[face];

		// Compute the coordinate vector relative to the face center as the reference point.
		let vx = block.polygon_center_x[face] - x;
		let vy = block.polygon_center_y[face] - y;
		let vz = block.polygon_center_z[face] - z;

		// Inner product.
		let d = material.vx * vx + material.vy * vy + material.vz * vz;

		d <= 0.0
	}

	/// `CheckPolygonFrontRx`: whether a direction is on the front side of a block face.
	pub fn check_polygon_front_rx(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		rx: f32,
	) -> bool {
		if !self.initialized {
			return false;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return false;
		}
		if (face < 0) || (6 <= face) {
			return false;
		}

		let bdata = blockdata_or_default(block_data_if, id);

		// Compute the vector.
		let vx = rx.cos();
		let vz = rx.sin();

		// Inner product.
		let d = bdata.material[face as usize].vx * vx + bdata.material[face as usize].vz * vz;

		d <= 0.0
	}

	/// `CheckIntersectTri`: collision check between a block face and a ray, returning the
	/// distance to the hit.
	///
	/// The ray is only tested against the face itself: the four edges of the face are
	/// checked against the face normal, so an intersection point outside the quad is
	/// rejected. Called from the ray checks below, which have already vetted the id.
	fn check_intersect_tri(
		&self,
		block_data_if: &BlockDataInterface,
		blockid: i32,
		face: i32,
		ray_pos_x: f32,
		ray_pos_y: f32,
		ray_pos_z: f32,
		ray_dir_x: f32,
		ray_dir_y: f32,
		ray_dir_z: f32,
	) -> Option<f32> {
		let data = blockdata_or_default(block_data_if, blockid);
		let mut v_id = [0i32; 4];
		blockdataface(face, Some(&mut v_id), None);
		let block = &self.cbdata[blockid as usize];
		let material = &data.material[face as usize];
		let v = [
			v_id[0] as usize,
			v_id[1] as usize,
			v_id[2] as usize,
			v_id[3] as usize,
		];

		// Inner product.
		let d1 = material.vx * ray_dir_x + material.vy * ray_dir_y + material.vz * ray_dir_z;

		if d1 >= 0.0 {
			// The face and ray are parallel, or the ray points away from the face.
			return None;
		}

		// Compute the coordinate vector relative to the face center as the reference point.
		let vx1 = ray_pos_x - block.polygon_center_x[face as usize];
		let vy1 = ray_pos_y - block.polygon_center_y[face as usize];
		let vz1 = ray_pos_z - block.polygon_center_z[face as usize];

		// Inner product, which gives the shortest distance to the face.
		let d2 = material.vx * vx1 + material.vy * vy1 + material.vz * vz1;

		// Distance and coordinates to the intersection point.
		let dist = 1.0 / (d1 * -1.0) * d2;
		let x = ray_pos_x + ray_dir_x * dist;
		let y = ray_pos_y + ray_dir_y * dist;
		let z = ray_pos_z + ray_dir_z * dist;

		// Exclude if the point is not inside the block's overall AABB.
		if (x < block.min_x) || (block.max_x < x) {
			return None;
		}
		if (y < block.min_y) || (block.max_y < y) {
			return None;
		}
		if (z < block.min_z) || (block.max_z < z) {
			return None;
		}

		// Below, check whether the intersection point is inside the block face.
		// Compute the positional relation with each of the 4 edges forming the face, and
		// compare with the face normal. Each edge runs from one vertex of the face to the
		// next in the order the face table gives them, and a cross product that leans
		// away from the normal is on the outside of that edge.
		let edges = [
			(v[0], v[1]),
			(v[1], v[2]),
			(v[2], v[3]),
			(v[3], v[0]),
		];
		for (a, b) in edges {
			// Cross product.
			let vx2 = ((block.y[b] - block.y[a]) * (z - block.z[a]))
				- ((y - block.y[a]) * (block.z[b] - block.z[a]));
			let vy2 = ((block.z[b] - block.z[a]) * (x - block.x[a]))
				- ((z - block.z[a]) * (block.x[b] - block.x[a]));
			let vz2 = ((block.x[b] - block.x[a]) * (y - block.y[a]))
				- ((x - block.x[a]) * (block.y[b] - block.y[a]));

			// Inner product: the relation to the block face normal.
			let d1 = material.vx * vx2 + material.vy * vy2 + material.vz * vz2;

			if d1 < 0.0 {
				// Exclude if outside.
				return None;
			}
		}

		Some(dist)
	}

	/// `CheckBlockInside`: whether a point is buried in a block.
	///
	/// The second half of the return is upstream's `*planeid`, the first face the point
	/// is in front of. That is only meaningful when the point is not buried, because the
	/// buried path finds no front face at all, and it is 0 then.
	///
	/// `worldgroup` enables the spatial subdivision pre-check, which skips the face tests
	/// of blocks on the far side of the map.
	pub fn check_block_inside(
		&self,
		block_data_if: &BlockDataInterface,
		blockid: i32,
		x: f32,
		y: f32,
		z: f32,
		worldgroup: bool,
	) -> (bool, i32) {
		if !self.initialized {
			return (false, 0);
		}
		if (blockid < 0) || (block_data_if.get_total_datas() <= blockid) {
			return (false, 0);
		}

		// Plate-like blocks are excluded from the calculation.
		let block = &self.cbdata[blockid as usize];
		if block.board_block {
			return (false, 0);
		}

		// Rough pre-check.
		if worldgroup {
			// Spatial subdivision.
			if block.worldgroup != 0 {
				// Get the spatial group of the observation point.
				let group = self.get_world_group(x, z);

				if group != 0 {
					// Exclude from calculation if the spatial group differs.
					if block.worldgroup != group {
						return (false, 0);
					}
				}
			}

			// Search within the range.
			if (x < block.min_x) || (block.max_x < x) {
				return (false, 0);
			}
			if (y < block.min_y) || (block.max_y < y) {
				return (false, 0);
			}
			if (z < block.min_z) || (block.max_z < z) {
				return (false, 0);
			}
		}

		// Check whether all 6 faces are back-facing.
		for i in 0..6 {
			if self.check_polygon_front(block_data_if, blockid, i, x, y, z) {
				// If front-facing, stop.
				return (false, i);
			}
		}

		(true, 0)
	}

	/// `CheckALLBlockInside`: whether a point is buried in any block.
	pub fn check_all_block_inside(
		&self,
		block_data_if: &BlockDataInterface,
		x: f32,
		y: f32,
		z: f32,
	) -> bool {
		if !self.initialized {
			return false;
		}

		for i in 0..block_data_if.get_total_datas() {
			if self.check_block_inside(block_data_if, i, x, y, z, true).0 {
				return true;
			}
		}

		false
	}

	/// `CheckBlockIntersectRay`: collision check between a block and a ray.
	///
	/// Returns the face number and the distance to the closest hit. Faces that are
	/// back-facing from the ray start point are ignored, as are plate-like blocks, and a
	/// face that only meets the ray outside `max_dist` is not a hit. `max_dist` below or
	/// equal to 0 disables the limit, which makes the search much slower.
	pub fn check_block_intersect_ray(
		&self,
		block_data_if: &BlockDataInterface,
		blockid: i32,
		ray_pos_x: f32,
		ray_pos_y: f32,
		ray_pos_z: f32,
		ray_dir_x: f32,
		ray_dir_y: f32,
		ray_dir_z: f32,
		max_dist: f32,
	) -> Option<(i32, f32)> {
		if !self.initialized {
			return None;
		}
		if (blockid < 0) || (block_data_if.get_total_datas() <= blockid) {
			return None;
		}
		if (ray_dir_x == 0.0) && (ray_dir_y == 0.0) && (ray_dir_z == 0.0) {
			return None;
		}

		// Plate-like blocks are excluded from the calculation.
		let block = &self.cbdata[blockid as usize];
		if block.board_block {
			return None;
		}

		let mut min_p_dist = f32::MAX;
		let mut min_blockface = -1;

		// Build the ray AABB and get the spatial groups of the start and end points,
		// which is only worth anything when there is a maximum distance.
		let (rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z, worldgroup_a, worldgroup_b) =
			if max_dist > 0.0 {
				let (rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z) = get_aabb_ray(
					ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, max_dist,
				);
				let worldgroup_a = self.get_world_group(ray_pos_x, ray_pos_z);
				let worldgroup_b = self.get_world_group(
					ray_pos_x + ray_dir_x * max_dist,
					ray_pos_z + ray_dir_z * max_dist,
				);
				(
					rmin_x,
					rmin_y,
					rmin_z,
					rmax_x,
					rmax_y,
					rmax_z,
					worldgroup_a,
					worldgroup_b,
				)
			} else {
				(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, 0)
			};

		if max_dist > 0.0 {
			if block.worldgroup != 0 {
				// If the start and end points do not leave the spatial group.
				if (worldgroup_a != 0) && (worldgroup_a == worldgroup_b) {
					// Exclude from calculation if the spatial group differs.
					if block.worldgroup == worldgroup_a {
						// Check between the bounding boxes.
						if !collide_box_aabb(
							block.min_x, block.min_y, block.min_z, block.max_x, block.max_y,
							block.max_z, rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z,
						) {
							return None;
						}
					}
				}
			}
		}

		// Check between the bounding box and the ray.
		if collide_aabb_ray(
			block.min_x, block.min_y, block.min_z, block.max_x, block.max_y, block.max_z,
			ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, -1.0,
		)
		.is_none()
		{
			return None;
		}

		// Check for each polygon.
		for i in 0..6 {
			if self.check_polygon_front(block_data_if, blockid, i, ray_pos_x, ray_pos_y, ray_pos_z) {
				if let Some(p_dist) = self.check_intersect_tri(
					block_data_if,
					blockid,
					i,
					ray_pos_x,
					ray_pos_y,
					ray_pos_z,
					ray_dir_x,
					ray_dir_y,
					ray_dir_z,
				) {
					if min_p_dist > p_dist {
						min_p_dist = p_dist;
						min_blockface = i;
					}
				}
			}
		}

		// If the found distance exceeds the maximum distance, invalidate the result.
		if max_dist >= 0.0 && min_p_dist > max_dist {
			min_blockface = -1;
		}

		if min_blockface == -1 {
			return None;
		}
		Some((min_blockface, min_p_dist))
	}

	/// `CheckALLBlockIntersectRay`: collision check between all blocks and a ray.
	///
	/// Returns the block id, the face number and the distance of the closest hit, which
	/// is the accurate and slower of the two all-block ray checks.
	pub fn check_all_block_intersect_ray(
		&self,
		block_data_if: &BlockDataInterface,
		ray_pos_x: f32,
		ray_pos_y: f32,
		ray_pos_z: f32,
		ray_dir_x: f32,
		ray_dir_y: f32,
		ray_dir_z: f32,
		max_dist: f32,
	) -> Option<(i32, i32, f32)> {
		if !self.initialized {
			return None;
		}
		if (ray_dir_x == 0.0) && (ray_dir_y == 0.0) && (ray_dir_z == 0.0) {
			return None;
		}

		let mut min_p_dist = f32::MAX;
		let mut min_blockid = -1;
		let mut min_blockface = -1;

		let (rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z, worldgroup_a, worldgroup_b) =
			if max_dist > 0.0 {
				// Build the ray AABB.
				let (rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z) = get_aabb_ray(
					ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, max_dist,
				);

				// Get the spatial groups of the start and end points.
				let worldgroup_a = self.get_world_group(ray_pos_x, ray_pos_z);
				let worldgroup_b = self.get_world_group(
					ray_pos_x + ray_dir_x * max_dist,
					ray_pos_z + ray_dir_z * max_dist,
				);
				(
					rmin_x,
					rmin_y,
					rmin_z,
					rmax_x,
					rmax_y,
					rmax_z,
					worldgroup_a,
					worldgroup_b,
				)
			} else {
				(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0, 0)
			};

		for blockid in 0..block_data_if.get_total_datas() {
			let block = &self.cbdata[blockid as usize];

			// Plate-like blocks are excluded from the calculation.
			if block.board_block {
				continue;
			}

			if max_dist > 0.0 {
				if block.worldgroup != 0 {
					// If the start and end points do not leave the spatial group.
					if (worldgroup_a != 0) && (worldgroup_a == worldgroup_b) {
						// Exclude from calculation if the spatial group differs.
						if block.worldgroup == worldgroup_a {
							// Check between the bounding boxes.
							if !collide_box_aabb(
								block.min_x, block.min_y, block.min_z, block.max_x, block.max_y,
								block.max_z, rmin_x, rmin_y, rmin_z, rmax_x, rmax_y, rmax_z,
							) {
								// If not hit, do not examine this block.
								continue;
							}
						}
					}
				}
			}

			// Check between the bounding box and the ray.
			if collide_aabb_ray(
				block.min_x, block.min_y, block.min_z, block.max_x, block.max_y, block.max_z,
				ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, -1.0,
			)
			.is_none()
			{
				// If not hit, do not examine this block.
				continue;
			}

			// Check for each polygon.
			for face in 0..6 {
				if self.check_polygon_front(block_data_if, blockid, face, ray_pos_x, ray_pos_y, ray_pos_z)
				{
					if let Some(p_dist) = self.check_intersect_tri(
						block_data_if,
						blockid,
						face,
						ray_pos_x,
						ray_pos_y,
						ray_pos_z,
						ray_dir_x,
						ray_dir_y,
						ray_dir_z,
					) {
						if min_p_dist > p_dist {
							min_p_dist = p_dist;
							min_blockid = blockid;
							min_blockface = face;
						}
					}
				}
			}
		}

		// If the found distance exceeds the maximum distance, invalidate the result.
		if max_dist >= 0.0 && min_p_dist > max_dist {
			min_blockid = -1;
		}

		if min_blockid == -1 {
			return None;
		}
		Some((min_blockid, min_blockface, min_p_dist))
	}

	/// `CheckALLBlockIntersectDummyRay`: whether a ray meets any block, the fast and
	/// coarse way.
	///
	/// Only three points of the ray are tested for being buried in a block, which is all
	/// the AI needs to know whether a short move is blocked. Upstream also fills a
	/// distance output with 0.0f and keeps one `surface` variable across the three point
	/// checks, so a hit reports the face the start point saw rather than the face that
	/// was entered. The pair returned here is `(block id, that face)`.
	///
	/// Upstream splits into two loops depending on whether the caller wants the id and
	/// the face at all. Both loops return the same hit, and the points are only ever
	/// tested with no side effects, so the start point is checked here when the end point
	/// turns out to be buried, which is the one case that reports its face.
	pub fn check_all_block_intersect_dummy_ray(
		&self,
		block_data_if: &BlockDataInterface,
		ray_pos_x: f32,
		ray_pos_y: f32,
		ray_pos_z: f32,
		ray_dir_x: f32,
		ray_dir_y: f32,
		ray_dir_z: f32,
		max_dist: f32,
	) -> Option<(i32, i32)> {
		if !self.initialized {
			return None;
		}
		if max_dist <= 0.0 {
			return None;
		}

		let ray_pos2_x = ray_pos_x + ray_dir_x * max_dist / 2.0;
		let ray_pos2_y = ray_pos_y + ray_dir_y * max_dist / 2.0;
		let ray_pos2_z = ray_pos_z + ray_dir_z * max_dist / 2.0;
		let ray_pos3_x = ray_pos_x + ray_dir_x * max_dist;
		let ray_pos3_y = ray_pos_y + ray_dir_y * max_dist;
		let ray_pos3_z = ray_pos_z + ray_dir_z * max_dist;

		// Upstream walks the whole array here rather than the number of loaded blocks,
		// and lets the bounds check inside CheckBlockInside reject the rest.
		for i in 0..MAX_BLOCKS as i32 {
			// End point.
			let (end_buried, end_face) = self.check_block_inside(
				block_data_if,
				i,
				ray_pos3_x,
				ray_pos3_y,
				ray_pos3_z,
				true,
			);
			if end_buried {
				// Upstream runs the start point check before this one and reports the
				// face that saw, because a buried point reports no face of its own.
				let (_, surface) = self.check_block_inside(
					block_data_if,
					i,
					ray_pos_x,
					ray_pos_y,
					ray_pos_z,
					false,
				);
				return Some((i, surface));
			}

			// Mid point.
			let (mid_buried, _) = self.check_block_inside(
				block_data_if,
				i,
				ray_pos2_x,
				ray_pos2_y,
				ray_pos2_z,
				true,
			);
			if mid_buried {
				// The end point carries its face over, since it was not buried.
				return Some((i, end_face));
			}
		}

		None
	}

	/// `ScratchVector`: the vector that moves along the block face, which is the input
	/// vector with its component along the face normal removed.
	pub fn scratch_vector(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		in_vx: f32,
		in_vy: f32,
		in_vz: f32,
	) -> Option<(f32, f32, f32)> {
		if !self.initialized {
			return None;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return None;
		}
		if (face < 0) || (6 <= face) {
			return None;
		}

		let bdata = blockdata_or_default(block_data_if, id);
		let material = &bdata.material[face as usize];

		// Inner product.
		let dot = in_vx * material.vx + in_vy * material.vy + in_vz * material.vz;

		Some((
			in_vx - dot * material.vx,
			in_vy - dot * material.vy,
			in_vz - dot * material.vz,
		))
	}

	/// `ReflectVector`: the vector reflected by the block face.
	pub fn reflect_vector(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		in_vx: f32,
		in_vy: f32,
		in_vz: f32,
	) -> Option<(f32, f32, f32)> {
		if !self.initialized {
			return None;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return None;
		}
		if (face < 0) || (6 <= face) {
			return None;
		}

		let bdata = blockdata_or_default(block_data_if, id);
		let material = &bdata.material[face as usize];

		// Inner product.
		let dot = in_vx * material.vx + in_vy * material.vy + in_vz * material.vz;

		Some((
			in_vx - 2.0 * dot * material.vx,
			in_vy - 2.0 * dot * material.vy,
			in_vz - 2.0 * dot * material.vz,
		))
	}

	/// `AngleVector`: the entry angle of a vector relative to the block face.
	///
	/// Front-facing runs from 0.0 to PI/2, back-facing from -0.0 to -PI/2, because the
	/// sign is flipped after the arcsine.
	pub fn angle_vector(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		mut in_vx: f32,
		mut in_vy: f32,
		mut in_vz: f32,
	) -> Option<f32> {
		if !self.initialized {
			return None;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return None;
		}
		if (face < 0) || (6 <= face) {
			return None;
		}
		if (in_vx == 0.0) && (in_vy == 0.0) && (in_vz == 0.0) {
			return None;
		}

		let bdata = blockdata_or_default(block_data_if, id);
		let material = &bdata.material[face as usize];

		vector_normalization(&mut in_vx, &mut in_vy, &mut in_vz);

		// Inner product.
		let dot = in_vx * material.vx + in_vy * material.vy + in_vz * material.vz;

		// Compute the angle.
		Some(dot.asin() * -1.0)
	}

	/// `AngleVector2`: the angle between a vector and the block face, from 0.0 to PI.
	///
	/// Back-facing vectors, which are the ones that can hit the face, are the only ones
	/// this answers for.
	pub fn angle_vector2(
		&self,
		block_data_if: &BlockDataInterface,
		id: i32,
		face: i32,
		in_vx: f32,
		in_vy: f32,
		in_vz: f32,
	) -> Option<f32> {
		if !self.initialized {
			return None;
		}
		if (id < 0) || (block_data_if.get_total_datas() <= id) {
			return None;
		}
		if (face < 0) || (6 <= face) {
			return None;
		}
		if (in_vx == 0.0) && (in_vy == 0.0) && (in_vz == 0.0) {
			return None;
		}

		let bdata = blockdata_or_default(block_data_if, id);
		let material = &bdata.material[face as usize];

		// Inner product.
		let dot = in_vx * material.vx + in_vy * material.vy + in_vz * material.vz;

		// Exclude if back-facing.
		if dot >= 0.0 {
			return None;
		}

		// Compute the angle between the vectors.
		Some(dot.acos())
	}
}

/// The body of `GetBlockPosMINMAX`: the block's own bounds, grown by
/// [`COLLISION_ADDSIZE`] on every side to counter calculation error.
fn block_pos_minmax(block: &Coll_Blockdata) -> (f32, f32, f32, f32, f32, f32) {
	let mut min_x = block.x[0];
	let mut min_y = block.y[0];
	let mut min_z = block.z[0];
	let mut max_x = block.x[0];
	let mut max_y = block.y[0];
	let mut max_z = block.z[0];
	for i in 1..8 {
		if min_x > block.x[i] {
			min_x = block.x[i];
		}
		if min_y > block.y[i] {
			min_y = block.y[i];
		}
		if min_z > block.z[i] {
			min_z = block.z[i];
		}

		if max_x < block.x[i] {
			max_x = block.x[i];
		}
		if max_y < block.y[i] {
			max_y = block.y[i];
		}
		if max_z < block.z[i] {
			max_z = block.z[i];
		}
	}

	(
		min_x - COLLISION_ADDSIZE,
		min_y - COLLISION_ADDSIZE,
		min_z - COLLISION_ADDSIZE,
		max_x + COLLISION_ADDSIZE,
		max_y + COLLISION_ADDSIZE,
		max_z + COLLISION_ADDSIZE,
	)
}

/// The block data of a block whose id every caller here has already range checked, which
/// is why an id the interface does not have reads as the zero block.
fn blockdata_or_default(block_data_if: &BlockDataInterface, id: i32) -> Blockdata {
	block_data_if.getdata(id).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// `GetAABBRay`: the AABB of the ray, as `(min_x, min_y, min_z, max_x, max_y, max_z)`.
///
/// The box is grown by [`COLLISION_ADDSIZE`] on every side, like the block bounds.
pub fn get_aabb_ray(
	ray_pos_x: f32,
	ray_pos_y: f32,
	ray_pos_z: f32,
	ray_dir_x: f32,
	ray_dir_y: f32,
	ray_dir_z: f32,
	max_dist: f32,
) -> (f32, f32, f32, f32, f32, f32) {
	// Build the ray AABB.
	let mut rmin_x = ray_pos_x + ray_dir_x * max_dist;
	let mut rmin_y = ray_pos_y + ray_dir_y * max_dist;
	let mut rmin_z = ray_pos_z + ray_dir_z * max_dist;
	let mut rmax_x = rmin_x;
	let mut rmax_y = rmin_y;
	let mut rmax_z = rmin_z;
	if rmin_x > ray_pos_x {
		rmin_x = ray_pos_x;
	}
	if rmin_y > ray_pos_y {
		rmin_y = ray_pos_y;
	}
	if rmin_z > ray_pos_z {
		rmin_z = ray_pos_z;
	}
	if rmax_x < ray_pos_x {
		rmax_x = ray_pos_x;
	}
	if rmax_y < ray_pos_y {
		rmax_y = ray_pos_y;
	}
	if rmax_z < ray_pos_z {
		rmax_z = ray_pos_z;
	}

	// Make it slightly larger to counter calculation error.
	(
		rmin_x - COLLISION_ADDSIZE,
		rmin_y - COLLISION_ADDSIZE,
		rmin_z - COLLISION_ADDSIZE,
		rmax_x + COLLISION_ADDSIZE,
		rmax_y + COLLISION_ADDSIZE,
		rmax_z + COLLISION_ADDSIZE,
	)
}

/// `CollideBoxAABB`: AABB collision check between two boxes.
///
/// A box with its minimum above its maximum is an error and never collides, and boxes
/// that merely touch do not collide either, since every comparison is strict.
#[allow(clippy::too_many_arguments)]
pub fn collide_box_aabb(
	box1_min_x: f32,
	box1_min_y: f32,
	box1_min_z: f32,
	box1_max_x: f32,
	box1_max_y: f32,
	box1_max_z: f32,
	box2_min_x: f32,
	box2_min_y: f32,
	box2_min_z: f32,
	box2_max_x: f32,
	box2_max_y: f32,
	box2_max_z: f32,
) -> bool {
	// Error handling.
	if box1_min_x > box1_max_x {
		return false;
	}
	if box1_min_y > box1_max_y {
		return false;
	}
	if box1_min_z > box1_max_z {
		return false;
	}
	if box2_min_x > box2_max_x {
		return false;
	}
	if box2_min_y > box2_max_y {
		return false;
	}
	if box2_min_z > box2_max_z {
		return false;
	}

	(box1_min_x < box2_max_x)
		&& (box1_max_x > box2_min_x)
		&& (box1_min_y < box2_max_y)
		&& (box1_max_y > box2_min_y)
		&& (box1_min_z < box2_max_z)
		&& (box1_max_z > box2_min_z)
}

/// `CollideCylinder`: collision check between two cylinders, with the contact angle seen
/// from the first one and the overlap distance, for pushing the two apart.
#[allow(clippy::too_many_arguments)]
pub fn collide_cylinder(
	c1_x: f32,
	c1_y: f32,
	c1_z: f32,
	c1_r: f32,
	c1_h: f32,
	c2_x: f32,
	c2_y: f32,
	c2_z: f32,
	c2_r: f32,
	c2_h: f32,
) -> Option<(f32, f32)> {
	// First, check with a simple height test.
	if (c1_y < c2_y + c2_h) && (c1_y + c1_h > c2_y) {
		// Check by distance.
		let x = c1_x - c2_x;
		let z = c1_z - c2_z;
		let caser = x * x + z * z;
		let minr = (c1_r + c2_r) * (c1_r + c2_r);
		if caser < minr {
			return Some((z.atan2(x), minr.sqrt() - caser.sqrt()));
		}
	}

	None
}

/// `CollideSphereInside`: collision check between a sphere and a point.
///
/// A point exactly on the surface is not inside, because the test is strict.
pub fn collide_sphere_inside(
	s_x: f32,
	s_y: f32,
	s_z: f32,
	s_r: f32,
	p_x: f32,
	p_y: f32,
	p_z: f32,
) -> bool {
	// Compute the distance.
	let x = s_x - p_x;
	let y = s_y - p_y;
	let z = s_z - p_z;
	let d = x * x + y * y + z * z;

	// Distance is closer than the radius.
	d < s_r * s_r
}

/// `CollideSphereRay`: collision check between a sphere and a ray, returning the distance
/// from the ray start point to the sphere surface.
///
/// `max_dist` below or equal to 0 disables the limit. Upstream notes that a ray starting
/// outside the sphere and pointing away from it is not judged correctly, and it is left
/// as it is.
#[allow(clippy::too_many_arguments)]
pub fn collide_sphere_ray(
	s_x: f32,
	s_y: f32,
	s_z: f32,
	s_r: f32,
	ray_pos_x: f32,
	ray_pos_y: f32,
	ray_pos_z: f32,
	ray_dir_x: f32,
	ray_dir_y: f32,
	ray_dir_z: f32,
	max_dist: f32,
) -> Option<f32> {
	if max_dist > 0.0 {
		// Build the ray AABB.
		let (pmin_x, pmin_y, pmin_z, pmax_x, pmax_y, pmax_z) =
			get_aabb_ray(ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, max_dist);

		// Check between the bounding boxes.
		if !collide_box_aabb(
			s_x - s_r,
			s_y - s_r,
			s_z - s_r,
			s_x + s_r,
			s_y + s_r,
			s_z + s_r,
			pmin_x,
			pmin_y,
			pmin_z,
			pmax_x,
			pmax_y,
			pmax_z,
		) {
			return None;
		}
	}

	// Distance between the point and the ray start point.
	let x = s_x - ray_pos_x;
	let y = s_y - ray_pos_y;
	let z = s_z - ray_pos_z;
	let d = (x * x + y * y + z * z).sqrt();

	// The ray start point is closer than the radius (that is, it is embedded).
	if d < s_r {
		return Some(0.0);
	}

	// Compute the shortest distance between the point (sphere center) and the ray.
	let min_dist = distance_pos_ray(
		s_x, s_y, s_z, ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z,
	);

	if min_dist <= s_r {
		let ray_dist = (d * d - min_dist * min_dist).sqrt(); // from the start point
		let r_dist = (s_r * s_r - min_dist * min_dist).sqrt(); // from the point radius

		// Ray-based minimum minus radius minimum is the distance to the surface.
		let dist = ray_dist - r_dist;

		// If farther than the maximum distance to check, treat as not hit.
		if (max_dist > 0.0) && (max_dist < dist) {
			return None;
		}
		return Some(dist);
	}

	None
}

/// `CollideAABBRay`: collision check between an AABB and a ray, returning the distance to
/// the entry point.
///
/// `max_dist` below or equal to 0 disables the limit. The direction is normalized here,
/// so the returned distance is in world units whatever the caller passed.
#[allow(clippy::too_many_arguments)]
pub fn collide_aabb_ray(
	box_min_x: f32,
	box_min_y: f32,
	box_min_z: f32,
	box_max_x: f32,
	box_max_y: f32,
	box_max_z: f32,
	ray_pos_x: f32,
	ray_pos_y: f32,
	ray_pos_z: f32,
	mut ray_dir_x: f32,
	mut ray_dir_y: f32,
	mut ray_dir_z: f32,
	max_dist: f32,
) -> Option<f32> {
	if box_min_x > box_max_x {
		return None;
	}
	if box_min_y > box_max_y {
		return None;
	}
	if box_min_z > box_max_z {
		return None;
	}

	// If the ray start point is inside the AABB, it is already hit.
	if (box_min_x <= ray_pos_x)
		&& (ray_pos_x <= box_max_x)
		&& (box_min_y <= ray_pos_y)
		&& (ray_pos_y <= box_max_y)
		&& (box_min_z <= ray_pos_z)
		&& (ray_pos_z <= box_max_z)
	{
		return Some(0.0);
	}

	if max_dist > 0.0 {
		// Build the ray AABB.
		let (pmin_x, pmin_y, pmin_z, pmax_x, pmax_y, pmax_z) =
			get_aabb_ray(ray_pos_x, ray_pos_y, ray_pos_z, ray_dir_x, ray_dir_y, ray_dir_z, max_dist);

		// Check between the bounding boxes.
		if !collide_box_aabb(
			box_min_x, box_min_y, box_min_z, box_max_x, box_max_y, box_max_z, pmin_x, pmin_y,
			pmin_z, pmax_x, pmax_y, pmax_z,
		) {
			return None;
		}
	}

	// Normalize the vector.
	vector_normalization(&mut ray_dir_x, &mut ray_dir_y, &mut ray_dir_z);

	// Store each coordinate into arrays.
	let box_min = [box_min_x, box_min_y, box_min_z];
	let box_max = [box_max_x, box_max_y, box_max_z];
	let ray_pos = [ray_pos_x, ray_pos_y, ray_pos_z];
	let ray_dir = [ray_dir_x, ray_dir_y, ray_dir_z];
	let mut ray_min = [0.0f32; 3];
	let mut ray_max = [0.0f32; 3];

	// Processing for the 3 axes X, Y, and Z.
	for axis in 0..3 {
		if ((ray_pos[axis] < box_min[axis]) || (box_max[axis] < ray_pos[axis]))
			&& (ray_dir[axis] == 0.0)
		{
			// Not inside, and if the vector direction is 0, it is already outside the AABB.
			return None;
		} else {
			// Not inside, but if the vector has a direction...
			// Get the distance to the intersection point where the vector pierces the AABB.
			let mut t1 = (box_min[axis] - ray_pos[axis]) / ray_dir[axis];
			let mut t2 = (box_max[axis] - ray_pos[axis]) / ray_dir[axis];

			// If the distances to the intersection point are reversed between minimum
			// and maximum, swap them.
			if t1 > t2 {
				std::mem::swap(&mut t1, &mut t2);
			}

			// Record as axis information.
			ray_min[axis] = t1;
			ray_max[axis] = t2;

			if (box_min[axis] <= ray_pos[axis]) && (ray_pos[axis] <= box_max[axis]) {
				// If inside, record it as axis information as is.
				ray_min[axis] = 0.0;
			}

			// If the distance is negative (that is, the vector points backwards), it does
			// not hit the AABB.
			if ray_min[axis] < 0.0 {
				return None;
			}
			if ray_max[axis] < 0.0 {
				return None;
			}
		}
	}

	// For each axis, compute the farthest 'minimum distance' and the nearest 'maximum
	// distance'.
	let mut ray_tmin = ray_min[0];
	let mut ray_tmax = ray_max[0];
	if ray_tmin < ray_min[1] {
		ray_tmin = ray_min[1];
	}
	if ray_tmax > ray_max[1] {
		ray_tmax = ray_max[1];
	}
	if ray_tmin < ray_min[2] {
		ray_tmin = ray_min[2];
	}
	if ray_tmax > ray_max[2] {
		ray_tmax = ray_max[2];
	}

	// If the relation between the minimum and maximum distances is valid...
	if (ray_tmax - ray_tmin) > 0.0 {
		// If farther than the maximum distance to check, the result is invalid.
		if max_dist > 0.0 && ray_tmin > max_dist {
			return None;
		}

		return Some(ray_tmin);
	}

	None
}

/// `CollideCylinderInside`: collision check between a cylinder and a point.
///
/// The circle on the X-Z plane is inclusive while the height is exclusive of nothing:
/// both ends of the height belong to the cylinder, unlike the sphere's strict test.
pub fn collide_cylinder_inside(
	c_x: f32,
	c_y: f32,
	c_z: f32,
	c_r: f32,
	c_h: f32,
	p_x: f32,
	p_y: f32,
	p_z: f32,
) -> bool {
	// Process as a circle on the X-Z plane.
	// Compute the distance.
	let x = c_x - p_x;
	let z = c_z - p_z;
	let d = x * x + z * z;

	// If the distance is farther than the radius, it does not hit.
	if d > c_r * c_r {
		return false;
	}

	// Processing only for the Y axis, the same as AABB.
	if (c_y <= p_y) && (p_y <= (c_y + c_h)) {
		// If it is also inside on the Y axis, it hits.
		return true;
	}

	false
}

/// `CollideCylinderRay`: collision check between a cylinder and a ray, returning the
/// distance to the entry point.
///
/// The cylinder stands vertically along the Y axis and cannot be tilted. Upstream
/// computes the ray end point with the X component of the direction for both X and Z,
/// which is left alone here so the hit distances stay the same.
#[allow(clippy::too_many_arguments)]
pub fn collide_cylinder_ray(
	c_x: f32,
	c_y: f32,
	c_z: f32,
	c_r: f32,
	c_h: f32,
	ray_pos_x: f32,
	ray_pos_y: f32,
	ray_pos_z: f32,
	ray_dir_x: f32,
	ray_dir_y: f32,
	ray_dir_z: f32,
	max_dist: f32,
) -> Option<f32> {
	// Process as a circle on the X-Z plane.

	// Distance between the point and the ray start point.
	let x = c_x - ray_pos_x;
	let z = c_z - ray_pos_z;
	let d = (x * x + z * z).sqrt();

	// Compute the shortest distance between the point (cylinder center) and the ray.
	let c_min_dist = distance_pos_ray(
		c_x, 0.0, c_z, ray_pos_x, 0.0, ray_pos_z, ray_dir_x, 0.0, ray_dir_z,
	);

	// If the shortest distance is farther than the radius, it does not hit.
	if c_min_dist > c_r {
		return None;
	}

	let c_ray_dist = (d * d - c_min_dist * c_min_dist).sqrt(); // from the start point
	let c_r_dist = (c_r * c_r - c_min_dist * c_min_dist).sqrt(); // from the point radius

	let ray_xz_min = if d < c_r {
		// If the start point is inside the circle, distance is zero.
		0.0
	} else {
		// Ray-based minimum minus radius minimum.
		c_ray_dist - c_r_dist
	};

	// Distance between the point and the ray end point.
	let x = c_x - ray_pos_x + ray_dir_x * max_dist;
	let z = c_z - ray_pos_z + ray_dir_x * max_dist;
	let d = (x * x + z * z).sqrt();

	let ray_xz_max = if d < c_r {
		// If the end point is inside the circle, maximum distance.
		max_dist
	} else {
		c_ray_dist + c_r_dist
	};

	// Before comparing with the Y axis, convert so it can be compared with the Y axis
	// (convert the base value to the hypotenuse).
	let ray_ry = ray_dir_y.atan2((ray_dir_x * ray_dir_x + ray_dir_z * ray_dir_z).sqrt());
	let mut ray_min = [ray_xz_min / ray_ry.cos(), 0.0];
	let mut ray_max = [ray_xz_max / ray_ry.cos(), 0.0];

	// Processing only for the Y axis, the same as AABB.
	if ((ray_pos_y < c_y) || ((c_y + c_h) < ray_pos_y)) && (ray_dir_y == 0.0) {
		// Not inside, and if the vector direction is 0, it is already outside the cylinder.
		return None;
	}

	// Not inside, but if the vector has a direction...
	// Get the distance to the intersection point where the vector pierces the cylinder.
	let mut t1 = (c_y - ray_pos_y) / ray_dir_y;
	let mut t2 = ((c_y + c_h) - ray_pos_y) / ray_dir_y;

	// If the distances to the intersection point are reversed between minimum and
	// maximum, swap them.
	if t1 > t2 {
		std::mem::swap(&mut t1, &mut t2);
	}

	// Record as axis information.
	ray_min[1] = t1;
	ray_max[1] = t2;

	if (c_y <= ray_pos_y) && (ray_pos_y <= (c_y + c_h)) {
		// If inside, record it as axis information as is.
		ray_min[1] = 0.0;
	}

	// If the distance is negative (that is, the vector points backwards), it does not hit
	// the cylinder.
	if ray_min[1] < 0.0 {
		return None;
	}
	if ray_max[1] < 0.0 {
		return None;
	}

	// Combine both calculations.
	// For each axis, compute the farthest 'minimum distance' and the nearest 'maximum
	// distance'.
	let mut ray_tmin = ray_min[0];
	let mut ray_tmax = ray_max[0];
	if ray_tmin < ray_min[1] {
		ray_tmin = ray_min[1];
	}
	if ray_tmax > ray_max[1] {
		ray_tmax = ray_max[1];
	}

	// If the relation between the minimum and maximum distances is valid...
	if (ray_tmax - ray_tmin) > 0.0 {
		// If farther than the maximum distance to check, the result is invalid.
		if max_dist > 0.0 && ray_tmin > max_dist {
			return None;
		}

		return Some(ray_tmin);
	}

	None
}

/// `DistancePosRay`: the shortest distance between a point and a ray.
///
/// The direction is considered but the length is not, and a ray pointing away from the
/// point reports the distance to its start point instead.
pub fn distance_pos_ray(
	pos_x: f32,
	pos_y: f32,
	pos_z: f32,
	ray_pos_x: f32,
	ray_pos_y: f32,
	ray_pos_z: f32,
	ray_dir_x: f32,
	ray_dir_y: f32,
	ray_dir_z: f32,
) -> f32 {
	let x1 = pos_x - ray_pos_x;
	let y1 = pos_y - ray_pos_y;
	let z1 = pos_z - ray_pos_z;
	let (x2, y2, z2) = (ray_dir_x, ray_dir_y, ray_dir_z);

	// Inner product.
	let dot = x1 * x2 + y1 * y2 + z1 * z2;

	// If the ray vector points backwards.
	if dot < 0.0 {
		return (x1 * x1 + y1 * y1 + z1 * z1).sqrt();
	}

	// Cross product.
	let x3 = y1 * z2 - z1 * y2;
	let y3 = z1 * x2 - x1 * z2;
	let z3 = x1 * y2 - y1 * x2;

	(x3 * x3 + y3 * y3 + z3 * z3).sqrt()
		/ (ray_dir_x * ray_dir_x + ray_dir_y * ray_dir_y + ray_dir_z * ray_dir_z).sqrt()
}

/// `Collide2DLine`: collision check between two line segments on a plane, returning the
/// intersection point.
///
/// Segments that only touch at an end point do not intersect, since every comparison of
/// the parameters is strict.
#[allow(clippy::too_many_arguments)]
pub fn collide_2d_line(
	a1x: i32,
	a1y: i32,
	a2x: i32,
	a2y: i32,
	b1x: i32,
	b1y: i32,
	b2x: i32,
	b2y: i32,
) -> Option<(i32, i32)> {
	// Compute the vector of the line segment. The upstream `int` arithmetic wraps rather
	// than trapping, and the coordinates come from the radar, so these wrap too.
	let avx = a2x.wrapping_sub(a1x);
	let avy = a2y.wrapping_sub(a1y);
	let bvx = b2x.wrapping_sub(b1x);
	let bvy = b2y.wrapping_sub(b1y);

	// Cross product.
	let v1_v2 = avx.wrapping_mul(bvy).wrapping_sub(avy.wrapping_mul(bvx)) as f32;
	if v1_v2 == 0.0 {
		// Parallel.
		return None;
	}

	let vx = (b1x - a1x) as f32;
	let vy = (b1y - a1y) as f32;
	let v_v1 = vx * avy as f32 - vy * avx as f32; // Cross product.
	let v_v2 = vx * bvy as f32 - vy * bvx as f32; // Cross product.
	let t1 = v_v2 / v1_v2;
	let t2 = v_v1 / v1_v2;

	if (t1 <= 0.0) || (1.0 <= t1) || (t2 <= 0.0) || (1.0 <= t2) {
		// Not intersecting.
		return None;
	}

	Some((
		(a1x as f32 + avx as f32 * t1) as i32,
		(a1y as f32 + avy as f32 * t1) as i32,
	))
}

/// `Get2DLineInBox`: the part of a line segment that fits within a rectangle, returning
/// the clipped segment.
///
/// The rectangle must be given as `box_x1 < box_x2` and `box_y1 < box_y2`, and a line
/// that misses it entirely is refused rather than clipped to nothing. This is the simple
/// radar map's drawing helper.
#[allow(clippy::too_many_arguments)]
pub fn get_2d_line_in_box(
	line_x1: i32,
	line_y1: i32,
	line_x2: i32,
	line_y2: i32,
	box_x1: i32,
	box_y1: i32,
	box_x2: i32,
	box_y2: i32,
) -> Option<(i32, i32, i32, i32)> {
	// Invalid rectangle specification.
	if (box_x1 >= box_x2) || (box_y1 >= box_y2) {
		return None;
	}

	// If it is entirely above, below, left, or right, it misses.
	if (line_x1 < box_x1) && (line_x2 < box_x1) {
		return None;
	}
	if (line_y1 < box_y1) && (line_y2 < box_y1) {
		return None;
	}
	if (box_x2 < line_x1) && (box_x2 < line_x2) {
		return None;
	}
	if (box_y2 < line_y1) && (box_y2 < line_y2) {
		return None;
	}

	// Already fits within the rectangle.
	if (box_x1 <= line_x1)
		&& (line_x1 <= box_x2)
		&& (box_y1 <= line_y1)
		&& (line_y1 <= box_y2)
		&& (box_x1 <= line_x2)
		&& (line_x2 <= box_x2)
		&& (box_y1 <= line_y2)
		&& (line_y2 <= box_y2)
	{
		return Some((line_x1, line_y1, line_x2, line_y2));
	}

	// The line is trimmed against each edge in turn. The end inside the rectangle is the
	// one that gets replaced, which is why the start is tested every round.
	let mut line = (line_x1, line_y1, line_x2, line_y2);
	let edges = [
		(box_x1, box_y1, box_x2, box_y1), // Top edge
		(box_x2, box_y1, box_x2, box_y2), // Right edge
		(box_x2, box_y2, box_x1, box_y2), // Bottom edge
		(box_x1, box_y2, box_x1, box_y1), // Left edge
	];
	for (ex1, ey1, ex2, ey2) in edges {
		if let Some((x, y)) = collide_2d_line(ex1, ey1, ex2, ey2, line.0, line.1, line.2, line.3) {
			// If the start point is inside the rectangle, rewrite the end point,
			// otherwise (that is, the end point is inside) rewrite the start point.
			if (box_x1 <= line.0)
				&& (line.0 <= box_x2)
				&& (box_y1 <= line.1)
				&& (line.1 <= box_y2)
			{
				line.2 = x;
				line.3 = y;
			} else {
				line.0 = x;
				line.1 = y;
			}
		}
	}

	// Check again whether it fits within the rectangle.
	if (box_x1 <= line.0)
		&& (line.0 <= box_x2)
		&& (box_y1 <= line.1)
		&& (line.1 <= box_y2)
		&& (box_x1 <= line.2)
		&& (line.2 <= box_x2)
		&& (box_y1 <= line.3)
		&& (line.3 <= box_y2)
	{
		return Some(line);
	}

	None
}

/// `CheckTargetAngle`: distance check and angle calculation from an observation point to
/// a target point.
///
/// Returns the horizontal angle to the target, the vertical angle, and the distance
/// <b>squared</b>, which is what upstream hands back and warns about. `checkdist` of 0 or
/// less disables the distance check, in which case the angles are always computed.
#[allow(clippy::too_many_arguments)]
pub fn check_target_angle(
	pos_x: f32,
	pos_y: f32,
	pos_z: f32,
	rx: f32,
	ry: f32,
	target_x: f32,
	target_y: f32,
	target_z: f32,
	checkdist: f32,
) -> Option<(f32, f32, f32)> {
	let x = target_x - pos_x;
	let y = target_y - pos_y;
	let z = target_z - pos_z;

	let dist2 = x * x + y * y + z * z;

	if checkdist > 0.0 && dist2 > checkdist * checkdist {
		return None;
	}

	let angle_x = angle_normalization(z.atan2(x) - rx);
	let angle_y = y.atan2((x * x + z * z).sqrt()) - ry;

	Some((angle_x, angle_y, dist2))
}

/// `VectorNormalization`: normalizes a vector in place and returns its length before the
/// normalization.
///
/// A zero vector has no direction and is left alone.
pub fn vector_normalization(vx: &mut f32, vy: &mut f32, vz: &mut f32) -> f32 {
	let r = ((*vx) * (*vx) + (*vy) * (*vy) + (*vz) * (*vz)).sqrt();
	if r > 0.0 {
		*vx /= r;
		*vy /= r;
		*vz /= r;
	}
	r
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::degree_to_radian;
	use crate::datafile::TOTAL_BLOCKTEXTURE;
	use std::path::PathBuf;

	// --- helpers ---------------------------------------------------------

	/// A path in the temp directory that this test can write to.
	fn temp_path(name: &str) -> PathBuf {
		std::env::temp_dir().join(format!("xops_collision_{}_{}", std::process::id(), name))
	}

	fn write_f32_le(out: &mut Vec<u8>, value: f32) {
		out.extend_from_slice(&value.to_bits().to_le_bytes());
	}

	/// A .bd1 image holding `count` unit cubes, the shape the block data tests use:
	/// vertex j sits at the corner the face table implies.
	fn bd1_image(count: usize) -> Vec<u8> {
		const CUBE: [(f32, f32, f32); 8] = [
			(0.0, 0.0, 0.0),
			(1.0, 0.0, 0.0),
			(1.0, 1.0, 0.0),
			(0.0, 1.0, 0.0),
			(0.0, 0.0, 1.0),
			(1.0, 0.0, 1.0),
			(1.0, 1.0, 1.0),
			(0.0, 1.0, 1.0),
		];

		let mut out = Vec::new();
		out.extend_from_slice(&[0u8; 31 * TOTAL_BLOCKTEXTURE]);
		out.extend_from_slice(&(count as u16).to_le_bytes());
		for _ in 0..count {
			for axis in 0..3 {
				for corner in CUBE {
					let value = match axis {
						0 => corner.0,
						1 => corner.1,
						_ => corner.2,
					};
					write_f32_le(&mut out, value);
				}
			}
			for i in 24..72 {
				write_f32_le(&mut out, i as f32);
			}
			for j in 0..6 {
				out.push(j as u8);
				out.push(0);
				out.push(0);
				out.push(0);
			}
			out.extend_from_slice(&[0; 4]);
		}
		out
	}

	/// A map of `count` unit cubes with its collision prepared the way a mission load
	/// leaves it: the normals and shadows are calculated first, then the collision.
	fn cube_map(name: &str, count: usize) -> (BlockDataInterface, Collision) {
		// The name is per test: cargo runs the tests of a crate at once, and a shared
		// path would let one test read a file another is still writing.
		let path = temp_path(name);
		xops_platform::fs::write_file(&path, &bd1_image(count)).expect("write temp map");

		let mut blocks = BlockDataInterface::new();
		assert_eq!(blocks.load_file_data(path.to_string_lossy().as_bytes()), 0);
		blocks.calculation_blockdata(false);

		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(Some(&blocks)), 0);
		(blocks, coll)
	}

	/// The distance from the block center to the grown face of the unit cube, measured
	/// along an axis: COLLISION_ADDSIZE along the cube's diagonal projects onto each
	/// axis as 0.01 / sqrt(3).
	const GROWN_FACE: f32 = 0.0057735026;

	// --- construction ----------------------------------------------------

	#[test]
	fn an_uninitialized_collision_answers_no_to_everything() {
		let (blocks, _) = cube_map("uninitialized.bd1", 1);
		let coll = Collision::new();

		assert!(coll.get_block_pos_minmax(0).is_some());
		assert!(!coll.check_block_aabb(&blocks, 0, 0.4, 0.4, 0.4, 0.6, 0.6, 0.6));
		assert!(!coll.check_all_block_aabb(&blocks, 0.4, 0.4, 0.4, 0.6, 0.6, 0.6));
		assert!(!coll.check_polygon_front(&blocks, 0, 0, 0.5, 0.5, 0.5));
		assert!(!coll.check_block_inside(&blocks, 0, 0.5, 0.5, 0.5, true).0);
		assert!(!coll.check_all_block_inside(&blocks, 0.5, 0.5, 0.5));
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 10.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 10.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_dummy_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 10.0)
			.is_none());
		assert!(coll.scratch_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).is_none());
		assert!(coll.reflect_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).is_none());
		assert!(coll.angle_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).is_none());
		assert!(coll.angle_vector2(&blocks, 0, 0, 1.0, 0.0, 0.0).is_none());

		// And a collision handed no block data reports the upstream failure code.
		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(None), 1);
	}

	#[test]
	fn init_grows_the_block_and_records_its_groups() {
		let (blocks, coll) = cube_map("groups.bd1", 1);

		// The unit cube grew outwards, so the x = 1 face of the raw cube is at
		// 1 + 0.0057735 and the corner vertices are the grown ones.
		let (min_x, min_y, min_z, max_x, max_y, max_z) = coll.get_block_pos_minmax(0).unwrap();
		assert!((min_x - (-GROWN_FACE - COLLISION_ADDSIZE)).abs() < 1e-5, "{min_x}");
		assert!((max_x - (1.0 + GROWN_FACE + COLLISION_ADDSIZE)).abs() < 1e-5, "{max_x}");
		assert!((min_y - min_x).abs() < 1e-6);
		assert!((min_z - min_x).abs() < 1e-6);
		assert!((max_y - max_x).abs() < 1e-6);
		assert!((max_z - max_x).abs() < 1e-6);

		// Upstream reads the prepared array with no bound check at all, so an id the map
		// does not have still answers here, and only an id past the array is nothing.
		assert!(coll.get_block_pos_minmax(1).is_some());
		assert!(coll.get_block_pos_minmax(-1).is_none());
		assert!(coll.get_block_pos_minmax(MAX_BLOCKS as i32).is_none());

		// The cube's corners sit in the +X +Z quadrant on both ends, so it is not split
		// across groups the way a block straddling an axis would be.
		assert_eq!(coll.get_world_group(1.0, 1.0), 1);
		assert_eq!(coll.get_world_group(-1.0, 1.0), 2);
		assert_eq!(coll.get_world_group(-1.0, -1.0), 3);
		assert_eq!(coll.get_world_group(1.0, -1.0), 4);
		assert_eq!(coll.get_world_group(0.0, 1.0), 0);

		// An empty map has no blocks to build.
		let path = temp_path("empty.bd1");
		xops_platform::fs::write_file(&path, &bd1_image(0)).expect("write temp map");
		let mut empty = BlockDataInterface::new();
		empty.load_file_data(path.to_string_lossy().as_bytes());
		let mut coll = Collision::new();
		assert_eq!(coll.init_collision(Some(&empty)), 0);
		assert!(!coll.check_all_block_inside(&empty, 0.5, 0.5, 0.5));

		let _ = blocks;
	}

	// --- points ----------------------------------------------------------

	#[test]
	fn a_point_inside_the_block_is_buried_and_one_outside_is_not() {
		let (blocks, coll) = cube_map("point_inside.bd1", 1);

		// Inside the cube: no face is front-facing, so the point is buried.
		assert_eq!(coll.check_block_inside(&blocks, 0, 0.5, 0.5, 0.5, true), (true, 0));
		assert!(coll.check_all_block_inside(&blocks, 0.5, 0.5, 0.5));

		// Just outside the grown block on the +Z side, still within its bounds: face 1 is
		// the z = 1 face and reports itself.
		let (buried, face) = coll.check_block_inside(&blocks, 0, 0.5, 0.5, 1.01, true);
		assert_eq!((buried, face), (false, 1));

		// Further out the rough pre-check rejects the block before any face is looked at,
		// and upstream leaves the caller's face variable alone there.
		assert_eq!(coll.check_block_inside(&blocks, 0, 0.5, 0.5, 2.0, true), (false, 0));
		assert!(!coll.check_all_block_inside(&blocks, 0.5, 0.5, 2.0));

		// The block was grown by COLLISION_ADDSIZE, so a point just beyond the raw face
		// is still buried and one further out is not. Face 5 is the x = 0 face.
		assert_eq!(
			coll.check_block_inside(&blocks, 0, -0.004, 0.5, 0.5, true),
			(true, 0)
		);
		assert_eq!(
			coll.check_block_inside(&blocks, 0, -0.008, 0.5, 0.5, true),
			(false, 5)
		);

		// The pre-check skips blocks by their group and by their bounds, but the answer
		// is the same either way for a point beside the cube.
		assert!(!coll.check_block_inside(&blocks, 0, 5.0, 0.5, 0.5, true).0);
		assert!(!coll.check_block_inside(&blocks, 0, 5.0, 0.5, 0.5, false).0);

		// Ids and faces outside the data are refused.
		assert_eq!(coll.check_block_inside(&blocks, 5, 0.5, 0.5, 0.5, true), (false, 0));
		assert!(!coll.check_polygon_front(&blocks, 0, 6, 0.5, 0.5, 0.5));
		assert!(!coll.check_polygon_front(&blocks, 0, -1, 0.5, 0.5, 0.5));
		assert!(!coll.check_polygon_front_rx(&blocks, 0, 6, 0.0));
	}

	#[test]
	fn the_face_pickers_agree_with_the_point_test() {
		let (blocks, coll) = cube_map("face_pickers.bd1", 1);

		// In front of the z = 0 face, and behind the face normal's side of the plane.
		assert!(coll.check_polygon_front(&blocks, 0, 0, 0.5, 0.5, -1.0));
		assert!(!coll.check_polygon_front(&blocks, 0, 0, 0.5, 0.5, 1.0));

		// Looking along -Z at the z = 0 face is front-facing, since its normal points that
		// way, and looking along +Z is the back of it.
		assert!(coll.check_polygon_front_rx(&blocks, 0, 0, std::f32::consts::FRAC_PI_2));
		assert!(!coll.check_polygon_front_rx(&blocks, 0, 0, -std::f32::consts::FRAC_PI_2));
	}

	// --- rays ------------------------------------------------------------

	#[test]
	fn a_ray_hits_a_face_and_misses_what_it_cannot_reach() {
		let (blocks, coll) = cube_map("ray_hit.bd1", 1);

		// Straight at the z = 0 face from -Z. The face sits at -GROWN_FACE, so the
		// distance is 5.0 minus that, which is as close as a distance is ever pinned.
		let (face, dist) = coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.expect("the ray meets the z = 0 face");
		assert_eq!(face, 0);
		assert!((dist - (5.0 - GROWN_FACE)).abs() < 1e-4, "{dist}");

		// The all-block version finds the same face and distance.
		let (id, face, dist2) = coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.expect("the ray meets the block");
		assert_eq!((id, face), (0, 0));
		assert!((dist2 - dist).abs() < 1e-6, "{dist2} {dist}");

		// A zero direction is refused before any work.
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, -5.0, 0.0, 0.0, 0.0, 100.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 0.0, 100.0)
			.is_none());

		// Beside the cube: the AABB test throws the block out before any face test.
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 5.0, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 5.0, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.is_none());

		// A ray that would hit but stops short is not a hit, one just long enough is,
		// which is the `min_dist > max_dist` comparison.
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 4.9)
			.is_none());
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 5.0)
			.is_some());

		// Below zero is the unlimited search.
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, -1.0)
			.is_some());
	}

	#[test]
	fn a_ray_pointing_away_from_a_face_does_not_hit_it() {
		let (blocks, coll) = cube_map("ray_away.bd1", 1);

		// From inside the cube every face is back-facing, so the ray hits nothing: the
		// faces a ray starts behind are the ones that are ignored.
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, 0.5, 0.0, 0.0, 1.0, 100.0)
			.is_none());

		// From outside, pointing away from the cube: nothing either.
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, -1.0, 100.0)
			.is_none());

		// From outside, pointing at it, the near face is the one that answers.
		let (face, dist) = coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, 5.0, 0.0, 0.0, -1.0, 100.0)
			.expect("the ray meets the z = 1 face");
		assert_eq!(face, 1);
		assert!((dist - (5.0 - (1.0 + GROWN_FACE))).abs() < 1e-4, "{dist}");
	}

	#[test]
	fn the_dummy_ray_tests_three_points_and_ignores_plate_blocks_bounds() {
		let (blocks, coll) = cube_map("dummy_ray.bd1", 1);

		// The end point is buried, so the face reported is the one the start point saw
		// rather than the face that was entered.
		let (id, face) = coll
			.check_all_block_intersect_dummy_ray(&blocks, 0.5, 0.5, 1.01, 0.0, 0.0, -1.0, 1.01)
			.expect("the end point is buried");
		assert_eq!((id, face), (0, 1));

		// The mid point is buried while the end point is not, so the end point's face is
		// the one carried into the result.
		let (id, face) = coll
			.check_all_block_intersect_dummy_ray(&blocks, 0.5, 0.5, -0.01, 0.0, 0.0, 1.0, 1.02)
			.expect("the mid point is buried");
		assert_eq!((id, face), (0, 1));

		// Short of the cube, and a maximum distance of zero is refused outright.
		assert!(coll
			.check_all_block_intersect_dummy_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 2.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_dummy_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 0.0)
			.is_none());
	}

	#[test]
	fn a_zero_thickness_block_is_never_hit() {
		// A plate: vertices 0 and 1 are the same point, which is what makes it a board.
		let mut image = bd1_image(1);
		// Vertex 1 (the second corner) of the first block: floats 8..11 are x, y, z of
		// vertices 0 and 1, and the block starts after the texture block and the count.
		let base = 31 * TOTAL_BLOCKTEXTURE + 2;
		for axis in 0..3 {
			let offset = base + (8 + axis) * 4;
			let value = image[base + axis * 4..base + axis * 4 + 4].to_vec();
			image[offset..offset + 4].copy_from_slice(&value);
		}

		let path = temp_path("plate.bd1");
		xops_platform::fs::write_file(&path, &image).expect("write temp map");
		let mut blocks = BlockDataInterface::new();
		blocks.load_file_data(path.to_string_lossy().as_bytes());
		blocks.calculation_blockdata(false);

		let mut coll = Collision::new();
		coll.init_collision(Some(&blocks));

		assert!(
			!coll.check_block_aabb(&blocks, 0, 0.4, 0.4, 0.4, 0.6, 0.6, 0.6),
			"a plate is excluded from the AABB check"
		);
		assert!(!coll.check_block_inside(&blocks, 0, 0.5, 0.5, 0.5, true).0);
		assert!(coll
			.check_block_intersect_ray(&blocks, 0, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.is_none());
		assert!(coll
			.check_all_block_intersect_ray(&blocks, 0.5, 0.5, -5.0, 0.0, 0.0, 1.0, 100.0)
			.is_none());
	}

	// --- slopes ----------------------------------------------------------

	#[test]
	fn slopes_above_the_climbable_angle_are_refused() {
		let (blocks, _coll) = cube_map("slopes.bd1", 1);

		// The objects ask whether acos(face normal Y) is above the limit. The x = 1 face
		// is a wall and the y = 1 face is flat.
		let wall = blocks.getdata(0).unwrap().material[3].vy;
		let floor = blocks.getdata(0).unwrap().material[4].vy;
		assert_eq!(wall, 0.0);
		assert_eq!(floor, 1.0);
		assert!(wall.acos() > HUMAN_MAPCOLLISION_SLOPEANGLE);
		assert!(floor.acos() < HUMAN_MAPCOLLISION_SLOPEANGLE);

		// The limit itself is 50 degrees and is not refused, and it stays that value.
		assert_eq!(HUMAN_MAPCOLLISION_SLOPEANGLE, degree_to_radian(50.0));
		assert!(degree_to_radian(60.0).cos().acos() > HUMAN_MAPCOLLISION_SLOPEANGLE);
		assert!(degree_to_radian(30.0).cos().acos() < HUMAN_MAPCOLLISION_SLOPEANGLE);

		// The other map tuning constants keep their upstream values.
		assert_eq!(HUMAN_MAPCOLLISION_R, 2.8);
		assert_eq!(HUMAN_MAPCOLLISION_HEIGHT, 10.0);
		assert_eq!(HUMAN_MAPCOLLISION_ADD_HEIGHT_A, 9.0);
		assert_eq!(HUMAN_MAPCOLLISION_ADD_HEIGHT_B, 13.0);
		assert_eq!(HUMAN_MAPCOLLISION_CLIMBHEIGHT, 3.2);
		assert_eq!(HUMAN_MAPCOLLISION_CLIMBADDY, 0.4);
		assert_eq!(HUMAN_MAPCOLLISION_GROUND_HEIGHT, -0.5);
		assert_eq!(HUMAN_MAPCOLLISION_GROUND_R1, 0.15);
		assert_eq!(HUMAN_MAPCOLLISION_GROUND_R2, 0.50);
		assert_eq!(HUMAN_MAPCOLLISION_GROUND_R3, 0.30);
		assert_eq!(HUMAN_MAPCOLLISION_CHECK_MAXDIST, 12.0);
		assert_eq!(HUMAN_MAPCOLLISION_CHECK_HEIGHT, 17.0);
		assert_eq!(COLLISION_ADDSIZE, 0.01);
	}

	// --- vectors ---------------------------------------------------------

	#[test]
	fn scratch_and_reflect_work_off_the_face_normal() {
		let (blocks, coll) = cube_map("scratch.bd1", 1);

		// Face 0 is the z = 0 face, normal -Z. A vector along +Z has all of its length
		// along the normal, so scratching it leaves nothing and reflecting reverses it.
		let scratch = coll.scratch_vector(&blocks, 0, 0, 0.0, 0.0, 1.0).unwrap();
		assert!(scratch.0.abs() < 1e-6 && scratch.1.abs() < 1e-6 && scratch.2.abs() < 1e-6);
		let reflect = coll.reflect_vector(&blocks, 0, 0, 0.0, 0.0, 1.0).unwrap();
		assert!(reflect.0.abs() < 1e-6 && reflect.1.abs() < 1e-6);
		assert!((reflect.2 + 1.0).abs() < 1e-6);

		// A vector parallel to the face is unchanged by both.
		let scratch = coll.scratch_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).unwrap();
		assert!((scratch.0 - 1.0).abs() < 1e-6 && scratch.2.abs() < 1e-6);
		let reflect = coll.reflect_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).unwrap();
		assert!((reflect.0 - 1.0).abs() < 1e-6 && reflect.2.abs() < 1e-6);

		// Angles: head on is 90 degrees with the sign flipped, parallel is nothing.
		let head_on = coll.angle_vector(&blocks, 0, 0, 0.0, 0.0, -1.0).unwrap();
		assert!((head_on + std::f32::consts::FRAC_PI_2).abs() < 1e-6, "{head_on}");
		let parallel = coll.angle_vector(&blocks, 0, 0, 1.0, 0.0, 0.0).unwrap();
		assert!(parallel.abs() < 1e-6);

		// AngleVector2 only answers for a vector that leans into the face, where the
		// angle between the two is the whole half turn.
		let into = coll.angle_vector2(&blocks, 0, 0, 0.0, 0.0, 1.0).unwrap();
		assert!((into - std::f32::consts::PI).abs() < 1e-6, "{into}");
		assert!(coll.angle_vector2(&blocks, 0, 0, 0.0, 0.0, -1.0).is_none());
		assert!(coll.angle_vector2(&blocks, 0, 0, 0.0, 0.0, 0.0).is_none());
		assert!(coll.scratch_vector(&blocks, 9, 0, 1.0, 0.0, 0.0).is_none());
	}

	#[test]
	fn vector_normalization_returns_the_length_it_had() {
		let (mut x, mut y, mut z) = (3.0f32, 0.0, 4.0);
		assert_eq!(vector_normalization(&mut x, &mut y, &mut z), 5.0);
		assert!((x - 0.6).abs() < 1e-6 && z == 0.8);

		// A zero vector has no direction and is left alone.
		let (mut x, mut y, mut z) = (0.0f32, 0.0, 0.0);
		assert_eq!(vector_normalization(&mut x, &mut y, &mut z), 0.0);
		assert_eq!((x, y, z), (0.0, 0.0, 0.0));
	}

	// --- the free geometry helpers ---------------------------------------

	#[test]
	fn boxes_overlap_only_when_they_really_do() {
		assert!(collide_box_aabb(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 2.0, 2.0, 2.0
		));
		// Touching is not overlapping.
		assert!(!collide_box_aabb(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0
		));
		// An inverted box is an error on either side.
		assert!(!collide_box_aabb(
			1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0
		));
		assert!(!collide_box_aabb(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0
		));
	}

	#[test]
	fn rays_enter_boxes_and_are_measured_from_the_start() {
		// Starting inside is a hit at zero.
		assert_eq!(
			collide_aabb_ray(
				0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0, -1.0
			),
			Some(0.0)
		);
		// From outside it is the distance to the entry point.
		let d = collide_aabb_ray(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, -5.0, 0.5, 0.5, 1.0, 0.0, 0.0, -1.0,
		)
		.unwrap();
		assert!((d - 5.0).abs() < 1e-5, "{d}");
		// Pointing away, parallel and outside, and an inverted box all miss.
		assert!(collide_aabb_ray(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, -5.0, 0.5, 0.5, -1.0, 0.0, 0.0, -1.0
		)
		.is_none());
		assert!(collide_aabb_ray(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, -5.0, 0.5, 0.5, 0.0, 1.0, 0.0, -1.0
		)
		.is_none());
		assert!(collide_aabb_ray(
			1.0, 0.0, 0.0, 0.0, 1.0, 1.0, -5.0, 0.5, 0.5, 1.0, 0.0, 0.0, -1.0
		)
		.is_none());

		// A maximum distance shorter than the entry point misses.
		assert!(collide_aabb_ray(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, -5.0, 0.5, 0.5, 1.0, 0.0, 0.0, 4.0
		)
		.is_none());
		assert!(collide_aabb_ray(
			0.0, 0.0, 0.0, 1.0, 1.0, 1.0, -5.0, 0.5, 0.5, 1.0, 0.0, 0.0, 6.0
		)
		.is_some());
	}

	#[test]
	fn the_ray_aabb_covers_both_ends() {
		// A ray along +X from the origin: the box spans the start and the end, grown by
		// COLLISION_ADDSIZE at both ends.
		let (min_x, min_y, _, max_x, _, max_z) =
			get_aabb_ray(0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 10.0);
		assert!((min_x + COLLISION_ADDSIZE).abs() < 1e-6, "{min_x}");
		assert!((max_x - (10.0 + COLLISION_ADDSIZE)).abs() < 1e-6, "{max_x}");
		assert!((min_y + COLLISION_ADDSIZE).abs() < 1e-6);
		assert!((max_z - COLLISION_ADDSIZE).abs() < 1e-6);

		// A ray pointing the other way swaps which end is the minimum.
		let (min_x, _, _, max_x, _, _) = get_aabb_ray(0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 10.0);
		assert!((min_x + 10.0 + COLLISION_ADDSIZE).abs() < 1e-6, "{min_x}");
		assert!((max_x - COLLISION_ADDSIZE).abs() < 1e-6, "{max_x}");
	}

	#[test]
	fn sphere_inside_is_strict_at_the_surface() {
		assert!(collide_sphere_inside(0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0));
		// Exactly on the surface is not inside.
		assert!(!collide_sphere_inside(0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0));
		assert!(collide_sphere_inside(0.0, 0.0, 0.0, 1.0, 0.999999, 0.0, 0.0));
		assert!(!collide_sphere_inside(0.0, 0.0, 0.0, 1.0, 1.000001, 0.0, 0.0));
	}

	#[test]
	fn sphere_rays_report_the_near_surface() {
		// A sphere of radius 1 at the origin, hit along +X from -5: the surface is one
		// radius before the closest approach point.
		let d = collide_sphere_ray(0.0, 0.0, 0.0, 1.0, -5.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0).unwrap();
		assert!((d - 4.0).abs() < 1e-5, "{d}");

		// Starting inside is a hit at zero.
		assert_eq!(
			collide_sphere_ray(0.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0),
			Some(0.0)
		);
		// Past the sphere, and a maximum distance short of the surface, both miss.
		assert!(collide_sphere_ray(0.0, 0.0, 0.0, 1.0, -5.0, 2.0, 0.0, 1.0, 0.0, 0.0, -1.0)
			.is_none());
		assert!(collide_sphere_ray(0.0, 0.0, 0.0, 1.0, -5.0, 0.0, 0.0, 1.0, 0.0, 0.0, 3.5)
			.is_none());
	}

	#[test]
	fn cylinders_are_circles_on_the_x_z_plane() {
		// Inside on both the circle and the height.
		assert!(collide_cylinder_inside(0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 1.0, 0.0));
		// Outside the circle, and above the top.
		assert!(!collide_cylinder_inside(0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 1.0, 0.9));
		assert!(!collide_cylinder_inside(0.0, 0.0, 0.0, 1.0, 2.0, 0.5, 2.1, 0.0));

		// A ray across the cylinder enters at the near surface.
		let d = collide_cylinder_ray(
			0.0, 0.0, 0.0, 1.0, 2.0, -5.0, 0.0, 0.0, 1.0, 0.0, 0.0, 100.0,
		)
		.unwrap();
		assert!((d - 4.0).abs() < 1e-5, "{d}");
		// Above the cylinder, and a maximum distance short of it, both miss.
		assert!(collide_cylinder_ray(
			0.0, 0.0, 0.0, 1.0, 2.0, -5.0, 5.0, 0.0, 1.0, 0.0, 0.0, 100.0
		)
		.is_none());
		assert!(collide_cylinder_ray(
			0.0, 0.0, 0.0, 1.0, 2.0, -5.0, 0.0, 0.0, 1.0, 0.0, 0.0, 3.0
		)
		.is_none());
	}

	#[test]
	fn cylinder_pairs_hit_by_height_and_distance() {
		let (angle, length) =
			collide_cylinder(0.0, 0.0, 0.0, 1.0, 2.0, 1.0, 0.0, 0.0, 1.0, 2.0).unwrap();
		assert!((angle - std::f32::consts::PI).abs() < 1e-6, "{angle}");
		assert!((length - 1.0).abs() < 1e-6, "{length}");

		// Too far apart, and too far apart in height.
		assert!(collide_cylinder(0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.0, 0.0, 1.0, 2.0).is_none());
		assert!(collide_cylinder(0.0, 0.0, 0.0, 1.0, 2.0, 1.0, 5.0, 0.0, 1.0, 2.0).is_none());
	}

	#[test]
	fn distance_to_a_ray_is_perpendicular_or_to_the_start() {
		// Beside the ray: the perpendicular distance.
		assert!(
			(distance_pos_ray(0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0) - 1.0).abs() < 1e-6
		);
		// Behind the ray start: the distance to the start point.
		assert!(
			(distance_pos_ray(-3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0) - 3.0).abs() < 1e-6
		);
		// A direction that is not normalized still gives a world distance.
		assert!(
			(distance_pos_ray(0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 7.0, 0.0, 0.0) - 2.0).abs() < 1e-6
		);
	}

	#[test]
	fn two_d_lines_cross_and_are_clipped_to_the_box() {
		assert_eq!(collide_2d_line(0, 0, 10, 10, 0, 10, 10, 0), Some((5, 5)));
		// Parallel, and not overlapping.
		assert!(collide_2d_line(0, 0, 10, 10, 0, 1, 10, 11).is_none());
		assert!(collide_2d_line(0, 0, 1, 1, 5, 5, 6, 6).is_none());

		// A line crossing the box comes back clipped to its edges.
		assert_eq!(
			get_2d_line_in_box(-10, 5, 10, 5, 0, 0, 20, 10),
			Some((0, 5, 10, 5))
		);
		// A line inside is untouched, and one entirely outside is refused.
		assert_eq!(get_2d_line_in_box(1, 1, 2, 2, 0, 0, 20, 10), Some((1, 1, 2, 2)));
		assert!(get_2d_line_in_box(-5, -5, -1, -1, 0, 0, 20, 10).is_none());
		// An inverted rectangle is an error.
		assert!(get_2d_line_in_box(1, 1, 2, 2, 20, 10, 0, 0).is_none());
	}

	#[test]
	fn target_angles_are_relative_to_the_observer() {
		// Facing along +X, a target straight ahead is at no angle, and the distance comes
		// back squared rather than as a length.
		let (rx, ry, dist2) =
			check_target_angle(0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 0.0, 0.0).unwrap();
		assert!(rx.abs() < 1e-6);
		assert!(ry.abs() < 1e-6);
		assert!((dist2 - 9.0).abs() < 1e-6, "{dist2}");

		// Off to the side and above: the angles are measured against the observer's own
		// rotation, so turning the observer by the same amount cancels them out.
		let (rx, ry, dist2) =
			check_target_angle(0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 4.0, 0.0).unwrap();
		assert!((rx - 4.0f32.atan2(3.0)).abs() < 1e-6, "{rx}");
		assert!(ry.abs() < 1e-6);
		assert!((dist2 - 25.0).abs() < 1e-6, "{dist2}");
		let (rx, _, _) =
			check_target_angle(0.0, 0.0, 0.0, 4.0f32.atan2(3.0), 0.0, 3.0, 0.0, 4.0, 0.0).unwrap();
		assert!(rx.abs() < 1e-6, "{rx}");

		// A target at 45 degrees around, and one the check distance refuses.
		let (rx, _, _) = check_target_angle(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0).unwrap();
		assert!((rx - std::f32::consts::FRAC_PI_4).abs() < 1e-6, "{rx}");
		assert!(check_target_angle(0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 4.0, 4.0).is_none());
		assert!(check_target_angle(0.0, 0.0, 0.0, 0.0, 0.0, 3.0, 0.0, 4.0, 5.0).is_some());
	}
}
