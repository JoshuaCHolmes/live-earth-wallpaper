//! Himawari-8 satellite image fetching
//!
//! Fetches full-disk Earth imagery from the Himawari-8 geostationary satellite
//! positioned at 140.7°E longitude.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use image::{DynamicImage, GenericImage, RgbaImage};
use std::time::Duration;

const HIMAWARI_BASE_URL: &str = "https://himawari8-dl.nict.go.jp";
const TILE_SIZE: u32 = 550;

/// Represents the resolution level of Himawari-8 imagery
#[derive(Debug, Clone, Copy)]
pub enum ImageLevel {
    /// 1x1 grid (550px) - fastest
    Level1 = 1,
    /// 2x2 grid (1100px)
    Level2 = 2,
    /// 4x4 grid (2200px)
    Level4 = 4,
    /// 8x8 grid (4400px)
    Level8 = 8,
    /// 16x16 grid (8800px) - highest quality
    Level16 = 16,
    /// 20x20 grid (11000px) - maximum resolution
    Level20 = 20,
}

impl ImageLevel {
    pub fn grid_size(&self) -> u32 {
        *self as u32
    }

    pub fn total_pixels(&self) -> u32 {
        let grid = self.grid_size();
        grid * TILE_SIZE
    }
}

/// Metadata for the latest available Himawari-8 image
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ImageMetadata {
    pub date: String,
    pub file: String,
}

/// Fetches the latest image metadata from Himawari-8
pub async fn fetch_latest_metadata(client: &reqwest::Client) -> Result<ImageMetadata> {
    let url = format!("{}/himawari8/img/FULL_24h/latest.json", HIMAWARI_BASE_URL);

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch Himawari-8 metadata")?;

    let metadata: ImageMetadata = response
        .json()
        .await
        .context("Failed to parse Himawari-8 metadata")?;

    Ok(metadata)
}

/// Converts metadata date string to URL path format
/// "2024-01-15 12:30:00" -> "2024/01/15/123000"
fn date_to_path(date: &str) -> String {
    date.replace(['-', ' '], "/").replace(':', "")
}

/// Fetches a single tile from Himawari-8
async fn fetch_tile(
    client: &reqwest::Client,
    date_path: &str,
    level: ImageLevel,
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

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to download tile ({}, {})", x, y))?;

    let img = image::load_from_memory(&bytes)
        .with_context(|| format!("Failed to decode tile ({}, {})", x, y))?;

    Ok(img)
}

/// Fetches the complete Earth image from Himawari-8
pub async fn fetch_earth_image(
    client: &reqwest::Client,
    level: ImageLevel,
) -> Result<(RgbaImage, DateTime<Utc>)> {
    let metadata = fetch_latest_metadata(client).await?;
    let date_path = date_to_path(&metadata.date);
    let grid_size = level.grid_size();
    let total_size = level.total_pixels();

    tracing::info!(
        "Fetching Himawari-8 image: {} ({}x{} tiles, {}px)",
        metadata.date,
        grid_size,
        grid_size,
        total_size
    );

    let mut composite = RgbaImage::new(total_size, total_size);

    // Fetch all tiles concurrently
    let mut handles = Vec::new();
    for y in 0..grid_size {
        for x in 0..grid_size {
            let client = client.clone();
            let date_path = date_path.clone();
            handles.push(tokio::spawn(async move {
                let tile = fetch_tile(&client, &date_path, level, x, y).await?;
                Ok::<_, anyhow::Error>((x, y, tile))
            }));
        }
    }

    // Collect results and compose image
    for handle in handles {
        let (x, y, tile) = handle.await??;
        let tile_rgba = tile.to_rgba8();
        composite
            .copy_from(&tile_rgba, x * TILE_SIZE, y * TILE_SIZE)
            .with_context(|| format!("Failed to composite tile ({}, {})", x, y))?;
    }

    // Parse the timestamp
    let timestamp = chrono::NaiveDateTime::parse_from_str(&metadata.date, "%Y-%m-%d %H:%M:%S")
        .context("Failed to parse image timestamp")?
        .and_utc();

    tracing::info!("Earth image fetched successfully");
    Ok((composite, timestamp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_to_path() {
        assert_eq!(
            date_to_path("2024-01-15 12:30:00"),
            "2024/01/15/123000"
        );
    }

    #[test]
    fn test_image_level() {
        assert_eq!(ImageLevel::Level4.grid_size(), 4);
        assert_eq!(ImageLevel::Level4.total_pixels(), 2200);
    }
}
