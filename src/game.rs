//! One running world: player, terrain streaming, block editing, HUD, saving.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use glam::Vec3;

use crate::blocks::{Atlas, Registry};
use crate::config::Config;
use crate::input::Input;
use crate::mesh;
use crate::player::{Keys, Player, HALF_WIDTH, HEIGHT};
use crate::render::{Camera, Renderer};
use crate::save::{self, PlayerSave, SaveData, Saved, WorldMeta};
use crate::ui::{col, col_a, Tex, Ui, WHITE};
use crate::world::{Hit, World, BLOCK, CH, CW};

const GEN_PER_FRAME: usize = 8;
const MESH_PER_FRAME: usize = 4;
const AUTOSAVE: Duration = Duration::from_secs(60);
const REPEAT_DELAY: f32 = 0.2;
const HOTBAR_SLOTS: usize = 9;

pub struct Session {
    pub world: World,
    pub player: Player,
    pub meta: WorldMeta,
    /// Index into the registry's non-air blocks.
    pub selected: usize,
    pub target: Option<Hit>,
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

        Session { world, player, meta, selected, target: None, cooldown: 0.0, last_save: Instant::now() }
    }

    fn block_count(&self) -> usize {
        self.world.registry.blocks.len() - 1
    }

    fn selected_id(&self) -> u16 {
        (self.selected + 1) as u16
    }

    pub fn eye(&self, cfg: &Config) -> Vec3 {
        self.player.pos + Vec3::new(0.0, cfg.player.eye_height + self.player.visual_dy, 0.0)
    }

    pub fn camera(&self, cfg: &Config) -> Camera {
        Camera { eye: self.eye(cfg), forward: self.player.forward() }
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
        self.target = self.world.raycast(self.eye(cfg), self.player.forward(), cfg.player.reach);
    }

    /// Generate nearby chunks and build a few meshes per frame.
    pub fn stream(&mut self, renderer: &mut Renderer, atlas: &Atlas, cfg: &Config) {
        let center = chunk_of(self.player.pos);
        let rd = cfg.render.render_distance;
        for key in self.world.stream(center, rd, GEN_PER_FRAME) {
            renderer.remove_chunk(key);
        }
        let mut built = 0;
        for key in self.world.mesh_candidates(center, rd) {
            if built >= MESH_PER_FRAME {
                break;
            }
            if renderer.has_mesh(key) {
                continue;
            }
            renderer.upload(key, mesh::build(&self.world, atlas, key));
            built += 1;
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
        let (pmin, pmax) = (Vec3::new(p.x - HALF_WIDTH, p.y, p.z - HALF_WIDTH), Vec3::new(p.x + HALF_WIDTH, p.y + HEIGHT, p.z + HALF_WIDTH));
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
