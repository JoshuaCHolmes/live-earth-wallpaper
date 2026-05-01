//! Multi-satellite support for geostationary Earth imagery
//!
//! Supports fetching full-disk Earth images from various geostationary satellites:
//! - Himawari-9 (140.7°E) - Japan/Asia-Pacific  
//! - GOES-East/GOES-19 (75.2°W) - Americas/Atlantic
//! - GOES-West/GOES-18 (137.2°W) - Pacific/West Americas
//! - GK2A (128.2°E) - Korea/Asia
//! - Meteosat-12 (0°) - Europe/Africa
//!
//! All satellites use consistent GOES-style green synthesis from Blue, Red, and Veggie bands
//! for uniform color appearance.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use image::{DynamicImage, GenericImage, RgbaImage};
use std::time::Duration;

/// Synthesize green channel from Red, Veggie, and Blue bands (GOES-style)
/// Formula: G = 0.45*R + 0.10*Veggie + 0.45*B
/// This provides consistent color across all satellites and naturally reduces
/// atmospheric Rayleigh scattering effects by averaging across channels.
#[inline]
fn synthesize_green(r: f32, veggie: f32, b: f32) -> f32 {
    0.45 * r + 0.10 * veggie + 0.45 * b
}

/// Apply gamma correction to brighten image slightly
/// Gamma < 1 brightens, > 1 darkens
const GAMMA: f32 = 1.0 / 1.1;

#[inline]
fn apply_gamma(value: f32) -> u8 {
    (255.0 * (value / 255.0).powf(GAMMA)).clamp(0.0, 255.0) as u8
}

/// Available geostationary satellites
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Satellite {
    /// Himawari-9 at 140.7°E (Japan Meteorological Agency)
    #[default]
    Himawari,
    /// GOES-East (GOES-19) at 75.2°W (NOAA)
    GoesEast,
    /// GOES-West (GOES-18) at 137.2°W (NOAA)
    GoesWest,
    /// GEO-KOMPSAT-2A at 128.2°E (Korea Meteorological Administration)
    Gk2a,
    /// Meteosat-12 at 0° (EUMETSAT) - Europe/Africa view
    Meteosat12,
}

impl Satellite {
    /// Satellite's geostationary longitude in degrees (East positive)
    pub fn longitude(&self) -> f64 {
        match self {
            Satellite::Himawari => 140.7,
            Satellite::GoesEast => -75.2,
            Satellite::GoesWest => -137.2,
            Satellite::Gk2a => 128.2,
            Satellite::Meteosat12 => 0.0,
        }
    }

    /// Short name for tray menu
    pub fn name(&self) -> &'static str {
        match self {
            Satellite::Himawari => "Himawari-9",
            Satellite::GoesEast => "GOES-East",
            Satellite::GoesWest => "GOES-West",
            Satellite::Gk2a => "GK2A",
            Satellite::Meteosat12 => "Meteosat-12",
        }
    }

    /// Get next satellite in rotation
    #[allow(dead_code)]
    pub fn next(&self) -> Self {
        match self {
            Satellite::Himawari => Satellite::GoesEast,
            Satellite::GoesEast => Satellite::GoesWest,
            Satellite::GoesWest => Satellite::Gk2a,
            Satellite::Gk2a => Satellite::Meteosat12,
            Satellite::Meteosat12 => Satellite::Himawari,
        }
    }

    /// All available satellites
    pub fn all() -> &'static [Satellite] {
        &[
            Satellite::Himawari,
            Satellite::GoesEast,
            Satellite::GoesWest,
            Satellite::Gk2a,
            Satellite::Meteosat12,
        ]
    }
}

const MAX_TILE_SIZE: usize = 2 * 1024 * 1024;

// ============================================================================
// SLIDER-based fetching (all satellites via RAMMB/CIRA SLIDER)
// ============================================================================

const SLIDER_BASE_URL: &str = "https://slider.cira.colostate.edu";
const MAX_SLIDER_METADATA_SIZE: usize = 64 * 1024; // 64KB for timestamps JSON

// Tile sizes vary by satellite (from SLIDER define-products.js)
const GK2A_TILE_SIZE: u32 = 688;
const HIMAWARI_TILE_SIZE: u32 = 688;
const GOES_TILE_SIZE: u32 = 678;
const METEOSAT12_TILE_SIZE: u32 = 696;

/// SLIDER timestamps response
#[derive(Debug, serde::Deserialize)]
struct SliderTimestamps {
    timestamps_int: Vec<i64>,
}

/// Determine optimal zoom level for SLIDER based on desired output size and tile size
fn slider_zoom_for_size(target_size: u32, tile_size: u32) -> u8 {
    // zoom 0 = tile_size, zoom 1 = tile_size*2, etc.
    // We want smallest zoom that gives us >= target_size
    for zoom in 0..=5 {
        let size = tile_size * (1 << zoom);
        if size >= target_size {
            return zoom;
        }
    }
    5 // Max zoom
}

/// Fetch latest timestamp for a SLIDER satellite/band
async fn fetch_slider_timestamp(
    client: &reqwest::Client,
    sat: &str,
    band: &str,
) -> Result<(i64, String)> {
    let url = format!(
        "{}/data/json/{}/full_disk/{}/latest_times.json",
        SLIDER_BASE_URL, sat, band
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch SLIDER timestamps")?;

    if let Some(len) = response.content_length() {
        if len as usize > MAX_SLIDER_METADATA_SIZE {
            anyhow::bail!("SLIDER metadata too large: {} bytes", len);
        }
    }

    let bytes = response.bytes().await?;
    let timestamps: SliderTimestamps = serde_json::from_slice(&bytes)
        .context("Failed to parse SLIDER timestamps")?;

    let ts = *timestamps.timestamps_int.first()
        .ok_or_else(|| anyhow::anyhow!("No timestamps available"))?;

    // Extract date parts for URL path: YYYYMMDDHHMMSS -> YYYY/MM/DD
    let ts_str = ts.to_string();
    let date_path = format!("{}/{}/{}", &ts_str[0..4], &ts_str[4..6], &ts_str[6..8]);

    Ok((ts, date_path))
}

/// Fetch a single SLIDER tile
async fn fetch_slider_tile(
    client: &reqwest::Client,
    sat: &str,
    band: &str,
    timestamp: i64,
    date_path: &str,
    zoom: u8,
    row: u32,
    col: u32,
) -> Result<DynamicImage> {
    let url = format!(
        "{}/data/imagery/{}/{}---full_disk/{}/{}/{:02}/{:03}_{:03}.png",
        SLIDER_BASE_URL, date_path, sat, band, timestamp, zoom, row, col
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("Failed to fetch SLIDER tile z{} r{} c{}", zoom, row, col))?;

    if !response.status().is_success() {
        anyhow::bail!("SLIDER tile request failed: {}", response.status());
    }

    if let Some(len) = response.content_length() {
        if len as usize > MAX_TILE_SIZE {
            anyhow::bail!("SLIDER tile too large: {} bytes", len);
        }
    }

    let bytes = response.bytes().await?;
    image::load_from_memory(&bytes)
        .with_context(|| format!("Failed to decode SLIDER tile z{} r{} c{}", zoom, row, col))
}

/// Fetch a complete band image from SLIDER at optimal resolution
/// Fetches tiles sequentially to minimize memory usage
async fn fetch_slider_band(
    client: &reqwest::Client,
    sat: &str,
    band: &str,
    timestamp: i64,
    date_path: &str,
    target_size: u32,
    tile_size: u32,
) -> Result<image::GrayImage> {
    let zoom = slider_zoom_for_size(target_size, tile_size);
    let tiles_per_side = 1u32 << zoom;
    let total_size = tile_size * tiles_per_side;

    tracing::debug!(
        "Fetching SLIDER {} band {} at zoom {} ({}x{} = {} tiles)",
        sat, band, zoom, tiles_per_side, tiles_per_side, tiles_per_side * tiles_per_side
    );

    let mut composite = image::GrayImage::new(total_size, total_size);

    // Fetch tiles sequentially to minimize peak memory
    for row in 0..tiles_per_side {
        for col in 0..tiles_per_side {
            let tile = fetch_slider_tile(client, sat, band, timestamp, date_path, zoom, row, col).await?;
            let gray = tile.to_luma8();
            composite
                .copy_from(&gray, col * tile_size, row * tile_size)
                .with_context(|| format!("Failed to composite tile ({}, {})", row, col))?;
            // tile and gray are dropped here, freeing memory
        }
    }

    Ok(composite)
}

/// Fetch GK2A image from SLIDER using GOES-style synthesis
/// GK2A bands: band_01=Blue(0.47µm), band_03=Red(0.64µm), band_04=Veggie(0.865µm)
/// Same wavelengths as GOES/Himawari for consistent color
async fn fetch_gk2a_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, "gk2a", "band_03").await?;

    tracing::debug!("Fetching GK2A (timestamp: {})...", timestamp);

    // Fetch Blue, Red, Veggie bands (same wavelengths as GOES/Himawari)
    let (band01, band03, band04) = tokio::try_join!(
        fetch_slider_band(client, "gk2a", "band_01", timestamp, &date_path, target_size, GK2A_TILE_SIZE),
        fetch_slider_band(client, "gk2a", "band_03", timestamp, &date_path, target_size, GK2A_TILE_SIZE),
        fetch_slider_band(client, "gk2a", "band_04", timestamp, &date_path, target_size, GK2A_TILE_SIZE),
    )?;

    tracing::debug!("Compositing GK2A...");

    let width = band03.width();
    let height = band03.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band03.get_pixel(x, y).0[0] as f32;
            let b = band01.get_pixel(x, y).0[0] as f32;
            let veggie = band04.get_pixel(x, y).0[0] as f32;

            let g = synthesize_green(r, veggie, b);
            composite.put_pixel(x, y, image::Rgba([apply_gamma(r), apply_gamma(g), apply_gamma(b), 255]));
        }
    }

    drop(band01);
    drop(band03);
    drop(band04);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::debug!("GK2A composite complete ({}x{})", width, height);

    Ok((composite, image_time))
}

/// Fetch Himawari image from SLIDER using GOES-style synthesis
/// Himawari bands: band_01=Blue(0.47µm), band_03=Red(0.64µm), band_04=Veggie(0.86µm)
/// Same wavelengths as GOES for consistency - green synthesized: G = 0.45*R + 0.10*V + 0.45*B
async fn fetch_himawari_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, "himawari", "band_03").await?;

    tracing::debug!("Fetching Himawari-9 (timestamp: {})...", timestamp);

    // Fetch Blue, Red, Veggie bands (same wavelengths as GOES)
    let (band01, band03, band04) = tokio::try_join!(
        fetch_slider_band(client, "himawari", "band_01", timestamp, &date_path, target_size, HIMAWARI_TILE_SIZE),
        fetch_slider_band(client, "himawari", "band_03", timestamp, &date_path, target_size, HIMAWARI_TILE_SIZE),
        fetch_slider_band(client, "himawari", "band_04", timestamp, &date_path, target_size, HIMAWARI_TILE_SIZE),
    )?;

    tracing::debug!("Compositing Himawari...");

    let width = band03.width();
    let height = band03.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band03.get_pixel(x, y).0[0] as f32;
            let b = band01.get_pixel(x, y).0[0] as f32;
            let veggie = band04.get_pixel(x, y).0[0] as f32;

            let g = synthesize_green(r, veggie, b);
            composite.put_pixel(x, y, image::Rgba([apply_gamma(r), apply_gamma(g), apply_gamma(b), 255]));
        }
    }

    drop(band01);
    drop(band03);
    drop(band04);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::debug!("Himawari composite complete ({}x{})", width, height);

    Ok((composite, image_time))
}

/// Fetch GOES satellite via SLIDER
/// GOES bands: band_01=Blue(0.47µm), band_02=Red(0.64µm), band_03=Veggie(0.86µm)
async fn fetch_goes_image_slider(
    client: &reqwest::Client,
    slider_sat: &str,
    name: &str,
) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, slider_sat, "band_02").await?;

    tracing::debug!("Fetching {} (timestamp: {})...", name, timestamp);

    // Fetch Blue, Red, Veggie bands
    let (band01, band02, band03) = tokio::try_join!(
        fetch_slider_band(client, slider_sat, "band_01", timestamp, &date_path, target_size, GOES_TILE_SIZE),
        fetch_slider_band(client, slider_sat, "band_02", timestamp, &date_path, target_size, GOES_TILE_SIZE),
        fetch_slider_band(client, slider_sat, "band_03", timestamp, &date_path, target_size, GOES_TILE_SIZE),
    )?;

    tracing::debug!("Compositing {}...", name);

    let width = band02.width();
    let height = band02.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band02.get_pixel(x, y).0[0] as f32;
            let b = band01.get_pixel(x, y).0[0] as f32;
            let veggie = band03.get_pixel(x, y).0[0] as f32;

            let g = synthesize_green(r, veggie, b);
            composite.put_pixel(x, y, image::Rgba([apply_gamma(r), apply_gamma(g), apply_gamma(b), 255]));
        }
    }

    drop(band01);
    drop(band02);
    drop(band03);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::debug!("{} composite complete ({}x{})", name, width, height);

    Ok((composite, image_time))
}

async fn fetch_goes_east_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_image_slider(client, "goes-19", "GOES-East").await
}

async fn fetch_goes_west_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_image_slider(client, "goes-18", "GOES-West").await
}

/// Fetch Meteosat-12 image from SLIDER
/// Meteosat-12 bands: band_01=Blue(0.44µm), band_03=Red(0.64µm), band_04=Veggie(0.865µm)
async fn fetch_meteosat12_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, "meteosat-12", "band_03").await?;

    tracing::debug!("Fetching Meteosat-12 (timestamp: {})...", timestamp);

    // Fetch Blue, Red, Veggie bands
    let (band01, band03, band04) = tokio::try_join!(
        fetch_slider_band(client, "meteosat-12", "band_01", timestamp, &date_path, target_size, METEOSAT12_TILE_SIZE),
        fetch_slider_band(client, "meteosat-12", "band_03", timestamp, &date_path, target_size, METEOSAT12_TILE_SIZE),
        fetch_slider_band(client, "meteosat-12", "band_04", timestamp, &date_path, target_size, METEOSAT12_TILE_SIZE),
    )?;

    tracing::debug!("Compositing Meteosat-12...");

    let width = band03.width();
    let height = band03.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band03.get_pixel(x, y).0[0] as f32;
            let b = band01.get_pixel(x, y).0[0] as f32;
            let veggie = band04.get_pixel(x, y).0[0] as f32;

            let g = synthesize_green(r, veggie, b);
            composite.put_pixel(x, y, image::Rgba([apply_gamma(r), apply_gamma(g), apply_gamma(b), 255]));
        }
    }

    drop(band01);
    drop(band03);
    drop(band04);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::debug!("Meteosat-12 composite complete ({}x{})", width, height);

    Ok((composite, image_time))
}

// ============================================================================
// Unified interface
// ============================================================================

/// Fetch Earth image from the specified satellite
pub async fn fetch_earth_image(
    client: &reqwest::Client,
    satellite: Satellite,
) -> Result<(RgbaImage, DateTime<Utc>)> {
    match satellite {
        Satellite::Himawari => fetch_himawari_image(client).await,
        Satellite::GoesEast => fetch_goes_east_image(client).await,
        Satellite::GoesWest => fetch_goes_west_image(client).await,
        Satellite::Gk2a => fetch_gk2a_image(client).await,
        Satellite::Meteosat12 => fetch_meteosat12_image(client).await,
    }
}

/// Get cache filename for a satellite
pub fn cache_filename(satellite: Satellite) -> &'static str {
    match satellite {
        Satellite::Himawari => "earth_cache_himawari.png",
        Satellite::GoesEast => "earth_cache_goes_east.png",
        Satellite::GoesWest => "earth_cache_goes_west.png",
        Satellite::Gk2a => "earth_cache_gk2a.png",
        Satellite::Meteosat12 => "earth_cache_meteosat12.png",
    }
}
