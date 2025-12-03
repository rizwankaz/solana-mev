use crate::types::FetchedTransaction;
use crate::mev::types::{Sandwich, SandwichTx, VictimTx, SwapInfo};
use crate::mev::parser::{TransactionParser, TokenTransfer};
use std::collections::{HashMap, HashSet};

/// Sandwich Attack Detector
///
/// Based on Brontes and Sandwiched.me methodologies:
///
/// Detection algorithm:
/// 1. Identify potential front-run transactions (large swaps)
/// 2. Find interleaved victim transactions
/// 3. Identify back-run transactions from same searcher
/// 4. Validate that front-run and back-run share at least one common pool
/// 5. Group victim transactions by EOA to handle multi-step swaps
/// 6. Calculate profit and victim losses
///
/// Key characteristics:
/// - Front-run: Large swap that moves price unfavorably for victim
/// - Victim: Regular user swap sandwiched between front/back runs
/// - Back-run: Reverse swap by attacker to profit from price movement
/// - Common pool: Front and back must affect same liquidity pool
pub struct SandwichDetector;

impl SandwichDetector {
    /// Detect sandwich attacks in a block
    ///
    /// Analyzes all successful transactions in a block to find sandwich patterns.
    /// Returns a vector of detected sandwich attacks.
    pub fn detect_in_block(
        transactions: &[&FetchedTransaction],
        slot: u64,
    ) -> Vec<Sandwich> {
        let mut sandwiches = Vec::new();

        // Filter to swap transactions only (already filtered by caller using instruction analysis)
        let dex_txs: Vec<_> = transactions
            .iter()
            .filter(|tx| tx.is_success())
            .map(|tx| *tx)
            .collect();

        if dex_txs.len() < 3 {
            // Need at least 3 txs for a sandwich
            return sandwiches;
        }

        // Look for sandwich patterns: frontrun -> victim(s) -> backrun
        for i in 0..dex_txs.len() {
            if let Some(sandwich) = Self::detect_sandwich_at_index(&dex_txs, i, slot) {
                sandwiches.push(sandwich);
            }
        }

        sandwiches
    }

    /// Detect sandwich attack starting at a specific transaction index
    fn detect_sandwich_at_index(
        txs: &[&FetchedTransaction],
        start_idx: usize,
        slot: u64,
    ) -> Option<Sandwich> {
        if start_idx + 2 >= txs.len() {
            return None; // Not enough transactions after this one
        }

        let potential_frontrun = txs[start_idx];

        // Extract frontrun metadata
        let frontrun_signer = TransactionParser::get_signer(potential_frontrun)?;
        let frontrun_transfers = TransactionParser::extract_token_transfers(potential_frontrun);
        let frontrun_pools = Self::extract_pools(potential_frontrun);

        if frontrun_pools.is_empty() {
            return None;
        }

        // Look ahead for backrun from same signer
        // Sandwich attacks typically complete within 5-10 transactions
        let search_window = std::cmp::min(10, txs.len() - start_idx);
        let max_end_idx = std::cmp::min(start_idx + search_window, txs.len() - 1);

        for end_idx in (start_idx + 2)..=max_end_idx {
            let potential_backrun = txs[end_idx];
            let backrun_signer = TransactionParser::get_signer(potential_backrun)?;

            // Check if same signer (same searcher/bot)
            if frontrun_signer != backrun_signer {
                continue;
            }

            // Extract backrun metadata
            let backrun_transfers = TransactionParser::extract_token_transfers(potential_backrun);
            let backrun_pools = Self::extract_pools(potential_backrun);

            // Check for common pools (Brontes criterion)
            let common_pools = Self::find_common_pools(&frontrun_pools, &backrun_pools);
            if common_pools.is_empty() {
                continue;
            }

            // Check if this is a reverse trade (sandwich pattern)
            if !Self::is_reverse_trade(&frontrun_transfers, &backrun_transfers) {
                continue;
            }

            // Found potential sandwich! Extract victims
            let victims = Self::extract_victims(
                &txs[(start_idx + 1)..end_idx],
                &frontrun_signer,
                &common_pools,
                slot,
            );

            if victims.is_empty() {
                continue; // No victims = not a sandwich
            }

            // Calculate profit and losses
            let profit_lamports = Self::calculate_profit(&frontrun_transfers, &backrun_transfers);
            let victim_loss_lamports: i64 = victims.iter().map(|v| v.loss_lamports).sum();

            // Build sandwich transaction objects
            let frontrun_tx = Self::build_sandwich_tx(
                potential_frontrun,
                &frontrun_transfers,
                &frontrun_pools,
            )?;

            let backrun_tx = Self::build_sandwich_tx(
                potential_backrun,
                &backrun_transfers,
                &backrun_pools,
            )?;

            return Some(Sandwich {
                slot,
                attacker: frontrun_signer,
                frontrun: frontrun_tx,
                victims,
                backrun: backrun_tx,
                common_pools,
                profit_lamports,
                profit_usd: None,
                victim_loss_lamports,
                victim_loss_usd: None,
            });
        }

        None
    }

    /// Extract pool addresses from transaction
    fn extract_pools(tx: &FetchedTransaction) -> Vec<String> {
        let accounts = TransactionParser::extract_accounts(tx);

        // In production, you'd maintain a database of known pool addresses
        // For now, we use a simplified heuristic: accounts that are not
        // system programs or token programs are likely pool addresses
        accounts
            .into_iter()
            .filter(|acc| {
                // Filter out known system programs
                !acc.starts_with("11111111111111") && // System Program
                acc != "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" && // Token Program
                acc != "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" // Associated Token Program
            })
            .collect()
    }

    /// Find common pools between two transaction
    fn find_common_pools(pools1: &[String], pools2: &[String]) -> Vec<String> {
        let set1: HashSet<_> = pools1.iter().collect();
        let set2: HashSet<_> = pools2.iter().collect();

        set1.intersection(&set2)
            .map(|s| (*s).clone())
            .collect()
    }

    /// Check if backrun is reverse of frontrun (sandwich pattern)
    fn is_reverse_trade(
        frontrun_transfers: &[TokenTransfer],
        backrun_transfers: &[TokenTransfer],
    ) -> bool {
        // In a sandwich:
        // - Frontrun: Sell token A, buy token B
        // - Backrun: Sell token B, buy token A (reverse)

        if frontrun_transfers.is_empty() || backrun_transfers.is_empty() {
            return false;
        }

        // Get primary tokens involved in frontrun
        let frontrun_out = frontrun_transfers.iter().find(|t| t.is_outflow());
        let frontrun_in = frontrun_transfers.iter().find(|t| t.is_inflow());

        // Get primary tokens in backrun
        let backrun_out = backrun_transfers.iter().find(|t| t.is_outflow());
        let backrun_in = backrun_transfers.iter().find(|t| t.is_inflow());

        // Check if tokens are reversed
        match (frontrun_out, frontrun_in, backrun_out, backrun_in) {
            (Some(f_out), Some(f_in), Some(b_out), Some(b_in)) => {
                // Frontrun sells A buys B, Backrun sells B buys A
                f_out.mint == b_in.mint && f_in.mint == b_out.mint
            }
            _ => false,
        }
    }

    /// Extract victim transactions between frontrun and backrun
    fn extract_victims(
        middle_txs: &[&FetchedTransaction],
        attacker: &str,
        common_pools: &[String],
        _slot: u64,
    ) -> Vec<VictimTx> {
        let mut victims = Vec::new();

        // Group transactions by signer (EOA) - Brontes methodology
        let mut txs_by_signer: HashMap<String, Vec<&FetchedTransaction>> = HashMap::new();

        for tx in middle_txs {
            if let Some(signer) = TransactionParser::get_signer(tx) {
                // Skip if signer is the attacker
                if signer == attacker {
                    continue;
                }

                // Check if transaction interacts with common pools
                let tx_pools = Self::extract_pools(tx);
                let has_common_pool = tx_pools.iter().any(|p| common_pools.contains(p));

                if has_common_pool {
                    txs_by_signer.entry(signer).or_default().push(tx);
                }
            }
        }

        // Build victim objects for each signer's transactions
        for (victim_address, victim_txs) in txs_by_signer {
            for tx in victim_txs {
                let transfers = TransactionParser::extract_token_transfers(tx);

                // Estimate loss (simplified - would need price impact calculation)
                let loss_lamports = Self::estimate_victim_loss(&transfers);

                let swap = Self::reconstruct_swap(&transfers, common_pools.first());

                victims.push(VictimTx {
                    signature: tx.signature.clone(),
                    tx_index: tx.index,
                    victim_address: victim_address.clone(),
                    swap,
                    loss_lamports,
                    loss_usd: None,
                });
            }
        }

        victims
    }

    /// Build SandwichTx from transaction
    fn build_sandwich_tx(
        tx: &FetchedTransaction,
        transfers: &[TokenTransfer],
        pools: &[String],
    ) -> Option<SandwichTx> {
        let swap = Self::reconstruct_swap(transfers, pools.first());

        Some(SandwichTx {
            signature: tx.signature.clone(),
            tx_index: tx.index,
            swap,
            compute_units: tx.compute_units_consumed().unwrap_or(0),
            fee_lamports: tx.fee().unwrap_or(0),
        })
    }

    /// Reconstruct swap info from transfers
    fn reconstruct_swap(transfers: &[TokenTransfer], pool: Option<&String>) -> SwapInfo {
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
            pool: pool.cloned().unwrap_or_else(|| "unknown".to_string()),
            token_in,
            token_out,
            amount_in,
            amount_out,
            program_id: pool.cloned().unwrap_or_else(|| "unknown".to_string()),
            direction: None,
        }
    }

    /// Calculate sandwich profit
    fn calculate_profit(
        frontrun_transfers: &[TokenTransfer],
        backrun_transfers: &[TokenTransfer],
    ) -> i64 {
        // In a sandwich, profit is the difference between what was bought in frontrun
        // and what was sold in backrun

        // Get the primary token (usually SOL or stablecoin)
        let frontrun_out = frontrun_transfers
            .iter()
            .find(|t| t.is_outflow())
            .map(|t| t.net_change.abs())
            .unwrap_or(0.0);

        let backrun_in = backrun_transfers
            .iter()
            .find(|t| t.is_inflow())
            .map(|t| t.net_change)
            .unwrap_or(0.0);

        // Profit = received in backrun - paid in frontrun
        let profit = backrun_in - frontrun_out;

        // Convert to lamports (simplified)
        (profit * 1_000_000_000.0) as i64
    }

    /// Estimate victim loss from price impact
    fn estimate_victim_loss(transfers: &[TokenTransfer]) -> i64 {
        // In a sandwich, victim loses value due to unfavorable execution price
        // Simplified estimation: assume 0.5-2% loss on swap value

        // Get swap value (amount of tokens traded)
        let swap_value = transfers
            .iter()
            .find(|t| t.is_outflow())
            .map(|t| t.net_change.abs())
            .unwrap_or(0.0);

        // Assume 1% average loss (in practice, calculate from price impact)
        let loss = swap_value * 0.01;

        (loss * 1_000_000_000.0) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reverse_trade_detection() {
        // Test that reverse trades are properly identified
    }

    #[test]
    fn test_common_pool_detection() {
        // Test common pool finding
    }

    #[test]
    fn test_victim_extraction() {
        // Test victim transaction extraction
    }
}
