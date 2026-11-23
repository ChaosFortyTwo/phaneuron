//! `assets/config.toml` -> typed settings. Every field is optional.

use anyhow::{Context, Result};
use serde::{de, Deserialize, Deserializer};

use crate::assets::Assets;

/// A color written as `"#rrggbb"` in TOML.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub fn parse(s: &str) -> Option<Rgb> {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(h, 16).ok()?;
        Some(Rgb([(v >> 16) as u8, (v >> 8) as u8, v as u8]))
    }
    pub fn linear(self) -> [f32; 3] {
        self.0.map(|c| srgb_to_linear(c as f32 / 255.0))
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Rgb::parse(&s).ok_or_else(|| de::Error::custom(format!("bad color '{s}', expected \"#rrggbb\"")))
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb_u8(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct WindowCfg {
    pub title: String,
    pub width: u32,
    pub height: u32,
}
impl Default for WindowCfg {
    fn default() -> Self {
        Self { title: "Phaneuron".into(), width: 1280, height: 720 }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct RenderCfg {
    /// View distance in chunks (16 blocks each).
    pub render_distance: i32,
    /// Every block/texture tile is resampled to this square size (pixels).
    pub tile_size: u32,
    /// UI size multiplier. 0 = automatic (follows the window height).
    pub ui_scale: f32,
    /// Vertical field of view in degrees.
    pub fov: f32,
}
impl Default for RenderCfg {
    fn default() -> Self {
        Self { render_distance: 12, tile_size: 32, ui_scale: 0.0, fov: 75.0 }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct WorldCfg {
    pub base_height: f32,
    pub amplitude: f32,
    pub frequency: f32,
    pub octaves: u32,
    /// Block ids (file names in `blocks/` without `.toml`).
    pub surface: String,
    pub subsurface: String,
    pub subsurface_depth: i32,
    pub deep: String,
}
impl Default for WorldCfg {
    fn default() -> Self {
        Self {
            base_height: 60.0,
            amplitude: 32.0,
            frequency: 0.006,
            octaves: 4,
            surface: "grass".into(),
            subsurface: "dirt".into(),
            subsurface_depth: 6,
            deep: "stone".into(),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct SkyCfg {
    pub top: Rgb,
    pub horizon: Rgb,
    /// Fog is the horizon color unless set. Distances are fractions of the view distance.
    pub fog_color: Option<Rgb>,
    pub fog_start: f32,
    pub fog_end: f32,
}
impl Default for SkyCfg {
    fn default() -> Self {
        Self {
            top: Rgb([0x4f, 0x8f, 0xe0]),
            horizon: Rgb([0xbf, 0xdd, 0xf2]),
            fog_color: None,
            fog_start: 0.55,
            fog_end: 0.95,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct PlayerCfg {
    pub walk_speed: f32,
    pub sprint_multiplier: f32,
    /// How high a jump gets, in blocks.
    pub jump_height: f32,
    pub gravity: f32,
    pub mouse_sensitivity: f32,
    /// Eye height while standing (the hitbox is 2.0 units = 4 blocks tall).
    pub eye_height: f32,
    /// How far you can break/place blocks, in world units.
    pub reach: f32,
    /// Walking speed multiplier while sneaking.
    pub sneak_multiplier: f32,
    /// Free-flying speed in world units per second.
    pub fly_speed: f32,
}
impl Default for PlayerCfg {
    fn default() -> Self {
        Self {
            walk_speed: 4.5,
            sprint_multiplier: 1.7,
            jump_height: 1.25,
            gravity: 28.0,
            mouse_sensitivity: 0.0022,
            eye_height: 1.8,
            reach: 5.0,
            sneak_multiplier: 0.35,
            fly_speed: 10.0,
        }
    }
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Config {
    pub window: WindowCfg,
    pub render: RenderCfg,
    pub world: WorldCfg,
    pub sky: SkyCfg,
    pub player: PlayerCfg,
}

impl Config {
    pub fn load(assets: &Assets) -> Result<Config> {
        match assets.resolve("config.toml") {
            Some(p) => {
                let s = std::fs::read_to_string(&p)?;
                toml::from_str(&s).with_context(|| format!("while parsing {}", p.display()))
            }
            None => Ok(Config::default()),
        }
    }
}
