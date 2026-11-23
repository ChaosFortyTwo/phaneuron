//! Player settings, saved next to the game as `settings.toml`.
//! They override the defaults in `assets/config.toml`.

use std::{collections::BTreeMap, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::keys::Keybinds;

pub const MIN_RENDER_DISTANCE: i32 = 4;
pub const MAX_RENDER_DISTANCE: i32 = 32;
pub const MIN_SENSITIVITY: f32 = 0.0004;
pub const MAX_SENSITIVITY: f32 = 0.0100;
pub const MIN_FOV: f32 = 50.0;
pub const MAX_FOV: f32 = 110.0;

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub render_distance: i32,
    pub mouse_sensitivity: f32,
    pub fov: f32,
    pub binds: Keybinds,
}

#[derive(Serialize, Deserialize, Default)]
struct SettingsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    render_distance: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mouse_sensitivity: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fov: Option<f64>,
    #[serde(default)]
    keys: BTreeMap<String, String>,
}

/// f32 -> f64 without the noise (0.0022 stays 0.0022 instead of 0.002199999988...).
fn clean(v: f32) -> f64 {
    format!("{v}").parse().unwrap_or(v as f64)
}

impl Settings {
    fn defaults(cfg: &Config) -> Settings {
        let mut s = Settings {
            render_distance: cfg.render.render_distance,
            mouse_sensitivity: cfg.player.mouse_sensitivity,
            fov: cfg.render.fov,
            binds: Keybinds::default(),
        };
        s.clamp();
        s
    }

    pub fn clamp(&mut self) {
        self.render_distance = self.render_distance.clamp(MIN_RENDER_DISTANCE, MAX_RENDER_DISTANCE);
        self.mouse_sensitivity = self.mouse_sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
        self.fov = self.fov.clamp(MIN_FOV, MAX_FOV);
    }

    /// Config defaults, then settings.toml on top. A broken file is reported and ignored.
    pub fn load(base: &Path, cfg: &Config) -> Settings {
        let mut s = Settings::defaults(cfg);
        let path = base.join("settings.toml");
        let Ok(text) = std::fs::read_to_string(&path) else { return s };
        match toml::from_str::<SettingsFile>(&text) {
            Ok(f) => {
                if let Some(v) = f.render_distance {
                    s.render_distance = v;
                }
                if let Some(v) = f.mouse_sensitivity {
                    s.mouse_sensitivity = v as f32;
                }
                if let Some(v) = f.fov {
                    s.fov = v as f32;
                }
                s.binds = Keybinds::from_map(&f.keys);
                s.clamp();
            }
            Err(e) => eprintln!("warning: ignoring {}: {e}", path.display()),
        }
        s
    }

    pub fn save(&self, base: &Path) -> Result<()> {
        let file = SettingsFile {
            render_distance: Some(self.render_distance),
            mouse_sensitivity: Some(clean(self.mouse_sensitivity)),
            fov: Some(clean(self.fov)),
            keys: self.binds.to_map(),
        };
        let path = base.join("settings.toml");
        std::fs::write(&path, toml::to_string_pretty(&file)?).with_context(|| format!("cannot write {}", path.display()))
    }

    pub fn apply(&self, cfg: &mut Config) {
        cfg.render.render_distance = self.render_distance;
        cfg.player.mouse_sensitivity = self.mouse_sensitivity;
        cfg.render.fov = self.fov;
    }
}
