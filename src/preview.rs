//! The 3D world preview on the Load World screen: the 3x3 chunks around the spot
//! where the player last stood (their chunk in the middle), cut into a slab, with the
//! player figure standing where they left off. Edits you made are included.

use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use glam::Vec3;

use crate::blocks::{Atlas, Registry};
use crate::config::Config;
use crate::game::chunk_of;
use crate::mesh::{self, MeshData};
use crate::player::Player;
use crate::player_model;
use crate::save::{self, WorldMeta};
use crate::world::{World, BLOCK, CW};

#[derive(Clone, Debug)]
pub struct PreviewInfo {
    pub folder: PathBuf,
    /// Where the player last stood (world units).
    pub pos: Vec3,
    pub edited_blocks: usize,
}

pub enum PreviewState {
    None,
    Ready(PreviewInfo),
    Failed(PathBuf, String),
}

pub struct Built {
    /// 9 terrain chunks followed by the player figure.
    pub meshes: Vec<MeshData>,
    /// What the preview camera orbits around.
    pub target: Vec3,
    pub info: PreviewInfo,
}

/// How many blocks of ground to keep below the lowest surface point of the area.
const SLAB_DEPTH: i32 = 6;

pub fn build(registry: &Arc<Registry>, atlas: &Atlas, cfg: &Config, meta: &WorldMeta) -> Result<Built> {
    let saved = save::load_world(meta, registry)?;
    let edited_blocks = saved.edits.values().map(|e| e.len()).sum();

    let mut world = World::new(registry.clone(), &cfg.world, meta.seed);
    world.set_edits(saved.edits);

    let (pos, yaw) = match saved.player {
        Some(p) => (Vec3::new(p.x, p.y, p.z), p.yaw),
        None => {
            let h = world.height_at(0, 0) as f32;
            (Vec3::new(0.5 * BLOCK, (h + 1.0) * BLOCK, 0.5 * BLOCK), 0.0)
        }
    };
    let (cx, cz) = chunk_of(pos);
    for dz in -1..=1 {
        for dx in -1..=1 {
            world.generate((cx + dx, cz + dz));
        }
    }

    // size of the slab: from just below the lowest ground in the area up to the surface
    let (x0, z0, n) = ((cx - 1) * CW as i32, (cz - 1) * CW as i32, 3 * CW as i32);
    let (mut min_h, mut sum, mut count) = (i32::MAX, 0i64, 0i64);
    for z in (0..n).step_by(2) {
        for x in (0..n).step_by(2) {
            let h = world.height_at(x0 + x, z0 + z);
            min_h = min_h.min(h);
            sum += h as i64;
            count += 1;
        }
    }
    let avg_h = sum as f32 / count as f32;
    let y_min = (min_h - SLAB_DEPTH).max(0);

    let mut meshes = Vec::with_capacity(10);
    for dz in -1..=1 {
        for dx in -1..=1 {
            meshes.push(mesh::build_from(&world, atlas, (cx + dx, cz + dz), y_min));
        }
    }
    let mut figure = Player::new(pos);
    figure.yaw = yaw;
    meshes.push(player_model::build(&figure, &registry.player_tiles, atlas));

    let target = Vec3::new(
        (cx as f32 * CW as f32 + CW as f32 / 2.0) * BLOCK,
        (y_min as f32 + avg_h) / 2.0 * BLOCK,
        (cz as f32 * CW as f32 + CW as f32 / 2.0) * BLOCK,
    );
    Ok(Built { meshes, target, info: PreviewInfo { folder: meta.folder.clone(), pos, edited_blocks } })
}
