use crate::types::{
    ArbitrageEvent, FetchedTransaction, MevEvent, Profitability, SandwichEvent,
    SandwichTransaction, SimpleTokenChange, TokenChange,
};
use crate::parsers::SwapParser;
use crate::oracle::OracleClient;
use std::collections::HashMap;

/// MEV detector for analyzing blocks
pub struct MevDetector {
    /// Minimum swap count for arbitrage
    pub min_swap_count: usize,
    /// Maximum block distance for sandwich detection
    pub max_sandwich_distance: usize,
    /// Swap parser for extracting swap information
    swap_parser: SwapParser,
    /// Oracle client for price data
    oracle: OracleClient,
}

impl MevDetector {
    pub fn new(timestamp: i64) -> Self {
        Self {
            min_swap_count: 2,
            max_sandwich_distance: 5,
            swap_parser: SwapParser::new(),
            oracle: OracleClient::new(timestamp),
        }
    }

    /// Detect all MEV events in a block
    pub async fn detect_mev(
        &mut self,
        slot: u64,
        transactions: &[FetchedTransaction],
    ) -> Vec<MevEvent> {
        let mut events = Vec::new();

        // Filter for successful transactions with swaps/transfers
        let candidates: Vec<_> = transactions
            .iter()
            .filter(|tx| tx.is_success() && self.has_swap_or_transfer(tx))
            .collect();

        // Detect arbitrage
        for tx in &candidates {
            if let Some(arb) = self.detect_arbitrage(tx).await {
                events.push(MevEvent::Arbitrage(arb));
            }
        }

        // Detect sandwich attacks
        for sandwich in self.detect_sandwiches(slot, &candidates).await {
            events.push(MevEvent::Sandwich(sandwich));
        }

        events
    }

    /// Check if transaction contains swap or transfer instructions
    fn has_swap_or_transfer(&self, tx: &FetchedTransaction) -> bool {
        use solana_transaction_status::option_serializer::OptionSerializer;

        if let Some(meta) = &tx.meta {
            let logs = match &meta.log_messages {
                OptionSerializer::Some(logs) => logs,
                _ => return false,
            };

            return logs.iter().any(|msg| {
                msg.contains("Instruction: Swap") || msg.contains("Instruction: Transfer")
            });
        }
        false
    }

    /// Detect arbitrage in a transaction
    async fn detect_arbitrage(
        &mut self,
        tx: &FetchedTransaction,
    ) -> Option<ArbitrageEvent> {
        let signer = tx.signer()?;

        // Extract swaps and token changes
        let swaps = self.swap_parser.extract_swaps(tx);
        let token_changes = self.swap_parser.extract_token_changes(tx);
        let program_addresses = self.swap_parser.extract_dex_programs(tx);

        // Must have multiple swaps for arbitrage
        if swaps.len() < self.min_swap_count {
            return None;
        }

        // Check for profit (any token with positive delta owned by signer)
        let signer_changes: Vec<_> = token_changes.iter()
            .filter(|tc| tc.owner == signer)
            .collect();

        let has_profit = signer_changes.iter().any(|tc| tc.delta > 0);
        if !has_profit {
            return None;
        }

        // Convert token changes to SimpleTokenChange format for output
        let token_changes_output: Vec<SimpleTokenChange> = signer_changes.iter()
            .map(|tc| tc.to_simple())
            .collect();

        // Calculate profitability
        let mut profit_usd = 0.0;
        for change in &signer_changes {
            if change.delta > 0 {
                let price = self.oracle.get_price_usd(&change.mint).await.unwrap_or(0.0);
                let amount = change.delta as f64 / 10_f64.powi(change.decimals as i32);
                profit_usd += amount * price;
            }
        }

        let fee = tx.fee().unwrap_or(0);
        let compute_units = tx.compute_units_consumed().unwrap_or(0);

        // Estimate priority fee (total fee minus base fee)
        // Base fee is roughly 5000 lamports per signature
        let priority_fee = fee.saturating_sub(5000);

        let fees_usd = fee as f64 / 1_000_000_000.0 * self.oracle.get_price_usd("So11111111111111111111111111111111111111112").await.unwrap_or(0.0);
        let net_profit_usd = profit_usd - fees_usd;

        Some(ArbitrageEvent {
            signature: tx.signature.clone(),
            signer,
            success: tx.is_success(),
            compute_units_consumed: compute_units,
            fee,
            priority_fee,
            swaps,
            program_addresses,
            token_changes: token_changes_output,
            profitability: Profitability {
                profit_usd,
                fees_usd,
                net_profit_usd,
            },
        })
    }

    /// Detect sandwich attacks
    async fn detect_sandwiches(
        &mut self,
        slot: u64,
        candidates: &[&FetchedTransaction],
    ) -> Vec<SandwichEvent> {
        let mut sandwiches = Vec::new();

        // Need at least 3 transactions
        if candidates.len() < 3 {
            return sandwiches;
        }

        // Sort by index to get ordering
        let mut sorted: Vec<_> = candidates.iter().collect();
        sorted.sort_by_key(|tx| tx.index);

        // Look for sandwich pattern
        for i in 0..sorted.len().saturating_sub(2) {
            let tx1 = sorted[i];
            let tx2 = sorted[i + 1];
            let tx3 = sorted[i + 2];

            let signer1 = match tx1.signer() {
                Some(s) => s,
                None => continue,
            };
            let signer2 = match tx2.signer() {
                Some(s) => s,
                None => continue,
            };
            let signer3 = match tx3.signer() {
                Some(s) => s,
                None => continue,
            };

            // Check sandwich pattern: same signer for tx1 and tx3, different for tx2
            if signer1 == signer3 && signer1 != signer2 {
                // Check they're close enough
                if tx3.index - tx1.index <= self.max_sandwich_distance {
                    // Extract swaps from all three transactions
                    let swaps1 = self.swap_parser.extract_swaps(tx1);
                    let swaps2 = self.swap_parser.extract_swaps(tx2);
                    let swaps3 = self.swap_parser.extract_swaps(tx3);
                    let mut all_swaps = Vec::new();
                    all_swaps.extend(swaps1);
                    all_swaps.extend(swaps2);
                    all_swaps.extend(swaps3);

                    // Extract token changes from attacker's transactions (tx1 and tx3)
                    let changes1 = self.swap_parser.extract_token_changes(tx1);
                    let changes3 = self.swap_parser.extract_token_changes(tx3);

                    // Combine token changes from front and back run
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

                    // Extract DEX programs
                    let progs1 = self.swap_parser.extract_dex_programs(tx1);
                    let progs2 = self.swap_parser.extract_dex_programs(tx2);
                    let progs3 = self.swap_parser.extract_dex_programs(tx3);
                    let mut program_addresses = Vec::new();
                    program_addresses.extend(progs1);
                    program_addresses.extend(progs2);
                    program_addresses.extend(progs3);
                    program_addresses.sort();
                    program_addresses.dedup();

                    // Calculate profitability
                    let mut profit_usd = 0.0;
                    for change in &token_changes {
                        if change.delta > 0 {
                            let price = self.oracle.get_price_usd(&change.mint).await.unwrap_or(0.0);
                            let amount = change.delta as f64 / 10_f64.powi(change.decimals as i32);
                            profit_usd += amount * price;
                        }
                    }

                    let total_fees = tx1.fee().unwrap_or(0)
                        + tx2.fee().unwrap_or(0)
                        + tx3.fee().unwrap_or(0);

                    let fees_usd = total_fees as f64 / 1_000_000_000.0 * self.oracle.get_price_usd("So11111111111111111111111111111111111111112").await.unwrap_or(0.0);
                    let net_profit_usd = profit_usd - fees_usd;

                    sandwiches.push(SandwichEvent {
                        slot,
                        signer: signer1.clone(),
                        victim_signature: tx2.signature.clone(),
                        front_run: SandwichTransaction {
                            signature: tx1.signature.clone(),
                            index: tx1.index,
                            signer: signer1.clone(),
                            compute_units: tx1.compute_units_consumed().unwrap_or(0),
                            fee: tx1.fee().unwrap_or(0),
                        },
                        victim: SandwichTransaction {
                            signature: tx2.signature.clone(),
                            index: tx2.index,
                            signer: signer2,
                            compute_units: tx2.compute_units_consumed().unwrap_or(0),
                            fee: tx2.fee().unwrap_or(0),
                        },
                        back_run: SandwichTransaction {
                            signature: tx3.signature.clone(),
                            index: tx3.index,
                            signer: signer3,
                            compute_units: tx3.compute_units_consumed().unwrap_or(0),
                            fee: tx3.fee().unwrap_or(0),
                        },
                        total_compute_units: tx1.compute_units_consumed().unwrap_or(0)
                            + tx2.compute_units_consumed().unwrap_or(0)
                            + tx3.compute_units_consumed().unwrap_or(0),
                        total_fees,
                        swaps: all_swaps,
                        program_addresses,
                        token_changes,
                        profitability: Profitability {
                            profit_usd,
                            fees_usd,
                            net_profit_usd,
                        },
                    });
                }
            }
        }

        sandwiches
    }
}
