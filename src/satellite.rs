//! Multi-satellite support for geostationary Earth imagery
//!
//! Supports fetching full-disk Earth images from various geostationary satellites:
//! - Himawari-9 (140.7°E) - Japan/Asia-Pacific  
//! - GOES-East/GOES-19 (75.2°W) - Americas/Atlantic
//! - GOES-West/GOES-18 (137.2°W) - Pacific/West Americas

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
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
}

impl Satellite {
    /// Satellite's geostationary longitude in degrees (East positive)
    pub fn longitude(&self) -> f64 {
        match self {
            Satellite::Himawari => 140.7,
            Satellite::GoesEast => -75.2,
            Satellite::GoesWest => -137.2,
        }
    }

    /// Short name for tray menu
    pub fn name(&self) -> &'static str {
        match self {
            Satellite::Himawari => "Himawari-9",
            Satellite::GoesEast => "GOES-East",
            Satellite::GoesWest => "GOES-West",
        }
    }

    /// Get next satellite in rotation
    pub fn next(&self) -> Self {
        match self {
            Satellite::Himawari => Satellite::GoesEast,
            Satellite::GoesEast => Satellite::GoesWest,
            Satellite::GoesWest => Satellite::Himawari,
        }
    }

    /// All available satellites
    pub fn all() -> &'static [Satellite] {
        &[
            Satellite::Himawari,
            Satellite::GoesEast,
            Satellite::GoesWest,
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

async fn fetch_himawari_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    let metadata = fetch_himawari_metadata(client).await?;
    let date_path = himawari_date_to_path(&metadata.date);
    let level = HimawariLevel::Level4;
    let grid_size = level.grid_size();
    let total_size = level.total_pixels();

    tracing::info!(
        "Fetching Himawari-9: {} ({}x{} tiles)",
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

// ============================================================================
// GOES fetching (high-res GeoColor images without watermarks)
// ============================================================================

const MAX_IMAGE_SIZE: usize = 50 * 1024 * 1024; // 50 MB max for high-res images
const GOES_IMAGE_SIZE: u32 = 10848; // High-res bands available in ZIP format

/// Fetch a single GOES band from ZIP file (clean, no watermarks)
async fn fetch_goes_band_zip(
    client: &reqwest::Client,
    satellite_id: &str,
    band: u8,
) -> Result<image::GrayImage> {
    let url = format!(
        "https://cdn.star.nesdis.noaa.gov/{}/ABI/FD/{:02}/{}x{}.jpg.zip",
        satellite_id,
        band,
        GOES_IMAGE_SIZE,
        GOES_IMAGE_SIZE
    );

    tracing::debug!("Fetching band {:02} from ZIP...", band);

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .with_context(|| format!("Failed to fetch band {:02}", band))?;

    if let Some(len) = response.content_length() {
        if len as usize > MAX_IMAGE_SIZE {
            anyhow::bail!("Band {:02} ZIP too large: {} bytes", band, len);
        }
    }

    let bytes = response.bytes().await?;
    
    // Extract JPEG from ZIP
    let cursor = std::io::Cursor::new(bytes.as_ref());
    let mut archive = zip::ZipArchive::new(cursor)
        .with_context(|| format!("Failed to open band {:02} ZIP", band))?;
    
    if archive.len() != 1 {
        anyhow::bail!("Unexpected ZIP structure for band {:02}", band);
    }
    
    let mut file = archive.by_index(0)
        .with_context(|| format!("Failed to read band {:02} from ZIP", band))?;
    
    let mut img_bytes = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut img_bytes)?;
    
    let img = image::load_from_memory(&img_bytes)
        .with_context(|| format!("Failed to decode band {:02}", band))?;

    Ok(img.to_luma8())
}

/// Create true-color composite from GOES ABI bands (clean, no watermarks)
/// Uses Band 02 (Red), Band 03 (Veggie), Band 01 (Blue)
/// Green is synthesized: G = 0.45*R + 0.10*Veggie + 0.45*B
async fn fetch_goes_truecolor(
    client: &reqwest::Client,
    satellite_id: &str,
    name: &str,
) -> Result<(RgbaImage, DateTime<Utc>)> {
    tracing::info!("Fetching {} true-color bands ({}x{})...", name, GOES_IMAGE_SIZE, GOES_IMAGE_SIZE);

    // Fetch all three bands in parallel
    let (band01, band02, band03) = tokio::try_join!(
        fetch_goes_band_zip(client, satellite_id, 1),  // Blue
        fetch_goes_band_zip(client, satellite_id, 2),  // Red  
        fetch_goes_band_zip(client, satellite_id, 3),  // Veggie (pseudo-green)
    )?;

    tracing::info!("Compositing {} true-color from bands...", name);

    let width = band02.width();
    let height = band02.height();

    // Create RGBA composite
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

    let timestamp = Utc::now();
    tracing::info!("{} true-color composite complete", name);

    Ok((composite, timestamp))
}

async fn fetch_goes_east_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_truecolor(client, "GOES19", "GOES-East").await
}

async fn fetch_goes_west_image(client: &reqwest::Client) -> Result<(RgbaImage, DateTime<Utc>)> {
    fetch_goes_truecolor(client, "GOES18", "GOES-West").await
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
    }
}

/// Get cache filename for a satellite
pub fn cache_filename(satellite: Satellite) -> &'static str {
    match satellite {
        Satellite::Himawari => "earth_cache_himawari.png",
        Satellite::GoesEast => "earth_cache_goes_east.png",
        Satellite::GoesWest => "earth_cache_goes_west.png",
    }
}
