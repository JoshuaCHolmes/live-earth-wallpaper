# Live Earth Wallpaper

A native Windows application that displays live Himawari-8 satellite imagery of Earth with an accurate star field as your desktop wallpaper.

![Preview](preview.png)

## Features

- **Live Earth imagery** from the Himawari-8 geostationary satellite (140.7°E)
- **Accurate star field** based on HYG (Hipparcos-Yale-Gliese) catalog with ~25,800 stars (mag ≤ 7.5)
- **Smooth updates** - stars/sun/moon/planets refresh every 60 seconds, Earth image updates every 10 minutes
- **Realistic Rendering**:
  - **Sun**: Gaussian-profile bright disk with smooth bloom
  - **Moon**: Textured 3D sphere with accurate phase shading
  - **Planets**: Point sources with correct color and magnitude-based brightness
  - **Stars**: ~25,800 stars with accurate magnitude and color index
- **Multi-monitor support** with two modes:
  - **Span**: Single continuous view across all monitors
  - **Duplicate**: Each monitor gets its own centered Earth view
- **High-DPI aware** - renders at native resolution on scaled displays
- **Offline fallback** - uses cached imagery (shown in grayscale) when network unavailable
- **System tray** - minimal UI with refresh, mode toggle, labels toggle, and startup options
- **Lightweight** - ~6MB executable, ~15-30MB memory footprint

## Requirements

- Windows 10 (1703+) or Windows 11
- Internet connection for satellite imagery updates

## Installation

### From Release

Download the latest `live-earth-wallpaper.exe` from [Releases](https://github.com/JoshuaCHolmes/live-earth-wallpaper/releases) and run it.

### From Source

```bash
git clone https://github.com/JoshuaCHolmes/live-earth-wallpaper.git
cd live-earth-wallpaper
cargo build --release
./target/release/live-earth-wallpaper.exe
```

## Usage

Run the application and it will:

1. Create a system tray icon
2. Detect your monitor configuration
3. Fetch the latest Himawari-8 satellite image
4. Render the star field, planets, and moon for the current time
5. Set the composite as your desktop wallpaper
6. Update every 10 minutes

### System Tray Menu

| Option | Description |
|--------|-------------|
| **Refresh Now** | Immediately fetch new imagery and update wallpaper |
| **Mode: Span/Duplicate** | Toggle between multi-monitor modes |
| **Show Labels** | Toggle text labels for bright stars, planets, and Moon |
| **Run on Startup** | Toggle automatic startup with Windows |
| **Exit** | Close the application |

### Command Line Flags

```bash
# Normal operation (runs in tray)
live-earth-wallpaper.exe

# Single update and exit (useful for testing or Task Scheduler)
live-earth-wallpaper.exe --update-once

# Use duplicate mode (Earth centered on each monitor)
live-earth-wallpaper.exe --duplicate

# Combine flags
live-earth-wallpaper.exe --update-once --duplicate
```

## How It Works

### Field of View

The wallpaper simulates the view from space looking toward Earth at 140.7°E longitude (Himawari-8's position). The camera is placed at a virtual distance where Earth subtends 60% of the vertical field of view (~17.4° angular diameter).

### Rendering Accuracy

- **Perspective**: Correct gnomonic projection centered on the anti-satellite point (the sky behind Earth)
- **Occlusion**: Celestial objects (Sun/Moon/Planets/Stars) are correctly hidden when behind Earth
- **Sizes**: Sun and Moon rendered at correct angular diameters relative to FOV
- **Lighting**: Sun position matches Earth's illumination phase

### Offline Mode

If the satellite imagery cannot be fetched (no internet, server issues), the application falls back to a cached image. **Cached images are displayed in grayscale** to visually indicate that the view is not live.

### Multi-Monitor Modes

- **Span** (default): Creates a single continuous star field spanning all monitors, with Earth centered on the virtual desktop. Best for immersive setups.
- **Duplicate**: Each monitor gets an independent view with Earth centered. Useful for mismatched monitor sizes or presentations.

## Technical Details

### Data Sources

| Data | Source |
|------|--------|
| Earth imagery | [NICT Himawari-8](https://himawari8.nict.go.jp/) (10-minute updates) |
| Star catalog | [HYG Database v4.2](https://codeberg.org/astronexus/hyg) (mag ≤ 7.5) |
| Planet positions | [NASA JPL](https://ssd.jpl.nasa.gov/planets/approx_pos.html) orbital elements |
| Moon position | Meeus lunar theory |
| Moon texture | [NASA SVS](https://svs.gsfc.nasa.gov/4720/) (CGI Moon Kit) |

### Defaults

| Setting | Value |
|---------|-------|
| Full update interval | 10 minutes (Earth image fetch) |
| Star refresh interval | 60 seconds (uses cached Earth) |
| Earth image resolution | 4×4 tiles (2200×2200 px) |
| Star magnitude limit | 7.5 (~25,800 stars) |
| Star brightness | Pogson's ratio relative to mag 5.75 |
| Earth screen coverage | 60% of viewport height |

## Development

### Building on Windows

Requires Visual Studio Build Tools with C++ workload:

```bash
cargo build --release
```

### Cross-Compiling from Linux/WSL

```bash
# Using mingw-w64 (NixOS example)
nix-shell -p pkgsCross.mingwW64.stdenv.cc pkgsCross.mingwW64.windows.pthreads rustup
rustup target add x86_64-pc-windows-gnu
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
cargo build --release --target x86_64-pc-windows-gnu
```

## Credits

- Original concept: [Live-Space-View](https://github.com/JoshuaCHolmes/Live-Space-View) (Wallpaper Engine)
- Satellite imagery: [NICT Science Cloud / Himawari-8](https://himawari8.nict.go.jp/)
- Star data: [HYG Database](https://codeberg.org/astronexus/hyg) by David Nash
- Orbital elements: [NASA JPL Solar System Dynamics](https://ssd.jpl.nasa.gov/)
- Moon texture: [NASA SVS](https://svs.gsfc.nasa.gov/)

## License

MIT License - see [LICENSE](LICENSE) for details.
