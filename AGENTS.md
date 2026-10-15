# AGENTS.md

Unofficial browser port of OpenXOPS 1.080: the upstream C++ game compiled to
WebAssembly, rendering through WebGL on SDL2. `README.md` covers the build, the
runtime and the verification, `LICENSE.md` the obligations that come with the
upstream code. Both are short, read them before changing behaviour.

## Layout

| Path | What it is |
| --- | --- |
| `vendor/openxops/` | OpenXOPS 1.080 sources, patched in place, CP932 encoded |
| `port/` | The platform layer: SDL2 window, input and audio, Win32 shims, GLU, browser text |
| `scripts/` | Build, serve, vendor patching, syntax check, comment translation helper |
| `web/` | Page shell, build output, example missions, `data/` (git-ignored, user supplied) |

## Rules

- `vendor/openxops/` is upstream plus this port's patches. Every edit there sits
  behind `#ifdef __EMSCRIPTEN__` so the file still builds for Windows, and gets a
  matching entry in `scripts/patch-vendor.py`. That script is the record of the
  vendor changes: re-applying it to a pristine checkout has to reproduce them, so
  a vendor edit without a script entry is a bug. `--check` verifies a tree.
- Those vendored files are CP932 (Shift-JIS), and `read` and `edit` refuse them.
  Comment work goes through `python3 scripts/comment-tools.py dump|check|apply
  <file>`: it dumps the source to a UTF-8 copy with one `NNNN\t` prefixed line per
  source line, and writes the edited dump back as CP932. Keep every number
  present, in order, to keep the mapping exact. `check` lists the lines whose
  Japanese still needs work.
- Comment edits touch comments only. String literals stay byte-identical: they
  are Shift-JIS data the engine parses (config keys, mission and weapon names,
  console commands) and the texture font indexes glyphs by byte value.
- Comments are English, ASCII and terse, and keep their Doxygen tags.
- `web/data/` stays out of git. The original game's graphics data must not be
  redistributed, and the upstream copyright banners stay in place: see
  `LICENSE.md`.
- Behaviour fixes go into the shared game code path, not into special cases in
  the port layer. The port replaces the platform, nothing else.
- Fast loop is `python3 scripts/check.py` (per translation unit, a few seconds),
  then `bash scripts/build.sh` for a real build. Verify runtime changes by
  playing the build in a browser, not by reading the diff.

## WebGPU renderer traps

`port/gl-webgpu.cpp` sits closer to the metal than the GL emulation, so these
bite silently. Every one of them cost real time; all four look like "the draw
never happened".

- **A default-constructed WebGPU handle does not compare equal to `nullptr`.**
  The pipeline cache tested `slot == nullptr`, which was never true, so no
  pipeline was ever created: every draw bound a garbage pipeline,
  `SetPipeline` invalidated the command buffer, and the whole frame's draws
  were dropped without a word. The cache now keeps a separate `g_PipelineMade`
  flag. Test "created" with your own flag, never with the handle.
- **A WebGPU drop is silent unless you ask.** The device descriptor installs an
  uncaptured-error callback (`port/webgpu-core.cpp`); it is what turned
  "nothing renders" into a one-line `Invalid RenderPipeline` report. Keep it.
- **`glClear` inside a frame must open a new pass.** The game draws the sky and
  then calls `D3DGraphics::ResetZbuffer()` so the sky cannot occlude the level.
  Ignoring that clear (only the frame-opening one was handled) left the sky's
  depth in place and hid everything behind it.
- **GL decides winding in window coordinates (y up), WebGPU in framebuffer
  coordinates (y down), so `glFrontFace` maps to the opposite `frontFace`.** The
  game flips to `GL_CW` for the map strip and relies on culling; get this
  backwards and the map disappears while the sky survives.
- **Never change the GL texture binding behind the renderer's back.** It caches
  the id it last bound and skips redundant binds, so an upload that leaves its
  own texture bound makes later draws sample it (`port/font-web.cpp` saves and
  restores the binding around the font upload for exactly this reason).
