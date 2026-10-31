# DOASCII development notes

## Target architecture

Use a real Doom engine, not a Rust raycaster that imitates Doom. The Doom engine runs as a separate headless process. A Rust terminal client reads its framebuffer, converts pixels to ASCII, displays frames without clearing and repainting the whole terminal, and forwards human/model input back to the engine.

```text
keyboard or decision model
          | input events
          v
Rust terminal client <---- framebuffer protocol ----> headless Doom engine
     ASCII renderer                                  DoomGeneric
```

## Doom engine source

`vendor/doomgeneric` is a Git submodule pointing to [ozkl/doomgeneric](https://github.com/ozkl/doomgeneric), a portable Doom source port. Its platform layer exposes `DG_DrawFrame`, `DG_GetKey`, timing, and the rendered framebuffer, which are the hooks needed for a headless bridge.

The source is GPL-2.0. Keep the upstream license and attribution with the submodule and account for GPL obligations when distributing a combined build. The source port does not include Doom's commercial game data (IWAD/WAD); provide a legally obtained compatible IWAD path at runtime or use a freely licensed compatible IWAD such as Freedoom.

## Process boundary

The Doom process should own game simulation and render into its framebuffer. The Rust process should own terminal input, ASCII conversion, and terminal output. Define a framed local IPC protocol for framebuffer updates and key down/up events. Keep protocol IO off terminal stdout so logs cannot corrupt the frame stream. The Rust client should maintain terminal state and update changed cells instead of clearing the full screen every frame, avoiding flicker.

Vey runs independently as well. Later, the Rust client/controller will send structured observations or candidate actions to a separate Vey adapter and translate the selected action into the same input events used by a human. Doom remains authoritative for game state and input validity.

## Current status

- DoomGeneric source is included as a submodule at `vendor/doomgeneric` and is built by Cargo with the headless platform backend.
- Doom runs in a separate worker process. It streams `DGF1` framebuffer packets over stdout; the Rust terminal client sends two-byte key-down/key-up events over stdin.
- The framebuffer is 640x400, scaled from Doom's original 320x200 software view. The terminal client targets a 1280x720 console area: it tries a 4x6 Windows console font and a 320x120 grid, or derives a grid from the existing font when the font API is unavailable. On non-Windows terminals it requests 160x45. The ASCII view is aspect-fitted and centered using the character-cell width/height ratio (0.5 fallback on non-Windows); one bottom row is reserved for controls. Each cell uses median luminance sampling and a gentle midtone lift to keep textured shapes legible without turning the image into a long-ramp punctuation pattern. Changed cells are redrawn without clearing the screen every frame. The underlying Doom scene remains 320x200.
- The worker launches directly into E1M1 with `-warp 1 1`, skipping the title-screen attract demo.
- Freedoom 0.13.0's `freedoom1.wad` is available locally under `assets/freedoom/` and is ignored by Git. It is used by default when present; set `DOASCII_IWAD` or pass `--iwad <path>` to use another compatible IWAD.
- The real-engine first-frame smoke test passes. Vey service integration and audio output remain future work.

## Renderer references

- [Chafa manual](https://manpages.debian.org/bookworm/chafa/chafa.1.en.html): recommends a 1:2 font width/height ratio by default, supports median color extraction for crisper output, and reserves a bottom margin to prevent terminal scrolling.
- [ascii-image-converter](https://github.com/TheZoraiz/ascii-image-converter): recommends reducing font size or increasing terminal width, fitting to the available dimensions while preserving aspect ratio, and using a short custom brightness map.
- [textdoom](https://github.com/russellbrenner/textdoom): renders Doom-like scenes by sampling wall textures into ASCII/color cells rather than converting a full framebuffer to grayscale. Its implementation is browser canvas based, so it is a reference for rendering style, not a drop-in terminal renderer.

## Run the preview

```sh
cargo run
```

Controls: W/S move, A/D strafe, arrow keys turn, Space fires, E uses/interacts, Q exits the terminal client. Escape is passed to Doom for its in-game menu. The terminal grid limits visible ASCII detail; the framebuffer upscaling does not increase Doom's original 320x200 world-rendering detail.

## Next implementation steps

1. Improve the ASCII conversion for terminal legibility and add optional color/contrast modes.
2. Add audio support and more robust key repeat/release handling.
3. Define versioned observations and candidate-action requests for the separate Vey service.
4. Validate full human gameplay and model-controlled playthroughs across compatible IWADs.
