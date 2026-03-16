//! Live Earth Wallpaper
//!
//! A native Windows application that displays live satellite imagery of Earth
//! with an accurate star field as your desktop wallpaper.

mod astronomy;
mod himawari;
mod monitor;
mod renderer;
mod wallpaper;

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

/// Update interval in minutes
const UPDATE_INTERVAL_MINUTES: u64 = 10;

/// Himawari-8 image resolution level
const IMAGE_LEVEL: himawari::ImageLevel = himawari::ImageLevel::Level4;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Live Earth Wallpaper starting...");

    // Check for --update-once flag for testing
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--update-once".to_string()) {
        tracing::info!("Running single update (--update-once mode)");
        if let Err(e) = update_wallpaper().await {
            tracing::error!("Update failed: {}", e);
            return Err(e);
        }
        return Ok(());
    }

    // Create shutdown signal
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Handle Ctrl+C
    ctrlc_handler(r);

    tracing::info!(
        "Wallpaper will update every {} minutes. Press Ctrl+C to exit.",
        UPDATE_INTERVAL_MINUTES
    );

    // Initial update
    if let Err(e) = update_wallpaper().await {
        tracing::error!("Initial update failed: {}", e);
    }

    // Main loop
    let mut timer = interval(Duration::from_secs(UPDATE_INTERVAL_MINUTES * 60));
    timer.tick().await; // Skip first tick (we just did initial update)

    while running.load(Ordering::SeqCst) {
        timer.tick().await;
        
        if !running.load(Ordering::SeqCst) {
            break;
        }

        tracing::info!("Scheduled update starting...");
        if let Err(e) = update_wallpaper().await {
            tracing::error!("Scheduled update failed: {}", e);
        }
    }

    tracing::info!("Shutting down...");
    Ok(())
}

fn ctrlc_handler(running: Arc<AtomicBool>) {
    #[cfg(unix)]
    {
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            running.store(false, Ordering::SeqCst);
        });
    }
    
    #[cfg(windows)]
    {
        let _ = ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        });
    }
}

async fn update_wallpaper() -> Result<()> {
    let start = std::time::Instant::now();
    
    // Detect monitors
    let layout = monitor::MonitorLayout::detect()
        .context("Failed to detect monitors")?;
    
    tracing::info!(
        "Rendering for {}x{} desktop ({} monitor(s))",
        layout.total_width,
        layout.total_height,
        layout.monitors.len()
    );

    // Create HTTP client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to create HTTP client")?;

    // Fetch Earth image
    tracing::info!("Fetching Himawari-8 satellite image...");
    let (earth_image, timestamp) = himawari::fetch_earth_image(&client, IMAGE_LEVEL)
        .await
        .context("Failed to fetch Earth image")?;

    tracing::info!(
        "Earth image: {}x{} from {}",
        earth_image.width(),
        earth_image.height(),
        timestamp.format("%Y-%m-%d %H:%M UTC")
    );

    // Render composite
    tracing::info!("Rendering wallpaper...");
    let mut renderer = renderer::Renderer::new();
    let wallpaper_image = renderer
        .render(&earth_image, layout.total_width, layout.total_height, &timestamp)
        .context("Failed to render wallpaper")?;

    // Save to file
    let wallpaper_dir = wallpaper::wallpaper_dir()?;
    let wallpaper_path = wallpaper_dir.join("current_wallpaper.png");
    
    wallpaper_image
        .save(&wallpaper_path)
        .context("Failed to save wallpaper image")?;

    tracing::info!("Saved wallpaper to: {}", wallpaper_path.display());

    // Set as wallpaper
    wallpaper::set_wallpaper(&wallpaper_path)
        .context("Failed to set wallpaper")?;

    let elapsed = start.elapsed();
    tracing::info!("Update complete in {:.1}s", elapsed.as_secs_f64());

    Ok(())
}
