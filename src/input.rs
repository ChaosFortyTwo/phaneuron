//! Input collected by the window and consumed once per frame.

#[derive(Default)]
pub struct Input {
    /// Printable characters typed this frame.
    pub typed: String,
    pub backspace: bool,
    pub enter: bool,
    pub tab: bool,
    pub escape: bool,
    /// Mouse wheel lines scrolled this frame (positive = away from you).
    pub wheel: f32,
    /// Hotbar number key pressed this frame (0-based).
    pub digit: Option<usize>,
    /// Left / right button went down this frame.
    pub click: bool,
    pub right_click: bool,
    pub left_held: bool,
    pub right_held: bool,
    /// Rebindable-key actions that fire once per press.
    pub toggle_debug: bool,
    pub toggle_console: bool,
    pub cycle_camera: bool,
    /// The "free mouse" key is currently held.
    pub free_mouse: bool,
    /// Last key pressed this frame (used while rebinding keys).
    pub last_key: Option<winit::keyboard::KeyCode>,
}

impl Input {
    /// Forget everything that only lasts one frame.
    pub fn end_frame(&mut self) {
        self.typed.clear();
        self.backspace = false;
        self.enter = false;
        self.tab = false;
        self.escape = false;
        self.wheel = 0.0;
        self.digit = None;
        self.click = false;
        self.right_click = false;
        self.toggle_debug = false;
        self.toggle_console = false;
        self.cycle_camera = false;
        self.last_key = None;
    }
}
