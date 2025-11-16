//! Adaptive MEV detector with network-aware thresholds

use super::detector::{BlockMevAnalysis, DetectorConfig, MevDetector};
use super::types::*;
use crate::{jito, network};
use crate::jito::{BundleDetector, TipTracker};
use crate::network::NetworkMonitor;
use crate::types::FetchedBlock;
use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, info};

/// Adaptive MEV detector that adjusts thresholds based on network conditions
pub struct AdaptiveMevDetector {
    /// Base MEV detector
    detector: MevDetector,

    /// Network monitor
    network_monitor: Arc<NetworkMonitor>,

    /// Jito bundle detector
    bundle_detector: BundleDetector,

    /// Jito tip tracker
    tip_tracker: TipTracker,

    /// Configuration
    config: AdaptiveConfig,
}

/// Configuration for adaptive detector
#[derive(Debug, Clone)]
pub struct AdaptiveConfig {
    /// Enable network-based threshold adaptation
    pub enable_adaptive_thresholds: bool,

    /// Enable Jito bundle detection
    pub enable_jito_detection: bool,

    /// Multiplier for profit thresholds in high congestion
    pub high_congestion_multiplier: f64,

    /// Update network state every N blocks
    pub network_update_interval: u64,

    /// Base detector configuration
    pub base_config: DetectorConfig,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            enable_adaptive_thresholds: true,
            enable_jito_detection: true,
            high_congestion_multiplier: 2.0,
            network_update_interval: 10,
            base_config: DetectorConfig::default(),
        }
    }
}

impl AdaptiveMevDetector {
    /// Create new adaptive detector
    pub fn new(network_monitor: Arc<NetworkMonitor>, config: AdaptiveConfig) -> Self {
        Self {
            detector: MevDetector::with_config(config.base_config.clone()),
            network_monitor,
            bundle_detector: BundleDetector::default(),
            tip_tracker: TipTracker::default(),
            config,
        }
    }

    /// Create with default configuration
    pub fn with_monitor(network_monitor: Arc<NetworkMonitor>) -> Self {
        Self::new(network_monitor, AdaptiveConfig::default())
    }

    /// Detect MEV in a block with adaptive thresholds
    pub async fn detect_block(&mut self, block: &FetchedBlock) -> Result<EnhancedMevAnalysis> {
        // Update network state
        if block.slot % self.config.network_update_interval == 0 {
            self.network_monitor.update_from_block(block)?;
        }

        // Get adaptive thresholds
        let adaptive_thresholds = if self.config.enable_adaptive_thresholds {
            Some(self.get_adaptive_thresholds())
        } else {
            None
        };

        // Run base MEV detection
        let base_analysis = self.detector.detect_block(block).await?;

        // Detect Jito bundles
        let bundles = if self.config.enable_jito_detection {
            self.bundle_detector.detect_bundles(
                &block.transactions,
                block.slot,
                block.timestamp().unwrap_or_else(chrono::Utc::now),
            )
        } else {
            Vec::new()
        };

        // Detect Jito tips
        let tips = if self.config.enable_jito_detection {
            self.tip_tracker
                .detect_tips_in_block(&block.transactions, block.slot)
        } else {
            Vec::new()
        };

        // Create enhanced analysis
        Ok(EnhancedMevAnalysis {
            base: base_analysis,
            jito_bundles: bundles,
            jito_tips: tips,
            network_state: self.network_monitor.get_state(),
            adaptive_thresholds,
        })
    }

    /// Get adaptive thresholds based on current network state
    fn get_adaptive_thresholds(&self) -> AdaptiveThresholds {
        AdaptiveThresholds {
            min_profit_lamports: self.network_monitor.suggested_min_profit(),
            priority_fee_threshold: self.network_monitor.suggested_priority_fee_threshold(),
            congestion_level: self.network_monitor.congestion_level(),
        }
    }

    /// Get mutable reference to base detector
    pub fn detector_mut(&mut self) -> &mut MevDetector {
        &mut self.detector
    }

    /// Get network monitor
    pub fn network_monitor(&self) -> &Arc<NetworkMonitor> {
        &self.network_monitor
    }

    /// Check if conditions are favorable for MEV detection
    pub fn is_favorable_for_mev(&self) -> bool {
        let state = self.network_monitor.get_state();

        // Network should be healthy
        if !state.is_healthy() {
            return false;
        }

        // Medium to high congestion is often good for MEV
        matches!(
            state.congestion,
            network::CongestionLevel::Medium | network::CongestionLevel::High
        )
    }

    /// Print status including network and MEV info
    pub fn print_status(&self) {
        info!("=== Adaptive MEV Detector Status ===");
        self.network_monitor.print_status();

        info!("Adaptive thresholds enabled: {}", self.config.enable_adaptive_thresholds);
        info!("Jito detection enabled: {}", self.config.enable_jito_detection);
        info!("Favorable for MEV: {}", self.is_favorable_for_mev());
        info!("===================================");
    }
}

/// Enhanced MEV analysis with Jito and network data
#[derive(Debug, Clone)]
pub struct EnhancedMevAnalysis {
    /// Base MEV analysis
    pub base: BlockMevAnalysis,

    /// Detected Jito bundles
    pub jito_bundles: Vec<jito::JitoBundle>,

    /// Detected Jito tips
    pub jito_tips: Vec<jito::TipPayment>,

    /// Network state at time of analysis
    pub network_state: network::NetworkState,

    /// Adaptive thresholds used (if any)
    pub adaptive_thresholds: Option<AdaptiveThresholds>,
}

/// Adaptive thresholds based on network conditions
#[derive(Debug, Clone)]
pub struct AdaptiveThresholds {
    /// Suggested minimum profit threshold
    pub min_profit_lamports: i64,

    /// Suggested priority fee threshold
    pub priority_fee_threshold: u64,

    /// Current congestion level
    pub congestion_level: network::CongestionLevel,
}

impl EnhancedMevAnalysis {
    /// Get total value including MEV profit and Jito tips
    pub fn total_value(&self) -> i64 {
        let mev_profit = self.base.total_profit();
        let jito_tips: i64 = self.jito_tips.iter().map(|t| t.amount as i64).sum();

        mev_profit + jito_tips
    }

    /// Get number of Jito bundles
    pub fn jito_bundle_count(&self) -> usize {
        self.jito_bundles.len()
    }

    /// Get total Jito tips
    pub fn total_jito_tips(&self) -> u64 {
        self.jito_tips.iter().map(|t| t.amount).sum()
    }

    /// Get summary statistics
    pub fn summary(&self) -> String {
        format!(
            "Slot {}: {} MEV events, {} bundles, {:.4} SOL total value (Congestion: {})",
            self.base.slot,
            self.base.events.len(),
            self.jito_bundle_count(),
            self.total_value() as f64 / 1e9,
            self.network_state.congestion.name()
        )
    }

    /// Check if this was a high-value slot
    pub fn is_high_value(&self, threshold_sol: f64) -> bool {
        let total_sol = self.total_value() as f64 / 1e9;
        total_sol >= threshold_sol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adaptive_config() {
        let config = AdaptiveConfig::default();
        assert!(config.enable_adaptive_thresholds);
        assert!(config.enable_jito_detection);
    }
}
