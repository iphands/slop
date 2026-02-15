//! CPU-based ray tracer renderer
//!
//! This module implements a path tracer that renders the scene to a framebuffer.

use crate::scene::{Scene, Camera, Sphere, Quad, Material, AABB};
use nalgebra::{Vector3, Point3};
use rayon::prelude::*;

/// A ray in 3D space
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    pub origin: Point3<f32>,
    pub direction: Vector3<f32>,
}

impl Ray {
    pub fn new(origin: Point3<f32>, direction: Vector3<f32>) -> Self {
        Self { origin, direction }
    }

    pub fn at(&self, t: f32) -> Point3<f32> {
        self.origin + self.direction * t
    }
}

/// Hit record for ray-object intersection
#[derive(Debug, Clone)]
pub struct HitRecord {
    pub t: f32,
    pub point: Point3<f32>,
    pub normal: Vector3<f32>,
    pub front_face: bool,
    pub material: Material,
}

impl HitRecord {
    pub fn new(t: f32, point: Point3<f32>, outward_normal: Vector3<f32>, ray: &Ray, material: Material) -> Self {
        let front_face = ray.direction.dot(&outward_normal) < 0.0;
        let normal = if front_face { outward_normal } else { -outward_normal };
        Self { t, point, normal, front_face, material }
    }
}

/// Trait for hittable objects
pub trait Hittable {
    fn hit(&self, ray: &Ray, t_min: f32, t_max: f32) -> Option<HitRecord>;
    fn bounds(&self) -> AABB;
}

impl Hittable for Sphere {
    fn hit(&self, ray: &Ray, t_min: f32, t_max: f32) -> Option<HitRecord> {
        let oc = ray.origin - self.center;
        let a = ray.direction.magnitude_squared();
        let half_b = oc.dot(&ray.direction);
        let c = oc.magnitude_squared() - self.radius * self.radius;

        let discriminant = half_b * half_b - a * c;
        if discriminant < 0.0 {
            return None;
        }

        let sqrt_d = discriminant.sqrt();

        // Find the nearest root in range
        let mut root = (-half_b - sqrt_d) / a;
        if root < t_min || root > t_max {
            root = (-half_b + sqrt_d) / a;
            if root < t_min || root > t_max {
                return None;
            }
        }

        let t = root;
        let point = ray.at(t);
        let outward_normal = (point - self.center) / self.radius;

        Some(HitRecord::new(t, point, outward_normal, ray, self.material))
    }

    fn bounds(&self) -> AABB {
        self.bounds()
    }
}

impl Hittable for Quad {
    fn hit(&self, ray: &Ray, t_min: f32, t_max: f32) -> Option<HitRecord> {
        let denom = self.normal.dot(&ray.direction);
        if denom.abs() < 1e-8 {
            return None;
        }

        let t = (self.corner - ray.origin).dot(&self.normal) / denom;
        if t < t_min || t > t_max {
            return None;
        }

        let point = ray.at(t);

        // Check if point is within quad bounds
        let p = point - self.corner;
        let u_dot = p.dot(&self.u);
        let v_dot = p.dot(&self.v);
        let u_len_sq = self.u.magnitude_squared();
        let v_len_sq = self.v.magnitude_squared();

        if u_dot < 0.0 || u_dot > u_len_sq || v_dot < 0.0 || v_dot > v_len_sq {
            return None;
        }

        Some(HitRecord::new(t, point, self.normal, ray, self.material))
    }

    fn bounds(&self) -> AABB {
        self.bounds()
    }
}

/// The path tracer renderer
pub struct Renderer {
    width: usize,
    height: usize,
    framebuffer: Vec<Vector3<f32>>,
    camera: Camera,
    samples_per_pixel: u32,
}

impl Renderer {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            framebuffer: vec![Vector3::zeros(); width * height],
            camera: Camera::default(),
            samples_per_pixel: 1,
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.framebuffer = vec![Vector3::zeros(); width * height];
        self.camera.set_aspect_ratio(width as f32 / height as f32);
    }

    pub fn set_camera(&mut self, camera: Camera) {
        self.camera = camera;
        self.camera.set_aspect_ratio(self.width as f32 / self.height as f32);
    }

    pub fn get_camera(&self) -> &Camera {
        &self.camera
    }

    pub fn get_camera_mut(&mut self) -> &mut Camera {
        &mut self.camera
    }

    /// Render the scene to the framebuffer (parallel version)
    pub fn render(&mut self, scene: &Scene) {
        let max_bounces = scene.max_bounces;
        let width = self.width;
        let height = self.height;
        let samples = self.samples_per_pixel;

        // Pre-compute camera data for parallel access
        let char_aspect = 0.5;
        let aspect_ratio = (width as f32 / height as f32) * char_aspect;
        let fov_rad = self.camera.fov.to_radians();
        let half_height = (fov_rad / 2.0).tan();
        let half_width = aspect_ratio * half_height;

        // Camera basis vectors
        let cam_origin = self.camera.origin;
        let w = (self.camera.origin - self.camera.look_at).normalize();
        let u = self.camera.up.cross(&w).normalize();
        let v = w.cross(&u);

        // Render in parallel using rayon
        let colors: Vec<Vector3<f32>> = (0..height)
            .into_par_iter()
            .flat_map(|y| {
                let scene = &scene;
                (0..width).into_par_iter().map(move |x| {
                    // Create deterministic RNG seeded by pixel position
                    let seed = (y * width + x) as u32;
                    let mut rng = SeededRng::new(seed.wrapping_add(12345));

                    let mut color = Vector3::zeros();

                    // Simple anti-aliasing with jittered samples
                    for _ in 0..samples {
                        let jitter = (rng.rand_float(), rng.rand_float());

                        // Generate ray (inlined from get_ray)
                        let px = (2.0 * ((x as f32 + jitter.0) / width as f32) - 1.0) * half_width;
                        let py = (1.0 - 2.0 * ((y as f32 + jitter.1) / height as f32)) * half_height;
                        let direction = (u * px + v * py - w).normalize();
                        let ray = Ray::new(cam_origin, direction);

                        color += trace_ray_standalone(&ray, scene, max_bounces, &mut rng);
                    }

                    color /= samples as f32;

                    // Gamma correction
                    color.x = color.x.sqrt().min(1.0);
                    color.y = color.y.sqrt().min(1.0);
                    color.z = color.z.sqrt().min(1.0);

                    color
                })
            })
            .collect();

        self.framebuffer = colors;
    }

    /// Convert framebuffer to ASCII string (grayscale)
    pub fn to_ascii(&self) -> String {
        let gradient = crate::ASCII_GRADIENT;
        let gradient_chars: Vec<char> = gradient.chars().collect();
        let mut result = String::with_capacity(self.width * self.height + self.height);

        for y in 0..self.height {
            for x in 0..self.width {
                let color = self.framebuffer[y * self.width + x];

                // Convert to luminance
                let luminance = 0.299 * color.x + 0.587 * color.y + 0.114 * color.z;
                let luminance = luminance.clamp(0.0, 1.0);

                // Map to character
                let index = ((luminance * (gradient_chars.len() - 1) as f32).round() as usize)
                    .min(gradient_chars.len() - 1);

                result.push(gradient_chars[index]);
            }
            result.push('\n');
        }

        result
    }

    /// Convert framebuffer to colored ASCII string with ANSI 24-bit color codes
    pub fn to_ascii_colored(&self) -> String {
        let gradient = crate::ASCII_GRADIENT;
        let gradient_chars: Vec<char> = gradient.chars().collect();
        // Pre-calculate buffer size (chars + ANSI codes + newlines + resets)
        let estimated_size = self.width * self.height * 20 + self.height * 10;
        let mut result = String::with_capacity(estimated_size);

        for y in 0..self.height {
            for x in 0..self.width {
                let color = self.framebuffer[y * self.width + x];

                // Clamp color values
                let r = (color.x.clamp(0.0, 1.0) * 255.0) as u8;
                let g = (color.y.clamp(0.0, 1.0) * 255.0) as u8;
                let b = (color.z.clamp(0.0, 1.0) * 255.0) as u8;

                // Convert to luminance for character selection
                let luminance = 0.299 * color.x + 0.587 * color.y + 0.114 * color.z;
                let luminance = luminance.clamp(0.0, 1.0);

                // Map to character
                let index = ((luminance * (gradient_chars.len() - 1) as f32).round() as usize)
                    .min(gradient_chars.len() - 1);

                // Write ANSI 24-bit color code: \x1b[38;2;R;G;Bm
                result.push_str(&format!("\x1b[38;2;{};{};{}m{}", r, g, b, gradient_chars[index]));
            }
            // Reset color at end of line and add newline
            result.push_str("\x1b[0m\n");
        }

        result
    }

    /// Convert RGB (0-255) to 256-color palette index
    fn rgb_to_256color(r: u8, g: u8, b: u8) -> u8 {
        // Use 6x6x6 color cube (colors 16-231)
        // Each component is mapped to 0-5 range
        let r6 = (r as u16 * 6 / 256) as u8;
        let g6 = (g as u16 * 6 / 256) as u8;
        let b6 = (b as u16 * 6 / 256) as u8;
        16 + 36 * r6 + 6 * g6 + b6
    }

    /// Convert framebuffer to half-block ASCII with 2x vertical resolution
    /// Uses ▀ (U+2580) to encode two vertical pixels per character
    /// Upper pixel = foreground color, Lower pixel = background color
    pub fn to_ascii_halfblock(&self) -> String {
        // Apply Floyd-Steinberg dithering to the framebuffer colors
        let mut dithered = self.framebuffer.clone();
        self.apply_dithering_to_colors(&mut dithered);

        // Output height is half (pairs of rows)
        let output_height = (self.height + 1) / 2;
        let estimated_size = self.width * output_height * 15;
        let mut result = String::with_capacity(estimated_size);

        // Track last colors for ANSI caching (using 256-color indices)
        let mut last_fg: Option<u8> = None;
        let mut last_bg: Option<u8> = None;

        for y in 0..output_height {
            let top_y = y * 2;
            let bottom_y = y * 2 + 1;

            for x in 0..self.width {
                // Get top pixel color (from dithered buffer)
                let top_idx = top_y * self.width + x;
                let top_color = dithered[top_idx];
                let top_r = (top_color.x.clamp(0.0, 1.0) * 255.0) as u8;
                let top_g = (top_color.y.clamp(0.0, 1.0) * 255.0) as u8;
                let top_b = (top_color.z.clamp(0.0, 1.0) * 255.0) as u8;
                let top_idx_256 = Self::rgb_to_256color(top_r, top_g, top_b);

                // Get bottom pixel color (or black if out of bounds)
                let bottom_idx_256 = if bottom_y < self.height {
                    let bottom_idx = bottom_y * self.width + x;
                    let bottom_color = dithered[bottom_idx];
                    let bottom_r = (bottom_color.x.clamp(0.0, 1.0) * 255.0) as u8;
                    let bottom_g = (bottom_color.y.clamp(0.0, 1.0) * 255.0) as u8;
                    let bottom_b = (bottom_color.z.clamp(0.0, 1.0) * 255.0) as u8;
                    Self::rgb_to_256color(bottom_r, bottom_g, bottom_b)
                } else {
                    16  // Black in 256-color palette
                };

                // Check which colors need updating
                let fg_changed = last_fg != Some(top_idx_256);
                let bg_changed = last_bg != Some(bottom_idx_256);

                // Combine ANSI codes when both colors change (saves bytes)
                if fg_changed && bg_changed {
                    result.push_str(&format!("\x1b[38;5;{};48;5;{}m", top_idx_256, bottom_idx_256));
                    last_fg = Some(top_idx_256);
                    last_bg = Some(bottom_idx_256);
                } else if fg_changed {
                    result.push_str(&format!("\x1b[38;5;{}m", top_idx_256));
                    last_fg = Some(top_idx_256);
                } else if bg_changed {
                    result.push_str(&format!("\x1b[48;5;{}m", bottom_idx_256));
                    last_bg = Some(bottom_idx_256);
                }

                // Use upper half-block (▀) - foreground shows top, background shows bottom
                result.push('\u{2580}');
            }

            // Don't reset - let colors carry across lines
            result.push('\n');
            // Keep last_fg and last_bg for next line
        }

        // Reset colors once at the very end
        result.push_str("\x1b[0m");

        result
    }

    /// Apply Floyd-Steinberg dithering to RGB colors (per-channel)
    fn apply_dithering_to_colors(&self, colors: &mut [Vector3<f32>]) {
        let width = self.width;

        for y in 0..self.height {
            for x in 0..width {
                let idx = y * width + x;

                // Quantize each channel
                for c in 0..3 {
                    let old_val = colors[idx][c];
                    let new_val = (old_val * 255.0).round() / 255.0;
                    colors[idx][c] = new_val;

                    let error = old_val - new_val;

                    // Distribute error to neighbors
                    // Right: 7/16
                    if x + 1 < width {
                        colors[idx + 1][c] += error * 7.0 / 16.0;
                    }
                    // Bottom-left: 3/16
                    if y + 1 < self.height && x > 0 {
                        colors[idx + width - 1][c] += error * 3.0 / 16.0;
                    }
                    // Bottom: 5/16
                    if y + 1 < self.height {
                        colors[idx + width][c] += error * 5.0 / 16.0;
                    }
                    // Bottom-right: 1/16
                    if y + 1 < self.height && x + 1 < width {
                        colors[idx + width + 1][c] += error * 1.0 / 16.0;
                    }
                }
            }
        }
    }
}

/// Reflect a vector around a normal
fn reflect(v: &Vector3<f32>, n: &Vector3<f32>) -> Vector3<f32> {
    *v - 2.0 * v.dot(n) * *n
}

/// Refract a vector through a surface
fn refract(uv: &Vector3<f32>, n: &Vector3<f32>, etai_over_etat: f32) -> Vector3<f32> {
    let cos_theta = (-uv.dot(n)).min(1.0);
    let r_out_perp = etai_over_etat * (*uv + *n * cos_theta);
    let r_out_parallel = -((1.0 - r_out_perp.magnitude_squared()).abs().sqrt()) * *n;
    r_out_perp + r_out_parallel
}

/// Schlick's approximation for reflectance
fn reflectance(cosine: f32, ref_idx: f32) -> f32 {
    let mut r0 = (1.0 - ref_idx) / (1.0 + ref_idx);
    r0 = r0 * r0;
    r0 + (1.0 - r0) * (1.0 - cosine).powi(5)
}

/// Seedable random float generator for parallel rendering
struct SeededRng {
    state: u32,
}

impl SeededRng {
    fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    fn rand_float(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(1103515245).wrapping_add(12345);
        (self.state % 10000) as f32 / 10000.0
    }
}

/// Random point in unit sphere using provided RNG
fn random_in_unit_sphere(rng: &mut SeededRng) -> Vector3<f32> {
    Vector3::new(
        rng.rand_float() * 2.0 - 1.0,
        rng.rand_float() * 2.0 - 1.0,
        rng.rand_float() * 2.0 - 1.0,
    ).normalize() * rng.rand_float().powf(1.0/3.0)
}

/// Standalone scene intersection function for parallel rendering
fn intersect_scene_standalone(ray: &Ray, t_min: f32, t_max: f32, scene: &Scene) -> Option<HitRecord> {
    let mut closest: Option<HitRecord> = None;
    let mut max_t = t_max;

    // Check spheres
    for sphere in &scene.spheres {
        if let Some(hit) = sphere.hit(ray, t_min, max_t) {
            max_t = hit.t;
            closest = Some(hit);
        }
    }

    // Check quads
    for quad in &scene.quads {
        if let Some(hit) = quad.hit(ray, t_min, max_t) {
            max_t = hit.t;
            closest = Some(hit);
        }
    }

    closest
}

/// Standalone shading function for parallel rendering
fn shade_point_standalone(point: Point3<f32>, normal: Vector3<f32>, scene: &Scene) -> f32 {
    let light_dir = (scene.light_position - point).normalize();
    let shadow_ray = Ray::new(point + normal * 0.001, light_dir);
    let light_distance = (scene.light_position - point).magnitude();

    // Check for shadows
    if intersect_scene_standalone(&shadow_ray, 0.001, light_distance, scene).is_some() {
        return 0.1; // In shadow
    }

    // Lambertian diffuse
    let diffuse = normal.dot(&light_dir).max(0.0);
    diffuse * scene.light_intensity * 0.1
}

/// Standalone ray tracing function for parallel rendering
fn trace_ray_standalone(ray: &Ray, scene: &Scene, depth: u32, rng: &mut SeededRng) -> Vector3<f32> {
    if depth == 0 {
        return Vector3::zeros();
    }

    if let Some(hit) = intersect_scene_standalone(ray, 0.001, f32::INFINITY, scene) {
        match hit.material {
            Material::Diffuse { albedo } => {
                let light_intensity = shade_point_standalone(hit.point, hit.normal, scene);
                let base_color = albedo * light_intensity;

                // Simple ambient occlusion approximation
                let ambient = albedo * 0.1;
                ambient + base_color
            }
            Material::Metal { albedo, roughness } => {
                let reflected = reflect(&ray.direction, &hit.normal);
                let scattered = Ray::new(hit.point, reflected + random_in_unit_sphere(rng) * roughness);

                if scattered.direction.dot(&hit.normal) > 0.0 {
                    let metallic_color = trace_ray_standalone(&scattered, scene, depth - 1, rng);
                    albedo.component_mul(&metallic_color) * 0.8
                } else {
                    Vector3::zeros()
                }
            }
            Material::Dielectric { refraction_index } => {
                let attenuation = Vector3::new(1.0, 1.0, 1.0);
                let etai_over_etat = if hit.front_face { 1.0 / refraction_index } else { refraction_index };

                let unit_direction = ray.direction.normalize();
                let cos_theta = (-unit_direction.dot(&hit.normal)).min(1.0);
                let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();

                let cannot_refract = etai_over_etat * sin_theta > 1.0;
                let direction: Vector3<f32>;

                if cannot_refract || reflectance(cos_theta, etai_over_etat) > rng.rand_float() {
                    direction = reflect(&unit_direction, &hit.normal);
                } else {
                    direction = refract(&unit_direction, &hit.normal, etai_over_etat);
                }

                let scattered = Ray::new(hit.point, direction);
                let refracted_color = trace_ray_standalone(&scattered, scene, depth - 1, rng);
                attenuation.component_mul(&refracted_color) * 0.9
            }
            Material::Emissive { color, intensity } => {
                color * intensity
            }
        }
    } else {
        // Background - dark gradient
        let t = 0.5 * (ray.direction.y + 1.0);
        Vector3::new(0.02, 0.02, 0.05) * (1.0 - t) + Vector3::new(0.05, 0.05, 0.1) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ray_creation() {
        let ray = Ray::new(Point3::origin(), Vector3::new(1.0, 0.0, 0.0));
        assert!((ray.at(5.0).x - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_sphere_intersection() {
        let sphere = Sphere::new(
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Material::Diffuse { albedo: Vector3::new(1.0, 1.0, 1.0) }
        );
        let ray = Ray::new(Point3::new(0.0, 0.0, 3.0), Vector3::new(0.0, 0.0, -1.0));

        let hit = sphere.hit(&ray, 0.0, 100.0);
        assert!(hit.is_some());
        let hit = hit.unwrap();
        assert!((hit.t - 2.0).abs() < 0.001);
    }

    #[test]
    fn test_renderer_creation() {
        let renderer = Renderer::new(80, 24);
        assert_eq!(renderer.width, 80);
        assert_eq!(renderer.height, 24);
    }

    #[test]
    fn test_renderer_resize() {
        let mut renderer = Renderer::new(80, 24);
        renderer.resize(100, 30);
        assert_eq!(renderer.width, 100);
        assert_eq!(renderer.height, 30);
    }

    #[test]
    fn test_reflect() {
        let v = Vector3::new(1.0, -1.0, 0.0);
        let n = Vector3::new(0.0, 1.0, 0.0);
        let r = reflect(&v, &n);
        assert!((r.x - 1.0).abs() < 0.001);
        assert!((r.y - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_reflectance() {
        let r = reflectance(0.5, 1.5);
        assert!(r >= 0.0 && r <= 1.0);
    }

    #[test]
    fn test_renderer_render() {
        let mut renderer = Renderer::new(20, 10);
        let scene = Scene::cornell_box();
        renderer.render(&scene);
        let ascii = renderer.to_ascii();
        assert!(!ascii.is_empty());
        assert!(ascii.contains('\n'));
    }
}
