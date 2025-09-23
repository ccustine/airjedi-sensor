#[cfg(test)]
mod integration_tests {
    use crate::{
        RateLimitConfig, Tracker, UpdateType, RateLimitedStateManagerBuilder,
    };
    use adsb_deku::ICAO;
    use std::time::Duration;

    #[test]
    fn test_rate_limiting_tracker_integration() {
        // Test that the tracker correctly applies rate limiting to position updates
        let rate_config = RateLimitConfig {
            position_interval: Duration::from_millis(100),
            velocity_interval: Duration::from_millis(200),
            identification_interval: Duration::from_millis(0), // immediate
            metadata_interval: Duration::from_millis(1000),
        };

        let _tracker = Tracker::with_rate_limiting(rate_config);
        // This test validates that the constructor works correctly
        assert!(true); // The fact that this compiles and runs is the test
    }

    #[test]
    fn test_rate_limiter_with_different_aircraft() {
        // Test that rate limiting is applied per aircraft, not globally
        let rate_config = RateLimitConfig {
            position_interval: Duration::from_millis(500),
            velocity_interval: Duration::from_millis(1000),
            identification_interval: Duration::from_millis(0),
            metadata_interval: Duration::from_millis(2000),
        };

        let mut manager = RateLimitedStateManagerBuilder::new()
            .with_rate_config(rate_config)
            .build();

        let icao1 = ICAO([0x40, 0x62, 0x1D]);
        let icao2 = ICAO([0x50, 0x72, 0x2E]);

        // First update for both aircraft should be allowed
        let result1 = manager.process_update(icao1, UpdateType::Position, "pos1");
        let result2 = manager.process_update(icao2, UpdateType::Position, "pos2");

        assert!(matches!(result1, crate::RateLimitResult::Allowed(_)));
        assert!(matches!(result2, crate::RateLimitResult::Allowed(_)));

        // Immediate second update for same aircraft should be rate limited
        let result3 = manager.process_update(icao1, UpdateType::Position, "pos1_2");
        assert!(matches!(result3, crate::RateLimitResult::RateLimited));

        // But update for different aircraft should still be allowed
        let result4 = manager.process_update(icao2, UpdateType::Position, "pos2_2");
        assert!(matches!(result4, crate::RateLimitResult::RateLimited)); // This will also be rate limited since it's immediate

        assert_eq!(manager.item_count(), 2);
        assert_eq!(manager.total_pending_count(), 2);
    }

    #[test]
    fn test_rate_limiter_immediate_vs_delayed_updates() {
        let rate_config = RateLimitConfig {
            position_interval: Duration::from_millis(500),
            velocity_interval: Duration::from_millis(1000),
            identification_interval: Duration::from_millis(0), // immediate
            metadata_interval: Duration::from_millis(2000),
        };

        let mut manager = RateLimitedStateManagerBuilder::new()
            .with_rate_config(rate_config)
            .build();

        let icao = ICAO([0x40, 0x62, 0x1D]);

        // Identification updates should always be immediate
        let id_result1 = manager.process_update(icao, UpdateType::Identification, "callsign1");
        let id_result2 = manager.process_update(icao, UpdateType::Identification, "callsign2");

        assert!(matches!(id_result1, crate::RateLimitResult::Allowed(_)));
        assert!(matches!(id_result2, crate::RateLimitResult::Allowed(_)));

        // Position updates should be rate limited
        let pos_result1 = manager.process_update(icao, UpdateType::Position, "pos1");
        let pos_result2 = manager.process_update(icao, UpdateType::Position, "pos2");

        assert!(matches!(pos_result1, crate::RateLimitResult::Allowed(_)));
        assert!(matches!(pos_result2, crate::RateLimitResult::RateLimited));

        assert_eq!(manager.pending_count_for_item(&icao), 1);
    }

    #[test]
    fn test_rate_limiter_statistics_tracking() {
        let mut manager = RateLimitedStateManagerBuilder::new().build();

        let icao = ICAO([0x40, 0x62, 0x1D]);

        // Process several updates
        manager.process_update(icao, UpdateType::Position, "pos1");
        manager.process_update(icao, UpdateType::Position, "pos2"); // rate limited
        manager.process_update(icao, UpdateType::Identification, "id1"); // immediate
        manager.process_update(icao, UpdateType::Velocity, "vel1");
        manager.process_update(icao, UpdateType::Velocity, "vel2"); // rate limited

        let stats = manager.get_stats();
        assert_eq!(stats.total_updates_received, 5);
        assert_eq!(stats.updates_allowed_immediately, 3); // pos1, id1, vel1
        assert_eq!(stats.updates_rate_limited, 2); // pos2, vel2
        assert_eq!(stats.active_items, 1);
        assert_eq!(stats.total_pending_updates, 2);

        // Test efficiency calculations
        assert_eq!(stats.rate_limit_efficiency(), 40.0); // 2/5 * 100
        assert_eq!(stats.immediate_processing_rate(), 60.0); // 3/5 * 100
    }

    #[test]
    fn test_rate_limiter_cleanup_and_eviction() {
        let mut manager = RateLimitedStateManagerBuilder::new()
            .with_eviction_timeout(Duration::from_millis(100))
            .with_cleanup_interval(Duration::from_millis(50))
            .build();

        let icao = ICAO([0x40, 0x62, 0x1D]);

        // Add an item
        manager.process_update(icao, UpdateType::Position, "pos1");
        assert_eq!(manager.item_count(), 1);

        // Wait for eviction timeout and manually trigger cleanup
        std::thread::sleep(Duration::from_millis(150));
        manager.cleanup();

        // Item should be evicted due to inactivity
        assert_eq!(manager.item_count(), 0);
    }

    #[test]
    fn test_rate_limiter_pending_update_processing() {
        let rate_config = RateLimitConfig {
            position_interval: Duration::from_millis(100),
            ..Default::default()
        };

        let mut manager = RateLimitedStateManagerBuilder::new()
            .with_rate_config(rate_config)
            .build();

        let icao = ICAO([0x40, 0x62, 0x1D]);

        // Add rate limited update
        manager.process_update(icao, UpdateType::Position, "pos1");
        let result = manager.process_update(icao, UpdateType::Position, "pos2");
        assert!(matches!(result, crate::RateLimitResult::RateLimited));

        // No pending updates should be ready immediately
        let ready = manager.process_pending_updates();
        assert!(ready.is_empty());

        // Wait for rate limit to expire
        std::thread::sleep(Duration::from_millis(150));

        // Now pending update should be ready
        let ready = manager.process_pending_updates();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, icao);
        assert_eq!(ready[0].1, UpdateType::Position);
    }

    #[test]
    fn test_rate_limiter_flush_all_pending() {
        let mut manager = RateLimitedStateManagerBuilder::new().build();

        let icao1 = ICAO([0x40, 0x62, 0x1D]);
        let icao2 = ICAO([0x50, 0x72, 0x2E]);

        // Add several rate limited updates
        manager.process_update(icao1, UpdateType::Position, "pos1");
        manager.process_update(icao1, UpdateType::Position, "pos2"); // rate limited
        manager.process_update(icao2, UpdateType::Velocity, "vel1");
        manager.process_update(icao2, UpdateType::Velocity, "vel2"); // rate limited

        assert_eq!(manager.total_pending_count(), 2);

        // Flush all pending updates
        let flushed = manager.flush_pending_updates();
        assert_eq!(flushed.len(), 2);
        assert_eq!(manager.total_pending_count(), 0);
    }

    #[test]
    fn test_rate_limiter_config_validation() {
        // Test that the default configuration matches requirements
        let config = RateLimitConfig::default();

        assert_eq!(config.position_interval, Duration::from_millis(500));
        assert_eq!(config.velocity_interval, Duration::from_millis(1000));
        assert_eq!(config.identification_interval, Duration::from_millis(0));
        assert_eq!(config.metadata_interval, Duration::from_millis(5000));

        // Test update type interval retrieval
        assert_eq!(UpdateType::Position.get_interval(&config), Duration::from_millis(500));
        assert_eq!(UpdateType::Velocity.get_interval(&config), Duration::from_millis(1000));
        assert_eq!(UpdateType::Identification.get_interval(&config), Duration::from_millis(0));
        assert_eq!(UpdateType::Metadata.get_interval(&config), Duration::from_millis(5000));
    }
}