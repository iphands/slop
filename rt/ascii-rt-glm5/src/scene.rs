//! Scene definitions for the Cornell box raytracer

use nalgebra::{Vector3, Point3};

/// Material types for ray tracing
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Material {
    Diffuse { albedo: Vector3<f32> },
    Metal { albedo: Vector3<f32>, roughness: f32 },
    Dielectric { refraction_index: f32 },
    Emissive { color: Vector3<f32>, intensity: f32 },
}

/// Axis-aligned bounding box
#[derive(Debug, Clone, Copy)]
pub struct AABB {
    pub min: Point3<f32>,
    pub max: Point3<f32>,
}

impl AABB {
    pub fn new(min: Point3<f32>, max: Point3<f32>) -> Self {
        Self { min, max }
    }
}

/// Sphere primitive
#[derive(Debug, Clone, Copy)]
pub struct Sphere {
    pub center: Point3<f32>,
    pub radius: f32,
    pub material: Material,
}

impl Sphere {
    pub fn new(center: Point3<f32>, radius: f32, material: Material) -> Self {
        Self { center, radius, material }
    }

    pub fn bounds(&self) -> AABB {
        let r = Vector3::new(self.radius, self.radius, self.radius);
        AABB::new(self.center - r, self.center + r)
    }
}

/// Axis-aligned quad (for Cornell box walls)
#[derive(Debug, Clone, Copy)]
pub struct Quad {
    pub corner: Point3<f32>,
    pub u: Vector3<f32>,
    pub v: Vector3<f32>,
    pub material: Material,
    pub normal: Vector3<f32>,
}

impl Quad {
    pub fn new(corner: Point3<f32>, u: Vector3<f32>, v: Vector3<f32>, material: Material) -> Self {
        let n = u.cross(&v);
        let normal = n.normalize();
        Self { corner, u, v, material, normal }
    }

    pub fn bounds(&self) -> AABB {
        let p1 = self.corner;
        let p2 = self.corner + self.u;
        let p3 = self.corner + self.v;
        let p4 = self.corner + self.u + self.v;

        let min = Point3::new(
            p1.x.min(p2.x).min(p3.x).min(p4.x),
            p1.y.min(p2.y).min(p3.y).min(p4.y),
            p1.z.min(p2.z).min(p3.z).min(p4.z),
        );
        let max = Point3::new(
            p1.x.max(p2.x).max(p3.x).max(p4.x),
            p1.y.max(p2.y).max(p3.y).max(p4.y),
            p1.z.max(p2.z).max(p3.z).max(p4.z),
        );

        AABB::new(min, max)
    }
}

/// The complete scene containing all objects and a light
#[derive(Debug, Clone)]
pub struct Scene {
    pub spheres: Vec<Sphere>,
    pub quads: Vec<Quad>,
    pub light_position: Point3<f32>,
    pub light_intensity: f32,
    pub max_bounces: u32,
}

impl Default for Scene {
    fn default() -> Self {
        Self::cornell_box()
    }
}

impl Scene {
    /// Create a Cornell box scene with a glass sphere
    pub fn cornell_box() -> Self {
        let mut spheres = Vec::new();
        let mut quads = Vec::new();

        // Glass sphere (floating)
        spheres.push(Sphere::new(
            Point3::new(0.0, 0.0, 0.0),
            0.4,
            Material::Dielectric { refraction_index: 1.5 },
        ));

        // Cornell box walls
        let box_size = 2.0;

        // Floor (white) - swapped u and v to fix normal direction
        quads.push(Quad::new(
            Point3::new(-box_size, -box_size, -box_size),
            Vector3::new(0.0, 0.0, 2.0 * box_size),
            Vector3::new(2.0 * box_size, 0.0, 0.0),
            Material::Diffuse { albedo: Vector3::new(0.73, 0.73, 0.73) },
        ));

        // Ceiling (white)
        quads.push(Quad::new(
            Point3::new(-box_size, box_size, -box_size),
            Vector3::new(2.0 * box_size, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 2.0 * box_size),
            Material::Diffuse { albedo: Vector3::new(0.73, 0.73, 0.73) },
        ));

        // Back wall (white)
        quads.push(Quad::new(
            Point3::new(-box_size, -box_size, -box_size),
            Vector3::new(2.0 * box_size, 0.0, 0.0),
            Vector3::new(0.0, 2.0 * box_size, 0.0),
            Material::Diffuse { albedo: Vector3::new(0.73, 0.73, 0.73) },
        ));

        // Left wall (red) - swapped u and v to fix normal direction
        quads.push(Quad::new(
            Point3::new(-box_size, -box_size, -box_size),
            Vector3::new(0.0, 2.0 * box_size, 0.0),
            Vector3::new(0.0, 0.0, 2.0 * box_size),
            Material::Diffuse { albedo: Vector3::new(0.65, 0.05, 0.05) },
        ));

        // Right wall (green)
        quads.push(Quad::new(
            Point3::new(box_size, -box_size, -box_size),
            Vector3::new(0.0, 0.0, 2.0 * box_size),
            Vector3::new(0.0, 2.0 * box_size, 0.0),
            Material::Diffuse { albedo: Vector3::new(0.12, 0.45, 0.15) },
        ));

        // Small diffuse sphere
        spheres.push(Sphere::new(
            Point3::new(-0.8, -1.2, 0.5),
            0.3,
            Material::Diffuse { albedo: Vector3::new(0.8, 0.8, 0.1) },
        ));

        // Metal sphere
        spheres.push(Sphere::new(
            Point3::new(0.7, -1.2, 0.7),
            0.35,
            Material::Metal { albedo: Vector3::new(0.9, 0.9, 0.9), roughness: 0.1 },
        ));

        Self {
            spheres,
            quads,
            light_position: Point3::new(0.0, 1.5, 0.0),
            light_intensity: 15.0,
            max_bounces: 3,
        }
    }

    /// Update the glass sphere position (floating animation)
    pub fn update_sphere(&mut self, time: f32) {
        if let Some(sphere) = self.spheres.first_mut() {
            // Floating motion
            sphere.center.x = (time * 0.3).sin() * 0.5;
            sphere.center.y = (time * 0.5).sin() * 0.2;
            sphere.center.z = (time * 0.2).cos() * 0.3;
        }
    }

    /// Adjust light height
    pub fn adjust_light_height(&mut self, delta: f32) {
        self.light_position.y = (self.light_position.y + delta).clamp(-1.5, 1.9);
    }

    /// Adjust max bounces
    pub fn adjust_bounces(&mut self, delta: i32) {
        let new_bounces = self.max_bounces as i32 + delta;
        self.max_bounces = new_bounces.clamp(1, 10) as u32;
    }
}

/// Camera for viewing the scene
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub origin: Point3<f32>,
    pub look_at: Point3<f32>,
    pub up: Vector3<f32>,
    pub fov: f32,
    pub aspect_ratio: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            origin: Point3::new(0.0, 0.0, 5.0),  // Moved back from 3.5 to 5.0
            look_at: Point3::new(0.0, 0.0, 0.0),
            up: Vector3::new(0.0, 1.0, 0.0),
            fov: 40.0,
            aspect_ratio: 16.0 / 9.0,
        }
    }
}

impl Camera {
    pub fn new(origin: Point3<f32>, look_at: Point3<f32>, fov: f32, aspect_ratio: f32) -> Self {
        Self {
            origin,
            look_at,
            up: Vector3::new(0.0, 1.0, 0.0),
            fov,
            aspect_ratio,
        }
    }

    pub fn set_aspect_ratio(&mut self, ratio: f32) {
        self.aspect_ratio = ratio;
    }

    /// Move camera forward/backward along the view direction
    pub fn adjust_distance(&mut self, delta: f32) {
        // Calculate view direction
        let view_dir = (self.look_at - self.origin).normalize();
        // Move camera along view direction, clamped to reasonable range
        self.origin += view_dir * delta;
        // Clamp distance from look_at point (1.0 to 10.0 units)
        let distance = (self.look_at - self.origin).magnitude();
        if distance < 1.0 || distance > 10.0 {
            let clamped_distance = distance.clamp(1.0, 10.0);
            self.origin = self.look_at - view_dir * clamped_distance;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_creation() {
        let scene = Scene::cornell_box();
        assert!(!scene.spheres.is_empty());
        assert!(!scene.quads.is_empty());
    }

    #[test]
    fn test_sphere_bounds() {
        let sphere = Sphere::new(Point3::new(0.0, 0.0, 0.0), 1.0, Material::Diffuse { albedo: Vector3::zeros() });
        let bounds = sphere.bounds();
        assert!(bounds.min.x < bounds.max.x);
    }

    #[test]
    fn test_light_adjustment() {
        let mut scene = Scene::cornell_box();
        let initial_y = scene.light_position.y;
        scene.adjust_light_height(0.5);
        assert!(scene.light_position.y > initial_y);
    }

    #[test]
    fn test_bounce_adjustment() {
        let mut scene = Scene::cornell_box();
        scene.adjust_bounces(2);
        assert_eq!(scene.max_bounces, 5);
        scene.adjust_bounces(-10);
        assert_eq!(scene.max_bounces, 1);
    }

    #[test]
    fn test_camera_aspect_ratio() {
        let mut camera = Camera::default();
        camera.set_aspect_ratio(2.0);
        assert!((camera.aspect_ratio - 2.0).abs() < 0.001);
    }
}
