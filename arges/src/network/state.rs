//! Network state tracking and metrics

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Current state of the Solana network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    /// Current slot
    pub current_slot: u64,

    /// Timestamp of last update
    pub last_update: DateTime<Utc>,

    /// Recent metrics
    pub metrics: NetworkMetrics,

    /// Congestion level
    pub congestion: CongestionLevel,

    /// Historical metrics (last N slots)
    pub history: VecDeque<SlotMetrics>,

    /// Maximum history size
    max_history: usize,
}

/// Network performance metrics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Average slot time (ms)
    pub avg_slot_time_ms: f64,

    /// Median priority fee (lamports)
    pub median_priority_fee: u64,

    /// 90th percentile priority fee (lamports)
    pub p90_priority_fee: u64,

    /// Average compute units per transaction
    pub avg_compute_units: u64,

    /// Transactions per slot
    pub avg_tx_per_slot: f64,

    /// Failed transaction rate
    pub failed_tx_rate: f64,

    /// Average block production rate (slots/sec)
    pub block_production_rate: f64,
}

/// Congestion level of the network
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CongestionLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Metrics for a single slot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotMetrics {
    pub slot: u64,
    pub timestamp: DateTime<Utc>,
    pub tx_count: usize,
    pub failed_tx_count: usize,
    pub total_fees: u64,
    pub total_compute_units: u64,
    pub median_priority_fee: u64,
    pub slot_time_ms: Option<u64>,
}

impl NetworkState {
    /// Create new network state tracker
    pub fn new(max_history: usize) -> Self {
        Self {
            current_slot: 0,
            last_update: Utc::now(),
            metrics: NetworkMetrics::default(),
            congestion: CongestionLevel::Low,
            history: VecDeque::with_capacity(max_history),
            max_history,
        }
    }

    /// Update state with new slot data
    pub fn update_slot(&mut self, metrics: SlotMetrics) {
        self.current_slot = metrics.slot;
        self.last_update = metrics.timestamp;

        // Add to history
        self.history.push_back(metrics);
        if self.history.len() > self.max_history {
            self.history.pop_front();
        }

        // Recalculate aggregate metrics
        self.recalculate_metrics();

        // Update congestion level
        self.update_congestion();
    }

    /// Recalculate aggregate metrics from history
    fn recalculate_metrics(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let n = self.history.len();

        // Calculate averages
        let total_tx: usize = self.history.iter().map(|m| m.tx_count).sum();
        let total_failed: usize = self.history.iter().map(|m| m.failed_tx_count).sum();
        let _total_fees: u64 = self.history.iter().map(|m| m.total_fees).sum();
        let total_compute: u64 = self.history.iter().map(|m| m.total_compute_units).sum();

        self.metrics.avg_tx_per_slot = total_tx as f64 / n as f64;
        self.metrics.failed_tx_rate = if total_tx > 0 {
            total_failed as f64 / total_tx as f64
        } else {
            0.0
        };

        self.metrics.avg_compute_units = if total_tx > 0 {
            total_compute / total_tx as u64
        } else {
            0
        };

        // Calculate median priority fee
        let mut fees: Vec<u64> = self.history.iter().map(|m| m.median_priority_fee).collect();
        fees.sort_unstable();
        if !fees.is_empty() {
            self.metrics.median_priority_fee = fees[fees.len() / 2];
            self.metrics.p90_priority_fee = fees[(fees.len() * 9) / 10];
        }

        // Calculate slot times
        if self.history.len() > 1 {
            let mut slot_times = Vec::new();
            for window in self.history.iter().collect::<Vec<_>>().windows(2) {
                if let (Some(prev), Some(curr)) = (window[0].slot_time_ms, window[1].slot_time_ms) {
                    slot_times.push(curr.saturating_sub(prev));
                }
            }

            if !slot_times.is_empty() {
                self.metrics.avg_slot_time_ms =
                    slot_times.iter().sum::<u64>() as f64 / slot_times.len() as f64;
            }
        }

        // Block production rate (inverse of slot time)
        if self.metrics.avg_slot_time_ms > 0.0 {
            self.metrics.block_production_rate = 1000.0 / self.metrics.avg_slot_time_ms;
        }
    }

    /// Update congestion level based on metrics
    fn update_congestion(&mut self) {
        let mut score = 0;

        // High transaction count
        if self.metrics.avg_tx_per_slot > 3000.0 {
            score += 2;
        } else if self.metrics.avg_tx_per_slot > 2000.0 {
            score += 1;
        }

        // High failure rate
        if self.metrics.failed_tx_rate > 0.3 {
            score += 2;
        } else if self.metrics.failed_tx_rate > 0.15 {
            score += 1;
        }

        // High priority fees
        if self.metrics.median_priority_fee > 100_000 {
            score += 2;
        } else if self.metrics.median_priority_fee > 50_000 {
            score += 1;
        }

        // Slow slot times
        if self.metrics.avg_slot_time_ms > 600.0 {
            score += 2;
        } else if self.metrics.avg_slot_time_ms > 500.0 {
            score += 1;
        }

        self.congestion = match score {
            0..=2 => CongestionLevel::Low,
            3..=5 => CongestionLevel::Medium,
            6..=7 => CongestionLevel::High,
            _ => CongestionLevel::Critical,
        };
    }

    /// Get suggested minimum profit threshold based on network state
    pub fn suggested_min_profit_lamports(&self) -> i64 {
        // Base threshold: 0.001 SOL
        let base = 1_000_000i64;

        // Adjust based on congestion
        let multiplier = match self.congestion {
            CongestionLevel::Low => 0.5,
            CongestionLevel::Medium => 1.0,
            CongestionLevel::High => 2.0,
            CongestionLevel::Critical => 5.0,
        };

        // Also consider median priority fee
        let fee_adjustment = (self.metrics.median_priority_fee as f64 / 10_000.0).max(1.0);

        (base as f64 * multiplier * fee_adjustment) as i64
    }

    /// Get suggested priority fee threshold
    pub fn suggested_priority_fee_threshold(&self) -> u64 {
        // Use 90th percentile as threshold
        self.metrics.p90_priority_fee
    }

    /// Check if network is healthy for MEV detection
    pub fn is_healthy(&self) -> bool {
        // Not too congested
        if matches!(self.congestion, CongestionLevel::Critical) {
            return false;
        }

        // Not too many failures
        if self.metrics.failed_tx_rate > 0.5 {
            return false;
        }

        // Recent update
        let age = Utc::now().signed_duration_since(self.last_update);
        if age.num_seconds() > 60 {
            return false;
        }

        true
    }

    /// Get a summary of current network state
    pub fn summary(&self) -> String {
        format!(
            "Slot: {}, Congestion: {:?}, Avg TX/slot: {:.0}, Failed rate: {:.2}%, Median priority fee: {} lamports",
            self.current_slot,
            self.congestion,
            self.metrics.avg_tx_per_slot,
            self.metrics.failed_tx_rate * 100.0,
            self.metrics.median_priority_fee
        )
    }
}

impl CongestionLevel {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            CongestionLevel::Low => "Low",
            CongestionLevel::Medium => "Medium",
            CongestionLevel::High => "High",
            CongestionLevel::Critical => "Critical",
        }
    }

    /// Get color code for visualization
    pub fn color_code(&self) -> &'static str {
        match self {
            CongestionLevel::Low => "green",
            CongestionLevel::Medium => "yellow",
            CongestionLevel::High => "orange",
            CongestionLevel::Critical => "red",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_state_creation() {
        let state = NetworkState::new(100);
        assert_eq!(state.max_history, 100);
        assert_eq!(state.current_slot, 0);
    }

    #[test]
    fn test_congestion_levels() {
        assert_eq!(CongestionLevel::Low.name(), "Low");
        assert_eq!(CongestionLevel::Critical.name(), "Critical");
    }

    #[test]
    fn test_suggested_thresholds() {
        let mut state = NetworkState::new(10);

        // Low congestion
        state.congestion = CongestionLevel::Low;
        state.metrics.median_priority_fee = 10_000;
        let threshold = state.suggested_min_profit_lamports();
        assert!(threshold > 0);

        // High congestion should increase threshold
        state.congestion = CongestionLevel::High;
        let high_threshold = state.suggested_min_profit_lamports();
        assert!(high_threshold > threshold);
    }
}
