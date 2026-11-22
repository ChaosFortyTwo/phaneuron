//! Block definitions (`blocks/*.toml`) and the texture atlas.
//!
//! Resolution rule for every face of every block, first hit wins:
//!   1. textures/blocks/<id>_<top|side|bottom>.png
//!   2. the `texture = "..."` path from the block's TOML
//!   3. textures/blocks/<id>.png
//!   4. a flat color: color_<face>, else color, else the glTF material color, else gray

use std::collections::HashMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use image::{imageops, Rgba, RgbaImage};
use serde::Deserialize;

use crate::assets::Assets;
use crate::config::Rgb;
use crate::model::{self, ModelMesh};

const GRAY: [u8; 3] = [180, 180, 180];
const ATLAS_COLUMNS: u32 = 16;

#[derive(Deserialize, Debug)]
#[serde(default)]
pub struct BlockDef {
    pub name: Option<String>,
    pub color: Option<Rgb>,
    pub color_top: Option<Rgb>,
    pub color_side: Option<Rgb>,
    pub color_bottom: Option<Rgb>,
    /// Blocks movement.
    pub solid: bool,
    /// Hides faces of neighbouring blocks. Models are never treated as opaque.
    pub opaque: bool,
    pub texture: Option<String>,
    pub model: Option<String>,
}
impl Default for BlockDef {
    fn default() -> Self {
        Self {
            name: None,
            color: None,
            color_top: None,
            color_side: None,
            color_bottom: None,
            solid: true,
            opaque: true,
            texture: None,
            model: None,
        }
    }
}

pub struct BlockInfo {
    /// File name in blocks/ without .toml. Stable key used in save files.
    pub id: String,
    /// Display name.
    pub name: String,
    pub solid: bool,
    /// True when this block hides neighbouring faces (opaque full cube).
    pub occluding: bool,
    /// Atlas tile index for [top, side, bottom].
    pub tiles: [u32; 3],
    pub model: Option<ModelMesh>,
}

pub struct Registry {
    /// Index 0 is always air.
    pub blocks: Vec<BlockInfo>,
    by_name: HashMap<String, u16>,
}

impl Registry {
    pub fn id_of(&self, name: &str) -> Option<u16> {
        self.by_name.get(name).copied()
    }
    pub fn first_solid(&self) -> u16 {
        self.blocks.iter().position(|b| b.solid).unwrap_or(0) as u16
    }
}

pub struct Atlas {
    pub image: RgbaImage,
    pub tile_size: u32,
    pub cols: u32,
}

impl Atlas {
    /// (uv_min, uv_max) of a tile, inset a hair so nearest sampling never bleeds.
    pub fn tile_uv(&self, idx: u32) -> ([f32; 2], [f32; 2]) {
        let (col, row) = (idx % self.cols, idx / self.cols);
        let (w, h) = (self.image.width() as f32, self.image.height() as f32);
        let ts = self.tile_size as f32;
        let e = 0.02;
        (
            [(col as f32 * ts + e) / w, (row as f32 * ts + e) / h],
            [((col + 1) as f32 * ts - e) / w, ((row + 1) as f32 * ts - e) / h],
        )
    }
}

struct TileSet {
    size: u32,
    tiles: Vec<RgbaImage>,
    keys: HashMap<String, u32>,
}

impl TileSet {
    fn push(&mut self, key: String, img: RgbaImage) -> u32 {
        let i = self.tiles.len() as u32;
        self.tiles.push(img);
        self.keys.insert(key, i);
        i
    }
    fn solid(&mut self, c: [u8; 3]) -> u32 {
        let key = format!("solid:{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        if let Some(&i) = self.keys.get(&key) {
            return i;
        }
        let img = RgbaImage::from_pixel(self.size, self.size, Rgba([c[0], c[1], c[2], 255]));
        self.push(key, img)
    }
    fn png(&mut self, path: &Path) -> Option<u32> {
        let key = format!("png:{}", path.display());
        if let Some(&i) = self.keys.get(&key) {
            return Some(i);
        }
        let img = match image::open(path) {
            Ok(i) => i.to_rgba8(),
            Err(e) => {
                eprintln!("warning: cannot read {}: {e}", path.display());
                return None;
            }
        };
        let img = if img.dimensions() == (self.size, self.size) {
            img
        } else {
            imageops::resize(&img, self.size, self.size, imageops::FilterType::Nearest)
        };
        Some(self.push(key, img))
    }
}

pub fn load(assets: &Assets, tile_size: u32) -> Result<(Registry, Atlas)> {
    let mut ts = TileSet { size: tile_size, tiles: Vec::new(), keys: HashMap::new() };
    let missing = ts.solid([255, 0, 255]); // tile 0: "missing" magenta, also used for air

    let mut blocks = vec![BlockInfo {
        id: "air".into(),
        name: "Air".into(),
        solid: false,
        occluding: false,
        tiles: [missing; 3],
        model: None,
    }];
    let mut by_name = HashMap::new();

    for (id, path) in assets.list("blocks", "toml") {
        let text = std::fs::read_to_string(&path)?;
        let def: BlockDef = toml::from_str(&text).with_context(|| format!("while parsing {}", path.display()))?;

        let model = match &def.model {
            Some(rel) => match assets.resolve(rel) {
                Some(p) => match model::load(&p) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        eprintln!("warning: block '{id}': {e:#}");
                        None
                    }
                },
                None => {
                    eprintln!("warning: block '{id}': model '{rel}' not found");
                    None
                }
            },
            None => None,
        };

        let base = def
            .color
            .map(|c| c.0)
            .or_else(|| model.as_ref().and_then(|m| m.base_color))
            .unwrap_or(GRAY);
        let face_colors = [def.color_top, def.color_side, def.color_bottom];

        let mut tiles = [missing; 3];
        for (i, face) in ["top", "side", "bottom"].iter().enumerate() {
            let mut candidates = vec![format!("textures/blocks/{id}_{face}.png")];
            if let Some(t) = &def.texture {
                candidates.push(t.clone());
            }
            candidates.push(format!("textures/blocks/{id}.png"));

            let mut found = None;
            for rel in &candidates {
                if let Some(p) = assets.resolve(rel) {
                    if let Some(t) = ts.png(&p) {
                        found = Some(t);
                        break;
                    }
                }
            }
            tiles[i] = match found {
                Some(t) => t,
                None => ts.solid(face_colors[i].map(|c| c.0).unwrap_or(base)),
            };
        }

        by_name.insert(id.clone(), blocks.len() as u16);
        blocks.push(BlockInfo {
            id: id.clone(),
            name: def.name.unwrap_or(id),
            solid: def.solid,
            occluding: def.opaque && model.is_none(),
            tiles,
            model,
        });
    }

    if blocks.len() == 1 {
        bail!("no block definitions found in assets/blocks/*.toml");
    }

    // Pack tiles into one atlas image.
    let n = ts.tiles.len() as u32;
    let cols = ATLAS_COLUMNS.min(n);
    let rows = n.div_ceil(cols);
    let mut image = RgbaImage::new(cols * tile_size, rows * tile_size);
    for (i, tile) in ts.tiles.iter().enumerate() {
        let (cx, cy) = (i as u32 % cols, i as u32 / cols);
        imageops::replace(&mut image, tile, (cx * tile_size) as i64, (cy * tile_size) as i64);
    }

    Ok((Registry { blocks, by_name }, Atlas { image, tile_size, cols }))
}
