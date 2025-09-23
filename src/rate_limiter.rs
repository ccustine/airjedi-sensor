use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};

/// Configuration for rate limiting different types of updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Minimum interval between position updates (default: 500ms)
    pub position_interval: Duration,
    /// Minimum interval between velocity updates (default: 1000ms)
    pub velocity_interval: Duration,
    /// Minimum interval between identification updates (default: 0ms - immediate)
    pub identification_interval: Duration,
    /// Minimum interval between metadata updates (default: 5000ms)
    pub metadata_interval: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            position_interval: Duration::from_millis(500),
            velocity_interval: Duration::from_millis(1000),
            identification_interval: Duration::from_millis(0), // Immediate
            metadata_interval: Duration::from_millis(5000),
        }
    }
}

/// Types of updates that can be rate limited
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UpdateType {
    Position,
    Velocity,
    Identification,
    Metadata,
}

impl UpdateType {
    /// Get the rate limit interval for this update type
    pub fn get_interval(&self, config: &RateLimitConfig) -> Duration {
        match self {
            UpdateType::Position => config.position_interval,
            UpdateType::Velocity => config.velocity_interval,
            UpdateType::Identification => config.identification_interval,
            UpdateType::Metadata => config.metadata_interval,
        }
    }
}

/// Tracks the last update time for each update type for a specific item
#[derive(Debug, Clone)]
pub struct UpdateTracker {
    last_updates: HashMap<UpdateType, Instant>,
}

impl UpdateTracker {
    pub fn new() -> Self {
        Self {
            last_updates: HashMap::new(),
        }
    }

    /// Check if an update of the given type is allowed based on rate limits
    pub fn is_update_allowed(&self, update_type: UpdateType, config: &RateLimitConfig) -> bool {
        let interval = update_type.get_interval(config);

        // If interval is zero, always allow (immediate updates)
        if interval.is_zero() {
            return true;
        }

        match self.last_updates.get(&update_type) {
            Some(last_update) => {
                let elapsed = last_update.elapsed();
                elapsed >= interval
            }
            None => true, // First update is always allowed
        }
    }

    /// Record that an update of the given type has occurred
    pub fn record_update(&mut self, update_type: UpdateType) {
        self.last_updates.insert(update_type, Instant::now());
    }

    /// Get the time since the last update of the given type
    pub fn time_since_last_update(&self, update_type: UpdateType) -> Option<Duration> {
        self.last_updates.get(&update_type).map(|instant| instant.elapsed())
    }

    /// Get the earliest time when the next update of the given type would be allowed
    pub fn next_allowed_update(&self, update_type: UpdateType, config: &RateLimitConfig) -> Instant {
        let interval = update_type.get_interval(config);

        match self.last_updates.get(&update_type) {
            Some(last_update) => *last_update + interval,
            None => Instant::now(), // Immediate if never updated
        }
    }
}

/// Manages pending updates that are waiting to be processed due to rate limiting
#[derive(Debug)]
pub struct PendingUpdate<T> {
    pub data: T,
    pub received_at: Instant,
    pub next_allowed_update: Instant,
    pub update_type: UpdateType,
}

impl<T> PendingUpdate<T> {
    pub fn new(data: T, update_type: UpdateType, next_allowed: Instant) -> Self {
        Self {
            data,
            received_at: Instant::now(),
            next_allowed_update: next_allowed,
            update_type,
        }
    }

    /// Check if this pending update is ready to be processed
    pub fn is_ready(&self) -> bool {
        Instant::now() >= self.next_allowed_update
    }

    /// How long until this update can be processed
    pub fn time_until_ready(&self) -> Duration {
        self.next_allowed_update.saturating_duration_since(Instant::now())
    }
}

/// Rate limiter for a specific item (e.g., aircraft)
#[derive(Debug)]
pub struct ItemRateLimiter<ItemId, UpdateData> {
    pub item_id: ItemId,
    pub update_tracker: UpdateTracker,
    pub pending_updates: HashMap<UpdateType, PendingUpdate<UpdateData>>,
    pub last_seen: Instant,
}

impl<ItemId, UpdateData> ItemRateLimiter<ItemId, UpdateData>
where
    ItemId: Clone,
{
    pub fn new(item_id: ItemId) -> Self {
        Self {
            item_id,
            update_tracker: UpdateTracker::new(),
            pending_updates: HashMap::new(),
            last_seen: Instant::now(),
        }
    }

    /// Attempt to process an update, either immediately or queue it for later
    pub fn process_update(
        &mut self,
        update_type: UpdateType,
        data: UpdateData,
        config: &RateLimitConfig,
    ) -> RateLimitResult<UpdateData> {
        self.last_seen = Instant::now();

        if self.update_tracker.is_update_allowed(update_type, config) {
            // Update is allowed, process immediately
            self.update_tracker.record_update(update_type);

            // Remove any pending update of the same type as it's now obsolete
            self.pending_updates.remove(&update_type);

            RateLimitResult::Allowed(data)
        } else {
            // Update is rate limited, queue it or replace existing pending update
            let next_allowed = self.update_tracker.next_allowed_update(update_type, config);
            let pending = PendingUpdate::new(data, update_type, next_allowed);

            // Replace any existing pending update of the same type (debouncing)
            self.pending_updates.insert(update_type, pending);

            RateLimitResult::RateLimited
        }
    }

    /// Get ready pending updates and process them
    pub fn process_pending_updates(&mut self) -> Vec<(UpdateType, UpdateData)> {
        let mut ready_updates = Vec::new();
        let mut to_remove = Vec::new();

        for (update_type, pending) in &self.pending_updates {
            if pending.is_ready() {
                to_remove.push(*update_type);
            }
        }

        // Remove processed updates and record them
        for update_type in to_remove {
            if let Some(pending) = self.pending_updates.remove(&update_type) {
                self.update_tracker.record_update(update_type);
                ready_updates.push((update_type, pending.data));
            }
        }

        ready_updates
    }

    /// Check if this item should be evicted due to inactivity
    pub fn should_evict(&self, timeout: Duration) -> bool {
        self.last_seen.elapsed() > timeout
    }

    /// Get the number of pending updates
    pub fn pending_count(&self) -> usize {
        self.pending_updates.len()
    }
}

/// Result of attempting to process an update through the rate limiter
#[derive(Debug)]
pub enum RateLimitResult<T> {
    /// Update was allowed and should be processed immediately
    Allowed(T),
    /// Update was rate limited and has been queued
    RateLimited,
}

/// Statistics about rate limiting performance
#[derive(Debug, Clone, Default, Serialize)]
pub struct RateLimitStats {
    pub total_updates_received: u64,
    pub updates_allowed_immediately: u64,
    pub updates_rate_limited: u64,
    pub updates_dropped_obsolete: u64,
    pub active_items: u64,
    pub total_pending_updates: u64,
}

impl RateLimitStats {
    /// Calculate the rate limiting efficiency (percentage of updates that were rate limited)
    pub fn rate_limit_efficiency(&self) -> f64 {
        if self.total_updates_received == 0 {
            0.0
        } else {
            (self.updates_rate_limited as f64 / self.total_updates_received as f64) * 100.0
        }
    }

    /// Calculate the immediate processing rate
    pub fn immediate_processing_rate(&self) -> f64 {
        if self.total_updates_received == 0 {
            0.0
        } else {
            (self.updates_allowed_immediately as f64 / self.total_updates_received as f64) * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.position_interval, Duration::from_millis(500));
        assert_eq!(config.velocity_interval, Duration::from_millis(1000));
        assert_eq!(config.identification_interval, Duration::from_millis(0));
    }

    #[test]
    fn test_update_tracker_first_update_allowed() {
        let tracker = UpdateTracker::new();
        let config = RateLimitConfig::default();

        assert!(tracker.is_update_allowed(UpdateType::Position, &config));
        assert!(tracker.is_update_allowed(UpdateType::Velocity, &config));
        assert!(tracker.is_update_allowed(UpdateType::Identification, &config));
    }

    #[test]
    fn test_update_tracker_rate_limiting() {
        let mut tracker = UpdateTracker::new();
        let config = RateLimitConfig::default();

        // First update should be allowed
        assert!(tracker.is_update_allowed(UpdateType::Position, &config));
        tracker.record_update(UpdateType::Position);

        // Immediate second update should be blocked
        assert!(!tracker.is_update_allowed(UpdateType::Position, &config));

        // But identification should still be allowed (immediate)
        assert!(tracker.is_update_allowed(UpdateType::Identification, &config));
    }

    #[test]
    fn test_pending_update_ready_check() {
        let data = "test_data";
        let update_type = UpdateType::Position;
        let next_allowed = Instant::now() + Duration::from_millis(100);

        let pending = PendingUpdate::new(data, update_type, next_allowed);

        // Should not be ready immediately
        assert!(!pending.is_ready());

        // Sleep and check again
        sleep(Duration::from_millis(150));
        assert!(pending.is_ready());
    }

    #[test]
    fn test_item_rate_limiter_immediate_processing() {
        let mut limiter = ItemRateLimiter::new("test_item");
        let config = RateLimitConfig::default();

        let result = limiter.process_update(UpdateType::Identification, "callsign", &config);

        match result {
            RateLimitResult::Allowed(data) => assert_eq!(data, "callsign"),
            RateLimitResult::RateLimited => panic!("Identification should be immediate"),
        }
    }

    #[test]
    fn test_item_rate_limiter_queuing() {
        let mut limiter = ItemRateLimiter::new("test_item");
        let config = RateLimitConfig::default();

        // First position update should be allowed
        let result1 = limiter.process_update(UpdateType::Position, "pos1", &config);
        assert!(matches!(result1, RateLimitResult::Allowed(_)));

        // Second immediate position update should be rate limited
        let result2 = limiter.process_update(UpdateType::Position, "pos2", &config);
        assert!(matches!(result2, RateLimitResult::RateLimited));

        assert_eq!(limiter.pending_count(), 1);
    }
}