//! Live Earth Wallpaper
//!
//! A native Windows application that displays live satellite imagery of Earth
//! with an accurate star field as your desktop wallpaper.

mod astronomy;
mod himawari;
mod monitor;
mod renderer;
mod tray;
mod wallpaper;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use monitor::MultiMonitorMode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Update interval in minutes
const UPDATE_INTERVAL_MINUTES: u64 = 10;

/// Himawari-8 image resolution level
const IMAGE_LEVEL: himawari::ImageLevel = himawari::ImageLevel::Level4;

fn main() -> Result<()> {
    // Enable per-monitor DPI awareness for accurate high-DPI rendering
    // Must be called before any window/GUI operations
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        // Ignore errors - falls back to system DPI awareness on older Windows
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Live Earth Wallpaper v{}", env!("CARGO_PKG_VERSION"));

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let duplicate_mode = args.contains(&"--duplicate".to_string());
    let initial_mode = if duplicate_mode {
        MultiMonitorMode::Duplicate
    } else {
        MultiMonitorMode::Span
    };

    // Check for --update-once flag for testing
    if args.contains(&"--update-once".to_string()) {
        tracing::info!("Running single update (--update-once mode, {:?})", initial_mode);
        return run_single_update(initial_mode);
    }

    // Run with system tray
    run_with_tray(initial_mode)
}

fn run_single_update(mode: MultiMonitorMode) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(update_wallpaper_with_mode(mode))
}

#[cfg(windows)]
fn run_with_tray(initial_mode: MultiMonitorMode) -> Result<()> {
    use tray::{startup, TrayCommand, TrayIcon};
    use winit::event_loop::{ControlFlow, EventLoop};

    // Current mode (mutable)
    let mut current_mode = initial_mode;

    // Check current startup state
    let startup_enabled = startup::is_enabled();
    tracing::info!("Run on startup: {}", if startup_enabled { "enabled" } else { "disabled" });
    tracing::info!("Monitor mode: {:?}", current_mode);

    // Create tray icon
    let tray = TrayIcon::new(startup_enabled, current_mode)?;
    tracing::info!("System tray icon created");

    // Create async runtime
    let rt = tokio::runtime::Runtime::new()?;

    // Shutdown flag
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    // Initial update
    tracing::info!("Performing initial wallpaper update...");
    if let Err(e) = rt.block_on(update_wallpaper_with_mode(current_mode)) {
        tracing::error!("Initial update failed: {}", e);
    }

    // Create event loop for Windows message pump (required for tray)
    let event_loop = EventLoop::new()?;
    
    let mut last_update = std::time::Instant::now();
    let update_interval = Duration::from_secs(UPDATE_INTERVAL_MINUTES * 60);

    tracing::info!(
        "Wallpaper will update every {} minutes. Use tray icon to control.",
        UPDATE_INTERVAL_MINUTES
    );

    event_loop.run(move |_event, elwt| {
        elwt.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_millis(100)
        ));

        // Check for tray commands
        if let Some(cmd) = tray.poll_command() {
            match cmd {
                TrayCommand::RefreshNow => {
                    tracing::info!("Manual refresh requested");
                    if let Err(e) = rt.block_on(update_wallpaper_with_mode(current_mode)) {
                        tracing::error!("Refresh failed: {}", e);
                    }
                    last_update = std::time::Instant::now();
                }
                TrayCommand::ToggleMode => {
                    current_mode = match current_mode {
                        MultiMonitorMode::Span => MultiMonitorMode::Duplicate,
                        MultiMonitorMode::Duplicate => MultiMonitorMode::Span,
                    };
                    tray.set_mode(current_mode);
                    tracing::info!("Switched to {:?} mode (will apply on next refresh)", current_mode);
                }
                TrayCommand::ToggleStartup => {
                    match startup::toggle() {
                        Ok(enabled) => {
                            tray.set_startup(enabled);
                            tracing::info!(
                                "Run on startup {}",
                                if enabled { "enabled" } else { "disabled" }
                            );
                        }
                        Err(e) => {
                            tracing::error!("Failed to toggle startup: {}", e);
                        }
                    }
                }
                TrayCommand::Exit => {
                    tracing::info!("Exit requested from tray");
                    running.store(false, Ordering::SeqCst);
                    elwt.exit();
                    return;
                }
            }
        }

        // Check for scheduled update
        if last_update.elapsed() >= update_interval {
            tracing::info!("Scheduled update starting...");
            if let Err(e) = rt.block_on(update_wallpaper_with_mode(current_mode)) {
                tracing::error!("Scheduled update failed: {}", e);
            }
            last_update = std::time::Instant::now();
        }

        // Check for Ctrl+C
        if !running.load(Ordering::SeqCst) {
            tracing::info!("Shutting down...");
            elwt.exit();
        }
    })?;

    Ok(())
}

#[cfg(not(windows))]
fn run_with_tray(initial_mode: MultiMonitorMode) -> Result<()> {
    use tokio::time::interval;

    let rt = tokio::runtime::Runtime::new()?;
    
    rt.block_on(async {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();

        // Handle Ctrl+C on Unix
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            r.store(false, Ordering::SeqCst);
        });

        tracing::info!(
            "Wallpaper will update every {} minutes. Press Ctrl+C to exit.",
            UPDATE_INTERVAL_MINUTES
        );

        // Initial update
        if let Err(e) = update_wallpaper_with_mode(initial_mode).await {
            tracing::error!("Initial update failed: {}", e);
        }

        let mut timer = interval(Duration::from_secs(UPDATE_INTERVAL_MINUTES * 60));
        timer.tick().await;

        while running.load(Ordering::SeqCst) {
            timer.tick().await;
            
            if !running.load(Ordering::SeqCst) {
                break;
            }

            tracing::info!("Scheduled update starting...");
            if let Err(e) = update_wallpaper_with_mode(initial_mode).await {
                tracing::error!("Scheduled update failed: {}", e);
            }
        }

        tracing::info!("Shutting down...");
        Ok(())
    })
}

async fn update_wallpaper() -> Result<()> {
    update_wallpaper_with_mode(monitor::MultiMonitorMode::Span).await
}

async fn update_wallpaper_with_mode(mode: monitor::MultiMonitorMode) -> Result<()> {
    let start = std::time::Instant::now();
    
    // Detect monitors
    let layout = monitor::MonitorLayout::detect()
        .context("Failed to detect monitors")?;
    
    tracing::info!(
        "Rendering for {}x{} desktop ({} monitor(s), {:?} mode)",
        layout.total_width,
        layout.total_height,
        layout.monitors.len(),
        mode
    );

    // Create HTTP client
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("Failed to create HTTP client")?;

    // Try to fetch Earth image, fall back to cached if available
    let (mut earth_image, timestamp, is_cached) = 
        match fetch_earth_with_fallback(&client).await {
            Ok(result) => result,
            Err(e) => {
                tracing::error!("Failed to fetch Earth image: {}", e);
                return Err(e);
            }
        };

    // If using cached image, convert to grayscale to indicate stale data
    if is_cached {
        tracing::info!("Using cached image - converting to grayscale");
        earth_image = convert_to_grayscale(&earth_image);
    }

    tracing::info!(
        "Earth image: {}x{} from {}{}",
        earth_image.width(),
        earth_image.height(),
        timestamp.format("%Y-%m-%d %H:%M UTC"),
        if is_cached { " (cached)" } else { "" }
    );

    // Render composite
    tracing::info!("Rendering wallpaper...");
    let mut renderer = renderer::Renderer::new();
    let wallpaper_image = renderer
        .render(&earth_image, &layout, mode, &timestamp)
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

/// Convert an RGBA image to grayscale (preserving alpha)
fn convert_to_grayscale(image: &image::RgbaImage) -> image::RgbaImage {
    let mut gray = image.clone();
    for pixel in gray.pixels_mut() {
        // Standard luminance formula
        let luma = (0.299 * pixel[0] as f32 
                  + 0.587 * pixel[1] as f32 
                  + 0.114 * pixel[2] as f32) as u8;
        pixel[0] = luma;
        pixel[1] = luma;
        pixel[2] = luma;
        // Keep alpha unchanged
    }
    gray
}

/// Fetch Earth image with fallback to cached version
/// Returns (image, timestamp, is_cached)
async fn fetch_earth_with_fallback(
    client: &reqwest::Client,
) -> Result<(image::RgbaImage, DateTime<Utc>, bool)> {
    
    // Try to fetch fresh image
    tracing::info!("Fetching Himawari-8 satellite image...");
    match himawari::fetch_earth_image(client, IMAGE_LEVEL).await {
        Ok((earth_image, timestamp)) => {
            // Cache the successful fetch
            if let Err(e) = cache_earth_image(&earth_image, &timestamp) {
                tracing::warn!("Failed to cache Earth image: {}", e);
            }
            Ok((earth_image, timestamp, false))
        }
        Err(e) => {
            tracing::warn!("Failed to fetch fresh image: {}", e);
            tracing::info!("Attempting to use cached image...");
            
            // Try to load cached image
            let (image, timestamp) = load_cached_earth_image()
                .context("No cached image available and fetch failed")?;
            Ok((image, timestamp, true))
        }
    }
}

/// Cache the Earth image for fallback
fn cache_earth_image(
    image: &image::RgbaImage,
    timestamp: &DateTime<Utc>,
) -> Result<()> {
    let cache_dir = wallpaper::wallpaper_dir()?;
    let cache_path = cache_dir.join("cached_earth.png");
    let meta_path = cache_dir.join("cached_earth.txt");
    
    image.save(&cache_path)?;
    std::fs::write(&meta_path, timestamp.to_rfc3339())?;
    
    tracing::debug!("Cached Earth image to {}", cache_path.display());
    Ok(())
}

/// Load cached Earth image
fn load_cached_earth_image() -> Result<(image::RgbaImage, DateTime<Utc>)> {
    let cache_dir = wallpaper::wallpaper_dir()?;
    let cache_path = cache_dir.join("cached_earth.png");
    let meta_path = cache_dir.join("cached_earth.txt");
    
    if !cache_path.exists() {
        anyhow::bail!("No cached image found");
    }
    
    let image = image::open(&cache_path)
        .context("Failed to load cached image")?
        .to_rgba8();
    
    let timestamp = if meta_path.exists() {
        let ts_str = std::fs::read_to_string(&meta_path)?;
        chrono::DateTime::parse_from_rfc3339(ts_str.trim())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now())
    } else {
        Utc::now()
    };
    
    tracing::info!("Using cached Earth image from {}", timestamp.format("%Y-%m-%d %H:%M UTC"));
    Ok((image, timestamp))
}
