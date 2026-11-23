//! The Settings screen (used from the main menu and the pause menu):
//! render distance, mouse sensitivity, field of view, and key bindings.

use crate::input::Input;
use crate::keys::{key_label, key_name, Bind, Keybinds};
use crate::settings::*;
use crate::ui::{col, col_a, Ui};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Main,
    Controls,
}

pub struct SettingsScreen {
    page: Page,
    rebinding: Option<Bind>,
}

impl SettingsScreen {
    pub fn new() -> Self {
        SettingsScreen { page: Page::Main, rebinding: None }
    }

    /// Draws the screen and applies clicks to `s`. Returns true when the player leaves it.
    pub fn ui(&mut self, ui: &mut Ui, input: &Input, s: &mut Settings) -> bool {
        match self.page {
            Page::Main => self.main_page(ui, input, s),
            Page::Controls => self.controls_page(ui, input, s),
        }
    }

    fn main_page(&mut self, ui: &mut Ui, input: &Input, s: &mut Settings) -> bool {
        let (pw, ph) = (290.0, 166.0);
        let px = (ui.w - pw) / 2.0;
        let py = ((ui.h - ph) / 2.0).max(44.0);
        ui.dl.text_centered(ui.w / 2.0, py - 34.0, 2.0, "Settings", col(0xffe9a8));
        ui.dl.panel(px - 12.0, py - 12.0, pw + 24.0, ph + 24.0, col_a(0x10131b, 0.88), col(0x0b0d12));

        // each row: label, [-] value [+]
        let row = |ui: &mut Ui, y: f32, label: &str, value: String| -> i32 {
            ui.dl.text(px, y + 6.0, 1.0, label, col(0xdcebff));
            let mut delta = 0;
            let bx = px + pw - 118.0;
            if ui.button(bx, y, 18.0, 20.0, "-") {
                delta -= 1;
            }
            ui.dl.text_centered(bx + 59.0, y + 6.0, 1.0, &value, col(0xffffff));
            if ui.button(bx + 100.0, y, 18.0, 20.0, "+") {
                delta += 1;
            }
            delta
        };

        let d = row(ui, py, "Render distance", format!("{} chunks", s.render_distance));
        s.render_distance += d * 2;
        let d = row(ui, py + 30.0, "Mouse speed", format!("{:.1}", s.mouse_sensitivity * 1000.0));
        s.mouse_sensitivity += d as f32 * 0.0002;
        let d = row(ui, py + 60.0, "Field of view", format!("{:.0}", s.fov));
        s.fov += d as f32 * 5.0;
        s.clamp();

        let hint = if s.render_distance > 20 {
            "Far views need a fast GPU and lots of memory."
        } else {
            "1 chunk = 16 blocks = 8 world units."
        };
        ui.dl.text(px, py + 88.0, 1.0, hint, col(0x8f9ab3));

        if ui.button(px, py + 114.0, pw, 22.0, "Controls...") {
            self.page = Page::Controls;
            self.rebinding = None;
        }
        let done = ui.button(px, py + 144.0, pw, 22.0, "Done");
        done || input.escape
    }

    fn controls_page(&mut self, ui: &mut Ui, input: &Input, s: &mut Settings) -> bool {
        let row_h = 18.0;
        let list_h = Bind::ALL.len() as f32 * row_h;
        let pw = 300.0;
        let px = (ui.w - pw) / 2.0;
        let py = ((ui.h - list_h - 70.0) / 2.0).max(48.0);
        ui.dl.text_centered(ui.w / 2.0, py - 40.0, 2.0, "Controls", col(0xffe9a8));
        ui.dl.panel(px - 12.0, py - 18.0, pw + 24.0, list_h + 74.0, col_a(0x10131b, 0.88), col(0x0b0d12));

        // Esc cancels a pending rebind first; otherwise it leaves the page.
        if self.rebinding.is_some() && input.escape {
            self.rebinding = None;
            return false;
        }
        if let (Some(bind), Some(key)) = (self.rebinding, input.last_key) {
            if key_name(key).is_some() {
                s.binds.set(bind, key);
                self.rebinding = None;
            }
        }

        let hint = if self.rebinding.is_some() { "Press the new key... (Esc cancels)" } else { "Click an action, then press a key." };
        ui.dl.text(px, py - 12.0, 1.0, hint, col(0x8f9ab3));

        for (i, &bind) in Bind::ALL.iter().enumerate() {
            let y = py + 2.0 + i as f32 * row_h;
            ui.dl.text(px, y + 5.0, 1.0, bind.label(), col(0xdcebff));
            let waiting = self.rebinding == Some(bind);
            let label = if waiting { "...".to_string() } else { key_label(s.binds.get(bind)) };
            let (bx, bw) = (px + pw - 120.0, 120.0);
            if waiting {
                ui.dl.panel(bx, y, bw, row_h - 2.0, col_a(0x5a4a1a, 0.95), col(0xffe9a8));
                ui.dl.text_centered(bx + bw / 2.0, y + 5.0, 1.0, &label, col(0xffe9a8));
            } else if ui.button(bx, y, bw, row_h - 2.0, &label) {
                self.rebinding = Some(bind);
            }
        }

        let by = py + list_h + 10.0;
        if ui.button(px, by, 144.0, 22.0, "Reset to defaults") {
            s.binds = Keybinds::default();
            self.rebinding = None;
        }
        let back = ui.button(px + pw - 144.0, by, 144.0, 22.0, "Back");
        if back || input.escape {
            self.page = Page::Main;
            self.rebinding = None;
        }
        false
    }
}
