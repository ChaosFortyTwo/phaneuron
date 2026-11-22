# TinyVoxel

A tiny, moddable, procedurally generated voxel world. One Rust code base (wgpu + winit), three builds:
Linux x86_64, Windows x86_64 and macOS Apple Silicon. A finished build is a few MB, so it fits on any drive.

## Play

| Key | Action |
| --- | --- |
| W A S D | walk |
| Mouse | look |
| Space | jump |
| Left Shift | sprint |
| Esc | release the mouse (press again to quit) |

The world is infinite and streams in as you walk. Change `seed` in `assets/config.toml` for a different one.

## Run from a drive

Each drive gets one folder from `dist/` (the binary, `assets/`, an empty `mods/`, `examples/`, `docs/`).
FAT32 works: the binary is a few MB and the biggest file is far below the 4 GiB per-file limit.

- **Windows:** double-click `tinyvoxel.exe`.
- **macOS:** open Terminal, `cd` to the folder, run `./tinyvoxel`. If Gatekeeper complains, run `xattr -dr com.apple.quarantine .` first.
- **Linux:** FAT32 has no executable bit and many desktops mount it `noexec`. Copy the folder to your home directory, or run `sh -c ./tinyvoxel` after `chmod +x`.
  You need a Vulkan-capable GPU driver.

Keep the `assets/` folder next to the executable; that is how the game finds its files.

## Build

```sh
cargo run --release          # run from the project folder
cargo test                   # logic tests: generation, meshing, physics, streaming
./package.sh                 # build + assemble dist/<your-os>/
```

Building all three at once is easiest with the included GitHub Actions workflow (`.github/workflows/build.yml`):
push the project and download the three artifacts. Cross-compiling the macOS build from another OS is possible but painful (Apple SDK), so building natively on a Mac is recommended.

## Modding

Everything the game shows is a plain file. See **`docs/asset-mapping.svg`** for the picture; the short version:

- `assets/blocks/<id>.toml` defines a block. A new file is a new block, no code needed.
- A PNG named `assets/textures/blocks/<id>.png` (or `_top` / `_side` / `_bottom`) replaces that block's flat color.
- `model = "models/blocks/<id>.gltf"` in the toml replaces the cube with a glTF model.
- `textures/ui/crosshair.png`, `textures/sky/sky.png` replace the crosshair and the sky gradient.
- `textures/{player,npc,enemy}/` and `sounds/` are reserved with the same rules for when those features land.
- `assets/config.toml` tunes the world, sky, rendering and player. Every key is optional.

Put mods in `mods/<name>/` using the same folder layout as `assets/`. Files in a mod override files with the same
path in `assets/`; mods load alphabetically and later ones win. `examples/crystal/` is a ready-made mod
(glTF model + PNG): copy it into `mods/` and follow its README.

glTF notes: origin is the bottom-center of the block, 1 unit = 1 block, triangle meshes only, embedded images are
ignored (textures come from PNG by name), flat shaded. Models are not merged with neighbours and collide as full cubes.

## Code map

| File | Job |
| --- | --- |
| `src/assets.rs` | finds files across `assets/` and `mods/*` |
| `src/config.rs` | `config.toml` settings |
| `src/blocks.rs` | block TOML, texture atlas, the PNG-over-color rule |
| `src/model.rs` | glTF loading |
| `src/world.rs` | infinite chunks, noise terrain, streaming |
| `src/mesh.rs` | chunk meshing with hidden-face culling |
| `src/player.rs` | gravity, jumping, collision |
| `src/render.rs` + `src/shaders/` | wgpu sky, terrain and UI passes |

## v1 limits (by design)

Walking only (no breaking/placing), rolling hills only, no audio playback yet, no entities yet.
