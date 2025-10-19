# AirJedi Systemd Service

This guide explains how to run AirJedi as a systemd service on Debian-based systems (Raspberry Pi OS, DietPi, Ubuntu, Debian).

## Features

The systemd service is configured to:
- ✅ Start automatically on boot
- ✅ Restart automatically on failure
- ✅ Run with proper permissions for RTL-SDR access
- ✅ Enable SBS-1 and WebSocket outputs by default
- ✅ Log to systemd journal for easy debugging

## Installation

### 1. Prepare the Installation Directory

```bash
# Create installation directory
sudo mkdir -p /opt/airjedi
cd /opt/airjedi

# Copy the binary and supporting files
sudo cp ~/airjedi /opt/airjedi/
sudo cp ~/config.toml /opt/airjedi/
sudo cp -r ~/dist /opt/airjedi/

# Set proper ownership
sudo chown -R pi:pi /opt/airjedi
```

Alternatively, you can use the `/home/pi/airjedi` directory if you prefer a user-space installation.

### 2. Install the Systemd Service

```bash
# Copy the service file to systemd directory
sudo cp airjedi.service /etc/systemd/system/

# If you installed to /opt/airjedi instead of /home/pi/airjedi,
# edit the service file to update paths:
sudo nano /etc/systemd/system/airjedi.service
# Change WorkingDirectory and ExecStart paths accordingly

# Reload systemd to recognize the new service
sudo systemctl daemon-reload
```

### 3. Ensure RTL-SDR Access

Make sure your user has access to USB devices:

```bash
# Add user to plugdev group (usually already done)
sudo usermod -a -G plugdev pi

# Create udev rule for RTL-SDR (if not already present)
echo 'SUBSYSTEM=="usb", ATTRS{idVendor}=="0bda", ATTRS{idProduct}=="2838", GROUP="plugdev", MODE="0666"' | sudo tee /etc/udev/rules.d/20-rtlsdr.rules

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger

# Verify RTL-SDR is detected
SoapySDRUtil --find
```

## Usage

### Start the Service

```bash
# Start the service immediately
sudo systemctl start airjedi

# Enable auto-start on boot
sudo systemctl enable airjedi

# Start and enable in one command
sudo systemctl enable --now airjedi
```

### Check Service Status

```bash
# View service status
sudo systemctl status airjedi

# View real-time logs
sudo journalctl -u airjedi -f

# View recent logs
sudo journalctl -u airjedi -n 100

# View logs since last boot
sudo journalctl -u airjedi -b
```

### Stop the Service

```bash
# Stop the service
sudo systemctl stop airjedi

# Disable auto-start on boot
sudo systemctl disable airjedi

# Stop and disable in one command
sudo systemctl disable --now airjedi
```

### Restart the Service

```bash
# Restart the service (useful after updates)
sudo systemctl restart airjedi

# Reload configuration (if service supports it)
sudo systemctl reload airjedi
```

## Configuration

### Customizing Service Parameters

Edit the service file to customize command-line arguments:

```bash
sudo nano /etc/systemd/system/airjedi.service
```

**Example configurations:**

```ini
# Default: SBS-1 and WebSocket outputs
ExecStart=/opt/airjedi/airjedi --sbs1 --websocket

# Enable all output formats
ExecStart=/opt/airjedi/airjedi --sbs1 --websocket --avr --beast --raw

# Custom ports
ExecStart=/opt/airjedi/airjedi --sbs1 --sbs1-port 30003 --websocket --websocket-port 8080

# With rate limiting
ExecStart=/opt/airjedi/airjedi --sbs1 --websocket --rate-limit

# Custom sample rate and gain
ExecStart=/opt/airjedi/airjedi --sbs1 --websocket --sample-rate 2.0e6 --gain 40.0

# With specific SDR device (if multiple devices)
ExecStart=/opt/airjedi/airjedi --sbs1 --websocket --args "driver=rtlsdr,serial=00000001"
```

After editing, reload systemd and restart the service:

```bash
sudo systemctl daemon-reload
sudo systemctl restart airjedi
```

### Customizing User and Directory

By default, the service runs as user `pi` in `/home/pi/airjedi`. To change this:

```bash
sudo nano /etc/systemd/system/airjedi.service
```

Update these lines:

```ini
User=your_username
Group=your_group
WorkingDirectory=/path/to/your/installation
ExecStart=/path/to/your/installation/airjedi --sbs1 --websocket
```

## Monitoring and Troubleshooting

### View Logs

```bash
# Follow logs in real-time
sudo journalctl -u airjedi -f

# Filter by priority (errors only)
sudo journalctl -u airjedi -p err

# Export logs to file
sudo journalctl -u airjedi > airjedi.log
```

### Common Issues

#### Service fails to start

```bash
# Check detailed status
sudo systemctl status airjedi -l

# View full logs
sudo journalctl -u airjedi -n 50
```

**Common causes:**
- RTL-SDR not connected or not recognized
- Incorrect permissions
- Missing dependencies
- Port already in use

#### RTL-SDR not found

```bash
# Verify device is connected
lsusb | grep RTL

# Check SoapySDR detection
SoapySDRUtil --find

# Verify udev rules
ls -l /etc/udev/rules.d/*rtl*

# Check user permissions
groups pi  # Should include 'plugdev'
```

#### Port conflicts

```bash
# Check if ports are already in use
sudo netstat -tlnp | grep -E '30003|8080'

# Or using ss command
sudo ss -tlnp | grep -E '30003|8080'
```

If a port is in use, either stop the conflicting service or use a different port in the ExecStart command.

#### Binary not found

```bash
# Verify binary exists and is executable
ls -l /opt/airjedi/airjedi

# Make it executable if needed
sudo chmod +x /opt/airjedi/airjedi
```

### Performance Monitoring

```bash
# Monitor CPU and memory usage
sudo systemctl status airjedi

# Or use top/htop
htop -p $(pidof airjedi)
```

## Testing Connectivity

### Test SBS-1 Output

```bash
# From another terminal or computer on the network
nc your-rpi-ip 30003

# You should see CSV-formatted aircraft data
```

### Test WebSocket Output

```bash
# Using websocat (install with: sudo apt install websocat)
websocat ws://your-rpi-ip:8080

# Or from a web browser console:
# const ws = new WebSocket('ws://your-rpi-ip:8080');
# ws.onmessage = (e) => console.log(e.data);
```

### Test Web Interface

Open a browser and navigate to:
```
http://your-rpi-ip:1337
```

You should see the aircraft map interface.

## Updating AirJedi

When you rebuild or update the binary:

```bash
# Stop the service
sudo systemctl stop airjedi

# Replace the binary
sudo cp ~/airjedi /opt/airjedi/airjedi

# Restart the service
sudo systemctl start airjedi

# Verify it's running
sudo systemctl status airjedi
```

## Uninstallation

```bash
# Stop and disable the service
sudo systemctl stop airjedi
sudo systemctl disable airjedi

# Remove the service file
sudo rm /etc/systemd/system/airjedi.service

# Reload systemd
sudo systemctl daemon-reload

# Optionally remove the installation directory
sudo rm -rf /opt/airjedi
```

## Advanced Configuration

### Running as Root (Not Recommended)

If you absolutely need to run as root (not recommended for security):

```bash
sudo nano /etc/systemd/system/airjedi.service
```

Change:
```ini
User=root
Group=root
```

### Environment Variables

If you need to set environment variables:

```ini
[Service]
Environment="RUST_LOG=info"
Environment="CUSTOM_VAR=value"
```

### Resource Limits

To limit CPU or memory usage:

```ini
[Service]
# Limit to 50% CPU
CPUQuota=50%

# Limit to 512MB RAM
MemoryMax=512M

# Limit file descriptors
LimitNOFILE=65536
```

## Integration with Other Services

### FlightAware (PiAware)

AirJedi can feed data to PiAware:

```bash
# Configure PiAware to read from AirJedi's BEAST output
sudo piaware-config receiver-type other
sudo piaware-config receiver-host 127.0.0.1
sudo piaware-config receiver-port 30005

# Restart PiAware
sudo systemctl restart piaware
```

### FR24 Feeder

Configure FR24 feeder to use AirJedi's BEAST output on port 30005.

### Virtual Radar Server

Point VRS to AirJedi's SBS-1 output on port 30003.

## Support

For issues or questions:
- Check the logs: `sudo journalctl -u airjedi -n 100`
- Verify RTL-SDR detection: `SoapySDRUtil --find`
- Review the project documentation: `CLAUDE.md`, `README.md`
- Report issues: https://github.com/ccustine/airjedi/issues
