//! System tray icon and menu for Windows
//!
//! Provides a minimal tray interface with:
//! - Refresh Now
//! - Satellite selection
//! - Toggle Mode (Span/Duplicate)
//! - Run on Startup (toggle)
//! - Exit

use crate::monitor::MultiMonitorMode;
use crate::satellite::Satellite;

/// Commands that can be triggered from the tray menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    RefreshNow,
    ToggleMode,
    ToggleEarth,
    ToggleLabels,
    ToggleStartup,
    SelectSatellite(Satellite),
    Exit,
}

#[cfg(windows)]
use std::sync::mpsc::{self, Receiver, Sender};

#[cfg(windows)]
use tray_icon::menu::MenuItem;

#[cfg(windows)]
pub struct TrayIcon {
    _tray: tray_icon::TrayIcon,
    menu_channel: Receiver<TrayCommand>,
    mode_item: MenuItem,
    earth_item: MenuItem,
    satellite_items: Vec<MenuItem>,
    labels_item: MenuItem,
    startup_item: MenuItem,
}

#[cfg(windows)]
impl TrayIcon {
    pub fn new(startup_enabled: bool, mode: MultiMonitorMode, labels_enabled: bool, earth_enabled: bool, current_satellite: Satellite) -> anyhow::Result<Self> {
        use anyhow::Context;
        use tray_icon::menu::{Menu, MenuEvent, PredefinedMenuItem, Submenu};
        use tray_icon::TrayIconBuilder;

        // Create menu items
        let menu = Menu::new();
        
        let refresh_item = MenuItem::with_id("refresh", "Refresh Now", true, None);
        let mode_item = MenuItem::with_id("mode", Self::mode_label(mode), true, None);
        let earth_item = MenuItem::with_id("earth", Self::earth_label(earth_enabled), true, None);
        
        // Satellite submenu
        let satellite_menu = Submenu::new("Satellite", true);
        let mut satellite_items = Vec::new();
        for sat in Satellite::all() {
            let label = Self::satellite_label(*sat, current_satellite);
            let item = MenuItem::with_id(
                format!("sat_{}", sat.name().to_lowercase().replace("-", "_")),
                label,
                true,
                None,
            );
            satellite_menu.append(&item)?;
            satellite_items.push(item);
        }
        
        let labels_item = MenuItem::with_id("labels", Self::labels_label(labels_enabled), true, None);
        let startup_item = MenuItem::with_id("startup", Self::startup_label(startup_enabled), true, None);
        let separator = PredefinedMenuItem::separator();
        let exit_item = MenuItem::with_id("exit", "Exit", true, None);

        menu.append(&refresh_item)?;
        menu.append(&mode_item)?;
        menu.append(&earth_item)?;
        menu.append(&satellite_menu)?;
        menu.append(&labels_item)?;
        menu.append(&startup_item)?;
        menu.append(&separator)?;
        menu.append(&exit_item)?;

        // Create icon (embedded 16x16 blue earth icon)
        let icon = create_icon()?;

        // Build tray icon
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Live Earth Wallpaper")
            .with_icon(icon)
            .build()
            .context("Failed to create tray icon")?;

        // Set up menu event channel
        let (tx, rx): (Sender<TrayCommand>, Receiver<TrayCommand>) = mpsc::channel();
        
        // Spawn thread to handle menu events
        let menu_rx = MenuEvent::receiver();
        std::thread::spawn(move || {
            loop {
                if let Ok(event) = menu_rx.recv() {
                    let cmd = match event.id.0.as_str() {
                        "refresh" => Some(TrayCommand::RefreshNow),
                        "mode" => Some(TrayCommand::ToggleMode),
                        "earth" => Some(TrayCommand::ToggleEarth),
                        "labels" => Some(TrayCommand::ToggleLabels),
                        "startup" => Some(TrayCommand::ToggleStartup),
                        "exit" => Some(TrayCommand::Exit),
                        "sat_himawari_9" => Some(TrayCommand::SelectSatellite(Satellite::Himawari)),
                        "sat_goes_east" => Some(TrayCommand::SelectSatellite(Satellite::GoesEast)),
                        "sat_goes_west" => Some(TrayCommand::SelectSatellite(Satellite::GoesWest)),
                        "sat_gk2a" => Some(TrayCommand::SelectSatellite(Satellite::Gk2a)),
                        "sat_meteosat_12" => Some(TrayCommand::SelectSatellite(Satellite::Meteosat12)),
                        _ => None,
                    };
                    if let Some(cmd) = cmd {
                        if tx.send(cmd).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            _tray: tray,
            menu_channel: rx,
            mode_item,
            earth_item,
            satellite_items,
            labels_item,
            startup_item,
        })
    }

    fn mode_label(mode: MultiMonitorMode) -> &'static str {
        match mode {
            MultiMonitorMode::Span => "✓ Span Across Monitors",
            MultiMonitorMode::Duplicate => "Span Across Monitors",
        }
    }

    fn satellite_label(sat: Satellite, current: Satellite) -> String {
        if sat == current {
            format!("✓ {}", sat.name())
        } else {
            sat.name().to_string()
        }
    }

    fn earth_label(enabled: bool) -> &'static str {
        if enabled { "✓ Show Earth" } else { "Show Earth" }
    }

    fn labels_label(enabled: bool) -> &'static str {
        if enabled { "✓ Show Labels" } else { "Show Labels" }
    }

    fn startup_label(enabled: bool) -> &'static str {
        if enabled { "✓ Run on Startup" } else { "Run on Startup" }
    }

    /// Update the mode menu item text
    pub fn set_mode(&self, mode: MultiMonitorMode) {
        let _ = self.mode_item.set_text(Self::mode_label(mode));
    }

    /// Update the earth menu item text
    pub fn set_earth(&self, enabled: bool) {
        let _ = self.earth_item.set_text(Self::earth_label(enabled));
    }

    /// Update the satellite menu items
    pub fn set_satellite(&self, current: Satellite) {
        for (i, sat) in Satellite::all().iter().enumerate() {
            if let Some(item) = self.satellite_items.get(i) {
                let _ = item.set_text(Self::satellite_label(*sat, current));
            }
        }
    }

    /// Update the labels menu item text
    pub fn set_labels(&self, enabled: bool) {
        let _ = self.labels_item.set_text(Self::labels_label(enabled));
    }

    /// Update the startup menu item text
    pub fn set_startup(&self, enabled: bool) {
        let _ = self.startup_item.set_text(Self::startup_label(enabled));
    }

    /// Check for pending tray commands (non-blocking)
    pub fn poll_command(&self) -> Option<TrayCommand> {
        self.menu_channel.try_recv().ok()
    }
}

#[cfg(windows)]
fn create_icon() -> anyhow::Result<tray_icon::Icon> {
    use anyhow::Context;
    
    // Create a clean 16x16 Earth icon with visible continents
    let size = 16u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    
    let center = size as f32 / 2.0;
    let radius = center - 1.5;
    
    // Define continent-like regions (simplified shapes)
    let is_land = |x: f32, y: f32| -> bool {
        // Normalize to -1..1 from center
        let nx = (x - center) / radius;
        let ny = (y - center) / radius;
        
        // Simple continent approximations
        // North America (upper left quadrant)
        if nx < -0.1 && nx > -0.8 && ny < -0.1 && ny > -0.7 {
            return true;
        }
        // South America (lower left)
        if nx < 0.0 && nx > -0.5 && ny > 0.1 && ny < 0.8 {
            return true;
        }
        // Europe/Africa (center-right)
        if nx > -0.1 && nx < 0.4 && ny > -0.6 && ny < 0.7 {
            return true;
        }
        // Asia (upper right)
        if nx > 0.2 && nx < 0.8 && ny < 0.0 && ny > -0.6 {
            return true;
        }
        // Australia (lower right)
        if nx > 0.4 && nx < 0.8 && ny > 0.3 && ny < 0.6 {
            return true;
        }
        false
    };
    
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            
            if dist <= radius {
                // Inside circle
                let dist_ratio = dist / radius;
                
                // Add subtle shading (darker at edges for 3D effect)
                let shade = 1.0 - (dist_ratio * 0.25);
                
                if is_land(x as f32, y as f32) {
                    // Land - bright vibrant green
                    let r = (80.0 * shade) as u8;
                    let g = (180.0 * shade) as u8;
                    let b = (60.0 * shade) as u8;
                    rgba.extend_from_slice(&[r, g, b, 255]);
                } else {
                    // Ocean - bright vivid blue
                    let r = (30.0 * shade) as u8;
                    let g = (120.0 * shade) as u8;
                    let b = (220.0 * shade) as u8;
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            } else if dist <= radius + 1.0 {
                // Subtle atmosphere glow at edge
                let alpha = ((radius + 1.0 - dist) * 180.0) as u8;
                rgba.extend_from_slice(&[120, 180, 255, alpha]);
            } else {
                // Outside - transparent
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    
    tray_icon::Icon::from_rgba(rgba, size, size)
        .context("Failed to create tray icon image")
}

// Startup registry management
#[cfg(windows)]
pub mod startup {
    use anyhow::{Context, Result};
    use std::path::PathBuf;
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, REG_VALUE_TYPE,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const APP_NAME: &str = "LiveEarthWallpaper";

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Check if the app is set to run on startup
    pub fn is_enabled() -> bool {
        unsafe {
            let key_path = to_wide(RUN_KEY);
            let mut hkey = HKEY::default();
            
            let result = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            );
            
            if result.is_err() {
                return false;
            }

            let value_name = to_wide(APP_NAME);
            let mut data_type = REG_VALUE_TYPE::default();
            let mut data_size = 0u32;
            
            let exists = RegQueryValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                None,
                Some(&mut data_type),
                None,
                Some(&mut data_size),
            ).is_ok();
            
            let _ = RegCloseKey(hkey);
            exists
        }
    }

    /// Enable run on startup
    pub fn enable() -> Result<()> {
        let exe_path = std::env::current_exe()
            .context("Failed to get executable path")?;
        
        set_startup_value(&exe_path)
    }

    /// Disable run on startup
    pub fn disable() -> Result<()> {
        remove_startup_value()
    }

    /// Toggle run on startup, returns new state
    pub fn toggle() -> Result<bool> {
        if is_enabled() {
            disable()?;
            Ok(false)
        } else {
            enable()?;
            Ok(true)
        }
    }

    fn set_startup_value(exe_path: &PathBuf) -> Result<()> {
        unsafe {
            let key_path = to_wide(RUN_KEY);
            let mut hkey = HKEY::default();
            
            let result = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                0,
                KEY_WRITE,
                &mut hkey,
            );
            
            if result.is_err() {
                anyhow::bail!("Failed to open registry key: {:?}", result);
            }

            let value_name = to_wide(APP_NAME);
            let exe_str = exe_path.to_string_lossy();
            let exe_wide = to_wide(&exe_str);
            let data_bytes: &[u8] = std::slice::from_raw_parts(
                exe_wide.as_ptr() as *const u8,
                exe_wide.len() * 2,
            );

            let result = RegSetValueExW(
                hkey,
                PCWSTR(value_name.as_ptr()),
                0,
                REG_SZ,
                Some(data_bytes),
            );
            
            let _ = RegCloseKey(hkey);
            
            if result.is_err() {
                anyhow::bail!("Failed to set registry value: {:?}", result);
            }
            
            tracing::info!("Enabled run on startup");
            Ok(())
        }
    }

    fn remove_startup_value() -> Result<()> {
        unsafe {
            let key_path = to_wide(RUN_KEY);
            let mut hkey = HKEY::default();
            
            let result = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(key_path.as_ptr()),
                0,
                KEY_WRITE,
                &mut hkey,
            );
            
            if result.is_err() {
                anyhow::bail!("Failed to open registry key: {:?}", result);
            }

            let value_name = to_wide(APP_NAME);
            
            let result = RegDeleteValueW(hkey, PCWSTR(value_name.as_ptr()));
            let _ = RegCloseKey(hkey);
            
            // Ignore error if value doesn't exist
            if result.is_ok() {
                tracing::info!("Disabled run on startup");
            }
            
            Ok(())
        }
    }
}

// Non-Windows stubs
#[cfg(not(windows))]
pub struct TrayIcon;

#[cfg(not(windows))]
impl TrayIcon {
    pub fn new(_startup_enabled: bool, _mode: crate::monitor::MultiMonitorMode, _labels_enabled: bool) -> anyhow::Result<Self> {
        tracing::warn!("System tray not supported on this platform");
        Ok(Self)
    }

    pub fn set_mode(&self, _mode: crate::monitor::MultiMonitorMode) {}
    pub fn set_labels(&self, _enabled: bool) {}
    pub fn set_startup(&self, _enabled: bool) {}

    pub fn poll_command(&self) -> Option<TrayCommand> {
        None
    }
}

#[cfg(not(windows))]
pub mod startup {
    use anyhow::Result;

    pub fn is_enabled() -> bool {
        false
    }

    pub fn toggle() -> Result<bool> {
        tracing::warn!("Startup toggle not supported on this platform");
        Ok(false)
    }
}
