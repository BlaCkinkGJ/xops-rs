//! Renderer tests that read the frame back and check where the geometry landed.
//!
//! Every one of these started as a real defect in some form: geometry transformed by the
//! wrong stack, a winding that culled the world away, a mid-frame depth clear that did
//! not open a new pass. They are cheap and they fail loudly, which reading a diff does
//! not.

use xops_platform::gfx::{Gfx, Vertex};
use xops_platform::gl;

const SIZE: u32 = 64;

const CLEAR: [u8; 4] = [0, 0, 0, 255];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn headless() -> Gfx {
	let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
		backends: wgpu::Backends::all(),
		..Default::default()
	});
	Gfx::new(&instance, None, SIZE, SIZE, None).expect("a headless renderer")
}

fn vertex(x: f32, y: f32, z: f32, colour: [f32; 4]) -> Vertex {
	Vertex {
		position: [x, y, z],
		uv: [0.0, 0.0],
		colour,
	}
}

/// A triangle covering the middle of the target, apex up in NDC.
fn triangle(z: f32, colour: [f32; 4]) -> Vec<Vertex> {
	vec![
		vertex(-0.8, -0.8, z, colour),
		vertex(0.8, -0.8, z, colour),
		vertex(0.0, 0.8, z, colour),
	]
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
	let index = ((y * SIZE + x) * 4) as usize;
	[
		pixels[index],
		pixels[index + 1],
		pixels[index + 2],
		pixels[index + 3],
	]
}

fn render(gfx: &mut Gfx, body: impl FnOnce(&mut Gfx)) -> Vec<u8> {
	gfx.begin_frame();
	gfx.clear_colour([0.0, 0.0, 0.0, 1.0]);
	gfx.clear(gl::GL_COLOR_BUFFER_BIT | gl::GL_DEPTH_BUFFER_BIT);
	body(gfx);
	gfx.end_frame().expect("encode frame");
	let (_, _, pixels) = gfx.read_frame().expect("read frame");
	pixels
}

#[test]
fn a_clear_colour_reaches_the_target() {
	let mut gfx = headless();
	gfx.begin_frame();
	gfx.clear_colour([1.0, 0.0, 0.0, 1.0]);
	gfx.clear(gl::GL_COLOR_BUFFER_BIT | gl::GL_DEPTH_BUFFER_BIT);
	gfx.end_frame().expect("encode");
	let (width, height, pixels) = gfx.read_frame().expect("read");
	assert_eq!((width, height), (SIZE, SIZE));
	assert_eq!(pixel(&pixels, 0, 0), [255, 0, 0, 255]);
	assert_eq!(pixel(&pixels, SIZE - 1, SIZE - 1), [255, 0, 0, 255]);
}

#[test]
fn geometry_lands_where_the_transform_stack_puts_it() {
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
	});

	// The centre is inside the triangle, so it takes the vertex colour. The white
	// texture multiplies by one and leaves it alone.
	assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 255, 0, 255]);
	// A corner is outside it and keeps the clear colour.
	assert_eq!(pixel(&pixels, 1, 1), CLEAR);

	// NDC +y is the top of the target, in GL and in the framebuffer here alike. The apex
	// is at NDC y = 0.8, which is row 6.4, and the base is at NDC y = -0.8, which is row
	// 57.6. So the middle rows are covered and the outermost ones are not, and if the
	// vertical mapping ever flips, the two end assertions swap over.
	assert_eq!(
		pixel(&pixels, SIZE / 2, 20),
		[0, 255, 0, 255],
		"upper middle"
	);
	assert_eq!(
		pixel(&pixels, SIZE / 2, 45),
		[0, 255, 0, 255],
		"lower middle"
	);
	assert_eq!(pixel(&pixels, SIZE / 2, 3), CLEAR, "above the apex");
	assert_eq!(pixel(&pixels, SIZE / 2, 62), CLEAR, "below the base");
}

#[test]
fn translating_the_projection_moves_the_geometry() {
	let mut gfx = headless();
	// Shifting the projection is the same as shifting the world, and it exercises the
	// second matrix stack the way the game's camera setup does. At the middle row the
	// triangle then spans a little past 0.8 in NDC, so the centre is bare and the right
	// edge is covered.
	let pixels = render(&mut gfx, |gfx| {
		gfx.matrix_mode(gl::GL_PROJECTION);
		gfx.push_matrix();
		gfx.translate(1.2, 0.0, 0.0);
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
		gfx.pop_matrix();
	});
	assert_eq!(
		pixel(&pixels, SIZE / 2, SIZE / 2),
		CLEAR,
		"pushed clear of the centre"
	);
	assert_eq!(
		pixel(&pixels, 60, SIZE / 2),
		[0, 255, 0, 255],
		"and visible on the far side"
	);
}

#[test]
fn popping_the_matrix_puts_geometry_back() {
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.matrix_mode(gl::GL_PROJECTION);
		gfx.push_matrix();
		gfx.translate(1.6, 0.0, 0.0);
		gfx.pop_matrix();
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
	});
	assert_eq!(
		pixel(&pixels, SIZE / 2, SIZE / 2),
		[0, 255, 0, 255],
		"back at the centre"
	);
}

#[test]
fn the_depth_test_keeps_the_nearer_draw() {
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.enable(gl::GL_DEPTH_TEST);
		// GL clip z runs -1 (near) to 1 (far), which is what the shader's remap assumes.
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.5, RED));
		gfx.draw(gl::GL_TRIANGLES, &triangle(-0.5, BLUE));
	});
	assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 0, 255, 255]);
}

#[test]
fn a_mid_frame_depth_clear_opens_a_new_pass() {
	// This is the sky: the game draws it, then clears the depth buffer so the world is
	// not occluded by it. Ignoring that clear leaves everything behind the sky hidden.
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.enable(gl::GL_DEPTH_TEST);
		gfx.draw(gl::GL_TRIANGLES, &triangle(-0.5, RED));
		gfx.clear(gl::GL_DEPTH_BUFFER_BIT);
		// Farther away than the red triangle, and drawn after the depth buffer was
		// cleared, so it must win.
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.5, GREEN));
	});
	assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [0, 255, 0, 255]);
}

#[test]
fn a_colour_only_clear_leaves_the_depth_buffer_alone() {
	// The complement of the test above: a clear that does not name the depth buffer must
	// not reset it. If it did, the sky fix would pass for the wrong reason.
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.enable(gl::GL_DEPTH_TEST);
		gfx.draw(gl::GL_TRIANGLES, &triangle(-0.5, RED));
		gfx.clear(gl::GL_COLOR_BUFFER_BIT);
		// Farther away, and the depth buffer still holds the red triangle's depth, so
		// this is rejected and the centre keeps the colour the clear left behind.
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.5, GREEN));
	});
	assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), CLEAR);
}

#[test]
fn blending_mixes_with_what_is_already_there() {
	let mut gfx = headless();
	let pixels = render(&mut gfx, |gfx| {
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
		gfx.enable(gl::GL_BLEND);
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, [1.0, 0.0, 0.0, 0.5]));
	});
	// Half the red over the green: the green channel keeps half its value.
	let centre = pixel(&pixels, SIZE / 2, SIZE / 2);
	assert!(
		centre[0] >= 127 && centre[1] >= 127 && centre[2] == 0,
		"{centre:?}"
	);
}

#[test]
fn culling_drops_the_winding_it_is_told_to() {
	let mut gfx = headless();
	// The same triangle with each winding named as the front face. Its vertices run
	// counter-clockwise in NDC. Naming GL_CCW as the front face keeps it and naming
	// GL_CW culls it, which is what the browser port found too and what its notes record:
	// the mapping is straight through, even though the argument from window coordinates
	// against framebuffer coordinates says it should be inverted. The argument was
	// settled by rendering there, and it is settled by rendering here, so the assertion
	// is the record.
	let ccw = render(&mut gfx, |gfx| {
		gfx.enable(gl::GL_CULL_FACE);
		gfx.front_face(gl::GL_CCW);
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
	});
	let cw = render(&mut gfx, |gfx| {
		gfx.enable(gl::GL_CULL_FACE);
		gfx.front_face(gl::GL_CW);
		gfx.draw(gl::GL_TRIANGLES, &triangle(0.0, GREEN));
	});

	assert_eq!(
		pixel(&ccw, SIZE / 2, SIZE / 2),
		[0, 255, 0, 255],
		"front face kept"
	);
	assert_eq!(pixel(&cw, SIZE / 2, SIZE / 2), CLEAR, "back face culled");
}
