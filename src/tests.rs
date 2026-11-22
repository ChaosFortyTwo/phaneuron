use std::sync::Arc;

use glam::Vec3;

use crate::assets::Assets;
use crate::blocks::{self, Atlas};
use crate::config::Config;
use crate::input::Input;
use crate::menu::{Action, Menu};
use crate::mesh;
use crate::player::{Keys, Player};
use crate::save;
use crate::ui::{DrawList, Ui};
use crate::world::{self, World, BLOCK};

const SEED: u32 = 1337;

fn test_world() -> (World, Atlas, Config) {
    let assets = Assets::discover();
    let cfg = Config::load(&assets).unwrap();
    let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size).unwrap();
    let mut world = World::new(Arc::new(registry), &cfg.world, SEED);
    for dz in -3..=3 {
        for dx in -3..=3 {
            world.generate((dx, dz));
        }
    }
    (world, atlas, cfg)
}

fn spawn_on_ground(world: &World) -> Player {
    let h = world.height_at(0, 0) as f32;
    Player::new(Vec3::new(0.5 * BLOCK, (h + 1.0) * BLOCK, 0.5 * BLOCK))
}

fn settle(p: &mut Player, world: &World, cfg: &Config) {
    for _ in 0..240 {
        p.update(1.0 / 60.0, &Keys::default(), world, &cfg.player);
    }
}

fn test_ui() -> Ui {
    Ui { dl: DrawList::default(), w: 640.0, h: 360.0, mouse: (0.0, 0.0), click: false, time: 0.0, banner: None, crosshair: None }
}

#[test]
fn default_assets_load() {
    let (world, atlas, _) = test_world();
    for id in ["grass", "dirt", "stone"] {
        assert!(world.registry.id_of(id).is_some(), "missing block {id}");
    }
    assert!(atlas.image.width() > 0);
}

#[test]
fn terrain_is_hilly_but_bounded() {
    let (world, _, _) = test_world();
    let hs: Vec<i32> = (-80..80).map(|i| world.height_at(i * 5, i * 3)).collect();
    let (min, max) = (*hs.iter().min().unwrap(), *hs.iter().max().unwrap());
    assert!(max - min >= 8, "terrain is flat: {min}..{max}");
    assert!(min >= 1 && max < world::CH as i32 - 1);
}

#[test]
fn blocks_are_half_size() {
    let (world, atlas, _) = test_world();
    let m = mesh::build(&world, &atlas, (0, 0));
    let max_x = m.vertices.iter().map(|v| v.pos[0]).fold(f32::MIN, f32::max);
    let min_x = m.vertices.iter().map(|v| v.pos[0]).fold(f32::MAX, f32::min);
    assert!((max_x - min_x - world::CW as f32 * BLOCK).abs() < 1e-3, "chunk should be 16 blocks = 8 units wide");
    // every vertex sits on the half-unit grid
    assert!(m.vertices.iter().all(|v| (v.pos[0] / BLOCK).fract().abs() < 1e-4 || (v.pos[0] / BLOCK).fract().abs() > 0.9999));
}

#[test]
fn meshing_skips_hidden_faces() {
    let (world, atlas, _) = test_world();
    let m = mesh::build(&world, &atlas, (0, 0));
    assert!(!m.indices.is_empty());
    let faces = m.indices.len() / 6;
    assert!(faces < 16 * 16 * 12, "too many faces: {faces}");
    assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
}

#[test]
fn player_lands_on_ground_and_can_jump() {
    let (world, _, cfg) = test_world();
    let ground = (world.height_at(0, 0) as f32 + 1.0) * BLOCK;
    let mut p = Player::new(Vec3::new(0.5 * BLOCK, ground + 10.0, 0.5 * BLOCK));
    settle(&mut p, &world, &cfg);
    assert!(p.on_ground);
    assert!((p.pos.y - ground).abs() < 0.01, "rests at {} expected {}", p.pos.y, ground);

    let jump = Keys { jump: true, ..Keys::default() };
    let mut peak = p.pos.y;
    for _ in 0..40 {
        p.update(1.0 / 60.0, &jump, &world, &cfg.player);
        peak = peak.max(p.pos.y);
    }
    assert!(peak - ground > 1.0, "jump too low: {}", peak - ground);
}

#[test]
fn player_steps_up_one_block_but_not_two() {
    let (mut world, _, cfg) = test_world();
    let top = world.height_at(0, 0);
    // a flat pad at the spawn column's height, 3 blocks wide, 43 blocks long (along +x)
    for x in -2..=40 {
        for z in -2..=2 {
            for y in top + 1..top + 8 {
                world.set_block(x, y, z, 0);
            }
            for y in 1..=top {
                world.set_block(x, y, z, 1);
            }
        }
    }
    // a one-block-high ledge from x = 6 onward, and a two-block-high wall on top of it at x = 20
    for x in 6..=40 {
        for z in -2..=2 {
            world.set_block(x, top + 1, z, 1);
        }
    }
    for z in -2..=2 {
        world.set_block(20, top + 2, z, 1);
        world.set_block(20, top + 3, z, 1);
    }

    let mut p = Player::new(Vec3::new(0.5 * BLOCK, (top as f32 + 1.0) * BLOCK, 0.5 * BLOCK));
    p.yaw = 90f32.to_radians(); // walk toward +x
    settle(&mut p, &world, &cfg);
    let walk = Keys { forward: true, ..Keys::default() };
    for _ in 0..60 {
        p.update(1.0 / 60.0, &walk, &world, &cfg.player);
    }
    assert!(p.pos.x > 7.0 * BLOCK, "should have climbed the 1-block ledge, x = {}", p.pos.x);
    assert!((p.pos.y - (top as f32 + 2.0) * BLOCK).abs() < 0.02, "should stand on the ledge, y = {}", p.pos.y);

    for _ in 0..300 {
        p.update(1.0 / 60.0, &walk, &world, &cfg.player);
    }
    assert!(p.pos.x < 20.0 * BLOCK, "a 2-block wall must stop the player, x = {}", p.pos.x);
    assert!(p.pos.x > 18.0 * BLOCK, "the player should have walked up to the wall, x = {}", p.pos.x);
}

#[test]
fn player_cannot_walk_through_walls() {
    let (world, _, cfg) = test_world();
    let mut p = spawn_on_ground(&world);
    let keys = Keys { forward: true, ..Keys::default() };
    for _ in 0..600 {
        p.update(1.0 / 60.0, &keys, &world, &cfg.player);
        assert!(!Player::collides(&world, p.pos));
    }
}

#[test]
fn raycast_hits_terrain_and_reports_placement_cell() {
    let (world, _, _) = test_world();
    let top = world.height_at(0, 0);
    let origin = Vec3::new(0.5 * BLOCK, (top as f32 + 6.0) * BLOCK, 0.5 * BLOCK);
    let hit = world.raycast(origin, Vec3::NEG_Y, 10.0).expect("ray straight down must hit the ground");
    assert_eq!(hit.cell, [0, top, 0]);
    assert_eq!(hit.prev, [0, top + 1, 0]);
    // out of reach
    assert!(world.raycast(origin, Vec3::NEG_Y, 1.0).is_none());
    // looking up at the empty sky
    assert!(world.raycast(origin, Vec3::Y, 50.0).is_none());
}

#[test]
fn break_and_place_blocks() {
    let (mut world, atlas, _) = test_world();
    let top = world.height_at(0, 0);
    let origin = Vec3::new(0.5 * BLOCK, (top as f32 + 6.0) * BLOCK, 0.5 * BLOCK);

    let hit = world.raycast(origin, Vec3::NEG_Y, 10.0).unwrap();
    world.set_block(hit.cell[0], hit.cell[1], hit.cell[2], 0);
    assert_eq!(world.get(0, top, 0), 0);
    let hit2 = world.raycast(origin, Vec3::NEG_Y, 10.0).unwrap();
    assert_eq!(hit2.cell, [0, top - 1, 0], "after breaking, the ray reaches the block below");

    let affected = world.set_block(hit2.prev[0], hit2.prev[1], hit2.prev[2], 3);
    assert_eq!(world.get(0, top, 0), 3);
    assert!(!affected.is_empty());
    // breaking then placing in the same cell is one edit whose final value is the placed block
    assert_eq!(world.edits().values().map(|e| e.len()).sum::<usize>(), 1);
    assert!(world.edits().values().any(|e| e.values().any(|&id| id == 3)));
    assert!(world.take_dirty());
    assert!(!world.take_dirty(), "dirty flag resets after being read");

    // border edits rebuild the neighbouring chunk too
    let affected = world.set_block(0, top, 5, 0);
    assert!(affected.contains(&(0, 0)) && affected.contains(&(-1, 0)));
    // meshing still works after edits
    assert!(!mesh::build(&world, &atlas, (0, 0)).indices.is_empty());
}

#[test]
fn edits_survive_save_and_load() {
    let assets = Assets::with_roots(vec!["assets".into()]);
    let cfg = Config::load(&assets).unwrap();
    let (registry, _) = blocks::load(&assets, cfg.render.tile_size).unwrap();
    let registry = Arc::new(registry);

    let dir = std::env::temp_dir().join(format!("phaneuron_saves_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let meta = save::create_world(&dir, "My: Test*World", 4242).unwrap();
    assert!(meta.folder.ends_with("My_ Test_World"), "folder name must be filesystem-safe: {:?}", meta.folder);
    let again = save::create_world(&dir, "My: Test*World", 1).unwrap();
    assert_ne!(meta.folder, again.folder, "duplicate names get their own folder");

    let mut world = World::new(registry.clone(), &cfg.world, 4242);
    world.generate((0, 0));
    let top = world.height_at(3, 4);
    world.set_block(3, top, 4, 0);
    world.set_block(3, top + 1, 4, 2);
    let data = save::SaveData {
        player: save::PlayerSave { x: 1.0, y: 2.0, z: 3.0, yaw: 0.5, pitch: -0.25 },
        selected_block: "stone".into(),
    };
    save::save_world(&meta, &data, world.edits(), &registry, true).unwrap();

    let listed = save::list_worlds(&dir);
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|w| w.name == "My: Test*World" && w.seed == 4242));

    let saved = save::load_world(&meta, &registry).unwrap();
    assert_eq!(saved.player, Some(data.player));
    assert_eq!(saved.selected_block.as_deref(), Some("stone"));

    let mut world2 = World::new(registry.clone(), &cfg.world, 4242);
    world2.set_edits(saved.edits);
    world2.generate((0, 0));
    assert_eq!(world2.get(3, top, 4), 0, "broken block stays broken");
    assert_eq!(world2.get(3, top + 1, 4), 2, "placed block stays placed");
    assert_eq!(world2.get(3, top - 1, 4), world.get(3, top - 1, 4), "untouched terrain regenerates identically");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupt_edit_files_are_rejected_not_crashed_on() {
    let assets = Assets::with_roots(vec!["assets".into()]);
    let (registry, _) = blocks::load(&assets, 32).unwrap();
    assert!(save::decode_edits(b"nope", &registry).is_err());
    assert!(save::decode_edits(b"PHED\x01\x01\x00", &registry).is_err());
    assert!(save::decode_edits(&[], &registry).is_err());
}

#[test]
fn seed_text_rules() {
    assert_eq!(save::seed_from_text("12345"), 12345);
    assert_eq!(save::seed_from_text(" 12345 "), 12345);
    assert_eq!(save::seed_from_text("hello"), save::seed_from_text("hello"));
    assert_ne!(save::seed_from_text("hello"), save::seed_from_text("world"));
    // empty seed -> some number; two calls a moment apart need not match, so just check it works
    let _ = save::seed_from_text("");
}

#[test]
fn new_world_form_takes_typing_and_creates() {
    let mut menu = Menu::New(crate::menu::NewWorld { name: String::new(), seed: String::new(), focus: 0 });
    let dir = std::env::temp_dir();
    let mut input = Input { typed: "Hi there\u{8}\n\u{e9}".into(), ..Input::default() };
    menu.ui(&mut test_ui(), &input, &dir, 0.0);
    input.end_frame();
    // Tab moves to the seed field, then type a seed and press Enter
    input.tab = true;
    menu.ui(&mut test_ui(), &input, &dir, 0.0);
    input.end_frame();
    input.typed = "777".into();
    menu.ui(&mut test_ui(), &input, &dir, 0.0);
    input.end_frame();
    input.enter = true;
    match menu.ui(&mut test_ui(), &input, &dir, 0.0) {
        Action::Create { name, seed } => {
            assert_eq!(name, "Hi there", "control and non-ASCII characters are filtered out");
            assert_eq!(seed, 777);
        }
        _ => panic!("Enter should create the world"),
    }
}

#[test]
fn menu_navigation_and_button_clicks() {
    let dir = std::env::temp_dir();
    let mut menu = Menu::main();
    // click "New World" (second button): find it by scanning the screen
    let mut clicked = false;
    'outer: for y in (100..300).step_by(2) {
        let mut ui = test_ui();
        ui.mouse = (320.0, y as f32);
        ui.click = true;
        let mut m = Menu::main();
        m.ui(&mut ui, &Input::default(), &dir, 0.0);
        if matches!(m, Menu::New(_)) {
            clicked = true;
            menu = m;
            break 'outer;
        }
    }
    assert!(clicked, "some button on the main menu leads to the New World form");
    assert!(matches!(menu, Menu::New(_)));
    // Esc goes back
    let input = Input { escape: true, ..Input::default() };
    menu.ui(&mut test_ui(), &input, &dir, 0.0);
    assert!(matches!(menu, Menu::Main { .. }));
    // the Game Files button yields its action
    let mut found = false;
    for y in (100..300).step_by(2) {
        let mut ui = test_ui();
        ui.mouse = (320.0, y as f32);
        ui.click = true;
        let mut m = Menu::main();
        if matches!(m.ui(&mut ui, &Input::default(), &dir, 0.0), Action::OpenFiles) {
            found = true;
        }
    }
    assert!(found, "a Game Files button exists");
}

#[test]
fn example_mod_overrides_and_adds_model_block() {
    let assets = Assets::with_roots(vec!["assets".into(), "examples/crystal".into()]);
    let mut cfg = Config::load(&assets).unwrap();
    cfg.world.surface = "crystal".into();
    let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size).unwrap();
    let id = registry.id_of("crystal").expect("crystal block from the mod");
    let crystal = &registry.blocks[id as usize];
    assert!(crystal.model.is_some(), "glTF model should load");
    assert!(!crystal.occluding, "model blocks never hide neighbours");
    assert_ne!(crystal.tiles[1], 0, "crystal.png should replace the missing-texture tile");
    let mut world = World::new(Arc::new(registry), &cfg.world, SEED);
    for dz in -1..=1 {
        for dx in -1..=1 {
            world.generate((dx, dz));
        }
    }
    let m = mesh::build(&world, &atlas, (0, 0));
    assert!(m.indices.len() >= 16 * 16 * 6 * 3);
}

#[test]
fn png_overrides_flat_color_and_later_roots_win() {
    let dir = std::env::temp_dir().join(format!("ph_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("textures/blocks")).unwrap();
    image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255])).save(dir.join("textures/blocks/grass.png")).unwrap();
    let assets = Assets::with_roots(vec!["assets".into(), dir.clone()]);
    let (reg, atlas) = blocks::load(&assets, 8).unwrap();
    let grass = &reg.blocks[reg.id_of("grass").unwrap() as usize];
    let (uv_min, _) = atlas.tile_uv(grass.tiles[1]);
    let px = atlas.image.get_pixel(
        (uv_min[0] * atlas.image.width() as f32) as u32 + 1,
        (uv_min[1] * atlas.image.height() as f32) as u32 + 1,
    );
    assert_eq!(px.0, [1, 2, 3, 255]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn streaming_loads_and_unloads() {
    let (mut world, _, _) = test_world();
    let removed = world.stream((100, 100), 4, 1000);
    assert!(!removed.is_empty());
    assert!(world.has_chunk((100, 100)));
    assert!(!world.has_chunk((0, 0)));
}
