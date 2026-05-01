//! Star catalog and rendering
//! 
//! Uses a subset of the HYG (Hipparcos-Yale-Gliese) database for accurate star positions.

use super::coordinates::{Equatorial, ScreenPosition, equatorial_to_screen};
use chrono::{DateTime, Utc};

// Include the generated star data
include!("../star_data.rs");

#[derive(Debug, Clone)]
pub struct Star {
    pub ra: f64,         // Right Ascension in hours
    pub dec: f64,        // Declination in degrees
    pub magnitude: f64,  // Visual magnitude
    pub color_index: f64, // B-V color index
    pub name: Option<String>,
}

impl Star {
    pub fn new(ra: f64, dec: f64, magnitude: f64, color_index: f64, name: Option<String>) -> Self {
        Self { ra, dec, magnitude, color_index, name }
    }

    pub fn equatorial(&self) -> Equatorial {
        Equatorial::new(self.ra, self.dec)
    }

    /// Convert B-V color index to RGB color
    pub fn color(&self) -> (u8, u8, u8) {
        bv_to_rgb(self.color_index)
    }

    /// Calculate star radius in pixels based on magnitude
    pub fn radius(&self, base_size: f64) -> f64 {
        // Brighter stars (lower magnitude) are larger
        let mag_factor = (6.0 - self.magnitude).max(0.3);
        base_size * mag_factor.powf(0.5)
    }

    pub fn screen_position(
        &self,
        dt: &DateTime<Utc>,
        width: u32,
        height: u32,
        fov: f64,
    ) -> ScreenPosition {
        equatorial_to_screen(&self.equatorial(), dt, width, height, fov)
    }
}

/// Convert B-V color index to RGB
/// Based on approximation of black body radiation colors
fn bv_to_rgb(bv: f64) -> (u8, u8, u8) {
    let bv = bv.clamp(-0.4, 2.0);
    
    let (r, g, b) = if bv < 0.0 {
        // Blue-white stars (O, B type)
        let t = (bv + 0.4) / 0.4;
        (
            0.6 + 0.4 * t,
            0.7 + 0.3 * t,
            1.0,
        )
    } else if bv < 0.4 {
        // White stars (A type)
        let t = bv / 0.4;
        (
            1.0,
            1.0,
            1.0 - 0.1 * t,
        )
    } else if bv < 0.8 {
        // Yellow-white to yellow (F, G type)
        let t = (bv - 0.4) / 0.4;
        (
            1.0,
            1.0 - 0.15 * t,
            0.9 - 0.3 * t,
        )
    } else if bv < 1.2 {
        // Orange (K type)
        let t = (bv - 0.8) / 0.4;
        (
            1.0,
            0.85 - 0.25 * t,
            0.6 - 0.3 * t,
        )
    } else {
        // Red (M type)
        let t = ((bv - 1.2) / 0.8).min(1.0);
        (
            1.0 - 0.2 * t,
            0.6 - 0.3 * t,
            0.3 - 0.2 * t,
        )
    };

    (
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    )
}

pub struct StarCatalog {
    stars: Vec<Star>,
    max_magnitude: f64,
}

impl StarCatalog {
    pub fn new(max_magnitude: f64) -> Self {
        Self {
            stars: Vec::new(),
            max_magnitude,
        }
    }

    /// Load embedded star data
    pub fn load_embedded(&mut self) {
        self.stars = STAR_DATA
            .iter()
            .filter(|s| s.2 <= self.max_magnitude)
            .map(|&(ra, dec, mag, ci, name)| {
                Star::new(ra, dec, mag, ci, name.map(String::from))
            })
            .collect();
        
        tracing::debug!(
            "Loaded {} stars (mag <= {:.1}) from HYG catalog",
            self.stars.len(),
            self.max_magnitude
        );
    }

    pub fn visible_stars(
        &self,
        dt: &DateTime<Utc>,
        width: u32,
        height: u32,
        fov: f64,
    ) -> Vec<(&Star, ScreenPosition)> {
        self.stars
            .iter()
            .filter_map(|star| {
                let pos = star.screen_position(dt, width, height, fov);
                if pos.visible {
                    Some((star, pos))
                } else {
                    None
                }
            })
            .collect()
    }
}
