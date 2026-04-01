//! Astronomical coordinate systems and transformations

use chrono::{DateTime, Datelike, Timelike, Utc};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// Default satellite longitude (Himawari-9 at 140.7°E)
const DEFAULT_SATELLITE_LONGITUDE: f64 = 140.7;

// Atomic storage for satellite longitude (stored as bits)
static SATELLITE_LONGITUDE_BITS: AtomicU64 = AtomicU64::new(0);
static SATELLITE_LONGITUDE_SET: AtomicBool = AtomicBool::new(false);

/// Get current satellite longitude
pub fn get_satellite_longitude() -> f64 {
    if SATELLITE_LONGITUDE_SET.load(Ordering::Relaxed) {
        f64::from_bits(SATELLITE_LONGITUDE_BITS.load(Ordering::Relaxed))
    } else {
        DEFAULT_SATELLITE_LONGITUDE
    }
}

/// Set satellite longitude for coordinate calculations
pub fn set_satellite_longitude(longitude: f64) {
    SATELLITE_LONGITUDE_BITS.store(longitude.to_bits(), Ordering::Relaxed);
    SATELLITE_LONGITUDE_SET.store(true, Ordering::Relaxed);
}

pub const SATELLITE_ALTITUDE_KM: f64 = 35793.0;
pub const EARTH_RADIUS_KM: f64 = 6371.0;
pub const EARTH_EQUATORIAL_RADIUS_KM: f64 = 6378.137;

#[inline]
pub fn deg_to_rad(deg: f64) -> f64 {
    deg * PI / 180.0
}

#[inline]
pub fn rad_to_deg(rad: f64) -> f64 {
    rad * 180.0 / PI
}

#[inline]
pub fn hours_to_rad(hours: f64) -> f64 {
    hours * PI / 12.0
}

#[inline]
pub fn normalize_degrees(deg: f64) -> f64 {
    ((deg % 360.0) + 360.0) % 360.0
}

#[inline]
pub fn normalize_radians(rad: f64) -> f64 {
    ((rad % (2.0 * PI)) + 2.0 * PI) % (2.0 * PI)
}

pub fn julian_day(dt: &DateTime<Utc>) -> f64 {
    let year = dt.year();
    let month = dt.month() as i32;
    let day = dt.day() as f64;

    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;

    let jdn = day as i32
        + (153 * m + 2) / 5
        + 365 * y
        + y / 4
        - y / 100
        + y / 400
        - 32045;

    let day_fraction = (dt.hour() as f64 - 12.0) / 24.0
        + dt.minute() as f64 / 1440.0
        + dt.second() as f64 / 86400.0;

    jdn as f64 + day_fraction
}

pub fn julian_centuries(dt: &DateTime<Utc>) -> f64 {
    (julian_day(dt) - 2451545.0) / 36525.0
}

pub fn gmst_degrees(dt: &DateTime<Utc>) -> f64 {
    let jd = julian_day(dt);
    let t = (jd - 2451545.0) / 36525.0;
    let gmst0 = 280.46061837 + 360.98564736629 * (jd - 2451545.0) + 0.000387933 * t * t
        - t * t * t / 38710000.0;
    normalize_degrees(gmst0)
}

pub fn lst_degrees(dt: &DateTime<Utc>, longitude: f64) -> f64 {
    normalize_degrees(gmst_degrees(dt) + longitude)
}

pub fn obliquity_degrees(dt: &DateTime<Utc>) -> f64 {
    let t = julian_centuries(dt);
    23.439291 - 0.0130042 * t - 0.00000016 * t * t + 0.000000504 * t * t * t
}

#[derive(Debug, Clone, Copy)]
pub struct Equatorial {
    pub ra: f64,  // hours (0-24)
    pub dec: f64, // degrees (-90 to +90)
}

impl Equatorial {
    pub fn new(ra: f64, dec: f64) -> Self {
        Self { ra, dec }
    }

    pub fn ra_degrees(&self) -> f64 {
        self.ra * 15.0
    }

    pub fn ra_radians(&self) -> f64 {
        hours_to_rad(self.ra)
    }

    pub fn dec_radians(&self) -> f64 {
        deg_to_rad(self.dec)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Ecliptic {
    pub lon: f64,
    pub lat: f64,
    pub distance: f64,
}

impl Ecliptic {
    pub fn new(lon: f64, lat: f64, distance: f64) -> Self {
        Self { lon, lat, distance }
    }

    pub fn to_cartesian(&self) -> (f64, f64, f64) {
        let lon_rad = deg_to_rad(self.lon);
        let lat_rad = deg_to_rad(self.lat);
        let x = self.distance * lat_rad.cos() * lon_rad.cos();
        let y = self.distance * lat_rad.cos() * lon_rad.sin();
        let z = self.distance * lat_rad.sin();
        (x, y, z)
    }

    pub fn to_equatorial(&self, dt: &DateTime<Utc>) -> Equatorial {
        let eps = deg_to_rad(obliquity_degrees(dt));
        let lon_rad = deg_to_rad(self.lon);
        let lat_rad = deg_to_rad(self.lat);

        let sin_lon = lon_rad.sin();
        let cos_lon = lon_rad.cos();
        let sin_lat = lat_rad.sin();
        let cos_lat = lat_rad.cos();
        let sin_eps = eps.sin();
        let cos_eps = eps.cos();

        let ra = (sin_lon * cos_eps - sin_lat / cos_lat * sin_eps).atan2(cos_lon);
        let dec = (sin_lat * cos_eps + cos_lat * sin_eps * sin_lon).asin();

        let ra_hours = normalize_radians(ra) * 12.0 / PI;
        Equatorial::new(ra_hours, rad_to_deg(dec))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScreenPosition {
    pub x: f64,
    pub y: f64,
    pub visible: bool,
}

impl ScreenPosition {
    pub fn new(x: f64, y: f64, visible: bool) -> Self {
        Self { x, y, visible }
    }

    pub fn hidden() -> Self {
        Self { x: 0.0, y: 0.0, visible: false }
    }
}

pub fn equatorial_to_screen(
    eq: &Equatorial,
    dt: &DateTime<Utc>,
    canvas_width: u32,
    canvas_height: u32,
    fov_degrees: f64,
) -> ScreenPosition {
    // We want the view looking TOWARD the satellite's longitude from space
    // So we're looking at the longitude opposite to the sky we want to see
    // The "center" of our sky view is (Satellite Longitude + 180 degrees)
    let view_longitude = get_satellite_longitude() + 180.0;
    
    let lst = lst_degrees(dt, view_longitude);
    let ha = deg_to_rad(lst - eq.ra_degrees());
    let dec_rad = eq.dec_radians();

    let cos_c = dec_rad.cos() * ha.cos();
    if cos_c <= 0.0 {
        return ScreenPosition::hidden();
    }

    let x = dec_rad.cos() * ha.sin() / cos_c;
    let y = dec_rad.sin() / cos_c;

    let fov_rad = deg_to_rad(fov_degrees);
    let scale = canvas_width.min(canvas_height) as f64 / (2.0 * (fov_rad / 2.0).tan());

    // Normal projection: positive HA (west) is to the right on sky maps
    let screen_x = canvas_width as f64 / 2.0 + x * scale;
    let screen_y = canvas_height as f64 / 2.0 - y * scale;

    let visible = screen_x >= 0.0
        && screen_x < canvas_width as f64
        && screen_y >= 0.0
        && screen_y < canvas_height as f64;

    ScreenPosition::new(screen_x, screen_y, visible)
}

/// Calculate the Sun's geocentric equatorial coordinates
pub fn sun_position(dt: &DateTime<Utc>) -> Equatorial {
    let jd = julian_day(dt);
    let n = jd - 2451545.0; // Days since J2000.0
    
    // Mean longitude and anomaly
    let l = normalize_degrees(280.460 + 0.9856474 * n);
    let g = normalize_degrees(357.528 + 0.9856003 * n);
    let g_rad = deg_to_rad(g);
    
    // Ecliptic longitude (with equation of center)
    let lambda = normalize_degrees(l + 1.915 * g_rad.sin() + 0.020 * (2.0 * g_rad).sin());
    
    // Ecliptic latitude is essentially 0 for the Sun
    let ecliptic = Ecliptic::new(lambda, 0.0, 1.0);
    ecliptic.to_equatorial(dt)
}

/// Get Sun's screen position
pub fn sun_screen_position(
    dt: &DateTime<Utc>,
    width: u32,
    height: u32,
    fov: f64,
) -> ScreenPosition {
    let eq = sun_position(dt);
    equatorial_to_screen(&eq, dt, width, height, fov)
}
