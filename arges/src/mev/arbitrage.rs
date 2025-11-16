//! Arbitrage detection
//!
//! Detects cross-DEX arbitrage opportunities and executions

use super::types::*;
use crate::dex::ParsedSwap;
use crate::types::FetchedBlock;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use tracing::warn;

/// Wrapped SOL (WSOL) token address
pub const WSOL_ADDRESS: &str = "So11111111111111111111111111111111111111112";

/// Arbitrage detector
pub struct ArbitrageDetector {
    /// Minimum profit threshold in lamports
    min_profit_lamports: i64,

    /// Maximum number of hops to consider for arbitrage
    max_hops: usize,
}

impl ArbitrageDetector {
    /// Create new arbitrage detector
    pub fn new(min_profit_lamports: i64, max_hops: usize) -> Self {
        Self {
            min_profit_lamports,
            max_hops,
        }
    }

    /// Detect arbitrage in a block
    pub fn detect(&self, block: &FetchedBlock, swaps: &[ParsedSwap]) -> Result<Vec<MevEvent>> {
        let mut arb_events = Vec::new();

        // Group swaps by user
        let swaps_by_user = self.group_swaps_by_user(swaps);

        // Analyze each user's swaps for arbitrage patterns
        for (user, user_swaps) in swaps_by_user.iter() {
            // Look for circular trades (e.g., SOL -> USDC -> SOL)
            if let Some(arb) = self.detect_circular_arbitrage(user, user_swaps, block)? {
                arb_events.push(arb);
            }

            // Look for cross-DEX arbitrage
            if let Some(arb) = self.detect_cross_dex_arbitrage(user, user_swaps, block)? {
                arb_events.push(arb);
            }
        }

        // Also look for atomic arbitrage (multiple swaps in single transaction)
        arb_events.extend(self.detect_atomic_arbitrage(swaps, block)?);

        Ok(arb_events)
    }

    /// Group swaps by user address
    fn group_swaps_by_user<'a>(
        &self,
        swaps: &'a [ParsedSwap],
    ) -> HashMap<String, Vec<&'a ParsedSwap>> {
        let mut by_user: HashMap<String, Vec<&ParsedSwap>> = HashMap::new();

        for swap in swaps {
            by_user.entry(swap.user.clone()).or_default().push(swap);
        }

        by_user
    }

    /// Detect circular arbitrage (e.g., A -> B -> C -> A with profit)
    fn detect_circular_arbitrage(
        &self,
        user: &str,
        swaps: &[&ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        if swaps.len() < 2 {
            return Ok(None);
        }

        // Build token flow graph
        let mut token_flow: HashMap<String, Vec<&ParsedSwap>> = HashMap::new();
        for swap in swaps {
            token_flow
                .entry(swap.token_in.clone())
                .or_default()
                .push(swap);
        }

        // Try to find cycles
        for start_token in token_flow.keys() {
            if let Some(cycle) = self.find_cycle(start_token, &token_flow, self.max_hops) {
                // Calculate profit
                let (_profit, net_profit) = self.calculate_arbitrage_profit(&cycle);

                if net_profit >= self.min_profit_lamports {
                    // Found profitable arbitrage!
                    let metadata = ArbitrageMetadata {
                        dexs: cycle.iter().map(|s| s.dex.name().to_string()).collect(),
                        token_path: self.extract_token_path_from_owned(&cycle),
                        swaps: cycle
                            .iter()
                            .map(|s| self.swap_to_details(s))
                            .collect(),
                        input_amount: cycle.first().map(|s| s.amount_in).unwrap_or(0),
                        output_amount: cycle.last().map(|s| s.amount_out).unwrap_or(0),
                        net_profit,
                        hop_count: cycle.len(),
                    };

                    return Ok(Some(MevEvent {
                        mev_type: MevType::Arbitrage,
                        slot: block.slot,
                        timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
                        transactions: cycle.iter().map(|s| s.signature.clone()).collect(),
                        profit_lamports: Some(net_profit),
                        profit_usd: None, // Would need price oracle
                        tokens: self.extract_unique_tokens_from_owned(&cycle),
                        metadata: MevMetadata::Arbitrage(metadata),
                        extractor: Some(user.to_string()),
                        confidence: self.calculate_confidence(&cycle),
                    }));
                }
            }
        }

        Ok(None)
    }

    /// Find a cycle in the token flow graph using DFS
    fn find_cycle(
        &self,
        start_token: &str,
        graph: &HashMap<String, Vec<&ParsedSwap>>,
        max_depth: usize,
    ) -> Option<Vec<ParsedSwap>> {
        let mut path = Vec::new();
        let mut visited = HashSet::new();

        self.dfs_cycle(
            start_token,
            start_token,
            graph,
            &mut path,
            &mut visited,
            max_depth,
        )
    }

    /// DFS to find cycle
    fn dfs_cycle(
        &self,
        current: &str,
        target: &str,
        graph: &HashMap<String, Vec<&ParsedSwap>>,
        path: &mut Vec<ParsedSwap>,
        visited: &mut HashSet<String>,
        max_depth: usize,
    ) -> Option<Vec<ParsedSwap>> {
        if path.len() >= max_depth {
            return None;
        }

        if path.len() > 0 && current == target {
            // Found a cycle!
            return Some(path.clone());
        }

        if visited.contains(current) {
            return None;
        }

        visited.insert(current.to_string());

        if let Some(swaps) = graph.get(current) {
            for swap in swaps {
                path.push((*swap).clone());
                if let Some(cycle) =
                    self.dfs_cycle(&swap.token_out, target, graph, path, visited, max_depth)
                {
                    return Some(cycle);
                }
                path.pop();
            }
        }

        visited.remove(current);
        None
    }

    /// Detect cross-DEX arbitrage (same token pair on different DEXs)
    fn detect_cross_dex_arbitrage(
        &self,
        user: &str,
        swaps: &[&ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Look for swaps of the same token pair on different DEXs
        for i in 0..swaps.len() {
            for j in i + 1..swaps.len() {
                let swap1 = swaps[i];
                let swap2 = swaps[j];

                // Check if they're opposite swaps on different DEXs
                if swap1.dex != swap2.dex
                    && swap1.token_in == swap2.token_out
                    && swap1.token_out == swap2.token_in
                {
                    // This could be cross-DEX arbitrage
                    // Validate we're comparing the same token (should be guaranteed by above check)
                    if swap1.token_in != swap2.token_out {
                        continue;
                    }

                    // Only calculate SOL profit for WSOL pairs
                    let profit = if swap1.token_in == WSOL_ADDRESS {
                        (swap2.amount_out as i64) - (swap1.amount_in as i64)
                    } else {
                        // For non-WSOL tokens, we can't calculate lamport profit without price data
                        continue;
                    };

                    if profit >= self.min_profit_lamports {
                        let metadata = ArbitrageMetadata {
                            dexs: vec![swap1.dex.name().to_string(), swap2.dex.name().to_string()],
                            token_path: vec![
                                swap1.token_in.clone(),
                                swap1.token_out.clone(),
                                swap2.token_out.clone(),
                            ],
                            swaps: vec![self.swap_to_details(swap1), self.swap_to_details(swap2)],
                            input_amount: swap1.amount_in,
                            output_amount: swap2.amount_out,
                            net_profit: profit,
                            hop_count: 2,
                        };

                        return Ok(Some(MevEvent {
                            mev_type: MevType::Arbitrage,
                            slot: block.slot,
                            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
                            transactions: vec![swap1.signature.clone(), swap2.signature.clone()],
                            profit_lamports: Some(profit),
                            profit_usd: None,
                            tokens: vec![swap1.token_in.clone(), swap1.token_out.clone()],
                            metadata: MevMetadata::Arbitrage(metadata),
                            extractor: Some(user.to_string()),
                            confidence: 0.85,
                        }));
                    }
                }
            }
        }

        Ok(None)
    }

    /// Detect atomic arbitrage (multiple swaps in a single transaction)
    fn detect_atomic_arbitrage(
        &self,
        swaps: &[ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        // Group swaps by transaction
        let mut by_tx: HashMap<String, Vec<&ParsedSwap>> = HashMap::new();
        for swap in swaps {
            by_tx.entry(swap.signature.clone()).or_default().push(swap);
        }

        // Look for transactions with multiple swaps forming arbitrage
        for (tx_sig, tx_swaps) in by_tx.iter() {
            if tx_swaps.len() < 2 {
                continue;
            }

            // Check if swaps form a cycle
            let first_token = &tx_swaps[0].token_in;
            let last_token = &tx_swaps[tx_swaps.len() - 1].token_out;

            if first_token == last_token {
                // Only track WSOL arbitrage for accurate SOL profit calculation
                // Other tokens would need price oracle data
                if first_token != WSOL_ADDRESS {
                    warn!(
                        "Skipping non-WSOL arbitrage cycle for token {} - need price oracle for accurate profit",
                        first_token
                    );
                    continue;
                }

                // Calculate profit - amounts are in lamports for WSOL
                let input = tx_swaps[0].amount_in;
                let output = tx_swaps[tx_swaps.len() - 1].amount_out;
                let profit = (output as i64) - (input as i64);

                if profit >= self.min_profit_lamports {
                    let user = &tx_swaps[0].user;

                    let metadata = ArbitrageMetadata {
                        dexs: tx_swaps.iter().map(|s| s.dex.name().to_string()).collect(),
                        token_path: self.extract_token_path(tx_swaps),
                        swaps: tx_swaps.iter().map(|s| self.swap_to_details(s)).collect(),
                        input_amount: input,
                        output_amount: output,
                        net_profit: profit,
                        hop_count: tx_swaps.len(),
                    };

                    events.push(MevEvent {
                        mev_type: MevType::Arbitrage,
                        slot: block.slot,
                        timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
                        transactions: vec![tx_sig.clone()],
                        profit_lamports: Some(profit),
                        profit_usd: None,
                        tokens: self.extract_unique_tokens(tx_swaps),
                        metadata: MevMetadata::Arbitrage(metadata),
                        extractor: Some(user.clone()),
                        confidence: 0.95, // High confidence for atomic arbitrage
                    });
                }
            }
        }

        Ok(events)
    }

    /// Calculate arbitrage profit from a cycle of swaps
    fn calculate_arbitrage_profit(&self, cycle: &[ParsedSwap]) -> (i64, i64) {
        if cycle.is_empty() {
            return (0, 0);
        }

        let first_token = &cycle[0].token_in;
        let last_token = &cycle[cycle.len() - 1].token_out;

        // CRITICAL: Only calculate profit if the cycle truly starts and ends with same token
        if first_token != last_token {
            warn!(
                "Arbitrage cycle token mismatch: start={}, end={}. Cannot calculate profit.",
                first_token, last_token
            );
            return (0, 0);
        }

        let input = cycle[0].amount_in as i64;
        let output = cycle[cycle.len() - 1].amount_out as i64;

        // Only calculate profit for WSOL cycles (where amounts are in lamports)
        // For other tokens, we'd need decimal information and price data
        if first_token == WSOL_ADDRESS {
            let gross_profit = output - input;
            let estimated_fees: i64 = cycle.len() as i64 * 5000; // 5000 lamports per tx
            let net_profit = gross_profit - estimated_fees;
            (gross_profit, net_profit)
        } else {
            // For non-WSOL tokens, we can't reliably calculate lamport-denominated profit
            // without knowing token decimals and prices
            warn!(
                "Cannot calculate SOL profit for non-WSOL arbitrage cycle (token: {}). Need price oracle.",
                first_token
            );
            (0, 0)
        }
    }

    /// Extract token path from swaps
    fn extract_token_path(&self, swaps: &[&ParsedSwap]) -> Vec<String> {
        let mut path = Vec::new();

        if swaps.is_empty() {
            return path;
        }

        path.push(swaps[0].token_in.clone());

        for swap in swaps {
            path.push(swap.token_out.clone());
        }

        path
    }

    /// Extract token path from owned swaps
    fn extract_token_path_from_owned(&self, swaps: &[ParsedSwap]) -> Vec<String> {
        let mut path = Vec::new();

        if swaps.is_empty() {
            return path;
        }

        path.push(swaps[0].token_in.clone());

        for swap in swaps {
            path.push(swap.token_out.clone());
        }

        path
    }

    /// Extract unique tokens from swaps
    fn extract_unique_tokens(&self, swaps: &[&ParsedSwap]) -> Vec<String> {
        let mut tokens = HashSet::new();

        for swap in swaps {
            tokens.insert(swap.token_in.clone());
            tokens.insert(swap.token_out.clone());
        }

        tokens.into_iter().collect()
    }

    /// Extract unique tokens from owned swaps
    fn extract_unique_tokens_from_owned(&self, swaps: &[ParsedSwap]) -> Vec<String> {
        let mut tokens = HashSet::new();

        for swap in swaps {
            tokens.insert(swap.token_in.clone());
            tokens.insert(swap.token_out.clone());
        }

        tokens.into_iter().collect()
    }

    /// Convert ParsedSwap to SwapDetails
    fn swap_to_details(&self, swap: &ParsedSwap) -> SwapDetails {
        SwapDetails {
            dex: swap.dex.name().to_string(),
            pool: swap.pool.clone(),
            token_in: swap.token_in.clone(),
            token_out: swap.token_out.clone(),
            amount_in: swap.amount_in,
            amount_out: swap.amount_out,
            price_impact: swap.price_impact,
            min_amount_out: swap.min_amount_out,
            signature: swap.signature.clone(),
            tx_index: swap.tx_index,
        }
    }

    /// Calculate confidence score for arbitrage detection
    fn calculate_confidence(&self, cycle: &[ParsedSwap]) -> f64 {
        // Base confidence
        let mut confidence: f64 = 0.7;

        // Higher confidence for atomic arbitrage (single tx)
        let unique_txs: HashSet<_> = cycle.iter().map(|s| &s.signature).collect();
        if unique_txs.len() == 1 {
            confidence += 0.2;
        }

        // Higher confidence for more DEXs involved
        let unique_dexs: HashSet<_> = cycle.iter().map(|s| s.dex).collect();
        if unique_dexs.len() >= 2 {
            confidence += 0.1;
        }

        confidence.min(1.0)
    }
}

impl Default for ArbitrageDetector {
    fn default() -> Self {
        Self::new(1_000_000, 5) // 0.001 SOL minimum profit, max 5 hops
    }
}
