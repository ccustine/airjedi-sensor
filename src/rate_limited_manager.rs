use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

use crate::rate_limiter::{
    ItemRateLimiter, RateLimitConfig, RateLimitResult, RateLimitStats, UpdateType,
};

/// A rate-limited state manager that tracks multiple items and enforces update rate limits
#[derive(Debug)]
pub struct RateLimitedStateManager<ItemId, UpdateData> {
    /// Configuration for rate limiting
    config: RateLimitConfig,
    /// Rate limiters for each tracked item
    item_limiters: HashMap<ItemId, ItemRateLimiter<ItemId, UpdateData>>,
    /// How long to keep items without updates before evicting them
    eviction_timeout: Duration,
    /// How often to run cleanup operations
    cleanup_interval: Duration,
    /// When cleanup was last performed
    last_cleanup: Instant,
    /// Statistics about rate limiting performance
    stats: RateLimitStats,
}

impl<ItemId, UpdateData> RateLimitedStateManager<ItemId, UpdateData>
where
    ItemId: Clone + Eq + Hash,
{
    /// Create a new rate-limited state manager with default configuration
    pub fn new() -> Self {
        Self::with_config(RateLimitConfig::default())
    }

    /// Create a new rate-limited state manager with custom configuration
    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            config,
            item_limiters: HashMap::new(),
            eviction_timeout: Duration::from_secs(300), // 5 minutes default
            cleanup_interval: Duration::from_secs(30),   // 30 seconds default
            last_cleanup: Instant::now(),
            stats: RateLimitStats::default(),
        }
    }

    /// Configure the eviction timeout for inactive items
    pub fn with_eviction_timeout(mut self, timeout: Duration) -> Self {
        self.eviction_timeout = timeout;
        self
    }

    /// Configure how often cleanup operations run
    pub fn with_cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Process an update for a specific item
    pub fn process_update(
        &mut self,
        item_id: ItemId,
        update_type: UpdateType,
        data: UpdateData,
    ) -> RateLimitResult<UpdateData> {
        self.stats.total_updates_received += 1;

        // Get or create rate limiter for this item
        let limiter = self
            .item_limiters
            .entry(item_id.clone())
            .or_insert_with(|| ItemRateLimiter::new(item_id.clone()));

        let result = limiter.process_update(update_type, data, &self.config);

        match result {
            RateLimitResult::Allowed(_) => {
                self.stats.updates_allowed_immediately += 1;
            }
            RateLimitResult::RateLimited => {
                self.stats.updates_rate_limited += 1;
            }
        }

        // Perform cleanup if needed
        self.maybe_cleanup();

        result
    }

    /// Process all pending updates that are now ready
    pub fn process_pending_updates(&mut self) -> Vec<(ItemId, UpdateType, UpdateData)> {
        let mut ready_updates = Vec::new();

        for (item_id, limiter) in &mut self.item_limiters {
            let item_updates = limiter.process_pending_updates();
            for (update_type, data) in item_updates {
                ready_updates.push((item_id.clone(), update_type, data));
            }
        }

        ready_updates
    }

    /// Force process all pending updates regardless of timing (useful for shutdown)
    pub fn flush_pending_updates(&mut self) -> Vec<(ItemId, UpdateType, UpdateData)> {
        let mut all_updates = Vec::new();

        for (item_id, limiter) in &mut self.item_limiters {
            // Drain all pending updates
            let pending_updates: Vec<_> = limiter.pending_updates.drain().collect();
            for (update_type, pending) in pending_updates {
                limiter.update_tracker.record_update(update_type);
                all_updates.push((item_id.clone(), update_type, pending.data));
            }
        }

        all_updates
    }

    /// Get statistics about the current state of the rate limiter
    pub fn get_stats(&self) -> RateLimitStats {
        let mut stats = self.stats.clone();
        stats.active_items = self.item_limiters.len() as u64;
        stats.total_pending_updates = self
            .item_limiters
            .values()
            .map(|limiter| limiter.pending_count())
            .sum::<usize>() as u64;
        stats
    }

    /// Get the current rate limit configuration
    pub fn get_config(&self) -> &RateLimitConfig {
        &self.config
    }

    /// Update the rate limit configuration
    pub fn update_config(&mut self, config: RateLimitConfig) {
        self.config = config;
    }

    /// Get the number of currently tracked items
    pub fn item_count(&self) -> usize {
        self.item_limiters.len()
    }

    /// Get the total number of pending updates across all items
    pub fn total_pending_count(&self) -> usize {
        self.item_limiters
            .values()
            .map(|limiter| limiter.pending_count())
            .sum()
    }

    /// Check if a specific item is being tracked
    pub fn is_tracking(&self, item_id: &ItemId) -> bool {
        self.item_limiters.contains_key(item_id)
    }

    /// Get pending update count for a specific item
    pub fn pending_count_for_item(&self, item_id: &ItemId) -> usize {
        self.item_limiters
            .get(item_id)
            .map(|limiter| limiter.pending_count())
            .unwrap_or(0)
    }

    /// Manually evict a specific item from tracking
    pub fn evict_item(&mut self, item_id: &ItemId) -> bool {
        self.item_limiters.remove(item_id).is_some()
    }

    /// Perform cleanup operations if enough time has passed
    fn maybe_cleanup(&mut self) {
        if self.last_cleanup.elapsed() >= self.cleanup_interval {
            self.cleanup();
        }
    }

    /// Remove items that haven't been seen for longer than the eviction timeout
    pub fn cleanup(&mut self) {
        let eviction_timeout = self.eviction_timeout;
        let items_before = self.item_limiters.len();

        self.item_limiters
            .retain(|_, limiter| !limiter.should_evict(eviction_timeout));

        let items_evicted = items_before - self.item_limiters.len();
        if items_evicted > 0 {
            futuresdr::tracing::debug!("Evicted {} inactive items from rate limiter", items_evicted);
        }

        self.last_cleanup = Instant::now();
    }

    /// Force cleanup all items (useful for testing or shutdown)
    pub fn clear_all(&mut self) {
        self.item_limiters.clear();
        self.stats = RateLimitStats::default();
    }
}

impl<ItemId, UpdateData> Default for RateLimitedStateManager<ItemId, UpdateData>
where
    ItemId: Clone + Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

/// Builder pattern for configuring a RateLimitedStateManager
pub struct RateLimitedStateManagerBuilder<ItemId, UpdateData> {
    config: RateLimitConfig,
    eviction_timeout: Duration,
    cleanup_interval: Duration,
    _phantom: std::marker::PhantomData<(ItemId, UpdateData)>,
}

impl<ItemId, UpdateData> Default for RateLimitedStateManagerBuilder<ItemId, UpdateData> {
    fn default() -> Self {
        Self {
            config: RateLimitConfig::default(),
            eviction_timeout: Duration::from_secs(300),
            cleanup_interval: Duration::from_secs(30),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<ItemId, UpdateData> RateLimitedStateManagerBuilder<ItemId, UpdateData>
where
    ItemId: Clone + Eq + Hash,
{
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_rate_config(mut self, config: RateLimitConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_position_interval(mut self, interval: Duration) -> Self {
        self.config.position_interval = interval;
        self
    }

    pub fn with_velocity_interval(mut self, interval: Duration) -> Self {
        self.config.velocity_interval = interval;
        self
    }

    pub fn with_identification_interval(mut self, interval: Duration) -> Self {
        self.config.identification_interval = interval;
        self
    }

    pub fn with_metadata_interval(mut self, interval: Duration) -> Self {
        self.config.metadata_interval = interval;
        self
    }

    pub fn with_eviction_timeout(mut self, timeout: Duration) -> Self {
        self.eviction_timeout = timeout;
        self
    }

    pub fn with_cleanup_interval(mut self, interval: Duration) -> Self {
        self.cleanup_interval = interval;
        self
    }

    pub fn build(self) -> RateLimitedStateManager<ItemId, UpdateData> {
        RateLimitedStateManager::with_config(self.config)
            .with_eviction_timeout(self.eviction_timeout)
            .with_cleanup_interval(self.cleanup_interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_manager_creation() {
        let manager: RateLimitedStateManager<String, &str> = RateLimitedStateManager::new();
        assert_eq!(manager.item_count(), 0);
        assert_eq!(manager.total_pending_count(), 0);
    }

    #[test]
    fn test_manager_update_processing() {
        let mut manager = RateLimitedStateManager::new();

        // First update should be allowed
        let result = manager.process_update("item1".to_string(), UpdateType::Position, "data1");
        assert!(matches!(result, RateLimitResult::Allowed(_)));
        assert_eq!(manager.item_count(), 1);

        // Immediate second update should be rate limited
        let result = manager.process_update("item1".to_string(), UpdateType::Position, "data2");
        assert!(matches!(result, RateLimitResult::RateLimited));
        assert_eq!(manager.total_pending_count(), 1);
    }

    #[test]
    fn test_manager_pending_processing() {
        let mut manager = RateLimitedStateManager::with_config(RateLimitConfig {
            position_interval: Duration::from_millis(100),
            ..Default::default()
        });

        // Add rate limited update
        let result = manager.process_update("item1".to_string(), UpdateType::Position, "data1");
        assert!(matches!(result, RateLimitResult::Allowed(_)));

        let result = manager.process_update("item1".to_string(), UpdateType::Position, "data2");
        assert!(matches!(result, RateLimitResult::RateLimited));

        // No pending updates should be ready immediately
        let ready = manager.process_pending_updates();
        assert!(ready.is_empty());

        // Wait for rate limit to expire
        sleep(Duration::from_millis(150));

        // Now pending update should be ready
        let ready = manager.process_pending_updates();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].0, "item1");
        assert_eq!(ready[0].1, UpdateType::Position);
    }

    #[test]
    fn test_manager_statistics() {
        let mut manager = RateLimitedStateManager::new();

        manager.process_update("item1".to_string(), UpdateType::Position, "data1");
        manager.process_update("item1".to_string(), UpdateType::Position, "data2");
        manager.process_update("item2".to_string(), UpdateType::Velocity, "data3");

        let stats = manager.get_stats();
        assert_eq!(stats.total_updates_received, 3);
        assert_eq!(stats.updates_allowed_immediately, 2); // First position + first velocity
        assert_eq!(stats.updates_rate_limited, 1); // Second position
        assert_eq!(stats.active_items, 2);
    }

    #[test]
    fn test_manager_builder() {
        let manager: RateLimitedStateManager<String, &str> =
            RateLimitedStateManagerBuilder::new()
                .with_position_interval(Duration::from_millis(200))
                .with_eviction_timeout(Duration::from_secs(60))
                .build();

        assert_eq!(
            manager.get_config().position_interval,
            Duration::from_millis(200)
        );
    }
}