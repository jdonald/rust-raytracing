use glam::{Mat4, Vec3};
use winit::keyboard::KeyCode;

pub struct Camera {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub mouse_sensitivity: f32,
}

impl Camera {
    pub fn new() -> Self {
        Self {
            position: Vec3::new(0.0, 2.0, 10.0),
            forward: Vec3::new(0.0, 0.0, -1.0),
            up: Vec3::Y,
            right: Vec3::X,
            yaw: -90.0,
            pitch: 0.0,
            speed: 0.1,
            mouse_sensitivity: 0.1,
        }
    }

    pub fn update_vectors(&mut self) {
        let front = Vec3::new(
            self.yaw.to_radians().cos() * self.pitch.to_radians().cos(),
            self.pitch.to_radians().sin(),
            self.yaw.to_radians().sin() * self.pitch.to_radians().cos(),
        ).normalize();
        self.forward = front;
        self.right = self.forward.cross(Vec3::Y).normalize();
        self.up = self.right.cross(self.forward).normalize();
    }

    pub fn handle_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::KeyW => self.position += self.forward * self.speed,
            KeyCode::KeyS => self.position -= self.forward * self.speed,
            KeyCode::KeyA => self.position -= self.right * self.speed,
            KeyCode::KeyD => self.position += self.right * self.speed,
            KeyCode::KeyQ => self.position += Vec3::Y * self.speed,
            KeyCode::KeyE => self.position -= Vec3::Y * self.speed,
            _ => {}
        }
    }

    pub fn handle_mouse_input(&mut self, dx: f64, dy: f64) {
        self.yaw += dx as f32 * self.mouse_sensitivity;
        self.pitch -= dy as f32 * self.mouse_sensitivity; // Invert Y

        if self.pitch > 89.0 {
            self.pitch = 89.0;
        }
        if self.pitch < -89.0 {
            self.pitch = -89.0;
        }
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position, self.position + self.forward, self.up)
    }

    pub fn proj_matrix(&self, aspect: f32) -> Mat4 {
        // Vulkan has inverted Y-axis compared to OpenGL
        let mut proj = Mat4::perspective_rh(45.0f32.to_radians(), aspect, 0.1, 1000.0);
        // Flip Y-axis for Vulkan's coordinate system
        proj.y_axis.y *= -1.0;
        proj
    }
}
