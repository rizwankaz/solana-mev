use crate::types::FetchedTransaction;
use crate::mev::types::{JitLiquidity, LiquidityTx, VictimTx, SwapInfo};
use crate::mev::parser::{TransactionParser, TokenTransfer};
use crate::mev::instruction_parser::InstructionClassifier;

/// JIT (Just-In-Time) Liquidity Detector
///
/// Based on Brontes methodology:
///
/// JIT liquidity is a MEV strategy where a searcher:
/// 1. Detects a large pending swap in the mempool
/// 2. Adds concentrated liquidity at the exact price range that will be used
/// 3. The victim's swap executes, paying fees to the concentrated liquidity
/// 4. Searcher immediately removes the liquidity, collecting the fees
///
/// Detection pattern:
/// - Add Liquidity transaction
/// - Large Swap transaction (victim)
/// - Remove Liquidity transaction
/// - All three must involve the same pool
/// - Liquidity add/remove by same searcher
/// - Minimal time between add and remove (usually same block or adjacent blocks)
///
/// This is most common on concentrated liquidity AMMs like:
/// - Orca Whirlpools
/// - Raydium CLMM
/// - Meteora DLMM
pub struct JitLiquidityDetector;

impl JitLiquidityDetector {
    /// Detect JIT liquidity attacks in a block
    ///
    /// Looks for add_liquidity → swap → remove_liquidity patterns
    pub fn detect_in_block(
        transactions: &[&FetchedTransaction],
        slot: u64,
    ) -> Vec<JitLiquidity> {
        let mut jit_attacks = Vec::new();

        // Transactions are already filtered to successful
        let successful_txs = transactions;

        if successful_txs.len() < 3 {
            return jit_attacks; // Need at least 3 txs for JIT pattern
        }

        // Look for add_liquidity → swap → remove_liquidity pattern
        for i in 0..(successful_txs.len().saturating_sub(2)) {
            if let Some(jit) = Self::detect_jit_at_index(successful_txs, i, slot) {
                jit_attacks.push(jit);
            }
        }

        jit_attacks
    }

    /// Detect JIT liquidity starting at a specific index
    fn detect_jit_at_index(
        txs: &[&FetchedTransaction],
        start_idx: usize,
        slot: u64,
    ) -> Option<JitLiquidity> {
        if start_idx + 2 >= txs.len() {
            return None;
        }

        let add_liq_tx = txs[start_idx];

        // Check if this is a liquidity add operation
        if !Self::is_liquidity_add(add_liq_tx) {
            return None;
        }

        let searcher = TransactionParser::get_signer(add_liq_tx)?;
        let add_pools = Self::extract_pools(add_liq_tx);

        if add_pools.is_empty() {
            return None;
        }

        // Search for remove_liquidity within next few transactions
        // JIT typically completes within 5-10 transactions
        let search_window = std::cmp::min(10, txs.len() - start_idx);

        for end_idx in (start_idx + 2)..=(start_idx + search_window) {
            let remove_liq_tx = txs[end_idx];

            // Check if this is remove liquidity by same searcher
            if !Self::is_liquidity_remove(remove_liq_tx) {
                continue;
            }

            let remove_signer = TransactionParser::get_signer(remove_liq_tx)?;
            if remove_signer != searcher {
                continue;
            }

            let remove_pools = Self::extract_pools(remove_liq_tx);

            // Must be same pool
            let common_pool = add_pools
                .iter()
                .find(|p| remove_pools.contains(p))?
                .clone();

            // Look for victim swap between add and remove
            let victim_swap = Self::find_victim_swap(
                &txs[(start_idx + 1)..end_idx],
                &searcher,
                &common_pool,
            )?;

            // Calculate fees collected and profit
            let add_liq = Self::build_liquidity_tx(add_liq_tx, true)?;
            let remove_liq = Self::build_liquidity_tx(remove_liq_tx, false)?;

            let fees_collected = Self::calculate_fees_collected(
                &add_liq,
                &remove_liq,
                &victim_swap,
            );

            let total_cost = add_liq.fee_lamports + remove_liq.fee_lamports;
            let profit_lamports = fees_collected - total_cost as i64;

            return Some(JitLiquidity {
                slot,
                searcher,
                add_liquidity: add_liq,
                victim_swap,
                remove_liquidity: remove_liq,
                pool: common_pool,
                fees_collected_lamports: fees_collected,
                fees_collected_usd: None,
                profit_lamports,
                profit_usd: None,
            });
        }

        None
    }

    /// Check if transaction is a liquidity add operation
    fn is_liquidity_add(tx: &FetchedTransaction) -> bool {
        let transfers = TransactionParser::extract_token_transfers(tx);
        InstructionClassifier::is_add_liquidity(tx, &transfers)
    }

    /// Check if transaction is a liquidity remove operation
    fn is_liquidity_remove(tx: &FetchedTransaction) -> bool {
        let transfers = TransactionParser::extract_token_transfers(tx);
        InstructionClassifier::is_remove_liquidity(tx, &transfers)
    }

    /// Extract pool addresses from transaction
    fn extract_pools(tx: &FetchedTransaction) -> Vec<String> {
        let accounts = TransactionParser::extract_accounts(tx);

        accounts
            .into_iter()
            .filter(|acc| {
                !acc.starts_with("11111111111111") &&
                acc != "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" &&
                acc != "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
            })
            .collect()
    }

    /// Find victim swap between liquidity add and remove
    fn find_victim_swap(
        middle_txs: &[&FetchedTransaction],
        searcher: &str,
        pool: &str,
    ) -> Option<VictimTx> {
        for tx in middle_txs {
            let tx = *tx;
            // Skip transactions by the searcher
            if let Some(signer) = TransactionParser::get_signer(tx) {
                if signer == searcher {
                    continue;
                }

                // Check if transaction involves the pool
                let tx_pools = Self::extract_pools(tx);
                if !tx_pools.contains(&pool.to_string()) {
                    continue;
                }

                // Check if this is a swap
                let tx_transfers = TransactionParser::extract_token_transfers(tx);
                if !InstructionClassifier::is_swap(tx, &tx_transfers) {
                    continue;
                }

                // Found victim swap!
                let transfers = TransactionParser::extract_token_transfers(tx);
                let swap = Self::reconstruct_swap(&transfers, Some(pool));

                // Estimate value of swap
                let swap_value = transfers
                    .iter()
                    .find(|t| t.is_outflow())
                    .map(|t| (t.net_change.abs() * 1_000_000_000.0) as i64)
                    .unwrap_or(0);

                return Some(VictimTx {
                    signature: tx.signature.clone(),
                    tx_index: tx.index,
                    victim_address: signer,
                    swap,
                    loss_lamports: swap_value / 1000, // Simplified: assume 0.1% fee
                    loss_usd: None,
                });
            }
        }

        None
    }

    /// Build liquidity transaction object
    fn build_liquidity_tx(
        tx: &FetchedTransaction,
        is_add: bool,
    ) -> Option<LiquidityTx> {
        let transfers = TransactionParser::extract_token_transfers(tx);

        // Extract the two tokens in the pair
        let token_a_transfer = if is_add {
            transfers.iter().filter(|t| t.is_outflow()).nth(0)
        } else {
            transfers.iter().filter(|t| t.is_inflow()).nth(0)
        };

        let token_b_transfer = if is_add {
            transfers.iter().filter(|t| t.is_outflow()).nth(1)
        } else {
            transfers.iter().filter(|t| t.is_inflow()).nth(1)
        };

        let token_a = token_a_transfer?;
        let token_b = token_b_transfer?;

        Some(LiquidityTx {
            signature: tx.signature.clone(),
            tx_index: tx.index,
            amount_a: (token_a.net_change.abs() * 1_000_000_000.0) as u64,
            amount_b: (token_b.net_change.abs() * 1_000_000_000.0) as u64,
            token_a: token_a.mint.clone(),
            token_b: token_b.mint.clone(),
            tick_lower: None, // Would need to parse instruction data
            tick_upper: None, // Would need to parse instruction data
            compute_units: tx.compute_units_consumed().unwrap_or(0),
            fee_lamports: tx.fee().unwrap_or(0),
        })
    }

    /// Reconstruct swap from transfers
    fn reconstruct_swap(transfers: &[TokenTransfer], pool: Option<&str>) -> SwapInfo {
        let token_in = transfers
            .iter()
            .find(|t| t.is_outflow())
            .map(|t| t.mint.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let token_out = transfers
            .iter()
            .find(|t| t.is_inflow())
            .map(|t| t.mint.clone())
            .unwrap_or_else(|| "unknown".to_string());

        let amount_in = transfers
            .iter()
            .find(|t| t.is_outflow())
            .map(|t| (t.net_change.abs() * 1_000_000_000.0) as u64)
            .unwrap_or(0);

        let amount_out = transfers
            .iter()
            .find(|t| t.is_inflow())
            .map(|t| (t.net_change * 1_000_000_000.0) as u64)
            .unwrap_or(0);

        SwapInfo {
            pool: pool.unwrap_or("unknown").to_string(),
            token_in,
            token_out,
            amount_in,
            amount_out,
            program_id: pool.unwrap_or("unknown").to_string(),
            direction: None,
        }
    }

    /// Calculate fees collected from JIT liquidity
    fn calculate_fees_collected(
        add_liq: &LiquidityTx,
        remove_liq: &LiquidityTx,
        _victim_swap: &VictimTx,
    ) -> i64 {
        // Fees collected = (removed amount - added amount)
        // In JIT, the removed liquidity includes the swap fees

        let added_value_a = add_liq.amount_a as i64;
        let removed_value_a = remove_liq.amount_a as i64;

        let fees_in_token_a = removed_value_a - added_value_a;

        // Simplified: return fees in token A
        // In practice, you'd convert both tokens to a common denomination
        fees_in_token_a.max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_liquidity_add_detection() {
        // Test liquidity add identification
    }

    #[test]
    fn test_liquidity_remove_detection() {
        // Test liquidity remove identification
    }

    #[test]
    fn test_jit_pattern_detection() {
        // Test full JIT pattern detection
    }
}
