use crate::types::{
    ArbitrageEvent, FetchedTransaction, MevEvent, Profitability, SandwichEvent,
    SandwichTransaction, SimpleTokenChange, TokenChange,
};
use crate::parsers::SwapParser;
use crate::oracle::OracleClient;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// MEV detector for analyzing blocks
pub struct MevDetector {
    /// Minimum swap count for arbitrage
    pub min_swap_count: usize,
    /// Maximum block distance for sandwich detection
    pub max_sandwich_distance: usize,
    /// Swap parser for extracting swap information
    swap_parser: Arc<SwapParser>,
    /// Oracle client for price data
    oracle: OracleClient,
}

impl MevDetector {
    pub fn new(slot: u64, timestamp: i64, rpc_url: String) -> Self {
        Self {
            min_swap_count: 2,
            max_sandwich_distance: 5,
            swap_parser: Arc::new(SwapParser::new()),
            oracle: OracleClient::new(slot, timestamp, rpc_url),
        }
    }

    /// Detect all MEV events in a block (optimized for performance)
    pub async fn detect_mev(
        &mut self,
        slot: u64,
        transactions: &[FetchedTransaction],
    ) -> Vec<MevEvent> {
        // Early filter: only successful transactions with potential MEV activity
        let candidates: Vec<_> = transactions
            .par_iter()
            .filter(|tx| tx.is_success() && Self::has_potential_mev(tx))
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        // Pre-extract swap data and token changes for all candidates (parallel)
        let swap_parser = self.swap_parser.clone();
        let extracted_data: Vec<_> = candidates
            .par_iter()
            .map(|&tx| {
                let swaps = swap_parser.extract_swaps(tx);
                let token_changes = swap_parser.extract_token_changes(tx);
                let program_addresses = swap_parser.extract_dex_programs(tx);
                (tx, swaps, token_changes, program_addresses)
            })
            .collect();

        // Collect all unique mints that need prices
        let mut unique_mints = HashSet::new();
        unique_mints.insert("So11111111111111111111111111111111111111112"); // SOL for fees

        for (_tx, _swaps, token_changes, _progs) in &extracted_data {
            for change in token_changes {
                if change.delta > 0 {
                    unique_mints.insert(change.mint.as_str());
                }
            }
        }

        // Batch fetch all prices concurrently
        let mints_vec: Vec<&str> = unique_mints.into_iter().collect();
        let price_map: HashMap<String, f64> = self.oracle
            .batch_get_prices(&mints_vec)
            .await
            .into_iter()
            .collect();

        let mut events = Vec::with_capacity(candidates.len());

        // Detect arbitrage (parallel with pre-fetched prices)
        let arbitrages: Vec<_> = extracted_data
            .par_iter()
            .filter_map(|(tx, swaps, token_changes, program_addresses)| {
                Self::detect_arbitrage_with_prices(
                    tx,
                    swaps,
                    token_changes,
                    program_addresses,
                    &price_map,
                    self.min_swap_count,
                )
            })
            .collect();

        for arb in arbitrages {
            events.push(MevEvent::Arbitrage(arb));
        }

        // Detect sandwich attacks (sequential, but optimized)
        for sandwich in self.detect_sandwiches_optimized(slot, &extracted_data, &price_map).await {
            events.push(MevEvent::Sandwich(sandwich));
        }

        events
    }

    /// Fast check if transaction has potential MEV activity
    #[inline]
    fn has_potential_mev(tx: &FetchedTransaction) -> bool {
        use solana_transaction_status::option_serializer::OptionSerializer;

        if let Some(meta) = &tx.meta {
            if let OptionSerializer::Some(logs) = &meta.log_messages {
                // Check for swap or transfer patterns
                return logs.iter().any(|msg| {
                    msg.contains("Instruction: Swap") ||
                    msg.contains("Instruction: Transfer") ||
                    msg.contains("Program log: Instruction: Swap")
                });
            }
        }
        false
    }

    /// Detect arbitrage with pre-fetched prices (parallelizable)
    fn detect_arbitrage_with_prices(
        tx: &FetchedTransaction,
        swaps: &[crate::types::SwapInfo],
        token_changes: &[TokenChange],
        program_addresses: &[String],
        price_map: &HashMap<String, f64>,
        min_swap_count: usize,
    ) -> Option<ArbitrageEvent> {
        let signer = tx.signer()?;

        // Must have multiple swaps for arbitrage
        if swaps.len() < min_swap_count {
            return None;
        }

        // Filter out directional trades: check if this forms a cycle
        // Arbitrage should start and end with the same token (completing a cycle)
        // Directional trades have large net position changes without cycling back
        let unique_tokens: std::collections::HashSet<&str> = swaps.iter()
            .flat_map(|s| [s.token0.as_str(), s.token1.as_str()])
            .collect();

        // Check for profit (any token with positive delta owned by signer)
        let signer_changes: Vec<_> = token_changes.iter()
            .filter(|tc| tc.owner == signer)
            .collect();

        let has_profit = signer_changes.iter().any(|tc| tc.delta > 0);
        if !has_profit {
            return None;
        }

        // Deduplicate token changes by mint (sum deltas for same mint)
        let mut changes_by_mint: HashMap<String, (i64, u8)> = HashMap::new();
        for change in &signer_changes {
            let entry = changes_by_mint.entry(change.mint.clone()).or_insert((0, change.decimals));
            entry.0 += change.delta;
        }

        // Convert to SimpleTokenChange format for output
        let token_changes_output: Vec<SimpleTokenChange> = changes_by_mint.iter()
            .map(|(mint, &(delta, decimals))| SimpleTokenChange {
                mint: mint.clone(),
                delta,
                decimals,
            })
            .collect();

        // Calculate profitability from swaps (more accurate than token_changes)
        // Build net position from swap flows: token0 is sold (-), token1 is bought (+)
        let mut net_position: HashMap<String, (f64, u8)> = HashMap::new();

        for swap in swaps {
            // Token0 is sold (negative position)
            let entry0 = net_position.entry(swap.token0.clone()).or_insert((0.0, swap.decimals0));
            entry0.0 -= swap.amount0;

            // Token1 is bought (positive position)
            let entry1 = net_position.entry(swap.token1.clone()).or_insert((0.0, swap.decimals1));
            entry1.0 += swap.amount1;
        }

        // Calculate profit from net positions
        let mut revenue_usd = 0.0;
        let mut cost_usd = 0.0;
        let mut unsupported_profit_tokens = Vec::new();

        for (mint, (amount, _decimals)) in &net_position {
            let price = price_map.get(mint).copied().unwrap_or(0.0);
            let value_usd = amount.abs() * price;

            // Only flag unsupported tokens if they have significant position (>1 token)
            // Ignore near-zero positions from rounding in cycled tokens
            let is_significant = amount.abs() > 1.0;

            if *amount > 0.0 {
                // Net positive = revenue
                if price == 0.0 && is_significant {
                    unsupported_profit_tokens.push(mint.clone());
                }
                revenue_usd += value_usd;
            } else if *amount < 0.0 {
                // Net negative = cost
                if price == 0.0 && is_significant {
                    unsupported_profit_tokens.push(mint.clone());
                }
                cost_usd += value_usd;
            }
        }

        // Gross profit = revenue - cost (before fees)
        let profit_usd = revenue_usd - cost_usd;

        // Filter out directional trades by checking if swaps form a cycle
        // Arbitrage: cycles back to starting token, so one token has ~zero net change
        // Directional trade: sell A to buy B, resulting in -A and +B positions
        if unique_tokens.len() == 2 && swaps.len() >= 2 {
            // Check if we have opposite-signed positions (one positive, one negative)
            // This indicates a directional trade: sold one thing to buy another
            let positions: Vec<_> = net_position.iter().collect();
            if positions.len() == 2 {
                let (_mint1, (amount1, _)) = positions[0];
                let (_mint2, (amount2, _)) = positions[1];

                // If positions have opposite signs, it's a directional trade
                // (sold one token to buy another, not cycling back)
                let opposite_signs = (amount1 > &0.0 && amount2 < &0.0) || (amount1 < &0.0 && amount2 > &0.0);

                if opposite_signs {
                    return None;
                }
            }
        }

        let fee = tx.fee().unwrap_or(0);
        let compute_units = tx.compute_units_consumed().unwrap_or(0);
        let priority_fee = fee.saturating_sub(5000);
        let jito_tip = tx.jito_tip().unwrap_or(0);

        let sol_price = price_map.get("So11111111111111111111111111111111111111112").copied().unwrap_or(131.0);
        let fees_usd = (fee + jito_tip) as f64 / 1_000_000_000.0 * sol_price;
        let net_profit_usd = profit_usd - fees_usd;

        Some(ArbitrageEvent {
            signature: tx.signature.clone(),
            signer,
            success: tx.is_success(),
            compute_units_consumed: compute_units,
            fee,
            priority_fee,
            jito_tip,
            swaps: swaps.to_vec(),
            program_addresses: program_addresses.to_vec(),
            token_changes: token_changes_output,
            profitability: Profitability {
                profit_usd,
                fees_usd,
                net_profit_usd,
                unsupported_profit_tokens,
            },
        })
    }

    /// Detect sandwich attacks (optimized version)
    async fn detect_sandwiches_optimized(
        &self,
        slot: u64,
        extracted_data: &[(
            &FetchedTransaction,
            Vec<crate::types::SwapInfo>,
            Vec<TokenChange>,
            Vec<String>,
        )],
        price_map: &HashMap<String, f64>,
    ) -> Vec<SandwichEvent> {
        let mut sandwiches = Vec::new();

        if extracted_data.len() < 3 {
            return sandwiches;
        }

        // Sort by index
        let mut sorted: Vec<_> = extracted_data.iter().collect();
        sorted.sort_by_key(|(tx, _, _, _)| tx.index);

        // Look for sandwich pattern: front_run, victim(s), back_run
        // Search all combinations within max_sandwich_distance
        for i in 0..sorted.len() {
            let (tx1, swaps1, changes1, progs1) = sorted[i];

            let signer1 = match tx1.signer() {
                Some(s) => s,
                None => continue,
            };

            // Look for potential back_runs within max_sandwich_distance
            for j in (i + 2)..sorted.len().min(i + self.max_sandwich_distance + 1) {
                let (tx3, swaps3, changes3, progs3) = sorted[j];

                let signer3 = match tx3.signer() {
                    Some(s) => s,
                    None => continue,
                };

                // Check if front_run and back_run have same signer
                if signer1 != signer3 {
                    continue;
                }

                // Check if there's at least one transaction with different signer in between (victim)
                let has_victim = (i + 1..j).any(|k| {
                    sorted[k].0.signer()
                        .map(|s| s != signer1)
                        .unwrap_or(false)
                });

                if !has_victim {
                    continue;
                }

                // Get all program addresses from transactions in between (for victim analysis)
                let mut victim_progs: Vec<String> = Vec::new();
                for k in (i + 1)..j {
                    victim_progs.extend(sorted[k].3.iter().cloned());
                }
                // Identify sandwiched token (token that's not SOL appearing in swaps)
                let mut tokens: std::collections::HashSet<String> = std::collections::HashSet::new();
                const SOL: &str = "So11111111111111111111111111111111111111112";

                for swap in swaps1.iter().chain(swaps3.iter()) {
                    if swap.token0 != SOL {
                        tokens.insert(swap.token0.clone());
                    }
                    if swap.token1 != SOL {
                        tokens.insert(swap.token1.clone());
                    }
                }

                // Sandwiched token is the most common non-SOL token
                let sandwiched_token = tokens.into_iter().next().unwrap_or_else(|| "Unknown".to_string());

                // Combine token changes
                let mut combined_changes: HashMap<String, (i64, u8)> = HashMap::new();
                for change in changes1.iter().chain(changes3.iter()) {
                    if change.owner == signer1 {
                        let entry = combined_changes.entry(change.mint.clone()).or_insert((0, change.decimals));
                        entry.0 += change.delta;
                    }
                }

                let token_changes: Vec<SimpleTokenChange> = combined_changes.iter()
                    .map(|(mint, (delta, decimals))| SimpleTokenChange {
                        mint: mint.clone(),
                        delta: *delta,
                        decimals: *decimals,
                    })
                    .collect();

                // Combine DEX programs
                let mut program_addresses = Vec::new();
                program_addresses.extend_from_slice(progs1);
                program_addresses.extend(victim_progs);
                program_addresses.extend_from_slice(progs3);
                program_addresses.sort_unstable();
                program_addresses.dedup();

                // Calculate profitability from swaps (front_run + back_run only)
                // Build net position from swap flows: token0 is sold (-), token1 is bought (+)
                let mut net_position: HashMap<String, (f64, u8)> = HashMap::new();

                // Only include attacker's swaps (tx1 and tx3, not victim transactions)
                for swap in swaps1.iter().chain(swaps3.iter()) {
                    // Token0 is sold (negative position)
                    let entry0 = net_position.entry(swap.token0.clone()).or_insert((0.0, swap.decimals0));
                    entry0.0 -= swap.amount0;

                    // Token1 is bought (positive position)
                    let entry1 = net_position.entry(swap.token1.clone()).or_insert((0.0, swap.decimals1));
                    entry1.0 += swap.amount1;
                }

                // Calculate profit from net positions
                let mut revenue_usd = 0.0;
                let mut cost_usd = 0.0;
                let mut unsupported_profit_tokens = Vec::new();

                for (mint, (amount, _decimals)) in &net_position {
                    let price = price_map.get(mint).copied().unwrap_or(0.0);
                    let value_usd = amount.abs() * price;

                    // Only flag unsupported tokens if they have significant position (>1 token)
                    // Ignore near-zero positions from rounding in cycled tokens
                    let is_significant = amount.abs() > 1.0;

                    if *amount > 0.0 {
                        // Net positive = revenue
                        if price == 0.0 && is_significant {
                            unsupported_profit_tokens.push(mint.clone());
                        }
                        revenue_usd += value_usd;
                    } else if *amount < 0.0 {
                        // Net negative = cost
                        if price == 0.0 && is_significant {
                            unsupported_profit_tokens.push(mint.clone());
                        }
                        cost_usd += value_usd;
                    }
                }

                let profit_usd = revenue_usd - cost_usd;

                // Only count attacker's fees (front_run and back_run)
                let total_fees = tx1.fee().unwrap_or(0) + tx3.fee().unwrap_or(0);
                let total_jito_tips = tx1.jito_tip().unwrap_or(0) + tx3.jito_tip().unwrap_or(0);
                let sol_price = price_map.get("So11111111111111111111111111111111111111112").copied().unwrap_or(131.0);
                let fees_usd = (total_fees + total_jito_tips) as f64 / 1_000_000_000.0 * sol_price;
                let net_profit_usd = profit_usd - fees_usd;

                sandwiches.push(SandwichEvent {
                    slot,
                    signer: signer1.clone(),
                    sandwiched_token,
                    front_run: SandwichTransaction {
                        signature: tx1.signature.clone(),
                        index: tx1.index,
                        signer: signer1.clone(),
                        compute_units: tx1.compute_units_consumed().unwrap_or(0),
                        fee: tx1.fee().unwrap_or(0),
                    },
                    back_run: SandwichTransaction {
                        signature: tx3.signature.clone(),
                        index: tx3.index,
                        signer: signer3,
                        compute_units: tx3.compute_units_consumed().unwrap_or(0),
                        fee: tx3.fee().unwrap_or(0),
                    },
                    front_run_swaps: swaps1.to_vec(),
                    back_run_swaps: swaps3.to_vec(),
                    total_compute_units: {
                        let mut total = tx1.compute_units_consumed().unwrap_or(0)
                            + tx3.compute_units_consumed().unwrap_or(0);
                        // Add compute units from all victim transactions
                        for k in (i + 1)..j {
                            total += sorted[k].0.compute_units_consumed().unwrap_or(0);
                        }
                        total
                    },
                    total_fees,
                    total_jito_tips,
                    program_addresses,
                    token_changes,
                    profitability: Profitability {
                        profit_usd,
                        fees_usd,
                        net_profit_usd,
                        unsupported_profit_tokens,
                    },
                });

                // Found a sandwich with this front_run, move to next front_run
                break;
            }
        }

        sandwiches
    }
}
