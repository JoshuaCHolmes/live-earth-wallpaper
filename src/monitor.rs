//! Monitor detection for multi-monitor setups
//!
//! On Windows, uses Win32 APIs to enumerate displays.
//! On other platforms, provides reasonable defaults.

use anyhow::Result;

/// How to handle multiple monitors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum MultiMonitorMode {
    /// Single image spans across all monitors (Earth centered on virtual desktop)
    #[default]
    Span,
    /// Each monitor gets its own centered Earth view
    Duplicate,
}

#[derive(Debug, Clone)]
pub struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct MonitorLayout {
    pub monitors: Vec<Monitor>,
    pub total_width: u32,
    pub total_height: u32,
    pub bounds: (i32, i32, i32, i32), // min_x, min_y, max_x, max_y
}

impl MonitorLayout {
    /// Detect all monitors and calculate total virtual desktop size
    pub fn detect() -> Result<Self> {
        #[cfg(windows)]
        {
            detect_windows_monitors()
        }
        
        #[cfg(not(windows))]
        {
            // Fallback for non-Windows (e.g., development on Linux)
            Ok(Self {
                monitors: vec![Monitor {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                    is_primary: true,
                    name: "Default".to_string(),
                }],
                total_width: 1920,
                total_height: 1080,
                bounds: (0, 0, 1920, 1080),
            })
        }
    }

    /// Get the primary monitor
    #[allow(dead_code)]
    pub fn primary(&self) -> Option<&Monitor> {
        self.monitors.iter().find(|m| m.is_primary)
    }

    /// Build a layout containing only the primary monitor, normalized to (0,0).
    /// Used for rendering the lock screen image at single-monitor size.
    pub fn primary_only(&self) -> Option<MonitorLayout> {
        let p = self.primary()?;
        let primary = Monitor {
            x: 0,
            y: 0,
            width: p.width,
            height: p.height,
            is_primary: true,
            name: p.name.clone(),
        };
        Some(MonitorLayout {
            monitors: vec![primary],
            total_width: p.width,
            total_height: p.height,
            bounds: (0, 0, p.width as i32, p.height as i32),
        })
    }
}

#[cfg(windows)]
fn detect_windows_monitors() -> Result<MonitorLayout> {
    use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
    };

    let mut monitors = Vec::new();

    unsafe extern "system" fn enum_callback(
        hmonitor: HMONITOR,
        _hdc: HDC,
        _rect: *mut RECT,
        lparam: LPARAM,
    ) -> BOOL {
        let monitors = &mut *(lparam.0 as *mut Vec<Monitor>);

        let mut info: MONITORINFOEXW = std::mem::zeroed();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

        if GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
            let rc = info.monitorInfo.rcMonitor;
            let is_primary = (info.monitorInfo.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY

            // Convert device name from wide string
            let name_end = info.szDevice.iter().position(|&c| c == 0).unwrap_or(32);
            let name = String::from_utf16_lossy(&info.szDevice[..name_end]);

            monitors.push(Monitor {
                x: rc.left,
                y: rc.top,
                width: (rc.right - rc.left) as u32,
                height: (rc.bottom - rc.top) as u32,
                is_primary,
                name,
            });
        }

        BOOL(1) // Continue enumeration
    }

    unsafe {
        let monitors_ptr = &mut monitors as *mut Vec<Monitor>;
        let result = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(enum_callback),
            LPARAM(monitors_ptr as isize),
        );
        if !result.as_bool() {
            anyhow::bail!("EnumDisplayMonitors failed");
        }
    }

    if monitors.is_empty() {
        // Fallback to default
        monitors.push(Monitor {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
            name: "Default".to_string(),
        });
    }

    // Calculate bounds
    let min_x = monitors.iter().map(|m| m.x).min().unwrap_or(0);
    let min_y = monitors.iter().map(|m| m.y).min().unwrap_or(0);
    let max_x = monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(1920);
    let max_y = monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(1080);

    let total_width = (max_x - min_x) as u32;
    let total_height = (max_y - min_y) as u32;

    tracing::debug!(
        "Detected {} monitor(s): {}x{} total",
        monitors.len(),
        total_width,
        total_height
    );

    for (i, m) in monitors.iter().enumerate() {
        tracing::debug!(
            "  Monitor {}: {} {}x{} at ({}, {}){}",
            i + 1,
            m.name,
            m.width,
            m.height,
            m.x,
            m.y,
            if m.is_primary { " [PRIMARY]" } else { "" }
        );
    }

    Ok(MonitorLayout {
        monitors,
        total_width,
        total_height,
        bounds: (min_x, min_y, max_x, max_y),
    })
}
