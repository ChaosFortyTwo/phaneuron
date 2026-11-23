mod app;
mod assets;
mod blocks;
mod config;
mod game;
mod input;
mod keys;
mod menu;
mod mesh;
mod model;
mod player;
mod player_model;
mod render;
mod save;
mod settings;
mod settings_ui;
mod ui;
mod world;

#[cfg(test)]
mod tests;

use anyhow::Result;
use winit::event_loop::{ControlFlow, EventLoop};

fn main() -> Result<()> {
    let assets = assets::Assets::discover();
    println!("asset roots: {:?}", assets.roots());
    let cfg = config::Config::load(&assets)?;
    let (registry, atlas) = blocks::load(&assets, cfg.render.tile_size)?;
    println!("loaded {} blocks, atlas {}x{}", registry.blocks.len() - 1, atlas.image.width(), atlas.image.height());
    let saves = save::saves_dir(&assets.base);

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = app::App::new(assets, cfg, std::sync::Arc::new(registry), atlas, saves);
    event_loop.run_app(&mut app)?;
    Ok(())
}
