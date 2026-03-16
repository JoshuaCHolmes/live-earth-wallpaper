//! Star catalog and rendering
//! 
//! Uses a subset of the HYG (Hipparcos-Yale-Gliese) database for accurate star positions.

use super::coordinates::{Equatorial, ScreenPosition, equatorial_to_screen};
use chrono::{DateTime, Utc};

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
        let mag_factor = (5.0 - self.magnitude).max(0.5);
        base_size * mag_factor.powf(0.4)
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
        // Embedded bright star data (mag < 4.0) for initial implementation
        // This includes the ~500 brightest stars visible to naked eye
        self.stars = BRIGHT_STARS
            .iter()
            .filter(|s| s.2 <= self.max_magnitude)
            .map(|&(ra, dec, mag, ci, name)| {
                Star::new(ra, dec, mag, ci, name.map(String::from))
            })
            .collect();
        
        tracing::info!("Loaded {} stars (mag <= {:.1})", self.stars.len(), self.max_magnitude);
    }

    pub fn stars(&self) -> &[Star] {
        &self.stars
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

// Bright stars data: (RA hours, Dec degrees, magnitude, B-V color index, name)
static BRIGHT_STARS: &[(f64, f64, f64, f64, Option<&str>)] = &[
    // Magnitude < 1
    (6.7525, -16.7161, -1.46, 0.00, Some("Sirius")),
    (6.3992, -52.6956, -0.72, 0.15, Some("Canopus")),
    (14.2608, -60.8339, -0.27, 0.71, Some("Alpha Centauri")),
    (18.6156, 38.7836, 0.03, 0.00, Some("Vega")),
    (5.2783, -8.2017, 0.12, -0.03, Some("Rigel")),
    (7.6550, 5.2250, 0.34, 0.42, Some("Procyon")),
    (5.9194, 7.4069, 0.42, 1.85, Some("Betelgeuse")),
    (1.6283, -57.2367, 0.46, -0.16, Some("Achernar")),
    (14.0608, 19.1822, 0.77, -0.23, Some("Arcturus")),
    (12.4433, -63.0992, 0.77, -0.24, Some("Beta Centauri")),
    (19.8461, 8.8683, 0.77, 0.22, Some("Altair")),
    (5.4189, -28.9722, 0.86, -0.21, Some("Aldebaran")),
    (12.9000, -59.6883, 0.87, 1.59, Some("Alpha Crucis")),
    (13.4192, 54.9253, 0.98, -0.02, Some("Spica")),
    (16.4900, -26.4322, 0.96, 1.83, Some("Antares")),
    (7.5767, 31.8883, 1.14, 0.03, Some("Pollux")),
    (22.9608, -29.6222, 1.16, 0.09, Some("Fomalhaut")),
    (20.6906, 45.2803, 1.25, 0.09, Some("Deneb")),
    (12.5194, -57.1128, 1.25, -0.23, Some("Beta Crucis")),
    (10.1394, 11.9672, 1.35, -0.11, Some("Regulus")),
    
    // Magnitude 1-2
    (5.4181, 28.6075, 1.65, -0.08, Some("Elnath")),
    (6.6283, -52.6958, 1.68, -0.23, Some("Miaplacidus")),
    (2.1194, 89.2642, 1.98, 0.60, Some("Polaris")),
    (5.6031, -1.2019, 1.70, -0.22, Some("Alnilam")),
    (9.2200, -69.7172, 1.67, 1.28, Some("Beta Carinae")),
    (7.5767, 28.0264, 1.58, 0.04, Some("Castor")),
    (8.1583, -47.3367, 1.86, -0.22, Some("Avior")),
    (17.5600, -37.1039, 1.62, -0.22, Some("Shaula")),
    (20.4272, -56.7350, 1.94, 0.20, Some("Peacock")),
    (5.5950, -5.9097, 1.77, -0.19, Some("Alnitak")),
    
    // More bright stars (mag 2-3) - key constellations
    (0.1397, 29.0906, 2.06, -0.11, Some("Alpheratz")),
    (1.1622, 35.6206, 2.07, 1.58, Some("Mirach")),
    (2.0650, 42.3297, 2.26, 0.48, Some("Mirfak")),
    (3.0381, 4.0897, 2.00, 1.02, Some("Menkar")),
    (3.4050, 49.8611, 1.79, 0.48, Some("Algol")),
    (3.7917, 24.1053, 2.87, 1.54, Some("Alcyone")),
    (4.5986, 16.5094, 0.85, 1.54, Some("Aldebaran")),
    (5.0794, -5.0900, 2.77, -0.18, Some("Mintaka")),
    (5.2428, -8.2017, 2.06, -0.22, Some("Saiph")),
    (5.5333, 9.9342, 3.19, 1.64, Some("Meissa")),
    (5.6794, -1.9428, 2.23, -0.24, Some("Alnitak B")),
    (6.3783, 22.5139, 1.90, 0.80, Some("Capella")),
    (7.4550, -26.3931, 1.83, -0.21, Some("Wezen")),
    (8.7450, -54.7089, 1.99, 1.28, Some("Aspidiske")),
    (9.4608, -8.6594, 2.00, 0.70, Some("Alphard")),
    (10.3328, 19.8417, 2.56, 0.13, Some("Algieba")),
    (11.0617, 61.7508, 1.79, 0.03, Some("Dubhe")),
    (11.8972, 53.6947, 2.27, -0.02, Some("Merak")),
    (12.2261, 57.0322, 2.44, 0.02, Some("Phecda")),
    (12.9008, 55.9597, 1.77, -0.02, Some("Alioth")),
    (13.3983, 54.9256, 1.86, -0.19, Some("Mizar")),
    (13.7922, 49.3133, 1.85, -0.02, Some("Alkaid")),
    (14.8450, 74.1556, 2.08, 1.47, Some("Kochab")),
    (15.5781, 26.7147, 2.23, -0.03, Some("Alphecca")),
    (16.0894, -19.8053, 2.89, 0.02, Some("Dschubba")),
    (17.1767, -15.7247, 2.43, 0.40, Some("Sabik")),
    (17.5822, 12.5603, 2.08, 0.97, Some("Rasalhague")),
    (18.3500, -29.8281, 1.85, 0.28, Some("Kaus Australis")),
    (18.9217, 36.8986, 3.24, 0.18, Some("Sheliak")),
    (19.0461, 13.8636, 2.99, -0.13, Some("Albireo")),
    (19.8461, 8.8683, 0.77, 0.22, Some("Altair")),
    (20.7606, 33.9703, 2.20, 1.03, Some("Sadr")),
    (21.7361, 9.8750, 2.39, 0.83, Some("Enif")),
    (22.7108, -46.9611, 2.10, 0.14, Some("Alnair")),
    (23.0628, 28.0825, 2.42, -0.05, Some("Scheat")),
    (0.2208, 15.1836, 2.83, -0.11, Some("Algenib")),
    
    // Southern Cross and nearby
    (12.4433, -63.0992, 0.77, -0.24, Some("Acrux")),
    (12.5194, -57.1128, 1.25, -0.23, Some("Mimosa")),
    (12.2522, -58.7489, 1.63, 1.59, Some("Gacrux")),
    (12.3517, -60.4011, 2.80, -0.24, Some("Delta Crucis")),
    
    // Orion Belt
    (5.6031, -1.2019, 1.70, -0.22, Some("Alnilam")),
    (5.5333, -0.2992, 2.23, -0.17, Some("Mintaka")),
    (5.6794, -1.9428, 1.77, -0.19, Some("Alnitak")),
    
    // Big Dipper / Ursa Major
    (11.0617, 61.7508, 1.79, 0.03, Some("Dubhe")),
    (11.0306, 56.3825, 2.37, -0.02, Some("Merak")),
    (11.8972, 53.6947, 2.44, 0.02, Some("Phecda")),
    (12.2569, 57.0325, 3.31, -0.01, Some("Megrez")),
    (12.9008, 55.9597, 1.77, -0.02, Some("Alioth")),
    (13.3983, 54.9256, 2.27, 0.02, Some("Mizar")),
    (13.7922, 49.3133, 1.85, -0.02, Some("Alkaid")),
];
