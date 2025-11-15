//! JIT (Just-In-Time) Liquidity detection
//!
//! Detects JIT liquidity provision where LPs add liquidity right before a large swap
//! and remove it immediately after to capture fees without long-term IL risk

use super::types::*;
use crate::dex::ParsedSwap;
use crate::types::{FetchedBlock, FetchedTransaction};
use anyhow::Result;
use std::collections::HashMap;

/// JIT liquidity detector
pub struct JitDetector {
    /// Maximum slot distance between add and remove
    max_slot_distance: u64,

    /// Minimum target swap size
    min_target_swap_size: u64,

    /// Minimum profit threshold
    min_profit_lamports: i64,
}

impl JitDetector {
    /// Create new JIT detector
    pub fn new(
        max_slot_distance: u64,
        min_target_swap_size: u64,
        min_profit_lamports: i64,
    ) -> Self {
        Self {
            max_slot_distance,
            min_target_swap_size,
            min_profit_lamports,
        }
    }

    /// Detect JIT liquidity in a single block
    pub fn detect(&self, block: &FetchedBlock, swaps: &[ParsedSwap]) -> Result<Vec<MevEvent>> {
        let mut jit_events = Vec::new();

        // Identify add/remove liquidity operations and large swaps
        let lp_operations = self.identify_lp_operations(block)?;
        let large_swaps = self.identify_large_swaps(swaps);

        // Look for JIT patterns: add liquidity -> large swap -> remove liquidity
        for large_swap in &large_swaps {
            if let Some(jit) = self.detect_jit_pattern(&lp_operations, large_swap, block)? {
                jit_events.push(jit);
            }
        }

        Ok(jit_events)
    }

    /// Identify liquidity provision operations
    fn identify_lp_operations(&self, block: &FetchedBlock) -> Result<Vec<LpOperation>> {
        let mut operations = Vec::new();

        for (idx, tx) in block.transactions.iter().enumerate() {
            if let Some(ops) = self.parse_lp_operations(tx, idx)? {
                operations.extend(ops);
            }
        }

        Ok(operations)
    }

    /// Parse LP operations from a transaction
    fn parse_lp_operations(
        &self,
        tx: &FetchedTransaction,
        tx_index: usize,
    ) -> Result<Option<Vec<LpOperation>>> {
        let _transaction = match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => ui_tx,
            _ => return Ok(None),
        };

        let mut operations = Vec::new();

        // Check logs for liquidity events
        if let Some(meta) = &tx.meta {
            if let solana_transaction_status::option_serializer::OptionSerializer::Some(logs) =
                &meta.log_messages
            {
                for log in logs {
                    if log.contains("AddLiquidity") || log.contains("add_liquidity") {
                        operations.push(LpOperation {
                            op_type: LpOpType::Add,
                            pool: "Unknown".to_string(),
                            user: tx.signer().unwrap_or_default(),
                            amount: 0, // Would parse from log
                            signature: tx.signature.clone(),
                            tx_index,
                        });
                    } else if log.contains("RemoveLiquidity") || log.contains("remove_liquidity") {
                        operations.push(LpOperation {
                            op_type: LpOpType::Remove,
                            pool: "Unknown".to_string(),
                            user: tx.signer().unwrap_or_default(),
                            amount: 0,
                            signature: tx.signature.clone(),
                            tx_index,
                        });
                    }
                }
            }
        }

        if operations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(operations))
        }
    }

    /// Identify large swaps that might be JIT targets
    fn identify_large_swaps<'a>(&self, swaps: &'a [ParsedSwap]) -> Vec<&'a ParsedSwap> {
        swaps
            .iter()
            .filter(|s| s.amount_in >= self.min_target_swap_size)
            .collect()
    }

    /// Detect JIT pattern around a large swap
    fn detect_jit_pattern(
        &self,
        lp_operations: &[LpOperation],
        target_swap: &ParsedSwap,
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Look for: AddLiquidity before swap, RemoveLiquidity after swap
        // Same user, same pool

        let pool = &target_swap.pool;
        let swap_tx_idx = target_swap.tx_index;

        // Find add operations before the swap
        let adds_before: Vec<_> = lp_operations
            .iter()
            .filter(|op| {
                matches!(op.op_type, LpOpType::Add)
                    && op.pool == *pool
                    && op.tx_index < swap_tx_idx
            })
            .collect();

        // Find remove operations after the swap
        let removes_after: Vec<_> = lp_operations
            .iter()
            .filter(|op| {
                matches!(op.op_type, LpOpType::Remove)
                    && op.pool == *pool
                    && op.tx_index > swap_tx_idx
            })
            .collect();

        // Match add/remove pairs by same user
        for add in &adds_before {
            for remove in &removes_after {
                if add.user == remove.user {
                    // Found a JIT pattern!
                    let profit = self.estimate_jit_profit(add, target_swap, remove);

                    if profit >= self.min_profit_lamports {
                        let metadata = JitMetadata {
                            add_liquidity_tx: add.signature.clone(),
                            remove_liquidity_tx: remove.signature.clone(),
                            target_swap_tx: target_swap.signature.clone(),
                            pool: pool.clone(),
                            dex: target_swap.dex.name().to_string(),
                            liquidity_added: add.amount,
                            fees_earned: self.estimate_fees_earned(add, target_swap),
                            net_profit: profit,
                        };

                        return Ok(Some(MevEvent {
                            mev_type: MevType::JitLiquidity,
                            slot: block.slot,
                            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
                            transactions: vec![
                                add.signature.clone(),
                                target_swap.signature.clone(),
                                remove.signature.clone(),
                            ],
                            profit_lamports: Some(profit),
                            profit_usd: None,
                            tokens: vec![target_swap.token_in.clone(), target_swap.token_out.clone()],
                            metadata: MevMetadata::JitLiquidity(metadata),
                            extractor: Some(add.user.clone()),
                            confidence: 0.85,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Estimate JIT profit
    fn estimate_jit_profit(
        &self,
        _add: &LpOperation,
        target_swap: &ParsedSwap,
        _remove: &LpOperation,
    ) -> i64 {
        // Simplified: estimate fees earned from the target swap
        // Real calculation would need pool fee rate and exact liquidity amounts

        // Typical fee is 0.3% (30 bps) on most AMMs
        let fee_bps = 30;
        let fees_earned = (target_swap.amount_in as i64 * fee_bps) / 10000;

        // Subtract gas costs
        let gas_cost = 3 * 5000; // 3 transactions
        fees_earned - gas_cost
    }

    /// Estimate fees earned from target swap
    fn estimate_fees_earned(&self, _add: &LpOperation, target_swap: &ParsedSwap) -> u64 {
        // Simplified calculation
        let fee_bps = 30;
        (target_swap.amount_in * fee_bps) / 10000
    }

    /// Detect JIT across multiple blocks
    pub fn detect_cross_block(
        &self,
        blocks: &[FetchedBlock],
        all_swaps: &HashMap<u64, Vec<ParsedSwap>>,
    ) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        // Collect all LP operations across blocks
        let mut all_lp_ops: HashMap<u64, Vec<LpOperation>> = HashMap::new();

        for block in blocks {
            let ops = self.identify_lp_operations(block)?;
            all_lp_ops.insert(block.slot, ops);
        }

        // Look for JIT patterns across blocks
        for i in 0..blocks.len().saturating_sub(1) {
            let current_block = &blocks[i];
            let next_block = &blocks[i + 1];

            if next_block.slot - current_block.slot > self.max_slot_distance {
                continue;
            }

            // Get swaps and LP ops
            if let Some(swaps) = all_swaps.get(&current_block.slot) {
                let large_swaps = self.identify_large_swaps(swaps);

                // Check for add in current block, swap in current, remove in next
                // or add in current, swap in next, remove in next
                // (Various patterns)

                // Simplified: just check within blocks for now
                if let Some(lp_ops) = all_lp_ops.get(&current_block.slot) {
                    for swap in &large_swaps {
                        if let Some(jit) = self.detect_jit_pattern(lp_ops, swap, current_block)? {
                            events.push(jit);
                        }
                    }
                }
            }
        }

        Ok(events)
    }
}

/// Liquidity operation type
#[derive(Debug, Clone)]
enum LpOpType {
    Add,
    Remove,
}

/// Liquidity operation
#[derive(Debug, Clone)]
struct LpOperation {
    op_type: LpOpType,
    pool: String,
    user: String,
    amount: u64,
    signature: String,
    tx_index: usize,
}

impl Default for JitDetector {
    fn default() -> Self {
        Self::new(
            2,               // Within 2 slots
            10_000_000,      // 0.01 SOL minimum target swap
            50_000,          // 0.00005 SOL minimum profit
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jit_detector_creation() {
        let detector = JitDetector::default();
        assert_eq!(detector.max_slot_distance, 2);
        assert_eq!(detector.min_target_swap_size, 10_000_000);
    }
}
