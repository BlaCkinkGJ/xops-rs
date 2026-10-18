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
- **No new dependency without a reason that survives being written down.**
  `xops-game` builds on the platform crate and nothing else, and that is worth
  keeping.
- `data/` stays out of git. The original game's graphics data must not be
  redistributed, and the upstream copyright banners stay in place: see
  `LICENSE.md`.

## Working

- `cargo test --workspace` is the suite, and it is fast. `cargo check --workspace
  --all-targets` must stay warning-free, on every target.
- **Most of the suite needs a GPU adapter**, because the renderer is what it is
  testing. A bare container has none, and there it looks like fifty-three
  failures rather than one cause: `Gfx::new` returns an error and every test that
  built a renderer panics at its `headless()` helper. Install a software driver
  (`mesa-vulkan-drivers` on Debian) and they run.
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
- **Upstream drops the depth test for the effect pass in the OpenGL backend and
  only drops the depth writes in the DirectX one.** The port keeps the test.
  Rendering the two the same way makes effects show through walls.
- **A screen that fails to load its data does not advance the state machine.** Its
  `create` returns early, before the click that would carry the state forward, so
  nothing is processed until something ends the run. What ends it is
  `WindowControl::close_requested`, which is the loop's own condition because that
  is the `WM_QUIT` upstream's `CloseWindow` posted. Without it the screen is rebuilt
  every frame and the same error is printed thirty times a second, which is what a
  real report looked like: hundreds of `block data open failed` lines, a window that
  answered nothing, and no clue which of the two was the cause.
- **The platform gathers mouse movement only while the game says it is looking.**
  `InputControl::get_input_state(input, mousemode)` is what says it, and a menu
  passes false because it wants the cursor's position instead. Nothing else sets
  that flag: leave it out and the game warps the cursor to the middle of the screen
  every frame while the platform drops every movement that comes with it, so the
  mouse turns nothing and the tests still pass, because they set the flag by hand.
- **The camera angle that looks like it should be negative is not.** `rx` in the
  debug overlay is positive, and reading that glyph as a minus cost a detour.
  When a matrix is available, compare against the matrix.
