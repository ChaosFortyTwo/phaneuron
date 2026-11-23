//! Infinite chunked world with procedural rolling hills.

use std::collections::HashMap;
use std::sync::Arc;

use glam::Vec3;

use crate::blocks::Registry;
use crate::config::WorldCfg;

pub const CW: usize = 16; // chunk width/depth in blocks
pub const CH: usize = 128; // world height in blocks
/// Edge length of one block in world units. The player is 1.8 units tall, so a
/// block is a little over a quarter of the player's height.
pub const BLOCK: f32 = 0.5;

pub type ChunkKey = (i32, i32);

/// Player edits on top of the generated terrain: chunk -> (index inside chunk -> block id).
pub type Edits = HashMap<ChunkKey, HashMap<u32, u16>>;

/// Result of a block ray cast. `prev` is the empty cell the ray passed through
/// just before hitting `cell` (where a new block would be placed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub cell: [i32; 3],
    pub prev: [i32; 3],
}

pub struct Chunk {
    blocks: Vec<u16>,
    /// Highest y that contains a non-air block.
    pub max_y: usize,
}

impl Chunk {
    #[inline]
    fn idx(x: usize, y: usize, z: usize) -> usize {
        (y * CW + z) * CW + x
    }
    #[inline]
    pub fn get(&self, x: usize, y: usize, z: usize) -> u16 {
        self.blocks[Self::idx(x, y, z)]
    }
}

pub struct World {
    pub registry: Arc<Registry>,
    cfg: WorldCfg,
    seed: u32,
    chunks: HashMap<ChunkKey, Chunk>,
    edits: Edits,
    edits_dirty: bool,
    surface: u16,
    subsurface: u16,
    deep: u16,
}

// ---------- noise ----------

fn hash2(ix: i32, iz: i32, seed: u32) -> f32 {
    let mut h = (ix as u32).wrapping_mul(0x27d4_eb2d)
        ^ (iz as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

fn value_noise(x: f32, z: f32, seed: u32) -> f32 {
    let (x0, z0) = (x.floor(), z.floor());
    let (fx, fz) = (x - x0, z - z0);
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let (ix, iz) = (x0 as i32, z0 as i32);
    let a = hash2(ix, iz, seed);
    let b = hash2(ix + 1, iz, seed);
    let c = hash2(ix, iz + 1, seed);
    let d = hash2(ix + 1, iz + 1, seed);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sz
}

/// Fractal noise, roughly 0..1.
fn fbm(x: f32, z: f32, seed: u32, octaves: u32) -> f32 {
    let (mut sum, mut amp, mut freq, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..octaves.max(1) {
        sum += amp * value_noise(x * freq, z * freq, seed.wrapping_add(o * 101));
        norm += amp;
        amp *= 0.5;
        freq *= 2.0;
    }
    sum / norm
}

// ---------- world ----------

impl World {
    pub fn new(registry: Arc<Registry>, cfg: &WorldCfg, seed: u32) -> Self {
        let find = |name: &str| -> u16 {
            registry.id_of(name).unwrap_or_else(|| {
                eprintln!("warning: world.toml block '{name}' not found, using a fallback");
                registry.first_solid()
            })
        };
        let (surface, subsurface, deep) = (find(&cfg.surface), find(&cfg.subsurface), find(&cfg.deep));
        World {
            registry,
            cfg: cfg.clone(),
            seed,
            chunks: HashMap::new(),
            edits: HashMap::new(),
            edits_dirty: false,
            surface,
            subsurface,
            deep,
        }
    }

    pub fn edits(&self) -> &Edits {
        &self.edits
    }

    /// Install edits loaded from disk. Call before generating chunks.
    pub fn set_edits(&mut self, edits: Edits) {
        self.edits = edits;
    }

    pub fn mark_dirty(&mut self) {
        self.edits_dirty = true;
    }

    /// True once if edits changed since the last call (used to skip needless saves).
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.edits_dirty)
    }

    /// Top solid block y of the terrain column.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        let c = &self.cfg;
        let n = fbm(x as f32 * c.frequency, z as f32 * c.frequency, self.seed, c.octaves);
        let h = c.base_height + (n - 0.5) * 2.0 * c.amplitude;
        (h.round() as i32).clamp(1, CH as i32 - 2)
    }

    pub fn has_chunk(&self, key: ChunkKey) -> bool {
        self.chunks.contains_key(&key)
    }

    pub fn chunk(&self, key: ChunkKey) -> Option<&Chunk> {
        self.chunks.get(&key)
    }

    pub fn generate(&mut self, (cx, cz): ChunkKey) {
        let mut blocks = vec![0u16; CW * CW * CH];
        let mut max_y = 0;
        for lz in 0..CW {
            for lx in 0..CW {
                let h = self.height_at(cx * CW as i32 + lx as i32, cz * CW as i32 + lz as i32);
                for y in 0..=h {
                    let id = if y == h {
                        self.surface
                    } else if y >= h - self.cfg.subsurface_depth {
                        self.subsurface
                    } else {
                        self.deep
                    };
                    blocks[Chunk::idx(lx, y as usize, lz)] = id;
                }
                max_y = max_y.max(h as usize);
            }
        }
        if let Some(chunk_edits) = self.edits.get(&(cx, cz)) {
            for (&idx, &id) in chunk_edits {
                if (idx as usize) < blocks.len() && (id as usize) < self.registry.blocks.len() {
                    blocks[idx as usize] = id;
                    if id != 0 {
                        max_y = max_y.max(idx as usize / (CW * CW));
                    }
                }
            }
        }
        self.chunks.insert((cx, cz), Chunk { blocks, max_y });
    }

    /// Change one block. Returns the chunks whose meshes must be rebuilt
    /// (the edited chunk plus neighbours when the edit touches a chunk border).
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u16) -> Vec<ChunkKey> {
        if y < 0 || y >= CH as i32 {
            return Vec::new();
        }
        let (cx, cz) = (x.div_euclid(CW as i32), z.div_euclid(CW as i32));
        let (lx, lz) = (x.rem_euclid(CW as i32) as usize, z.rem_euclid(CW as i32) as usize);
        let Some(chunk) = self.chunks.get_mut(&(cx, cz)) else { return Vec::new() };
        let idx = Chunk::idx(lx, y as usize, lz);
        if chunk.blocks[idx] == id {
            return Vec::new();
        }
        chunk.blocks[idx] = id;
        if id != 0 {
            chunk.max_y = chunk.max_y.max(y as usize);
        }
        self.edits.entry((cx, cz)).or_default().insert(idx as u32, id);
        self.edits_dirty = true;

        let mut affected = vec![(cx, cz)];
        if lx == 0 {
            affected.push((cx - 1, cz));
        }
        if lx == CW - 1 {
            affected.push((cx + 1, cz));
        }
        if lz == 0 {
            affected.push((cx, cz - 1));
        }
        if lz == CW - 1 {
            affected.push((cx, cz + 1));
        }
        affected
    }

    /// Walk a ray through the block grid (Amanatides & Woo) and return the first
    /// non-air block within `reach` world units. `origin` and `dir` are in world units.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, reach: f32) -> Option<Hit> {
        let d = dir.normalize_or_zero();
        if d == Vec3::ZERO {
            return None;
        }
        let o = origin / BLOCK;
        let max_t = reach / BLOCK;
        let mut cell = [o.x.floor() as i32, o.y.floor() as i32, o.z.floor() as i32];
        if self.get(cell[0], cell[1], cell[2]) != 0 {
            return None; // camera is inside a block
        }
        let mut step = [0i32; 3];
        let mut t_max = [f32::INFINITY; 3];
        let mut t_delta = [f32::INFINITY; 3];
        for i in 0..3 {
            if d[i] > 0.0 {
                step[i] = 1;
                t_delta[i] = 1.0 / d[i];
                t_max[i] = (cell[i] as f32 + 1.0 - o[i]) / d[i];
            } else if d[i] < 0.0 {
                step[i] = -1;
                t_delta[i] = -1.0 / d[i];
                t_max[i] = (o[i] - cell[i] as f32) / -d[i];
            }
        }
        loop {
            let axis = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };
            if t_max[axis] > max_t {
                return None;
            }
            let prev = cell;
            cell[axis] += step[axis];
            t_max[axis] += t_delta[axis];
            if self.get(cell[0], cell[1], cell[2]) != 0 {
                return Some(Hit { cell, prev });
            }
        }
    }

    pub fn get(&self, x: i32, y: i32, z: i32) -> u16 {
        if y < 0 || y >= CH as i32 {
            return 0;
        }
        let (cx, cz) = (x.div_euclid(CW as i32), z.div_euclid(CW as i32));
        match self.chunks.get(&(cx, cz)) {
            Some(c) => c.get(x.rem_euclid(CW as i32) as usize, y as usize, z.rem_euclid(CW as i32) as usize),
            None => 0,
        }
    }

    /// For collisions: unloaded chunks and the void below the world count as solid,
    /// so the player can never fall through ground that has not been generated yet.
    pub fn is_solid(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return true;
        }
        if y >= CH as i32 {
            return false;
        }
        let (cx, cz) = (x.div_euclid(CW as i32), z.div_euclid(CW as i32));
        match self.chunks.get(&(cx, cz)) {
            Some(c) => {
                let id = c.get(x.rem_euclid(CW as i32) as usize, y as usize, z.rem_euclid(CW as i32) as usize);
                self.registry.blocks[id as usize].solid
            }
            None => true,
        }
    }

    /// For meshing: does this cell hide the face next to it?
    pub fn occludes(&self, x: i32, y: i32, z: i32) -> bool {
        if y < 0 {
            return true;
        }
        self.registry.blocks[self.get(x, y, z) as usize].occluding
    }

    /// Chunks inside the generation circle that are not loaded yet, nearest first.
    pub fn missing_chunks(&self, center: ChunkKey, render_distance: i32) -> Vec<ChunkKey> {
        let r = render_distance + 1;
        let mut wanted: Vec<(i32, ChunkKey)> = Vec::new();
        for dz in -r..=r {
            for dx in -r..=r {
                let d2 = dx * dx + dz * dz;
                let key = (center.0 + dx, center.1 + dz);
                if d2 <= r * r && !self.chunks.contains_key(&key) {
                    wanted.push((d2, key));
                }
            }
        }
        wanted.sort_unstable();
        wanted.into_iter().map(|(_, k)| k).collect()
    }

    /// Drop chunks far outside the view; returns the dropped keys.
    pub fn unload_far(&mut self, center: ChunkKey, render_distance: i32) -> Vec<ChunkKey> {
        let limit = render_distance + 3;
        let far: Vec<ChunkKey> = self
            .chunks
            .keys()
            .copied()
            .filter(|k| (k.0 - center.0).abs() > limit || (k.1 - center.1).abs() > limit)
            .collect();
        for k in &far {
            self.chunks.remove(k);
        }
        far
    }

    /// Generate up to `budget` missing chunks near `center` (nearest first) and
    /// drop chunks that are far away. Returns the keys that were dropped.
    #[cfg(test)]
    pub fn stream(&mut self, center: ChunkKey, render_distance: i32, budget: usize) -> Vec<ChunkKey> {
        for key in self.missing_chunks(center, render_distance).into_iter().take(budget) {
            self.generate(key);
        }
        self.unload_far(center, render_distance)
    }

    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Chunks inside the render circle that can be meshed (all 4 neighbours loaded), nearest first.
    pub fn mesh_candidates(&self, center: ChunkKey, render_distance: i32) -> Vec<ChunkKey> {
        let r = render_distance;
        let mut out: Vec<(i32, ChunkKey)> = Vec::new();
        for dz in -r..=r {
            for dx in -r..=r {
                let d2 = dx * dx + dz * dz;
                if d2 > r * r {
                    continue;
                }
                let k = (center.0 + dx, center.1 + dz);
                let ready = [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .all(|(ox, oz)| self.chunks.contains_key(&(k.0 + ox, k.1 + oz)));
                if ready {
                    out.push((d2, k));
                }
            }
        }
        out.sort_unstable();
        out.into_iter().map(|(_, k)| k).collect()
    }
}
