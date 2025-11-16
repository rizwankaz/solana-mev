//! Jito tip detection and tracking

use crate::types::FetchedTransaction;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Known Jito tip payment accounts
pub const JITO_TIP_ACCOUNTS: &[&str] = &[
    "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
    "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
    "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
    "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
    "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
    "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
    "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
    "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
];

/// Jito tip payment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TipPayment {
    /// Transaction signature
    pub signature: String,

    /// Slot number
    pub slot: u64,

    /// Tip account that received the payment
    pub tip_account: String,

    /// Tip amount in lamports
    pub amount: u64,

    /// Sender address
    pub sender: String,

    /// Transaction index in block
    pub tx_index: usize,
}

/// Tip tracker for analyzing Jito tip payments
pub struct TipTracker {
    /// Known tip accounts
    tip_accounts: Vec<String>,
}

impl TipTracker {
    /// Create new tip tracker with default Jito accounts
    pub fn new() -> Self {
        Self {
            tip_accounts: JITO_TIP_ACCOUNTS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Create with custom tip accounts
    pub fn with_accounts(accounts: Vec<String>) -> Self {
        Self {
            tip_accounts: accounts,
        }
    }

    /// Detect tip payment in a transaction
    pub fn detect_tip(&self, tx: &FetchedTransaction, slot: u64, tx_index: usize) -> Result<Option<TipPayment>> {
        // Check if transaction is successful
        if !tx.is_success() {
            return Ok(None);
        }

        // Simplified tip detection based on high priority fees
        // In a full implementation, would parse instruction data to detect actual transfers
        // to known Jito tip accounts

        // High priority fee might indicate Jito tip
        if let Some(fee) = tx.fee() {
            if fee > 100_000 { // 0.0001 SOL
                // Simplified: assume this could be a tip
                return Ok(Some(TipPayment {
                    signature: tx.signature.clone(),
                    slot,
                    tip_account: self.tip_accounts[0].clone(), // Simplified
                    amount: fee,
                    sender: tx.signer().unwrap_or_default(),
                    tx_index,
                }));
            }
        }

        Ok(None)
    }

    /// Detect all tips in a block
    pub fn detect_tips_in_block(&self, transactions: &[FetchedTransaction], slot: u64) -> Vec<TipPayment> {
        let mut tips = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            if let Ok(Some(tip)) = self.detect_tip(tx, slot, idx) {
                tips.push(tip);
            }
        }

        tips
    }

    /// Calculate total tips in a block
    pub fn total_tips(&self, tips: &[TipPayment]) -> u64 {
        tips.iter().map(|t| t.amount).sum()
    }

    /// Group tips by sender
    pub fn tips_by_sender(&self, tips: &[TipPayment]) -> HashMap<String, Vec<TipPayment>> {
        let mut by_sender: HashMap<String, Vec<TipPayment>> = HashMap::new();

        for tip in tips {
            by_sender
                .entry(tip.sender.clone())
                .or_default()
                .push(tip.clone());
        }

        by_sender
    }

    /// Get top tippers
    pub fn top_tippers(&self, tips: &[TipPayment], n: usize) -> Vec<(String, u64)> {
        let by_sender = self.tips_by_sender(tips);

        let mut totals: Vec<(String, u64)> = by_sender
            .into_iter()
            .map(|(sender, sender_tips)| {
                let total = sender_tips.iter().map(|t| t.amount).sum();
                (sender, total)
            })
            .collect();

        totals.sort_by_key(|(_, amount)| std::cmp::Reverse(*amount));
        totals.truncate(n);

        totals
    }

    /// Check if an account is a known tip account
    pub fn is_tip_account(&self, account: &str) -> bool {
        self.tip_accounts.contains(&account.to_string())
    }
}

impl Default for TipTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tip_tracker_creation() {
        let tracker = TipTracker::new();
        assert_eq!(tracker.tip_accounts.len(), JITO_TIP_ACCOUNTS.len());
    }

    #[test]
    fn test_is_tip_account() {
        let tracker = TipTracker::new();
        assert!(tracker.is_tip_account(JITO_TIP_ACCOUNTS[0]));
        assert!(!tracker.is_tip_account("invalid_account"));
    }

    #[test]
    fn test_total_tips() {
        let tracker = TipTracker::new();
        let tips = vec![
            TipPayment {
                signature: "sig1".to_string(),
                slot: 1,
                tip_account: JITO_TIP_ACCOUNTS[0].to_string(),
                amount: 100_000,
                sender: "sender1".to_string(),
                tx_index: 0,
            },
            TipPayment {
                signature: "sig2".to_string(),
                slot: 1,
                tip_account: JITO_TIP_ACCOUNTS[0].to_string(),
                amount: 200_000,
                sender: "sender2".to_string(),
                tx_index: 1,
            },
        ];

        assert_eq!(tracker.total_tips(&tips), 300_000);
    }
}
