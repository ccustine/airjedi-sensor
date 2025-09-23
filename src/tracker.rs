use futuresdr::async_io::Timer;
use futuresdr::macros::async_trait;
use futuresdr::macros::message_handler;
use futuresdr::runtime::BlockMeta;
use futuresdr::runtime::BlockMetaBuilder;
use futuresdr::runtime::Kernel;
use futuresdr::runtime::MessageIo;
use futuresdr::runtime::MessageIoBuilder;
use futuresdr::runtime::Pmt;
use futuresdr::runtime::Result;
use futuresdr::runtime::StreamIo;
use futuresdr::runtime::StreamIoBuilder;
use futuresdr::runtime::TypedBlock;
use futuresdr::runtime::WorkIo;
use futuresdr::tracing::info;
use futuresdr::tracing::warn;
use std::cmp::Ordering;
use std::time::{Duration, Instant};

use crate::decoder::DecoderMetaData;
use crate::output_module::OutputModuleManager;
use crate::rate_limiter::{RateLimitConfig, RateLimitResult, UpdateType};
use crate::rate_limited_manager::RateLimitedStateManager;
use crate::*;

/// The duration considered to be recent when decoding CPR frames
const ADSB_TIME_RECENT: Duration = Duration::new(10, 0);

/// Data types that can be rate limited in the tracker
#[derive(Debug, Clone)]
pub enum TrackerUpdateData {
    Identification(AdsbIdentification),
    Position(AdsbPosition, DecoderMetaData),
    Velocity(AdsbVelocity),
}

pub struct Tracker {
    /// When to prune aircraft from the register.
    prune_after: Option<Duration>,
    /// A register of the received aircraft.
    aircraft_register: AircraftRegister,
    /// Dynamic output module manager for all broadcast formats
    output_manager: OutputModuleManager,
    /// Rate limiter for managing update frequencies
    rate_limiter: Option<RateLimitedStateManager<AdsbIcao, TrackerUpdateData>>,
    /// Track when we last logged statistics
    last_stats_log: Instant,
}

impl Tracker {
    /// Creates a new tracker without pruning.
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> TypedBlock<Self> {
        Self::new_with_modules(None, OutputModuleManager::new())
    }

    /// Creates a new tracker with specified pruning duration
    pub fn with_pruning(after: Duration) -> TypedBlock<Self> {
        Self::new_with_modules(Some(after), OutputModuleManager::new())
    }

    /// Creates a new tracker with rate limiting enabled
    pub fn with_rate_limiting(rate_config: RateLimitConfig) -> TypedBlock<Self> {
        Self::new_with_modules_and_rate_limiting(None, OutputModuleManager::new(), Some(rate_config))
    }

    /// Creates a new tracker with both pruning and rate limiting
    pub fn with_pruning_and_rate_limiting(prune_after: Duration, rate_config: RateLimitConfig) -> TypedBlock<Self> {
        Self::new_with_modules_and_rate_limiting(Some(prune_after), OutputModuleManager::new(), Some(rate_config))
    }

    /// Creates a new tracker with specified output modules and optional pruning
    pub fn new_with_modules(prune_after: Option<Duration>, output_manager: OutputModuleManager) -> TypedBlock<Self> {
        Self::new_with_modules_and_rate_limiting(prune_after, output_manager, None)
    }

    /// Creates a new tracker with full configuration options
    pub fn new_with_modules_and_rate_limiting(
        prune_after: Option<Duration>,
        output_manager: OutputModuleManager,
        rate_config: Option<RateLimitConfig>
    ) -> TypedBlock<Self> {
        let aircraft_register = AircraftRegister {
            register: HashMap::new(),
        };

        let rate_limiter = rate_config.map(|config| {
            RateLimitedStateManager::with_config(config)
                .with_eviction_timeout(prune_after.unwrap_or(Duration::from_secs(300)))
        });

        TypedBlock::new(
            BlockMetaBuilder::new("Tracker").build(),
            StreamIoBuilder::new().build(),
            MessageIoBuilder::new()
                .add_input("in", Self::packet_received)
                .add_input("ctrl_port", Self::handle_ctrl_port)
                .build(),
            Self {
                prune_after,
                aircraft_register,
                output_manager,
                rate_limiter,
                last_stats_log: Instant::now(),
            },
        )
    }

    /// This function handles control port messages.
    #[message_handler]
    async fn handle_ctrl_port(
        &mut self,
        io: &mut WorkIo,
        _mio: &mut MessageIo<Self>,
        _meta: &mut BlockMeta,
        p: Pmt,
    ) -> Result<Pmt> {
        match p {
            Pmt::Null => {
                // Reply with register
                let json = serde_json::to_string(&self.aircraft_register).unwrap();
                Ok(Pmt::String(json))
            }
            Pmt::String(cmd) => {
                match cmd.as_str() {
                    "stats" => {
                        // Return rate limiting statistics if available
                        if let Some(stats) = self.get_rate_limit_stats() {
                            let json = serde_json::to_string(&stats).unwrap();
                            Ok(Pmt::String(json))
                        } else {
                            Ok(Pmt::String("{\"rate_limiting\": \"disabled\"}".to_string()))
                        }
                    }
                    "aircraft" => {
                        // Return aircraft register (same as Pmt::Null for backward compatibility)
                        let json = serde_json::to_string(&self.aircraft_register).unwrap();
                        Ok(Pmt::String(json))
                    }
                    _ => {
                        warn!("Unknown control port command: {}", cmd);
                        Ok(Pmt::String(format!("{{\"error\": \"Unknown command: {}\"}}", cmd)))
                    }
                }
            }
            Pmt::Finished => {
                io.finished = true;
                Ok(Pmt::Ok)
            }
            x => {
                warn!("Received unexpected PMT type: {:?}", x);
                Ok(Pmt::Null)
            }
        }
    }

    /// This function handles received packets passed to the block.
    #[message_handler]
    async fn packet_received(
        &mut self,
        io: &mut WorkIo,
        _mio: &mut MessageIo<Self>,
        _meta: &mut BlockMeta,
        p: Pmt,
    ) -> Result<Pmt> {
        match p {
            Pmt::Any(a) => {
                if let Some(adsb_packet) = a.downcast_ref::<AdsbPacket>() {
                    // We received a packet. Update the register.
                    info!("Received {:?}", adsb_packet);
                    if let adsb_deku::DF::ADSB(adsb) = &adsb_packet.message.df {
                        let metadata = &adsb_packet.decoder_metadata;

                        // Broadcast messages if enabled (always immediate for external consumers)
                        self.broadcast_output_messages(adsb_packet);

                        // Process messages through rate limiter if enabled, otherwise process directly
                        if self.rate_limiter.is_some() {
                            self.process_message_with_rate_limiting(&adsb.icao, &adsb.me, metadata);
                        } else {
                            // Direct processing without rate limiting (legacy behavior)
                            match &adsb.me {
                                adsb_deku::adsb::ME::AircraftIdentification(identification) => self
                                    .aircraft_identification_received(
                                        &adsb.icao,
                                        identification,
                                        metadata,
                                    ),
                                adsb_deku::adsb::ME::AirbornePositionBaroAltitude(altitude)
                                | adsb_deku::adsb::ME::AirbornePositionGNSSAltitude(altitude) => {
                                    self.airborne_position_received(&adsb.icao, altitude, metadata)
                                }
                                adsb_deku::adsb::ME::AirborneVelocity(velocity) => {
                                    self.airborne_velocity_received(&adsb.icao, velocity, metadata)
                                }
                                _ => (),
                            }
                        }
                    }
                }
            }
            Pmt::Finished => {
                io.finished = true;
            }
            x => {
                warn!("Received unexpected PMT type: {:?}", x);
            }
        }
        Ok(Pmt::Ok)
    }

    fn update_last_seen(&mut self, icao: &AdsbIcao) {
        if let Some(rec) = self.aircraft_register.register.get_mut(icao) {
            // Update the time stamp in the register record
            rec.last_seen = SystemTime::now();
        }
    }

    fn register_aircraft(&mut self, icao: &AdsbIcao) {
        // Add an aircraft record to our register map
        let now = SystemTime::now();
        let record = AircraftRecord {
            icao: *icao,
            callsign: None,
            emitter_category: None,
            positions: Vec::new(),
            velocities: Vec::new(),
            last_cpr_even: None,
            last_cpr_odd: None,
            last_seen: now,
        };
        if self.aircraft_register.register.contains_key(icao) {
            warn!("Aircraft {} is already registered and will be reset", icao);
        }
        self.aircraft_register.register.insert(*icao, record);
    }

    fn prune_records(&mut self) {
        if let Some(prune_time) = self.prune_after {
            let now = SystemTime::now();
            self.aircraft_register
                .register
                .retain(|_, v| v.last_seen + prune_time >= now);
        }
    }

    fn aircraft_identification_received(
        &mut self,
        icao: &AdsbIcao,
        identification: &AdsbIdentification,
        _metadata: &DecoderMetaData,
    ) {
        if !self.aircraft_register.register.contains_key(icao) {
            self.register_aircraft(icao);
        }
        let rec = self.aircraft_register.register.get_mut(icao).unwrap();
        rec.callsign = Some(identification.cn.clone());
        rec.emitter_category = Some(identification.ca);
        self.update_last_seen(icao);
    }

    fn airborne_position_received(
        &mut self,
        icao: &AdsbIcao,
        altitude: &AdsbPosition,
        _metadata: &DecoderMetaData,
    ) {
        if !self.aircraft_register.register.contains_key(icao) {
            self.register_aircraft(icao);
        }
        let now = SystemTime::now();
        let rec = self.aircraft_register.register.get_mut(icao).unwrap();

        // Update record
        let cpr_rec = CprFrameRecord {
            cpr_frame: *altitude,
            time: now,
        };
        match altitude.odd_flag {
            adsb_deku::CPRFormat::Even => rec.last_cpr_even = Some(cpr_rec),
            adsb_deku::CPRFormat::Odd => rec.last_cpr_odd = Some(cpr_rec),
        }

        // Check if we can calculate the position. This requires both an odd
        // and an even frame.
        // Make rec immutable
        let rec = self.aircraft_register.register.get(icao).unwrap();
        if rec.last_cpr_even.is_some() && rec.last_cpr_odd.is_some() {
            // The frames must be recent
            let even_cpr_rec = rec.last_cpr_even.as_ref().unwrap();
            let odd_cpr_rec = rec.last_cpr_odd.as_ref().unwrap();
            if even_cpr_rec.time < now + ADSB_TIME_RECENT
                && odd_cpr_rec.time < now + ADSB_TIME_RECENT
            {
                // The CPR frames must be orderd by time
                let (cpr1, cpr2) = match even_cpr_rec.time.cmp(&odd_cpr_rec.time) {
                    Ordering::Less => (even_cpr_rec, odd_cpr_rec),
                    Ordering::Greater | Ordering::Equal => (odd_cpr_rec, even_cpr_rec),
                };
                if let Some(pos) = adsb_deku::cpr::get_position((&cpr1.cpr_frame, &cpr2.cpr_frame))
                {
                    // We got a position!
                    // Add it to the record
                    let new_pos = AircraftPosition {
                        latitude: pos.latitude,
                        longitude: pos.longitude,
                        altitude: altitude.alt,
                        type_code: altitude.tc,
                    };
                    let new_rec = AircraftPositionRecord {
                        position: new_pos,
                        time: now,
                    };
                    let rec = self.aircraft_register.register.get_mut(icao).unwrap();
                    rec.positions.push(new_rec);
                }
            }
        }
        self.update_last_seen(icao);
    }

    fn airborne_velocity_received(
        &mut self,
        icao: &AdsbIcao,
        velocity: &AdsbVelocity,
        _metadata: &DecoderMetaData,
    ) {
        if !self.aircraft_register.register.contains_key(icao) {
            self.register_aircraft(icao);
        }
        let now = SystemTime::now();
        // Calculate the velocity
        if let Some((heading, ground_speed, vertical_rate)) = velocity.calculate() {
            // Add it to the record
            let new_velocity = AircraftVelocity {
                heading: heading as f64,
                ground_speed,
                vertical_rate,
                vertical_rate_source: match velocity.vrate_src {
                    adsb_deku::adsb::VerticalRateSource::BarometricPressureAltitude => {
                        AircraftVerticalRateSource::BarometricPressureAltitude
                    }
                    adsb_deku::adsb::VerticalRateSource::GeometricAltitude => {
                        AircraftVerticalRateSource::GeometricAltitude
                    }
                },
            };
            let new_record = AircraftVelocityRecord {
                velocity: new_velocity,
                time: now,
            };
            let rec = self.aircraft_register.register.get_mut(icao).unwrap();
            rec.velocities.push(new_record);
        }
        self.update_last_seen(icao);
    }

    /// Broadcast an ADS-B packet via all enabled output modules
    fn broadcast_output_messages(&self, adsb_packet: &AdsbPacket) {
        self.output_manager.broadcast_to_all(&adsb_packet.raw_bytes, &adsb_packet.decoder_metadata);
    }

    /// Process a message through the rate limiter
    fn process_message_with_rate_limiting(
        &mut self,
        icao: &AdsbIcao,
        me: &adsb_deku::adsb::ME,
        metadata: &DecoderMetaData,
    ) {
        let rate_limiter = self.rate_limiter.as_mut().unwrap();
        match me {
            adsb_deku::adsb::ME::AircraftIdentification(identification) => {
                let update_data = TrackerUpdateData::Identification(identification.clone());
                match rate_limiter.process_update(*icao, UpdateType::Identification, update_data) {
                    RateLimitResult::Allowed(TrackerUpdateData::Identification(id)) => {
                        self.aircraft_identification_received(icao, &id, metadata);
                    }
                    RateLimitResult::RateLimited => {
                        // Will be processed later when rate limit allows
                    }
                    _ => unreachable!("Mismatched update data type"),
                }
            }
            adsb_deku::adsb::ME::AirbornePositionBaroAltitude(altitude)
            | adsb_deku::adsb::ME::AirbornePositionGNSSAltitude(altitude) => {
                let update_data = TrackerUpdateData::Position(altitude.clone(), metadata.clone());
                match rate_limiter.process_update(*icao, UpdateType::Position, update_data) {
                    RateLimitResult::Allowed(TrackerUpdateData::Position(pos, meta)) => {
                        self.airborne_position_received(icao, &pos, &meta);
                    }
                    RateLimitResult::RateLimited => {
                        // Will be processed later when rate limit allows
                    }
                    _ => unreachable!("Mismatched update data type"),
                }
            }
            adsb_deku::adsb::ME::AirborneVelocity(velocity) => {
                let update_data = TrackerUpdateData::Velocity(velocity.clone());
                match rate_limiter.process_update(*icao, UpdateType::Velocity, update_data) {
                    RateLimitResult::Allowed(TrackerUpdateData::Velocity(vel)) => {
                        self.airborne_velocity_received(icao, &vel, metadata);
                    }
                    RateLimitResult::RateLimited => {
                        // Will be processed later when rate limit allows
                    }
                    _ => unreachable!("Mismatched update data type"),
                }
            }
            _ => {
                // Other message types are not rate limited
            }
        }
    }

    /// Process pending updates that are now ready
    fn process_pending_updates(&mut self) {
        if let Some(ref mut rate_limiter) = self.rate_limiter {
            let ready_updates = rate_limiter.process_pending_updates();
            for (icao, _update_type, data) in ready_updates {
                match data {
                    TrackerUpdateData::Identification(identification) => {
                        // We need a dummy metadata for consistency
                        let dummy_metadata = DecoderMetaData {
                            preamble_index: 0,
                            preamble_correlation: 0.0,
                            crc_passed: true,
                            timestamp: std::time::SystemTime::now(),
                        };
                        self.aircraft_identification_received(&icao, &identification, &dummy_metadata);
                    }
                    TrackerUpdateData::Position(position, metadata) => {
                        self.airborne_position_received(&icao, &position, &metadata);
                    }
                    TrackerUpdateData::Velocity(velocity) => {
                        let dummy_metadata = DecoderMetaData {
                            preamble_index: 0,
                            preamble_correlation: 0.0,
                            crc_passed: true,
                            timestamp: std::time::SystemTime::now(),
                        };
                        self.airborne_velocity_received(&icao, &velocity, &dummy_metadata);
                    }
                }
            }
        }
    }

    /// Log rate limiting statistics periodically
    fn log_rate_limit_stats(&self) {
        if let Some(stats) = self.get_rate_limit_stats() {
            info!(
                "Rate Limiting Stats: {} total updates, {}% immediate, {}% rate-limited, {} active aircraft, {} pending updates",
                stats.total_updates_received,
                stats.immediate_processing_rate() as u32,
                stats.rate_limit_efficiency() as u32,
                stats.active_items,
                stats.total_pending_updates
            );
        }
    }

    /// Get rate limiting statistics if rate limiting is enabled
    pub fn get_rate_limit_stats(&self) -> Option<crate::rate_limiter::RateLimitStats> {
        self.rate_limiter.as_ref().map(|limiter| limiter.get_stats())
    }
}

#[async_trait]
impl Kernel for Tracker {
    async fn work(
        &mut self,
        _io: &mut WorkIo,
        _sio: &mut StreamIo,
        _mio: &mut MessageIo<Self>,
        _meta: &mut BlockMeta,
    ) -> Result<()> {
        // Process pending rate-limited updates
        self.process_pending_updates();

        // Set up pruning timer.
        // To keep things simple, we just run the prune and cleanup
        // functions every second, although this means that any
        // item may remain for sec. longer than the prune duration.
        if self.prune_after.is_some() || self.rate_limiter.is_some() {
            Timer::after(Duration::from_millis(1000)).await;

            // Prune aircraft records if enabled
            if self.prune_after.is_some() {
                self.prune_records();
            }

            // Cleanup rate limiter if enabled
            if let Some(ref mut rate_limiter) = self.rate_limiter {
                rate_limiter.cleanup();
            }

            // Log rate limiting statistics every 30 seconds
            if self.rate_limiter.is_some() && self.last_stats_log.elapsed() >= Duration::from_secs(30) {
                self.log_rate_limit_stats();
                self.last_stats_log = Instant::now();
            }
        }

        Ok(())
    }
}
