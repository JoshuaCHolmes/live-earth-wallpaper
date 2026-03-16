# Live Earth Wallpaper

A native Windows application that displays live Himawari-8 satellite imagery of Earth with an accurate star field as your desktop wallpaper.

![Preview](https://raw.githubusercontent.com/JoshuaCHolmes/Live-Space-View/main/ultimate%20live%20earth/preview.png)

## Features

- **Live Earth imagery** from the Himawari-8 geostationary satellite (140.7°E)
- **Accurate star field** based on HYG (Hipparcos-Yale-Gliese) catalog
- **Planet positions** calculated using NASA JPL orbital elements
- **Moon phase and position** with realistic illumination
- **Multi-monitor support** - automatically detects and spans all displays
- **Lightweight** - ~5-15MB memory footprint, no browser/WebView overhead

## Requirements

- Windows 10/11 (ARM64 or x64)
- Internet connection for satellite imagery

## Installation

### From Release

Download the latest `.exe` from [Releases](https://github.com/JoshuaCHolmes/live-earth-wallpaper/releases) and run it.

### From Source

```bash
# Clone the repository
git clone https://github.com/JoshuaCHolmes/live-earth-wallpaper.git
cd live-earth-wallpaper

# Build release binary
cargo build --release

# Run
./target/release/live-earth-wallpaper.exe
```

## Usage

Simply run the application. It will:

1. Detect your monitor configuration
2. Fetch the latest Himawari-8 satellite image
3. Render the star field and planets for the current time
4. Set the composite as your desktop wallpaper
5. Update every 10 minutes

### Command Line Options

- `--update-once` - Perform a single update and exit (useful for testing)

### Exit

Press `Ctrl+C` in the console window to exit gracefully.

## Technical Details

### Data Sources

- **Earth imagery**: [Himawari-8](https://himawari8.nict.go.jp/) real-time full disk images
- **Star catalog**: HYG Database v4.2 (subset of ~100 brightest stars embedded)
- **Planet ephemeris**: NASA JPL orbital elements for J2000.0 epoch

### Astronomical Accuracy

Star and planet positions are calculated for the view from Himawari-8's geostationary position. The satellite is located at 140.7°E longitude, 35,793 km above Earth's surface, providing a view of the night sky "behind" Earth from its perspective.

### Architecture

```
┌─────────────────────────────────────────┐
│  Main Loop (10 min interval)            │
│  ├─ Detect monitors (Win32 API)         │
│  ├─ Fetch Himawari-8 tiles              │
│  ├─ Calculate celestial positions       │
│  ├─ Render composite image              │
│  └─ Set wallpaper (IDesktopWallpaper)   │
└─────────────────────────────────────────┘
```

## Configuration

Currently, the application uses sensible defaults:

- Update interval: 10 minutes
- Image resolution: 4x4 tiles (2200×2200 Earth image)
- Star magnitude limit: 6.5 (naked eye visibility)
- Field of view: 120°

Future versions may include a configuration file or system tray UI.

## Development

### Building on Windows

```bash
cargo build --release
```

### Cross-compiling from Linux

```bash
# Install Windows target
rustup target add x86_64-pc-windows-gnu
# or for ARM64
rustup target add aarch64-pc-windows-msvc

# Build (requires mingw-w64 or Windows SDK)
cargo build --release --target x86_64-pc-windows-gnu
```

## Credits

- Original concept: [Live-Space-View](https://github.com/JoshuaCHolmes/Live-Space-View) Wallpaper Engine project
- Satellite imagery: [NICT Himawari-8](https://himawari8.nict.go.jp/)
- Star data: [HYG Database](https://github.com/astronexus/HYG-Database)
- Orbital elements: [NASA JPL](https://ssd.jpl.nasa.gov/planets/approx_pos.html)

## License

MIT License - see [LICENSE](LICENSE) for details.
