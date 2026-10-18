// One shader module serves every draw.
//
// Only what the GPU has to bake into a pipeline varies with render state (depth
// handling and blending), so the things the game flips per draw - the alpha test, fog,
// textured against untextured - are uniform data or a bind group switch and do not
// multiply the pipeline count. Untextured draws bind a 1x1 white texture, which leaves
// the vertex colour untouched under multiply.
//
// Lifted from the browser port's renderer, which was verified against it pixel for
// pixel; the z remap and the view depth below are what make that comparison hold.

struct Uniforms {
	transform : mat4x4<f32>,	// projection * modelview
	modelview : mat4x4<f32>,
	fog : vec4<f32>,		// rgb = colour, a > 0.5 = enabled
	fogRange : vec4<f32>,		// x = start, y = end
	params : vec4<f32>,		// x = alpha reference, 0 = no alpha test
};

@group(0) @binding(0) var<uniform> u : Uniforms;
@group(1) @binding(0) var tex : texture_2d<f32>;
@group(1) @binding(1) var smp : sampler;

struct VSOut {
	@builtin(position) pos : vec4<f32>,
	@location(0) uv : vec2<f32>,
	@location(1) colour : vec4<f32>,
	@location(2) viewdepth : f32,
};

@vertex
fn vs_main(@location(0) position : vec3<f32>,
	   @location(1) uv : vec2<f32>,
	   @location(2) colour : vec4<f32>) -> VSOut {
	var out : VSOut;
	let world = vec4<f32>(position, 1.0);
	var clip = u.transform * world;
	// GL clip space puts depth in [-1, 1] and the modern APIs want [0, 1]. The matrices
	// here stay GL-shaped on purpose (glGetFloatv hands them to the game, and
	// gluUnProject reads them back), so the remap happens here, on z only, and nothing
	// else moves.
	clip.z = 0.5 * clip.z + 0.5 * clip.w;
	out.pos = clip;
	out.uv = uv;
	out.colour = colour;
	out.viewdepth = -(u.modelview * world).z;
	return out;
}

@fragment
fn fs_main(in : VSOut) -> @location(0) vec4<f32> {
	var colour = textureSample(tex, smp, in.uv) * in.colour;
	if (u.params.x > 0.0 && colour.a < u.params.x) { discard; }
	if (u.fog.a > 0.5) {
		let span = max(u.fogRange.y - u.fogRange.x, 0.001);
		let amount = clamp((in.viewdepth - u.fogRange.x) / span, 0.0, 1.0);
		colour = vec4<f32>(mix(colour.rgb, u.fog.rgb, amount), colour.a);
	}
	return colour;
}
