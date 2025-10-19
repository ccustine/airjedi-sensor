# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

AirJedi is an ADS-B (Automatic Dependent Surveillance–Broadcast) decoder written in Rust using the FutureSDR framework. It receives radio signals from aircraft transponders at 1090 MHz, demodulates and decodes the ADS-B messages, and displays aircraft positions on a web-based map interface.

## Build and Run Commands

```bash
# Development build and run (recommended for actual use)
cargo run

# Build only
cargo build

# Build and run in release mode
cargo run --release

# Run with custom parameters (see listen-adsb binary for all options)
cargo run -- --gain 40.0 --preamble-threshold 12.0

# Run with rate limiting enabled to reduce CPU usage on high-traffic scenarios
cargo run -- --rate-limit

# Run with custom rate limiting intervals
cargo run -- --rate-limit --position-rate-ms 1000 --velocity-rate-ms 2000
```

## Architecture

### Core Components

- **Preamble Detector** (`src/preamble_detector.rs`): Detects ADS-B message preambles in the signal
- **Demodulator** (`src/demodulator.rs`): Demodulates detected frames into bit streams  
- **Decoder** (`src/decoder.rs`): Decodes bit streams into ADS-B packets using the `adsb_deku` library
- **Tracker** (`src/tracker.rs`): Maintains aircraft state and position tracking with optional aircraft lifetime management and rate limiting
- **Rate Limiter** (`src/rate_limiter.rs`): Provides configurable rate limiting to reduce CPU usage on high-frequency updates

### Signal Processing Flow

The application uses FutureSDR's flowgraph architecture:
1. **Source**: SDR device (via seify) or file input
2. **Resampling**: Converts input sample rate to 4 MHz demodulator rate
3. **Signal Processing**: Magnitude calculation, noise floor estimation, preamble correlation
4. **Detection**: Preamble detection based on correlation and threshold
5. **Demodulation**: Symbol extraction and packet demodulation
6. **Decoding**: ADS-B message parsing
7. **Tracking**: Aircraft state management and web serving

### Web Interface

- **Frontend**: Located in `dist/` directory with HTML, JavaScript, and Leaflet map
- **Server**: Embedded HTTP server serves the map interface on `127.0.0.1:1337`
- **Configuration**: Server settings in `config.toml`

## Configuration

Edit `config.toml` to modify:
- `log_level`: Logging verbosity
- `ctrlport_bind`: Web server bind address and port
- `frontend_path`: Path to web interface files

## Rate Limiting

AirJedi includes sophisticated rate limiting capabilities to reduce CPU usage in high-traffic scenarios where aircraft send frequent, repetitive updates. This is particularly useful for GPS data processing where updates need to be filtered to prevent resource waste.

### Features

- **Per-aircraft rate limiting**: Each aircraft is tracked independently
- **Per-message type limiting**: Different limits for positions, velocities, identification, and metadata
- **Time-based debouncing**: Rapid updates are queued and only the latest is processed
- **Automatic cleanup**: Inactive aircraft are automatically removed from tracking
- **Real-time monitoring**: Statistics and metrics available via control port

### Default Rate Limits

- **Position updates**: 500ms (maximum 2 updates per second)
- **Velocity updates**: 1000ms (maximum 1 update per second)
- **Identification updates**: 0ms (immediate, no rate limiting)
- **Metadata updates**: 5000ms (maximum 1 update per 5 seconds)

### Usage

```bash
# Enable rate limiting with default settings
cargo run -- --rate-limit

# Customize rate limiting intervals (in milliseconds)
cargo run -- --rate-limit \
    --position-rate-ms 1000 \
    --velocity-rate-ms 2000 \
    --identification-rate-ms 0 \
    --metadata-rate-ms 10000

# Rate limiting works with all output formats
cargo run -- --rate-limit --beast --raw --sbs1 --websocket
```

### Monitoring

Rate limiting statistics are logged every 30 seconds when enabled:
```
Rate Limiting Stats: 1250 total updates, 65% immediate, 35% rate-limited, 12 active aircraft, 8 pending updates
```

You can also query statistics via the control port:
- Send `"stats"` to get rate limiting statistics in JSON format
- Send `"aircraft"` to get aircraft data (same as before)

### Architecture

The rate limiting system uses a **Rate-Limited State Manager** pattern that combines:
- **State tracking** per aircraft with configurable update intervals
- **Time-based debouncing** to handle rapid update bursts
- **LRU-style cleanup** to prevent memory leaks from inactive aircraft
- **Multi-tier output pipeline** separating immediate broadcasting from state updates

This ensures external consumers (via BEAST, Raw, SBS-1, WebSocket outputs) always receive immediate updates while internal state tracking is intelligently rate-limited.

## Dependencies

- **FutureSDR**: Signal processing framework (version 0.0.38 from crates.io)
- **adsb_deku**: ADS-B message parsing
- **Seify**: SDR hardware abstraction layer
- **Clap**: Command-line argument parsing

## Hardware Support

Supports SDR devices through FutureSDR's seify backend including:
- RTL-SDR dongles (with `rtlsdr` feature)
- SoapySDR-compatible devices (default `soapy` feature)
- Aaronia devices (with `aaronia_http` feature)

## Running from File

For testing/replay:
```bash
cargo run --release -- --file samples.cf32
```

Input files should be Complex32 format at any sample rate ≥ 2 MHz.

## Cross-Compilation for Raspberry Pi 5

AirJedi can be cross-compiled for ARM64 targets like Raspberry Pi 5 running DietPi or Raspberry Pi OS using the native macOS toolchain.

### Quick Build Script

The easiest way to cross-compile:
```bash
./platform/debian/build-rpi5.sh
```

This script handles all the setup and builds the ARM64 binary automatically.

### Manual Cross-Compilation

```bash
# One-time setup (install native ARM64 cross-compiler)
brew tap messense/macos-cross-toolchains
brew install aarch64-unknown-linux-gnu
rustup target add aarch64-unknown-linux-gnu

# Build for ARM64 using native toolchain (fast, no Docker required)
cargo build --release --target aarch64-unknown-linux-gnu --no-default-features
```

**Note**: The `--no-default-features` flag is required because SoapySDR cannot be cross-compiled easily. The resulting binary will need SDR drivers installed on the target Raspberry Pi.

**Benefits of Native Toolchain:**
- ✅ Much faster builds (~35 seconds vs 2+ minutes)
- ✅ No Docker/Colima overhead
- ✅ Simpler setup
- ✅ Direct control over compiler flags

### Deployment

See `platform/debian/DEPLOY_RPI5.md` for complete deployment instructions including:
- Transferring binaries and files to Raspberry Pi
- Installing runtime dependencies
- Setting up systemd service
- Performance optimization tips
- Troubleshooting common issues

### Building with SDR Support on Raspberry Pi

For full SDR functionality, build natively on the Raspberry Pi:

```bash
# On the Raspberry Pi
sudo apt-get install -y build-essential pkg-config soapysdr-tools libsoapysdr-dev
cargo build --release
```

This approach provides full SoapySDR integration and better hardware compatibility.

## Output Formats

AirJedi supports multiple output formats for compatibility with various ADS-B tools and applications:

### BEAST Format (Port 30005)
- **Default**: Enabled by default
- **Description**: Binary format compatible with dump1090's BEAST mode
- **Usage**: `--beast` / `--no-beast`, `--beast-port <PORT>`
- **Protocol**: Binary messages with timestamps and signal levels
- **Compatibility**: dump1090, tar1090, FlightRadar24 feeders

### Raw Format (Port 30002)
- **Default**: Enabled by default
- **Description**: Simple hex format compatible with dump1090's raw output
- **Usage**: `--raw` / `--no-raw`, `--raw-port <PORT>`
- **Protocol**: `*{hexdata};\n` format
- **Compatibility**: dump1090 port 30002, simple monitoring tools

### AVR Format (Port 30001)
- **Default**: Disabled by default
- **Description**: Text format with timestamps and signal levels
- **Usage**: `--avr`, `--avr-port <PORT>`
- **Protocol**: `@{timestamp}\n*{hexdata};\n` format
- **Compatibility**: dump1090 AVR format, debugging tools

### SBS-1/BaseStation Format (Port 30003)
- **Default**: Disabled by default
- **Description**: CSV format compatible with BaseStation and SBS-1 receivers
- **Usage**: `--sbs1`, `--sbs1-port <PORT>`
- **Protocol**: Comma-separated values with aircraft data
- **Compatibility**: BaseStation, Virtual Radar Server, PlanePlotter
- **References**: 
  - [BaseStation Protocol](http://woodair.net/sbs/article/barebones42_socket_data.htm)
  - [SBS-1 Data Format](http://www.homepages.mcb.net/bones/SBS/Article/Barebones42_Socket_Data.htm)

### WebSocket Format (Port 8080)
- **Default**: Disabled by default
- **Description**: Real-time WebSocket streaming with SBS-1 CSV format messages
- **Usage**: `--websocket`, `--websocket-port <PORT>`
- **Protocol**: Text WebSocket messages containing SBS-1/BaseStation CSV data
- **Compatibility**: Web browsers, JavaScript applications, real-time web dashboards
- **Message Format**: Each WebSocket message contains SBS-1 CSV formatted aircraft data (MSG,1/2/3/4)
- **Use Cases**: Real-time web applications, live flight tracking interfaces, custom dashboards
- **Note**: WebSocket broadcasts state-based updates (identification, position, velocity) rather than raw packets

## Example Usage

```bash
# Run with defaults (BEAST + Raw)
cargo run

# Enable all output formats
cargo run -- --avr --sbs1 --websocket

# Custom ports to avoid conflicts
cargo run -- --beast-port 40005 --raw-port 40002 --sbs1-port 40003 --websocket-port 9090

# Enable only WebSocket output for web applications
cargo run -- --no-beast --no-raw --websocket

# Enable SBS-1 and WebSocket for comprehensive coverage
cargo run -- --sbs1 --websocket
```

## WebSocket Usage Example

For web applications connecting to the WebSocket output:

```javascript
// Connect to AirJedi WebSocket server
const ws = new WebSocket('ws://localhost:8080');

// Handle SBS-1 CSV text messages
ws.onmessage = function(event) {
    // event.data is a string containing SBS-1 CSV format data
    const csvData = event.data;
    const fields = csvData.split(',');

    // SBS-1 format: MSG,type,session,aircraft,icao,flight,date_gen,time_gen,date_log,time_log,...
    if (fields[0] === 'MSG') {
        const messageType = parseInt(fields[1]);
        const icao = fields[4];

        switch (messageType) {
            case 1:  // Identification (callsign)
                console.log('Aircraft Identification:', {
                    icao: icao,
                    callsign: fields[10]
                });
                break;
            case 3:  // Airborne Position
                console.log('Aircraft Position:', {
                    icao: icao,
                    altitude: fields[11],
                    latitude: fields[14],
                    longitude: fields[15]
                });
                break;
            case 4:  // Airborne Velocity
                console.log('Aircraft Velocity:', {
                    icao: icao,
                    groundSpeed: fields[12],
                    track: fields[13],
                    verticalRate: fields[16]
                });
                break;
        }
    }
};

ws.onopen = function() {
    console.log('Connected to AirJedi WebSocket stream');
};

ws.onerror = function(error) {
    console.error('WebSocket error:', error);
};
```

## WebSocket Security Model

**IMPORTANT**: The WebSocket server is designed for **local-only use** and has minimal security controls:

### Security Characteristics

- **No Authentication**: Any client that can reach the port can connect and receive data
- **No Authorization**: All connected clients receive all aircraft data
- **No Origin Validation**: No CORS checks - any website can connect via JavaScript
- **No TLS/Encryption**: Plain TCP WebSocket (ws://) without encryption (wss:// not supported)
- **No Rate Limiting**: Clients can connect and consume data without throttling
- **Public Data**: ADS-B data is inherently public information broadcast by aircraft

### Recommended Deployment

1. **Bind to localhost only** (default: 127.0.0.1:8080) - prevents external access
2. **Use firewall rules** to restrict access if binding to non-localhost interfaces
3. **Deploy behind reverse proxy** if internet access is needed (add authentication/TLS there)
4. **Monitor connections** via client_count metrics to detect unexpected clients

### Acceptable Use Cases

- ✅ Local development and testing
- ✅ Single-user desktop applications
- ✅ Trusted local network deployments
- ✅ Behind authenticated reverse proxy (nginx, Apache with auth)

### Unsuitable Use Cases

- ❌ Direct internet exposure without additional security layers
- ❌ Multi-tenant environments without isolation
- ❌ Scenarios requiring data access auditing
- ❌ Compliance-regulated environments without additional controls