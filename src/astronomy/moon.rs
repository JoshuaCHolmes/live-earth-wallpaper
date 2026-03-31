//! Moon position and phase calculations

use super::coordinates::{
    deg_to_rad, julian_day, normalize_degrees, obliquity_degrees, rad_to_deg,
    Equatorial, ScreenPosition, equatorial_to_screen,
};
use chrono::{DateTime, Utc};

pub struct Moon {
    // Cached position data
    last_update: Option<DateTime<Utc>>,
    cached_position: Option<Equatorial>,
    cached_phase: f64,
    cached_illumination: f64,
}

impl Moon {
    pub fn new() -> Self {
        Self {
            last_update: None,
            cached_position: None,
            cached_phase: 0.0,
            cached_illumination: 0.0,
        }
    }

    /// Calculate the Moon's geocentric equatorial coordinates
    pub fn position(&mut self, dt: &DateTime<Utc>) -> Equatorial {
        // Use cached value if recent enough (within 1 minute)
        if let Some(last) = self.last_update {
            let diff = (*dt - last).num_seconds().abs();
            if diff < 60 {
                if let Some(pos) = self.cached_position {
                    return pos;
                }
            }
        }

        let jd = julian_day(dt);
        let t = (jd - 2451545.0) / 36525.0;

        // Moon's mean longitude
        let l0 = normalize_degrees(218.3164477 + 481267.88123421 * t
            - 0.0015786 * t * t + t * t * t / 538841.0);

        // Mean elongation
        let d = normalize_degrees(297.8501921 + 445267.1114034 * t
            - 0.0018819 * t * t + t * t * t / 545868.0);

        // Sun's mean anomaly
        let m = normalize_degrees(357.5291092 + 35999.0502909 * t
            - 0.0001536 * t * t);

        // Moon's mean anomaly
        let m_prime = normalize_degrees(134.9633964 + 477198.8675055 * t
            + 0.0087414 * t * t + t * t * t / 69699.0);

        // Moon's argument of latitude
        let f = normalize_degrees(93.2720950 + 483202.0175233 * t
            - 0.0036539 * t * t);

        // Longitude corrections (simplified)
        let d_rad = deg_to_rad(d);
        let m_rad = deg_to_rad(m);
        let m_prime_rad = deg_to_rad(m_prime);
        let f_rad = deg_to_rad(f);

        let longitude = l0
            + 6.289 * m_prime_rad.sin()
            - 1.274 * (2.0 * d_rad - m_prime_rad).sin()
            - 0.658 * (2.0 * d_rad).sin()
            - 0.214 * (2.0 * m_prime_rad).sin()
            - 0.186 * m_rad.sin();

        let latitude = 5.128 * f_rad.sin()
            + 0.281 * (m_prime_rad + f_rad).sin()
            + 0.278 * (m_prime_rad - f_rad).sin();

        // Convert ecliptic to equatorial
        let eps = deg_to_rad(obliquity_degrees(dt));
        let lon_rad = deg_to_rad(longitude);
        let lat_rad = deg_to_rad(latitude);

        let sin_lon = lon_rad.sin();
        let cos_lon = lon_rad.cos();
        let sin_lat = lat_rad.sin();
        let cos_lat = lat_rad.cos();
        let sin_eps = eps.sin();
        let cos_eps = eps.cos();

        let ra = (sin_lon * cos_eps - sin_lat / cos_lat * sin_eps).atan2(cos_lon);
        let dec = (sin_lat * cos_eps + cos_lat * sin_eps * sin_lon).asin();

        let ra_hours = (rad_to_deg(ra) / 15.0 + 24.0) % 24.0;
        let dec_deg = rad_to_deg(dec);

        let position = Equatorial::new(ra_hours, dec_deg);
        
        // Update cache
        self.last_update = Some(*dt);
        self.cached_position = Some(position);
        self.update_phase(dt);

        position
    }

    /// Calculate moon phase (0 = new, 0.5 = full, 1 = new again)
    fn update_phase(&mut self, dt: &DateTime<Utc>) {
        let jd = julian_day(dt);
        let t = (jd - 2451545.0) / 36525.0;

        // Mean elongation of the Moon
        let d = normalize_degrees(297.8501921 + 445267.1114034 * t);
        
        // Phase angle (0-360)
        self.cached_phase = d / 360.0;
        
        // Illumination fraction (approximate)
        let phase_angle = deg_to_rad(d);
        self.cached_illumination = (1.0 - phase_angle.cos()) / 2.0;
    }

    /// Get current phase (0-1, 0 = new moon, 0.5 = full moon)
    pub fn phase(&self) -> f64 {
        self.cached_phase
    }

    /// Get illumination fraction (0-1)
    pub fn illumination(&self) -> f64 {
        self.cached_illumination
    }

    /// Get phase name
    pub fn phase_name(&self) -> &'static str {
        let phase = self.cached_phase;
        if phase < 0.0625 || phase >= 0.9375 {
            "New Moon"
        } else if phase < 0.1875 {
            "Waxing Crescent"
        } else if phase < 0.3125 {
            "First Quarter"
        } else if phase < 0.4375 {
            "Waxing Gibbous"
        } else if phase < 0.5625 {
            "Full Moon"
        } else if phase < 0.6875 {
            "Waning Gibbous"
        } else if phase < 0.8125 {
            "Last Quarter"
        } else {
            "Waning Crescent"
        }
    }

    /// Get screen position
    pub fn screen_position(
        &mut self,
        dt: &DateTime<Utc>,
        width: u32,
        height: u32,
        fov: f64,
    ) -> ScreenPosition {
        let eq = self.position(dt);
        equatorial_to_screen(&eq, dt, width, height, fov)
    }

    /// Get approximate apparent magnitude
    pub fn magnitude(&self) -> f64 {
        // Full moon is about -12.7, new moon not visible
        -12.7 * self.cached_illumination + 1.0 * (1.0 - self.cached_illumination)
    }
}

impl Default for Moon {
    fn default() -> Self {
        Self::new()
    }
}
