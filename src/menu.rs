//! Start menu: main screen, new-world form, and the saved-world list.

use std::path::Path;

use crate::input::Input;
use crate::preview::{PreviewInfo, PreviewState};
use crate::save::{self, WorldMeta};
use crate::settings::Settings;
use crate::settings_ui::SettingsScreen;
use crate::ui::{col, col_a, Tex, Ui, GLYPH, PREVIEW_H, PREVIEW_W, WHITE};

pub enum Action {
    None,
    Create { name: String, seed: u32 },
    /// Start playing this world.
    Load(WorldMeta),
    /// A world was clicked in the list: show its preview.
    Select(WorldMeta),
    OpenFiles,
}

pub struct NewWorld {
    pub name: String,
    pub seed: String,
    /// 0 = name field, 1 = seed field
    pub focus: usize,
}

pub enum Menu {
    Main { toast: Option<(String, f32)> },
    New(NewWorld),
    Load { worlds: Vec<WorldMeta>, scroll: f32, selected: Option<usize> },
    Settings(SettingsScreen),
}

const NAME_MAX: usize = 24;
const SEED_MAX: usize = 16;
const ENTRY_H: f32 = 26.0;

impl Menu {
    pub fn main() -> Menu {
        Menu::Main { toast: None }
    }

    pub fn main_with_message(msg: String) -> Menu {
        Menu::Main { toast: Some((msg, 6.0)) }
    }

    pub fn ui(&mut self, ui: &mut Ui, input: &Input, saves: &Path, dt: f32, settings: &mut Settings, preview: &PreviewState) -> Action {
        ui.banner_bg();
        let mut action = Action::None;
        let mut next: Option<Menu> = None;

        match self {
            Menu::Main { toast } => {
                let title_scale = if ui.w < 330.0 { 2.0 } else { 4.0 };
                let (bw, bh, gap) = (168.0, 22.0, 8.0);
                let total = 4.0 * bh + 3.0 * gap;
                let top = ((ui.h - total) / 2.0 + 26.0).max(title_scale * GLYPH + 40.0);
                let cx = ui.w / 2.0;
                let title_y = (top - title_scale * GLYPH - 34.0).max(6.0);
                ui.dl.text_centered(cx, title_y, title_scale, "PHANEURON", col(0xffe9a8));
                ui.dl.text_centered(cx, title_y + title_scale * GLYPH + 8.0, 1.0, "a procedural voxel world", col(0xdcebff));

                let x = cx - bw / 2.0;
                if ui.button(x, top, bw, bh, "Settings") {
                    next = Some(Menu::Settings(SettingsScreen::new()));
                }
                if ui.button(x, top + (bh + gap), bw, bh, "New World") {
                    next = Some(Menu::New(NewWorld { name: "New World".into(), seed: String::new(), focus: 0 }));
                }
                if ui.button(x, top + 2.0 * (bh + gap), bw, bh, "Load World") {
                    next = Some(Menu::Load { worlds: save::list_worlds(saves), scroll: 0.0, selected: None });
                }
                if ui.button(x, top + 3.0 * (bh + gap), bw, bh, "Game Files") {
                    action = Action::OpenFiles;
                }

                if let Some((msg, t)) = toast {
                    *t -= dt;
                    if *t > 0.0 {
                        ui.dl.text_centered(cx, top + total + 14.0, 1.0, msg, col(0xffb3a8));
                    } else {
                        *toast = None;
                    }
                }
            }

            Menu::Settings(screen) => {
                if screen.ui(ui, input, settings) {
                    next = Some(Menu::main());
                }
            }

            Menu::New(form) => {
                let (pw, ph) = (236.0, 148.0);
                let px = (ui.w - pw) / 2.0;
                let py = ((ui.h - ph) / 2.0).max(40.0);
                ui.dl.text_centered(ui.w / 2.0, py - 30.0, 2.0, "New World", col(0xffe9a8));
                ui.dl.panel(px - 12.0, py - 12.0, pw + 24.0, ph + 24.0, col_a(0x10131b, 0.85), col(0x0b0d12));

                // typing goes into the focused field
                let (field, max) = if form.focus == 0 { (&mut form.name, NAME_MAX) } else { (&mut form.seed, SEED_MAX) };
                for c in input.typed.chars() {
                    if (c.is_ascii_graphic() || c == ' ') && field.chars().count() < max {
                        field.push(c);
                    }
                }
                if input.backspace {
                    field.pop();
                }
                if input.tab {
                    form.focus = 1 - form.focus;
                }

                ui.dl.text(px, py, 1.0, "World name", col(0xdcebff));
                if ui.text_field(px, py + 12.0, pw, 20.0, &form.name, form.focus == 0) {
                    form.focus = 0;
                }
                ui.dl.text(px, py + 44.0, 1.0, "Seed (empty = random)", col(0xdcebff));
                if ui.text_field(px, py + 56.0, pw, 20.0, &form.seed, form.focus == 1) {
                    form.focus = 1;
                }

                let create = ui.button(px, py + 96.0, pw, 22.0, "Create World");
                if create || input.enter {
                    action = Action::Create { name: form.name.trim().to_string(), seed: save::seed_from_text(&form.seed) };
                }
                if ui.button(px, py + 126.0, pw, 22.0, "Back") || input.escape {
                    next = Some(Menu::main());
                }
            }

            Menu::Load { worlds, scroll, selected } => {
                ui.dl.text_shadow(16.0, 14.0, 2.0, "Load World", col(0xffe9a8));

                let list_x = 16.0;
                let list_y = 44.0;
                let list_w = (ui.w * 0.36).clamp(180.0, 280.0).min(ui.w - 32.0);
                let list_h = (ui.h - list_y - 44.0).max(ENTRY_H + 8.0);
                ui.dl.panel(list_x, list_y, list_w, list_h, col_a(0x10131b, 0.88), col(0x0b0d12));

                let visible = ((list_h - 8.0) / ENTRY_H).floor().max(1.0) as usize;
                let max_scroll = worlds.len().saturating_sub(visible) as f32;
                *scroll = (*scroll - input.wheel).clamp(0.0, max_scroll);
                let first = scroll.round() as usize;

                if worlds.is_empty() {
                    ui.dl.text(list_x + 8.0, list_y + 10.0, 1.0, "No saved worlds yet.", col(0xaab4c8));
                }
                let max_chars = ((list_w - 16.0) / GLYPH) as usize;
                for (row, meta) in worlds.iter().enumerate().skip(first).take(visible) {
                    let y = list_y + 4.0 + (row - first) as f32 * ENTRY_H;
                    let (x, w) = (list_x + 4.0, list_w - 8.0);
                    let hover = ui.hover(x, y, w, ENTRY_H - 2.0);
                    let is_selected = *selected == Some(row);
                    if is_selected {
                        ui.dl.panel(x, y, w, ENTRY_H - 2.0, col_a(0x35507f, 0.95), col(0xffe9a8));
                    } else if hover {
                        ui.dl.panel(x, y, w, ENTRY_H - 2.0, col_a(0x4a5f8f, 0.7), col(0x6d7fa8));
                    }
                    let name: String = meta.name.chars().take(max_chars).collect();
                    let lit = is_selected || hover;
                    ui.dl.text(x + 4.0, y + 4.0, 1.0, &name, if lit { col(0xffe9a8) } else { col(0xffffff) });
                    ui.dl.text(x + 4.0, y + 15.0, 1.0, &format!("seed {}", meta.seed), col(0x8f9ab3));
                    if hover && ui.click {
                        *selected = Some(row);
                        action = Action::Select(meta.clone());
                    }
                }
                if worlds.len() > visible {
                    let track = list_h - 8.0;
                    let thumb = (track * visible as f32 / worlds.len() as f32).max(10.0);
                    let ty = list_y + 4.0 + (track - thumb) * (*scroll / max_scroll.max(1.0));
                    ui.dl.rect(list_x + list_w - 5.0, ty, 3.0, thumb, col(0x8f9ab3));
                }

                // ---- details panel on the right ----
                let right_x = list_x + list_w + 12.0;
                let chosen = selected.and_then(|i| worlds.get(i));
                if right_x + 80.0 < ui.w {
                    let (rw, rh) = (ui.w - right_x - 16.0, list_h);
                    ui.dl.panel(right_x, list_y, rw, rh, col_a(0x10131b, 0.88), col(0x0b0d12));
                    match chosen {
                        None => {
                            ui.dl.text_centered(right_x + rw / 2.0, list_y + rh / 2.0 - 4.0, 1.0, "Select a world to preview it", col(0x8f9ab3));
                        }
                        Some(meta) => {
                            if let Some(play) = details_panel(ui, right_x, list_y, rw, rh, meta, preview) {
                                action = Action::Load(play);
                            }
                            if input.enter {
                                action = Action::Load(meta.clone());
                            }
                        }
                    }
                }

                ui.dl.text_shadow(128.0, ui.h - 27.0, 1.0, "Click a world to preview it", col(0xdcebff));
                if ui.button(16.0, ui.h - 34.0, 100.0, 22.0, "Back") || input.escape {
                    next = Some(Menu::main());
                }
            }
        }

        if let Some(n) = next {
            *self = n;
        }
        action
    }
}

/// Name, details, the orbiting 3D preview, and the Play button for the selected world.
/// Returns the world when Play was clicked.
fn details_panel(ui: &mut Ui, x: f32, y: f32, w: f32, h: f32, meta: &WorldMeta, preview: &PreviewState) -> Option<WorldMeta> {
    let chars = ((w - 16.0) / GLYPH) as usize;
    let name: String = meta.name.chars().take(chars).collect();
    ui.dl.text(x + 8.0, y + 8.0, 1.0, &name, col(0xffe9a8));
    ui.dl.text(x + 8.0, y + 19.0, 1.0, &format!("seed {}", meta.seed), col(0x8f9ab3));

    let ready: Option<&PreviewInfo> = match preview {
        PreviewState::Ready(info) if info.folder == meta.folder => Some(info),
        _ => None,
    };
    if let Some(info) = ready {
        let line = format!("last at {:.0} {:.0} {:.0}  -  {} edits", info.pos.x, info.pos.y, info.pos.z, info.edited_blocks);
        ui.dl.text(x + 8.0, y + 30.0, 1.0, &line.chars().take(chars).collect::<String>(), col(0x8f9ab3));
    }

    // the 3D view, 4:3, as large as fits between the text and the Play button
    let top = y + 46.0;
    let (avail_w, avail_h) = (w - 16.0, h - 46.0 - 34.0);
    let ratio = PREVIEW_W as f32 / PREVIEW_H as f32;
    let fw = avail_w.min(avail_h * ratio).max(0.0);
    let fh = fw / ratio;
    let (ix, iy) = (x + (w - fw) / 2.0, top + (avail_h - fh).max(0.0) / 2.0);
    if fw >= 48.0 {
        ui.dl.rect(ix - 1.0, iy - 1.0, fw + 2.0, fh + 2.0, col(0x0b0d12));
        match (ready, preview) {
            (Some(_), _) => ui.dl.quad(Tex::Preview, ix, iy, fw, fh, [0.0, 0.0], [1.0, 1.0], WHITE),
            (None, PreviewState::Failed(folder, msg)) if *folder == meta.folder => {
                ui.dl.rect(ix, iy, fw, fh, col(0x2a1a1a));
                let text: String = format!("Cannot preview: {msg}").chars().take(((fw - 8.0) / GLYPH) as usize * 3).collect();
                for (i, chunk) in text.chars().collect::<Vec<_>>().chunks(((fw - 8.0) / GLYPH).max(4.0) as usize).take(3).enumerate() {
                    ui.dl.text(ix + 4.0, iy + 6.0 + i as f32 * 10.0, 1.0, &chunk.iter().collect::<String>(), col(0xffb3a8));
                }
            }
            _ => {
                ui.dl.rect(ix, iy, fw, fh, col(0x161922));
                ui.dl.text_centered(ix + fw / 2.0, iy + fh / 2.0 - 4.0, 1.0, "Loading...", col(0x8f9ab3));
            }
        }
    }

    let bw = (w - 16.0).min(180.0);
    let clicked = ui.button(x + (w - bw) / 2.0, y + h - 30.0, bw, 22.0, "Play");
    clicked.then(|| meta.clone())
}
