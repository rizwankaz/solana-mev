use crate::types::FetchedTransaction;
use crate::mev::types::{AtomicArbitrage, SwapInfo};
use crate::mev::parser::{TransactionParser, DexPrograms};
use std::collections::HashSet;

/// Atomic Arbitrage Detector
///
/// Based on Brontes methodology:
/// - Detects single transactions with multiple swaps across different pools
/// - Identifies circular trading routes (e.g., SOL → USDC → RAY → SOL)
/// - Calculates net profit by comparing input and output of same token
/// - Accounts for gas costs and fees
///
/// Key detection criteria:
/// 1. Transaction must have 2+ swaps
/// 2. Swaps must involve at least 2 different pools
/// 3. Must have a circular token route (start and end with same token)
/// 4. Net profit after fees must be positive
pub struct AtomicArbDetector;

impl AtomicArbDetector {
    /// Detect atomic arbitrage in a single transaction
    ///
    /// Returns Some(AtomicArbitrage) if the transaction is an atomic arb, None otherwise
    pub fn detect(tx: &FetchedTransaction, slot: u64) -> Option<AtomicArbitrage> {
        // Criterion 1: Transaction must be successful
        if !tx.is_success() {
            return None;
        }

        // Criterion 2: Must have token transfers (indicating swaps occurred)
        let transfers = TransactionParser::extract_token_transfers(tx);
        if transfers.len() < 2 {
            return None; // Need at least 2 tokens involved
        }

        // Criterion 3: Must interact with DEX programs
        if !TransactionParser::is_dex_swap(tx) {
            return None;
        }

        // Criterion 4: Check for circular arbitrage by analyzing token transfers
        // In atomic arb, the searcher starts and ends with the same token (usually SOL or a stablecoin)
        let arb_analysis = Self::analyze_arbitrage(&transfers, tx)?;

        // Criterion 5: Must have net positive profit after fees
        if arb_analysis.net_profit <= 0 {
            return None;
        }

        // Extract metadata
        let searcher = TransactionParser::get_signer(tx)?;
        let compute_units = tx.compute_units_consumed().unwrap_or(0);
        let fee_lamports = tx.fee().unwrap_or(0);

        Some(AtomicArbitrage {
            signature: tx.signature.clone(),
            slot,
            tx_index: tx.index,
            searcher,
            swaps: arb_analysis.swaps,
            profit_lamports: arb_analysis.net_profit,
            profit_usd: None, // Would require price oracle
            compute_units,
            fee_lamports,
            pools: arb_analysis.pools,
            token_route: arb_analysis.token_route,
        })
    }

    /// Analyze token transfers to detect arbitrage pattern
    fn analyze_arbitrage(
        transfers: &[crate::mev::parser::TokenTransfer],
        tx: &FetchedTransaction,
    ) -> Option<ArbitrageAnalysis> {
        // Group transfers by token mint to find net changes
        let mut token_net_changes = std::collections::HashMap::new();

        for transfer in transfers {
            *token_net_changes.entry(transfer.mint.clone())
                .or_insert(0.0) += transfer.net_change;
        }

        // Find the primary profit token (usually the one with positive net change)
        let mut profit_token = None;
        let mut max_profit = 0.0;

        for (token, net_change) in &token_net_changes {
            if *net_change > max_profit {
                max_profit = *net_change;
                profit_token = Some(token.clone());
            }
        }

        // Check if we have a profit token with positive balance
        if max_profit <= 0.0 {
            return None;
        }

        // Build token route from transfers
        let token_route = Self::build_token_route(transfers);

        // Must have at least 3 tokens in route for arbitrage (A -> B -> C -> A)
        if token_route.len() < 3 {
            return None;
        }

        // Check if route is circular (starts and ends with same token)
        let is_circular = token_route.first() == token_route.last();
        if !is_circular {
            return None;
        }

        // Extract pools involved (from accounts in transaction)
        let accounts = TransactionParser::extract_accounts(tx);
        let pools: Vec<String> = accounts
            .iter()
            .filter(|acc| {
                // In production, you'd check if account is a known pool address
                // For now, we'll include DEX program addresses
                DexPrograms::is_dex_program(acc)
            })
            .cloned()
            .collect();

        // Must involve at least 2 different pools
        let unique_pools: HashSet<_> = pools.iter().collect();
        if unique_pools.len() < 2 {
            return None;
        }

        // Estimate profit in lamports
        // For SOL transfers, we can directly use the net change
        // For other tokens, we'd need price conversion
        let net_profit = Self::estimate_profit_lamports(
            profit_token.as_ref()?,
            max_profit,
            transfers,
        );

        // Subtract transaction fee
        let fee_lamports = tx.fee().unwrap_or(0);
        let net_profit = net_profit - fee_lamports as i64;

        // Build swap info (simplified - would need more detailed parsing)
        let swaps = Self::reconstruct_swaps(transfers, &pools);

        Some(ArbitrageAnalysis {
            swaps,
            net_profit,
            pools,
            token_route,
        })
    }

    /// Build token route from transfers
    fn build_token_route(transfers: &[crate::mev::parser::TokenTransfer]) -> Vec<String> {
        // Sort transfers by the magnitude of change to determine order
        let mut sorted_transfers = transfers.to_vec();
        sorted_transfers.sort_by(|a, b| {
            a.account_index.cmp(&b.account_index)
        });

        // Extract unique tokens in order
        let mut route = Vec::new();
        let mut seen = HashSet::new();

        for transfer in sorted_transfers {
            if seen.insert(transfer.mint.clone()) {
                route.push(transfer.mint.clone());
            }
        }

        // Add the first token again at the end if circular
        if let (Some(first), Some(last)) = (route.first(), route.last()) {
            if first != last {
                // Check if any transfer has same token as first with opposite sign
                for transfer in transfers {
                    if &transfer.mint == first && transfer.net_change > 0.0 {
                        route.push(first.clone());
                        break;
                    }
                }
            }
        }

        route
    }

    /// Estimate profit in lamports
    fn estimate_profit_lamports(
        profit_token: &str,
        ui_amount: f64,
        transfers: &[crate::mev::parser::TokenTransfer],
    ) -> i64 {
        // Native SOL token (represented by System Program or wrapped SOL)
        const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
        const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

        if profit_token == WSOL_MINT || profit_token.contains("11111111111111") {
            // This is SOL, convert directly
            (ui_amount * LAMPORTS_PER_SOL) as i64
        } else {
            // For other tokens, we'd need price data
            // For now, return 0 if we can't price it
            // In production, you'd integrate with price oracles or DEX pricing
            0
        }
    }

    /// Reconstruct swap information from transfers
    fn reconstruct_swaps(
        transfers: &[crate::mev::parser::TokenTransfer],
        pools: &[String],
    ) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        // Group transfers into pairs (outflow + inflow = swap)
        let outflows: Vec<_> = transfers.iter().filter(|t| t.is_outflow()).collect();
        let inflows: Vec<_> = transfers.iter().filter(|t| t.is_inflow()).collect();

        // Match outflows with inflows to reconstruct swaps
        for (i, outflow) in outflows.iter().enumerate() {
            if let Some(inflow) = inflows.get(i) {
                let pool = pools.get(i).cloned().unwrap_or_else(|| "unknown".to_string());

                swaps.push(SwapInfo {
                    pool: pool.clone(),
                    token_in: outflow.mint.clone(),
                    token_out: inflow.mint.clone(),
                    amount_in: (outflow.net_change.abs() * 1_000_000_000.0) as u64, // Estimate
                    amount_out: (inflow.net_change * 1_000_000_000.0) as u64, // Estimate
                    program_id: pool,
                    direction: None,
                });
            }
        }

        swaps
    }

    /// Batch detect atomic arbitrages in multiple transactions
    pub fn detect_batch(
        transactions: &[FetchedTransaction],
        slot: u64,
    ) -> Vec<AtomicArbitrage> {
        transactions
            .iter()
            .filter_map(|tx| Self::detect(tx, slot))
            .collect()
    }
}

/// Internal structure for arbitrage analysis
struct ArbitrageAnalysis {
    swaps: Vec<SwapInfo>,
    net_profit: i64,
    pools: Vec<String>,
    token_route: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_requires_success() {
        // Test that detector only processes successful transactions
        // Would need to create mock transactions for full testing
    }

    #[test]
    fn test_circular_route_detection() {
        // Test that circular routes are properly identified
    }

    #[test]
    fn test_profit_calculation() {
        // Test profit calculation with fees
    }
}
