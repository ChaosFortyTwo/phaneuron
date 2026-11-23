//! Rebindable keys. Hotbar digits, Esc, Enter, Tab and Backspace are fixed.

use std::collections::{BTreeMap, HashMap};

use winit::keyboard::KeyCode;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Bind {
    Forward,
    Back,
    Left,
    Right,
    Jump,
    Sprint,
    Sneak,
    FreeMouse,
    Debug,
    Console,
    Camera,
}

impl Bind {
    pub const ALL: [Bind; 11] = [
        Bind::Forward,
        Bind::Back,
        Bind::Left,
        Bind::Right,
        Bind::Jump,
        Bind::Sprint,
        Bind::Sneak,
        Bind::FreeMouse,
        Bind::Debug,
        Bind::Console,
        Bind::Camera,
    ];

    /// Name used in settings.toml.
    pub fn id(self) -> &'static str {
        match self {
            Bind::Forward => "forward",
            Bind::Back => "back",
            Bind::Left => "left",
            Bind::Right => "right",
            Bind::Jump => "jump",
            Bind::Sprint => "sprint",
            Bind::Sneak => "sneak",
            Bind::FreeMouse => "free_mouse",
            Bind::Debug => "debug",
            Bind::Console => "console",
            Bind::Camera => "camera",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Bind::Forward => "Move forward",
            Bind::Back => "Move back",
            Bind::Left => "Move left",
            Bind::Right => "Move right",
            Bind::Jump => "Jump / fly up",
            Bind::Sprint => "Sprint",
            Bind::Sneak => "Sneak / fly down",
            Bind::FreeMouse => "Free mouse (hold)",
            Bind::Debug => "Debug screen",
            Bind::Console => "Console",
            Bind::Camera => "Camera view",
        }
    }

    pub fn default_key(self) -> KeyCode {
        match self {
            Bind::Forward => KeyCode::KeyW,
            Bind::Back => KeyCode::KeyS,
            Bind::Left => KeyCode::KeyA,
            Bind::Right => KeyCode::KeyD,
            Bind::Jump => KeyCode::Space,
            Bind::Sprint => KeyCode::ShiftLeft,
            Bind::Sneak => KeyCode::KeyC,
            Bind::FreeMouse => KeyCode::AltLeft,
            Bind::Debug => KeyCode::F3,
            Bind::Console => KeyCode::F2,
            Bind::Camera => KeyCode::F4,
        }
    }
}

macro_rules! key_table {
    ($($k:ident),* $(,)?) => {
        const KEY_TABLE: &[(&str, KeyCode)] = &[ $( (stringify!($k), KeyCode::$k) ),* ];
    };
}

key_table!(
    KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO, KeyP, KeyQ, KeyR,
    KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    Space, Tab, ShiftLeft, ShiftRight, ControlLeft, ControlRight, AltLeft, AltRight,
    ArrowUp, ArrowDown, ArrowLeft, ArrowRight,
    Comma, Period, Slash, Semicolon, Quote, BracketLeft, BracketRight, Backslash, Minus, Equal, Backquote,
    CapsLock, Home, End, PageUp, PageDown, Insert, Delete,
    Numpad0, Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9,
);

/// Stable name of a bindable key (None for keys that cannot be bound).
pub fn key_name(code: KeyCode) -> Option<&'static str> {
    KEY_TABLE.iter().find(|(_, c)| *c == code).map(|(n, _)| *n)
}

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEY_TABLE.iter().find(|(n, _)| *n == name).map(|(_, c)| *c)
}

/// Human-readable label, e.g. `KeyW` -> "W", `ShiftLeft` -> "Left Shift", `PageUp` -> "Page Up".
pub fn key_label(code: KeyCode) -> String {
    let Some(name) = key_name(code) else { return "?".into() };
    if let Some(rest) = name.strip_prefix("Key") {
        return rest.to_string();
    }
    if let Some(rest) = name.strip_prefix("Arrow") {
        return format!("{rest} Arrow");
    }
    for side in ["Left", "Right"] {
        if let Some(base) = name.strip_suffix(side) {
            if matches!(base, "Shift" | "Control" | "Alt") {
                return format!("{side} {base}");
            }
        }
    }
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() && !name.starts_with('F') {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

#[derive(Clone, PartialEq, Debug)]
pub struct Keybinds {
    map: HashMap<Bind, KeyCode>,
}

impl Default for Keybinds {
    fn default() -> Self {
        Keybinds { map: Bind::ALL.iter().map(|&b| (b, b.default_key())).collect() }
    }
}

impl Keybinds {
    pub fn get(&self, b: Bind) -> KeyCode {
        self.map.get(&b).copied().unwrap_or_else(|| b.default_key())
    }

    pub fn lookup(&self, code: KeyCode) -> Option<Bind> {
        Bind::ALL.iter().copied().find(|&b| self.get(b) == code)
    }

    /// Bind `code` to `b`. If another action already used that key, the two swap keys,
    /// so every key always has at most one action.
    pub fn set(&mut self, b: Bind, code: KeyCode) {
        if let Some(other) = self.lookup(code) {
            if other != b {
                let old = self.get(b);
                self.map.insert(other, old);
            }
        }
        self.map.insert(b, code);
    }

    pub fn to_map(&self) -> BTreeMap<String, String> {
        Bind::ALL
            .iter()
            .filter_map(|&b| key_name(self.get(b)).map(|n| (b.id().to_string(), n.to_string())))
            .collect()
    }

    /// Defaults overlaid with whatever the file says; unknown entries are ignored.
    pub fn from_map(map: &BTreeMap<String, String>) -> Keybinds {
        let mut binds = Keybinds::default();
        for &b in &Bind::ALL {
            if let Some(code) = map.get(b.id()).and_then(|n| key_from_name(n)) {
                binds.set(b, code);
            }
        }
        binds
    }
}
