//! Image rendering - composites Earth, stars, planets, and moon

use crate::astronomy::{Moon, PlanetarySystem, StarCatalog};
use anyhow::Result;
use chrono::{DateTime, Utc};
use image::{Rgba, RgbaImage};

/// Field of view in degrees for the star field
const DEFAULT_FOV: f64 = 120.0;

/// Maximum star magnitude to render
const MAX_STAR_MAGNITUDE: f64 = 6.5;

pub struct Renderer {
    star_catalog: StarCatalog,
    planetary_system: PlanetarySystem,
    moon: Moon,
    fov: f64,
}

impl Renderer {
    pub fn new() -> Self {
        let mut star_catalog = StarCatalog::new(MAX_STAR_MAGNITUDE);
        star_catalog.load_embedded();

        Self {
            star_catalog,
            planetary_system: PlanetarySystem::new(),
            moon: Moon::new(),
            fov: DEFAULT_FOV,
        }
    }

    /// Render complete wallpaper with Earth centered
    pub fn render(
        &mut self,
        earth_image: &RgbaImage,
        width: u32,
        height: u32,
        timestamp: &DateTime<Utc>,
    ) -> Result<RgbaImage> {
        let mut canvas = RgbaImage::new(width, height);

        // Fill with black background
        for pixel in canvas.pixels_mut() {
            *pixel = Rgba([0, 0, 0, 255]);
        }

        // Render stars first (background)
        self.render_stars(&mut canvas, timestamp);

        // Render planets
        self.render_planets(&mut canvas, timestamp);

        // Render moon
        self.render_moon(&mut canvas, timestamp);

        // Composite Earth on top, centered
        self.composite_earth(&mut canvas, earth_image);

        Ok(canvas)
    }

    fn render_stars(&self, canvas: &mut RgbaImage, dt: &DateTime<Utc>) {
        let width = canvas.width();
        let height = canvas.height();

        let visible = self.star_catalog.visible_stars(dt, width, height, self.fov);
        let count = visible.len();

        for (star, pos) in visible {
            let (r, g, b) = star.color();
            let radius = star.radius(1.5);
            
            draw_star(canvas, pos.x as i32, pos.y as i32, radius, r, g, b, star.magnitude);
        }

        tracing::debug!("Rendered {} visible stars", count);
    }

    fn render_planets(&self, canvas: &mut RgbaImage, dt: &DateTime<Utc>) {
        let width = canvas.width();
        let height = canvas.height();

        let visible = self.planetary_system.visible_planets(
            dt, width, height, self.fov, MAX_STAR_MAGNITUDE
        );

        for (planet, _eq, pos, mag) in visible {
            let (r, g, b) = planet.color;
            let radius = magnitude_to_radius(mag, 2.0);
            
            draw_planet(canvas, pos.x as i32, pos.y as i32, radius, r, g, b);
            
            tracing::debug!(
                "Rendered {} at ({:.0}, {:.0}) mag {:.1}",
                planet.name, pos.x, pos.y, mag
            );
        }
    }

    fn render_moon(&mut self, canvas: &mut RgbaImage, dt: &DateTime<Utc>) {
        let width = canvas.width();
        let height = canvas.height();

        let pos = self.moon.screen_position(dt, width, height, self.fov);
        
        if pos.visible {
            let phase = self.moon.phase();
            let illumination = self.moon.illumination();
            let radius = 8.0; // Moon appears larger than stars
            
            draw_moon(canvas, pos.x as i32, pos.y as i32, radius, phase, illumination);
            
            tracing::debug!(
                "Rendered Moon at ({:.0}, {:.0}) phase: {} ({:.0}% illuminated)",
                pos.x, pos.y, self.moon.phase_name(), illumination * 100.0
            );
        }
    }

    fn composite_earth(&self, canvas: &mut RgbaImage, earth: &RgbaImage) {
        let canvas_w = canvas.width();
        let canvas_h = canvas.height();
        let earth_w = earth.width();
        let earth_h = earth.height();

        // Scale Earth to fit nicely (about 60% of smaller dimension)
        let scale = (canvas_w.min(canvas_h) as f64 * 0.6) / earth_w.max(earth_h) as f64;
        let scaled_w = (earth_w as f64 * scale) as u32;
        let scaled_h = (earth_h as f64 * scale) as u32;

        // Center position
        let offset_x = (canvas_w - scaled_w) / 2;
        let offset_y = (canvas_h - scaled_h) / 2;

        // Scale and composite
        let scaled_earth = image::imageops::resize(
            earth,
            scaled_w,
            scaled_h,
            image::imageops::FilterType::Lanczos3,
        );

        // Create circular mask for Earth
        let center_x = scaled_w as f64 / 2.0;
        let center_y = scaled_h as f64 / 2.0;
        let radius = scaled_w.min(scaled_h) as f64 / 2.0;

        for y in 0..scaled_h {
            for x in 0..scaled_w {
                let dx = x as f64 - center_x;
                let dy = y as f64 - center_y;
                let dist = (dx * dx + dy * dy).sqrt();

                if dist <= radius {
                    let src_pixel = scaled_earth.get_pixel(x, y);
                    let dst_x = offset_x + x;
                    let dst_y = offset_y + y;

                    if dst_x < canvas_w && dst_y < canvas_h {
                        // Smooth edge with anti-aliasing
                        let edge_dist = radius - dist;
                        let alpha = if edge_dist < 2.0 {
                            (edge_dist / 2.0 * 255.0) as u8
                        } else {
                            255
                        };

                        let dst_pixel = canvas.get_pixel_mut(dst_x, dst_y);
                        blend_pixel(dst_pixel, src_pixel, alpha);
                    }
                }
            }
        }
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Draw a star with glow effect
fn draw_star(canvas: &mut RgbaImage, cx: i32, cy: i32, radius: f64, r: u8, g: u8, b: u8, magnitude: f64) {
    let width = canvas.width() as i32;
    let height = canvas.height() as i32;
    
    // Brightness based on magnitude (brighter = lower magnitude)
    let brightness = ((6.0 - magnitude) / 7.0).clamp(0.3, 1.0);
    
    let ir = (radius * 2.0).ceil() as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < width && py >= 0 && py < height {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius * 2.0 {
                    // Gaussian falloff for glow
                    let intensity = (-(dist * dist) / (radius * radius)).exp() * brightness;
                    let alpha = (intensity * 255.0) as u8;
                    
                    if alpha > 0 {
                        let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                        blend_pixel(pixel, &Rgba([r, g, b, 255]), alpha);
                    }
                }
            }
        }
    }
}

/// Draw a planet (similar to star but slightly larger)
fn draw_planet(canvas: &mut RgbaImage, cx: i32, cy: i32, radius: f64, r: u8, g: u8, b: u8) {
    let width = canvas.width() as i32;
    let height = canvas.height() as i32;
    
    let ir = (radius * 1.5).ceil() as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < width && py >= 0 && py < height {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius {
                    // Solid core with soft edge
                    let edge = radius - dist;
                    let alpha = if edge < 1.0 { (edge * 255.0) as u8 } else { 255 };
                    
                    let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                    blend_pixel(pixel, &Rgba([r, g, b, 255]), alpha);
                }
            }
        }
    }
}

/// Draw moon with phase
/// 
/// We're viewing from Himawari-8's position (geostationary at 140.7°E), looking outward into space.
/// The sun illuminates the moon from the same direction it illuminates Earth.
/// 
/// Phase convention (from Earth's surface, northern hemisphere):
/// - Phase 0.0: New moon (between Earth and sun, dark side facing us)
/// - Phase 0.25: First quarter (right half lit when viewed from Earth's surface)
/// - Phase 0.5: Full moon (opposite side from sun, fully lit face toward Earth)
/// - Phase 0.75: Last quarter (left half lit when viewed from Earth's surface)
/// 
/// Since we're looking FROM the satellite toward the moon (same direction as from Earth),
/// the illumination direction follows the same pattern as seen from Earth.
fn draw_moon(canvas: &mut RgbaImage, cx: i32, cy: i32, radius: f64, phase: f64, illumination: f64) {
    let width = canvas.width() as i32;
    let height = canvas.height() as i32;
    
    let ir = radius.ceil() as i32;
    let moon_color = Rgba([240, 240, 230, 255]); // Slightly warm white
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < width && py >= 0 && py < height {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius {
                    // Normalized position on moon disk (-1 to 1)
                    let normalized_x = dx as f64 / radius;
                    
                    // Terminator position based on phase:
                    // At phase 0 (new): terminator at x=1 (nothing lit from our view)
                    // At phase 0.25 (first quarter): terminator at x=0 (right half lit)
                    // At phase 0.5 (full): terminator at x=-1 (all lit)
                    // At phase 0.75 (last quarter): terminator at x=0 (left half lit)
                    
                    let lit = if phase < 0.5 {
                        // Waxing: illumination grows from right side
                        // terminator moves from +1 toward -1
                        let terminator = 1.0 - phase * 4.0; // 1.0 at phase 0, -1.0 at phase 0.5
                        normalized_x > terminator
                    } else {
                        // Waning: illumination shrinks from right side  
                        // terminator moves from -1 toward +1
                        let terminator = (phase - 0.5) * 4.0 - 1.0; // -1.0 at phase 0.5, 1.0 at phase 1.0
                        normalized_x < terminator
                    };
                    
                    let brightness = if lit { 1.0 } else { 0.1 };
                    let edge = radius - dist;
                    let alpha = if edge < 1.0 { (edge * 255.0 * brightness) as u8 } else { (255.0 * brightness) as u8 };
                    
                    if alpha > 0 {
                        let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                        blend_pixel(pixel, &moon_color, alpha);
                    }
                }
            }
        }
    }
}

/// Convert magnitude to display radius
fn magnitude_to_radius(mag: f64, base: f64) -> f64 {
    let factor = (4.0 - mag).max(0.5);
    base * factor.powf(0.4)
}

/// Alpha blend a source pixel onto destination
fn blend_pixel(dst: &mut Rgba<u8>, src: &Rgba<u8>, alpha: u8) {
    let a = alpha as f32 / 255.0;
    let inv_a = 1.0 - a;
    
    dst[0] = (src[0] as f32 * a + dst[0] as f32 * inv_a) as u8;
    dst[1] = (src[1] as f32 * a + dst[1] as f32 * inv_a) as u8;
    dst[2] = (src[2] as f32 * a + dst[2] as f32 * inv_a) as u8;
    dst[3] = 255;
}
