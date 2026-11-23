//! One running world: player, terrain streaming, block editing, camera, HUD,
//! debug screen, console, saving.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use glam::Vec3;

use crate::blocks::{Atlas, Registry};
use crate::config::Config;
use crate::input::Input;
use crate::mesh::{self, MeshData};
use crate::player::{Keys, Player, HALF_WIDTH};
use crate::player_model;
use crate::render::{Camera, Renderer};
use crate::save::{self, PlayerSave, SaveData, Saved, WorldMeta};
use crate::ui::{col, col_a, Tex, Ui, GLYPH, WHITE};
use crate::world::{Hit, World, BLOCK, CH, CW};

/// Time per frame spent generating and meshing chunks.
const GEN_BUDGET: Duration = Duration::from_millis(4);
const MESH_BUDGET: Duration = Duration::from_millis(6);
const AUTOSAVE: Duration = Duration::from_secs(60);
const REPEAT_DELAY: f32 = 0.2;
const HOTBAR_SLOTS: usize = 9;
const THIRD_PERSON_DISTANCE: f32 = 3.5;
const CONSOLE_LINES: usize = 12;
const MAX_COORD: f32 = 1_000_000.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    First,
    ThirdBack,
    ThirdFront,
}

#[derive(Default)]
pub struct Console {
    pub open: bool,
    pub input: String,
    /// (text, age in seconds)
    pub lines: Vec<(String, f32)>,
}

pub struct Session {
    pub world: World,
    pub player: Player,
    pub meta: WorldMeta,
    /// Index into the registry's non-air blocks.
    pub selected: usize,
    pub target: Option<Hit>,
    pub view: View,
    pub console: Console,
    cooldown: f32,
    last_save: Instant,
}

pub fn chunk_of(p: Vec3) -> (i32, i32) {
    let cell = |v: f32| (v / BLOCK).floor() as i32;
    (cell(p.x).div_euclid(CW as i32), cell(p.z).div_euclid(CW as i32))
}

impl Session {
    pub fn start(registry: Arc<Registry>, cfg: &Config, meta: WorldMeta, saved: Option<Saved>) -> Session {
        let mut world = World::new(registry.clone(), &cfg.world, meta.seed);
        let mut selected = 0;
        let mut saved_player = None;
        if let Some(s) = saved {
            world.set_edits(s.edits);
            saved_player = s.player;
            if let Some(name) = s.selected_block {
                if let Some(id) = registry.id_of(&name) {
                    selected = id as usize - 1;
                }
            }
        }

        let player = match saved_player {
            Some(p) => {
                let mut pl = Player::new(Vec3::new(p.x, p.y, p.z));
                pl.yaw = p.yaw;
                pl.pitch = p.pitch;
                pl
            }
            None => {
                let h = world.height_at(0, 0) as f32;
                Player::new(Vec3::new(0.5 * BLOCK, (h + 1.0) * BLOCK, 0.5 * BLOCK))
            }
        };

        // Generate the area around the player up front so the first frame has ground.
        let (cx, cz) = chunk_of(player.pos);
        for dz in -2..=2 {
            for dx in -2..=2 {
                world.generate((cx + dx, cz + dz));
            }
        }

        Session {
            world,
            player,
            meta,
            selected,
            target: None,
            view: View::First,
            console: Console::default(),
            cooldown: 0.0,
            last_save: Instant::now(),
        }
    }

    fn block_count(&self) -> usize {
        self.world.registry.blocks.len() - 1
    }

    fn selected_id(&self) -> u16 {
        (self.selected + 1) as u16
    }

    /// First-person eye position (also where block targeting starts).
    pub fn eye(&self) -> Vec3 {
        self.player.pos + Vec3::new(0.0, self.player.eye + self.player.visual_dy, 0.0)
    }

    /// Distance you can move from `origin` along `dir` before hitting a block (at most `max`).
    fn free_distance(&self, origin: Vec3, dir: Vec3, max: f32) -> f32 {
        let mut t = 0.1;
        while t < max {
            let p = (origin + dir * t) / BLOCK;
            if self.world.get(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32) != 0 {
                return (t - 0.2).max(0.1);
            }
            t += 0.05;
        }
        max
    }

    pub fn camera(&self) -> Camera {
        let eye = self.eye();
        let fwd = self.player.forward();
        match self.view {
            View::First => Camera { eye, forward: fwd },
            View::ThirdBack => {
                let d = self.free_distance(eye, -fwd, THIRD_PERSON_DISTANCE);
                Camera { eye: eye - fwd * d, forward: fwd }
            }
            View::ThirdFront => {
                let d = self.free_distance(eye, fwd, THIRD_PERSON_DISTANCE);
                Camera { eye: eye + fwd * d, forward: -fwd }
            }
        }
    }

    pub fn cycle_view(&mut self) {
        self.view = match self.view {
            View::First => View::ThirdBack,
            View::ThirdBack => View::ThirdFront,
            View::ThirdFront => View::First,
        };
    }

    /// The player figure, only drawn in third person.
    pub fn model_mesh(&self, atlas: &Atlas) -> Option<MeshData> {
        (self.view != View::First).then(|| player_model::build(&self.player, &self.world.registry.player_tiles, atlas))
    }

    /// Outline box of the block under the crosshair.
    pub fn highlight(&self) -> Option<(Vec3, Vec3)> {
        self.target.map(|h| {
            let lo = Vec3::new(h.cell[0] as f32, h.cell[1] as f32, h.cell[2] as f32) * BLOCK;
            (lo, lo + Vec3::splat(BLOCK))
        })
    }

    pub fn update(&mut self, dt: f32, keys: &Keys, cfg: &Config) {
        self.player.update(dt, keys, &self.world, &cfg.player);
        self.target = self.world.raycast(self.eye(), self.player.forward(), cfg.player.reach);
        for line in &mut self.console.lines {
            line.1 += dt;
        }
        self.console.lines.retain(|l| l.1 < 120.0);
    }

    /// Generate nearby chunks and build meshes, within a per-frame time budget.
    pub fn stream(&mut self, renderer: &mut Renderer, atlas: &Atlas, cfg: &Config) {
        let center = chunk_of(self.player.pos);
        let rd = cfg.render.render_distance;
        for key in self.world.unload_far(center, rd) {
            renderer.remove_chunk(key);
        }

        let start = Instant::now();
        for key in self.world.missing_chunks(center, rd) {
            self.world.generate(key);
            if start.elapsed() >= GEN_BUDGET {
                break;
            }
        }

        let start = Instant::now();
        for key in self.world.mesh_candidates(center, rd) {
            if renderer.has_mesh(key) {
                continue;
            }
            renderer.upload(key, mesh::build(&self.world, atlas, key));
            if start.elapsed() >= MESH_BUDGET {
                break;
            }
        }
    }

    pub fn select_delta(&mut self, delta: i32) {
        let n = self.block_count() as i32;
        self.selected = (self.selected as i32 + delta).rem_euclid(n) as usize;
    }

    pub fn select(&mut self, index: usize) {
        if index < self.block_count() {
            self.selected = index;
        }
    }

    fn remesh(&self, keys: Vec<(i32, i32)>, renderer: &mut Renderer, atlas: &Atlas) {
        for key in keys {
            if self.world.has_chunk(key) {
                renderer.upload(key, mesh::build(&self.world, atlas, key));
            }
        }
    }

    fn overlaps_player(&self, cell: [i32; 3]) -> bool {
        let lo = Vec3::new(cell[0] as f32, cell[1] as f32, cell[2] as f32) * BLOCK;
        let hi = lo + Vec3::splat(BLOCK);
        let p = self.player.pos;
        let (pmin, pmax) = (
            Vec3::new(p.x - HALF_WIDTH, p.y, p.z - HALF_WIDTH),
            Vec3::new(p.x + HALF_WIDTH, p.y + self.player.height(), p.z + HALF_WIDTH),
        );
        lo.x < pmax.x && hi.x > pmin.x && lo.y < pmax.y && hi.y > pmin.y && lo.z < pmax.z && hi.z > pmin.z
    }

    /// Break with the left button, place with the right (hold to repeat).
    pub fn interact(&mut self, dt: f32, input: &Input, renderer: &mut Renderer, atlas: &Atlas) {
        self.cooldown = (self.cooldown - dt).max(0.0);
        let Some(hit) = self.target else { return };
        let ready = self.cooldown <= 0.0;
        if input.click || (input.left_held && ready) {
            self.break_block(hit, renderer, atlas);
        } else if input.right_click || (input.right_held && ready) {
            self.place_block(hit, renderer, atlas);
        }
    }

    pub fn break_block(&mut self, hit: Hit, renderer: &mut Renderer, atlas: &Atlas) {
        if hit.cell[1] <= 0 {
            return; // keep the bottom layer so nobody falls out of the world
        }
        let affected = self.world.set_block(hit.cell[0], hit.cell[1], hit.cell[2], 0);
        self.remesh(affected, renderer, atlas);
        self.cooldown = REPEAT_DELAY;
    }

    pub fn place_block(&mut self, hit: Hit, renderer: &mut Renderer, atlas: &Atlas) {
        let c = hit.prev;
        if c[1] < 0 || c[1] >= CH as i32 || self.overlaps_player(c) {
            return;
        }
        let affected = self.world.set_block(c[0], c[1], c[2], self.selected_id());
        self.remesh(affected, renderer, atlas);
        self.cooldown = REPEAT_DELAY;
    }

    pub fn autosave(&mut self) {
        if self.last_save.elapsed() >= AUTOSAVE {
            if let Err(e) = self.save() {
                eprintln!("autosave failed: {e:#}");
            }
        }
    }

    pub fn save(&mut self) -> Result<()> {
        self.last_save = Instant::now();
        let p = &self.player;
        let data = SaveData {
            player: PlayerSave { x: p.pos.x, y: p.pos.y, z: p.pos.z, yaw: p.yaw, pitch: p.pitch },
            selected_block: self.world.registry.blocks[self.selected_id() as usize].id.clone(),
        };
        let dirty = self.world.take_dirty();
        let r = save::save_world(&self.meta, &data, self.world.edits(), &self.world.registry, dirty);
        if r.is_err() && dirty {
            // keep the "needs saving" flag so the next attempt writes the edits again
            self.world.mark_dirty();
        }
        r
    }

    // ---------- console commands ----------

    fn say(&mut self, text: impl Into<String>) {
        self.console.lines.push((text.into(), 0.0));
        if self.console.lines.len() > 200 {
            self.console.lines.remove(0);
        }
    }

    /// Run one console line such as `/tp 10 40 -5`. Output goes to the console.
    pub fn run_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.say(format!("> {line}"));
        let mut parts = line.trim_start_matches('/').split_whitespace();
        let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
        let args: Vec<&str> = parts.collect();
        match cmd.as_str() {
            "tp" => self.cmd_tp(&args),
            "switch" => {
                self.player.flying = !self.player.flying;
                self.player.vel_y = 0.0;
                let msg = if self.player.flying {
                    "Free flying ON (jump = up, sneak = down, sprint = faster). /switch to land."
                } else {
                    "Free flying OFF."
                };
                self.say(msg);
            }
            "help" => {
                self.say("/tp <x> <y> <z>   teleport (world units, ~ = relative)");
                self.say("/tp <x> <z>       teleport to the surface there");
                self.say("/switch           toggle free flying");
            }
            _ => self.say(format!("Unknown command '{cmd}'. Try /help")),
        }
    }

    fn cmd_tp(&mut self, args: &[&str]) {
        let usage = "Usage: /tp <x> <y> <z>  or  /tp <x> <z>   (numbers in world units, ~ is relative)";
        if args.len() != 2 && args.len() != 3 {
            return self.say(usage);
        }
        let cur = self.player.pos;
        let parse = |s: &str, base: f32| -> Option<f32> {
            let v = if let Some(rest) = s.strip_prefix('~') {
                base + if rest.is_empty() { 0.0 } else { rest.parse::<f32>().ok()? }
            } else {
                s.parse::<f32>().ok()?
            };
            v.is_finite().then_some(v)
        };
        let (x, y, z) = if args.len() == 3 {
            (parse(args[0], cur.x), parse(args[1], cur.y).map(Some), parse(args[2], cur.z))
        } else {
            (parse(args[0], cur.x), Some(None), parse(args[1], cur.z))
        };
        let (Some(x), Some(y), Some(z)) = (x, y, z) else { return self.say(usage) };
        let (x, z) = (x.clamp(-MAX_COORD, MAX_COORD), z.clamp(-MAX_COORD, MAX_COORD));

        // Load the destination right away so we never land inside ungenerated ground.
        let (cx, cz) = chunk_of(Vec3::new(x, 0.0, z));
        for dz in -1..=1 {
            for dx in -1..=1 {
                if !self.world.has_chunk((cx + dx, cz + dz)) {
                    self.world.generate((cx + dx, cz + dz));
                }
            }
        }
        let height = self.player.height();
        let mut y = match y {
            Some(y) => y.max(0.0),
            None => {
                let (bx, bz) = ((x / BLOCK).floor() as i32, (z / BLOCK).floor() as i32);
                (self.world.height_at(bx, bz) as f32 + 1.0) * BLOCK
            }
        };
        // If the target is inside terrain, rise to the first free spot.
        let top = CH as f32 * BLOCK;
        while Player::collides(&self.world, Vec3::new(x, y, z), height) && y < top {
            y += BLOCK;
        }
        self.player.pos = Vec3::new(x, y, z);
        self.player.vel_y = 0.0;
        self.say(format!("Teleported to {:.2} {:.2} {:.2}", x, y, z));
    }

    /// Typing, Enter and Esc for the console, then draw it. Call every frame.
    pub fn console_ui(&mut self, ui: &mut Ui, input: &Input) {
        if self.console.open {
            for c in input.typed.chars() {
                if (c.is_ascii_graphic() || c == ' ') && self.console.input.len() < 120 {
                    self.console.input.push(c);
                }
            }
            if input.backspace {
                self.console.input.pop();
            }
            if input.enter {
                let line = std::mem::take(&mut self.console.input);
                self.run_command(&line);
            }
            if input.escape {
                self.console.open = false;
            }
        }

        let max_chars = ((ui.w - 8.0) / GLYPH) as usize;
        let fit = |s: &str| -> String { s.chars().take(max_chars).collect() };
        let n = if self.console.open { CONSOLE_LINES } else { 6 };
        let visible: Vec<&String> = self
            .console
            .lines
            .iter()
            .filter(|l| self.console.open || l.1 < 6.0)
            .rev()
            .take(n)
            .map(|l| &l.0)
            .collect();

        let row = 10.0;
        let input_y = ui.h - 16.0;
        let base_y = if self.console.open { input_y - 4.0 } else { ui.h - 44.0 };
        if self.console.open {
            let top = base_y - visible.len() as f32 * row - 6.0;
            ui.dl.rect(0.0, top, ui.w, ui.h - top, col_a(0x000000, 0.72));
        }
        for (i, line) in visible.iter().enumerate() {
            let y = base_y - (i as f32 + 1.0) * row;
            ui.dl.text_shadow(4.0, y, 1.0, &fit(line), col(0xe8eef8));
        }
        if self.console.open {
            let shown = fit(&format!("> {}", self.console.input));
            let w = ui.dl.text(4.0, input_y, 1.0, &shown, col(0xffffff));
            if (ui.time * 2.0) % 2.0 < 1.0 {
                ui.dl.text(4.0 + w, input_y, 1.0, "_", col(0xffe9a8));
            }
        }
    }

    // ---------- HUD ----------

    /// Lines for the F3 screen.
    pub fn debug_lines(&self, fps: f32, cfg: &Config) -> Vec<String> {
        let p = &self.player;
        let cell = |v: f32| (v / BLOCK).floor() as i32;
        let (ch_x, ch_z) = chunk_of(p.pos);
        let looking = match self.target {
            Some(h) => {
                let id = self.world.get(h.cell[0], h.cell[1], h.cell[2]);
                let b = &self.world.registry.blocks[id as usize];
                format!("{} (id {}, \"{}\") at {} {} {}", b.name, id, b.id, h.cell[0], h.cell[1], h.cell[2])
            }
            None => "nothing in reach".to_string(),
        };
        let mode = if p.flying {
            "flying"
        } else if p.sneaking {
            "sneaking"
        } else if p.on_ground {
            "walking"
        } else {
            "airborne"
        };
        let view = match self.view {
            View::First => "first person",
            View::ThirdBack => "third person (back)",
            View::ThirdFront => "third person (front)",
        };
        vec![
            format!("Phaneuron  {fps:.0} fps"),
            format!("XYZ: {:.2} / {:.2} / {:.2}", p.pos.x, p.pos.y, p.pos.z),
            format!("Block: {} / {} / {}   Chunk: {} / {}", cell(p.pos.x), cell(p.pos.y), cell(p.pos.z), ch_x, ch_z),
            format!("Facing: yaw {:.0} deg, pitch {:.0} deg", p.yaw.to_degrees().rem_euclid(360.0), p.pitch.to_degrees()),
            format!("Looking at: {looking}"),
            format!("Mode: {mode}   View: {view}"),
            format!("Render distance: {} chunks   Loaded: {}", cfg.render.render_distance, self.world.chunk_count()),
            format!("World: {}   Seed: {}", self.meta.name, self.meta.seed),
        ]
    }

    pub fn debug_overlay(&self, ui: &mut Ui, fps: f32, cfg: &Config) {
        let lines = self.debug_lines(fps, cfg);
        let max_chars = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let w = (max_chars as f32 * GLYPH + 10.0).min(ui.w - 4.0);
        ui.dl.rect(2.0, 2.0, w, lines.len() as f32 * 11.0 + 6.0, col_a(0x000000, 0.55));
        for (i, l) in lines.iter().enumerate() {
            let color = if i == 0 { col(0xffe9a8) } else { col(0xffffff) };
            ui.dl.text(6.0, 5.0 + i as f32 * 11.0, 1.0, l, color);
        }
    }

    /// Crosshair and hotbar.
    pub fn hud(&self, ui: &mut Ui, atlas: &Atlas) {
        if let Some((cw, ch)) = ui.crosshair {
            let (cw, ch) = (cw as f32, ch as f32);
            ui.dl.quad(Tex::Crosshair, (ui.w - cw) / 2.0, (ui.h - ch) / 2.0, cw, ch, [0.0, 0.0], [1.0, 1.0], WHITE);
        }

        let n = self.block_count();
        let shown = n.min(HOTBAR_SLOTS);
        let start = self.selected.saturating_sub(HOTBAR_SLOTS / 2).min(n - shown);
        let (slot, gap) = (22.0, 2.0);
        let total = shown as f32 * slot + (shown as f32 - 1.0) * gap;
        let x0 = (ui.w - total) / 2.0;
        let y = ui.h - slot - 8.0;
        for i in 0..shown {
            let idx = start + i;
            let x = x0 + i as f32 * (slot + gap);
            let active = idx == self.selected;
            ui.dl.panel(x, y, slot, slot, col_a(0x10131b, 0.8), if active { col(0xffffff) } else { col(0x0b0d12) });
            let tile = self.world.registry.blocks[idx + 1].tiles[1];
            let (uv0, uv1) = atlas.tile_uv(tile);
            ui.dl.quad(Tex::Atlas, x + 3.0, y + 3.0, slot - 6.0, slot - 6.0, uv0, uv1, WHITE);
            if active {
                ui.dl.outline(x - 1.0, y - 1.0, slot + 2.0, slot + 2.0, col(0xffe9a8));
            }
            if idx < 9 && !active {
                ui.dl.text(x + 2.0, y + 2.0, 1.0, &(idx + 1).to_string(), col_a(0xffffff, 0.7));
            }
        }
        let name = &self.world.registry.blocks[self.selected_id() as usize].name;
        ui.dl.text_centered(ui.w / 2.0, y - 14.0, 1.0, name, col(0xffffff));
    }
}
