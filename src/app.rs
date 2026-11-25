//! Window, input plumbing, and the screen state machine
//! (menu -> playing <-> paused -> menu).

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{DeviceEvent, DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

use crate::assets::Assets;
use crate::blocks::{Atlas, Registry};
use crate::config::Config;
use crate::game::Session;
use crate::input::Input;
use crate::keys::Bind;
use crate::menu::{Action, Menu};
use crate::player::Keys;
use crate::preview::{self, PreviewState};
use crate::render::{Renderer, Scene};
use crate::save::{self, WorldMeta};
use crate::settings::Settings;
use crate::settings_ui::SettingsScreen;
use crate::ui::{col, col_a, DrawList, Tex, Ui};

/// Read-only things every screen needs.
struct Ctx<'a> {
    registry: &'a Arc<Registry>,
    atlas: &'a Atlas,
    saves: &'a PathBuf,
    base: &'a PathBuf,
}

enum Screen {
    Menu(Menu),
    Playing(Session),
    Paused(Session),
}

struct State {
    window: Arc<Window>,
    renderer: Renderer,
    screen: Screen,
    input: Input,
    keys: Keys,
    mouse: (f32, f32), // physical pixels
    grabbed: bool,
    last: Instant,
    started: Instant,
    fps_timer: Instant,
    frames: u32,
    fps: f32,
    show_debug: bool,
    preview: PreviewState,
    cfg: Config,
    settings: Settings,
    pause_settings: Option<SettingsScreen>,
    base: PathBuf,
}

pub struct App {
    assets: Assets,
    cfg: Config,
    registry: Arc<Registry>,
    atlas: Atlas,
    saves: PathBuf,
    state: Option<State>,
}

impl App {
    pub fn new(assets: Assets, cfg: Config, registry: Arc<Registry>, atlas: Atlas, saves: PathBuf) -> App {
        App { assets, cfg, registry, atlas, saves, state: None }
    }
}

/// Open a folder in the system file manager.
fn open_folder(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    if let Err(e) = std::process::Command::new(cmd).arg(path).spawn() {
        eprintln!("cannot open {}: {e}", path.display());
    }
}

impl State {
    fn new(event_loop: &ActiveEventLoop, assets: &Assets, cfg: &Config, atlas: &Atlas) -> Result<State> {
        let base = assets.base.clone();
        let settings = Settings::load(&base, cfg);
        let mut cfg = cfg.clone();
        settings.apply(&mut cfg);

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(cfg.window.title.clone())
                        .with_inner_size(LogicalSize::new(cfg.window.width, cfg.window.height)),
                )
                .context("cannot create window")?,
        );
        let renderer = pollster::block_on(Renderer::new(window.clone(), assets, &cfg, atlas))?;
        window.request_redraw();
        Ok(State {
            window,
            renderer,
            screen: Screen::Menu(Menu::main()),
            input: Input::default(),
            keys: Keys::default(),
            mouse: (0.0, 0.0),
            grabbed: false,
            last: Instant::now(),
            started: Instant::now(),
            fps_timer: Instant::now(),
            frames: 0,
            fps: 0.0,
            show_debug: false,
            preview: PreviewState::None,
            cfg,
            settings,
            pause_settings: None,
            base,
        })
    }

    fn console_open(&self) -> bool {
        matches!(&self.screen, Screen::Playing(s) if s.console.open)
    }

    fn set_grab(&mut self, grab: bool) {
        self.grabbed = grab;
        if grab {
            let r = self
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined));
            if let Err(e) = r {
                eprintln!("warning: cannot capture the mouse: {e}");
            }
            self.window.set_cursor_visible(false);
        } else {
            let _ = self.window.set_cursor_grab(CursorGrabMode::None);
            self.window.set_cursor_visible(true);
        }
    }

    // ---------- events ----------

    fn key(&mut self, ev: &KeyEvent) {
        let down = ev.state == ElementState::Pressed;
        let fresh = down && !ev.repeat;
        if down {
            if let Some(t) = &ev.text {
                self.input.typed.push_str(t.as_str());
            }
        }
        let PhysicalKey::Code(code) = ev.physical_key else { return };
        if fresh {
            self.input.last_key = Some(code);
        }

        // fixed keys
        match code {
            KeyCode::Backspace if down => self.input.backspace = true,
            KeyCode::Enter | KeyCode::NumpadEnter if fresh => self.input.enter = true,
            KeyCode::Tab if fresh => self.input.tab = true,
            KeyCode::Escape if fresh => self.input.escape = true,
            KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 | KeyCode::Digit4 | KeyCode::Digit5
            | KeyCode::Digit6 | KeyCode::Digit7 | KeyCode::Digit8 | KeyCode::Digit9
                if down =>
            {
                self.input.digit = Some(match code {
                    KeyCode::Digit1 => 0,
                    KeyCode::Digit2 => 1,
                    KeyCode::Digit3 => 2,
                    KeyCode::Digit4 => 3,
                    KeyCode::Digit5 => 4,
                    KeyCode::Digit6 => 5,
                    KeyCode::Digit7 => 6,
                    KeyCode::Digit8 => 7,
                    _ => 8,
                });
            }
            _ => {}
        }

        // rebindable keys. While the console is open, movement keys are ignored but
        // key-up still clears them, so nothing gets stuck.
        let typing = self.console_open();
        let Some(bind) = self.settings.binds.lookup(code) else { return };
        let held = down && !typing;
        match bind {
            Bind::Forward => self.keys.forward = held,
            Bind::Back => self.keys.back = held,
            Bind::Left => self.keys.left = held,
            Bind::Right => self.keys.right = held,
            Bind::Jump => self.keys.jump = held,
            Bind::Sprint => self.keys.sprint = held,
            Bind::Sneak => self.keys.sneak = held,
            Bind::FreeMouse => self.input.free_mouse = held,
            Bind::Debug if fresh => self.input.toggle_debug = true,
            Bind::Console if fresh => self.input.toggle_console = true,
            Bind::Camera if fresh && !typing => self.input.cycle_camera = true,
            _ => {}
        }
    }

    fn mouse_button(&mut self, state: ElementState, button: MouseButton) {
        let down = state == ElementState::Pressed;
        match button {
            MouseButton::Left => {
                self.input.left_held = down;
                if down {
                    self.input.click = true;
                }
            }
            MouseButton::Right => {
                self.input.right_held = down;
                if down {
                    self.input.right_click = true;
                }
            }
            _ => {}
        }
    }

    fn wheel(&mut self, delta: MouseScrollDelta) {
        self.input.wheel += match delta {
            MouseScrollDelta::LineDelta(_, y) => y,
            MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
        };
    }

    fn mouse_motion(&mut self, (dx, dy): (f64, f64)) {
        if !self.grabbed {
            return;
        }
        if let Screen::Playing(s) = &mut self.screen {
            let sens = self.cfg.player.mouse_sensitivity;
            s.player.yaw += dx as f32 * sens;
            let lim = 89f32.to_radians();
            s.player.pitch = (s.player.pitch - dy as f32 * sens).clamp(-lim, lim);
        }
    }

    fn lost_focus(&mut self) {
        if let Screen::Playing(_) = self.screen {
            self.pause();
        }
        self.keys = Keys::default();
        self.input.free_mouse = false;
    }

    // ---------- transitions ----------

    fn pause(&mut self) {
        let screen = std::mem::replace(&mut self.screen, Screen::Menu(Menu::main()));
        self.screen = match screen {
            Screen::Playing(mut s) => {
                s.console.open = false;
                Screen::Paused(s)
            }
            other => other,
        };
        self.pause_settings = None;
        self.keys = Keys::default();
    }

    fn resume(&mut self) {
        let screen = std::mem::replace(&mut self.screen, Screen::Menu(Menu::main()));
        self.screen = match screen {
            Screen::Paused(s) => Screen::Playing(s),
            other => other,
        };
        self.pause_settings = None;
        self.input.left_held = false;
    }

    fn start_session(&mut self, ctx: &Ctx, meta: WorldMeta) {
        let saved = match save::load_world(&meta, ctx.registry) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("cannot load world: {e:#}");
                self.screen = Screen::Menu(Menu::main_with_message(format!("Could not load: {e}")));
                return;
            }
        };
        self.renderer.clear_chunks();
        let session = Session::start(ctx.registry.clone(), &self.cfg, meta, saved);
        self.screen = Screen::Playing(session);
        self.input.left_held = false;
        self.keys = Keys::default();
    }

    /// Build the 3D preview for a world picked on the Load World screen.
    fn build_preview(&mut self, ctx: &Ctx, meta: &WorldMeta) {
        match preview::build(ctx.registry, ctx.atlas, &self.cfg, meta) {
            Ok(built) => {
                self.renderer.set_preview(&built.meshes, built.target);
                self.preview = PreviewState::Ready(built.info);
            }
            Err(e) => {
                eprintln!("cannot preview {}: {e:#}", meta.folder.display());
                self.renderer.clear_preview();
                self.preview = PreviewState::Failed(meta.folder.clone(), format!("{e}"));
            }
        }
    }

    /// Save the running world (if any) and show the main menu.
    fn leave_to_menu(&mut self) {
        let screen = std::mem::replace(&mut self.screen, Screen::Menu(Menu::main()));
        if let Screen::Playing(mut s) | Screen::Paused(mut s) = screen {
            if let Err(e) = s.save() {
                eprintln!("cannot save world: {e:#}");
                self.screen = Screen::Menu(Menu::main_with_message(format!("Save failed: {e}")));
            }
        }
        self.pause_settings = None;
        self.renderer.clear_chunks();
    }

    fn apply(&mut self, ctx: &Ctx, action: Action) {
        match action {
            Action::None => {}
            Action::OpenFiles => open_folder(ctx.base),
            Action::Load(meta) => self.start_session(ctx, meta),
            Action::Select(meta) => self.build_preview(ctx, &meta),
            Action::Create { name, seed } => match save::create_world(ctx.saves, &name, seed) {
                Ok(meta) => self.start_session(ctx, meta),
                Err(e) => self.screen = Screen::Menu(Menu::main_with_message(format!("Could not create world: {e}"))),
            },
        }
    }

    // ---------- one frame ----------

    fn frame(&mut self, ctx: &Ctx) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.05);
        self.last = now;
        let settings_before = self.settings.clone();

        let (pw, ph) = self.renderer.size();
        let scale = if self.cfg.render.ui_scale > 0.0 { self.cfg.render.ui_scale } else { (ph as f32 / 360.0).floor().max(1.0) };
        let mut ui = Ui {
            dl: DrawList::default(),
            w: pw as f32 / scale,
            h: ph as f32 / scale,
            mouse: (self.mouse.0 / scale, self.mouse.1 / scale),
            click: self.input.click,
            time: self.started.elapsed().as_secs_f32(),
            banner: self.renderer.ui_size(Tex::Banner),
            crosshair: self.renderer.ui_size(Tex::Crosshair),
        };

        let mut action = Action::None;
        let mut want_pause = false;
        let mut want_resume = false;
        let mut want_menu = false;
        let mut close_settings = false;

        match &mut self.screen {
            Screen::Menu(menu) => action = menu.ui(&mut ui, &self.input, ctx.saves, dt, &mut self.settings, &self.preview),
            Screen::Playing(s) => {
                // Esc pauses, unless it is closing the console this frame.
                want_pause = self.input.escape && !s.console.open;
                if self.input.toggle_console {
                    s.console.open = !s.console.open;
                    self.input.typed.clear();
                }
                if self.input.toggle_debug {
                    self.show_debug = !self.show_debug;
                }
                if self.input.cycle_camera {
                    s.cycle_view();
                }
                let typing = s.console.open;
                let keys = if typing { Keys::default() } else { self.keys };

                s.update(dt, &keys, &self.cfg);
                s.stream(&mut self.renderer, ctx.atlas, &self.cfg);
                if !typing {
                    if self.input.wheel != 0.0 {
                        s.select_delta(if self.input.wheel > 0.0 { -1 } else { 1 });
                    }
                    if let Some(d) = self.input.digit {
                        s.select(d);
                    }
                    // clicks only edit blocks while the mouse is captured (not while Alt frees it)
                    if self.grabbed && !self.input.free_mouse {
                        s.interact(dt, &self.input, &mut self.renderer, ctx.atlas);
                    }
                }
                s.hud(&mut ui, ctx.atlas);
                if self.show_debug {
                    s.debug_overlay(&mut ui, self.fps, &self.cfg);
                }
                s.console_ui(&mut ui, &self.input);
                s.autosave();
            }
            Screen::Paused(s) => {
                s.stream(&mut self.renderer, ctx.atlas, &self.cfg);
                ui.dl.rect(0.0, 0.0, ui.w, ui.h, col_a(0x000000, 0.55));
                if let Some(ss) = &mut self.pause_settings {
                    close_settings = ss.ui(&mut ui, &self.input, &mut self.settings);
                } else {
                    let (bw, bh, gap) = (176.0, 22.0, 8.0);
                    let top = (ui.h - (3.0 * bh + 2.0 * gap)) / 2.0;
                    let x = (ui.w - bw) / 2.0;
                    ui.dl.text_centered(ui.w / 2.0, top - 34.0, 2.0, "Paused", col(0xffe9a8));
                    if ui.button(x, top, bw, bh, "Resume") || self.input.escape {
                        want_resume = true;
                    }
                    if ui.button(x, top + bh + gap, bw, bh, "Settings") {
                        self.pause_settings = Some(SettingsScreen::new());
                    }
                    if ui.button(x, top + 2.0 * (bh + gap), bw, bh, "Save and Quit") {
                        want_menu = true;
                    }
                }
            }
        }
        if close_settings {
            self.pause_settings = None;
        }

        // The mouse is captured only while actually playing: not in menus, not while
        // typing in the console, and not while the "free mouse" key is held.
        let want_grab = matches!(&self.screen, Screen::Playing(s) if !s.console.open) && !self.input.free_mouse;
        if want_grab != self.grabbed {
            self.set_grab(want_grab);
        }

        // ---- render ----
        let camera = match &self.screen {
            Screen::Playing(s) | Screen::Paused(s) => Some(s.camera()),
            Screen::Menu(_) => None,
        };
        let highlight = match &self.screen {
            Screen::Playing(s) => s.highlight(),
            _ => None,
        };
        let model = match &self.screen {
            Screen::Playing(s) | Screen::Paused(s) => s.model_mesh(ctx.atlas),
            Screen::Menu(_) => None,
        };
        let showing_preview = matches!(&self.screen, Screen::Menu(Menu::Load { .. })) && matches!(self.preview, PreviewState::Ready(_));
        let preview_angle = showing_preview.then(|| self.started.elapsed().as_secs_f32() * 0.4);
        let scene = Scene { camera: camera.as_ref(), highlight, dynamic: model.as_ref(), ui: &ui.dl, ui_scale: scale, preview_angle };
        match self.renderer.render(&scene) {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                let (w, h) = self.renderer.size();
                self.renderer.resize(w, h);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => panic!("GPU out of memory"),
            Err(e) => eprintln!("render error: {e:?}"),
        }

        // ---- transitions (after drawing, so no borrows are alive) ----
        self.input.end_frame();
        if !matches!(&self.screen, Screen::Menu(Menu::Load { .. })) && !matches!(self.preview, PreviewState::None) {
            self.renderer.clear_preview();
            self.preview = PreviewState::None;
        }
        if want_pause {
            self.pause();
        }
        if want_resume {
            self.resume();
        }
        if want_menu {
            self.leave_to_menu();
        }
        self.apply(ctx, action);

        // ---- settings changed this frame: apply live and save ----
        if self.settings != settings_before {
            self.settings.apply(&mut self.cfg);
            self.renderer.set_view(self.cfg.render.render_distance, self.cfg.render.fov);
            if let Err(e) = self.settings.save(&self.base) {
                eprintln!("cannot save settings: {e:#}");
            }
        }

        // ---- title / fps ----
        self.frames += 1;
        if self.fps_timer.elapsed() >= Duration::from_millis(500) {
            self.fps = self.frames as f32 / self.fps_timer.elapsed().as_secs_f32();
            let title = match &self.screen {
                Screen::Playing(s) | Screen::Paused(s) => format!("{} | {}", self.cfg.window.title, s.meta.name),
                Screen::Menu(_) => self.cfg.window.title.clone(),
            };
            self.window.set_title(&title);
            self.frames = 0;
            self.fps_timer = Instant::now();
        }
    }

    /// Save on the way out.
    fn shutdown(&mut self) {
        if let Screen::Playing(s) | Screen::Paused(s) = &mut self.screen {
            if let Err(e) = s.save() {
                eprintln!("cannot save world: {e:#}");
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match State::new(event_loop, &self.assets, &self.cfg, &self.atlas) {
            Ok(s) => self.state = Some(s),
            Err(e) => {
                eprintln!("fatal: {e:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(st) = self.state.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                st.shutdown();
                event_loop.exit();
            }
            WindowEvent::Resized(s) => st.renderer.resize(s.width, s.height),
            WindowEvent::KeyboardInput { event, .. } => st.key(&event),
            WindowEvent::CursorMoved { position, .. } => st.mouse = (position.x as f32, position.y as f32),
            WindowEvent::MouseInput { state, button, .. } => st.mouse_button(state, button),
            WindowEvent::MouseWheel { delta, .. } => st.wheel(delta),
            WindowEvent::Focused(false) => st.lost_focus(),
            WindowEvent::RedrawRequested => {
                let ctx = Ctx { registry: &self.registry, atlas: &self.atlas, saves: &self.saves, base: &self.assets.base };
                st.frame(&ctx);
                st.window.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, Some(st)) = (event, self.state.as_mut()) {
            st.mouse_motion(delta);
        }
    }
}
