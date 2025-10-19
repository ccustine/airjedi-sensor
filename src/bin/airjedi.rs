use airjedi::DEMOD_SAMPLE_RATE;
use airjedi::OutputModuleManager;
use airjedi::{BeastOutput, AvrOutput, RawOutput, Sbs1Output, WebSocketOutput};
use airjedi::Decoder;
use airjedi::Demodulator;
use airjedi::PreambleDetector;
use airjedi::Tracker;
use airjedi::RateLimitConfig;
use anyhow::Result;
use clap::Parser;
use clap::command;
use futuresdr::blocks::Apply;
use futuresdr::blocks::FileSource;
use futuresdr::blocks::FirBuilder;
use futuresdr::blocks::Throttle;
use futuresdr::blocks::seify::SourceBuilder;
use futuresdr::num_complex::Complex32;
use futuresdr::num_integer;
use futuresdr::runtime::Flowgraph;
use futuresdr::runtime::Runtime;
use futuresdr::tracing::warn;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(version)]
struct Args {
    /// Antenna
    #[arg(long)]
    antenna: Option<String>,
    /// Seify Args
    #[arg(short, long)]
    args: Option<String>,
    /// Gain
    #[arg(short, long, default_value_t = 30.0)]
    gain: f64,
    /// Sample rate
    #[arg(short, long, default_value_t = 2.2e6, value_parser = sample_rate_parser)]
    sample_rate: f64,
    /// Preamble detection threshold
    #[arg(short, long, default_value_t = 10.0)]
    preamble_threshold: f32,
    /// Use a file instead of a device
    #[arg(short, long)]
    file: Option<String>,
    /// Remove aircrafts when no packets have been received for the specified number of seconds
    #[arg(short, long)]
    lifetime: Option<u64>,

    // Rate limiting arguments
    /// Enable rate limiting to reduce CPU usage on high-frequency updates
    #[arg(long)]
    rate_limit: bool,
    /// Position update rate limit in milliseconds (default: 500ms)
    #[arg(long, default_value_t = 500)]
    position_rate_ms: u64,
    /// Velocity update rate limit in milliseconds (default: 1000ms)
    #[arg(long, default_value_t = 1000)]
    velocity_rate_ms: u64,
    /// Identification update rate limit in milliseconds (default: 0ms = immediate)
    #[arg(long, default_value_t = 0)]
    identification_rate_ms: u64,
    /// Metadata update rate limit in milliseconds (default: 5000ms)
    #[arg(long, default_value_t = 5000)]
    metadata_rate_ms: u64,

    // Output module arguments
    /// Enable BEAST mode output (dump1090 compatible)
    #[arg(long, default_value_t = true)]
    beast: bool,
    /// Disable BEAST mode output
    #[arg(long, conflicts_with = "beast")]
    no_beast: bool,
    /// Port for BEAST mode output
    #[arg(long, default_value_t = 30005)]
    beast_port: u16,

    /// Enable AVR format output (dump1090 compatible with timestamps)
    #[arg(long)]
    avr: bool,
    /// Port for AVR format output
    #[arg(long, default_value_t = 30001)]
    avr_port: u16,

    /// Enable raw format output (dump1090 port 30002 compatible)
    #[arg(long, default_value_t = true)]
    raw: bool,
    /// Disable raw format output
    #[arg(long, conflicts_with = "raw")]
    no_raw: bool,
    /// Port for raw format output
    #[arg(long, default_value_t = 30002)]
    raw_port: u16,

    /// Enable SBS-1/BaseStation format output (port 30003 compatible)
    #[arg(long)]
    sbs1: bool,
    /// Port for SBS-1/BaseStation format output
    #[arg(long, default_value_t = 30003)]
    sbs1_port: u16,

    /// Enable WebSocket output for real-time web application streaming
    #[arg(long)]
    websocket: bool,
    /// Port for WebSocket output
    #[arg(long, default_value_t = 30008)]
    websocket_port: u16,

    /// List available RTL-SDR devices and exit
    #[arg(long)]
    list_devices: bool,
}

fn sample_rate_parser(sample_rate_str: &str) -> Result<f64, String> {
    let sample_rate: f64 = sample_rate_str
        .parse()
        .map_err(|_| format!("`{sample_rate_str}` is not a valid sample rate"))?;
    // Sample rate must be at least 2 MHz
    if sample_rate < 2e6 {
        Err("Sample rate must be at least 2 MHz".to_string())
    } else {
        Ok(sample_rate)
    }
}

/// Check if any SDR devices are available (returns true if devices found)
fn check_sdr_devices() -> bool {
    use std::process::Command;

    println!("Checking for available SDR devices...");

    // Try using SoapySDRUtil to check for devices
    let output = Command::new("SoapySDRUtil")
        .arg("--find")
        .output();

    match output {
        Ok(result) if result.status.success() => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            // Check if we got actual device output (not empty, not "No devices found")
            let has_devices = !stdout.trim().is_empty() && !stdout.contains("No devices found");

            if has_devices {
                println!("SoapySDRUtil found device(s):");
                for line in stdout.lines() {
                    if !line.trim().is_empty() {
                        println!("  {}", line.trim());
                    }
                }
            } else {
                println!("SoapySDRUtil found no devices");
            }

            has_devices
        }
        Err(e) => {
            println!("Could not run SoapySDRUtil: {}", e);
            println!("Trying fallback method (rtl_test)...");

            // Fallback: try rtl_test
            let rtl_output = Command::new("rtl_test")
                .arg("-t")
                .output();

            match rtl_output {
                Ok(result) if result.status.success() => {
                    println!("rtl_test found RTL-SDR device(s)");
                    true
                }
                Ok(_) => {
                    println!("rtl_test found no devices");
                    false
                }
                Err(e) => {
                    println!("Could not run rtl_test: {}", e);
                    println!("Unable to check for devices - proceeding anyway");
                    false
                }
            }
        }
        Ok(result) => {
            // SoapySDRUtil ran but returned non-zero exit code
            let stderr = String::from_utf8_lossy(&result.stderr);
            println!("SoapySDRUtil failed: {}", stderr);
            println!("Trying fallback method (rtl_test)...");

            let rtl_output = Command::new("rtl_test")
                .arg("-t")
                .output();

            match rtl_output {
                Ok(result) if result.status.success() => {
                    println!("rtl_test found RTL-SDR device(s)");
                    true
                }
                _ => {
                    println!("No devices found via rtl_test either");
                    false
                }
            }
        }
    }
}

/// List available SDR devices using SoapySDR
fn list_sdr_devices() -> Result<()> {
    use std::process::Command;

    println!("Enumerating available SDR devices...\n");

    // Try using SoapySDRUtil to list devices
    let output = Command::new("SoapySDRUtil")
        .arg("--find")
        .output();

    match output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);

                if stdout.trim().is_empty() || stdout.contains("No devices found") {
                    println!("No SDR devices found.");
                    println!("\nTroubleshooting:");
                    println!("  • Make sure your RTL-SDR is plugged in");
                    println!("  • Check that RTL-SDR drivers are installed (rtl-sdr)");
                    println!("  • Verify SoapySDR is installed with RTL-SDR support");
                    println!("  • Try running with sudo if permissions are an issue");
                } else {
                    // Parse and display the output
                    println!("{}", stdout);
                    println!("\nTo use a specific device with AirJedi:");
                    println!("  airjedi --args 'driver=rtlsdr'");
                    println!("  airjedi --args 'driver=rtlsdr,serial=00000001'");
                }
            } else {
                let stderr = String::from_utf8_lossy(&result.stderr);
                eprintln!("SoapySDRUtil failed: {}", stderr);
                eprintln!("\nMake sure SoapySDR is installed.");
            }
        }
        Err(e) => {
            eprintln!("Could not run SoapySDRUtil: {}", e);
            eprintln!("\nTrying alternate method (rtl_test)...\n");

            // Fallback: try rtl_test
            let rtl_output = Command::new("rtl_test")
                .arg("-t")
                .output();

            match rtl_output {
                Ok(result) => {
                    let stdout = String::from_utf8_lossy(&result.stdout);
                    let stderr = String::from_utf8_lossy(&result.stderr);
                    println!("{}{}", stdout, stderr);
                }
                Err(_) => {
                    eprintln!("Could not run rtl_test either.");
                    eprintln!("\nPlease install one of the following:");
                    eprintln!("  • SoapySDR (recommended): brew install soapysdr");
                    eprintln!("  • rtl-sdr tools: brew install librtlsdr");
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Handle device listing
    if args.list_devices {
        list_sdr_devices()?;
        return Ok(());
    }

    // Log startup configuration and SDR backend availability
    println!("AirJedi starting up...");

    // Detect which SDR backends are compiled in
    let mut backends = Vec::new();
    if cfg!(feature = "soapy") {
        backends.push("SoapySDR");
    }
    if cfg!(feature = "rtlsdr") {
        backends.push("RTL-SDR");
    }
    if cfg!(feature = "aaronia_http") {
        backends.push("Aaronia HTTP");
    }

    if backends.is_empty() {
        println!("WARNING: No SDR backends compiled in! (built with --no-default-features)");
        println!("         This binary cannot connect to SDR hardware.");
        println!("         To fix this issue:");
        println!("         1. Install SoapySDR on your system:");
        println!("            - Raspberry Pi: sudo apt install soapysdr-tools libsoapysdr-dev");
        println!("            - macOS: brew install soapysdr");
        println!("         2. Rebuild the binary natively on this system:");
        println!("            cargo build --release");
        println!("         3. Or cross-compile with SDR features enabled (advanced)");
        println!();
    } else {
        println!("Compiled SDR backends: {}", backends.join(", "));
    }

    let mut fg = Flowgraph::new();
    futuresdr::runtime::init();

    let src = match args.file {
        Some(f) => {
            let file_src_block = fg.add_block(FileSource::<Complex32>::new(f, false))?;
            let throttle_block = fg.add_block(Throttle::<Complex32>::new(args.sample_rate))?;
            fg.connect_stream(file_src_block, "out", throttle_block, "in")?;
            throttle_block
        }
        None => {
            // Check if SDR devices are available before attempting to connect
            if !check_sdr_devices() {
                eprintln!("Error: No RTL-SDR or compatible SDR devices found!");
                eprintln!("\nTroubleshooting:");
                eprintln!("  • Make sure your RTL-SDR dongle is plugged in");
                eprintln!("  • Check that RTL-SDR drivers are installed (rtl-sdr)");
                eprintln!("  • Verify SoapySDR is installed with RTL-SDR support:");
                eprintln!("    - macOS: brew install soapysdr soapyrtlsdr");
                eprintln!("    - Linux: apt install soapysdr-tools soapysdr-module-rtlsdr");
                eprintln!("  • Try running with sudo if you have permissions issues");
                eprintln!("\nFor detailed device information, run:");
                eprintln!("  airjedi --list-devices");
                anyhow::bail!("No SDR devices available");
            }

            // Log SourceBuilder configuration
            println!("Configuring SDR source:");
            println!("  Frequency: {:.2} MHz", 1090.0);
            println!("  Sample rate: {:.2} MHz", args.sample_rate / 1e6);
            println!("  Gain: {:.1} dB", args.gain);
            if let Some(ref ant) = args.antenna {
                println!("  Antenna: {}", ant);
            }
            if let Some(ref a) = args.args {
                println!("  Args: {}", a);
            }
            println!();

            // Load seify source
            println!("Attempting to connect to SDR device...");
            let builder = SourceBuilder::new()
                .frequency(1090e6)
                .sample_rate(args.sample_rate)
                .gain(args.gain)
                .antenna(args.antenna.clone())
                .args(args.args.clone())?;

            let src = match builder.build() {
                Ok(source) => {
                    println!("Successfully connected to SDR device!");
                    source
                }
                Err(e) => {
                    eprintln!("\nERROR: Failed to connect to SDR device!");
                    eprintln!("Error details: {}", e);
                    eprintln!();

                    // Provide context-specific troubleshooting
                    if backends.is_empty() {
                        eprintln!("ROOT CAUSE: No SDR backends are compiled into this binary.");
                        eprintln!("  This binary was built with --no-default-features,");
                        eprintln!("  which excludes SoapySDR and other SDR driver support.");
                        eprintln!();
                        eprintln!("SOLUTION:");
                        eprintln!("  1. Install SoapySDR and RTL-SDR drivers on this system:");
                        eprintln!("     sudo apt install soapysdr-tools libsoapysdr-dev soapysdr-module-rtlsdr");
                        eprintln!("  2. Rebuild the binary natively on this system:");
                        eprintln!("     cargo build --release");
                        eprintln!("     (This will automatically include SoapySDR support)");
                        eprintln!();
                        eprintln!("NOTE: The cross-compiled binary cannot access SDR hardware.");
                        eprintln!("      You must rebuild natively for full SDR functionality.");
                    } else {
                        eprintln!("TROUBLESHOOTING:");
                        eprintln!("  • Verify your SDR device is properly connected");
                        eprintln!("  • Check USB connection and power");
                        eprintln!("  • Try running: SoapySDRUtil --find");
                        eprintln!("  • Check for permission issues (may need sudo)");
                        eprintln!("  • Verify driver installation: SoapySDRUtil --info");
                    }
                    eprintln!();

                    return Err(anyhow::anyhow!("Failed to connect to SDR device: {}", e));
                }
            };

            fg.add_block(src)?
        }
    };

    // Change sample rate to our demodulator sample rate.
    // Using a sample rate higher than the signal bandwidth allows
    // us to use a simple symbol synchronization mechanism and have
    // more clear symbol transitions.
    let gcd = num_integer::gcd(args.sample_rate as usize, DEMOD_SAMPLE_RATE);
    let interp = DEMOD_SAMPLE_RATE / gcd;
    let decim = args.sample_rate as usize / gcd;
    if interp > 100 || decim > 100 {
        warn!(
            "Warning: Interpolation/decimation factor is large. \
             Use a sampling frequency that is a divisor of {DEMOD_SAMPLE_RATE} for the best performance."
        );
    }
    let interp_block = fg.add_block(FirBuilder::resampling::<Complex32, Complex32>(
        interp, decim,
    ))?;
    fg.connect_stream(src, "out", interp_block, "in")?;

    let complex_to_mag_2 = fg.add_block(Apply::new(|i: &Complex32| i.norm_sqr()))?;
    fg.connect_stream(interp_block, "out", complex_to_mag_2, "in")?;

    let nf_est_block = fg.add_block(FirBuilder::new::<f32, f32, _>(vec![1.0f32 / 32.0; 32]))?;
    fg.connect_stream(complex_to_mag_2, "out", nf_est_block, "in")?;

    let preamble_taps: Vec<f32> = PreambleDetector::preamble_correlator_taps();
    let preamble_corr_block = fg.add_block(FirBuilder::new::<f32, f32, _>(preamble_taps))?;
    fg.connect_stream(complex_to_mag_2, "out", preamble_corr_block, "in")?;

    let preamble_detector = fg.add_block(PreambleDetector::new(args.preamble_threshold))?;
    fg.connect_stream(complex_to_mag_2, "out", preamble_detector, "in_samples")?;
    fg.connect_stream(nf_est_block, "out", preamble_detector, "in_nf")?;
    fg.connect_stream(
        preamble_corr_block,
        "out",
        preamble_detector,
        "in_preamble_corr",
    )?;

    let adsb_demod = fg.add_block(Demodulator::new())?;
    fg.connect_stream(preamble_detector, "out", adsb_demod, "in")?;

    let adsb_decoder = fg.add_block(Decoder::new(false))?;
    fg.connect_message(adsb_demod, "out", adsb_decoder, "in")?;

    // Set up dynamic output module system
    let mut output_manager = OutputModuleManager::new();

    // Register raw output modules (BEAST, Raw, AVR)
    if args.beast && !args.no_beast {
        let config = airjedi::OutputModuleConfig::new("beast", args.beast_port).with_buffer_capacity(1024);
        match BeastOutput::new(config).await {
            Ok(module) => {
                println!("BEAST mode server started on port {}", args.beast_port);
                output_manager.add_raw_module(Box::new(module));
            }
            Err(e) => eprintln!("Failed to start BEAST server: {}", e),
        }
    }

    if args.avr {
        let config = airjedi::OutputModuleConfig::new("avr", args.avr_port).with_buffer_capacity(1024);
        match AvrOutput::new(config).await {
            Ok(module) => {
                println!("AVR format server started on port {}", args.avr_port);
                output_manager.add_raw_module(Box::new(module));
            }
            Err(e) => eprintln!("Failed to start AVR server: {}", e),
        }
    }

    if args.raw && !args.no_raw {
        let config = airjedi::OutputModuleConfig::new("raw", args.raw_port).with_buffer_capacity(1024);
        match RawOutput::new(config).await {
            Ok(module) => {
                println!("Raw format server started on port {}", args.raw_port);
                output_manager.add_raw_module(Box::new(module));
            }
            Err(e) => eprintln!("Failed to start raw server: {}", e),
        }
    }

    if args.websocket {
        let config = airjedi::OutputModuleConfig::new("websocket", args.websocket_port).with_buffer_capacity(1024);
        match WebSocketOutput::new(config).await {
            Ok(module) => {
                println!("WebSocket server started on port {} (SBS-1 format)", args.websocket_port);
                output_manager.add_state_module(Box::new(module));
            }
            Err(e) => eprintln!("Failed to start WebSocket server: {}", e),
        }
    }

    // Register state output modules (SBS-1, WebSocket)
    if args.sbs1 {
        let config = airjedi::OutputModuleConfig::new("sbs1", args.sbs1_port).with_buffer_capacity(1024);
        match Sbs1Output::new(config).await {
            Ok(module) => {
                println!("SBS-1/BaseStation format server started on port {}", args.sbs1_port);
                output_manager.add_state_module(Box::new(module));
            }
            Err(e) => eprintln!("Failed to start SBS-1 server: {}", e),
        }
    }

    // Create tracker with dynamic output module system and optional rate limiting
    let prune_after = args.lifetime.map(Duration::from_secs);
    let tracker = if args.rate_limit {
        let rate_config = RateLimitConfig {
            position_interval: Duration::from_millis(args.position_rate_ms),
            velocity_interval: Duration::from_millis(args.velocity_rate_ms),
            identification_interval: Duration::from_millis(args.identification_rate_ms),
            metadata_interval: Duration::from_millis(args.metadata_rate_ms),
        };
        println!(
            "Rate limiting enabled: Position {}ms, Velocity {}ms, ID {}ms, Metadata {}ms",
            args.position_rate_ms, args.velocity_rate_ms, args.identification_rate_ms, args.metadata_rate_ms
        );
        Tracker::new_with_modules_and_rate_limiting(prune_after, output_manager, Some(rate_config))
    } else {
        Tracker::new_with_modules(prune_after, output_manager)
    };
    
    let adsb_tracker = fg.add_block(tracker)?;
    fg.connect_message(adsb_decoder, "out", adsb_tracker, "in")?;

    println!("Please open the map in the browser: http://127.0.0.1:1337/");
    Runtime::new().run(fg)?;

    Ok(())
}
