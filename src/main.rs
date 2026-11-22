mod assets;
mod blocks;
mod config;
mod mesh;
mod model;
mod player;
mod render;
mod world;

use std::{sync::Arc, time::Instant};

use anyhow::{Context, Result};
use glam::Vec3;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use assets::Assets;
use blocks::Atlas;
use config::Config;
use player::{Keys, Player};
use render::{Camera, Renderer};
use world::{World, CW};

const GEN_PER_FRAME: usize = 4;
const MESH_PER_FRAME: usize = 3;

struct Game {
    window: Arc<Window>,
    renderer: Renderer,
    world: World,
    atlas: Atlas,
    player: Player,
    keys: Keys,
    cfg: Config,
    grabbed: bool,
    last: Instant,
    fps_timer: Instant,
    frames: u32,
}

fn chunk_of(p: Vec3) -> (i32, i32) {
    ((p.x.floor() as i32).div_euclid(CW as i32), (p.z.floor() as i32).div_euclid(CW as i32))
}

impl Game {
    fn new(event_loop: &ActiveEventLoop, assets: &Assets, cfg: Config) -> Result<Game> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(cfg.window.title.clone())
                        .with_inner_size(LogicalSize::new(cfg.window.width, cfg.window.height)),
                )
                .context("cannot create window")?,
        );

        let (registry, atlas) = blocks::load(assets, cfg.render.tile_size)?;
        println!("loaded {} blocks, atlas {}x{}", registry.blocks.len() - 1, atlas.image.width(), atlas.image.height());
        let mut world = World::new(registry, &cfg.world);

        // Generate the area around spawn up front so the first frame has ground.
        for dz in -2..=2 {
            for dx in -2..=2 {
                world.generate((dx, dz));
            }
        }
        let spawn_y = world.height_at(0, 0) as f32 + 1.0;
        let player = Player::new(Vec3::new(0.5, spawn_y, 0.5));

        let renderer = pollster::block_on(Renderer::new(window.clone(), assets, &cfg, &atlas))?;

        let mut game = Game {
            window,
            renderer,
            world,
            atlas,
            player,
            keys: Keys::default(),
            cfg,
            grabbed: false,
            last: Instant::now(),
            fps_timer: Instant::now(),
            frames: 0,
        };
        game.set_grab(true);
        game.window.request_redraw();
        Ok(game)
    }

    fn set_grab(&mut self, grab: bool) {
        self.grabbed = grab;
        if grab {
            let r = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = r {
                eprintln!("warning: cannot capture the mouse: {e}");
            }
            self.window.set_cursor_visible(false);
        } else {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
            self.keys = Keys::default();
        }
    }

    fn key(&mut self, event_loop: &ActiveEventLoop, ev: &KeyEvent) {
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        let down = ev.state == ElementState::Pressed;
        match code {
            KeyCode::KeyW => self.keys.forward = down,
            KeyCode::KeyS => self.keys.back = down,
            KeyCode::KeyA => self.keys.left = down,
            KeyCode::KeyD => self.keys.right = down,
            KeyCode::Space => self.keys.jump = down,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.keys.sprint = down,
            KeyCode::Escape if down && !ev.repeat => {
                if self.grabbed {
                    self.set_grab(false);
                } else {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn mouse(&mut self, (dx, dy): (f64, f64)) {
        if !self.grabbed {
            return;
        }
        let s = self.cfg.player.mouse_sensitivity;
        self.player.yaw += dx as f32 * s;
        let lim = 89f32.to_radians();
        self.player.pitch = (self.player.pitch - dy as f32 * s).clamp(-lim, lim);
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;

        self.player.update(dt, &self.keys, &self.world, &self.cfg.player);

        // Stream chunks around the player, then mesh a few of them.
        let center = chunk_of(self.player.pos);
        let rd = self.cfg.render.render_distance;
        for key in self.world.stream(center, rd, GEN_PER_FRAME) {
            self.renderer.remove_chunk(key);
        }
        let mut built = 0;
        for key in self.world.mesh_candidates(center, rd) {
            if built >= MESH_PER_FRAME {
                break;
            }
            if self.renderer.has_mesh(key) {
                continue;
            }
            let data = mesh::build(&self.world, &self.atlas, key);
            self.renderer.upload(key, data);
            built += 1;
        }

        let cam = Camera {
            eye: self.player.pos + Vec3::new(0.0, self.cfg.player.eye_height, 0.0),
            forward: self.player.forward(),
        };
        match self.renderer.render(&cam) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let (w, h) = self.renderer.size();
                self.renderer.resize(w, h);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => panic!("GPU out of memory"),
            Err(e) => eprintln!("render error: {e:?}"),
        }

        self.frames += 1;
        if self.fps_timer.elapsed().as_secs_f32() >= 0.5 {
            let fps = self.frames as f32 / self.fps_timer.elapsed().as_secs_f32();
            let p = self.player.pos;
            self.window.set_title(&format!(
                "{} | {:.0} fps | x {:.1} y {:.1} z {:.1}{}",
                self.cfg.window.title,
                fps,
                p.x,
                p.y,
                p.z,
                if self.grabbed { "" } else { " | click to capture mouse, Esc to quit" }
            ));
            self.frames = 0;
            self.fps_timer = Instant::now();
        }
    }
}

struct App {
    assets: Assets,
    cfg: Config,
    game: Option<Game>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.game.is_some() {
            return;
        }
        match Game::new(event_loop, &self.assets, self.cfg.clone()) {
            Ok(g) => self.game = Some(g),
            Err(e) => {
                eprintln!("fatal: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(g) = self.game.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => g.renderer.resize(s.width, s.height),
            WindowEvent::KeyboardInput { event, .. } => g.key(event_loop, &event),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } if !g.grabbed => {
                g.set_grab(true)
            }
            WindowEvent::Focused(false) => g.set_grab(false),
            WindowEvent::RedrawRequested => {
                g.frame();
                g.window.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, Some(g)) = (event, self.game.as_mut()) {
            g.mouse(delta);
        }
    }
}

fn main() -> Result<()> {
    let assets = Assets::discover();
    println!("asset roots: {:?}", assets.roots());
    let cfg = Config::load(&assets)?;
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { assets, cfg, game: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_world() -> (World, Atlas, Config) {
        let assets = Assets::discover();
        let cfg = Config::load(&assets).unwrap();
        let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size).unwrap();
        let mut world = World::new(registry, &cfg.world);
        for dz in -3..=3 {
            for dx in -3..=3 {
                world.generate((dx, dz));
            }
        }
        (world, atlas, cfg)
    }

    #[test]
    fn default_assets_load() {
        let (world, atlas, _) = test_world();
        assert!(world.registry.id_of("grass").is_some());
        assert!(world.registry.id_of("dirt").is_some());
        assert!(world.registry.id_of("stone").is_some());
        assert!(atlas.image.width() > 0);
    }

    #[test]
    fn terrain_is_hilly_but_bounded() {
        let (world, _, _) = test_world();
        let hs: Vec<i32> = (-40..40).map(|i| world.height_at(i * 3, i * 2)).collect();
        let (min, max) = (*hs.iter().min().unwrap(), *hs.iter().max().unwrap());
        assert!(max - min >= 4, "terrain is flat: {min}..{max}");
        assert!(min >= 1 && max < world::CH as i32 - 1);
    }

    #[test]
    fn meshing_skips_hidden_faces() {
        let (world, atlas, _) = test_world();
        let m = mesh::build(&world, &atlas, (0, 0));
        assert!(!m.indices.is_empty());
        // A fully exposed 16x16 chunk of solid blocks would need far more faces than the surface alone.
        let faces = m.indices.len() / 6;
        assert!(faces < 16 * 16 * 12, "too many faces: {faces}");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.vertices.len()));
    }

    #[test]
    fn player_lands_on_ground_and_can_jump() {
        let (world, _, cfg) = test_world();
        let ground = world.height_at(0, 0) as f32 + 1.0;
        let mut p = Player::new(Vec3::new(0.5, ground + 10.0, 0.5));
        let keys = Keys::default();
        for _ in 0..300 {
            p.update(1.0 / 60.0, &keys, &world, &cfg.player);
        }
        assert!(p.on_ground);
        assert!((p.pos.y - ground).abs() < 0.01, "rests at {} expected {}", p.pos.y, ground);

        let jump = Keys { jump: true, ..Keys::default() };
        let mut peak = p.pos.y;
        for _ in 0..40 {
            p.update(1.0 / 60.0, &jump, &world, &cfg.player);
            peak = peak.max(p.pos.y);
        }
        assert!(peak - ground > 0.9, "jump too low: {}", peak - ground);
    }

    #[test]
    fn player_cannot_walk_through_walls() {
        let (world, _, cfg) = test_world();
        let ground = world.height_at(0, 0) as f32 + 1.0;
        let mut p = Player::new(Vec3::new(0.5, ground, 0.5));
        let keys = Keys { forward: true, ..Keys::default() };
        for _ in 0..600 {
            p.update(1.0 / 60.0, &keys, &world, &cfg.player);
            assert!(!Player::collides_for_test(&world, p.pos));
        }
    }

    #[test]
    fn example_mod_overrides_and_adds_model_block() {
        let roots = vec!["assets".into(), "examples/crystal".into()];
        let assets = Assets::with_roots(roots);
        let mut cfg = Config::load(&assets).unwrap();
        cfg.world.surface = "crystal".into();
        let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size).unwrap();
        let id = registry.id_of("crystal").expect("crystal block from the mod");
        let crystal = &registry.blocks[id as usize];
        assert!(crystal.model.is_some(), "glTF model should load");
        assert!(!crystal.occluding, "model blocks never hide neighbours");
        assert_ne!(crystal.tiles[1], 0, "crystal.png should replace the missing-texture tile");
        let mut world = World::new(registry, &cfg.world);
        for dz in -1..=1 {
            for dx in -1..=1 {
                world.generate((dx, dz));
            }
        }
        let m = mesh::build(&world, &atlas, (0, 0));
        // 16x16 crystals, 6 triangles each, plus dirt faces visible around them.
        assert!(m.indices.len() >= 16 * 16 * 6 * 3);
    }

    #[test]
    fn png_overrides_flat_color_and_later_roots_win() {
        let dir = std::env::temp_dir().join(format!("tv_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("textures/blocks")).unwrap();
        image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]))
            .save(dir.join("textures/blocks/grass.png"))
            .unwrap();
        let assets = Assets::with_roots(vec!["assets".into(), dir.clone()]);
        let (reg, atlas) = blocks::load(&assets, 8).unwrap();
        let grass = &reg.blocks[reg.id_of("grass").unwrap() as usize];
        let (uv_min, _) = atlas.tile_uv(grass.tiles[1]);
        let px = atlas.image.get_pixel(
            (uv_min[0] * atlas.image.width() as f32) as u32 + 1,
            (uv_min[1] * atlas.image.height() as f32) as u32 + 1,
        );
        assert_eq!(px.0, [1, 2, 3, 255], "PNG from the later root must replace the flat color");
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
}
