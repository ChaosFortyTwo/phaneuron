//! Worlds on disk.
//!
//!   saves/<folder>/world.toml   name, seed, timestamps, player position (human readable)
//!   saves/<folder>/edits.bin    every block the player changed (compact binary)
//!
//! Terrain itself is never stored: it is regenerated from the seed, so a world
//! with a few edits is a few kilobytes. Block ids in edits.bin are stored as
//! names, so adding or removing mods never scrambles an old world.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::blocks::Registry;
use crate::world::Edits;

const MAGIC: &[u8; 4] = b"PHED";
const VERSION: u8 = 1;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PlayerSave {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct WorldFile {
    name: String,
    seed: u32,
    created: u64,
    last_played: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_block: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    player: Option<PlayerSave>,
}

#[derive(Clone, Debug)]
pub struct WorldMeta {
    pub folder: PathBuf,
    pub name: String,
    pub seed: u32,
    pub last_played: u64,
}

pub struct Saved {
    pub player: Option<PlayerSave>,
    pub selected_block: Option<String>,
    pub edits: Edits,
}

pub struct SaveData {
    pub player: PlayerSave,
    pub selected_block: String,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `<game folder>/saves`, created if missing.
pub fn saves_dir(base: &Path) -> PathBuf {
    let dir = base.join("saves");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Seed field text -> seed. Empty = random, digits = that number, anything else = hashed text.
pub fn seed_from_text(text: &str) -> u32 {
    let t = text.trim();
    if t.is_empty() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(1);
        return fnv(&nanos.to_le_bytes());
    }
    if let Ok(n) = t.parse::<i64>() {
        return n as u32;
    }
    fnv(t.as_bytes())
}

fn fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn folder_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let s = s.trim().to_string();
    if s.is_empty() {
        "World".into()
    } else {
        s
    }
}

pub fn create_world(saves: &Path, name: &str, seed: u32) -> Result<WorldMeta> {
    let name = if name.trim().is_empty() { "New World" } else { name.trim() };
    let base = folder_name(name);
    let mut folder = saves.join(&base);
    let mut n = 2;
    while folder.exists() {
        folder = saves.join(format!("{base} ({n})"));
        n += 1;
    }
    fs::create_dir_all(&folder).with_context(|| format!("cannot create {}", folder.display()))?;
    let file = WorldFile { name: name.to_string(), seed, created: now(), last_played: now(), selected_block: None, player: None };
    write_world_file(&folder, &file)?;
    Ok(WorldMeta { folder, name: name.to_string(), seed, last_played: file.last_played })
}

fn write_world_file(folder: &Path, file: &WorldFile) -> Result<()> {
    let text = toml::to_string_pretty(file)?;
    atomic_write(&folder.join("world.toml"), text.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}

fn read_world_file(folder: &Path) -> Result<WorldFile> {
    let p = folder.join("world.toml");
    let text = fs::read_to_string(&p).with_context(|| format!("cannot read {}", p.display()))?;
    toml::from_str(&text).with_context(|| format!("while parsing {}", p.display()))
}

/// All worlds in `saves/`, most recently played first.
pub fn list_worlds(saves: &Path) -> Vec<WorldMeta> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(saves) else { return out };
    for entry in rd.flatten() {
        let folder = entry.path();
        if !folder.is_dir() {
            continue;
        }
        match read_world_file(&folder) {
            Ok(f) => out.push(WorldMeta { folder, name: f.name, seed: f.seed, last_played: f.last_played }),
            Err(e) => eprintln!("warning: skipping {}: {e:#}", folder.display()),
        }
    }
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played).then_with(|| a.name.cmp(&b.name)));
    out
}

pub fn load_world(meta: &WorldMeta, registry: &Registry) -> Result<Saved> {
    let f = read_world_file(&meta.folder)?;
    let edits_path = meta.folder.join("edits.bin");
    let edits = if edits_path.is_file() {
        let bytes = fs::read(&edits_path)?;
        decode_edits(&bytes, registry).with_context(|| format!("while reading {}", edits_path.display()))?
    } else {
        HashMap::new()
    };
    Ok(Saved { player: f.player, selected_block: f.selected_block, edits })
}

/// Write world.toml always; write edits.bin only when `write_edits` is set.
pub fn save_world(meta: &WorldMeta, data: &SaveData, edits: &Edits, registry: &Registry, write_edits: bool) -> Result<()> {
    let old = read_world_file(&meta.folder)?;
    let file = WorldFile {
        name: old.name,
        seed: old.seed,
        created: old.created,
        last_played: now(),
        selected_block: Some(data.selected_block.clone()),
        player: Some(data.player),
    };
    write_world_file(&meta.folder, &file)?;
    if write_edits {
        atomic_write(&meta.folder.join("edits.bin"), &encode_edits(edits, registry))?;
    }
    Ok(())
}

// ---------- edits.bin ----------
// "PHED" | version u8 | palette_len u16 | palette_len x (len u8, utf8) |
// chunk_count u32 | chunk_count x (cx i32, cz i32, n u32, n x (index u32, palette u16))   (little endian)

pub fn encode_edits(edits: &Edits, registry: &Registry) -> Vec<u8> {
    let mut palette: Vec<&str> = vec!["air"];
    let mut pal_index: HashMap<u16, u16> = HashMap::new();
    pal_index.insert(0, 0);
    for per_chunk in edits.values() {
        for &id in per_chunk.values() {
            if !pal_index.contains_key(&id) {
                pal_index.insert(id, palette.len() as u16);
                palette.push(registry.blocks[id as usize].id.as_str());
            }
        }
    }
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&(palette.len() as u16).to_le_bytes());
    for name in &palette {
        let b = name.as_bytes();
        out.push(b.len().min(255) as u8);
        out.extend_from_slice(&b[..b.len().min(255)]);
    }
    let chunks: Vec<_> = edits.iter().filter(|(_, e)| !e.is_empty()).collect();
    out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
    for ((cx, cz), per_chunk) in chunks {
        out.extend_from_slice(&cx.to_le_bytes());
        out.extend_from_slice(&cz.to_le_bytes());
        out.extend_from_slice(&(per_chunk.len() as u32).to_le_bytes());
        for (&idx, &id) in per_chunk {
            out.extend_from_slice(&idx.to_le_bytes());
            out.extend_from_slice(&pal_index[&id].to_le_bytes());
        }
    }
    out
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.b.len() {
            bail!("file is truncated");
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

pub fn decode_edits(bytes: &[u8], registry: &Registry) -> Result<Edits> {
    let mut r = Reader { b: bytes, pos: 0 };
    if r.take(4)? != MAGIC {
        bail!("not a Phaneuron edits file");
    }
    let version = r.u8()?;
    if version != VERSION {
        bail!("unsupported edits version {version}");
    }
    let n_pal = r.u16()? as usize;
    let mut palette: Vec<u16> = Vec::with_capacity(n_pal);
    for _ in 0..n_pal {
        let len = r.u8()? as usize;
        let name = String::from_utf8_lossy(r.take(len)?).to_string();
        palette.push(match registry.id_of(&name) {
            Some(id) => id,
            None => {
                if name != "air" {
                    eprintln!("warning: block '{name}' from the save is not installed; it becomes air");
                }
                0
            }
        });
    }
    let mut edits: Edits = HashMap::new();
    let n_chunks = r.u32()?;
    for _ in 0..n_chunks {
        let (cx, cz, n) = (r.i32()?, r.i32()?, r.u32()?);
        let per_chunk = edits.entry((cx, cz)).or_default();
        for _ in 0..n {
            let (idx, p) = (r.u32()?, r.u16()? as usize);
            per_chunk.insert(idx, *palette.get(p).context("bad palette index")?);
        }
    }
    Ok(edits)
}
