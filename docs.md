# DOASCII

A playable DoomGeneric port rendered in a Windows terminal, with a Vey-powered
feature-assisted controller. These instructions are written for **Windows CMD**.

## Install and run

### 1. Install the prerequisites

Install Rust with `rustup` and install [uv](https://docs.astral.sh/uv/) (used to
create the Python environment for Vey). DOASCII uses the included Freedoom IWAD
when it is present. If the asset is missing, obtain a compatible IWAD legally
and set `DOASCII_IWAD` as shown below.

From the project root, initialize the Doom source submodule:

```cmd
git submodule update --init --recursive
```

### 2. Install the pinned Vey runtime

Vey requires Python 3.10 or newer. The following creates a project-local
Python 3.11 environment and installs the exact Vey source revision used by
this adapter:

```cmd
uv venv .vey-venv --python 3.11
uv pip install --python .vey-venv\Scripts\python.exe -r tools\requirements-vey.txt
```

This installs Vey's Python dependencies, including PyTorch and Transformers.
For DOASCII's current numeric, structured decisions, Vey does **not** need to
fetch or load its learned-model weights. The Vey package is licensed
FSL-1.1-MIT; read its license before redistributing or offering Vey-based
functionality.

### 3. Start the game with Vey

In the same CMD window, from the project root:

```cmd
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
cargo run -- --vey
```

This opens the game in CMD and opens a **second terminal window** that streams
Vey's live decisions, observations, candidate actions, confidence, and
certificate evidence. Keep both windows open. Press `Q` in the game window to
quit. Close the decision viewer separately when finished.

To play manually without starting Vey:

```cmd
cargo run
```

To run the already-built executable instead of Cargo:

```cmd
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
target\debug\doascii.exe --vey
```

### 4. Use another IWAD (optional)

If no local Freedoom asset is present, set the path to a compatible IWAD before
starting DOASCII:

```cmd
set DOASCII_IWAD=C:\Games\freedoom1.wad
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
cargo run -- --vey
```

You can also pass the IWAD explicitly to the executable:

```cmd
target\debug\doascii.exe --iwad "C:\Games\freedoom1.wad" --vey
```

### 5. Build and test

```cmd
cargo fmt --check
cargo test
cargo build
cargo check
.vey-venv\Scripts\python.exe -m unittest discover -s tools -p test_vey_adapter.py
```

The local Vey environment and runtime decision log are ignored by Git. The
Vey trace is written to `.doascii\vey-decisions.log`.

## Controls

| Key | Action |
| --- | --- |
| `W` / `Up` | Move forward |
| `S` / `Down` | Move backward |
| `A` / `D` | Strafe left / right |
| `Left` / `Right` | Turn |
| `Space` / `F` | Fire |
| `E` | Use / interact |
| `Esc` | Doom's in-game menu |
| `Q` | Quit the terminal client |

Windows consoles may only repeat the most recently pressed key. DOASCII drains
queued keyboard events, refreshes held-key timeouts when other keys repeat, and
reference-counts aliases such as `W` and `Up`; releasing one alias therefore
does not cancel the other.

## What the Vey controller does

Vey mode is opt-in (`--vey`) and leaves human input enabled. The controller is
not a generic vision model or a full Doom speedrun agent. Vey never receives
ASCII or framebuffer pixels. The Doom engine provides typed facts, and the Rust
controller constructs a small, closed set of actions from those facts.

The current policy supports:

- **Explore:** move forward when Doom's collision checks report a clear path.
  When the route is blocked, compare the open distance at 45 degrees left and
  right, then turn toward the better opening. Back up only when a reverse path
  is clear and no better turn is available.
- **Aim:** when living enemies are visible off-center, turn toward the nearest
  visible enemy.
- **Attack:** fire only when a living enemy's visible box crosses the
  horizontal aim line and the current weapon has ammo. Fist/chainsaw attacks
  are offered only for sufficiently large, nearby enemy boxes.
- **Approach / retreat:** approach a centered distant enemy when the forward
  clearance is safe; retreat from a very close enemy at low health only when
  the reverse clearance permits it.
- **Wait / abstain:** do nothing for dead players, no safe candidate, low Vey
  confidence, non-`structured` decision lanes, adapter errors, invalid actions,
  stale replies, or failed protocol checks.

Candidate descriptions contain measured numeric facts such as
`tactical_priority` and `ammo_cost`. Vey's deterministic `structured` lane
ranks those facts; Rust accepts only a confident result whose action ID is in
the current allowlist. A Vey action is translated locally to a short, bounded
Doom key pulse. Model output cannot provide arbitrary key codes or hold a key
indefinitely. Requests are rate-limited and run in a background worker, so a
slow decision does not freeze rendering or keyboard input.

This is a heuristic combat/exploration controller, not map-wide planning. It
can choose locally safe movement and work toward visible enemies, but it does
not yet build a complete map, plan a route to the exit, or guarantee level
completion. The terminal trace shows the facts and evidence behind each choice.

## Enemy visibility and density

Enemy highlighting is based on Doom's own renderer rather than color guesses.
During sprite rasterization the engine records opaque monster pixels that
survive per-column wall and silhouette clipping. The Rust renderer maps each
visible actor's bounds onto the terminal grid and draws a small red `M`/box
outline over the scene. The ASCII image remains visible inside the outline.

The headless port creates two additional legal spawn placements for each
map-placed, kill-counting monster, for up to roughly **three times the original
map population**. Extra placements are tested against Doom's collision and map
geometry; a crowded or enclosed spawn area can prevent all copies from being
placed. Player starts, pickups, decorations, and non-kill-counting objects are
not tripled. Doom's kill totals include successfully spawned copies.

## Rendering and terminal setup

- DoomGeneric renders at its original 320x200 logical resolution and scales the
  framebuffer to 640x400 for the bridge. Upscaling does not add world detail.
- On Windows, DOASCII tries a compact 4x6 console font and a 320x120 grid in a
  1280x720 console area. If the console rejects the font change, the existing
  font's cell dimensions determine the target. Other terminals use their
  current dimensions (with a 160x45 size request where supported).
- The view is centered and aspect-fitted using the measured character-cell
  aspect ratio. One terminal row is kept for controls and Vey status.
- Each output cell uses median sampled brightness, a per-frame 3rd–98th
  percentile contrast stretch, a short shade ramp, and the source palette
  color. Changed glyphs and colors are updated without clearing the entire
  terminal each frame.
- Enemy marker positions are transformed from framebuffer pixels to the
  fitted image area, so markers stay aligned when the terminal is resized.

References that informed the display fit and sampling choices:

- [Chafa manual](https://manpages.debian.org/bookworm/chafa/chafa.1.en.html)
  discusses character-cell aspect ratios, median color extraction, and a bottom
  safety margin.
- [ascii-image-converter](https://github.com/TheZoraiz/ascii-image-converter)
  documents aspect-preserving fit and short custom character maps.
- [textdoom](https://github.com/russellbrenner/textdoom) demonstrates
  texture-sampled ASCII/color rendering. It is browser-based, not a terminal
  renderer to copy directly.

## Architecture

```text
CMD game terminal                         decision-viewer terminal
  keyboard                                  append-only live tail
     |                                               ^
     v                                               |
Rust terminal client <---- Vey JSONL worker --------+
     |       ^                 |
     |       | DGF2 frames     | typed question, state, candidates
     v       |                 v
DoomGeneric worker             Vey Python runtime
     |                         structured lane
     +-- game simulation / framebuffer / visible actor bounds
```

Doom remains authoritative for movement, collisions, combat, health, ammo,
actor visibility, and game progress. The Python process is persistent and
communicates through schema-versioned JSON Lines. The binary framebuffer and
keyboard streams remain separate from Vey's JSON protocol and from the
terminal's stdout.

## DGF2 observation protocol

Each Doom frame is sent as one binary packet:

1. `DGF2` magic.
2. 12-byte header: framebuffer width and height (`u16` little-endian), pixel
   byte size (`u8`, currently 4), metadata length (`u16` little-endian), and a
   reserved byte.
3. `width × height` little-endian 32-bit RGB pixels.
4. Version-1 metadata, followed by zero or more visible-enemy rectangles.

The fixed metadata prefix contains the player state, enemy count, weapon, health,
armor, ammo (`-1` for unavailable/no-ammo weapon), Doom fixed-point world
coordinates, angle, game tic, and four directional movement-clearance readings, and the level's total kill count.
Each rectangle is four `u16` values (`x`, `y`, `width`, `height`) in framebuffer
pixels, with a maximum of 128 boxes. The Rust parser rejects unsupported sizes,
invalid metadata versions, and out-of-bounds rectangles. Legacy `DGF1` frames
remain readable, but do not contain observations or enemy markers.

The C engine gathers enemy boxes at the masked sprite-post stage after Doom's
wall clipping. The same bridge obtains player state directly from `player_t`.
For navigation clearance it probes forward, left/right at 45 degrees, and
backward in 16-unit steps with Doom's collision checker; probe pickup behavior
is disabled. The observation is generated from simulation/render state, never
inferred from ASCII.

## Enemy population adjustment

Map spawns tagged `MF_COUNTKILL` retain their original monster and receive up
to two additional monsters. Candidate locations are checked with Doom's
`P_CheckPosition`; copies inherit the monster type, facing, ambush flag, and
map spawn record. Failed candidate locations are skipped. Only successfully
spawned copies increment the total kill count. This happens while the map's
things are initialized, so player starts and pickups are unaffected.

This deliberately increases combat pressure. Use a compatible PWAD/IWAD that
can tolerate clustered monsters. The feature is currently a fixed 3x target;
it is not a run-time difficulty slider.

## Vey JSONL adapter

`tools/vey_adapter.py` accepts one request per input line and emits one response
per line. Request fields:

```json
{
  "schema": 1,
  "sequence": 12,
  "question": "Choose the action with the greatest tactical_priority, then the lowest ammo_cost.",
  "state": "health=100; forward_clearance=3",
  "candidates": {
    "wait": "tactical_priority 0; ammo_cost 0",
    "advance": "tactical_priority 80; ammo_cost 0"
  }
}
```

Responses include the same `schema` and `sequence`, selected `action` (or
`null`), `decision_mode`, trust state, and Vey's machine-evidence certificate.
The adapter reuses one CPU `vey.Runtime`, keeps diagnostic output off stdout,
validates the candidate set, and returns errors without inventing an action.
Rust checks sequence, schema, decision lane, confidence, and action allowlist
before injecting a short key pulse.

To exercise the adapter manually after installing Vey:

```cmd
set DOASCII_VEY_PYTHON=.vey-venv\Scripts\python.exe
.vey-venv\Scripts\python.exe tools\vey_adapter.py
```

Paste one JSON request per line. The response appears on the next line. Press
`Ctrl+Z`, then `Enter` to close stdin in CMD.

## Live Vey decision window

When `--vey` is used, Rust opens `.doascii\vey-decisions.log` and launches a
second PowerShell console running `Get-Content -Wait` on that file. Each
completed request logs:

- sequence number, selected action, decision lane, trust state, and margin;
- the decision question and structured state;
- every action candidate and its supplied numeric facts;
- the Vey decision certificate (program, evidence, and survivors), or an
  adapter error.

The log is reset at game start and appended/flushed after each decision. The
viewer is read-only; closing it does not stop the game or Vey. If the console
viewer cannot start, the game continues and the log remains available at the
path above.

## Troubleshooting

### The player keeps walking into a wall

Use `--vey` with the pinned Vey environment and restart the game. The current
DGF2 controller uses Doom collision probes rather than only detecting that the
player stopped. If it still repeats a turn, check that the trace window shows
nonzero/zero `forward_clearance`, `left_clearance`, and `right_clearance` values.
A stuck game can also be escaped manually with the arrow keys or `A`/`D`.

### The game stands still in Vey mode

Look at the lower-row Vey status and the second terminal. `VEY ERROR` usually
means the selected Python executable does not have the pinned `vey` package;
set `DOASCII_VEY_PYTHON` to `.vey-venv\Scripts\python.exe`. `VEY ABSTAINED` or
`VEY UNSUPPORTED LANE` means Rust deliberately ignored an uncertain or
non-structured result. In a dead state the controller does not move until Doom
accepts player respawn input.

### Enemies are not marked

Markers appear only for living kill-counting monsters that produce at least
one unoccluded opaque sprite pixel in Doom's rendered view. Enemies behind
walls, fully outside the view, corpses, non-counting actors, and actors clipped
to zero visible pixels are not marked. The first frame may have no visible
monster; turn or move to reveal one.

### The CMD view is too small or distorted

Resize the game window before launching. A monospace font is required. The
Windows client attempts a compact font, but the grid still follows the actual
console dimensions and reserves a controls row. Close old DOASCII windows and
restart after building; an old process continues to use its old executable.

### The second decision terminal did not open

Check `.doascii\vey-decisions.log`. PowerShell must be available for the
separate Windows viewer. The file is still written if the viewer fails.

### The WAD was not found

Set `DOASCII_IWAD` to a compatible IWAD or pass `--iwad <path>`. The default
Freedoom file is ignored by Git and may not be present in a fresh clone.

## Development and validation

Useful commands from the repository root:

```cmd
cargo fmt --check
cargo test
cargo build
cargo check
.vey-venv\Scripts\python.exe -m unittest discover -s tools -p test_vey_adapter.py
.vey-venv\Scripts\python.exe -m py_compile tools\vey_adapter.py
target\debug\doascii.exe --check-engine
```

The first-frame smoke test verifies the `DGF2` packet, player health/armor/ammo,
frame dimensions, and worker startup. Rust unit tests cover legacy and current
frame parsing, bounds validation, red enemy markers, Vey candidate gating, and
exploration/turn selection. Python unit tests cover Vey response shape,
sequence handling, candidate validation, and malformed requests. A real
structured-lane smoke test can be run without model downloads:

```cmd
.vey-venv\Scripts\python.exe -c "import vey; r=vey.decide('Choose the action with the greatest tactical_priority, then the lowest ammo_cost.', {'wait':'tactical_priority 0; ammo_cost 0','advance':'tactical_priority 55; ammo_cost 0'}, runtime=vey.Runtime(device='cpu'), explain=True); print(r.answer, r.decision_mode, r.trust)"
```

## Source, assets, and licenses

- `vendor/doomgeneric` is the `ozkl/doomgeneric` source submodule, licensed
  GPL-2.0. Preserve the upstream license and attribution and follow GPL
  obligations when distributing the combined work.
- Doom source does not include commercial game data. Use a legally obtained
  compatible IWAD or the freely licensed Freedoom IWAD.
- Vey is pinned in `tools/requirements-vey.txt` to source revision
  `e6b046ffbd138cbdbfb2f89c6ae77525fe6b0b18`, under FSL-1.1-MIT. Its
  restriction window and commercial-use terms are described by Vey upstream.
  The Vey brand/name is separately trademarked.
- The `.doascii` runtime log and `.vey-venv` environment are local artifacts
  and are intentionally ignored by Git.

## Known limitations and next steps

- The ASCII grid is lower resolution than Doom's framebuffer; fine texture
  detail will be lost even though enemy markers remain explicit.
- The controller's movement probes are local clearance measurements, not a
  global map or route planner. It does not yet reason about locked doors,
  keys, switches, lifts, secrets, or map exits.
- Enemy classification currently uses living `MF_COUNTKILL` actors; unusual
  non-counting hostiles may not receive markers.
- Extra monster placements are best-effort legal positions around each original
  map spawn and can be fewer than two in constrained geometry.
- Audio output, save/load controls, a configurable enemy multiplier, and a
  complete map-navigation policy remain future work.
