//! Network monitoring service

use super::state::{NetworkState, SlotMetrics};
use crate::types::FetchedBlock;
use anyhow::Result;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

/// Network monitor that tracks Solana network state
pub struct NetworkMonitor {
    /// Current network state
    state: Arc<RwLock<NetworkState>>,

    /// Configuration
    config: MonitorConfig,
}

/// Configuration for network monitor
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    /// Number of slots to track in history
    pub history_size: usize,

    /// Enable adaptive threshold adjustment
    pub enable_adaptive_thresholds: bool,

    /// Update interval in slots
    pub update_interval_slots: u64,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            history_size: 100,
            enable_adaptive_thresholds: true,
            update_interval_slots: 10,
        }
    }
}

impl NetworkMonitor {
    /// Create new network monitor
    pub fn new(config: MonitorConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(NetworkState::new(config.history_size))),
            config,
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(MonitorConfig::default())
    }

    /// Update state with new block data
    pub fn update_from_block(&self, block: &FetchedBlock) -> Result<()> {
        let metrics = self.extract_metrics(block);

        let mut state = self.state.write().unwrap();
        state.update_slot(metrics);

        if block.slot % self.config.update_interval_slots == 0 {
            debug!("Network state: {}", state.summary());
        }

        Ok(())
    }

    /// Extract metrics from a block
    fn extract_metrics(&self, block: &FetchedBlock) -> SlotMetrics {
        let tx_count = block.transactions.len();
        let failed_tx_count = block.transactions.iter().filter(|tx| !tx.is_success()).count();

        let total_fees = block.total_fees();
        let total_compute_units = block.total_compute_units();

        // Calculate median priority fee
        let mut priority_fees: Vec<u64> = block
            .transactions
            .iter()
            .filter_map(|tx| tx.fee())
            .filter(|&fee| fee > 5000) // Filter out base fees
            .collect();

        priority_fees.sort_unstable();
        let median_priority_fee = if !priority_fees.is_empty() {
            priority_fees[priority_fees.len() / 2]
        } else {
            0
        };

        SlotMetrics {
            slot: block.slot,
            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
            tx_count,
            failed_tx_count,
            total_fees,
            total_compute_units,
            median_priority_fee,
            slot_time_ms: None, // Would need previous block timestamp to calculate
        }
    }

    /// Get current network state (read-only)
    pub fn get_state(&self) -> NetworkState {
        self.state.read().unwrap().clone()
    }

    /// Get suggested minimum profit threshold
    pub fn suggested_min_profit(&self) -> i64 {
        self.state.read().unwrap().suggested_min_profit_lamports()
    }

    /// Get suggested priority fee threshold
    pub fn suggested_priority_fee_threshold(&self) -> u64 {
        self.state
            .read()
            .unwrap()
            .suggested_priority_fee_threshold()
    }

    /// Check if network is healthy for MEV detection
    pub fn is_healthy(&self) -> bool {
        self.state.read().unwrap().is_healthy()
    }

    /// Get current congestion level
    pub fn congestion_level(&self) -> super::state::CongestionLevel {
        self.state.read().unwrap().congestion
    }

    /// Print network status
    pub fn print_status(&self) {
        let state = self.state.read().unwrap();
        info!("=== Network Status ===");
        info!("{}", state.summary());
        info!(
            "Suggested min profit: {} lamports ({:.4} SOL)",
            state.suggested_min_profit_lamports(),
            state.suggested_min_profit_lamports() as f64 / 1e9
        );
        info!(
            "Suggested priority fee threshold: {} lamports",
            state.suggested_priority_fee_threshold()
        );
        info!("Network healthy: {}", state.is_healthy());
        info!("=====================");
    }

    /// Get reference to shared state (for advanced use)
    pub fn state_ref(&self) -> Arc<RwLock<NetworkState>> {
        Arc::clone(&self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monitor_creation() {
        let monitor = NetworkMonitor::default();
        assert!(monitor.is_healthy());
    }

    #[test]
    fn test_custom_config() {
        let config = MonitorConfig {
            history_size: 50,
            enable_adaptive_thresholds: true,
            update_interval_slots: 5,
        };

        let monitor = NetworkMonitor::new(config);
        let state = monitor.get_state();
        assert_eq!(state.history.capacity(), 50);
    }
}
