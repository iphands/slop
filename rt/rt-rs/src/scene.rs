use winit::keyboard::KeyCode;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SceneUBO {
    pub light_pos: [f32; 3],
    pub light_intensity: f32,
    pub light_color: [f32; 3],
    pub sphere_center_y: f32,
    pub sphere_center: [f32; 3],
    pub sphere_radius: f32,
    pub max_bounces: i32,
    pub _pad: [f32; 3],
}

pub struct SceneState {
    pub light_pos: [f32; 3],
    pub light_color: [f32; 3],
    pub light_intensity: f32,
    pub sphere_orbit_angle: f32,
    pub sphere_radius: f32,
    pub orbit_radius: f32,
    pub orbit_speed: f32,
    pub max_bounces: i32,
}

impl SceneState {
    pub fn new() -> Self {
        SceneState {
            light_pos: [0.0, 0.9, 0.0],
            light_color: [1.0, 1.0, 1.0],
            light_intensity: 2.0,
            sphere_orbit_angle: 0.0,
            sphere_radius: 0.3,
            orbit_radius: 0.35,
            orbit_speed: 1.0,
            max_bounces: 5,
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.sphere_orbit_angle += self.orbit_speed * dt;
        if self.sphere_orbit_angle > std::f32::consts::TAU {
            self.sphere_orbit_angle -= std::f32::consts::TAU;
        }
    }

    pub fn sphere_center(&self) -> [f32; 3] {
        let x = self.orbit_radius * self.sphere_orbit_angle.cos();
        let z = self.orbit_radius * self.sphere_orbit_angle.sin();
        let y = -1.0 + self.sphere_radius; // sitting on the floor
        [x, y, z]
    }

    pub fn to_ubo(&self) -> SceneUBO {
        let center = self.sphere_center();
        SceneUBO {
            light_pos: self.light_pos,
            light_intensity: self.light_intensity,
            light_color: self.light_color,
            sphere_center_y: center[1],
            sphere_center: center,
            sphere_radius: self.sphere_radius,
            max_bounces: self.max_bounces,
            _pad: [0.0; 3],
        }
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        let step = 0.05;
        match key {
            KeyCode::ArrowLeft => self.light_pos[0] -= step,
            KeyCode::ArrowRight => self.light_pos[0] += step,
            KeyCode::ArrowUp => self.light_pos[2] -= step,
            KeyCode::ArrowDown => self.light_pos[2] += step,
            KeyCode::PageUp => self.light_pos[1] += step,
            KeyCode::PageDown => self.light_pos[1] -= step,
            KeyCode::KeyR => self.light_color[0] = if self.light_color[0] > 0.5 { 0.0 } else { 1.0 },
            KeyCode::KeyG => self.light_color[1] = if self.light_color[1] > 0.5 { 0.0 } else { 1.0 },
            KeyCode::KeyB => self.light_color[2] = if self.light_color[2] > 0.5 { 0.0 } else { 1.0 },
            KeyCode::Equal => self.light_intensity = (self.light_intensity + 0.2).min(10.0),
            KeyCode::Minus => self.light_intensity = (self.light_intensity - 0.2).max(0.0),
            KeyCode::BracketRight => {
                self.max_bounces = (self.max_bounces + 1).min(31);
                println!("Max bounces: {}", self.max_bounces);
            }
            KeyCode::BracketLeft => {
                self.max_bounces = (self.max_bounces - 1).max(0);
                println!("Max bounces: {}", self.max_bounces);
            }
            _ => {}
        }
    }
}
