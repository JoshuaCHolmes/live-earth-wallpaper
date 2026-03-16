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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Update interval in minutes
const UPDATE_INTERVAL_MINUTES: u64 = 10;

/// Himawari-8 image resolution level
const IMAGE_LEVEL: himawari::ImageLevel = himawari::ImageLevel::Level4;

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    tracing::info!("Live Earth Wallpaper v{}", env!("CARGO_PKG_VERSION"));

    // Check for --update-once flag for testing
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--update-once".to_string()) {
        tracing::info!("Running single update (--update-once mode)");
        return run_single_update();
    }

    // Run with system tray
    run_with_tray()
}

fn run_single_update() -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(update_wallpaper())
}

#[cfg(windows)]
fn run_with_tray() -> Result<()> {
    use tray::{startup, TrayCommand, TrayIcon};
    use winit::event_loop::{ControlFlow, EventLoop};

    // Check current startup state
    let startup_enabled = startup::is_enabled();
    tracing::info!("Run on startup: {}", if startup_enabled { "enabled" } else { "disabled" });

    // Create tray icon
    let tray = TrayIcon::new(startup_enabled)?;
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
    if let Err(e) = rt.block_on(update_wallpaper()) {
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
                    if let Err(e) = rt.block_on(update_wallpaper()) {
                        tracing::error!("Refresh failed: {}", e);
                    }
                    last_update = std::time::Instant::now();
                }
                TrayCommand::ToggleStartup => {
                    match startup::toggle() {
                        Ok(enabled) => {
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
            if let Err(e) = rt.block_on(update_wallpaper()) {
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
fn run_with_tray() -> Result<()> {
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
        if let Err(e) = update_wallpaper().await {
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
            if let Err(e) = update_wallpaper().await {
                tracing::error!("Scheduled update failed: {}", e);
            }
        }

        tracing::info!("Shutting down...");
        Ok(())
    })
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
