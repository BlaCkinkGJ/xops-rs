# AGENTS.md

OpenXOPS 1.080 as a cross-platform program: the upstream C++ game rewritten in
Rust, rendering through wgpu. `README.md` covers the build and the controls,
`LICENSE.md` the obligations that come with the upstream code. Both are short.
Read them before changing behaviour.

The rule that matters most: **the game's behaviour is upstream's, not ours.**
Porting means the same numbers, the same order of operations and the same edge
cases, including the ones that look like mistakes. `PORTING.md` records the
conventions that translation follows, and the upstream C++ is the reference when
the two disagree.

## Layout

| Path | What it is |
| --- | --- |
| `crates/xops/` | The binary: the command line and the frame loop |
| `crates/xops-game/` | The game: data formats, collision, AI, objects, screens, the scheduler |
| `crates/xops-platform/` | Everything that touches the OS: window, input, audio, files, fonts, the renderer |
| `data/` | The game's data. Not in git, must not be redistributed |
| `missions/` | Two example missions, our own |
| `web/` | The optional browser build: its page, and the scripts' output |
| `scripts/` | The web build. The desktop build is plain cargo |
| `vendor/openxops/` | The upstream sources, kept as the reference for behaviour |

## Rules

- **Collaborators are parameters, never stored borrows.** The renderer, the sound
  manager, the input state, the log and the parameter tables all arrive as
  arguments. A structure that stores one of them cannot be a sibling of the thing
  it borrows, and the game loop has to be exactly that. The same reason is why
  `SoundManager` holds only its cue lists and `World` is boxed.
- **The screens own the world.** `Stage` in `scheduler.rs` moves it from one
  screen to the next, because a screen lives for many frames and a borrow of the
  world cannot be stored beside the world itself. Do not add a second holder.
- **`datafile::path_from_bytes` is the only place a path is converted.** Game
  paths are Shift-JIS bytes with backslash separators. Unix gets those bytes
  unchanged, Windows gets them decoded, both get the slash. A `Path` built
  anywhere else will not resolve on Linux.
- **`web_time`, not `std::time`.** `std`'s clock and `Instant` are unimplemented
  on the web target and panic rather than returning something wrong, which costs
  an afternoon because the panic names the allocation, not the caller.
- **No new dependency without a reason that survives being written down.**
  `xops-game` builds on the platform crate and nothing else, and that is worth
  keeping.
- `data/` stays out of git. The original game's graphics data must not be
  redistributed, and the upstream copyright banners stay in place: see
  `LICENSE.md`.

## Working

- `cargo test --workspace` is the suite, and it is fast. `cargo check --workspace
  --all-targets` must stay warning-free, on every target.
- `cargo check -p xops --target wasm32-unknown-unknown` is the web build's gate,
  and `scripts/build_web.sh` builds it for real.
- **Verify by running it, not by reading the diff.** A change to rendering or the
  frame loop means launching the game; a change to a loader means loading a
  mission. `--frames N --out DIR` renders offscreen, which is how a difference
  gets compared rather than described.
- A test earns its place by catching a plausible mistake. Prefer one that fails
  before the fix and passes after it, and say in the name what it pins down.
  Tests that restate the implementation are worse than no test.

## Traps that cost real time

- **A large struct travelling by value overflows the stack in debug builds.** The
  world is 130 KB, and a debug build gives every moved temporary its own stack
  slot, so a dozen copies in one function's arms came to seven megabytes of
  frame. Box it. The release build hid this for a while.
- **A screenshot of a WebGL canvas comes back empty** unless the drawing buffer
  is preserved, so pixel checks on the browser build have to go through the
  compositor's screenshot rather than a canvas readback.
- **winit's web backend does not deliver the size the canvas actually has.** The
  canvas is created at the page's size and the platform resizes it to the window,
  so the loop tells the app once the window exists. Anything that reads the size
  before that sees the page's.
- **Upstream drops the depth test for the effect pass in the OpenGL backend and
  only drops the depth writes in the DirectX one.** The port keeps the test.
  Rendering the two the same way makes effects show through walls.
- **The camera angle that looks like it should be negative is not.** `rx` in the
  debug overlay is positive, and reading that glyph as a minus cost a detour.
  When a matrix is available, compare against the matrix.
