# PORTING.md

How the upstream C++ becomes Rust. This is the contract the game-logic port follows, so
that forty translation units written in parallel come out as one program.

## Layout

| Path | What it is | Replaces |
| --- | --- | --- |
| `crates/xops-platform/` | Window, input, audio, the wgpu renderer, math. The only crate that touches the OS or the GPU. | `window.cpp`, `input.cpp`, `sound-ezds.cpp`, `d3dgraphics-opengl.cpp`, `port/` |
| `crates/xops-game/` | The game: data files, collision, AI, objects, missions, menus. No GPU or window types. | `datafile.cpp`, `collision.cpp`, `ai.cpp`, `object.cpp`, `objectmanager.cpp`, `parameter.cpp`, `gamemain.cpp`, `scene.cpp`, `statemachine.cpp`, `event.cpp`, `resource.cpp`, `gui-object.cpp`, `debug.cpp`, `config.cpp`, `main.cpp` |
| `crates/xops/` | The binary: argument handling, the frame loop host. | `WinMain` in `main.cpp` |

`vendor/openxops/` stays as the reference. It is not edited further.

## What is fixed and must not drift

- **Every numeric constant.** Game feel is these numbers. `HUMAN_JUMP_SPEED 2.2f` stays
  `2.2`, and a constant named in the upstream headers keeps its name.
- **The 30 ms frame step.** `GAMEFRAMEMS` is 30, so the world advances by a fixed step and
  the display does not change it.
- **The uniform block is 176 bytes**: two `mat4x4`, then three `vec4`. The browser port's
  recorder writes that block and the parity harness compares against it.
- **`glFrontFace` maps straight through** to the front face, with no inversion. That was
  settled by rendering in the browser port and by the culling test here. The argument
  from window coordinates against framebuffer coordinates says otherwise and is wrong.
- **Client arrays only.** The game issues every draw as `glVertexPointer` plus
  `glDrawArrays`; there is no immediate mode to support.

## Conventions

**Names.** Upstream names are kept, in Rust's spelling: `SetPosData` becomes `set_pos_data`,
`HUMAN_JUMP_SPEED` becomes `HUMAN_JUMP_SPEED`. A reader can put the two files side by side
and follow them line for line. Naming things differently is how a port stops being
checkable.

**Out-parameters become returns.** Upstream is full of `void GetPosData(float*, float*,
float*, float*)`. Rust returns a tuple:

```rust
// upstream: void human::GetPosData(float *x, float *y, float *z, float *rx, float *ry)
pub fn pos_data(&self) -> (f32, f32, f32, f32, f32)
```

A function that fills a caller-provided buffer keeps taking `&mut` of that buffer when the
buffer is large or reused, because copying it out every frame is the kind of change that
shows up as a frame-rate regression.

**Strings are bytes, not `String`.** Upstream strings are Shift-JIS, and the texture font
indexes glyphs by byte value, so the bytes are the data. Game strings are `Vec<u8>` or a
fixed `[u8; N]`, never `String`. Decode with CP932 only when a value has to reach a
terminal or a log line.

**Fixed C arrays keep their bounds.** `float x[8]` becomes `[f32; 8]`, `char name[24]`
becomes `[u8; 24]`. Bounds are behaviour: the upstream code relies on truncation in places,
so widening one changes what the game does.

**Integers keep their width.** `signed short int` for the point parameters is `i16`; a
value that wraps upstream has to wrap here. Do not reach for `i32` because it is more
comfortable.

**Return codes are behaviour.** These functions return `int` where the value is a status
the caller branches on. Keep the codes and the branching.

**Polymorphism.** The object hierarchy (`object` → `human`, `weapon`, `smallobject`,
`bullet` → `grenade`, `effect`) is dispatched through a container of `object*` upstream.
Use `Box<dyn Object>` for that container and leave the virtual methods as trait methods
with the same signatures. Where the concrete type is known at the call site, call it
directly.

**Escaping the borrow checker is not allowed.** Upstream aliases freely, and the port will
hit cases where two objects need each other. Options, in order of preference: restructure
so the borrows are disjoint, pass the second object as a parameter, or split the struct.
`Rc<RefCell<_>>` is a last resort and needs a comment saying why.

**Globals become arguments.** Upstream has three: `OutputLog`, `GameState`, `GameConfig`.
They are passed as part of a context struct rather than made `static mut`.

## Verifying

- `cargo test --workspace` runs the renderer tests, which read pixels back and check where
  geometry landed. They run headless.
- `cargo run -p xops -- --frames N --out DIR` writes frames to PNG. That is the comparison
  input for the browser build's frames, and `renderer/compare.py` is the harness.
- Behaviour differences are found by playing, not by reading a diff. A mission that loads
  and can be walked through is the bar.
