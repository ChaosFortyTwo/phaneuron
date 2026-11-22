//! Infinite chunked world with procedural rolling hills.

use std::collections::HashMap;

use crate::blocks::Registry;
use crate::config::WorldCfg;

pub const CW: usize = 16; // chunk width/depth in blocks
pub const CH: usize = 96; // world height in blocks

pub type ChunkKey = (i32, i32);

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
    pub registry: Registry,
    cfg: WorldCfg,
    chunks: HashMap<ChunkKey, Chunk>,
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
    pub fn new(registry: Registry, cfg: &WorldCfg) -> Self {
        let find = |name: &str| -> u16 {
            registry.id_of(name).unwrap_or_else(|| {
                eprintln!("warning: world.toml block '{name}' not found, using a fallback");
                registry.first_solid()
            })
        };
        let (surface, subsurface, deep) = (find(&cfg.surface), find(&cfg.subsurface), find(&cfg.deep));
        World { registry, cfg: cfg.clone(), chunks: HashMap::new(), surface, subsurface, deep }
    }

    /// Top solid block y of the terrain column.
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        let c = &self.cfg;
        let n = fbm(x as f32 * c.frequency, z as f32 * c.frequency, c.seed, c.octaves);
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
        self.chunks.insert((cx, cz), Chunk { blocks, max_y });
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

    /// Generate up to `budget` missing chunks near `center` (nearest first) and
    /// drop chunks that are far away. Returns the keys that were dropped.
    pub fn stream(&mut self, center: ChunkKey, render_distance: i32, budget: usize) -> Vec<ChunkKey> {
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
        for (_, key) in wanted.into_iter().take(budget) {
            self.generate(key);
        }

        let limit = r + 2;
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
