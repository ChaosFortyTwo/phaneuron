use std::sync::Arc;

use glam::Vec3;

use crate::assets::Assets;
use crate::blocks::{self, Atlas};
use crate::config::Config;
use crate::input::Input;
use crate::menu::{Action, Menu};
use crate::mesh;
use crate::game::{Session, View};
use crate::keys::{self, Bind, Keybinds};
use crate::player::{Keys, Player, HEIGHT, SNEAK_HEIGHT};
use crate::player_model;
use crate::settings::Settings;
use winit::keyboard::KeyCode;
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

fn settings() -> Settings {
    Settings::load(std::path::Path::new("/nonexistent"), &Config::default())
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
        assert!(!Player::collides(&world, p.pos, HEIGHT));
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
    menu.ui(&mut test_ui(), &input, &dir, 0.0, &mut settings());
    input.end_frame();
    // Tab moves to the seed field, then type a seed and press Enter
    input.tab = true;
    menu.ui(&mut test_ui(), &input, &dir, 0.0, &mut settings());
    input.end_frame();
    input.typed = "777".into();
    menu.ui(&mut test_ui(), &input, &dir, 0.0, &mut settings());
    input.end_frame();
    input.enter = true;
    match menu.ui(&mut test_ui(), &input, &dir, 0.0, &mut settings()) {
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
        m.ui(&mut ui, &Input::default(), &dir, 0.0, &mut settings());
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
    menu.ui(&mut test_ui(), &input, &dir, 0.0, &mut settings());
    assert!(matches!(menu, Menu::Main { .. }));
    // the Game Files button yields its action
    let mut found = false;
    for y in (100..300).step_by(2) {
        let mut ui = test_ui();
        ui.mouse = (320.0, y as f32);
        ui.click = true;
        let mut m = Menu::main();
        if matches!(m.ui(&mut ui, &Input::default(), &dir, 0.0, &mut settings()), Action::OpenFiles) {
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


// ---------------------------------------------------------------------------
// v3: sneaking, flying, console, camera, model, keybinds, settings
// ---------------------------------------------------------------------------

fn test_session() -> (Session, Atlas, Config) {
    let assets = Assets::with_roots(vec!["assets".into()]);
    let cfg = Config::load(&assets).unwrap();
    let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size).unwrap();
    let meta = save::WorldMeta { folder: std::env::temp_dir(), name: "T".into(), seed: SEED, last_played: 0 };
    (Session::start(Arc::new(registry), &cfg, meta, None), atlas, cfg)
}

#[test]
fn hitbox_is_four_voxels_and_sneak_is_two() {
    assert!((HEIGHT / BLOCK - 4.0).abs() < 1e-6);
    assert!((SNEAK_HEIGHT / BLOCK - 2.0).abs() < 1e-6);
}

#[test]
fn sneaking_halves_hitbox_and_cannot_stand_under_a_low_ceiling() {
    let (mut world, _, cfg) = test_world();
    let top = world.height_at(0, 0);
    // clear a pad, then put a ceiling exactly 3 blocks above the floor (fits a 2-block sneak, not a 4-block stand)
    for x in -3..=3 {
        for z in -3..=3 {
            for y in top + 1..top + 10 {
                world.set_block(x, y, z, 0);
            }
            world.set_block(x, top + 4, z, 1);
        }
    }
    let mut p = Player::new(Vec3::new(0.5 * BLOCK, (top as f32 + 1.0) * BLOCK, 0.5 * BLOCK));
    let sneak = Keys { sneak: true, ..Keys::default() };
    for _ in 0..30 {
        p.update(1.0 / 60.0, &sneak, &world, &cfg.player);
    }
    assert!(p.sneaking);
    assert_eq!(p.height(), SNEAK_HEIGHT);
    assert!(p.eye < cfg.player.eye_height * 0.75, "eye should drop while sneaking: {}", p.eye);

    // release sneak under the ceiling: must stay crouched
    for _ in 0..30 {
        p.update(1.0 / 60.0, &Keys::default(), &world, &cfg.player);
    }
    assert!(p.sneaking, "no headroom, so the player stays crouched");

    // walk out from under it (remove the ceiling): now can stand
    for x in -3..=3 {
        for z in -3..=3 {
            world.set_block(x, top + 4, z, 0);
        }
    }
    for _ in 0..30 {
        p.update(1.0 / 60.0, &Keys::default(), &world, &cfg.player);
    }
    assert!(!p.sneaking, "with headroom the player stands up");
    assert!(p.eye > cfg.player.eye_height * 0.9);
}

#[test]
fn sneaking_is_slower() {
    let (world, _, cfg) = test_world();
    let run = |sneak: bool| {
        let mut p = spawn_on_ground(&world);
        p.yaw = 0.0;
        settle(&mut p, &world, &cfg);
        let start = p.pos;
        let keys = Keys { forward: true, sneak, ..Keys::default() };
        for _ in 0..30 {
            p.update(1.0 / 60.0, &keys, &world, &cfg.player);
        }
        (p.pos - start).length()
    };
    assert!(run(true) < run(false) * 0.6);
}

#[test]
fn free_flying_ignores_gravity_and_moves_vertically() {
    let (world, _, cfg) = test_world();
    let mut p = spawn_on_ground(&world);
    p.pos.y += 20.0;
    p.flying = true;
    let y0 = p.pos.y;
    for _ in 0..60 {
        p.update(1.0 / 60.0, &Keys::default(), &world, &cfg.player);
    }
    assert!((p.pos.y - y0).abs() < 0.01, "hovers in place when no key is held");
    let up = Keys { jump: true, ..Keys::default() };
    for _ in 0..60 {
        p.update(1.0 / 60.0, &up, &world, &cfg.player);
    }
    assert!(p.pos.y > y0 + 5.0, "jump flies up: {}", p.pos.y - y0);
    let down = Keys { sneak: true, ..Keys::default() };
    for _ in 0..120 {
        p.update(1.0 / 60.0, &down, &world, &cfg.player);
    }
    assert!(p.pos.y < y0 + 5.0 - 4.0, "sneak flies down");
    assert!(!p.sneaking, "no crouching while flying");
}

#[test]
fn console_switch_and_help() {
    let (mut s, _, _) = test_session();
    assert!(!s.player.flying);
    s.run_command("/switch");
    assert!(s.player.flying);
    s.run_command("switch"); // the slash is optional
    assert!(!s.player.flying);
    s.run_command("/help");
    s.run_command("/nonsense");
    let text: Vec<&str> = s.console.lines.iter().map(|l| l.0.as_str()).collect();
    assert!(text.iter().any(|l| l.contains("/tp")));
    assert!(text.iter().any(|l| l.contains("Unknown command")));
}

#[test]
fn console_tp_absolute_relative_and_surface() {
    let (mut s, _, _) = test_session();
    s.run_command("/tp 100 60 -40");
    assert!((s.player.pos - Vec3::new(100.0, 60.0, -40.0)).length() < 0.01, "{:?}", s.player.pos);
    assert!(s.world.has_chunk(crate::game::chunk_of(s.player.pos)), "destination chunk is generated immediately");

    s.run_command("/tp ~5 ~ ~-2");
    assert!((s.player.pos - Vec3::new(105.0, 60.0, -42.0)).length() < 0.01, "{:?}", s.player.pos);

    // two numbers = go to the surface at x z
    s.run_command("/tp 300 300");
    let (bx, bz) = ((300.0 / BLOCK) as i32, (300.0 / BLOCK) as i32);
    let surface = (s.world.height_at(bx, bz) as f32 + 1.0) * BLOCK;
    assert!((s.player.pos.y - surface).abs() < 0.01, "surface {} got {}", surface, s.player.pos.y);
    assert!(!Player::collides(&s.world, s.player.pos, s.player.height()), "never lands inside terrain");

    // a target inside the ground is lifted to the first free spot
    s.run_command("/tp 300 1 300");
    assert!(!Player::collides(&s.world, s.player.pos, s.player.height()));
    assert!(s.player.pos.y > 1.0);

    // bad input leaves the player where they are
    let before = s.player.pos;
    s.run_command("/tp abc 1 2");
    s.run_command("/tp 1");
    s.run_command("/tp nan 1 2");
    assert_eq!(s.player.pos, before);
}

#[test]
fn third_person_camera_orbits_and_stops_at_walls() {
    let (mut s, _, _) = test_session();
    let first = s.camera();
    assert_eq!(first.eye, s.eye());
    s.cycle_view();
    assert_eq!(s.view, View::ThirdBack);
    let back = s.camera();
    assert!(((back.eye - first.eye).length() - 3.5).abs() < 0.1, "3.5 units behind the head in open air");
    assert_eq!(back.forward, first.forward);
    s.cycle_view();
    assert_eq!(s.view, View::ThirdFront);
    let front = s.camera();
    assert!((front.forward + first.forward).length() < 1e-4, "front view looks back at the player");
    s.cycle_view();
    assert_eq!(s.view, View::First);

    // wall right behind the player pulls the camera in
    s.cycle_view();
    let eye = s.eye();
    let behind = -s.player.forward();
    let wall = eye + behind * 1.0;
    let (wx, wy, wz) = ((wall.x / BLOCK).floor() as i32, (wall.y / BLOCK).floor() as i32, (wall.z / BLOCK).floor() as i32);
    for dy in -3..=3 {
        for dx in -3..=3 {
            for dz in -3..=3 {
                s.world.set_block(wx + dx, wy + dy, wz + dz, 1);
            }
        }
    }
    let cam = s.camera();
    assert!((cam.eye - eye).length() < 1.2, "camera must not pass through the wall: {}", (cam.eye - eye).length());
}

#[test]
fn player_model_fits_the_hitbox_and_follows_sneak_and_yaw() {
    let (s, atlas, _) = test_session();
    let tiles = s.world.registry.player_tiles;
    let mut p = Player::new(Vec3::new(10.0, 20.0, 10.0));
    let extent = |m: &mesh::MeshData| {
        let ys: Vec<f32> = m.vertices.iter().map(|v| v.pos[1]).collect();
        (ys.iter().cloned().fold(f32::MAX, f32::min), ys.iter().cloned().fold(f32::MIN, f32::max))
    };
    let m = player_model::build(&p, &tiles, &atlas);
    assert!(!m.indices.is_empty() && m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
    assert!(m.vertices.len() <= 1024 && m.indices.len() <= 2048, "must fit the renderer's dynamic buffers");
    let (lo, hi) = extent(&m);
    assert!((lo - 20.0).abs() < 0.01 && (hi - 22.0).abs() < 0.01, "standing model spans 2.0 units: {lo}..{hi}");

    p.sneaking = true;
    let (lo, hi) = extent(&player_model::build(&p, &tiles, &atlas));
    assert!((hi - lo - 1.0).abs() < 0.01, "sneaking model is half as tall: {}", hi - lo);

    // turning around moves the eyes (front of the head) to the other side
    let eyes = |yaw: f32| {
        let mut q = Player::new(Vec3::ZERO);
        q.yaw = yaw;
        let m = player_model::build(&q, &tiles, &atlas);
        // the last 16 vertices belong to the two eyes
        let e = &m.vertices[m.vertices.len() - 48..];
        let c: Vec3 = e.iter().map(|v| Vec3::from(v.pos)).sum::<Vec3>() / e.len() as f32;
        c
    };
    let (a, b) = (eyes(0.0), eyes(std::f32::consts::PI));
    assert!(a.z < -0.2 && b.z > 0.2, "eyes face -z at yaw 0 and +z when turned around: {a:?} {b:?}");
}

#[test]
fn walk_cycle_swings_limbs_only_while_moving() {
    let (world, _, cfg) = test_world();
    let mut p = spawn_on_ground(&world);
    settle(&mut p, &world, &cfg);
    assert!(p.walk_amp < 0.05);
    let walk = Keys { forward: true, ..Keys::default() };
    for _ in 0..30 {
        p.update(1.0 / 60.0, &walk, &world, &cfg.player);
    }
    assert!(p.walk_amp > 0.8 && p.walk_phase > 1.0);
    settle(&mut p, &world, &cfg);
    assert!(p.walk_amp < 0.05, "limbs relax when standing still");
}

#[test]
fn debug_screen_reports_position_fps_and_targeted_block() {
    let (mut s, _, cfg) = test_session();
    // look straight down at the ground so the crosshair targets a block
    s.player.pitch = -89f32.to_radians();
    s.update(1.0 / 60.0, &Keys::default(), &cfg);
    let lines = s.debug_lines(144.0, &cfg);
    let all = lines.join("\n");
    assert!(all.contains("144 fps"), "{all}");
    assert!(all.contains("XYZ:"), "{all}");
    assert!(all.contains("Looking at:") && all.contains("id ") && !all.contains("nothing in reach"), "{all}");
    let hit = s.target.unwrap();
    let id = s.world.get(hit.cell[0], hit.cell[1], hit.cell[2]);
    assert!(all.contains(&format!("(id {id},")), "block id of the crosshair block must be shown: {all}");

    s.player.pitch = 89f32.to_radians();
    s.update(1.0 / 60.0, &Keys::default(), &cfg);
    assert!(s.debug_lines(60.0, &cfg).join("\n").contains("nothing in reach"));
}

#[test]
fn console_ui_typing_enter_and_escape() {
    let (mut s, _, _) = test_session();
    s.console.open = true;
    // control characters (here a backspace char) are not typed; the rest is
    let mut input = Input { typed: "/switch\u{8}h".into(), ..Input::default() };
    s.console_ui(&mut test_ui(), &input);
    assert_eq!(s.console.input, "/switchh");
    // the Backspace key removes the stray "h"
    input.end_frame();
    input.backspace = true;
    s.console_ui(&mut test_ui(), &input);
    assert_eq!(s.console.input, "/switch");
    // Enter runs it
    input.end_frame();
    input.enter = true;
    s.console_ui(&mut test_ui(), &input);
    assert!(s.player.flying, "Enter ran /switch");
    assert!(s.console.input.is_empty());
    // closed console ignores typing
    input.end_frame();
    input.escape = true;
    s.console_ui(&mut test_ui(), &input);
    assert!(!s.console.open, "Esc closes the console");
    input.end_frame();
    input.typed = "abc".into();
    s.console_ui(&mut test_ui(), &input);
    assert!(s.console.input.is_empty(), "a closed console does not collect typing");
}

#[test]
fn key_table_roundtrips_and_labels_read_well() {
    for code in [KeyCode::KeyW, KeyCode::F3, KeyCode::AltLeft, KeyCode::ShiftRight, KeyCode::ArrowUp, KeyCode::PageUp] {
        let name = keys::key_name(code).expect("bindable");
        assert_eq!(keys::key_from_name(name), Some(code));
    }
    assert_eq!(keys::key_label(KeyCode::KeyW), "W");
    assert_eq!(keys::key_label(KeyCode::ShiftLeft), "Left Shift");
    assert_eq!(keys::key_label(KeyCode::AltLeft), "Left Alt");
    assert_eq!(keys::key_label(KeyCode::F3), "F3");
    assert_eq!(keys::key_label(KeyCode::ArrowUp), "Up Arrow");
    assert_eq!(keys::key_label(KeyCode::PageUp), "Page Up");
    assert!(keys::key_name(KeyCode::Escape).is_none() && keys::key_name(KeyCode::Digit1).is_none(), "fixed keys are not bindable");
}

#[test]
fn default_binds_match_the_requested_controls() {
    let b = Keybinds::default();
    assert_eq!(b.get(Bind::FreeMouse), KeyCode::AltLeft);
    assert_eq!(b.get(Bind::Sneak), KeyCode::KeyC);
    assert_eq!(b.get(Bind::Debug), KeyCode::F3);
    assert_eq!(b.get(Bind::Console), KeyCode::F2);
    assert_eq!(b.get(Bind::Camera), KeyCode::F4);
    assert_eq!(b.lookup(KeyCode::KeyC), Some(Bind::Sneak));
}

#[test]
fn rebinding_swaps_conflicts_and_keys_stay_unique() {
    let mut b = Keybinds::default();
    b.set(Bind::Sneak, KeyCode::KeyV);
    assert_eq!(b.get(Bind::Sneak), KeyCode::KeyV);
    assert_eq!(b.lookup(KeyCode::KeyC), None, "old key is free");
    // bind Sneak to W: Forward takes Sneak's old key (V)
    b.set(Bind::Sneak, KeyCode::KeyW);
    assert_eq!(b.get(Bind::Sneak), KeyCode::KeyW);
    assert_eq!(b.get(Bind::Forward), KeyCode::KeyV);
    let mut seen = std::collections::HashSet::new();
    for bind in Bind::ALL {
        assert!(seen.insert(b.get(bind)), "{bind:?} shares a key");
    }
}

#[test]
fn settings_persist_clamp_and_survive_garbage() {
    let dir = std::env::temp_dir().join(format!("ph_settings_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::default();

    let fresh = Settings::load(&dir, &cfg);
    assert_eq!(fresh.render_distance, cfg.render.render_distance);

    let mut s = fresh.clone();
    s.render_distance = 28;
    s.mouse_sensitivity = 0.004;
    s.fov = 90.0;
    s.binds.set(Bind::Sneak, KeyCode::ControlLeft);
    s.save(&dir).unwrap();
    let loaded = Settings::load(&dir, &cfg);
    assert_eq!(loaded, s);

    let mut c = cfg.clone();
    loaded.apply(&mut c);
    assert_eq!((c.render.render_distance, c.render.fov), (28, 90.0));

    // out-of-range values are clamped
    std::fs::write(dir.join("settings.toml"), "render_distance = 9999\nfov = 1.0\n[keys]\nforward = \"NotAKey\"\n").unwrap();
    let clamped = Settings::load(&dir, &cfg);
    assert_eq!(clamped.render_distance, crate::settings::MAX_RENDER_DISTANCE);
    assert_eq!(clamped.fov, crate::settings::MIN_FOV);
    assert_eq!(clamped.binds.get(Bind::Forward), KeyCode::KeyW, "unknown key names fall back to the default");

    // unreadable file: defaults, no crash
    std::fs::write(dir.join("settings.toml"), "this is = not [valid").unwrap();
    assert_eq!(Settings::load(&dir, &cfg).render_distance, cfg.render.render_distance);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn settings_screen_changes_values_and_rebinds_keys() {
    use crate::settings_ui::SettingsScreen;
    // Button positions on a 640x360 logical screen, derived from the layout in settings_ui.rs.
    // Main page: panel 290x166 centered; the "-"/"+" buttons sit at x = px+pw-118 and +100.
    let (pw, ph) = (290.0_f32, 166.0_f32);
    let (px, py) = ((640.0 - pw) / 2.0, ((360.0 - ph) / 2.0).max(44.0));
    let (minus, plus) = ((px + pw - 118.0 + 9.0, py + 10.0), (px + pw - 118.0 + 100.0 + 9.0, py + 10.0));
    let controls_btn = (px + pw / 2.0, py + 114.0 + 11.0);
    // Controls page: 11 rows of 18px; key buttons at x = px+pw-120 .. px+pw (px for a 300px panel)
    let cpy = ((360.0 - 11.0 * 18.0 - 70.0) / 2.0_f32).max(48.0);
    let row_btn = |i: usize| (((640.0 - 300.0) / 2.0) + 300.0 - 60.0, cpy + 2.0 + i as f32 * 18.0 + 4.0);

    let click = |(x, y): (f32, f32)| {
        let mut ui = test_ui();
        ui.mouse = (x, y);
        ui.click = true;
        ui
    };
    let mut s = settings();
    let mut screen = SettingsScreen::new();

    let rd = s.render_distance;
    screen.ui(&mut click(plus), &Input::default(), &mut s);
    assert_eq!(s.render_distance, rd + 2, "+ raises render distance by 2");
    screen.ui(&mut click(minus), &Input::default(), &mut s);
    assert_eq!(s.render_distance, rd, "- lowers it again");
    for _ in 0..40 {
        screen.ui(&mut click(plus), &Input::default(), &mut s);
    }
    assert_eq!(s.render_distance, crate::settings::MAX_RENDER_DISTANCE, "capped at the maximum");
    for _ in 0..40 {
        screen.ui(&mut click(minus), &Input::default(), &mut s);
    }
    assert_eq!(s.render_distance, crate::settings::MIN_RENDER_DISTANCE, "and at the minimum");

    // open Controls, click Sneak's key button (row 6), press V
    assert!(!screen.ui(&mut click(controls_btn), &Input::default(), &mut s));
    screen.ui(&mut click(row_btn(6)), &Input::default(), &mut s);
    assert_eq!(s.binds.get(Bind::Sneak), KeyCode::KeyC, "nothing changes until a key is pressed");
    let press = Input { last_key: Some(KeyCode::KeyV), ..Input::default() };
    screen.ui(&mut test_ui(), &press, &mut s);
    assert_eq!(s.binds.get(Bind::Sneak), KeyCode::KeyV, "the pressed key is now bound to Sneak");

    // Esc while waiting cancels without changing anything
    screen.ui(&mut click(row_btn(6)), &Input::default(), &mut s);
    let esc = Input { escape: true, last_key: Some(KeyCode::KeyX), ..Input::default() };
    assert!(!screen.ui(&mut test_ui(), &esc, &mut s), "Esc cancels the rebind but stays on the page");
    assert_eq!(s.binds.get(Bind::Sneak), KeyCode::KeyV);
    // keys that cannot be bound are ignored while waiting (digits stay reserved for the hotbar)
    screen.ui(&mut click(row_btn(6)), &Input::default(), &mut s);
    let bad = Input { last_key: Some(KeyCode::Digit1), ..Input::default() };
    screen.ui(&mut test_ui(), &bad, &mut s);
    assert_eq!(s.binds.get(Bind::Sneak), KeyCode::KeyV);
}
