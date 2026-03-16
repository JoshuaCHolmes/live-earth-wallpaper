//! Planetary position calculations
//!
//! Uses Keplerian orbital elements from NASA JPL to calculate
//! accurate positions for all major planets.

use super::coordinates::{
    deg_to_rad, julian_centuries, normalize_degrees, obliquity_degrees, rad_to_deg,
    Ecliptic, Equatorial, ScreenPosition, equatorial_to_screen,
};
use chrono::{DateTime, Utc};
use std::f64::consts::PI;

#[derive(Debug, Clone)]
pub struct OrbitalElements {
    // Base elements at J2000.0
    pub a: f64,          // Semi-major axis (AU)
    pub e: f64,          // Eccentricity
    pub i: f64,          // Inclination (degrees)
    pub l: f64,          // Mean longitude (degrees)
    pub long_peri: f64,  // Longitude of perihelion (degrees)
    pub long_node: f64,  // Longitude of ascending node (degrees)
    // Rates per century
    pub a_dot: f64,
    pub e_dot: f64,
    pub i_dot: f64,
    pub l_dot: f64,
    pub long_peri_dot: f64,
    pub long_node_dot: f64,
}

#[derive(Debug, Clone)]
pub struct Planet {
    pub name: &'static str,
    pub symbol: &'static str,
    pub elements: OrbitalElements,
    pub base_magnitude: f64,
    pub color: (u8, u8, u8),
}

impl Planet {
    /// Calculate heliocentric ecliptic coordinates for a given date
    pub fn heliocentric_position(&self, dt: &DateTime<Utc>) -> Ecliptic {
        let t = julian_centuries(dt);
        let elem = &self.elements;

        // Current orbital elements
        let a = elem.a + elem.a_dot * t;
        let e = elem.e + elem.e_dot * t;
        let i = deg_to_rad(elem.i + elem.i_dot * t);
        let l = deg_to_rad(elem.l + elem.l_dot * t);
        let long_peri = deg_to_rad(elem.long_peri + elem.long_peri_dot * t);
        let long_node = deg_to_rad(elem.long_node + elem.long_node_dot * t);

        let arg_peri = long_peri - long_node;
        let m = l - long_peri; // Mean anomaly

        // Solve Kepler's equation for eccentric anomaly
        let mut ea = m;
        for _ in 0..10 {
            ea = m + e * ea.sin();
        }

        // True anomaly
        let nu = 2.0 * ((1.0 + e).sqrt() * (ea / 2.0).sin())
            .atan2((1.0 - e).sqrt() * (ea / 2.0).cos());

        // Heliocentric distance
        let r = a * (1.0 - e * ea.cos());

        // Position in orbital plane
        let x_orb = r * nu.cos();
        let y_orb = r * nu.sin();

        // Transform to ecliptic coordinates
        let cos_arg = arg_peri.cos();
        let sin_arg = arg_peri.sin();
        let cos_i = i.cos();
        let sin_i = i.sin();
        let cos_node = long_node.cos();
        let sin_node = long_node.sin();

        let x_ecl = (cos_arg * cos_node - sin_arg * sin_node * cos_i) * x_orb
            + (-sin_arg * cos_node - cos_arg * sin_node * cos_i) * y_orb;
        let y_ecl = (cos_arg * sin_node + sin_arg * cos_node * cos_i) * x_orb
            + (-sin_arg * sin_node + cos_arg * cos_node * cos_i) * y_orb;
        let z_ecl = sin_arg * sin_i * x_orb + cos_arg * sin_i * y_orb;

        // Convert to longitude/latitude
        let lon = rad_to_deg(y_ecl.atan2(x_ecl));
        let lat = rad_to_deg(z_ecl.atan2((x_ecl * x_ecl + y_ecl * y_ecl).sqrt()));

        Ecliptic::new(normalize_degrees(lon), lat, r)
    }

    /// Calculate apparent magnitude based on distances
    pub fn apparent_magnitude(&self, geo_dist: f64, helio_dist: f64) -> f64 {
        self.base_magnitude + 5.0 * (geo_dist * helio_dist).log10()
    }
}

pub struct PlanetarySystem {
    planets: Vec<Planet>,
}

impl PlanetarySystem {
    pub fn new() -> Self {
        Self {
            planets: vec![
                Planet {
                    name: "Mercury",
                    symbol: "☿",
                    elements: OrbitalElements {
                        a: 0.38709927, e: 0.20563593, i: 7.00497902,
                        l: 252.25032350, long_peri: 77.45779628, long_node: 48.33076593,
                        a_dot: 0.00000037, e_dot: 0.00001906, i_dot: -0.00594749,
                        l_dot: 149472.67411175, long_peri_dot: 0.16047689, long_node_dot: -0.12534081,
                    },
                    base_magnitude: -0.42,
                    color: (166, 146, 112),
                },
                Planet {
                    name: "Venus",
                    symbol: "♀",
                    elements: OrbitalElements {
                        a: 0.72333566, e: 0.00677672, i: 3.39467605,
                        l: 181.97909950, long_peri: 131.60246718, long_node: 76.67984255,
                        a_dot: 0.00000390, e_dot: -0.00004107, i_dot: -0.00078890,
                        l_dot: 58517.81538729, long_peri_dot: 0.00268329, long_node_dot: -0.27769418,
                    },
                    base_magnitude: -4.6,
                    color: (255, 244, 200),
                },
                Planet {
                    name: "Mars",
                    symbol: "♂",
                    elements: OrbitalElements {
                        a: 1.52371034, e: 0.09339410, i: 1.84969142,
                        l: -4.55343205, long_peri: -23.94362959, long_node: 49.55953891,
                        a_dot: 0.00001847, e_dot: 0.00007882, i_dot: -0.00813131,
                        l_dot: 19140.30268499, long_peri_dot: 0.44441088, long_node_dot: -0.29257343,
                    },
                    base_magnitude: -2.94,
                    color: (205, 92, 92),
                },
                Planet {
                    name: "Jupiter",
                    symbol: "♃",
                    elements: OrbitalElements {
                        a: 5.20288700, e: 0.04838624, i: 1.30439695,
                        l: 34.39644051, long_peri: 14.72847983, long_node: 100.47390909,
                        a_dot: -0.00011607, e_dot: -0.00013253, i_dot: -0.00183714,
                        l_dot: 3034.74612775, long_peri_dot: 0.21252668, long_node_dot: 0.20469106,
                    },
                    base_magnitude: -2.94,
                    color: (216, 202, 157),
                },
                Planet {
                    name: "Saturn",
                    symbol: "♄",
                    elements: OrbitalElements {
                        a: 9.53667594, e: 0.05386179, i: 2.48599187,
                        l: 49.95424423, long_peri: 92.59887831, long_node: 113.66242448,
                        a_dot: -0.00125060, e_dot: -0.00050991, i_dot: 0.00193609,
                        l_dot: 1222.49362201, long_peri_dot: -0.41897216, long_node_dot: -0.28867794,
                    },
                    base_magnitude: 0.67,
                    color: (250, 241, 169),
                },
                Planet {
                    name: "Uranus",
                    symbol: "♅",
                    elements: OrbitalElements {
                        a: 19.18916464, e: 0.04725744, i: 0.77263783,
                        l: 313.23810451, long_peri: 170.95427630, long_node: 74.01692503,
                        a_dot: -0.00196176, e_dot: -0.00004397, i_dot: -0.00242939,
                        l_dot: 428.48202785, long_peri_dot: 0.40805281, long_node_dot: 0.04240589,
                    },
                    base_magnitude: 5.32,
                    color: (157, 208, 230),
                },
                Planet {
                    name: "Neptune",
                    symbol: "♆",
                    elements: OrbitalElements {
                        a: 30.06992276, e: 0.00859048, i: 1.77004347,
                        l: -55.12002969, long_peri: 44.96476227, long_node: 131.78422574,
                        a_dot: 0.00026291, e_dot: 0.00005105, i_dot: 0.00035372,
                        l_dot: 218.45945325, long_peri_dot: -0.32241464, long_node_dot: -0.00508664,
                    },
                    base_magnitude: 7.78,
                    color: (102, 146, 204),
                },
            ],
        }
    }

    /// Get Earth's heliocentric position
    fn earth_position(&self, dt: &DateTime<Utc>) -> Ecliptic {
        let t = julian_centuries(dt);
        
        let a = 1.00000261 + 0.00000562 * t;
        let e = 0.01671123 - 0.00004392 * t;
        let i = deg_to_rad(-0.00001531 - 0.01294668 * t);
        let l = deg_to_rad(100.46457166 + 35999.37244981 * t);
        let long_peri = deg_to_rad(102.93768193 + 0.32327364 * t);
        let long_node = deg_to_rad(0.0);

        let arg_peri = long_peri - long_node;
        let m = l - long_peri;

        let mut ea = m;
        for _ in 0..10 {
            ea = m + e * ea.sin();
        }

        let nu = 2.0 * ((1.0 + e).sqrt() * (ea / 2.0).sin())
            .atan2((1.0 - e).sqrt() * (ea / 2.0).cos());

        let r = a * (1.0 - e * ea.cos());

        let x_orb = r * nu.cos();
        let y_orb = r * nu.sin();

        let cos_arg = arg_peri.cos();
        let sin_arg = arg_peri.sin();
        let cos_i = i.cos();
        let sin_i = i.sin();
        let cos_node = long_node.cos();
        let sin_node = long_node.sin();

        let x_ecl = (cos_arg * cos_node - sin_arg * sin_node * cos_i) * x_orb
            + (-sin_arg * cos_node - cos_arg * sin_node * cos_i) * y_orb;
        let y_ecl = (cos_arg * sin_node + sin_arg * cos_node * cos_i) * x_orb
            + (-sin_arg * sin_node + cos_arg * cos_node * cos_i) * y_orb;
        let z_ecl = sin_arg * sin_i * x_orb + cos_arg * sin_i * y_orb;

        let lon = rad_to_deg(y_ecl.atan2(x_ecl));
        let lat = rad_to_deg(z_ecl.atan2((x_ecl * x_ecl + y_ecl * y_ecl).sqrt()));

        Ecliptic::new(normalize_degrees(lon), lat, r)
    }

    /// Calculate geocentric equatorial coordinates for a planet
    pub fn planet_position(&self, planet: &Planet, dt: &DateTime<Utc>) -> (Equatorial, f64, f64) {
        let helio = planet.heliocentric_position(dt);
        let earth = self.earth_position(dt);

        let (px, py, pz) = helio.to_cartesian();
        let (ex, ey, ez) = earth.to_cartesian();

        // Geocentric ecliptic coordinates
        let gx = px - ex;
        let gy = py - ey;
        let gz = pz - ez;
        let geo_dist = (gx * gx + gy * gy + gz * gz).sqrt();

        // Convert to equatorial
        let eps = deg_to_rad(obliquity_degrees(dt));
        let x = gx;
        let y = gy * eps.cos() - gz * eps.sin();
        let z = gy * eps.sin() + gz * eps.cos();

        let ra = rad_to_deg(y.atan2(x)) / 15.0; // Convert to hours
        let dec = rad_to_deg(z.atan2((x * x + y * y).sqrt()));

        let ra_normalized = ((ra % 24.0) + 24.0) % 24.0;

        (Equatorial::new(ra_normalized, dec), geo_dist, helio.distance)
    }

    /// Get all visible planets with their screen positions
    pub fn visible_planets(
        &self,
        dt: &DateTime<Utc>,
        width: u32,
        height: u32,
        fov: f64,
        max_magnitude: f64,
    ) -> Vec<(&Planet, Equatorial, ScreenPosition, f64)> {
        self.planets
            .iter()
            .filter_map(|planet| {
                let (eq, geo_dist, helio_dist) = self.planet_position(planet, dt);
                let mag = planet.apparent_magnitude(geo_dist, helio_dist);
                
                if mag > max_magnitude {
                    return None;
                }

                let pos = equatorial_to_screen(&eq, dt, width, height, fov);
                if pos.visible {
                    Some((planet, eq, pos, mag))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn planets(&self) -> &[Planet] {
        &self.planets
    }
}

impl Default for PlanetarySystem {
    fn default() -> Self {
        Self::new()
    }
}
