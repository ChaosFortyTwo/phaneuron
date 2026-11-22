//! Minimal glTF reader: flattens every triangle mesh in the default scene into
//! one list of vertices. Embedded images/materials are ignored on purpose:
//! textures come from PNG files by naming convention (see docs/asset-mapping.svg).
//!
//! Convention: model origin = bottom-center of the block, 1 unit = 1 block,
//! so a full-size model spans x,z in -0.5..0.5 and y in 0..1.

use std::path::Path;

use anyhow::{Context, Result};
use glam::{Mat4, Vec3};

use crate::config::linear_to_srgb_u8;

#[derive(Debug, Clone, Default)]
pub struct ModelMesh {
    pub vertices: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// sRGB base color of the first material, used when no PNG is supplied.
    pub base_color: Option<[u8; 3]>,
}

pub fn load(path: &Path) -> Result<ModelMesh> {
    let (doc, buffers, _images) = gltf::import(path).with_context(|| format!("reading {}", path.display()))?;
    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .context("glTF has no scene")?;
    let mut out = ModelMesh::default();
    for node in scene.nodes() {
        visit(node, Mat4::IDENTITY, &buffers, &mut out);
    }
    if out.indices.is_empty() {
        anyhow::bail!("glTF contains no triangles");
    }
    Ok(out)
}

fn visit(node: gltf::Node, parent: Mat4, buffers: &[gltf::buffer::Data], out: &mut ModelMesh) {
    let m = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else { continue };
            let base = out.vertices.len() as u32;
            for p in positions {
                out.vertices.push(m.transform_point3(Vec3::from(p)).to_array());
            }
            let n = out.vertices.len() as u32 - base;
            match reader.read_tex_coords(0) {
                Some(tc) => out.uvs.extend(tc.into_f32()),
                None => out.uvs.extend((0..n).map(|_| [0.0, 0.0])),
            }
            match reader.read_indices() {
                Some(ix) => out.indices.extend(ix.into_u32().map(|i| i + base)),
                None => out.indices.extend(base..base + n),
            }
            if out.base_color.is_none() {
                let c = prim.material().pbr_metallic_roughness().base_color_factor();
                out.base_color = Some([linear_to_srgb_u8(c[0]), linear_to_srgb_u8(c[1]), linear_to_srgb_u8(c[2])]);
            }
        }
    }
    for child in node.children() {
        visit(child, m, buffers, out);
    }
}
