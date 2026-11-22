//! First-person walking: gravity, jumping, AABB collision against solid blocks.

use glam::Vec3;

use crate::config::PlayerCfg;
use crate::world::World;

const HALF_WIDTH: f32 = 0.3;
const HEIGHT: f32 = 1.8;

#[derive(Default, Clone, Copy)]
pub struct Keys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub jump: bool,
    pub sprint: bool,
}

pub struct Player {
    pub pos: Vec3, // feet, centered
    pub vel_y: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl Player {
    pub fn new(pos: Vec3) -> Self {
        Player { pos, vel_y: 0.0, yaw: 0.0, pitch: 0.0, on_ground: false }
    }

    pub fn forward(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, -cy * cp)
    }

    fn collides(world: &World, p: Vec3) -> bool {
        let (min, max) = (
            Vec3::new(p.x - HALF_WIDTH, p.y, p.z - HALF_WIDTH),
            Vec3::new(p.x + HALF_WIDTH, p.y + HEIGHT, p.z + HALF_WIDTH),
        );
        let eps = 1e-4;
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

    #[cfg(test)]
    pub fn collides_for_test(world: &World, p: Vec3) -> bool {
        Self::collides(world, p)
    }

    pub fn update(&mut self, dt: f32, keys: &Keys, world: &World, cfg: &PlayerCfg) {
        let (sy, cy) = self.yaw.sin_cos();
        let fwd = Vec3::new(sy, 0.0, -cy);
        let right = Vec3::new(cy, 0.0, sy);
        let mut wish = fwd * (keys.forward as i32 - keys.back as i32) as f32
            + right * (keys.right as i32 - keys.left as i32) as f32;
        wish = wish.normalize_or_zero();
        let speed = cfg.walk_speed * if keys.sprint { cfg.sprint_multiplier } else { 1.0 };
        let vx = wish.x * speed;
        let vz = wish.z * speed;

        if keys.jump && self.on_ground {
            self.vel_y = (2.0 * cfg.gravity * cfg.jump_height).sqrt();
            self.on_ground = false;
        }
        self.vel_y = (self.vel_y - cfg.gravity * dt).max(-60.0);

        // Small sub-steps keep fast falls from tunnelling and keep gaps to walls tiny.
        let longest = (vx.abs().max(vz.abs()).max(self.vel_y.abs())) * dt;
        let steps = ((longest / 0.05).ceil() as i32).max(1);
        let h = dt / steps as f32;

        for _ in 0..steps {
            let mut np = self.pos;
            np.x += vx * h;
            if !Self::collides(world, np) {
                self.pos = np;
            }
            let mut np = self.pos;
            np.z += vz * h;
            if !Self::collides(world, np) {
                self.pos = np;
            }
            let mut np = self.pos;
            np.y += self.vel_y * h;
            if !Self::collides(world, np) {
                self.pos = np;
            } else {
                if self.vel_y < 0.0 {
                    self.pos.y = self.pos.y.floor(); // rest exactly on top of the block
                }
                self.vel_y = 0.0;
            }
        }

        self.on_ground = Self::collides(world, self.pos - Vec3::new(0.0, 0.01, 0.0));
    }
}
