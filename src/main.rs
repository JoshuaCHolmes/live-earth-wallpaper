//! Live Earth Wallpaper
//!
//! A native Windows application that displays live satellite imagery of Earth
//! with an accurate star field as your desktop wallpaper.

// Hide console window on Windows (GUI subsystem)
#![windows_subsystem = "windows"]

mod astronomy;
mod himawari;
mod monitor;
mod moon_texture;
mod renderer;
mod tray;
mod wallpaper;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use monitor::MultiMonitorMode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Full update interval (fetch new Earth image) in minutes
const FULL_UPDATE_INTERVAL_MINUTES: u64 = 10;

/// Star-only refresh interval in seconds (uses cached Earth image)
const STAR_REFRESH_INTERVAL_SECS: u64 = 60;

/// Himawari-8 image resolution level
const IMAGE_LEVEL: himawari::ImageLevel = himawari::ImageLevel::Level4;

fn main() -> Result<()> {
    // Ensure single instance using a named mutex
    #[cfg(windows)]
    let _mutex = {
        use windows::core::PCSTR;
        use windows::Win32::Foundation::GetLastError;
        use windows::Win32::System::Threading::CreateMutexA;
        
        let mutex_name = b"Global\\LiveEarthWallpaper\0";
        let mutex = unsafe { CreateMutexA(None, true, PCSTR(mutex_name.as_ptr())) };
        
        if let Ok(handle) = mutex {
            // Check if mutex already existed (another instance is running)
            if unsafe { GetLastError() } == windows::Win32::Foundation::ERROR_ALREADY_EXISTS {
                // Another instance is running - exit silently
                return Ok(());
            }
            Some(handle)
        } else {
            None // Couldn't create mutex, continue anyway
        }
    };

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
    use winit::event::Event;
    use winit::event_loop::{ControlFlow, EventLoop};

    // Current mode (mutable)
    let mut current_mode = initial_mode;

    // Check current startup state
    let startup_enabled = startup::is_enabled();
    tracing::info!("Run on startup: {}", if startup_enabled { "enabled" } else { "disabled" });
    tracing::info!("Monitor mode: {:?}", current_mode);

    // Labels state
    let mut show_labels = false;

    // Create tray icon
    let tray = TrayIcon::new(startup_enabled, current_mode, show_labels)?;
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

    // Track current monitor layout for change detection
    let mut current_layout = monitor::MonitorLayout::detect()
        .map(|l| (l.monitors.len(), l.total_width, l.total_height))
        .unwrap_or((1, 1920, 1080));

    // Initial update (full, with Earth fetch)
    tracing::info!("Performing initial wallpaper update...");
    let mut cached_earth: Option<(image::RgbaImage, chrono::DateTime<Utc>)> = None;
    let mut is_stale = false;
    match rt.block_on(fetch_and_update_wallpaper(current_mode, show_labels)) {
        Ok((earth_img, timestamp, stale)) => {
            cached_earth = Some((earth_img, timestamp));
            is_stale = stale;
        }
        Err(e) => {
            tracing::error!("Initial update failed: {}", e);
            is_stale = true;
        }
    }

    // Create event loop for Windows message pump (required for tray)
    let event_loop = EventLoop::new()?;
    
    let mut last_full_update = std::time::Instant::now();
    let mut last_star_refresh = std::time::Instant::now();
    let full_update_interval = Duration::from_secs(FULL_UPDATE_INTERVAL_MINUTES * 60);
    let star_refresh_interval = Duration::from_secs(STAR_REFRESH_INTERVAL_SECS);

    tracing::info!(
        "Full updates every {} min, star refresh every {} sec.",
        FULL_UPDATE_INTERVAL_MINUTES,
        STAR_REFRESH_INTERVAL_SECS
    );

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(ControlFlow::WaitUntil(
            std::time::Instant::now() + Duration::from_millis(500)
        ));

        // Check for display configuration changes (monitor added/removed/resized)
        if matches!(event, Event::UserEvent(_) | Event::MemoryWarning | Event::Resumed) {
            // These events can indicate system state changes, check monitor layout
        }
        
        // Periodically check if monitor configuration changed (every poll cycle)
        // This catches WM_DISPLAYCHANGE indirectly via layout detection
        if let Ok(new_layout) = monitor::MonitorLayout::detect() {
            let new_state = (new_layout.monitors.len(), new_layout.total_width, new_layout.total_height);
            if new_state != current_layout {
                tracing::info!(
                    "Monitor configuration changed: {} monitor(s), {}x{} -> {} monitor(s), {}x{}",
                    current_layout.0, current_layout.1, current_layout.2,
                    new_state.0, new_state.1, new_state.2
                );
                current_layout = new_state;
                
                // Re-render with new monitor configuration
                if let Some((ref earth_img, ref timestamp)) = cached_earth {
                    tracing::info!("Re-rendering wallpaper for new display configuration...");
                    if let Err(e) = rt.block_on(render_with_cached_earth(earth_img, timestamp, current_mode, show_labels)) {
                        tracing::error!("Display change re-render failed: {}", e);
                    }
                }
            }
        }

        // Check for tray commands
        if let Some(cmd) = tray.poll_command() {
            match cmd {
                TrayCommand::RefreshNow => {
                    tracing::info!("Manual refresh requested");
                    match rt.block_on(fetch_and_update_wallpaper(current_mode, show_labels)) {
                        Ok((earth_img, timestamp, stale)) => {
                            cached_earth = Some((earth_img, timestamp));
                            is_stale = stale;
                        }
                        Err(e) => {
                            tracing::error!("Refresh failed: {}", e);
                            is_stale = true;
                        }
                    }
                    last_full_update = std::time::Instant::now();
                    last_star_refresh = std::time::Instant::now();
                }
                TrayCommand::ToggleMode => {
                    current_mode = match current_mode {
                        MultiMonitorMode::Span => MultiMonitorMode::Duplicate,
                        MultiMonitorMode::Duplicate => MultiMonitorMode::Span,
                    };
                    tray.set_mode(current_mode);
                    tracing::info!("Switched to {:?} mode", current_mode);
                    // Immediate refresh to apply mode change
                    if let Some((ref earth_img, ref timestamp)) = cached_earth {
                        if let Err(e) = rt.block_on(render_with_cached_earth(earth_img, timestamp, current_mode, show_labels)) {
                            tracing::error!("Mode switch refresh failed: {}", e);
                        }
                    }
                }
                TrayCommand::ToggleLabels => {
                    show_labels = !show_labels;
                    tray.set_labels(show_labels);
                    tracing::info!("Labels {}", if show_labels { "enabled" } else { "disabled" });
                    // Immediate refresh to show/hide labels
                    if let Some((ref earth_img, ref timestamp)) = cached_earth {
                        if let Err(e) = rt.block_on(render_with_cached_earth(earth_img, timestamp, current_mode, show_labels)) {
                            tracing::error!("Label refresh failed: {}", e);
                        }
                    }
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

        // Check for full scheduled update (fetch new Earth image)
        if last_full_update.elapsed() >= full_update_interval {
            tracing::info!("Scheduled full update starting...");
            match rt.block_on(fetch_and_update_wallpaper(current_mode, show_labels)) {
                Ok((earth_img, timestamp, stale)) => {
                    cached_earth = Some((earth_img, timestamp));
                    is_stale = stale;
                }
                Err(e) => {
                    tracing::error!("Scheduled update failed: {}", e);
                    is_stale = true;
                }
            }
            last_full_update = std::time::Instant::now();
            last_star_refresh = std::time::Instant::now();
        }
        // Check for star-only refresh (use cached Earth) - retry full fetch if stale
        else if last_star_refresh.elapsed() >= star_refresh_interval {
            if is_stale {
                // We're using stale data - try to fetch fresh Earth image
                tracing::info!("Stale data detected, attempting to fetch fresh Earth image...");
                match rt.block_on(fetch_and_update_wallpaper(current_mode, show_labels)) {
                    Ok((earth_img, timestamp, stale)) => {
                        cached_earth = Some((earth_img, timestamp));
                        is_stale = stale;
                        if !stale {
                            tracing::info!("Successfully recovered from stale state!");
                        }
                    }
                    Err(e) => {
                        tracing::debug!("Still unable to fetch fresh image: {}", e);
                        // Fall back to star refresh with cached (grayscale) image
                        if let Some((ref earth_img, ref timestamp)) = cached_earth {
                            if let Err(e) = rt.block_on(render_with_cached_earth(earth_img, timestamp, current_mode, show_labels)) {
                                tracing::error!("Star refresh failed: {}", e);
                            }
                        }
                    }
                }
            } else if let Some((ref earth_img, ref timestamp)) = cached_earth {
                tracing::info!("Star refresh (cached Earth from {})...", timestamp.format("%H:%M UTC"));
                if let Err(e) = rt.block_on(render_with_cached_earth(earth_img, timestamp, current_mode, show_labels)) {
                    tracing::error!("Star refresh failed: {}", e);
                }
            }
            last_star_refresh = std::time::Instant::now();
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
    let rt = tokio::runtime::Runtime::new()?;
    
    rt.block_on(async {
        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let show_labels = false; // No tray on non-Windows, default off

        // Handle Ctrl+C on Unix
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            r.store(false, Ordering::SeqCst);
        });

        tracing::info!(
            "Full updates every {} min, star refresh every {} sec.",
            FULL_UPDATE_INTERVAL_MINUTES,
            STAR_REFRESH_INTERVAL_SECS
        );

        // Initial update
        let mut cached_earth: Option<(image::RgbaImage, DateTime<Utc>)> = None;
        let mut is_stale = false;
        match fetch_and_update_wallpaper(initial_mode, show_labels).await {
            Ok((earth_img, timestamp, stale)) => {
                cached_earth = Some((earth_img, timestamp));
                is_stale = stale;
            }
            Err(e) => {
                tracing::error!("Initial update failed: {}", e);
                is_stale = true;
            }
        }

        let mut full_update_timer = tokio::time::interval(Duration::from_secs(FULL_UPDATE_INTERVAL_MINUTES * 60));
        let mut star_refresh_timer = tokio::time::interval(Duration::from_secs(STAR_REFRESH_INTERVAL_SECS));
        full_update_timer.tick().await;
        star_refresh_timer.tick().await;

        loop {
            tokio::select! {
                _ = full_update_timer.tick() => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    tracing::info!("Scheduled full update starting...");
                    match fetch_and_update_wallpaper(initial_mode, show_labels).await {
                        Ok((earth_img, timestamp, stale)) => {
                            cached_earth = Some((earth_img, timestamp));
                            is_stale = stale;
                        }
                        Err(e) => {
                            tracing::error!("Scheduled update failed: {}", e);
                            is_stale = true;
                        }
                    }
                }
                _ = star_refresh_timer.tick() => {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    if is_stale {
                        // Try to recover from stale state
                        match fetch_and_update_wallpaper(initial_mode, show_labels).await {
                            Ok((earth_img, timestamp, stale)) => {
                                cached_earth = Some((earth_img, timestamp));
                                is_stale = stale;
                            }
                            Err(_) => {
                                // Keep using cached image
                                if let Some((ref earth_img, ref timestamp)) = cached_earth {
                                    let _ = render_with_cached_earth(earth_img, timestamp, initial_mode, show_labels).await;
                                }
                            }
                        }
                    } else if let Some((ref earth_img, ref timestamp)) = cached_earth {
                        tracing::debug!("Star refresh...");
                        if let Err(e) = render_with_cached_earth(earth_img, timestamp, initial_mode, show_labels).await {
                            tracing::error!("Star refresh failed: {}", e);
                        }
                    }
                }
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

/// Fetch Earth image and update wallpaper, returning the Earth image for caching
/// Fetch and update wallpaper, returning (earth_image, timestamp, is_stale)
async fn fetch_and_update_wallpaper(
    mode: monitor::MultiMonitorMode,
    show_labels: bool,
) -> Result<(image::RgbaImage, DateTime<Utc>, bool)> {
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
    let (mut earth_image, timestamp, is_cached) = fetch_earth_with_fallback(&client).await?;

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

    // Render composite with current time for accurate star positions
    let render_time = Utc::now();
    let mut renderer = renderer::Renderer::new();
    renderer.set_show_labels(show_labels);
    let wallpaper_image = renderer
        .render(&earth_image, &layout, mode, &render_time)
        .context("Failed to render wallpaper")?;

    // Save and set wallpaper
    let wallpaper_dir = wallpaper::wallpaper_dir()?;
    let wallpaper_path = wallpaper_dir.join("current_wallpaper.png");
    wallpaper_image.save(&wallpaper_path).context("Failed to save wallpaper")?;
    wallpaper::set_wallpaper(&wallpaper_path).context("Failed to set wallpaper")?;

    let elapsed = start.elapsed();
    tracing::info!("Full update complete in {:.1}s", elapsed.as_secs_f64());

    // Return the original (non-grayscale) earth image for caching, plus stale flag
    // Re-fetch if it was cached (grayscale), otherwise use what we have
    if is_cached {
        // Load the original cached image (not grayscale)
        let (original, _) = load_cached_earth_image()?;
        Ok((original, timestamp, true))
    } else {
        Ok((earth_image, timestamp, false))
    }
}

/// Render wallpaper using cached Earth image with updated star positions
async fn render_with_cached_earth(
    earth_image: &image::RgbaImage,
    _earth_timestamp: &DateTime<Utc>,
    mode: monitor::MultiMonitorMode,
    show_labels: bool,
) -> Result<()> {
    let start = std::time::Instant::now();
    
    // Detect monitors
    let layout = monitor::MonitorLayout::detect()
        .context("Failed to detect monitors")?;

    // Use current time for star positions
    let render_time = Utc::now();
    
    let mut renderer = renderer::Renderer::new();
    renderer.set_show_labels(show_labels);
    let wallpaper_image = renderer
        .render(earth_image, &layout, mode, &render_time)
        .context("Failed to render wallpaper")?;

    // Save and set wallpaper
    let wallpaper_dir = wallpaper::wallpaper_dir()?;
    let wallpaper_path = wallpaper_dir.join("current_wallpaper.png");
    wallpaper_image.save(&wallpaper_path).context("Failed to save wallpaper")?;
    wallpaper::set_wallpaper(&wallpaper_path).context("Failed to set wallpaper")?;

    let elapsed = start.elapsed();
    tracing::debug!("Star refresh complete in {:.1}ms", elapsed.as_millis());

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
