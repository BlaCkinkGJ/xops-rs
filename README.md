# xops: OpenXOPS for Windows, Linux and macOS

An **unofficial** port of OpenXOPS 1.080, the open-source remake of
*X operations*, as an ordinary game program: one binary per platform, no browser,
no runtime to install beyond the graphics and audio drivers you already have.

This is not a reimplementation. The whole FPS is here: the opening, the mission
menu with its live demo, the briefing, the missions, the AI, collision, weapons,
bullet and grenade physics, effects, sound, the radar, the HUD and the debug
console. The upstream C++ was rewritten in Rust rather than wrapped, and only the
layer that talks to the operating system is new.

It ships no game data. You need *X operations* 0.96, which nine-two released as
freeware, and [LICENSE.md](LICENSE.md) has the terms before you redistribute
anything. Working on the code? [AGENTS.md](AGENTS.md) has the rules that are easy
to get wrong.

## Build

A Rust toolchain, and one system package on Linux.

```sh
cargo build --release
```

| Platform | Result | Notes |
| --- | --- | --- |
| Windows | `target\release\xops.exe` | MSVC toolchain, no extra packages |
| Linux | `target/release/xops` | `libasound2-dev` to build (the audio backend), plus your usual OpenGL or Vulkan driver at run time |
| macOS | `target/release/xops` | nothing extra |

The binary is about 7 MB and self-contained. The build takes well under a minute
on a laptop.

## Run

The game's own paths are relative to the directory it runs in, so the data goes
in a `data` folder beside it, as it does for the original.

```sh
./target/release/xops                              # opening, then the menu
./target/release/xops --mission missions/training.mif   # straight into a mission
./target/release/xops --frames 60 --out frames     # offscreen, no window
```

| Option | What it does |
| --- | --- |
| `--dir PATH` | The directory the game's own paths are relative to. The current directory by default |
| `--mission PATH` | Start in this mission instead of at the opening screen |
| `--frames N` | Render N frames offscreen, write them to `--out`, exit |
| `--out DIR` | Where those frames go, as PNGs |
| `--size WxH` | Frame size for both modes, 640x480 by default |

`data/` is the *X operations* data and must not be redistributed. `missions/`
holds two example missions written for this port, and they use the official
TRAINING YARD and BUILDING maps, so they run against the original data.

## Controls

The game's own, which `config.dat` rebinds. The keys that are not configurable:

| Key | |
| --- | --- |
| `Esc` | Leave the current screen, back to the menu |
| `F1` | Third person: the camera pulls back over the player's shoulder |
| `F2` | Simplified HUD |
| `F11` | Debug console, and `help` lists its commands |

Movement, aiming and firing are the eighteen control keys in `config.dat`,
created with the original defaults on first run: `WASD` to move, mouse to look,
the mouse buttons to fire and aim, `R` to reload.

## Architecture

Three crates, and only the lowest one touches the operating system.

```mermaid
flowchart TB
	subgraph bin["crates/xops"]
		cli["the command line, the window, the frame loop"]
	end
	subgraph game["crates/xops-game"]
		top["gamemain · maingame · gamemenu · briefing · scheduler"]
		mid["object · objectmanager · ai · collision · event · human · weapon · parameter · datafile"]
	end
	subgraph plat["crates/xops-platform"]
		r["gfx - wgpu: a fixed function pipeline, the GL enum set, textures, matrices"]
		w["window · input · sound · fs · font · path"]
	end
	cli --> game
	top --> mid
	mid --> plat
```

The split is the one the upstream is written with: everything above the platform
layer is a translation of the original C++, file for file and function for
function, and `PORTING.md` records how that translation is done.

| Upstream | Here |
| --- | --- |
| `main.cpp`, `gamemain.cpp` | `crates/xops/src/main.rs`, `gamemain.rs`, `scheduler.rs` |
| `maingame.cpp`, `gamemenu.cpp`, `briefing.cpp` | `maingame.rs`, `gamemenu.rs`, `briefing.rs` |
| `object.cpp`, `objectmanager.cpp`, `ai.cpp`, `collision.cpp` | the same names |
| `d3dgraphics.cpp` | `d3dgraphics.rs` over `xops-platform`'s `gfx` |
| `datafile.cpp` | `datafile.rs` |
| `window.cpp`, `input.cpp`, `sound.cpp` | `window.rs`, `input.rs`, `sound.rs` in each crate |
| `d3dgraphics-opengl.cpp` and the Direct3D 9 backend | one wgpu renderer |

The renderer is the one large piece that is not a translation. Upstream draws
through Direct3D 9 or OpenGL 1.1's fixed function pipeline, which no current API
has, so the subset those two use is reimplemented on wgpu: `glBegin`-style draw
calls, the matrix stacks, the blend and cull states, the texture formats the game
loads. The game code above it calls the same functions it always did.

### Why wgpu

The graphics API is **wgpu**, and it is the whole of the engine: there is no scene
graph, no asset pipeline and no game loop of its own, because the game already has
all three and they are upstream's. What was needed is only what a platform layer
needs, a way to put triangles on the screen of whichever machine this is running
on.

- **It is the native API, not a browser one.** wgpu speaks Vulkan on Linux and
  Windows, Metal on macOS, and Direct3D 12 on Windows, whichever the driver
  offers. There is no WebGL or WebGPU anywhere in this build, and no browser.
- **It is the only cross-platform GPU API with first-class Rust support**, which
  matters because the whole game is Rust and the alternative is a C++ engine with
  a binding layer in front of it.
- **It is a thin abstraction.** The alternative candidates are engines: Bevy,
  Fyrox and the like bring their own ECS, their own asset system and their own
  opinions about how a frame is built. This game has all of that already, ported
  from upstream, and adopting an engine would mean rewriting it.
- **It costs nothing at build time.** One Cargo dependency, no SDK to install and
  no C toolchain beyond the one Rust already uses.
- **Its output was checked against the reference renderer** before this port grew
  the rest of the game, on identical recorded input, which is how the draw
  ordering, the winding and the uniform layout came to be trusted.

If a different choice were wanted, the seam is small and deliberate: everything
above `xops-platform`'s `gfx` module calls GL-shaped functions, so a different
backend would be a second implementation of that module rather than a rewrite of
the game.


## Frame

```mermaid
flowchart LR
	sky["sky box"] --> map["map blocks"] --> obj["humans, weapons, small objects, bullets, grenades"] --> fx["effects - billboards, depth tested, no depth writes"] --> hud["2D HUD, radar, menus"]
```

One pass is `Input`, `Process`, `Sound`, then one `RenderMain`, at the 30 ms step
the game's timers assume. The console's `speed` command repeats the first three
within a single drawn frame.

The game's step is the game's own rate, and the game is written for it: a frame
of movement is a frame's worth of movement, so running the step faster runs the
game faster. A display wants frames more often than that, though -- 33 frames a
second does not divide a 60 Hz refresh, so some frames are shown for one refresh
and some for two, which is what an uneven picture looks like even when the game
is keeping up. Between steps the host therefore draws the same frame again,
partway to the one before it, with each draw's matrices blended: the display gets
a frame every refresh and the simulation stays at its own rate. `XOPS_INTERPOLATE=0`
turns that off, which leaves exactly the frames the game makes.

`XOPS_TRACE=1` prints a line a second with the frame rate, the step's cost, the
mouse's movement as the platform received it and as the game took it, the player's
view angle, and where the HUD weapon was placed.

## Verified

Run, not read:

- The opening sequence, the menu with its demo, and a mission on the official
  TRAINING YARD map: the map, the player, other humans, the HUD panels, the
  weapon slot, the health readout and the mission clock.
- The debug console, over F11.
- The camera transform against the reference build's own recorded matrix: the
  rotation matches to three thousandths, and the map batches and their winding
  are identical.

And on Linux, in a container: the whole workspace compiles, and the suite runs
under a software Vulkan driver with the renderer tests included. A container with
no driver fails those instead, which is fifty-three red lines with one cause
behind them.

Windows is checked rather than run: the whole workspace type-checks for
`x86_64-pc-windows-msvc`. There is no Windows machine here to play it on, so what
is confirmed is that nothing in the Rust is platform specific beyond the font
paths, which all three platforms have.

## Known limitations

- **Sound is wired but not verified by ear.** The mixer, the cue lists and the
  device output all run, but nobody has listened to them on hardware.
- `config.dat` is written where the game runs, so a read-only directory means
  settings do not persist. The game still starts with its defaults.
- A system font is looked up by path for the Japanese in the menus and weapon
  names. A machine with none of the listed fonts runs the game with that text
  missing rather than refusing to start.
- `.mif` files are read, and the two in `missions/` are written for this port.
  The original's mission files are not.

## License

3-clause BSD, from the OpenXOPS Project. See [LICENSE.md](LICENSE.md): the full
text, the third-party parts that belong to nine-two and TENKUU, and the rule that
the original game's graphics data must not be redistributed.
