//! System tray icon and menu for Windows
//!
//! Provides a minimal tray interface with:
//! - Refresh Now
//! - Run on Startup (toggle)
//! - Exit

/// Commands that can be triggered from the tray menu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    RefreshNow,
    ToggleStartup,
    Exit,
}

#[cfg(windows)]
use std::sync::mpsc::{self, Receiver, Sender};

#[cfg(windows)]
pub struct TrayIcon {
    _tray: tray_icon::TrayIcon,
    menu_channel: Receiver<TrayCommand>,
}

#[cfg(windows)]
impl TrayIcon {
    pub fn new(startup_enabled: bool) -> anyhow::Result<Self> {
        use anyhow::Context;
        use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
        use tray_icon::{Icon, TrayIconBuilder};

        // Create menu items
        let menu = Menu::new();
        
        let refresh_item = MenuItem::with_id("refresh", "Refresh Now", true, None);
        let startup_item = MenuItem::with_id(
            "startup",
            if startup_enabled { "✓ Run on Startup" } else { "  Run on Startup" },
            true,
            None,
        );
        let separator = PredefinedMenuItem::separator();
        let exit_item = MenuItem::with_id("exit", "Exit", true, None);

        menu.append(&refresh_item)?;
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
                        "startup" => Some(TrayCommand::ToggleStartup),
                        "exit" => Some(TrayCommand::Exit),
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
        })
    }

    /// Check for pending tray commands (non-blocking)
    pub fn poll_command(&self) -> Option<TrayCommand> {
        self.menu_channel.try_recv().ok()
    }
}

#[cfg(windows)]
fn create_icon() -> anyhow::Result<tray_icon::Icon> {
    use anyhow::Context;
    
    // Create a simple 16x16 blue/green earth-like icon
    let size = 16u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    
    let center = size as f32 / 2.0;
    let radius = center - 1.0;
    
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            
            if dist <= radius {
                // Inside circle - blue/green earth colors
                let angle = dy.atan2(dx);
                let normalized_dist = dist / radius;
                
                // Create some "continent" patterns
                let pattern = ((angle * 3.0).sin() * (normalized_dist * 5.0).cos()).abs();
                
                if pattern > 0.5 {
                    // Land (green)
                    rgba.extend_from_slice(&[34, 139, 34, 255]);
                } else {
                    // Ocean (blue)
                    rgba.extend_from_slice(&[30, 90, 180, 255]);
                }
            } else {
                // Outside circle - transparent
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
    pub fn new(_startup_enabled: bool) -> anyhow::Result<Self> {
        tracing::warn!("System tray not supported on this platform");
        Ok(Self)
    }

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
