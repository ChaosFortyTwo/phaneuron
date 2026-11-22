//! Asset discovery. Everything the game loads goes through here, so mods can
//! override any file just by shipping a file with the same relative path.
//!
//! Search order (later wins): `assets/`, then `mods/<name>/` in alphabetical order.

use std::{collections::BTreeMap, fs, path::PathBuf};

pub struct Assets {
    roots: Vec<PathBuf>,
}

impl Assets {
    pub fn discover() -> Self {
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.to_path_buf());
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }
        let base = candidates
            .into_iter()
            .find(|d| d.join("assets").is_dir())
            .unwrap_or_else(|| PathBuf::from("."));

        let mut roots = vec![base.join("assets")];
        if let Ok(rd) = fs::read_dir(base.join("mods")) {
            let mut mods: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
            mods.sort();
            roots.extend(mods);
        }
        Assets { roots }
    }

    #[cfg(test)]
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Assets { roots }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Find a file by its relative path (e.g. `textures/ui/crosshair.png`).
    /// The last root that contains it wins.
    pub fn resolve(&self, rel: &str) -> Option<PathBuf> {
        self.roots.iter().rev().map(|r| r.join(rel)).find(|p| p.is_file())
    }

    /// List `dir/*.ext` across all roots. Files with the same name in a later
    /// root replace earlier ones. Returns `(file stem, full path)` sorted by name.
    pub fn list(&self, dir: &str, ext: &str) -> Vec<(String, PathBuf)> {
        let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in &self.roots {
            let Ok(rd) = fs::read_dir(root.join(dir)) else { continue };
            for entry in rd.flatten() {
                let path = entry.path();
                let ok = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case(ext));
                if ok && path.is_file() {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        found.insert(stem.to_string(), path.clone());
                    }
                }
            }
        }
        found.into_iter().collect()
    }
}

