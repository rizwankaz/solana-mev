//! Jito bundle detection

use crate::types::FetchedTransaction;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Detected Jito bundle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoBundle {
    /// Bundle ID (if available)
    pub bundle_id: Option<String>,

    /// Slot where bundle was included
    pub slot: u64,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Transaction signatures in the bundle
    pub transactions: Vec<String>,

    /// Total tip paid for this bundle
    pub total_tip: u64,

    /// Bundle status
    pub status: BundleStatus,

    /// Transaction indices in the block
    pub tx_indices: Vec<usize>,

    /// Bundle submitter (if identifiable)
    pub submitter: Option<String>,
}

/// Status of a Jito bundle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BundleStatus {
    /// All transactions succeeded
    Success,

    /// Some transactions failed
    PartialFailure,

    /// All transactions failed
    Failed,

    /// Status unknown
    Unknown,
}

/// Jito bundle detector
pub struct BundleDetector {
    /// Minimum tip to consider as Jito bundle
    min_tip_threshold: u64,

    /// Maximum transactions in a bundle
    max_bundle_size: usize,
}

impl BundleDetector {
    /// Create new bundle detector
    pub fn new(min_tip_threshold: u64, max_bundle_size: usize) -> Self {
        Self {
            min_tip_threshold,
            max_bundle_size,
        }
    }

    /// Detect bundles in a block
    pub fn detect_bundles(
        &self,
        transactions: &[FetchedTransaction],
        slot: u64,
        timestamp: DateTime<Utc>,
    ) -> Vec<JitoBundle> {
        let mut bundles = Vec::new();

        // Strategy 1: Group consecutive transactions from same sender with high tips
        bundles.extend(self.detect_by_consecutive_txs(transactions, slot, timestamp));

        // Strategy 2: Look for atomic transaction groups (same blockhash, close indices)
        bundles.extend(self.detect_by_atomic_groups(transactions, slot, timestamp));

        bundles
    }

    /// Detect bundles by finding consecutive transactions from same sender
    fn detect_by_consecutive_txs(
        &self,
        transactions: &[FetchedTransaction],
        slot: u64,
        timestamp: DateTime<Utc>,
    ) -> Vec<JitoBundle> {
        let mut bundles = Vec::new();
        let mut i = 0;

        while i < transactions.len() {
            let tx = &transactions[i];

            // Check if this could be start of a bundle (high priority fee)
            if let Some(fee) = tx.fee() {
                if fee >= self.min_tip_threshold {
                    // Look ahead for more txs from same sender
                    let sender = tx.signer().unwrap_or_default();
                    let mut bundle_txs = vec![tx];
                    let mut bundle_indices = vec![i];

                    // Look at next few transactions
                    for j in (i + 1)..std::cmp::min(i + self.max_bundle_size, transactions.len()) {
                        let next_tx = &transactions[j];
                        if next_tx.signer().unwrap_or_default() == sender {
                            bundle_txs.push(next_tx);
                            bundle_indices.push(j);
                        } else {
                            break; // Bundle ended
                        }
                    }

                    // If we found multiple txs, consider it a bundle
                    if bundle_txs.len() > 1 {
                        let status = self.determine_bundle_status(&bundle_txs);
                        let total_tip = bundle_txs
                            .iter()
                            .filter_map(|tx| tx.fee())
                            .sum();

                        bundles.push(JitoBundle {
                            bundle_id: None,
                            slot,
                            timestamp,
                            transactions: bundle_txs.iter().map(|tx| tx.signature.clone()).collect(),
                            total_tip,
                            status,
                            tx_indices: bundle_indices.clone(),
                            submitter: Some(sender),
                        });

                        i = *bundle_indices.last().unwrap();
                    }
                }
            }

            i += 1;
        }

        bundles
    }

    /// Detect bundles by finding atomic transaction groups
    fn detect_by_atomic_groups(
        &self,
        _transactions: &[FetchedTransaction],
        _slot: u64,
        _timestamp: DateTime<Utc>,
    ) -> Vec<JitoBundle> {
        // TODO: Implement atomic group detection
        // Would need to analyze blockhashes and transaction dependencies
        Vec::new()
    }

    /// Determine bundle status from transactions
    fn determine_bundle_status(&self, transactions: &[&FetchedTransaction]) -> BundleStatus {
        let success_count = transactions.iter().filter(|tx| tx.is_success()).count();

        if success_count == transactions.len() {
            BundleStatus::Success
        } else if success_count == 0 {
            BundleStatus::Failed
        } else {
            BundleStatus::PartialFailure
        }
    }

    /// Group bundles by submitter
    pub fn bundles_by_submitter(&self, bundles: &[JitoBundle]) -> HashMap<String, Vec<JitoBundle>> {
        let mut by_submitter: HashMap<String, Vec<JitoBundle>> = HashMap::new();

        for bundle in bundles {
            if let Some(submitter) = &bundle.submitter {
                by_submitter
                    .entry(submitter.clone())
                    .or_default()
                    .push(bundle.clone());
            }
        }

        by_submitter
    }

    /// Get bundle success rate
    pub fn success_rate(&self, bundles: &[JitoBundle]) -> f64 {
        if bundles.is_empty() {
            return 0.0;
        }

        let successful = bundles
            .iter()
            .filter(|b| matches!(b.status, BundleStatus::Success))
            .count();

        successful as f64 / bundles.len() as f64
    }

    /// Get total tips paid
    pub fn total_tips(&self, bundles: &[JitoBundle]) -> u64 {
        bundles.iter().map(|b| b.total_tip).sum()
    }
}

impl Default for BundleDetector {
    fn default() -> Self {
        Self::new(
            100_000,  // 0.0001 SOL minimum tip
            10,       // Max 10 transactions per bundle
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundle_detector_creation() {
        let detector = BundleDetector::default();
        assert_eq!(detector.min_tip_threshold, 100_000);
    }

    #[test]
    fn test_bundle_status() {
        let status = BundleStatus::Success;
        assert_eq!(status, BundleStatus::Success);
        assert_ne!(status, BundleStatus::Failed);
    }
}
