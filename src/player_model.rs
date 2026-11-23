//! A provisional blocky player figure, built from boxes every frame.
//! Parts take their look from `textures/player/{head,body,arm,leg,eye}.png`
//! or fall back to flat colors. Limbs swing while walking; the head follows the pitch.

use glam::{Mat4, Vec3};

use crate::blocks::Atlas;
use crate::mesh::{shade, MeshData, Vertex, CORNER_UV, FACES};
use crate::player::{Player, HEIGHT};

struct Part {
    pivot: Vec3,
    min: Vec3,
    max: Vec3,
    angle: f32,
    tile: usize, // index into Registry::player_tiles
}

const HEAD: usize = 0;
const BODY: usize = 1;
const ARM: usize = 2;
const LEG: usize = 3;
const EYE: usize = 4;

pub fn build(p: &Player, tiles: &[u32; 5], atlas: &Atlas) -> MeshData {
    // The model is 2.0 units tall; sneaking squashes it to match the smaller hitbox.
    let squash = p.height() / HEIGHT;
    let root = Mat4::from_translation(p.pos) * Mat4::from_rotation_y(-p.yaw) * Mat4::from_scale(Vec3::new(1.0, squash, 1.0));
    let swing = p.walk_phase.sin() * 0.8 * p.walk_amp;
    let neck = Vec3::new(0.0, 1.5, 0.0);

    let limb = |x: f32, y: f32, w: f32, d: f32, len: f32, angle: f32, tile: usize| Part {
        pivot: Vec3::new(x, y, 0.0),
        min: Vec3::new(-w, -len, -d),
        max: Vec3::new(w, 0.0, d),
        angle,
        tile,
    };
    let parts = [
        limb(-0.1, 0.85, 0.1, 0.12, 0.85, swing, LEG),
        limb(0.1, 0.85, 0.1, 0.12, 0.85, -swing, LEG),
        Part { pivot: neck, min: Vec3::new(-0.22, -0.65, -0.12), max: Vec3::new(0.22, 0.0, 0.12), angle: 0.0, tile: BODY },
        limb(-0.32, 1.5, 0.1, 0.1, 0.65, -swing, ARM),
        limb(0.32, 1.5, 0.1, 0.1, 0.65, swing, ARM),
        Part { pivot: neck, min: Vec3::new(-0.25, 0.0, -0.25), max: Vec3::new(0.25, 0.5, 0.25), angle: p.pitch, tile: HEAD },
        // eyes sit on the front (-z) face of the head and turn with it
        Part { pivot: neck, min: Vec3::new(-0.15, 0.27, -0.262), max: Vec3::new(-0.05, 0.36, -0.245), angle: p.pitch, tile: EYE },
        Part { pivot: neck, min: Vec3::new(0.05, 0.27, -0.262), max: Vec3::new(0.15, 0.36, -0.245), angle: p.pitch, tile: EYE },
    ];

    let mut mesh = MeshData::default();
    for part in &parts {
        let m = root * Mat4::from_translation(part.pivot) * Mat4::from_rotation_x(part.angle);
        let (uv_min, uv_max) = atlas.tile_uv(tiles[part.tile]);
        for (_, corners, _) in &FACES {
            let pts: Vec<Vec3> = corners
                .iter()
                .map(|c| {
                    let local = part.min + Vec3::new(c[0], c[1], c[2]) * (part.max - part.min);
                    m.transform_point3(local)
                })
                .collect();
            let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]).normalize_or_zero();
            let s = shade(n);
            let base = mesh.vertices.len() as u32;
            for (pt, cuv) in pts.iter().zip(CORNER_UV) {
                mesh.vertices.push(Vertex {
                    pos: pt.to_array(),
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
    mesh
}
