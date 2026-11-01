# DOASCII

so basically this is Doom running inside a terminal. not a screenshot of Doom and not just a video of Doom — the game is actually running, you can move around and shoot, and the renderer turns the real game frame into coloured ASCII characters.

The renderer is made from scratch for this project. Doom runs in the background through DoomGeneric, and DOASCII takes the framebuffer it produces and draws a fitted version of it in the terminal. The idea is to keep the game recognisable and playable even though the whole screen is made out of text.

There are also creative features mixed in, like enemy highlighting and a controller that can let Vey make decisions from facts supplied by the game. Vey is optional: you can just play normally with the keyboard, or start it in Vey mode and watch its decisions in another terminal.

And yeah, this is still a work in progress. The controller is not a magic Doom speedrunner and the ASCII view is never going to contain as much detail as the original framebuffer. But the game itself is running, the controls work, and the renderer and game bridge are built for this project.

## Install and run

These instructions are for **Windows CMD**. Start by opening CMD and moving into the folder where you want the project:

```cmd
cd C:\path\to\where\you\keep\projects
```

### 1. Install the tools

You need:

- **Git**, including Git support in CMD.
- **Rust** with Cargo. Install it using [rustup](https://rustup.rs/), then close and reopen CMD so `cargo` is available.
- A working C compiler for Rust's `cc` build dependency. The project prefers `C:\Program Files\LLVM\bin\clang.exe` when it exists. Otherwise install a compiler supported by Rust's `cc` crate, such as the C++ build tools from Visual Studio Build Tools.
- A compatible Doom IWAD file. Freedoom is a free option; commercial Doom data is not included in this repository.

Vey is only needed if you want the optional decision-making controller. Its extra Python setup is in step 5.

### 2. Get the project and Doom source

```cmd
git clone --recurse-submodules https://github.com/NeticYTOF/DOASCII.git
cd DOASCII
```

If you cloned it without `--recurse-submodules`, initialize the Doom source now:

```cmd
git submodule update --init --recursive
```

The submodule contains the DoomGeneric engine source used by the build. The Rust client and the engine bridge are in this repository; the submodule is still required to compile them together.

### 3. Point DOASCII at a game data file

DOASCII needs a Doom IWAD to load a level. The repository does not include commercial Doom data, and a fresh clone might not have a Freedoom file either. Get a compatible IWAD legally, then tell DOASCII where it is.

For example, if your IWAD is `C:\Games\freedoom1.wad`, set the environment variable in the CMD window you will use to start the game:

```cmd
set DOASCII_IWAD=C:\Games\freedoom1.wad
```

That setting lasts for the current CMD window. You can set it again in a new window, or pass the path directly as a command-line argument:

```cmd
cargo run -- --iwad "C:\Games\freedoom1.wad"
```

### 4. Build and play normally

From the project root:

```cmd
cargo run
```

The first build takes longer because Cargo compiles the Rust client and Doom engine. Later starts are quicker. The game opens in the terminal; if the text looks cramped, maximize the window and try a monospace font. The display scales to the actual terminal cells, so a wide terminal gives the renderer more room.

You can also build once and launch the executable directly:

```cmd
cargo build
target\debug\doascii.exe
```

If you set `DOASCII_IWAD`, keep using the same CMD window so that the game can find it. Otherwise pass `--iwad` to the executable:

```cmd
target\debug\doascii.exe --iwad "C:\Games\freedoom1.wad"
```

### 5. Optional: install and run Vey

Vey mode is optional. Normal keyboard play does not need Python, `uv`, PyTorch, or Vey installed.

For Vey mode, install [uv](https://docs.astral.sh/uv/) and Python 3.11, then from the project root make a local environment and install the pinned adapter requirements:

```cmd
uv venv .vey-venv --python 3.11
uv pip install --python .vey-venv\Scripts\python.exe -r tools\requirements-vey.txt
```

Start Vey and the game together:

```cmd
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
cargo run -- --vey
```

This opens the game and, when Windows allows it, a second terminal that follows Vey's live decision log. Keep both windows open. The viewer is just a readable trace; closing it does not stop the game. You can find the full log at `.doascii\vey-decisions.log`.

Vey currently uses its structured decision lane to rank a small set of actions described with measured game facts. It does not need to download model weights for this controller. The Python package has its own license and terms, so check those before redistributing it or offering Vey-based functionality.

### Handy command list

```cmd
cargo run
cargo run -- --vey
cargo run -- --iwad "C:\Games\freedoom1.wad"
cargo run -- --check-engine
cargo fmt --check
cargo test
cargo build
cargo check
```

To quit the game, press `Q` in the game terminal.

## Controls

| Key | What it does |
| --- | --- |
| `W` or `Up` | Move forward |
| `S` or `Down` | Move backward |
| `A` / `D` | Strafe left / right |
| `Left` / `Right` | Turn |
| `Space` or `F` | Fire |
| `E` | Use / interact |
| `Esc` | Doom's in-game menu |
| `Q` | Quit DOASCII |

You can move and turn at the same time. Windows terminals can be a bit weird about sending multiple held keys, so the client drains queued key events and tracks aliases separately. If a key gets stuck or movement feels wrong, try tapping the key again or using the arrow-key alternatives.

## What you see on screen

Doom renders a 320x200 game frame. DOASCII samples that image, fits it to the terminal's character-cell shape, and selects glyphs based on brightness. It keeps the image's color, uses a compact shade ramp and adjusts contrast for each frame. The terminal output is updated in place rather than clearing and redrawing the whole screen every time.

Character cells are not square, and terminal fonts vary, which is why simply mapping one pixel to one character tends to look like random letters or stretch the scene. DOASCII measures the terminal cell dimensions and fits the scene to the available area while leaving room for the controls/status line. A larger terminal usually gives a clearer view.

Enemies are highlighted using information from Doom's renderer. The engine reports monster sprite areas that remain visible after wall clipping, then the terminal renderer draws a red `M`/outline over the corresponding area. That means an enemy behind a wall or fully outside the view will not be highlighted; it is not an all-seeing enemy radar.

The current engine setup also tries to spawn two extra copies of each map-placed kill-counting monster, making the target population roughly three times the original. Spawns are checked against Doom's collision rules, so crowded or enclosed places can prevent some copies from appearing. This is a fixed gameplay change, not a difficulty setting.

## What Vey does (and does not do)

Vey mode is turned on with `--vey`; human keyboard input still works. Doom remains in charge of the game: it decides whether movement is possible, whether shots hit, where enemies are, how much health and ammo the player has, and what is visible.

The bridge sends structured observations such as player health, weapon, ammo, position, nearby enemy boxes, and short movement-clearance checks. Vey does **not** receive the ASCII screen or raw framebuffer as visual input. The Rust side creates a small list of allowed actions, and Vey ranks those actions using their supplied facts. Rust checks the returned action and only permits a short, bounded key pulse from that allowlist.

The current controller can:

- Move forward when Doom reports that the route ahead is clear.
- Turn toward a more open direction when movement is blocked.
- Aim at visible enemies and fire when the enemy is near the aim line and the weapon can shoot.
- Approach a distant visible enemy or back away from a very close threat in certain situations.
- Wait instead of acting when the state is unsafe, the response is uncertain, or the protocol does not check out.

The second terminal shows the facts and candidates behind each decision, along with the selected action and evidence from Vey. If that window does not open, look for the log at `.doascii\vey-decisions.log`.

This is local exploration and combat, not a complete map planner. It does not yet reliably plan routes to exits or reason its way through keys, switches, locked doors, lifts, and secrets. It may get stuck, and you can still take over with the keyboard.

## Project layout

```text
src/                         Rust terminal app, renderer, input, game bridge
native/                      C bridge between the client and DoomGeneric
vendor/doomgeneric/           DoomGeneric source submodule
  doomgeneric/                Engine source compiled into the game
  .git                        Submodule metadata
tools/                       Vey JSONL adapter and adapter tests
tools/requirements-vey.txt    Pinned Vey runtime requirements
docs.md                      More detailed internals and protocol notes
```

A quick version of how the pieces talk to each other:

```text
keyboard -> Rust terminal client -> DoomGeneric simulation
                     ^                    |
                     | framebuffer + facts|
                     +--------------------+
                     |
                     +---- optional JSONL ---- Vey controller
```

The game engine runs in a worker and sends frames and observations to the Rust client. The client handles the terminal, keyboard, rendering, and (when enabled) the Vey process. Vey communicates through line-delimited JSON; its diagnostic output is kept separate from the game's screen.

## Troubleshooting

### It says the IWAD cannot be found

Set `DOASCII_IWAD` in the same CMD window before running the game, or pass the path with `--iwad`. Check the spelling and make sure it points to the actual `.wad` file, not just the folder containing it. You need a compatible IWAD; the repository does not ship commercial Doom data.

### The build says DoomGeneric is missing

From the project root run:

```cmd
git submodule update --init --recursive
```

Then run `cargo build` again. Also make sure the compiler from the prerequisites is available to Cargo.

### The screen is tiny, stretched, or hard to read

Maximize the terminal and use a monospace font. DOASCII measures your terminal cells, but it cannot make a very small window contain more detail. Resize the terminal before starting the game if possible.

### Vey is standing still or keeps turning

Check that you started with `--vey` and set `DOASCII_VEY_PYTHON` to `.vey-venv\Scripts\python.exe`. Look at the Vey status in the game and the second terminal's log. `VEY ERROR` usually means Python or the pinned package could not start; an abstention means the controller rejected a response or chose not to act. You can always steer manually.

### There are no enemy markers

Markers only appear when a living monster has sprite pixels visible after Doom's wall clipping. Turn around or move to reveal enemies. Enemies behind walls, out of frame, or already dead are not marked.

### I changed the code but the old game is still running

Quit old DOASCII windows with `Q` before rebuilding. Windows can keep an executable locked while it is running, and an old open game will keep using the old build.

## Development

Run the Rust checks from the project root:

```cmd
cargo fmt --check
cargo test
cargo check
cargo build
```

If you installed Vey, run its adapter tests too:

```cmd
.vey-venv\Scripts\python.exe -m unittest discover -s tools -p test_vey_adapter.py
```

For engine startup and observation checks:

```cmd
target\debug\doascii.exe --check-engine
```

The adapter can also be run by itself. Start it, paste one JSON request per line, and it will print one JSON response per line:

```cmd
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
.vey-venv\Scripts\python.exe tools\vey_adapter.py
```

In CMD, send `Ctrl+Z` and then `Enter` to close its input.

More implementation detail — including the frame protocol, renderer choices, observation fields, enemy population change, and known limitations — lives in [`docs.md`](docs.md).

## Credits and licenses

- DoomGeneric is based on the DoomGeneric project and is included as a Git submodule. Keep its upstream attribution and license notices intact; the engine source is GPL-2.0.
- DOASCII does not include commercial Doom game data. Use a compatible IWAD that you have the right to use; Freedoom is a free option.
- Vey is an optional, separately licensed dependency. Its current source revision and license notes are recorded in `tools/requirements-vey.txt` and [`docs.md`](docs.md). Read the applicable terms before redistributing it.

This README is intentionally written in the same informal way as the project itself. If you are reviewing the project: sorry if that sounds a bit too casual — I just wanted the page to explain what it is without pretending the unfinished bits are finished.
