//! Chunk -> triangle mesh. Faces hidden by opaque neighbours are skipped.
//! glTF block models are appended with flat per-triangle shading.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::blocks::Atlas;
use crate::world::{ChunkKey, World, BLOCK, CH, CW};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub shade: f32,
}

#[derive(Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// (neighbour offset, 4 corners counter-clockwise seen from outside, atlas slot: 0 top, 1 side, 2 bottom)
pub const FACES: [([i32; 3], [[f32; 3]; 4], usize); 6] = [
    ([1, 0, 0], [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]], 1),
    ([-1, 0, 0], [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]], 1),
    ([0, 0, 1], [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]], 1),
    ([0, 0, -1], [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]], 1),
    ([0, 1, 0], [[0., 1., 1.], [1., 1., 1.], [1., 1., 0.], [0., 1., 0.]], 0),
    ([0, -1, 0], [[0., 0., 0.], [1., 0., 0.], [1., 0., 1.], [0., 0., 1.]], 2),
];
pub const CORNER_UV: [[f32; 2]; 4] = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];

/// Half-lambert shading from a fixed sun direction: gives the flat low-poly look.
pub fn shade(n: Vec3) -> f32 {
    let sun = Vec3::new(0.4, 1.0, 0.25).normalize();
    0.55 + 0.45 * (n.dot(sun) * 0.5 + 0.5)
}

pub fn build(world: &World, atlas: &Atlas, key: ChunkKey) -> MeshData {
    build_from(world, atlas, key, 0)
}

/// Like `build`, but ignores everything below block row `y_min` and closes the mesh
/// there with a floor, giving a clean slab (used for the world preview).
pub fn build_from(world: &World, atlas: &Atlas, key: ChunkKey, y_min: i32) -> MeshData {
    let mut mesh = MeshData::default();
    let Some(chunk) = world.chunk(key) else { return mesh };
    let reg = &world.registry;
    let occ = |x: i32, y: i32, z: i32| if y_min > 0 && y < y_min { false } else { world.occludes(x, y, z) };

    for y in (y_min.max(0) as usize)..=chunk.max_y.min(CH - 1) {
        for z in 0..CW {
            for x in 0..CW {
                let id = chunk.get(x, y, z);
                if id == 0 {
                    continue;
                }
                let block = &reg.blocks[id as usize];
                let (wx, wy, wz) = (key.0 * CW as i32 + x as i32, y as i32, key.1 * CW as i32 + z as i32);

                if let Some(model) = &block.model {
                    let origin = Vec3::new(wx as f32 + 0.5, wy as f32, wz as f32 + 0.5) * BLOCK;
                    push_model(&mut mesh, model, atlas, block.tiles[1], origin);
                    continue;
                }

                for (n, corners, slot) in &FACES {
                    if occ(wx + n[0], wy + n[1], wz + n[2]) {
                        continue;
                    }
                    let (uv_min, uv_max) = atlas.tile_uv(block.tiles[*slot]);
                    let s = shade(Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32));
                    let base = mesh.vertices.len() as u32;
                    for (c, cuv) in corners.iter().zip(CORNER_UV) {
                        mesh.vertices.push(Vertex {
                            pos: [(wx as f32 + c[0]) * BLOCK, (wy as f32 + c[1]) * BLOCK, (wz as f32 + c[2]) * BLOCK],
                            uv: [
                                uv_min[0] + (uv_max[0] - uv_min[0]) * cuv[0],
                                uv_min[1] + (uv_max[1] - uv_min[1]) * cuv[1],
                            ],
                            shade: s,
                        });
                    }
                    mesh.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }
        }
    }
    mesh
}

fn push_model(mesh: &mut MeshData, model: &crate::model::ModelMesh, atlas: &Atlas, tile: u32, origin: Vec3) {
    let (uv_min, uv_max) = atlas.tile_uv(tile);
    for tri in model.indices.chunks_exact(3) {
        let p: Vec<Vec3> = tri.iter().map(|&i| Vec3::from(model.vertices[i as usize]) * BLOCK + origin).collect();
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        let s = shade(n);
        let base = mesh.vertices.len() as u32;
        for (k, &i) in tri.iter().enumerate() {
            let uv = model.uvs[i as usize];
            let (u, v) = (uv[0].clamp(0.0, 1.0), uv[1].clamp(0.0, 1.0));
            mesh.vertices.push(Vertex {
                pos: p[k].to_array(),
                uv: [uv_min[0] + (uv_max[0] - uv_min[0]) * u, uv_min[1] + (uv_max[1] - uv_min[1]) * v],
                shade: s,
            });
        }
        mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
}
