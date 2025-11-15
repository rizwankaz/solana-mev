//! Comprehensive MEV detector
//!
//! Orchestrates all MEV detection strategies and provides unified interface

use super::arbitrage::ArbitrageDetector;
use super::classifier::MevClassifier;
use super::jit::JitDetector;
use super::liquidation::LiquidationDetector;
use super::sandwich::SandwichDetector;
use super::types::*;
use crate::dex::DexParser;
use crate::types::FetchedBlock;
use anyhow::Result;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Comprehensive MEV detector that orchestrates all detection strategies
pub struct MevDetector {
    /// Arbitrage detector
    arbitrage_detector: ArbitrageDetector,

    /// Sandwich attack detector
    sandwich_detector: SandwichDetector,

    /// Liquidation detector
    liquidation_detector: LiquidationDetector,

    /// JIT liquidity detector
    jit_detector: JitDetector,

    /// MEV classifier
    classifier: MevClassifier,

    /// Configuration
    config: DetectorConfig,
}

/// Configuration for MEV detector
#[derive(Debug, Clone)]
pub struct DetectorConfig {
    /// Enable arbitrage detection
    pub detect_arbitrage: bool,

    /// Enable sandwich detection
    pub detect_sandwich: bool,

    /// Enable liquidation detection
    pub detect_liquidation: bool,

    /// Enable JIT liquidity detection
    pub detect_jit: bool,

    /// Minimum confidence score to report
    pub min_confidence: f64,

    /// Enable cross-block detection
    pub enable_cross_block: bool,

    /// Maximum blocks to analyze for cross-block patterns
    pub cross_block_window: usize,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            detect_arbitrage: true,
            detect_sandwich: true,
            detect_liquidation: true,
            detect_jit: true,
            min_confidence: 0.6,
            enable_cross_block: false,
            cross_block_window: 5,
        }
    }
}

impl MevDetector {
    /// Create new MEV detector with default configuration
    pub fn new() -> Self {
        Self::with_config(DetectorConfig::default())
    }

    /// Create MEV detector with custom configuration
    pub fn with_config(config: DetectorConfig) -> Self {
        Self {
            arbitrage_detector: ArbitrageDetector::default(),
            sandwich_detector: SandwichDetector::default(),
            liquidation_detector: LiquidationDetector::default(),
            jit_detector: JitDetector::default(),
            classifier: MevClassifier::default(),
            config,
        }
    }

    /// Get mutable reference to classifier (for adding known bots, etc.)
    pub fn classifier_mut(&mut self) -> &mut MevClassifier {
        &mut self.classifier
    }

    /// Detect all MEV in a single block
    pub fn detect_block(&self, block: &FetchedBlock) -> Result<BlockMevAnalysis> {
        info!("Analyzing block {} for MEV", block.slot);

        let mut all_events = Vec::new();

        // Parse all swaps from the block
        let swaps = DexParser::parse_block(&block.transactions);
        debug!("Parsed {} swaps from block {}", swaps.len(), block.slot);

        // Run each detector if enabled
        if self.config.detect_arbitrage {
            match self.arbitrage_detector.detect(block, &swaps) {
                Ok(events) => {
                    debug!("Found {} arbitrage events", events.len());
                    all_events.extend(events);
                }
                Err(e) => warn!("Arbitrage detection failed: {}", e),
            }
        }

        if self.config.detect_sandwich {
            match self.sandwich_detector.detect(block, &swaps) {
                Ok(events) => {
                    debug!("Found {} sandwich events", events.len());
                    all_events.extend(events);
                }
                Err(e) => warn!("Sandwich detection failed: {}", e),
            }
        }

        if self.config.detect_liquidation {
            match self.liquidation_detector.detect(block) {
                Ok(events) => {
                    debug!("Found {} liquidation events", events.len());
                    all_events.extend(events);
                }
                Err(e) => warn!("Liquidation detection failed: {}", e),
            }
        }

        if self.config.detect_jit {
            match self.jit_detector.detect(block, &swaps) {
                Ok(events) => {
                    debug!("Found {} JIT liquidity events", events.len());
                    all_events.extend(events);
                }
                Err(e) => warn!("JIT detection failed: {}", e),
            }
        }

        // Filter by confidence
        all_events.retain(|e| e.confidence >= self.config.min_confidence);

        // Calculate metrics
        let metrics = self.classifier.aggregate_slot_metrics(block, &all_events);

        info!(
            "Block {} MEV summary: {} events, {} total profit",
            block.slot,
            all_events.len(),
            metrics.total_profit_lamports
        );

        Ok(BlockMevAnalysis {
            slot: block.slot,
            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
            events: all_events,
            metrics,
            swap_count: swaps.len(),
        })
    }

    /// Detect MEV across multiple blocks
    pub fn detect_blocks(&self, blocks: &[FetchedBlock]) -> Result<Vec<BlockMevAnalysis>> {
        let mut analyses = Vec::new();

        // Analyze each block individually
        for block in blocks {
            match self.detect_block(block) {
                Ok(analysis) => analyses.push(analysis),
                Err(e) => {
                    warn!("Failed to analyze block {}: {}", block.slot, e);
                }
            }
        }

        // If cross-block detection is enabled, look for patterns across blocks
        if self.config.enable_cross_block {
            info!("Running cross-block MEV detection");
            let cross_block_events = self.detect_cross_block_patterns(blocks)?;
            debug!(
                "Found {} cross-block MEV events",
                cross_block_events.len()
            );

            // Add cross-block events to the appropriate block analyses
            for event in cross_block_events {
                if let Some(analysis) = analyses.iter_mut().find(|a| a.slot == event.slot) {
                    analysis.events.push(event);
                }
            }

            // Recalculate metrics after adding cross-block events
            for (i, block) in blocks.iter().enumerate() {
                if i < analyses.len() {
                    analyses[i].metrics = self
                        .classifier
                        .aggregate_slot_metrics(block, &analyses[i].events);
                }
            }
        }

        Ok(analyses)
    }

    /// Detect cross-block MEV patterns
    fn detect_cross_block_patterns(&self, blocks: &[FetchedBlock]) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        // Collect all swaps by slot
        let mut swaps_by_slot: HashMap<u64, Vec<_>> = HashMap::new();
        for block in blocks {
            let swaps = DexParser::parse_block(&block.transactions);
            swaps_by_slot.insert(block.slot, swaps);
        }

        // Run cross-block sandwich detection
        if self.config.detect_sandwich {
            if let Ok(sandwich_events) = self
                .sandwich_detector
                .detect_cross_block(blocks, &swaps_by_slot)
            {
                events.extend(sandwich_events);
            }
        }

        // Run cross-block JIT detection
        if self.config.detect_jit {
            if let Ok(jit_events) = self.jit_detector.detect_cross_block(blocks, &swaps_by_slot) {
                events.extend(jit_events);
            }
        }

        Ok(events)
    }

    /// Aggregate MEV metrics for an epoch
    pub fn aggregate_epoch(
        &self,
        epoch: u64,
        block_analyses: &[BlockMevAnalysis],
    ) -> Result<EpochMevAnalysis> {
        if block_analyses.is_empty() {
            return Ok(EpochMevAnalysis {
                epoch,
                start_slot: 0,
                end_slot: 0,
                metrics: EpochMevMetrics::default(),
                total_blocks: 0,
                blocks_with_mev: 0,
            });
        }

        let start_slot = block_analyses.first().unwrap().slot;
        let end_slot = block_analyses.last().unwrap().slot;

        let slot_metrics: Vec<_> = block_analyses.iter().map(|a| a.metrics.clone()).collect();

        let metrics =
            self.classifier
                .aggregate_epoch_metrics(epoch, start_slot, end_slot, &slot_metrics);

        let blocks_with_mev = block_analyses.iter().filter(|a| !a.events.is_empty()).count();

        // Collect all events for top extractor analysis
        let all_events: Vec<_> = block_analyses
            .iter()
            .flat_map(|a| a.events.iter())
            .cloned()
            .collect();

        let top_extractors = self.classifier.identify_top_extractors(&all_events, 10);
        let concentration = self.classifier.calculate_concentration(&all_events, 10);

        let mut final_metrics = metrics;
        final_metrics.top_extractors = top_extractors;
        final_metrics.concentration_ratio = concentration;

        Ok(EpochMevAnalysis {
            epoch,
            start_slot,
            end_slot,
            metrics: final_metrics,
            total_blocks: block_analyses.len(),
            blocks_with_mev,
        })
    }

    /// Get configuration
    pub fn config(&self) -> &DetectorConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: DetectorConfig) {
        self.config = config;
    }
}

/// Result of analyzing a single block
#[derive(Debug, Clone)]
pub struct BlockMevAnalysis {
    /// Slot number
    pub slot: u64,

    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,

    /// All MEV events detected
    pub events: Vec<MevEvent>,

    /// Aggregated metrics
    pub metrics: SlotMevMetrics,

    /// Total number of swaps in block
    pub swap_count: usize,
}

impl BlockMevAnalysis {
    /// Get events of a specific type
    pub fn events_by_type(&self, mev_type: MevType) -> Vec<&MevEvent> {
        self.events.iter().filter(|e| e.mev_type == mev_type).collect()
    }

    /// Get total profit
    pub fn total_profit(&self) -> i64 {
        self.metrics.total_profit_lamports
    }

    /// Check if block has any MEV
    pub fn has_mev(&self) -> bool {
        !self.events.is_empty()
    }
}

/// Result of analyzing an epoch
#[derive(Debug, Clone)]
pub struct EpochMevAnalysis {
    /// Epoch number
    pub epoch: u64,

    /// Start slot
    pub start_slot: u64,

    /// End slot
    pub end_slot: u64,

    /// Aggregated metrics
    pub metrics: EpochMevMetrics,

    /// Total blocks analyzed
    pub total_blocks: usize,

    /// Blocks with MEV activity
    pub blocks_with_mev: usize,
}

impl EpochMevAnalysis {
    /// Get MEV activity percentage
    pub fn mev_percentage(&self) -> f64 {
        if self.total_blocks == 0 {
            return 0.0;
        }
        (self.blocks_with_mev as f64 / self.total_blocks as f64) * 100.0
    }
}

impl Default for MevDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = MevDetector::new();
        assert!(detector.config.detect_arbitrage);
        assert!(detector.config.detect_sandwich);
    }

    #[test]
    fn test_custom_config() {
        let mut config = DetectorConfig::default();
        config.detect_arbitrage = false;
        config.min_confidence = 0.8;

        let detector = MevDetector::with_config(config);
        assert!(!detector.config.detect_arbitrage);
        assert_eq!(detector.config.min_confidence, 0.8);
    }
}
