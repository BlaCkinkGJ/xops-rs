//! Model loading, from the upstream `D3DGraphics::LoadModel` and its neighbours.
//!
//! The models are Direct3D `.x` text files. Upstream parses them by hand and expands every
//! polygon into six vertices laid out as a triangle strip, and that layout is what the
//! draw call and the morphing path both assume, so it is reproduced here exactly. Nothing
//! about it is an implementation detail: the render path indexes these arrays directly.

/// Maximum number of model slots, from `d3dgraphics.h`.
pub const MAX_MODEL: usize = 72;

/// One vertex of the intermediate `.x` data before a face is expanded.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
struct XVertex {
	x: f32,
	y: f32,
	z: f32,
	u: f32,
	v: f32,
}

/// A loaded model, in the shape the client-array draw path wants.
///
/// Every polygon contributes exactly six vertices, arranged so the whole thing draws as
/// one triangle strip. A triangle becomes `a a c b b b` and a quad becomes `a a d b c c`,
/// which is the upstream expansion, and one `glDrawArrays(GL_TRIANGLE_STRIP, 1, ...)` or
/// its equivalent covers the whole model.
///
/// The last two vertices of each polygon must be the same one. A strip joins one polygon to
/// the next with the triangle `(4, 5, 6)`, taking the fifth and sixth vertices of this
/// polygon and the first of the following one, and that join is degenerate exactly when the
/// fifth and sixth are equal. End them differently and every polygon boundary across the
/// whole model grows a long thin triangle instead: on a soldier's face it is a dark streak
/// from the nose to the chest, and on a gun it is a piece sticking out of the side.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelData {
	pub useflag: bool,
	pub polygons: i32,
	/// `polygons * 6 * 3`, positions only. X is negated, see [`parse_model_file`].
	pub vertex_ary: Vec<f32>,
	/// `polygons * 6 * 4`, the lit material colour.
	pub color_ary: Vec<f32>,
	/// `polygons * 6 * 4`, the darkened material colour.
	pub color_gray_ary: Vec<f32>,
	/// `polygons * 6 * 2`, texture coordinates.
	pub tex_coord_ary: Vec<f32>,
}

impl ModelData {
	/// The number of vertices a draw should submit, which is what the upstream render
	/// passes to `glDrawArrays`.
	pub fn vertex_count(&self) -> i32 {
		self.polygons * 6
	}
}

/// The model slots, indexed the way the upstream `pmodel` array is.
pub struct ModelTable {
	slots: Vec<Option<ModelData>>,
}

impl Default for ModelTable {
	fn default() -> Self {
		ModelTable::new()
	}
}

impl ModelTable {
	pub fn new() -> ModelTable {
		ModelTable {
			slots: vec![None; MAX_MODEL],
		}
	}

	fn free_slot(&self) -> Option<usize> {
		self.slots.iter().position(|slot| slot.is_none())
	}

	pub fn get(&self, id: i32) -> Option<&ModelData> {
		if id < 0 || id as usize >= MAX_MODEL {
			return None;
		}
		self.slots[id as usize].as_ref()
	}

	pub fn get_mut(&mut self, id: i32) -> Option<&mut ModelData> {
		if id < 0 || id as usize >= MAX_MODEL {
			return None;
		}
		self.slots[id as usize].as_mut()
	}

	/// Number of slots in use, which is what the upstream counts by walking the array.
	pub fn get_total(&self) -> i32 {
		self.slots.iter().filter(|slot| slot.is_some()).count() as i32
	}

	/// Loads a model file into the first free slot. Returns the slot, or -1 on failure,
	/// which is the upstream contract the callers branch on.
	pub fn load(&mut self, path: &std::path::Path) -> i32 {
		let data =
			match xops_platform::fs::read_file(xops_platform::path::resolve_case_insensitive(path))
			{
				Ok(data) => data,
				Err(_) => return -1,
			};
		let model = match parse_model_file(&data) {
			Ok(model) => model,
			Err(_) => return -1,
		};
		let Some(id) = self.free_slot() else {
			return -1;
		};
		self.slots[id] = Some(model);
		id as i32
	}

	/// Creates the model halfway between two loaded ones.
	///
	/// The upstream only checks that the polygon counts match, not the vertex counts it
	/// documents as a requirement, and it averages position by position. Both are kept.
	pub fn morph(&mut self, id_a: i32, id_b: i32) -> i32 {
		let (Some(a), Some(b)) = (self.get(id_a), self.get(id_b)) else {
			return -1;
		};
		if a.polygons != b.polygons {
			return -1;
		}
		let Some(id) = self.free_slot() else {
			return -1;
		};

		let count = (a.polygons * 6) as usize;
		let mut model = ModelData {
			useflag: true,
			polygons: a.polygons,
			vertex_ary: vec![0.0; count * 3],
			color_ary: vec![0.0; count * 4],
			color_gray_ary: vec![0.0; count * 4],
			tex_coord_ary: vec![0.0; count * 2],
		};

		for i in 0..count * 3 {
			model.vertex_ary[i] = (a.vertex_ary[i] + b.vertex_ary[i]) / 2.0;
		}
		// The material comes from A alone, so a morph between two differently coloured
		// models keeps the first one's colour.
		model.color_ary.copy_from_slice(&a.color_ary[..count * 4]);
		model
			.color_gray_ary
			.copy_from_slice(&a.color_gray_ary[..count * 4]);
		model
			.tex_coord_ary
			.copy_from_slice(&a.tex_coord_ary[..count * 2]);

		self.slots[id] = Some(model);
		id as i32
	}

	pub fn cleanup(&mut self, id: i32) {
		if id < 0 || id as usize >= MAX_MODEL {
			return;
		}
		self.slots[id as usize] = None;
	}

	pub fn cleanup_all(&mut self) {
		for slot in self.slots.iter_mut() {
			*slot = None;
		}
	}
}

/// Parses a `.x` file's bytes.
///
/// The caller is expected to have read the file. X is negated, because the game's data is
/// left handed and the renderer is not, and the UV coordinates come from the
/// `MeshTextureCoords` block that follows each mesh.
pub fn parse_model_file(data: &[u8]) -> Result<ModelData, String> {
	// The upstream truncates the first line at the magic code's length and compares, so
	// a file whose first eleven bytes differ is not an X file at all.
	if data.len() < 11 || &data[..11] != b"xof 0302txt" {
		return Err("not an X file".to_string());
	}

	let lines = split_lines(data);
	let mut cursor = 1;
	let mut vertices: Vec<XVertex> = Vec::new();
	// Each entry is [vertex count, v1, v2, v3, v4]. Upstream allocates five slots per
	// polygon whatever the polygon is, and only the first `count + 1` are meaningful.
	let mut indices: Vec<[i32; 5]> = Vec::new();

	while cursor < lines.len() {
		let line = lines[cursor];
		cursor += 1;

		// Upstream truncates at four characters and compares, so any line starting with
		// "Mesh" opens a mesh block.
		if line.len() < 4 || &line[..4] != b"Mesh" {
			continue;
		}

		let in_vertexs = read_int(fetch(&lines, &mut cursor)?);
		if in_vertexs < 0 {
			return Err(format!("negative vertex count {in_vertexs}"));
		}
		let base = vertices.len();
		for _ in 0..in_vertexs {
			let line = fetch(&lines, &mut cursor)?;
			let mut tokens = tokenize(line);
			let x = next_f32(&mut tokens) * -1.0;
			let y = next_f32(&mut tokens);
			let z = next_f32(&mut tokens);
			vertices.push(XVertex {
				x,
				y,
				z,
				u: 0.0,
				v: 0.0,
			});
		}

		// One line sits between the vertex list and the polygon count. Upstream reads it
		// and throws it away.
		let _separator = fetch(&lines, &mut cursor)?;

		let in_polygons = read_int(fetch(&lines, &mut cursor)?);
		if in_polygons < 0 {
			return Err(format!("negative polygon count {in_polygons}"));
		}
		for _ in 0..in_polygons {
			let line = fetch(&lines, &mut cursor)?;
			let mut tokens = tokenize(line);
			let mut entry = [0i32; 5];
			entry[0] = next_i32(&mut tokens);
			// Upstream writes one slot per named vertex without bounding the count, so a
			// face naming five vertices overruns its allocation. Clamping is the only
			// difference here and it only shows on a malformed file.
			let sides = entry[0].clamp(0, 4);
			for slot in entry.iter_mut().take(sides as usize + 1).skip(1) {
				*slot = next_i32(&mut tokens) + base as i32;
			}
			indices.push(entry);
		}

		// The texture coordinates follow the face list, and upstream stops looking at the
		// first line that is not the block it wants.
		while cursor < lines.len() {
			let line = lines[cursor];
			cursor += 1;
			// `strlen(" MeshTextureCoords")` is 18, and that is the prefix upstream
			// truncates to before comparing.
			if line.len() < 18 || &line[..18] != b" MeshTextureCoords" {
				continue;
			}
			let count = read_int(fetch(&lines, &mut cursor)?);
			if count != in_vertexs {
				break;
			}
			for index in 0..in_vertexs as usize {
				let line = fetch(&lines, &mut cursor)?;
				let mut tokens = tokenize(line);
				let u = next_f32(&mut tokens);
				let v = next_f32(&mut tokens);
				vertices[base + index].u = u;
				vertices[base + index].v = v;
			}
			break;
		}
	}

	Ok(expand_model(&vertices, &indices))
}

/// Turns the parsed vertices and faces into the strip layout the renderer draws.
fn expand_model(vertices: &[XVertex], indices: &[[i32; 5]]) -> ModelData {
	let polygons = indices.len();
	let mut model = ModelData {
		useflag: true,
		polygons: polygons as i32,
		vertex_ary: vec![0.0; polygons * 6 * 3],
		color_ary: vec![0.0; polygons * 6 * 4],
		color_gray_ary: vec![0.0; polygons * 6 * 4],
		tex_coord_ary: vec![0.0; polygons * 6 * 2],
	};

	let mut cursor = 0usize;
	for entry in indices {
		// Six vertices per polygon, in the order the upstream writes them. A triangle is
		// a a c b b b -- the fifth and sixth are the same vertex, which is what keeps the
		// join to the next polygon degenerate -- and anything else is a a d b c c, using
		// the face's slots 1, 2, 3 and 4. `slot` reads a face vertex, treating an out of
		// range index as the origin rather than panicking the way a raw pointer would.
		let order: [usize; 6] = if entry[0] == 3 {
			[1, 1, 3, 2, 2, 2]
		} else {
			[1, 1, 4, 2, 3, 3]
		};

		for (step, &slot) in order.iter().enumerate() {
			let vertex_index = entry[slot];
			let vertex = if vertex_index >= 0 && (vertex_index as usize) < vertices.len() {
				vertices[vertex_index as usize]
			} else {
				XVertex::default()
			};

			let base = (cursor + step) * 3;
			model.vertex_ary[base] = vertex.x;
			model.vertex_ary[base + 1] = vertex.y;
			model.vertex_ary[base + 2] = vertex.z;

			let ubase = (cursor + step) * 2;
			model.tex_coord_ary[ubase] = vertex.u;
			model.tex_coord_ary[ubase + 1] = vertex.v;
		}

		cursor += 6;
	}

	if polygons > 0 {
		// Upstream fills the first vertex and then copies it over the rest.
		let lit = [1.0f32, 1.0, 1.0, 1.0];
		let dim = [0.8f32, 0.8, 0.8, 1.0];
		for step in 0..polygons * 6 {
			model.color_ary[step * 4..step * 4 + 4].copy_from_slice(&lit);
			model.color_gray_ary[step * 4..step * 4 + 4].copy_from_slice(&dim);
		}
	}

	model
}

/// Splits on newlines, dropping a trailing carriage return so CRLF files parse too.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
	data.split(|&byte| byte == b'\n')
		.map(|line| {
			if line.last() == Some(&b'\r') {
				&line[..line.len() - 1]
			} else {
				line
			}
		})
		.collect()
}

/// Takes the next line, or fails at the end of the file the way the upstream `fgets`
/// checks do.
fn fetch<'a>(lines: &[&'a [u8]], cursor: &mut usize) -> Result<&'a [u8], String> {
	let line = lines
		.get(*cursor)
		.ok_or_else(|| "unexpected end of file".to_string())?;
	*cursor += 1;
	Ok(line)
}

/// `atoi`, reading whatever integer leads the line and yielding zero otherwise.
fn read_int(line: &[u8]) -> i32 {
	let mut tokens = tokenize(line);
	let token = tokens.next().unwrap_or(b"");
	std::str::from_utf8(token)
		.ok()
		.and_then(|text| text.parse::<i32>().ok())
		.unwrap_or(0)
}

/// `strtok` with the upstream delimiter set, which treats a run of delimiters as one.
fn tokenize(line: &[u8]) -> impl Iterator<Item = &[u8]> {
	line.split(|byte| matches!(byte, b' ' | b';' | b','))
		.filter(|token| !token.is_empty())
}

/// `atof` on the next token. A missing token reads as zero instead of dereferencing null,
/// which is the one place the upstream would crash rather than misbehave.
fn next_f32(tokens: &mut impl Iterator<Item = impl AsRef<[u8]>>) -> f32 {
	let token = tokens.next();
	let Some(token) = token else {
		return 0.0;
	};
	let bytes = token.as_ref();
	std::str::from_utf8(bytes)
		.ok()
		.and_then(|text| text.parse::<f32>().ok())
		.unwrap_or(0.0)
}

fn next_i32(tokens: &mut impl Iterator<Item = impl AsRef<[u8]>>) -> i32 {
	let token = tokens.next();
	let Some(token) = token else {
		return 0;
	};
	let bytes = token.as_ref();
	std::str::from_utf8(bytes)
		.ok()
		.and_then(|text| text.parse::<i32>().ok())
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use super::*;

	// The data lines below start at column zero on purpose. The parser matches `Mesh`
	// and ` MeshTextureCoords` at the start of a line, and the space before the second one
	// is part of what it matches, so the literal cannot be indented or joined with
	// backslash continuations (which strip the next line's leading whitespace).

	/// One triangle: three vertices, one face, texture coordinates in the block after it.
	const TRIANGLE: &[u8] = b"xof 0302txt 0064
Mesh {
3;
0.0;0.0;0.0;,
1.0;0.0;0.0;,
0.0;1.0;0.0;,
;
1;
3;0,1,2;,
 MeshTextureCoords {
3;
0.0;0.0;,
1.0;0.0;,
0.0;1.0;,
";

	/// One quad: four vertices, one four-sided face.
	const QUAD: &[u8] = b"xof 0302txt 0064
Mesh {
4;
0.0;0.0;0.0;,
1.0;0.0;0.0;,
1.0;1.0;0.0;,
0.0;1.0;0.0;,
;
1;
4;0,1,2,3;,
 MeshTextureCoords {
4;
0.0;0.0;,
1.0;0.0;,
1.0;1.0;,
0.0;1.0;,
";

	fn position(model: &ModelData, vertex: usize) -> (f32, f32, f32) {
		let base = vertex * 3;
		(
			model.vertex_ary[base],
			model.vertex_ary[base + 1],
			model.vertex_ary[base + 2],
		)
	}

	fn texcoord(model: &ModelData, vertex: usize) -> (f32, f32) {
		let base = vertex * 2;
		(model.tex_coord_ary[base], model.tex_coord_ary[base + 1])
	}

	#[test]
	fn a_missing_magic_code_is_not_a_model() {
		assert!(parse_model_file(b"not an x file at all\n").is_err());
		assert!(parse_model_file(b"").is_err());
		// The magic has to be at the very start, not merely present.
		assert!(parse_model_file(b" xof 0302txt 0064\n").is_err());
	}

	#[test]
	fn a_triangle_expands_to_a_six_vertex_strip() {
		let model = parse_model_file(TRIANGLE).expect("parse");
		assert_eq!(model.polygons, 1);
		assert_eq!(model.vertex_count(), 6);
		assert_eq!(model.vertex_ary.len(), 18);
		assert_eq!(model.tex_coord_ary.len(), 12);

		// The face names vertices 0, 1, 2 and the upstream order is a a c b b b: the last
		// two are the same vertex, which is what makes the join to the next polygon
		// degenerate rather than a triangle across the model.
		let a = (0.0, 0.0, 0.0);
		let b = (-1.0, 0.0, 0.0);
		let c = (0.0, 1.0, 0.0);
		assert_eq!(position(&model, 0), a);
		assert_eq!(position(&model, 1), a);
		assert_eq!(position(&model, 2), c);
		assert_eq!(position(&model, 3), b);
		assert_eq!(position(&model, 4), b);
		assert_eq!(position(&model, 5), b);

		// X is negated on the way in, because the model data is left handed.
		assert_eq!(position(&model, 3).0, -1.0);
	}

	#[test]
	fn texture_coordinates_follow_the_same_vertices() {
		let model = parse_model_file(TRIANGLE).expect("parse");
		assert_eq!(texcoord(&model, 0), (0.0, 0.0));
		assert_eq!(texcoord(&model, 1), (0.0, 0.0));
		assert_eq!(texcoord(&model, 2), (0.0, 1.0));
		assert_eq!(texcoord(&model, 3), (1.0, 0.0));
		assert_eq!(texcoord(&model, 4), (1.0, 0.0));
		assert_eq!(texcoord(&model, 5), (1.0, 0.0));
	}

	#[test]
	fn a_quad_uses_the_other_expansion_order() {
		let model = parse_model_file(QUAD).expect("parse");
		assert_eq!(model.polygons, 1);
		// a a d b c c, so slot 4 comes third.
		let a = (0.0, 0.0, 0.0);
		let b = (-1.0, 0.0, 0.0);
		let c = (-1.0, 1.0, 0.0);
		let d = (0.0, 1.0, 0.0);
		assert_eq!(position(&model, 0), a);
		assert_eq!(position(&model, 1), a);
		assert_eq!(position(&model, 2), d);
		assert_eq!(position(&model, 3), b);
		assert_eq!(position(&model, 4), c);
		assert_eq!(position(&model, 5), c);
	}

	#[test]
	fn the_material_arrays_are_uniform() {
		let model = parse_model_file(TRIANGLE).expect("parse");
		assert_eq!(model.color_ary.len(), 24);
		for step in 0..6 {
			assert_eq!(
				&model.color_ary[step * 4..step * 4 + 4],
				&[1.0, 1.0, 1.0, 1.0]
			);
			assert_eq!(
				&model.color_gray_ary[step * 4..step * 4 + 4],
				&[0.8, 0.8, 0.8, 1.0]
			);
		}
	}

	#[test]
	fn a_second_mesh_indexes_from_where_the_first_ended() {
		// The upstream accumulates vertices and indices across mesh blocks and offsets
		// the second block's face indices by the running vertex count. Getting this wrong
		// makes every mesh after the first reference the wrong vertices.
		let mut data = TRIANGLE.to_vec();
		data.extend_from_slice(
			b"Mesh {
3;
0.0;0.0;1.0;,
1.0;0.0;1.0;,
0.0;1.0;1.0;,
;
1;
3;0,1,2;,
",
		);

		let model = parse_model_file(&data).expect("parse");
		assert_eq!(model.polygons, 2);
		// The second mesh's first vertex landed at index 3, so the second polygon's `a`
		// is vertex 3 and its z is 1.
		assert_eq!(position(&model, 6), (0.0, 0.0, 1.0));
		assert_eq!(position(&model, 7), (0.0, 0.0, 1.0));
		// And `b` is vertex 4, whose x is negated from 1.0.
		assert_eq!(position(&model, 9), (-1.0, 0.0, 1.0));
	}

	#[test]
	fn a_model_with_no_faces_is_empty_rather_than_broken() {
		let data = b"xof 0302txt 0064\nMesh {\n0;\n;\n0;\n";
		let model = parse_model_file(data).expect("parse");
		assert_eq!(model.polygons, 0);
		assert!(model.vertex_ary.is_empty());
		assert!(model.color_ary.is_empty());
	}

	#[test]
	fn the_table_hands_out_the_first_free_slot_and_takes_it_back() {
		let mut table = ModelTable::new();
		assert_eq!(table.get_total(), 0);

		// A slot can only be filled through a file, so drive the slot bookkeeping directly
		// through the same path a load would take.
		table.slots[0] = Some(parse_model_file(TRIANGLE).expect("parse"));
		table.slots[1] = Some(parse_model_file(QUAD).expect("parse"));
		assert_eq!(table.get_total(), 2);
		assert!(table.get(0).is_some());
		assert!(table.get(2).is_none());
		// Out of range ids are refused rather than panicking.
		assert!(table.get(-1).is_none());
		assert!(table.get(MAX_MODEL as i32).is_none());

		table.cleanup(0);
		assert_eq!(table.get_total(), 1);
		assert!(table.get(0).is_none());
		// The freed slot is the first one handed out again.
		assert_eq!(table.free_slot(), Some(0));

		table.cleanup_all();
		assert_eq!(table.get_total(), 0);
	}

	#[test]
	fn morphing_averages_positions_and_keeps_the_first_material() {
		let mut table = ModelTable::new();
		table.slots[0] = Some(parse_model_file(TRIANGLE).expect("parse"));
		// A second model with the same shape moved along z.
		let moved: &[u8] = b"xof 0302txt 0064
Mesh {
3;
0.0;0.0;2.0;,
1.0;0.0;2.0;,
0.0;1.0;2.0;,
;
1;
3;0,1,2;,
";
		table.slots[1] = Some(parse_model_file(moved).expect("parse"));

		let id = table.morph(0, 1);
		assert_eq!(id, 2);
		let morphed = table.get(id).expect("morphed model");
		assert_eq!(morphed.polygons, 1);
		// Position is the midpoint, so z is 1 on every vertex.
		for vertex in 0..6 {
			assert_eq!(position(morphed, vertex).2, 1.0);
		}
		// The material comes from A, not from an average.
		assert_eq!(&morphed.color_ary[0..4], &[1.0, 1.0, 1.0, 1.0]);
		assert_eq!(&morphed.color_gray_ary[0..4], &[0.8, 0.8, 0.8, 1.0]);
	}

	#[test]
	fn morphing_refuses_a_polygon_count_mismatch() {
		// Two faces against one, so the counts genuinely differ. A quad against a
		// triangle would not do: both are a single face and the upstream only compares
		// the face count, not the vertex count its own documentation asks for.
		let two_faces: &[u8] = b"xof 0302txt 0064
Mesh {
4;
0.0;0.0;0.0;,
1.0;0.0;0.0;,
1.0;1.0;0.0;,
0.0;1.0;0.0;,
;
2;
3;0,1,2;,
3;0,2,3;,
";

		let mut table = ModelTable::new();
		table.slots[0] = Some(parse_model_file(TRIANGLE).expect("parse"));
		table.slots[1] = Some(parse_model_file(two_faces).expect("parse"));
		assert_eq!(table.get(0).expect("model").polygons, 1);
		assert_eq!(table.get(1).expect("model").polygons, 2);
		assert_eq!(table.morph(0, 1), -1);
		// And refuses ids that hold nothing.
		assert_eq!(table.morph(0, 5), -1);
		assert_eq!(table.morph(-1, 0), -1);
	}

	#[test]
	fn a_quad_and_a_triangle_morph_because_only_the_face_count_is_checked() {
		// The documented requirement is matching vertex, polygon and index counts, but the
		// code only compares polygon counts. Keeping the behaviour matters: it is what
		// decides whether a mission's morphing models load at all.
		let mut table = ModelTable::new();
		table.slots[0] = Some(parse_model_file(TRIANGLE).expect("parse"));
		table.slots[1] = Some(parse_model_file(QUAD).expect("parse"));
		assert_eq!(table.morph(0, 1), 2);
	}

	#[test]
	fn loading_a_missing_file_fails_with_the_upstream_code() {
		let mut table = ModelTable::new();
		assert_eq!(
			table.load(std::path::Path::new("/nonexistent/xops.none")),
			-1
		);
		assert_eq!(table.get_total(), 0);
	}
}
