//! MEV classification and profit calculation
//!
//! Classifies transactions and calculates MEV profits

use super::types::*;
use crate::dex::ParsedSwap;
use crate::types::{FetchedBlock, FetchedTransaction};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// MEV classifier for analyzing transactions and calculating metrics
pub struct MevClassifier {
    /// Known MEV bot addresses
    known_bots: HashSet<String>,

    /// Priority fee threshold for suspicious activity
    high_priority_fee_threshold: u64,
}

impl MevClassifier {
    /// Create new classifier
    pub fn new() -> Self {
        Self {
            known_bots: HashSet::new(),
            high_priority_fee_threshold: 100_000, // 0.0001 SOL
        }
    }

    /// Add known bot address
    pub fn add_known_bot(&mut self, address: String) {
        self.known_bots.insert(address);
    }

    /// Check if transaction is from a known MEV bot
    pub fn is_known_bot(&self, address: &str) -> bool {
        self.known_bots.contains(address)
    }

    /// Calculate profit from an MEV event
    pub fn calculate_profit(&self, event: &MevEvent) -> Result<i64> {
        match &event.metadata {
            MevMetadata::Arbitrage(arb) => Ok(arb.net_profit),
            MevMetadata::Sandwich(sandwich) => Ok(sandwich.profit),
            MevMetadata::Liquidation(liq) => Ok(liq.liquidation_bonus),
            MevMetadata::JitLiquidity(jit) => Ok(jit.net_profit),
            MevMetadata::AtomicBackrun(backrun) => Ok(backrun.profit),
            MevMetadata::PriorityFee(_) => {
                // Priority fee MEV is harder to quantify
                event.profit_lamports.ok_or_else(|| {
                    anyhow::anyhow!("No profit data for priority fee MEV")
                })
            }
            MevMetadata::JitoBundle(bundle) => {
                // Net of tip
                let tip = bundle.tip_amount as i64;
                event.profit_lamports
                    .map(|p| p - tip)
                    .ok_or_else(|| anyhow::anyhow!("No profit data for Jito bundle"))
            }
            MevMetadata::Other(_) => {
                event.profit_lamports.ok_or_else(|| {
                    anyhow::anyhow!("No profit data for other MEV type")
                })
            }
        }
    }

    /// Classify transaction by analyzing its characteristics
    pub fn classify_transaction(
        &self,
        tx: &FetchedTransaction,
        swaps: &[ParsedSwap],
    ) -> TransactionClassification {
        let mut classification = TransactionClassification {
            is_mev_related: false,
            has_high_priority_fee: false,
            swap_count: 0,
            unique_dexs: HashSet::new(),
            is_atomic_arbitrage: false,
            is_from_known_bot: false,
        };

        // Check if from known bot
        if let Some(signer) = tx.signer() {
            classification.is_from_known_bot = self.is_known_bot(&signer);
        }

        // Check priority fee
        let fee = tx.fee().unwrap_or(0);
        if fee > self.high_priority_fee_threshold {
            classification.has_high_priority_fee = true;
        }

        // Analyze swaps in this transaction
        let tx_swaps: Vec<_> = swaps
            .iter()
            .filter(|s| s.signature == tx.signature)
            .collect();

        classification.swap_count = tx_swaps.len();

        for swap in &tx_swaps {
            classification.unique_dexs.insert(swap.dex);
        }

        // Check for atomic arbitrage (circular swaps in one tx)
        if tx_swaps.len() >= 2 {
            let first_token = &tx_swaps[0].token_in;
            let last_token = &tx_swaps[tx_swaps.len() - 1].token_out;

            if first_token == last_token {
                classification.is_atomic_arbitrage = true;
                classification.is_mev_related = true;
            }
        }

        // Other MEV indicators
        if classification.swap_count > 2
            || classification.unique_dexs.len() > 1
            || classification.has_high_priority_fee
            || classification.is_from_known_bot
        {
            classification.is_mev_related = true;
        }

        classification
    }

    /// Aggregate MEV metrics for a slot
    pub fn aggregate_slot_metrics(
        &self,
        block: &FetchedBlock,
        events: &[MevEvent],
    ) -> SlotMevMetrics {
        let mut metrics = SlotMevMetrics::new(block.slot, block.timestamp().unwrap_or_else(chrono::Utc::now));

        // Count events by type
        for event in events {
            metrics.add_event(event);
        }

        // Calculate unique extractors
        let extractors: HashSet<_> = events
            .iter()
            .filter_map(|e| e.extractor.as_ref())
            .collect();
        metrics.unique_extractors = extractors.len();

        // Find top extractor
        let mut extractor_profits: HashMap<String, i64> = HashMap::new();
        for event in events {
            if let (Some(extractor), Some(profit)) = (&event.extractor, event.profit_lamports) {
                *extractor_profits.entry(extractor.clone()).or_insert(0) += profit;
            }
        }

        if let Some((top_extractor, _profit)) = extractor_profits
            .iter()
            .max_by_key(|(_, profit)| *profit)
        {
            metrics.top_extractor = Some(top_extractor.clone());
        }

        // Calculate Jito tips
        for event in events {
            if let MevMetadata::JitoBundle(bundle) = &event.metadata {
                metrics.total_jito_tips += bundle.tip_amount;
            }
        }

        metrics
    }

    /// Aggregate MEV metrics for an epoch
    pub fn aggregate_epoch_metrics(
        &self,
        epoch: u64,
        start_slot: u64,
        end_slot: u64,
        slot_metrics: &[SlotMevMetrics],
    ) -> EpochMevMetrics {
        let mut metrics = EpochMevMetrics {
            epoch,
            start_slot,
            end_slot,
            total_slots: (end_slot - start_slot + 1) as usize,
            ..Default::default()
        };

        // Aggregate from slot metrics
        for slot_metric in slot_metrics {
            metrics.total_events += slot_metric.total_events;
            metrics.total_profit_lamports += slot_metric.total_profit_lamports;

            if let Some(usd) = slot_metric.total_profit_usd {
                let current = metrics.total_profit_usd.unwrap_or(0.0);
                metrics.total_profit_usd = Some(current + usd);
            }

            // Aggregate events by type
            for (mev_type, count) in &slot_metric.events_by_type {
                *metrics.events_by_type.entry(*mev_type).or_insert(0) += count;
            }

            if slot_metric.total_events > 0 {
                metrics.slots_with_mev += 1;
            }
        }

        // Calculate average MEV per slot
        if metrics.total_slots > 0 {
            metrics.avg_mev_per_slot =
                metrics.total_profit_lamports as f64 / metrics.total_slots as f64;
        }

        metrics
    }

    /// Identify top extractors across multiple slots
    pub fn identify_top_extractors(
        &self,
        events: &[MevEvent],
        top_n: usize,
    ) -> Vec<(String, i64)> {
        let mut extractor_profits: HashMap<String, i64> = HashMap::new();

        for event in events {
            if let (Some(extractor), Some(profit)) = (&event.extractor, event.profit_lamports) {
                *extractor_profits.entry(extractor.clone()).or_insert(0) += profit;
            }
        }

        let mut sorted: Vec<_> = extractor_profits.into_iter().collect();
        sorted.sort_by_key(|(_, profit)| -profit);
        sorted.truncate(top_n);

        sorted
    }

    /// Calculate MEV concentration (what % of MEV goes to top extractors)
    pub fn calculate_concentration(
        &self,
        events: &[MevEvent],
        top_n: usize,
    ) -> f64 {
        let total_profit: i64 = events
            .iter()
            .filter_map(|e| e.profit_lamports)
            .sum();

        if total_profit == 0 {
            return 0.0;
        }

        let top_extractors = self.identify_top_extractors(events, top_n);
        let top_profit: i64 = top_extractors.iter().map(|(_, p)| p).sum();

        (top_profit as f64 / total_profit as f64) * 100.0
    }
}

/// Transaction classification result
#[derive(Debug, Clone)]
pub struct TransactionClassification {
    pub is_mev_related: bool,
    pub has_high_priority_fee: bool,
    pub swap_count: usize,
    pub unique_dexs: HashSet<crate::dex::DexProtocol>,
    pub is_atomic_arbitrage: bool,
    pub is_from_known_bot: bool,
}

impl Default for MevClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classifier_creation() {
        let classifier = MevClassifier::new();
        assert_eq!(classifier.known_bots.len(), 0);
    }

    #[test]
    fn test_add_known_bot() {
        let mut classifier = MevClassifier::new();
        classifier.add_known_bot("bot123".to_string());

        assert!(classifier.is_known_bot("bot123"));
        assert!(!classifier.is_known_bot("unknown"));
    }
}
