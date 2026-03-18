//! Image rendering - composites Earth, stars, planets, and moon

use crate::astronomy::{Moon, PlanetarySystem, StarCatalog};
use crate::astronomy::coordinates::{SATELLITE_ALTITUDE_KM, EARTH_RADIUS_KM};
use crate::monitor::{MonitorLayout, MultiMonitorMode};
use crate::moon_texture::MOON_TEXTURE_PNG;
use anyhow::Result;
use chrono::{DateTime, Utc};
use image::{Rgba, RgbaImage};

/// Fraction of the smaller dimension that Earth should occupy
const EARTH_SCREEN_FRACTION: f64 = 0.6;

/// Calculate the actual FOV that shows Earth at the correct angular size
/// From geostationary orbit, Earth subtends ~17.4° 
fn calculate_earth_angular_diameter() -> f64 {
    let distance_to_earth_center = SATELLITE_ALTITUDE_KM + EARTH_RADIUS_KM;
    let earth_angular_radius = (EARTH_RADIUS_KM / distance_to_earth_center).asin();
    earth_angular_radius.to_degrees() * 2.0
}

/// Maximum star magnitude to render
const MAX_STAR_MAGNITUDE: f64 = 7.5;

pub struct Renderer {
    star_catalog: StarCatalog,
    planetary_system: PlanetarySystem,
    moon: Moon,
    moon_texture: Option<RgbaImage>,
    show_labels: bool,
}

impl Renderer {
    pub fn new() -> Self {
        let mut star_catalog = StarCatalog::new(MAX_STAR_MAGNITUDE);
        star_catalog.load_embedded();
        
        // Load moon texture
        let moon_texture = image::load_from_memory(MOON_TEXTURE_PNG)
            .ok()
            .map(|img| img.to_rgba8());
        
        if moon_texture.is_some() {
            tracing::debug!("Loaded moon texture");
        }
        
        Self {
            star_catalog,
            planetary_system: PlanetarySystem::new(),
            moon: Moon::new(),
            moon_texture,
            show_labels: false,
        }
    }

    /// Enable or disable object labels
    pub fn set_show_labels(&mut self, show: bool) {
        self.show_labels = show;
    }

    /// Render wallpaper for the given monitor layout and mode
    pub fn render(
        &mut self,
        earth_image: &RgbaImage,
        layout: &MonitorLayout,
        mode: MultiMonitorMode,
        timestamp: &DateTime<Utc>,
    ) -> Result<RgbaImage> {
        match mode {
            MultiMonitorMode::Span => self.render_span(earth_image, layout, timestamp),
            MultiMonitorMode::Duplicate => self.render_duplicate(earth_image, layout, timestamp),
        }
    }

    /// Render a single image spanning all monitors
    /// Earth is centered on the virtual desktop, stars extend across all monitors
    fn render_span(
        &mut self,
        earth_image: &RgbaImage,
        layout: &MonitorLayout,
        timestamp: &DateTime<Utc>,
    ) -> Result<RgbaImage> {
        let width = layout.total_width;
        let height = layout.total_height;
        
        // For span mode, use the height as the reference for FOV
        // This ensures consistent vertical FOV regardless of how wide the setup is
        let earth_angular_diameter = calculate_earth_angular_diameter();
        let fov = earth_angular_diameter / EARTH_SCREEN_FRACTION;
        
        let mut canvas = RgbaImage::new(width, height);
        
        // Fill with black
        for pixel in canvas.pixels_mut() {
            *pixel = Rgba([0, 0, 0, 255]);
        }

        // Render celestial objects - use height for vertical FOV, width scales horizontally
        self.render_stars_viewport(&mut canvas, timestamp, fov, 0, 0, width, height);
        self.render_planets_viewport(&mut canvas, timestamp, fov, 0, 0, width, height);
        self.render_moon_viewport(&mut canvas, timestamp, fov, 0, 0, width, height);

        // Composite Earth centered on the canvas
        self.composite_earth_at(&mut canvas, earth_image, width / 2, height / 2, height);

        Ok(canvas)
    }

    /// Render with Earth duplicated/centered on each monitor
    fn render_duplicate(
        &mut self,
        earth_image: &RgbaImage,
        layout: &MonitorLayout,
        timestamp: &DateTime<Utc>,
    ) -> Result<RgbaImage> {
        let width = layout.total_width;
        let height = layout.total_height;
        let (min_x, min_y, _, _) = layout.bounds;
        
        let mut canvas = RgbaImage::new(width, height);
        
        // Fill with black
        for pixel in canvas.pixels_mut() {
            *pixel = Rgba([0, 0, 0, 255]);
        }

        // Render each monitor independently
        for monitor in &layout.monitors {
            // Calculate monitor's position in the canvas (offset from bounds origin)
            let canvas_x = (monitor.x - min_x) as u32;
            let canvas_y = (monitor.y - min_y) as u32;
            
            // FOV based on this monitor's dimensions
            let earth_angular_diameter = calculate_earth_angular_diameter();
            let fov = earth_angular_diameter / EARTH_SCREEN_FRACTION;
            
            // Render stars for this monitor's viewport
            self.render_stars_viewport(
                &mut canvas, timestamp, fov,
                canvas_x, canvas_y, monitor.width, monitor.height
            );
            self.render_planets_viewport(
                &mut canvas, timestamp, fov,
                canvas_x, canvas_y, monitor.width, monitor.height
            );
            self.render_moon_viewport(
                &mut canvas, timestamp, fov,
                canvas_x, canvas_y, monitor.width, monitor.height
            );
            
            // Earth centered on this monitor
            let earth_center_x = canvas_x + monitor.width / 2;
            let earth_center_y = canvas_y + monitor.height / 2;
            self.composite_earth_at(
                &mut canvas, earth_image,
                earth_center_x, earth_center_y,
                monitor.height
            );
        }

        Ok(canvas)
    }

    /// Render stars into a viewport region of the canvas
    fn render_stars_viewport(
        &self,
        canvas: &mut RgbaImage,
        dt: &DateTime<Utc>,
        fov: f64,
        vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
    ) {
        // Get visible stars for this viewport
        let visible = self.star_catalog.visible_stars(dt, vp_w, vp_h, fov);

        for (star, pos) in visible {
            let (r, g, b) = star.color();
            let radius = star.radius(1.5);
            
            // Offset position to viewport location in canvas
            let cx = vp_x as i32 + pos.x as i32;
            let cy = vp_y as i32 + pos.y as i32;
            
            draw_star_bounded(canvas, cx, cy, radius, r, g, b, star.magnitude,
                              vp_x, vp_y, vp_w, vp_h);
            
            // Draw label for stars
            if self.show_labels {
                if let Some(ref label) = star.name {
                    // Proper names (no digits) shown brighter than Bayer/Flamsteed designations
                    let is_proper_name = !label.chars().any(|c| c.is_ascii_digit());
                    let brightness = if is_proper_name { 180 } else { 140 };
                    draw_label(canvas, cx + 6, cy - 2, label, brightness, vp_x, vp_y, vp_w, vp_h);
                } else {
                    // Unnamed stars show magnitude
                    let mag_label = format!("{:.1}", star.magnitude);
                    draw_label(canvas, cx + 6, cy - 2, &mag_label, 120, vp_x, vp_y, vp_w, vp_h);
                }
            }
        }
    }

    /// Render planets into a viewport region
    fn render_planets_viewport(
        &self,
        canvas: &mut RgbaImage,
        dt: &DateTime<Utc>,
        fov: f64,
        vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
    ) {
        let visible = self.planetary_system.visible_planets(dt, vp_w, vp_h, fov, MAX_STAR_MAGNITUDE);

        for (planet, _eq, pos, mag) in visible {
            let (r, g, b) = planet.color;
            let radius = magnitude_to_radius(mag, 2.0);
            
            let cx = vp_x as i32 + pos.x as i32;
            let cy = vp_y as i32 + pos.y as i32;
            
            draw_planet_bounded(canvas, cx, cy, radius, r, g, b, vp_x, vp_y, vp_w, vp_h);
            
            // Draw planet label
            if self.show_labels {
                draw_label(canvas, cx + 8, cy - 2, planet.name, 220, vp_x, vp_y, vp_w, vp_h);
            }
        }
    }

    /// Render moon into a viewport region
    fn render_moon_viewport(
        &mut self,
        canvas: &mut RgbaImage,
        dt: &DateTime<Utc>,
        fov: f64,
        vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
    ) {
        let pos = self.moon.screen_position(dt, vp_w, vp_h, fov);
        
        if pos.visible {
            let phase = self.moon.phase();
            let radius = 8.0;
            
            let cx = vp_x as i32 + pos.x as i32;
            let cy = vp_y as i32 + pos.y as i32;
            
            if let Some(ref texture) = self.moon_texture {
                draw_moon_textured(canvas, cx, cy, radius, phase, texture, vp_x, vp_y, vp_w, vp_h);
            } else {
                draw_moon_bounded(canvas, cx, cy, radius, phase, vp_x, vp_y, vp_w, vp_h);
            }
            
            // Draw moon label
            if self.show_labels {
                draw_label(canvas, cx + 12, cy - 2, "Moon", 220, vp_x, vp_y, vp_w, vp_h);
            }
        }
    }

    /// Composite Earth at a specific center position
    fn composite_earth_at(
        &self,
        canvas: &mut RgbaImage,
        earth: &RgbaImage,
        center_x: u32,
        center_y: u32,
        reference_size: u32, // Usually the viewport height
    ) {
        let canvas_w = canvas.width();
        let canvas_h = canvas.height();
        let earth_w = earth.width();
        let earth_h = earth.height();

        // Scale Earth based on reference size (typically viewport height)
        let scale = (reference_size as f64 * EARTH_SCREEN_FRACTION) / earth_w.max(earth_h) as f64;
        let scaled_w = (earth_w as f64 * scale) as u32;
        let scaled_h = (earth_h as f64 * scale) as u32;

        // Position relative to center
        let offset_x = center_x.saturating_sub(scaled_w / 2);
        let offset_y = center_y.saturating_sub(scaled_h / 2);

        // Scale the earth image
        let scaled_earth = image::imageops::resize(
            earth,
            scaled_w,
            scaled_h,
            image::imageops::FilterType::Lanczos3,
        );

        // Create circular mask for Earth
        let earth_center_x = scaled_w as f64 / 2.0;
        let earth_center_y = scaled_h as f64 / 2.0;
        let radius = scaled_w.min(scaled_h) as f64 / 2.0;

        for y in 0..scaled_h {
            for x in 0..scaled_w {
                let dx = x as f64 - earth_center_x;
                let dy = y as f64 - earth_center_y;
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

/// Draw a star with bounds checking for viewport
fn draw_star_bounded(
    canvas: &mut RgbaImage,
    cx: i32, cy: i32,
    radius: f64,
    r: u8, g: u8, b: u8,
    magnitude: f64,
    vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
) {
    let canvas_w = canvas.width() as i32;
    let canvas_h = canvas.height() as i32;
    
    // Pogson's ratio: each magnitude step is 2.512× dimmer
    // Reference point (5.75) chosen so mag 7.5 ≈ 20% brightness
    // This preserves accurate relative brightness while scaling for screen visibility
    let brightness = (2.512_f64).powf(5.75 - magnitude).clamp(0.0, 1.0);
    let ir = (radius * 2.0).ceil() as i32;
    
    // Viewport bounds in canvas coordinates
    let vp_left = vp_x as i32;
    let vp_top = vp_y as i32;
    let vp_right = vp_left + vp_w as i32;
    let vp_bottom = vp_top + vp_h as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            // Check both canvas bounds and viewport bounds
            if px >= 0 && px < canvas_w && py >= 0 && py < canvas_h
               && px >= vp_left && px < vp_right && py >= vp_top && py < vp_bottom
            {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius * 2.0 {
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

/// Draw a planet with bounds checking
fn draw_planet_bounded(
    canvas: &mut RgbaImage,
    cx: i32, cy: i32,
    radius: f64,
    r: u8, g: u8, b: u8,
    vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
) {
    let canvas_w = canvas.width() as i32;
    let canvas_h = canvas.height() as i32;
    
    // Extend rendering area for glow effect
    let glow_radius = radius * 2.5;
    let ir = glow_radius.ceil() as i32;
    
    let vp_left = vp_x as i32;
    let vp_top = vp_y as i32;
    let vp_right = vp_left + vp_w as i32;
    let vp_bottom = vp_top + vp_h as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < canvas_w && py >= 0 && py < canvas_h
               && px >= vp_left && px < vp_right && py >= vp_top && py < vp_bottom
            {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                let alpha = if dist <= radius {
                    // Core: solid with anti-aliased edge
                    let edge = radius - dist;
                    if edge < 1.0 { (edge * 255.0) as u8 } else { 255 }
                } else if dist <= glow_radius {
                    // Glow: exponential falloff beyond core
                    let glow_dist = (dist - radius) / (glow_radius - radius);
                    let glow_intensity = (1.0 - glow_dist).powi(2) * 0.4;
                    (glow_intensity * 255.0) as u8
                } else {
                    0
                };
                
                if alpha > 0 {
                    let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                    blend_pixel(pixel, &Rgba([r, g, b, 255]), alpha);
                }
            }
        }
    }
}

/// Draw moon with texture and phase shading
fn draw_moon_textured(
    canvas: &mut RgbaImage,
    cx: i32, cy: i32,
    radius: f64,
    phase: f64,
    texture: &RgbaImage,
    vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
) {
    let canvas_w = canvas.width() as i32;
    let canvas_h = canvas.height() as i32;
    let ir = radius.ceil() as i32;
    let tex_w = texture.width() as f64;
    let tex_h = texture.height() as f64;
    
    let vp_left = vp_x as i32;
    let vp_top = vp_y as i32;
    let vp_right = vp_left + vp_w as i32;
    let vp_bottom = vp_top + vp_h as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < canvas_w && py >= 0 && py < canvas_h
               && px >= vp_left && px < vp_right && py >= vp_top && py < vp_bottom
            {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius {
                    // Map screen position to sphere surface
                    let nx = dx as f64 / radius; // -1 to 1
                    let ny = dy as f64 / radius; // -1 to 1
                    
                    // z coordinate on unit sphere (front face)
                    let nz_sq = 1.0 - nx * nx - ny * ny;
                    if nz_sq < 0.0 { continue; }
                    let nz = nz_sq.sqrt();
                    
                    // Convert to spherical coordinates for texture lookup
                    // Latitude: -90 to 90 from top to bottom
                    // Longitude: 0 to 360 around equator
                    let lat = ny.asin(); // -π/2 to π/2
                    let lon = nx.atan2(nz); // -π to π (centered on front)
                    
                    // Map to texture coordinates
                    // Texture is equirectangular: lon maps to x (0-360), lat maps to y (90 to -90)
                    let u = (lon + std::f64::consts::PI) / (2.0 * std::f64::consts::PI); // 0 to 1
                    let v = 0.5 - lat / std::f64::consts::PI; // 0 to 1 (top to bottom)
                    
                    let tex_x = ((u * tex_w) as u32).min(texture.width() - 1);
                    let tex_y = ((v * tex_h) as u32).min(texture.height() - 1);
                    
                    let tex_pixel = texture.get_pixel(tex_x, tex_y);
                    
                    // Apply phase shading (terminator)
                    // nx is the normalized x position (-1 left to +1 right)
                    let lit = if phase < 0.5 {
                        // Waxing: right side lit first, terminator moves left
                        let terminator = 1.0 - phase * 4.0; // 1 to -1
                        nx > terminator
                    } else {
                        // Waning: left side stays lit, terminator moves right
                        let terminator = (phase - 0.5) * 4.0 - 1.0; // -1 to 1
                        nx < terminator
                    };
                    
                    // Brightness based on illumination
                    let phase_brightness = if lit { 1.0 } else { 0.1 };
                    
                    // Apply edge falloff for anti-aliasing
                    let edge = radius - dist;
                    let edge_alpha = if edge < 1.0 { edge } else { 1.0 };
                    
                    let r = (tex_pixel[0] as f64 * phase_brightness * edge_alpha) as u8;
                    let g = (tex_pixel[1] as f64 * phase_brightness * edge_alpha) as u8;
                    let b = (tex_pixel[2] as f64 * phase_brightness * edge_alpha) as u8;
                    let a = (255.0 * edge_alpha) as u8;
                    
                    if a > 0 {
                        let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                        blend_pixel(pixel, &Rgba([r, g, b, 255]), a);
                    }
                }
            }
        }
    }
}

/// Draw moon with phase and bounds checking (fallback without texture)
fn draw_moon_bounded(
    canvas: &mut RgbaImage,
    cx: i32, cy: i32,
    radius: f64,
    phase: f64,
    vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
) {
    let canvas_w = canvas.width() as i32;
    let canvas_h = canvas.height() as i32;
    let ir = radius.ceil() as i32;
    let moon_color = Rgba([240, 240, 230, 255]);
    
    let vp_left = vp_x as i32;
    let vp_top = vp_y as i32;
    let vp_right = vp_left + vp_w as i32;
    let vp_bottom = vp_top + vp_h as i32;
    
    for dy in -ir..=ir {
        for dx in -ir..=ir {
            let px = cx + dx;
            let py = cy + dy;
            
            if px >= 0 && px < canvas_w && py >= 0 && py < canvas_h
               && px >= vp_left && px < vp_right && py >= vp_top && py < vp_bottom
            {
                let dist = ((dx * dx + dy * dy) as f64).sqrt();
                
                if dist <= radius {
                    let normalized_x = dx as f64 / radius;
                    
                    let lit = if phase < 0.5 {
                        let terminator = 1.0 - phase * 4.0;
                        normalized_x > terminator
                    } else {
                        let terminator = (phase - 0.5) * 4.0 - 1.0;
                        normalized_x < terminator
                    };
                    
                    let brightness = if lit { 1.0 } else { 0.1 };
                    let edge = radius - dist;
                    let alpha = if edge < 1.0 { 
                        (edge * 255.0 * brightness) as u8 
                    } else { 
                        (255.0 * brightness) as u8 
                    };
                    
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

/// Simple 5x7 pixel font for labels (A-Z, a-z, space)
fn get_char_bitmap(c: char) -> Option<[u8; 7]> {
    match c.to_ascii_uppercase() {
        'A' => Some([0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
        'B' => Some([0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110]),
        'C' => Some([0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110]),
        'D' => Some([0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110]),
        'E' => Some([0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111]),
        'F' => Some([0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000]),
        'G' => Some([0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110]),
        'H' => Some([0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001]),
        'I' => Some([0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
        'J' => Some([0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100]),
        'K' => Some([0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001]),
        'L' => Some([0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111]),
        'M' => Some([0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001]),
        'N' => Some([0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001]),
        'O' => Some([0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
        'P' => Some([0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000]),
        'Q' => Some([0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101]),
        'R' => Some([0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001]),
        'S' => Some([0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110]),
        'T' => Some([0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100]),
        'U' => Some([0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110]),
        'V' => Some([0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100]),
        'W' => Some([0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001]),
        'X' => Some([0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001]),
        'Y' => Some([0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100]),
        'Z' => Some([0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111]),
        '0' => Some([0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110]),
        '1' => Some([0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110]),
        '2' => Some([0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111]),
        '3' => Some([0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110]),
        '4' => Some([0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010]),
        '5' => Some([0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110]),
        '6' => Some([0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110]),
        '7' => Some([0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000]),
        '8' => Some([0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110]),
        '9' => Some([0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110]),
        '.' => Some([0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100]),
        '-' => Some([0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000]),
        ' ' => Some([0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000]),
        _ => None,
    }
}

/// Draw a text label at the given position
fn draw_label(
    canvas: &mut RgbaImage,
    x: i32, y: i32,
    text: &str,
    brightness: u8,
    vp_x: u32, vp_y: u32, vp_w: u32, vp_h: u32,
) {
    let canvas_w = canvas.width() as i32;
    let canvas_h = canvas.height() as i32;
    let vp_left = vp_x as i32;
    let vp_top = vp_y as i32;
    let vp_right = vp_left + vp_w as i32;
    let vp_bottom = vp_top + vp_h as i32;
    
    let color = Rgba([brightness, brightness, brightness, 255]);
    let mut cursor_x = x;
    
    for c in text.chars() {
        if let Some(bitmap) = get_char_bitmap(c) {
            for (row, &bits) in bitmap.iter().enumerate() {
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 == 1 {
                        let px = cursor_x + col;
                        let py = y + row as i32;
                        
                        if px >= 0 && px < canvas_w && py >= 0 && py < canvas_h
                           && px >= vp_left && px < vp_right && py >= vp_top && py < vp_bottom
                        {
                            let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                            blend_pixel(pixel, &color, brightness);
                        }
                    }
                }
            }
        }
        cursor_x += 6; // 5px char width + 1px spacing
    }
}
