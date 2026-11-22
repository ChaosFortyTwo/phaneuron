# Phaneuron

A tiny, moddable, procedurally generated voxel world. One Rust code base (wgpu + winit), three builds:
Linux x86_64, Windows x86_64 and macOS Apple Silicon. A finished build is a few MB, so it fits on any drive.

## Playing

Start the game, then use the menu:

- **New World**: pick a name and a seed (empty = random; numbers are used as-is, other text is hashed).
- **Load World**: lists every folder in `saves/`; click one to play it. (The right-hand panel is reserved for later.)
- **Game Files**: opens the game folder (`assets/`, `mods/`, `saves/`) in your file manager.
- **Settings**: placeholder for now.

| Key / button | Action |
| --- | --- |
| W A S D | walk (one-block ledges are stepped up automatically) |
| Mouse | look |
| Space | jump |
| Left Shift | sprint |
| Left click (hold) | break the targeted block |
| Right click (hold) | place the selected block |
| 1 to 9 / mouse wheel | choose the block to place |
| Esc | pause: Resume, or Save and Quit to the menu |

Worlds autosave every minute and when you quit to the menu or close the window.

## Saves

```
saves/<world folder>/world.toml   name, seed, last played, player position (readable text)
saves/<world folder>/edits.bin    only the blocks you changed (a few bytes per edit)
```

Terrain is never stored. It is regenerated from the seed, so a world is tiny and
copying a folder between machines or drives just works. Block ids in `edits.bin` are stored by name, so
adding or removing mods does not scramble old worlds (blocks of a removed mod turn into air).

## Run from a drive

Each drive gets one folder from `dist/` (the binary, `assets/`, `mods/`, `saves/`, `examples/`, `docs/`).
FAT32 works: the binary is a few MB and nothing comes close to the 4 GiB per-file limit.

- **Windows:** double-click `phaneuron.exe`. SmartScreen may warn about an unsigned app: **More info -> Run anyway**.
- **macOS:** in Terminal, `cd` into the folder, then `chmod +x phaneuron && xattr -dr com.apple.quarantine . && ./phaneuron`.
- **Linux:** FAT32 has no executable bit and is often mounted `noexec`. Copy the folder to your home directory, then
  `chmod +x phaneuron && ./phaneuron`. You need a Vulkan-capable GPU driver.

Keep `assets/` next to the executable; that is how the game finds its files. Worlds are saved in the `saves/` folder next to it.

## Build

```sh
cargo run --release          # run from the project folder
cargo test                   # logic tests: terrain, meshing, physics, editing, saving, menus
./package.sh                 # build + assemble dist/<your-os>/
```

The included GitHub Actions workflow (`.github/workflows/build.yml`) builds all three platforms: push the project and
download the three artifacts. Building macOS natively (on a Mac or in CI) is far easier than cross-compiling.

## Block size

A block is **half a world unit** (the player is 1.8 units tall, about 3.6 blocks). Terrain numbers in
`config.toml [world]` are in blocks; player numbers (speed, jump height, reach) are in world units. glTF models still use
"1 unit = 1 block" and are scaled automatically.

## Modding

Everything the game shows is a plain file. See **`docs/asset-mapping.svg`** for the picture; the short version:

- `assets/blocks/<id>.toml` defines a block. A new file is a new block, no code needed. It appears in the hotbar automatically.
- A PNG named `assets/textures/blocks/<id>.png` (or `_top` / `_side` / `_bottom`) replaces that block's flat color.
- `model = "models/blocks/<id>.gltf"` in the toml replaces the cube with a glTF model.
- `textures/ui/crosshair.png`, `banner.png`, `font.png` and `textures/sky/sky.png` reskin the UI and sky.
  The font is a 16x6 grid of 8x8 glyphs for ASCII 32 to 127 (the bundled one is Press Start 2P, SIL OFL; see `FONT-LICENSE.txt`).
- `textures/{player,npc,enemy}/` and `sounds/` are reserved with the same rules for when those features land.
- `assets/config.toml` tunes the world, sky, rendering and player. Every key is optional.

Put mods in `mods/<name>/` using the same folder layout as `assets/`. Files in a mod override files with the same
path in `assets/`; mods load alphabetically and later ones win. `examples/crystal/` is a ready-made mod
(glTF model + PNG): copy it into `mods/` and follow its README.

glTF notes: origin is the bottom-center of the block, 1 unit = 1 block, triangle meshes only, embedded images are
ignored (textures come from PNG by name), flat shaded. Models do not hide neighbouring faces and collide as full cubes.

## Code map

| File | Job |
| --- | --- |
| `src/app.rs` | window, input, screen flow (menu, playing, paused) |
| `src/menu.rs` | main menu, new-world form, world list |
| `src/ui.rs` | tiny immediate-mode UI: quads, text from `font.png`, buttons, text fields |
| `src/game.rs` | one running world: streaming, breaking/placing, hotbar, saving |
| `src/save.rs` | `saves/` folders, `world.toml`, `edits.bin` |
| `src/world.rs` | infinite chunks, noise terrain, edits, ray casting |
| `src/mesh.rs` | chunk meshing with hidden-face culling |
| `src/player.rs` | gravity, jumping, collision, step-up |
| `src/blocks.rs`, `src/model.rs` | block TOML, texture atlas, glTF loading |
| `src/assets.rs`, `src/config.rs` | file lookup across `assets/` and `mods/`, `config.toml` |
| `src/render.rs` + `src/shaders/` | wgpu sky, terrain, block outline and UI passes |

## Not yet

Audio playback, entities (NPCs, enemies), biomes, water and caves, a working Settings screen.
