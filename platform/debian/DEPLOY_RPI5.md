# Deploying AirJedi to Raspberry Pi 5 (DietPi)

This guide covers deploying the cross-compiled AirJedi ARM64 binary to a Raspberry Pi 5 running DietPi.

## Prerequisites

- **Development Machine**: macOS with ARM64 binary built via `cross`
- **Target Device**: Raspberry Pi 5 running DietPi (ARM64)
- **Network**: SSH access to the Raspberry Pi
- **SDR Hardware**: RTL-SDR dongle or SoapySDR-compatible device

## Cross-Compilation Summary

The binary was built using the native macOS ARM64 cross-compiler:
```bash
cargo build --release --target aarch64-unknown-linux-gnu --no-default-features
```

**Toolchain**: Native GCC 13.3.0 cross-compiler from Homebrew (`aarch64-unknown-linux-gnu`)

**Important**: The binary was built without SDR backend features to avoid cross-compilation issues with SoapySDR. You'll need to either:
1. Install SoapySDR on the Pi and rebuild locally, OR
2. Use file input mode for testing

**Build Time**: ~35 seconds (native toolchain is much faster than Docker-based approaches)

## Deployment Steps

### 1. Transfer the Binary to Raspberry Pi

From your development machine:

```bash
# Option A: Using scp
scp target/aarch64-unknown-linux-gnu/release/airjedi pi@raspberrypi.local:/home/pi/

# Option B: Using rsync (preserves permissions)
rsync -avz target/aarch64-unknown-linux-gnu/release/airjedi pi@raspberrypi.local:/home/pi/
```

### 2. Transfer Supporting Files

```bash
# Transfer configuration and web interface
scp config.toml pi@raspberrypi.local:/home/pi/
scp -r dist/ pi@raspberrypi.local:/home/pi/
```

### 3. Install Runtime Dependencies on Raspberry Pi

SSH into your Raspberry Pi:
```bash
ssh pi@raspberrypi.local
```

Install required dependencies:
```bash
# Update package list
sudo apt-get update

# Install basic runtime dependencies
sudo apt-get install -y libgcc-s1 libc6

# Optional: Install SoapySDR for SDR hardware support
sudo apt-get install -y soapysdr-tools libsoapysdr-dev

# Optional: Install RTL-SDR support
sudo apt-get install -y rtl-sdr librtlsdr-dev soapysdr-module-rtlsdr

# Optional: Install other SDR drivers
# For HackRF:
# sudo apt-get install -y hackrf soapysdr-module-hackrf
# For AirSpy:
# sudo apt-get install -y soapysdr-module-airspy
```

### 4. Verify Binary Execution

```bash
cd /home/pi
chmod +x airjedi
./airjedi --help
```

If you see the help output, the binary is working correctly!

## Running AirJedi on Raspberry Pi 5

### Option 1: Build Locally with SDR Support (Recommended)

Since the cross-compiled binary doesn't include SDR support, you can build natively on the Pi:

```bash
# Install Rust on the Raspberry Pi
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Install build dependencies
sudo apt-get install -y build-essential pkg-config
sudo apt-get install -y soapysdr-tools libsoapysdr-dev

# Clone or transfer your project
# cd /path/to/airjedi-sensor

# Build with SoapySDR support
cargo build --release

# The binary will be at: target/release/airjedi
```

### Option 2: File Input Mode (No SDR Required)

If you have pre-recorded IQ samples:

```bash
./airjedi --file samples.cf32
```

### Option 3: With SDR Hardware

If you installed SoapySDR and drivers:

```bash
# List available SDR devices
SoapySDRUtil --probe

# Run with default settings
./airjedi

# Run with custom gain and threshold
./airjedi --gain 40.0 --preamble-threshold 12.0

# Run with rate limiting
./airjedi --rate-limit --position-rate-ms 1000
```

## Systemd Service (Auto-start on Boot)

AirJedi includes a comprehensive systemd service file configured for Debian-based systems.

### Quick Installation

```bash
# Copy the service file (included in the repository)
sudo cp airjedi.service /etc/systemd/system/

# If you installed to a different location than /home/pi/airjedi,
# edit the service file to update the paths
sudo nano /etc/systemd/system/airjedi.service

# Reload systemd and enable the service
sudo systemctl daemon-reload
sudo systemctl enable --now airjedi

# Check status
sudo systemctl status airjedi

# View logs
sudo journalctl -u airjedi -f
```

### Service Features

The included `airjedi.service` file provides:
- ✅ **SBS-1 and WebSocket outputs enabled by default** (`--sbs1 --websocket`)
- ✅ **Automatic restart on failure**
- ✅ **Proper RTL-SDR USB permissions** (plugdev group)
- ✅ **Systemd journal logging**
- ✅ **Starts automatically on boot**

### Detailed Documentation

For complete systemd service documentation including:
- Customizing command-line arguments
- Changing ports and output formats
- Monitoring and troubleshooting
- Performance tuning
- Integration with FlightAware/FR24

See the comprehensive guide: **[SYSTEMD.md](SYSTEMD.md)**

## Network Configuration

### Accessing the Web Interface

The web interface runs on port 1337 by default. Access it from another computer:

```
http://raspberrypi.local:1337
```

Or use the IP address:
```
http://192.168.1.XXX:1337
```

### Port Forwarding (Optional)

If you want to access output formats from other computers:

| Format | Default Port | Purpose |
|--------|-------------|----------|
| BEAST  | 30005       | Binary format (dump1090-compatible) |
| Raw    | 30002       | Hex format |
| AVR    | 30001       | Text with timestamps |
| SBS-1  | 30003       | CSV format (BaseStation) |
| WebSocket | 8080     | Real-time web streaming |
| Web UI | 1337        | Map interface |

No firewall configuration needed on DietPi by default, but ensure your router allows access if needed.

## Performance Optimization for Raspberry Pi 5

### CPU Governor
Set CPU governor to performance mode for better real-time processing:

```bash
# Install cpufrequtils
sudo apt-get install -y cpufrequtils

# Set to performance mode
sudo cpufreq-set -g performance

# Make permanent (add to /etc/rc.local or systemd service)
```

### Memory

The application should run fine with default settings. Monitor with:
```bash
htop
```

### USB Power for SDR

Ensure adequate USB power for SDR devices:
```bash
# Edit config.txt
sudo nano /boot/config.txt

# Add or increase USB current limit
max_usb_current=1
```

Reboot after changes:
```bash
sudo reboot
```

## Troubleshooting

### Binary won't execute - "No such file or directory"

This usually means missing dynamic linker. Verify architecture:
```bash
file ./airjedi
uname -m  # Should show "aarch64"
```

### SDR Device Not Found

```bash
# Check USB devices
lsusb

# Test with SoapySDR
SoapySDRUtil --find

# For RTL-SDR, test with rtl_test
rtl_test
```

### Permission Issues with SDR

Add user to plugdev group:
```bash
sudo usermod -a -G plugdev pi
sudo reboot
```

### High CPU Usage

Enable rate limiting to reduce CPU load:
```bash
./airjedi --rate-limit \
  --position-rate-ms 1000 \
  --velocity-rate-ms 2000 \
  --metadata-rate-ms 5000
```

### Can't Access Web Interface

Check that the service is running and listening:
```bash
sudo netstat -tulpn | grep 1337
# or
sudo ss -tulpn | grep 1337
```

Verify firewall settings (DietPi usually has no firewall by default):
```bash
sudo iptables -L
```

## Building with Full SDR Support (Alternative)

If you need the full SoapySDR integration and want to build on the Pi:

```bash
# On Raspberry Pi 5
cd /home/pi/airjedi-sensor

# Build with SoapySDR (default feature)
cargo build --release --features soapy

# Or with RTL-SDR
cargo build --release --no-default-features --features rtlsdr

# Or with both
cargo build --release --features "soapy,rtlsdr"
```

## Next Steps

1. **Configure `config.toml`** for your specific needs
2. **Set up systemd service** for automatic startup
3. **Monitor performance** and adjust rate limiting if needed
4. **Connect to feeding services** (FlightRadar24, FlightAware, etc.) if desired
5. **Set up remote access** via reverse proxy (nginx) if needed

## Additional Resources

- [SoapySDR Documentation](https://github.com/pothosware/SoapySDR/wiki)
- [RTL-SDR Quick Start](https://www.rtl-sdr.com/rtl-sdr-quick-start-guide/)
- [DietPi Documentation](https://dietpi.com/docs/)
- [AirJedi Repository](https://github.com/ccustine/airjedi)

## Notes

- **Binary Size**: ~8.7MB (release build, not stripped)
- **Architecture**: ARM64 (aarch64)
- **Rust Version**: Built with Rust 1.90.0
- **Deployment**: Statically linked Rust code, dynamically linked system libraries
- **License**: Apache-2.0
