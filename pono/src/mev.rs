use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;
use crate::swap::{SwapParser, SwapInfo};
use crate::oracle::OracleClient;

/// MEV event type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MevEvent {
    Arbitrage(ArbitrageEvent),
    Sandwich(SandwichEvent),
}

/// Arbitrage MEV event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageEvent {
    pub signature: String,
    pub signer: String,
    pub success: bool,
    pub compute_units_consumed: u64,
    pub fee: u64,
    pub priority_fee: u64,
    pub swaps: Vec<SwapInfo>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<TokenChange>,
    pub profitability: Profitability,
}

/// Sandwich attack MEV event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichEvent {
    pub slot: u64,
    pub attacker: String,
    pub victim_signature: String,
    pub front_run: SandwichTransaction,
    pub victim: SandwichTransaction,
    pub back_run: SandwichTransaction,
    pub total_compute_units: u64,
    pub total_fees: u64,
    pub swaps: Vec<SwapInfo>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<TokenChange>,
    pub profitability: Profitability,
}

/// Transaction in sandwich pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichTransaction {
    pub signature: String,
    pub index: usize,
    pub signer: String,
    pub compute_units: u64,
    pub fee: u64,
}

/// Profitability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profitability {
    pub profit_usd: f64,
    pub fees_usd: f64,
    pub net_profit_usd: f64,
}

/// Token balance change
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenChange {
    pub mint: String,
    pub delta: i64,
    pub decimals: u8,
}

/// Token balance change
#[derive(Debug, Clone)]
pub struct TokenTransfer {
    pub account_index: usize,
    pub mint: String,
    pub owner: String,
    pub pre_amount: u64,
    pub post_amount: u64,
    pub delta: i64,
    pub decimals: u8,
}

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

    /// Count swap instructions in transaction
    fn count_swaps(&self, tx: &FetchedTransaction) -> usize {
        use solana_transaction_status::option_serializer::OptionSerializer;

        if let Some(meta) = &tx.meta {
            let logs = match &meta.log_messages {
                OptionSerializer::Some(logs) => logs,
                _ => return 0,
            };

            return logs
                .iter()
                .filter(|msg| msg.contains("Instruction: Swap"))
                .count();
        }
        0
    }

    /// Count transfer instructions
    fn count_transfers(&self, tx: &FetchedTransaction) -> usize {
        use solana_transaction_status::option_serializer::OptionSerializer;

        if let Some(meta) = &tx.meta {
            let logs = match &meta.log_messages {
                OptionSerializer::Some(logs) => logs,
                _ => return 0,
            };

            return logs
                .iter()
                .filter(|msg| msg.contains("Instruction: Transfer"))
                .count();
        }
        0
    }

    /// Extract token transfers from transaction
    fn extract_transfers(&self, tx: &FetchedTransaction) -> Vec<TokenTransfer> {
        let mut transfers = Vec::new();

        if let Some(meta) = &tx.meta {
            let pre_balances = meta.pre_token_balances.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
            let post_balances = meta.post_token_balances.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);

            // Index balances by account index
            use std::collections::HashMap;
            let mut pre_map: HashMap<usize, _> = HashMap::new();
            let mut post_map: HashMap<usize, _> = HashMap::new();

            for balance in pre_balances {
                pre_map.insert(balance.account_index as usize, balance);
            }

            for balance in post_balances {
                post_map.insert(balance.account_index as usize, balance);
            }

            // Find all accounts with token balance changes
            let all_indices: std::collections::HashSet<_> = pre_map
                .keys()
                .chain(post_map.keys())
                .copied()
                .collect();

            for idx in all_indices {
                let pre = pre_map.get(&idx);
                let post = post_map.get(&idx);

                if let (Some(pre_bal), Some(post_bal)) = (pre, post) {
                    let pre_amount = pre_bal.ui_token_amount.amount.parse::<u64>().unwrap_or(0);
                    let post_amount = post_bal.ui_token_amount.amount.parse::<u64>().unwrap_or(0);

                    if pre_amount != post_amount {
                        use solana_transaction_status::option_serializer::OptionSerializer;

                        let owner = match &post_bal.owner {
                            OptionSerializer::Some(o) => o.clone(),
                            _ => String::new(),
                        };

                        transfers.push(TokenTransfer {
                            account_index: idx,
                            mint: post_bal.mint.clone(),
                            owner,
                            pre_amount,
                            post_amount,
                            delta: post_amount as i64 - pre_amount as i64,
                            decimals: post_bal.ui_token_amount.decimals,
                        });
                    }
                }
            }
        }

        transfers
    }

    /// Extract program IDs from transaction
    fn extract_programs(&self, tx: &FetchedTransaction) -> Vec<String> {
        use solana_transaction_status::{EncodedTransaction, UiMessage};

        match &tx.transaction {
            EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    UiMessage::Parsed(parsed) => {
                        // Extract from instructions
                        parsed.instructions.iter()
                            .filter_map(|inst| {
                                match inst {
                                    solana_transaction_status::UiInstruction::Parsed(_) => {
                                        // UiParsedInstruction has different structure
                                        // We'll just use the account keys instead
                                        None
                                    }
                                    solana_transaction_status::UiInstruction::Compiled(compiled) => {
                                        // Get program id from account keys
                                        let program_id_index = compiled.program_id_index as usize;
                                        parsed.account_keys.get(program_id_index)
                                            .map(|key| key.pubkey.clone())
                                    }
                                }
                            })
                            .collect()
                    }
                    UiMessage::Raw(raw) => {
                        // Get unique programs from instructions
                        let mut programs: Vec<String> = raw.instructions.iter()
                            .filter_map(|inst| {
                                let idx = inst.program_id_index as usize;
                                raw.account_keys.get(idx).cloned()
                            })
                            .collect();
                        programs.sort();
                        programs.dedup();
                        programs
                    }
                }
            }
            _ => Vec::new(),
        }
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

        // Convert token changes to TokenChange format
        let token_changes_output: Vec<TokenChange> = signer_changes.iter()
            .map(|tc| TokenChange {
                mint: tc.mint.clone(),
                delta: tc.delta,
                decimals: tc.decimals,
            })
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
                    use std::collections::HashMap;
                    let mut combined_changes: HashMap<String, (i64, u8)> = HashMap::new();

                    for change in changes1.iter().chain(changes3.iter()) {
                        if change.owner == signer1 {
                            let entry = combined_changes.entry(change.mint.clone()).or_insert((0, change.decimals));
                            entry.0 += change.delta;
                        }
                    }

                    let token_changes: Vec<TokenChange> = combined_changes.iter()
                        .map(|(mint, (delta, decimals))| TokenChange {
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
                        attacker: signer1.clone(),
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
