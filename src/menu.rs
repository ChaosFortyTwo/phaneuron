//! Start menu: main screen, new-world form, and the saved-world list.

use std::path::Path;

use crate::input::Input;
use crate::save::{self, WorldMeta};
use crate::settings::Settings;
use crate::settings_ui::SettingsScreen;
use crate::ui::{col, col_a, Ui, GLYPH};

pub enum Action {
    None,
    Create { name: String, seed: u32 },
    Load(WorldMeta),
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
    Load { worlds: Vec<WorldMeta>, scroll: f32 },
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

    pub fn ui(&mut self, ui: &mut Ui, input: &Input, saves: &Path, dt: f32, settings: &mut Settings) -> Action {
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
                    next = Some(Menu::Load { worlds: save::list_worlds(saves), scroll: 0.0 });
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

            Menu::Load { worlds, scroll } => {
                ui.dl.text_shadow(16.0, 14.0, 2.0, "Load World", col(0xffe9a8));

                let list_x = 16.0;
                let list_y = 44.0;
                let list_w = (ui.w * 0.42).clamp(180.0, 300.0).min(ui.w - 32.0);
                let list_h = (ui.h - list_y - 44.0).max(ENTRY_H + 8.0);
                ui.dl.panel(list_x, list_y, list_w, list_h, col_a(0x10131b, 0.88), col(0x0b0d12));

                // empty panel on the right, reserved for later
                let right_x = list_x + list_w + 12.0;
                if right_x + 40.0 < ui.w {
                    ui.dl.panel(right_x, list_y, ui.w - right_x - 16.0, list_h, col_a(0x10131b, 0.55), col_a(0x0b0d12, 0.8));
                }

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
                    if hover {
                        ui.dl.panel(x, y, w, ENTRY_H - 2.0, col_a(0x4a5f8f, 0.9), col(0xffe9a8));
                    }
                    let name: String = meta.name.chars().take(max_chars).collect();
                    ui.dl.text(x + 4.0, y + 4.0, 1.0, &name, if hover { col(0xffe9a8) } else { col(0xffffff) });
                    ui.dl.text(x + 4.0, y + 15.0, 1.0, &format!("seed {}", meta.seed), col(0x8f9ab3));
                    if hover && ui.click {
                        action = Action::Load(meta.clone());
                    }
                }
                if worlds.len() > visible {
                    let track = list_h - 8.0;
                    let thumb = (track * visible as f32 / worlds.len() as f32).max(10.0);
                    let ty = list_y + 4.0 + (track - thumb) * (*scroll / max_scroll.max(1.0));
                    ui.dl.rect(list_x + list_w - 5.0, ty, 3.0, thumb, col(0x8f9ab3));
                }

                ui.dl.text_shadow(128.0, ui.h - 27.0, 1.0, "Click a world to play it", col(0xdcebff));
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
