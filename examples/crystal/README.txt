Example mod: Crystal block (glTF model + PNG texture)

1. Copy this whole folder to  mods/crystal/   (next to the game, beside assets/).
2. Open assets/config.toml and change    surface = "grass"    to    surface = "crystal"
3. Start the game. Every hilltop is now covered in little crystals.

Experiments:
- Delete textures/blocks/crystal.png  -> the model falls back to its glTF material color.
- Delete the "model = ..." line       -> the block becomes a plain textured cube.
- Add  color = "#ff4080"  and delete the PNG and the model line -> a flat pink cube.
