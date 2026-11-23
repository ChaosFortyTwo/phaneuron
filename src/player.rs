//! First-person walking: gravity, jumping, sneaking, free flight, AABB collision
//! against solid blocks, and automatic step-up for one-block ledges.

use glam::Vec3;

use crate::config::PlayerCfg;
use crate::world::{World, BLOCK};

pub const HALF_WIDTH: f32 = 0.3;
/// Standing hitbox: 2.0 units = 4 blocks.
pub const HEIGHT: f32 = 2.0;
/// Sneaking hitbox: 1.0 unit = 2 blocks.
pub const SNEAK_HEIGHT: f32 = 1.0;

#[derive(Default, Clone, Copy)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sprint: bool,
    pub sneak: bool,
}

pub struct Player {
    pub pos: Vec3, // feet, centered, in world units
    pub vel_y: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub sneaking: bool,
    pub flying: bool,
    /// Current eye height above the feet (eases when crouching).
    pub eye: f32,
    /// Camera offset that smooths out step-ups (<= 0, decays back to 0).
    pub visual_dy: f32,
    /// Walk-cycle state for the player model.
    pub walk_phase: f32,
    pub walk_amp: f32,
}

impl Player {
    pub fn new(pos: Vec3) -> Self {
        Player {
            pos,
            vel_y: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            sneaking: false,
            flying: false,
            eye: 1.8,
            visual_dy: 0.0,
            walk_phase: 0.0,
            walk_amp: 0.0,
        }
    }

    pub fn height(&self) -> f32 {
        if self.sneaking {
            SNEAK_HEIGHT
        } else {
            HEIGHT
        }
    }

    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, -cy * cp)
    }

    /// Would a box of `height` standing at feet position `p` overlap a solid block?
    pub fn collides(world: &World, p: Vec3, height: f32) -> bool {
        let inv = 1.0 / BLOCK;
        let eps = 1e-4;
        let (min, max) = (
            Vec3::new(p.x - HALF_WIDTH, p.y, p.z - HALF_WIDTH) * inv,
            Vec3::new(p.x + HALF_WIDTH, p.y + height, p.z + HALF_WIDTH) * inv,
        );
        for y in min.y.floor() as i32..=(max.y - eps).floor() as i32 {
            for z in min.z.floor() as i32..=(max.z - eps).floor() as i32 {
                for x in min.x.floor() as i32..=(max.x - eps).floor() as i32 {
                    if world.is_solid(x, y, z) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn update(&mut self, dt: f32, keys: &Keys, world: &World, cfg: &PlayerCfg) {
        let (sy, cy) = self.yaw.sin_cos();
        let fwd_flat = Vec3::new(sy, 0.0, -cy);
        let right = Vec3::new(cy, 0.0, sy);
        let axis = |pos: bool, neg: bool| pos as i32 as f32 - neg as i32 as f32;

        // Crouching shrinks instantly, but standing back up needs headroom.
        if self.flying {
            self.sneaking = false;
        } else if keys.sneak {
            self.sneaking = true;
        } else if self.sneaking && !Self::collides(world, self.pos, HEIGHT) {
            self.sneaking = false;
        }
        let height = self.height();
        let target_eye = cfg.eye_height * height / HEIGHT;
        self.eye += (target_eye - self.eye) * (14.0 * dt).min(1.0);

        let (vx, vy, vz);
        if self.flying {
            let wish = (self.forward() * axis(keys.forward, keys.back)
                + right * axis(keys.right, keys.left)
                + Vec3::Y * axis(keys.jump, keys.sneak))
            .normalize_or_zero();
            let speed = cfg.fly_speed * if keys.sprint { 2.0 } else { 1.0 };
            (vx, vy, vz) = (wish.x * speed, wish.y * speed, wish.z * speed);
            self.vel_y = 0.0;
        } else {
            let wish = (fwd_flat * axis(keys.forward, keys.back) + right * axis(keys.right, keys.left)).normalize_or_zero();
            let speed = cfg.walk_speed
                * if self.sneaking {
                    cfg.sneak_multiplier
                } else if keys.sprint {
                    cfg.sprint_multiplier
                } else {
                    1.0
                };
            if keys.jump && self.on_ground && !self.sneaking {
                self.vel_y = (2.0 * cfg.gravity * cfg.jump_height).sqrt();
                self.on_ground = false;
            }
            self.vel_y = (self.vel_y - cfg.gravity * dt).max(-60.0);
            (vx, vy, vz) = (wish.x * speed, self.vel_y, wish.z * speed);
        }
        self.visual_dy = (self.visual_dy + 4.0 * dt).min(0.0);

        // Small sub-steps keep fast movement from tunnelling and keep gaps to walls tiny.
        let longest = vx.abs().max(vy.abs()).max(vz.abs()) * dt;
        let steps = ((longest / 0.05).ceil() as i32).max(1);
        let h = dt / steps as f32;
        let mut vy_now = vy;

        for _ in 0..steps {
            for axis_i in 0..2 {
                let mut np = self.pos;
                if axis_i == 0 {
                    np.x += vx * h;
                } else {
                    np.z += vz * h;
                }
                if !Self::collides(world, np, height) {
                    self.pos = np;
                } else if !self.flying && self.on_ground && self.vel_y <= 0.0 {
                    // Blocked: try stepping up exactly one block.
                    let mut up = np;
                    up.y = self.pos.y + BLOCK;
                    if !Self::collides(world, up, height) {
                        self.pos = up;
                        self.visual_dy = (self.visual_dy - BLOCK).max(-BLOCK * 1.5);
                    }
                }
            }
            let mut np = self.pos;
            np.y += vy_now * h;
            if !Self::collides(world, np, height) {
                self.pos = np;
            } else {
                if !self.flying {
                    if self.vel_y < 0.0 {
                        self.pos.y = (self.pos.y / BLOCK).floor() * BLOCK; // rest exactly on the block below
                    }
                    self.vel_y = 0.0;
                }
                vy_now = 0.0;
            }
        }

        self.on_ground = !self.flying && Self::collides(world, self.pos - Vec3::new(0.0, 0.01, 0.0), height);

        // walk cycle for the model
        let speed = (vx * vx + vz * vz).sqrt();
        let moving = speed > 0.1 && (self.on_ground || self.flying);
        let target = if moving { 1.0 } else { 0.0 };
        self.walk_amp += (target - self.walk_amp) * (10.0 * dt).min(1.0);
        self.walk_phase += speed * dt * 2.2;
    }
}
