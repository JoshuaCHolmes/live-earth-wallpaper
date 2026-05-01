//! Windows wallpaper API integration
//!
//! Sets the desktop wallpaper using IDesktopWallpaper COM interface, and the
//! lock screen wallpaper using the WinRT LockScreen API.

use anyhow::{Context, Result};
use std::path::Path;

/// Where the wallpaper image should be applied
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WallpaperTarget {
    Desktop,
    LockScreen,
    Both,
}

impl WallpaperTarget {
    pub fn includes_desktop(self) -> bool {
        matches!(self, WallpaperTarget::Desktop | WallpaperTarget::Both)
    }
    pub fn includes_lockscreen(self) -> bool {
        matches!(self, WallpaperTarget::LockScreen | WallpaperTarget::Both)
    }
    pub fn label(self) -> &'static str {
        match self {
            WallpaperTarget::Desktop => "Desktop",
            WallpaperTarget::LockScreen => "Lock Screen",
            WallpaperTarget::Both => "Both",
        }
    }
}

impl Default for WallpaperTarget {
    fn default() -> Self {
        WallpaperTarget::Desktop
    }
}

/// Set the desktop wallpaper to the given image path
pub fn set_wallpaper(image_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        set_wallpaper_windows(image_path)
    }

    #[cfg(not(windows))]
    {
        // On non-Windows, just log the action
        tracing::info!("Would set wallpaper to: {}", image_path.display());
        Ok(())
    }
}

#[cfg(windows)]
fn set_wallpaper_windows(image_path: &Path) -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize,
        CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        DesktopWallpaper, IDesktopWallpaper, DWPOS_SPAN,
    };

    // Convert path to wide string
    let path_wide: Vec<u16> = OsStr::new(image_path.as_os_str())
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // Initialize COM
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .context("Failed to initialize COM")?;

        let result = (|| -> Result<()> {
            // Create IDesktopWallpaper instance
            let wallpaper: IDesktopWallpaper =
                CoCreateInstance(&DesktopWallpaper, None, CLSCTX_ALL)
                    .context("Failed to create DesktopWallpaper instance")?;

            // Set wallpaper position to SPAN for multi-monitor support
            wallpaper
                .SetPosition(DWPOS_SPAN)
                .context("Failed to set wallpaper position")?;

            // Set the wallpaper (None for monitor ID = all monitors)
            wallpaper
                .SetWallpaper(PCWSTR::null(), PCWSTR(path_wide.as_ptr()))
                .context("Failed to set wallpaper")?;

            tracing::debug!("Wallpaper set to: {}", image_path.display());
            Ok(())
        })();

        CoUninitialize();
        result
    }
}

/// Get the wallpaper storage directory
pub fn wallpaper_dir() -> Result<std::path::PathBuf> {
    let local_app_data = if cfg!(windows) {
        std::env::var("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    } else {
        std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".local/share"))
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    };

    let dir = local_app_data.join("LiveEarthWallpaper");
    std::fs::create_dir_all(&dir).context("Failed to create wallpaper directory")?;
    
    Ok(dir)
}

/// Set the Windows lock screen wallpaper using the WinRT LockScreen API.
/// This applies to the lock screen image; Windows uses the primary monitor.
pub fn set_lock_screen(image_path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        set_lock_screen_windows(image_path)
    }

    #[cfg(not(windows))]
    {
        tracing::info!("Would set lock screen to: {}", image_path.display());
        Ok(())
    }
}

#[cfg(windows)]
fn set_lock_screen_windows(image_path: &Path) -> Result<()> {
    use windows::core::HSTRING;
    use windows::Storage::StorageFile;
    use windows::System::UserProfile::LockScreen;

    // Canonicalize so WinRT gets an absolute path with no relative components
    let abs = std::fs::canonicalize(image_path)
        .with_context(|| format!("Failed to canonicalize lock screen image path: {}", image_path.display()))?;

    // Strip the Windows extended-length prefix (\\?\) — StorageFile rejects it.
    let abs_str = abs.to_string_lossy();
    let cleaned = abs_str.strip_prefix(r"\\?\").unwrap_or(&abs_str);

    let path_h = HSTRING::from(cleaned);

    // GetFileFromPathAsync -> StorageFile -> LockScreen::SetImageFileAsync
    let file_op = StorageFile::GetFileFromPathAsync(&path_h)
        .context("StorageFile::GetFileFromPathAsync failed to start")?;
    let file = file_op.get().context("StorageFile::GetFileFromPathAsync failed")?;

    let set_op = LockScreen::SetImageFileAsync(&file)
        .context("LockScreen::SetImageFileAsync failed to start")?;
    set_op.get().context("LockScreen::SetImageFileAsync failed")?;

    tracing::debug!("Lock screen set to: {}", cleaned);
    Ok(())
}
