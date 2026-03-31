//! Multi-satellite support for geostationary Earth imagery
//!
//! Supports fetching full-disk Earth images from various geostationary satellites:
//! - Himawari-9 (140.7°E) - Japan/Asia-Pacific  
//! - GOES-East/GOES-19 (75.2°W) - Americas/Atlantic
//! - GOES-West/GOES-18 (137.2°W) - Pacific/West Americas
//! - GK2A (128.2°E) - Korea/Asia (GEO-KOMPSAT-2A with true RGB bands)

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use image::{DynamicImage, GenericImage, RgbaImage};
use std::time::Duration;

/// Available geostationary satellites
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Satellite {
    /// Himawari-9 at 140.7°E (Japan Meteorological Agency)
    #[default]
    Himawari,
    /// GOES-East (GOES-19) at 75.2°W (NOAA)
    GoesEast,
    /// GOES-West (GOES-18) at 137.2°W (NOAA)
    GoesWest,
    /// GEO-KOMPSAT-2A at 128.2°E (Korea Meteorological Administration)
    /// Uses RAMMB SLIDER for clean true-RGB imagery
    Gk2a,
}

impl Satellite {
    /// Satellite's geostationary longitude in degrees (East positive)
    pub fn longitude(&self) -> f64 {
        match self {
            Satellite::Himawari => 140.7,
            Satellite::GoesEast => -75.2,
            Satellite::GoesWest => -137.2,
            Satellite::Gk2a => 128.2,
        }
    }

    /// Short name for tray menu
    pub fn name(&self) -> &'static str {
        match self {
            Satellite::Himawari => "Himawari-9",
            Satellite::GoesEast => "GOES-East",
            Satellite::GoesWest => "GOES-West",
            Satellite::Gk2a => "GK2A",
        }
    }

    /// Get next satellite in rotation
    pub fn next(&self) -> Self {
        match self {
            Satellite::Himawari => Satellite::GoesEast,
            Satellite::GoesEast => Satellite::GoesWest,
            Satellite::GoesWest => Satellite::Gk2a,
            Satellite::Gk2a => Satellite::Himawari,
        }
    }

    /// All available satellites
    pub fn all() -> &'static [Satellite] {
        &[
            Satellite::Himawari,
            Satellite::GoesEast,
            Satellite::GoesWest,
            Satellite::Gk2a,
        ]
    }
}

// ============================================================================
// Himawari fetching (tile-based from NICT)
// ============================================================================

const HIMAWARI_BASE_URL: &str = "https://himawari8-dl.nict.go.jp";
const HIMAWARI_TILE_SIZE: u32 = 550;
const MAX_METADATA_SIZE: usize = 1024;
const MAX_TILE_SIZE: usize = 2 * 1024 * 1024;

/// Himawari resolution level
#[derive(Debug, Clone, Copy)]
pub enum HimawariLevel {
    Level4 = 4,
}

impl HimawariLevel {
    pub fn grid_size(&self) -> u32 {
        *self as u32
    }

    pub fn total_pixels(&self) -> u32 {
        self.grid_size() * HIMAWARI_TILE_SIZE
    }
}

#[derive(Debug, serde::Deserialize)]
struct HimawariMetadata {
    date: String,
}

async fn fetch_himawari_metadata(client: &reqwest::Client) -> Result<HimawariMetadata> {
    let url = format!("{}/himawari8/img/FULL_24h/latest.json", HIMAWARI_BASE_URL);
    
    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch Himawari metadata")?;

    if let Some(len) = response.content_length() {
        if len as usize > MAX_METADATA_SIZE {
            anyhow::bail!("Metadata too large: {} bytes", len);
        }
    }

    let bytes = response.bytes().await?;
    if bytes.len() > MAX_METADATA_SIZE {
        anyhow::bail!("Metadata too large: {} bytes", bytes.len());
    }

    serde_json::from_slice(&bytes).context("Failed to parse Himawari metadata")
}

fn himawari_date_to_path(date: &str) -> String {
    date.replace(['-', ' '], "/").replace(':', "")
}

async fn fetch_himawari_tile(
    client: &reqwest::Client,
    date_path: &str,
    level: HimawariLevel,
    x: u32,
    y: u32,
) -> Result<DynamicImage> {
    let url = format!(
        "{}/himawari8/img/D531106/{}d/550/{}_{}_{}.png",
        HIMAWARI_BASE_URL,
        level.grid_size(),
        date_path,
        x,
        y
    );

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("Failed to fetch tile ({}, {})", x, y))?;

    if let Some(len) = response.content_length() {
        if len as usize > MAX_TILE_SIZE {
            anyhow::bail!("Tile too large: {} bytes", len);
        }
    }

    let bytes = response.bytes().await?;
    if bytes.len() > MAX_TILE_SIZE {
        anyhow::bail!("Tile too large: {} bytes", bytes.len());
    }

    image::load_from_memory(&bytes).with_context(|| format!("Failed to decode tile ({}, {})", x, y))
}

async fn fetch_himawari_image_nict(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let metadata = fetch_himawari_metadata(client).await?;
    let date_path = himawari_date_to_path(&metadata.date);
    let level = HimawariLevel::Level4;
    let grid_size = level.grid_size();
    let total_size = level.total_pixels();

    tracing::info!(
        "Fetching Himawari-9 from NICT: {} ({}x{} tiles)",
        metadata.date, grid_size, grid_size
    );

    let mut composite = RgbaImage::new(total_size, total_size);

    let mut handles = Vec::new();
    for y in 0..grid_size {
        for x in 0..grid_size {
            let client = client.clone();
            let date_path = date_path.clone();
            handles.push(tokio::spawn(async move {
                let tile = fetch_himawari_tile(&client, &date_path, level, x, y).await?;
                Ok::<_, anyhow::Error>((x, y, tile))
            }));
        }
    }

    for handle in handles {
        let (x, y, tile) = handle.await??;
        composite
            .copy_from(&tile.to_rgba8(), x * HIMAWARI_TILE_SIZE, y * HIMAWARI_TILE_SIZE)
            .with_context(|| format!("Failed to composite tile ({}, {})", x, y))?;
    }

    let timestamp = chrono::NaiveDateTime::parse_from_str(&metadata.date, "%Y-%m-%d %H:%M:%S")
        .context("Failed to parse timestamp")?
        .and_utc();

    Ok((composite, timestamp))
}

/// Fetch Himawari image - tries NICT first, falls back to SLIDER
async fn fetch_himawari_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    // Try NICT first (faster, pre-composited true color)
    match fetch_himawari_image_nict(client).await {
        Ok(result) => return Ok(result),
        Err(e) => {
            tracing::warn!("NICT Himawari failed, trying SLIDER fallback: {}", e);
        }
    }
    
    // Fallback to SLIDER (requires band compositing but more reliable)
    fetch_himawari_image_slider(client).await
}

// ============================================================================
// SLIDER-based fetching (GK2A, Himawari fallback, GOES)
// ============================================================================

const SLIDER_BASE_URL: &str = "https://slider.cira.colostate.edu";
const MAX_SLIDER_METADATA_SIZE: usize = 64 * 1024; // 64KB for timestamps JSON

// Tile sizes vary by satellite (from SLIDER define-products.js)
const GK2A_TILE_SIZE: u32 = 688;
const HIMAWARI_SLIDER_TILE_SIZE: u32 = 688;
const GOES_SLIDER_TILE_SIZE: u32 = 678;

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

    // Fetch all tiles in parallel
    let mut handles = Vec::new();
    for row in 0..tiles_per_side {
        for col in 0..tiles_per_side {
            let client = client.clone();
            let sat = sat.to_string();
            let band = band.to_string();
            let date_path = date_path.to_string();
            handles.push(tokio::spawn(async move {
                let tile = fetch_slider_tile(&client, &sat, &band, timestamp, &date_path, zoom, row, col).await?;
                Ok::<_, anyhow::Error>((row, col, tile))
            }));
        }
    }

    for handle in handles {
        let (row, col, tile) = handle.await??;
        let gray = tile.to_luma8();
        composite
            .copy_from(&gray, col * tile_size, row * tile_size)
            .with_context(|| format!("Failed to composite tile ({}, {})", row, col))?;
    }

    Ok(composite)
}

/// Fetch GK2A true-color image from SLIDER
/// GK2A has actual RGB bands: band_01=Blue, band_02=Green, band_03=Red
async fn fetch_gk2a_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, "gk2a", "band_03").await?;

    tracing::info!("Fetching GK2A true-color (timestamp: {})...", timestamp);

    // Fetch bands sequentially to reduce peak memory usage
    tracing::info!("  Fetching GK2A band 1/3 (blue)...");
    let band01 = fetch_slider_band(client, "gk2a", "band_01", timestamp, &date_path, target_size, GK2A_TILE_SIZE).await?;
    tracing::info!("  Fetching GK2A band 2/3 (green)...");
    let band02 = fetch_slider_band(client, "gk2a", "band_02", timestamp, &date_path, target_size, GK2A_TILE_SIZE).await?;
    tracing::info!("  Fetching GK2A band 3/3 (red)...");
    let band03 = fetch_slider_band(client, "gk2a", "band_03", timestamp, &date_path, target_size, GK2A_TILE_SIZE).await?;

    tracing::info!("Compositing GK2A true-color...");

    let width = band03.width();
    let height = band03.height();

    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band03.get_pixel(x, y).0[0];
            let g = band02.get_pixel(x, y).0[0]; // True green!
            let b = band01.get_pixel(x, y).0[0];

            // Apply light gamma correction for visual consistency
            let gamma = 1.0 / 1.1;
            let r_out = (255.0 * (r as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let g_out = (255.0 * (g as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let b_out = (255.0 * (b as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;

            composite.put_pixel(x, y, image::Rgba([r_out, g_out, b_out, 255]));
        }
    }

    // Drop bands to free memory
    drop(band01);
    drop(band02);
    drop(band03);

    // Parse timestamp: 20260330222000 -> DateTime
    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::info!("GK2A true-color composite complete ({}x{})", width, height);

    Ok((composite, image_time))
}

/// Fetch Himawari image from SLIDER (fallback when NICT is unavailable)
/// Himawari has bands: band_01=Blue, band_02=Green, band_03=Red (like GK2A)
async fn fetch_himawari_image_slider(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let target_size = 2200; // Match NICT level 4

    let (timestamp, date_path) = fetch_slider_timestamp(client, "himawari", "band_03").await?;

    tracing::info!("Fetching Himawari-9 from SLIDER (timestamp: {})...", timestamp);

    // Fetch bands sequentially to reduce peak memory usage
    tracing::info!("  Fetching Himawari band 1/3 (blue)...");
    let band01 = fetch_slider_band(client, "himawari", "band_01", timestamp, &date_path, target_size, HIMAWARI_SLIDER_TILE_SIZE).await?;
    tracing::info!("  Fetching Himawari band 2/3 (green)...");
    let band02 = fetch_slider_band(client, "himawari", "band_02", timestamp, &date_path, target_size, HIMAWARI_SLIDER_TILE_SIZE).await?;
    tracing::info!("  Fetching Himawari band 3/3 (red)...");
    let band03 = fetch_slider_band(client, "himawari", "band_03", timestamp, &date_path, target_size, HIMAWARI_SLIDER_TILE_SIZE).await?;

    tracing::info!("Compositing Himawari true-color...");

    let width = band03.width();
    let height = band03.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band03.get_pixel(x, y).0[0];
            let g = band02.get_pixel(x, y).0[0];
            let b = band01.get_pixel(x, y).0[0];

            let gamma = 1.0 / 1.1;
            let r_out = (255.0 * (r as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let g_out = (255.0 * (g as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let b_out = (255.0 * (b as f32 / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;

            composite.put_pixel(x, y, image::Rgba([r_out, g_out, b_out, 255]));
        }
    }

    // Drop bands to free memory
    drop(band01);
    drop(band02);
    drop(band03);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::info!("Himawari SLIDER composite complete ({}x{})", width, height);

    Ok((composite, image_time))
}

/// Fetch GOES satellite via SLIDER (faster than NOAA CDN ZIP files)
/// GOES bands: band_01=Blue, band_02=Red, band_03=Veggie
/// Green is synthesized: G = 0.45*R + 0.10*Veggie + 0.45*B
async fn fetch_goes_image_slider(
    client: &reqwest::Client,
    slider_sat: &str,
    name: &str,
) -> Result<(RgbaImage, DateTime<Utc>)> {
    // Use 2200px target to match Himawari NICT (reduces memory vs 2712)
    let target_size = 2200;

    let (timestamp, date_path) = fetch_slider_timestamp(client, slider_sat, "band_02").await?;

    tracing::info!(
        "Fetching {} from SLIDER (timestamp: {})...",
        name, timestamp
    );

    // Fetch bands sequentially to reduce peak memory usage
    tracing::info!("  Fetching {} band 1/3 (blue)...", name);
    let band01 = fetch_slider_band(client, slider_sat, "band_01", timestamp, &date_path, target_size, GOES_SLIDER_TILE_SIZE).await?;
    tracing::info!("  Fetching {} band 2/3 (red)...", name);
    let band02 = fetch_slider_band(client, slider_sat, "band_02", timestamp, &date_path, target_size, GOES_SLIDER_TILE_SIZE).await?;
    tracing::info!("  Fetching {} band 3/3 (veggie)...", name);
    let band03 = fetch_slider_band(client, slider_sat, "band_03", timestamp, &date_path, target_size, GOES_SLIDER_TILE_SIZE).await?;

    tracing::info!("Compositing {} true-color...", name);

    let width = band02.width();
    let height = band02.height();
    let mut composite = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let r = band02.get_pixel(x, y).0[0] as f32;
            let veggie = band03.get_pixel(x, y).0[0] as f32;
            let b = band01.get_pixel(x, y).0[0] as f32;

            // Synthesize green channel using standard GOES true-color formula
            let g = 0.45 * r + 0.10 * veggie + 0.45 * b;

            // Apply gamma correction for better visual appearance
            let gamma = 1.0 / 1.1;
            let r_out = (255.0 * (r / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let g_out = (255.0 * (g / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;
            let b_out = (255.0 * (b / 255.0).powf(gamma)).clamp(0.0, 255.0) as u8;

            composite.put_pixel(x, y, image::Rgba([r_out, g_out, b_out, 255]));
        }
    }

    // Drop bands to free memory before returning
    drop(band01);
    drop(band02);
    drop(band03);

    let ts_str = timestamp.to_string();
    let image_time = NaiveDateTime::parse_from_str(&ts_str, "%Y%m%d%H%M%S")
        .context("Failed to parse SLIDER timestamp")?
        .and_utc();

    tracing::info!("{} SLIDER composite complete ({}x{})", name, width, height);

    Ok((composite, image_time))
}

async fn fetch_goes_east_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_image_slider(client, "goes-19", "GOES-East").await
}

async fn fetch_goes_west_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_image_slider(client, "goes-18", "GOES-West").await
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
    }
}

/// Get cache filename for a satellite
pub fn cache_filename(satellite: Satellite) -> &'static str {
    match satellite {
        Satellite::Himawari => "earth_cache_himawari.png",
        Satellite::GoesEast => "earth_cache_goes_east.png",
        Satellite::GoesWest => "earth_cache_goes_west.png",
        Satellite::Gk2a => "earth_cache_gk2a.png",
    }
}
