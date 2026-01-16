use crate::oracle::OracleClient;
use crate::parsers::SwapParser;
use crate::types::{
    ArbitrageEvent, ArbitrageType, FetchedTransaction, MevEvent, Profitability, SandwichEvent,
    SandwichTransaction, SimpleTokenChange, SwapInfo, TokenChange,
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// inpsector!
pub struct MevInspector {
    /// arbs must have at least 2 swaps
    pub min_swap_count: usize,
    /// the sauce
    swap_parser: Arc<SwapParser>,
    /// get prices
    oracle: OracleClient,
}

/// lazy sandwiches
struct OwnedSandwich<'a> {
    front_run_tx: &'a FetchedTransaction,
    back_run_tx: &'a FetchedTransaction,
    front_run_swaps: Vec<crate::types::SwapInfo>,
    back_run_swaps: Vec<crate::types::SwapInfo>,
    front_run_changes: Vec<TokenChange>,
    back_run_changes: Vec<TokenChange>,
    front_run_progs: Vec<String>,
    back_run_progs: Vec<String>,
    signer: String,
    sandwiched_token: String,
    victim_progs: Vec<String>,
}

impl MevInspector {
    pub fn new(slot: u64, timestamp: i64, rpc_url: String) -> Self {
        Self {
            min_swap_count: 2,
            swap_parser: Arc::new(SwapParser::new()),
            oracle: OracleClient::new(slot, timestamp, rpc_url),
        }
    }

    /// Check if a token is a stablecoin based on its price from the oracle
    /// A stablecoin is a token with a price close to $1 (within ±$0.05)
    fn is_stablecoin(token: &str, price_map: &HashMap<String, f64>) -> bool {
        if let Some(&price) = price_map.get(token) {
            // Check if price is close to $1 (stablecoins typically trade around $1)
            price >= 0.95 && price <= 1.05
        } else {
            false
        }
    }

    /// Check if two tokens form a stable pair (both are stablecoins)
    fn is_stable_pair(token1: &str, token2: &str, price_map: &HashMap<String, f64>) -> bool {
        Self::is_stablecoin(token1, price_map) && Self::is_stablecoin(token2, price_map)
    }

    /// Classify arbitrage type based on swap patterns
    fn classify_arbitrage(swaps: &[SwapInfo], price_map: &HashMap<String, f64>) -> ArbitrageType {
        let swap_count = swaps.len();

        if swap_count == 0 || swap_count == 1 {
            return ArbitrageType::LongTail;
        }

        let first_swap = &swaps[0];
        let last_swap = &swaps[swap_count - 1];
        let first_token = &first_swap.token0;
        let last_token = &last_swap.token1;

        // Check if swaps are continuous (each swap's output matches next swap's input)
        let is_continuous = swaps.windows(2).all(|pair| {
            let current = &pair[0];
            let next = &pair[1];
            current.token1 == next.token0
        });

        if swap_count == 2 {
            // Two Swaps Logic

            // Triangle Arbitrage: Input token of Swap 1 matches output token of Swap 2, and swaps are continuous
            if first_token == last_token && is_continuous {
                return ArbitrageType::TriangleArbitrage;
            }

            // Stablecoin Arbitrage (Triangle): Both tokens are stablecoins and forms a cycle
            if Self::is_stable_pair(first_token, last_token, price_map) && is_continuous {
                return ArbitrageType::StablecoinArbitrage;
            }

            // Stablecoin Arbitrage (Non-Triangle): Input of Swap 1 and output of Swap 2 form a stable pair
            if Self::is_stable_pair(first_token, last_token, price_map) {
                return ArbitrageType::StablecoinArbitrage;
            }

            // Cross-Pair Arbitrage: Starts and ends with same token, but break in continuity
            if first_token == last_token && !is_continuous {
                return ArbitrageType::CrossPairArbitrage;
            }

            // Long Tail: Everything else
            return ArbitrageType::LongTail;
        }

        // Three or More Swaps Logic
        if swap_count >= 3 {
            // Stablecoin Arbitrage: First and last tokens form a stable pair
            if Self::is_stable_pair(first_token, last_token, price_map) {
                return ArbitrageType::StablecoinArbitrage;
            }

            // Cross-Pair Arbitrage: Starts and ends with same token, but break in continuity
            if first_token == last_token && !is_continuous {
                return ArbitrageType::CrossPairArbitrage;
            }

            // Triangle Arbitrage: All swaps continuous and ends with starting token
            if first_token == last_token && is_continuous {
                return ArbitrageType::TriangleArbitrage;
            }

            // Long Tail: Everything else
            return ArbitrageType::LongTail;
        }

        ArbitrageType::LongTail
    }

    /// find mev
    pub async fn detect_mev(
        &mut self,
        slot: u64,
        transactions: &[FetchedTransaction],
    ) -> Vec<MevEvent> {
        // single-pass
        let swap_parser = self.swap_parser.clone();

        // parallel arb detection
        let arbitrage_candidates: Vec<_> = transactions
            .par_iter()
            .filter(|tx| tx.is_success() && Self::has_potential_mev(tx))
            .filter_map(|tx| {
                // lazy
                let swaps = swap_parser.extract_swaps(tx);

                // do we have enough swaps?
                if swaps.len() < self.min_swap_count {
                    return None;
                }

                let signer = tx.signer()?;
                let token_changes = swap_parser.extract_token_changes(tx);
                let has_profit = token_changes
                    .iter()
                    .any(|tc| tc.owner == signer && tc.delta > 0);

                if has_profit {
                    let program_addresses = swap_parser.extract_dex_programs(tx);
                    return Some((tx, swaps, token_changes, program_addresses));
                }

                None
            })
            .collect();

        // find sandwiches
        let sandwich_candidates = if transactions.len() >= 3 {
            Self::identify_sandwiches_lazy(transactions, &swap_parser)
        } else {
            Vec::new()
        };

        // exit
        if arbitrage_candidates.is_empty() && sandwich_candidates.is_empty() {
            tracing::debug!("no mev found in slot {}", slot);
            return Vec::new();
        }

        tracing::debug!(
            "found {} arb and {} sw candidates in slot {}",
            slot,
            arbitrage_candidates.len(),
            sandwich_candidates.len()
        );

        // pricing tokens; please get an api
        let mut unique_mints = HashSet::new();
        unique_mints.insert("So11111111111111111111111111111111111111112"); // we always need sol

        for (tx, _swaps, token_changes, _progs) in &arbitrage_candidates {
            if let Some(signer) = tx.signer() {
                for change in token_changes.iter() {
                    if change.owner == signer && change.delta != 0 {
                        unique_mints.insert(change.mint.as_str());
                    }
                }
            }
        }

        for candidate in &sandwich_candidates {
            for change in candidate.front_run_changes.iter() {
                if change.owner == candidate.signer && change.delta != 0 {
                    unique_mints.insert(change.mint.as_str());
                }
            }
            for change in candidate.back_run_changes.iter() {
                if change.owner == candidate.signer && change.delta != 0 {
                    unique_mints.insert(change.mint.as_str());
                }
            }
        }

        // batch fetch price
        let mints_vec: Vec<&str> = unique_mints.into_iter().collect();
        let price_map: HashMap<String, f64> = self
            .oracle
            .batch_get_prices(&mints_vec)
            .await
            .into_iter()
            .collect();

        let mut events = Vec::with_capacity(arbitrage_candidates.len() + sandwich_candidates.len());

        // only profitable arbs
        let arbitrages: Vec<_> = arbitrage_candidates
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
            .filter(|arb| arb.profitability.profit_usd > 0.0) // Only include profitable arbitrages
            .collect();

        for arb in arbitrages {
            events.push(MevEvent::Arbitrage(arb));
        }

        // only profitable sws
        for sandwich in
            Self::calculate_sandwich_profitability(slot, sandwich_candidates, &price_map)
        {
            events.push(MevEvent::Sandwich(sandwich));
        }

        events
    }
    // swap?
    #[inline]
    fn has_potential_mev(tx: &FetchedTransaction) -> bool {
        use solana_transaction_status::option_serializer::OptionSerializer;

        if let Some(meta) = &tx.meta {
            if let OptionSerializer::Some(inner) = &meta.inner_instructions {
                if !inner.is_empty() {
                    return true;
                }
            }

            if let OptionSerializer::Some(logs) = &meta.log_messages {
                return logs.iter().any(|msg| {
                    msg.contains("Instruction: Swap")
                        || msg.contains("Instruction: Transfer")
                        || msg.contains("Program log: Instruction: Swap")
                        || msg.contains("swap")
                        || msg.contains("Swap")
                });
            }
        }
        false
    }

    fn detect_arbitrage_with_prices(
        tx: &FetchedTransaction,
        swaps: &[crate::types::SwapInfo],
        token_changes: &[TokenChange],
        program_addresses: &[String],
        price_map: &HashMap<String, f64>,
        min_swap_count: usize,
    ) -> Option<ArbitrageEvent> {
        let signer = tx.signer()?;

        if swaps.len() < min_swap_count {
            return None;
        }

        // Classify the arbitrage type
        let arbitrage_type = Self::classify_arbitrage(swaps, price_map);

        // Filter out Long Tail transactions (not true arbitrage)
        if matches!(arbitrage_type, ArbitrageType::LongTail) {
            return None;
        }

        // profit please
        let signer_changes: Vec<_> = token_changes
            .iter()
            .filter(|tc| tc.owner == signer)
            .collect();

        let has_profit = signer_changes.iter().any(|tc| tc.delta > 0);
        if !has_profit {
            return None;
        }

        // dedupe token changes
        let mut changes_by_mint: HashMap<String, (i64, u8)> = HashMap::new();
        for change in &signer_changes {
            let entry = changes_by_mint
                .entry(change.mint.clone())
                .or_insert((0, change.decimals));
            entry.0 += change.delta;
        }

        // SimpleTokenChange format for output
        let token_changes_output: Vec<SimpleTokenChange> = changes_by_mint
            .iter()
            .map(|(mint, &(delta, decimals))| SimpleTokenChange {
                mint: mint.clone(),
                delta,
                decimals,
            })
            .collect();

        let mut net_position: HashMap<String, (f64, u8)> = HashMap::new();

        for (mint, (delta, decimals)) in &changes_by_mint {
            let normalized_amount = *delta as f64 / 10_f64.powi(*decimals as i32);
            net_position.insert(mint.clone(), (normalized_amount, *decimals));
        }

        let mut revenue_usd = 0.0;
        let mut cost_usd = 0.0;
        // this shouldn't be much of a problem with a better api but for now
        let mut unsupported_profit_tokens = Vec::new();

        for (mint, (amount, _decimals)) in &net_position {
            let price = price_map.get(mint).copied().unwrap_or(0.0);
            let value_usd = amount.abs() * price;
            let is_significant = amount.abs() > 1.0;

            if *amount > 0.0 {
                if price == 0.0 && is_significant {
                    unsupported_profit_tokens.push(mint.clone());
                }
                revenue_usd += value_usd;
            } else if *amount < 0.0 {
                if price == 0.0 && is_significant {
                    unsupported_profit_tokens.push(mint.clone());
                }
                cost_usd += value_usd;
            }
        }

        let revenue_usd = revenue_usd - cost_usd;

        // consider defaults
        let fee = tx.fee().unwrap_or(0);
        let compute_units = tx.compute_units_consumed().unwrap_or(0);
        let priority_fee = fee.saturating_sub(5000);
        let jito_tip = tx.jito_tip().unwrap_or(0);
        let sol_price = price_map
            .get("So11111111111111111111111111111111111111112")
            .copied()
            .unwrap_or(130.0);
        let fees_usd = (fee + jito_tip) as f64 / 1_000_000_000.0 * sol_price;
        let profit_usd = revenue_usd - fees_usd;

        Some(ArbitrageEvent {
            signature: tx.signature.clone(),
            signer,
            compute_units_consumed: compute_units,
            fee,
            priority_fee,
            jito_tip,
            swaps: swaps.to_vec(),
            program_addresses: program_addresses.to_vec(),
            token_changes: token_changes_output,
            profitability: Profitability {
                revenue_usd,
                fees_usd,
                profit_usd,
                unsupported_profit_tokens,
            },
            arbitrage_type,
        })
    }

    /// lazy sandwich detection
    fn identify_sandwiches_lazy<'a>(
        transactions: &'a [FetchedTransaction],
        swap_parser: &Arc<SwapParser>,
    ) -> Vec<OwnedSandwich<'a>> {
        let mut candidates = Vec::new();

        if transactions.len() < 3 {
            return candidates;
        }

        let mut tx_by_signer: HashMap<String, Vec<&FetchedTransaction>> = HashMap::new();
        for tx in transactions {
            // Only consider successful transactions
            if !tx.is_success() {
                continue;
            }

            if let Some(signer) = tx.signer() {
                tx_by_signer.entry(signer.to_string()).or_default().push(tx);
            }
        }

        for (signer, txs) in tx_by_signer.iter() {
            if txs.len() < 2 {
                continue;
            }

            // greedy matching
            let mut used = vec![false; txs.len()];

            for i in 0..txs.len() {
                if used[i] {
                    continue;
                }

                for j in (i + 1)..txs.len() {
                    if used[j] {
                        continue;
                    }

                    let front_run_tx = txs[i];
                    let back_run_tx = txs[j];

                    // Check for successful victim transaction between front-run and back-run
                    let has_victim = transactions
                        .iter()
                        .filter(|tx| tx.index > front_run_tx.index && tx.index < back_run_tx.index)
                        .filter(|tx| tx.is_success())  // Only count successful victims
                        .any(|tx| tx.signer().map(|s| s != *signer).unwrap_or(false));

                    if !has_victim {
                        continue;
                    }

                    let front_swaps = swap_parser.extract_swaps(front_run_tx);
                    let back_swaps = swap_parser.extract_swaps(back_run_tx);

                    if front_swaps.len() != 1 || back_swaps.len() != 1 {
                        continue;
                    }

                    let front_swap = &front_swaps[0];
                    let back_swap = &back_swaps[0];

                    let front_pair = if front_swap.token0 < front_swap.token1 {
                        (&front_swap.token0, &front_swap.token1)
                    } else {
                        (&front_swap.token1, &front_swap.token0)
                    };
                    let back_pair = if back_swap.token0 < back_swap.token1 {
                        (&back_swap.token0, &back_swap.token1)
                    } else {
                        (&back_swap.token1, &back_swap.token0)
                    };

                    if front_pair != back_pair {
                        continue;
                    }

                    let same_direction = front_swap.token0 == back_swap.token0
                        && front_swap.token1 == back_swap.token1;
                    if same_direction {
                        continue;
                    }

                    let front_changes = swap_parser.extract_token_changes(front_run_tx);
                    let back_changes = swap_parser.extract_token_changes(back_run_tx);
                    let front_progs = swap_parser.extract_dex_programs(front_run_tx);
                    let back_progs = swap_parser.extract_dex_programs(back_run_tx);

                    const SOL: &str = "So11111111111111111111111111111111111111112";
                    let sandwiched_token = if front_swap.token0 == SOL {
                        front_swap.token1.clone()
                    } else if front_swap.token1 == SOL {
                        front_swap.token0.clone()
                    } else {
                        front_swap.token1.clone()
                    };

                    let mut victim_progs = Vec::new();
                    for tx in transactions.iter() {
                        if tx.index > front_run_tx.index
                            && tx.index < back_run_tx.index
                            && tx.is_success()
                        {
                            if let Some(victim_signer) = tx.signer() {
                                if victim_signer != *signer {
                                    victim_progs.extend(swap_parser.extract_dex_programs(tx));
                                }
                            }
                        }
                    }
                    victim_progs.sort_unstable();
                    victim_progs.dedup();

                    candidates.push(OwnedSandwich {
                        front_run_tx,
                        back_run_tx,
                        front_run_swaps: front_swaps,
                        back_run_swaps: back_swaps,
                        front_run_changes: front_changes,
                        back_run_changes: back_changes,
                        front_run_progs: front_progs,
                        back_run_progs: back_progs,
                        victim_progs,
                        signer: signer.to_string(),
                        sandwiched_token,
                    });

                    used[i] = true;
                    used[j] = true;
                    break;
                }
            }
        }

        candidates
    }

    /// only profitable sandwiches
    fn calculate_sandwich_profitability(
        slot: u64,
        candidates: Vec<OwnedSandwich>,
        price_map: &HashMap<String, f64>,
    ) -> Vec<SandwichEvent> {
        let mut sandwiches = Vec::new();

        for candidate in candidates {
            tracing::debug!(
                "  sandwich candidate: front={} back={}",
                &candidate.front_run_tx.signature[..12],
                &candidate.back_run_tx.signature[..12]
            );

            let front_swap = &candidate.front_run_swaps[0];
            let back_swap = &candidate.back_run_swaps[0];

            tracing::debug!(
                "    front swap: token0={} amount0={}, token1={} amount1={}",
                &front_swap.token0[..8],
                front_swap.amount0,
                &front_swap.token1[..8],
                front_swap.amount1
            );
            tracing::debug!(
                "    back swap: token0={} amount0={}, token1={} amount1={}",
                &back_swap.token0[..8],
                back_swap.amount0,
                &back_swap.token1[..8],
                back_swap.amount1
            );

            let payment_token = if front_swap.token0
                == "So11111111111111111111111111111111111111112"
                || front_swap.token1 == "So11111111111111111111111111111111111111112"
            {
                "So11111111111111111111111111111111111111112" // SOL
            } else {
                &front_swap.token0
            };

            tracing::debug!("    payment token: {}", &payment_token[..8]);
            tracing::debug!(
                "    front_swap.token0 ({}) == payment_token? {}",
                &front_swap.token0[..8],
                front_swap.token0 == payment_token
            );

            let (spent, received) = if front_swap.token0 == payment_token {
                tracing::debug!("    branch 1: spent=front.amount0, received=back.amount1");
                let spent = front_swap.amount0;
                let received = back_swap.amount1;
                (spent, received)
            } else {
                tracing::debug!("    branch 2: spent=back.amount0, received=front.amount1");
                let spent = back_swap.amount0;
                let received = front_swap.amount1;
                (spent, received)
            };

            let profit_in_token = received - spent;

            tracing::debug!(
                "    swap-based profit: spent={:.6} {}, received={:.6} {}, profit={:.6}",
                spent,
                &payment_token[..8],
                received,
                &payment_token[..8],
                profit_in_token
            );

            let token_price = price_map.get(payment_token).copied().unwrap_or_else(|| {
                if payment_token == "So11111111111111111111111111111111111111112" {
                    130.0 // default?
                } else {
                    1.0 // probably a stable
                }
            });

            let revenue_usd = profit_in_token.max(0.0) * token_price;

            // fees!
            let total_fees = candidate.front_run_tx.fee().unwrap_or(0)
                + candidate.back_run_tx.fee().unwrap_or(0);
            let total_jito_tips = candidate.front_run_tx.jito_tip().unwrap_or(0)
                + candidate.back_run_tx.jito_tip().unwrap_or(0);
            let sol_price = price_map
                .get("So11111111111111111111111111111111111111112")
                .copied()
                .unwrap_or(127.0);
            let fees_usd = (total_fees + total_jito_tips) as f64 / 1_000_000_000.0 * sol_price;
            let profit_usd = revenue_usd - fees_usd;
            let unsupported_profit_tokens: Vec<String> = vec![];

            tracing::debug!(
                "  profitability: revenue=${:.4}, fees=${:.4}, profit=${:.4}",
                revenue_usd,
                fees_usd,
                profit_usd
            );

            if profit_usd <= 0.0 {
                tracing::debug!("  filtered: unprofitable (profit=${:.4})", profit_usd);
                continue;
            }

            tracing::info!("  sandwich detected; profit: ${:.4}", profit_usd);

            let mut combined_changes: HashMap<String, (i64, u8)> = HashMap::new();
            for change in candidate
                .front_run_changes
                .iter()
                .chain(candidate.back_run_changes.iter())
            {
                if change.owner == candidate.signer {
                    let entry = combined_changes
                        .entry(change.mint.clone())
                        .or_insert((0, change.decimals));
                    entry.0 += change.delta;
                }
            }

            let token_changes: Vec<SimpleTokenChange> = combined_changes
                .iter()
                .map(|(mint, (delta, decimals))| SimpleTokenChange {
                    mint: mint.clone(),
                    delta: *delta,
                    decimals: *decimals,
                })
                .collect();

            let mut program_addresses = Vec::new();
            program_addresses.extend(candidate.front_run_progs.iter().cloned());
            program_addresses.extend(candidate.victim_progs.iter().cloned());
            program_addresses.extend(candidate.back_run_progs.iter().cloned());
            program_addresses.sort_unstable();
            program_addresses.dedup();

            sandwiches.push(SandwichEvent {
                slot,
                signer: candidate.signer.clone(),
                sandwiched_token: candidate.sandwiched_token,
                front_run: SandwichTransaction {
                    signature: candidate.front_run_tx.signature.clone(),
                    index: candidate.front_run_tx.index,
                    signer: candidate.signer.clone(),
                    compute_units: candidate.front_run_tx.compute_units_consumed().unwrap_or(0),
                    fee: candidate.front_run_tx.fee().unwrap_or(0),
                    swap: candidate.front_run_swaps.to_vec(),
                },
                back_run: SandwichTransaction {
                    signature: candidate.back_run_tx.signature.clone(),
                    index: candidate.back_run_tx.index,
                    signer: candidate.signer.clone(),
                    compute_units: candidate.back_run_tx.compute_units_consumed().unwrap_or(0),
                    fee: candidate.back_run_tx.fee().unwrap_or(0),
                    swap: candidate.back_run_swaps.to_vec(),
                },
                total_compute_units: candidate.front_run_tx.compute_units_consumed().unwrap_or(0)
                    + candidate.back_run_tx.compute_units_consumed().unwrap_or(0),
                total_fees,
                total_jito_tips,
                program_addresses,
                token_changes,
                profitability: Profitability {
                    revenue_usd,
                    fees_usd,
                    profit_usd,
                    unsupported_profit_tokens,
                },
            });
        }

        sandwiches
    }
}
