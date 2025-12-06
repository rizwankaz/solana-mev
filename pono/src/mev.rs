use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;

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
    pub slot: u64,
    pub signer: String,
    pub swap_count: usize,
    pub transfer_count: usize,
    pub compute_units: u64,
    pub fee: u64,
    pub profit_tokens: Vec<TokenProfit>,
    pub programs: Vec<String>,
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

/// Token profit info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenProfit {
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
}

impl Default for MevDetector {
    fn default() -> Self {
        Self {
            min_swap_count: 2,
            max_sandwich_distance: 5,
        }
    }
}

impl MevDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Detect all MEV events in a block
    pub fn detect_mev(
        &self,
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
            if let Some(arb) = self.detect_arbitrage(slot, tx) {
                events.push(MevEvent::Arbitrage(arb));
            }
        }

        // Detect sandwich attacks
        for sandwich in self.detect_sandwiches(slot, &candidates) {
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
    fn detect_arbitrage(
        &self,
        slot: u64,
        tx: &FetchedTransaction,
    ) -> Option<ArbitrageEvent> {
        let swap_count = self.count_swaps(tx);

        // Must have multiple swaps for arbitrage
        if swap_count < self.min_swap_count {
            return None;
        }

        let transfers = self.extract_transfers(tx);
        let signer = tx.signer()?;

        // Check for profit (any token with positive delta owned by signer)
        let mut profit_tokens = Vec::new();
        for transfer in &transfers {
            if transfer.delta > 0 && transfer.owner == signer {
                profit_tokens.push(TokenProfit {
                    mint: transfer.mint.clone(),
                    delta: transfer.delta,
                    decimals: transfer.decimals,
                });
            }
        }

        // Must have some profit
        if profit_tokens.is_empty() {
            return None;
        }

        Some(ArbitrageEvent {
            signature: tx.signature.clone(),
            slot,
            signer,
            swap_count,
            transfer_count: self.count_transfers(tx),
            compute_units: tx.compute_units_consumed().unwrap_or(0),
            fee: tx.fee().unwrap_or(0),
            profit_tokens,
            programs: self.extract_programs(tx),
        })
    }

    /// Detect sandwich attacks
    fn detect_sandwiches(
        &self,
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
                        total_fees: tx1.fee().unwrap_or(0)
                            + tx2.fee().unwrap_or(0)
                            + tx3.fee().unwrap_or(0),
                    });
                }
            }
        }

        sandwiches
    }
}
