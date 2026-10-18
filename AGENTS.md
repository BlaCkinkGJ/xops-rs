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
- **A path built from a fixed buffer needs its C string taken first.** The menu's
  demo path is a `[u8; MAX_PATH]`, which is mostly terminators, and appending the
  array whole puts one in the middle of the path, so the game opens the directory
  and the load fails naming `data/map5/` rather than the file. Take each part up to
  its terminator when joining, as `strcat` does.
- **Upstream drops the depth test for the effect pass in the OpenGL backend and
  only drops the depth writes in the DirectX one.** The port keeps the test.
  Rendering the two the same way makes effects show through walls.
- **`get_input_state` is called once per frame, by the screen.** It copies this
  frame's keys over the previous frame's, so a second call in the same frame makes
  the two identical and every `CheckKeyDown` false: menus then move on held keys
  only, and one-shot keys like Esc and F1 and F11 do nothing at all. The loop used
  to read it before handing the screen a state it had already advanced, which looks
  correct at both ends and is not. Upstream's loop does not read it either, and the
  briefings and the result screen reach it through the shared scene base.
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

- **`std::thread::sleep` does not last what it is asked for on macOS.** The system
  coalesces timers, so a request for 30 ms came back after 80 and the game paced
  itself at a third of its rate. `host::sleep_millis` sleeps in small steps and
  re-reads the clock, which keeps the total honest. Anything that waits for a
  fraction of a frame goes through it.
- **Movement must not be read from the cursor's position.** The game warps the
  cursor to the middle of the window every frame and reads how far it went, so the
  warp itself is a move: the player's `+m` and the warp's `−m` cancel and the view
  cannot move at all. The raw device events carry the mouse's own motion and no warp
  produces them, which is why the movement comes from there. The cursor's position is
  still tracked, because the menus read that instead.

- **The game's screen size is its own, and everything drawn in screen
  coordinates is derived from it.** `D3DGraphics` keeps a width and height and
  works the HUD weapon's place out from them. The window is fitted to the
  display after the renderer is built, so those numbers move; `set_size` has to
  move the derived places with them, or the weapon is drawn for a screen that no
  longer exists. Upstream gets this for free by rerunning `InitD3D` on a
  resolution change.
- **Do not ask the window for a redraw every frame.** The game presents its own
  frame at the end of its own step, so the request asks for nothing, and it is
  answered immediately with a `RedrawRequested` that lands back in
  `about_to_wait`: the loop then runs around itself hundreds of thousands of
  times a second, one core at full tilt, while the frames it is meant to be
  pacing get what is left.
- **The cursor is held, and the game's own warp is skipped while it is.** macOS
  supports only `CursorGrabMode::Locked` (asking for `Confined` is refused, and
  discarding that error hides it), and `Locked` is what mouse look wants: the
  pointer stops moving the cursor and every mouse event carries the movement as
  a delta, which is where the game's movement comes from. Two traps come with
  it: the game asks for a cursor warp every frame, and on macOS that warp
  re-associates the pointer with the cursor, cancelling the hold it depends on;
  and the movement must be read from those deltas, never from the cursor's
  position, because the warp is itself a move.

- **Exactly one thing paces the frame.** The game's own frame rate control sleeps
  out the rest of each 30 ms frame, and the host waits until the next frame is
  due. Running both sleeps out two frames for every one and the game runs at two
  thirds of its rate -- which is what "the frame rate is too low" looked like
  here, with a step of 20 ms and a host wait of 15 ms in the same frame. The
  windowed host paces (`WaitUntil`), so its `step` call says so and the game's own
  control does not run; the offscreen run has no host, so there it does. The host
  must not poll instead: polling with the game's control disabled is a busy loop
  that runs the loop a hundred thousand times a second between frames.

- **The game's rate and the display's rate are different things.** The step is
  30 ms because every movement, animation and timer in the game is written per
  frame, so running the step faster runs the game faster. 30 ms is 33 frames a
  second and a 60 Hz display shows a frame for 16.7 ms, so the two never divide
  and the picture is uneven however fast the game is: the host draws the frame
  again between steps with the matrices blended, which is where smoothness comes
  from. The blend is linear, so it is not itself rigid; it is correct to about a
  tenth of a percent per frame at this game's turn rates.
- **One frame in flight.** `desired_maximum_frame_latency` decides both how long
  `get_current_texture` blocks, which is most of what a frame costs here, and how
  much input lag the queue adds: at two, a 30 ms game is up to 60 ms behind the
  mouse.
- **A sub-frame wait on this system is not a wait.** `ControlFlow::WaitUntil` with
  a period below about thirty milliseconds comes back after twenty or thirty
  regardless, so anything that wants to happen between the game's steps cannot be
  scheduled with it: it has to sleep in small slices and re-read the clock, which
  is what `host::sleep_until` is for. This is the same timer coalescing that makes
  a single `thread::sleep` lose most of its duration.
- **The interpolation factor is measured against the step's schedule, not against
  a clock started when the step ran.** The step costs a millisecond or two, so a
  clock started with it runs out early and the factor passes one and then jumps
  back to nought when the next step runs: the picture goes backwards once a step,
  which is what a stutter is. Against `next_frame - frame_step()` it cannot, and
  the trace shows the factor staying within nought to one with no repeats.
- **The step draws, the host shows.** The game's step used to present its own
  frame as well, which put an unblended frame at whatever phase it finished at in
  the middle of the frames drawn for the display. `Gfx::end_frame` now draws into
  the colour target and stops; `Gfx::present` is the only thing that shows a
  frame.
