//! A very small immediate-mode UI.
//!
//! Everything is drawn as textured, tinted quads in "logical" pixels (the
//! renderer multiplies by the UI scale). Text comes from `textures/ui/font.png`:
//! a 16x6 grid holding ASCII 32..127, so modders can reskin it.

use bytemuck::{Pod, Zeroable};

use crate::config::srgb_to_linear;

/// Logical size of one glyph at text scale 1.
pub const GLYPH: f32 = 8.0;
const FONT_COLS: f32 = 16.0;
const FONT_ROWS: f32 = 6.0;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct UiVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tex {
    White,
    Font,
    Banner,
    Crosshair,
    Atlas,
}

pub struct Cmd {
    pub tex: Tex,
    pub start: u32,
    pub count: u32,
}

/// sRGB hex (0xRRGGBB) -> linear RGBA, ready for an sRGB surface.
pub fn col(hex: u32) -> [f32; 4] {
    col_a(hex, 1.0)
}
pub fn col_a(hex: u32, a: f32) -> [f32; 4] {
    let c = |shift: u32| srgb_to_linear(((hex >> shift) & 0xff) as f32 / 255.0);
    [c(16), c(8), c(0), a]
}

pub const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

#[derive(Default)]
pub struct DrawList {
    pub verts: Vec<UiVertex>,
    pub cmds: Vec<Cmd>,
}

impl DrawList {
    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, tex: Tex, x: f32, y: f32, w: f32, h: f32, uv0: [f32; 2], uv1: [f32; 2], color: [f32; 4]) {
        let v = |px: f32, py: f32, u: f32, vv: f32| UiVertex { pos: [px, py], uv: [u, vv], color };
        let (x1, y1) = (x + w, y + h);
        let start = self.verts.len() as u32;
        self.verts.extend_from_slice(&[
            v(x, y, uv0[0], uv0[1]),
            v(x1, y, uv1[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x, y, uv0[0], uv0[1]),
            v(x1, y1, uv1[0], uv1[1]),
            v(x, y1, uv0[0], uv1[1]),
        ]);
        match self.cmds.last_mut() {
            Some(c) if c.tex == tex => c.count += 6,
            _ => self.cmds.push(Cmd { tex, start, count: 6 }),
        }
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.quad(Tex::White, x, y, w, h, [0.0, 0.0], [1.0, 1.0], color);
    }

    /// Filled box with a 1px border.
    pub fn panel(&mut self, x: f32, y: f32, w: f32, h: f32, fill: [f32; 4], border: [f32; 4]) {
        self.rect(x, y, w, h, border);
        self.rect(x + 1.0, y + 1.0, w - 2.0, h - 2.0, fill);
    }

    /// Hollow 1px rectangle outline.
    pub fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        self.rect(x, y, w, 1.0, color);
        self.rect(x, y + h - 1.0, w, 1.0, color);
        self.rect(x, y + 1.0, 1.0, h - 2.0, color);
        self.rect(x + w - 1.0, y + 1.0, 1.0, h - 2.0, color);
    }

    /// Draw text; returns its width.
    pub fn text(&mut self, x: f32, y: f32, scale: f32, s: &str, color: [f32; 4]) -> f32 {
        let size = GLYPH * scale;
        let mut cx = x;
        for ch in s.chars() {
            let code = ch as u32;
            let idx = if (33..127).contains(&code) { code - 32 } else if code == 32 { 0 } else { '?' as u32 - 32 };
            if ch != ' ' {
                let (c, r) = ((idx % 16) as f32, (idx / 16) as f32);
                self.quad(
                    Tex::Font,
                    cx,
                    y,
                    size,
                    size,
                    [c / FONT_COLS, r / FONT_ROWS],
                    [(c + 1.0) / FONT_COLS, (r + 1.0) / FONT_ROWS],
                    color,
                );
            }
            cx += size;
        }
        cx - x
    }

    pub fn text_shadow(&mut self, x: f32, y: f32, scale: f32, s: &str, color: [f32; 4]) {
        self.text(x + scale.max(1.0), y + scale.max(1.0), scale, s, col_a(0x000000, 0.65));
        self.text(x, y, scale, s, color);
    }

    pub fn text_centered(&mut self, cx: f32, y: f32, scale: f32, s: &str, color: [f32; 4]) {
        let w = text_width(s, scale);
        self.text_shadow(cx - w / 2.0, y, scale, s, color);
    }
}

pub fn text_width(s: &str, scale: f32) -> f32 {
    s.chars().count() as f32 * GLYPH * scale
}

/// Per-frame UI context: draw list plus mouse state, in logical pixels.
pub struct Ui {
    pub dl: DrawList,
    pub w: f32,
    pub h: f32,
    pub mouse: (f32, f32),
    pub click: bool,
    pub time: f32,
    pub banner: Option<(u32, u32)>,
    pub crosshair: Option<(u32, u32)>,
}

impl Ui {
    pub fn hover(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.mouse.0 >= x && self.mouse.0 < x + w && self.mouse.1 >= y && self.mouse.1 < y + h
    }

    /// Full-screen banner image, scaled to cover the window (cropping the overflow).
    pub fn banner_bg(&mut self) {
        match self.banner {
            Some((bw, bh)) => {
                let (bw, bh) = (bw as f32, bh as f32);
                let s = (self.w / bw).max(self.h / bh);
                let (vis_w, vis_h) = (self.w / s, self.h / s);
                let u0 = (bw - vis_w) / 2.0 / bw;
                let v0 = (bh - vis_h) / 2.0 / bh;
                self.dl.quad(Tex::Banner, 0.0, 0.0, self.w, self.h, [u0, v0], [1.0 - u0, 1.0 - v0], WHITE);
            }
            None => {
                let (w, h) = (self.w, self.h);
                self.dl.rect(0.0, 0.0, w, h / 2.0, col(0x1b2a49));
                self.dl.rect(0.0, h / 2.0, w, h / 2.0, col(0x2f4a78));
            }
        }
    }

    pub fn button(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str) -> bool {
        let hover = self.hover(x, y, w, h);
        let fill = if hover { col_a(0x4a5f8f, 0.96) } else { col_a(0x2b3140, 0.92) };
        let border = if hover { col(0xffe9a8) } else { col(0x0b0d12) };
        self.dl.panel(x, y, w, h, fill, border);
        let tw = text_width(label, 1.0);
        let color = if hover { col(0xffe9a8) } else { col(0xffffff) };
        self.dl.text_shadow(x + (w - tw) / 2.0, y + (h - GLYPH) / 2.0, 1.0, label, color);
        hover && self.click
    }

    /// Single-line text box. Returns true when clicked (so the caller can move focus).
    pub fn text_field(&mut self, x: f32, y: f32, w: f32, h: f32, value: &str, focused: bool) -> bool {
        let hover = self.hover(x, y, w, h);
        let border = if focused { col(0xffe9a8) } else { col(0x0b0d12) };
        self.dl.panel(x, y, w, h, col_a(0x161922, 0.95), border);

        let max_chars = (((w - 8.0) / GLYPH) as usize).saturating_sub(1).max(1);
        let n = value.chars().count();
        let shown: String = value.chars().skip(n.saturating_sub(max_chars)).collect();
        let ty = y + (h - GLYPH) / 2.0;
        let used = self.dl.text(x + 4.0, ty, 1.0, &shown, col(0xffffff));
        if focused && (self.time * 2.0) % 2.0 < 1.0 {
            self.dl.text(x + 4.0 + used, ty, 1.0, "_", col(0xffe9a8));
        }
        hover && self.click
    }
}
