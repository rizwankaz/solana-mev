use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;

/// Individual swap within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapInfo {
    pub token0: String,
    pub amount0: f64,
    pub token1: String,
    pub amount1: f64,
    pub dex: String,
    pub decimals0: u8,
    pub decimals1: u8,
}

/// Swap parser for extracting swap details from transactions
pub struct SwapParser;

impl SwapParser {
    pub fn new() -> Self {
        Self
    }

    /// Extract all swaps from a transaction
    pub fn extract_swaps(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        // Get token balance changes
        let token_changes = self.extract_token_changes(tx);

        // Get programs used in transaction
        let programs = self.extract_dex_programs(tx);

        if programs.is_empty() {
            return swaps;
        }

        // Get the DEX program to use for labeling
        let dex = programs.first().unwrap_or(&"Unknown".to_string()).clone();

        // Group changes by positive and negative, per unique owner
        use std::collections::HashMap;
        let mut owner_changes: HashMap<String, (Vec<&TokenChange>, Vec<&TokenChange>)> = HashMap::new();

        for change in &token_changes {
            let entry = owner_changes.entry(change.owner.clone()).or_insert((Vec::new(), Vec::new()));
            if change.delta < 0 {
                entry.0.push(change);
            } else if change.delta > 0 {
                entry.1.push(change);
            }
        }

        // For each owner, try to match negative and positive changes
        for (_owner, (negative_changes, positive_changes)) in owner_changes {
            // Match each negative change with each positive change for different tokens
            for from_change in &negative_changes {
                for to_change in &positive_changes {
                    // Only create swap if tokens are different
                    if from_change.mint != to_change.mint {
                        swaps.push(SwapInfo {
                            token0: from_change.mint.clone(),
                            amount0: from_change.delta.abs() as f64 / 10_f64.powi(from_change.decimals as i32),
                            token1: to_change.mint.clone(),
                            amount1: to_change.delta as f64 / 10_f64.powi(to_change.decimals as i32),
                            dex: dex.clone(),
                            decimals0: from_change.decimals,
                            decimals1: to_change.decimals,
                        });
                    }
                }
            }
        }

        swaps
    }

    /// Extract token balance changes
    pub fn extract_token_changes(&self, tx: &FetchedTransaction) -> Vec<TokenChange> {
        let mut changes = Vec::new();

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

                        changes.push(TokenChange {
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

        changes
    }

    /// Check if a program is a known system/utility program (not a DEX)
    fn is_system_program(program: &str) -> bool {
        matches!(program,
            "ComputeBudget111111111111111111111111111111" |
            "11111111111111111111111111111111" | // System program
            "system" |
            "spl-token" |
            "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" | // Token program
            "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" | // Associated Token program
            "spl-associated-token-account" |
            "Vote111111111111111111111111111111111111111" | // Vote program
            "Config1111111111111111111111111111111111111" | // Config program
            "Stake11111111111111111111111111111111111111" // Stake program
        )
    }

    /// Extract programs from transaction
    pub fn extract_dex_programs(&self, tx: &FetchedTransaction) -> Vec<String> {
        use solana_transaction_status::{EncodedTransaction, UiMessage, UiInstruction, UiParsedInstruction};

        let programs: Vec<String> = match &tx.transaction {
            EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    UiMessage::Parsed(parsed) => {
                        parsed.instructions.iter()
                            .filter_map(|inst| {
                                match inst {
                                    UiInstruction::Parsed(parsed_inst) => {
                                        // UiParsedInstruction is an enum, extract program based on variant
                                        match parsed_inst {
                                            UiParsedInstruction::Parsed(info) => {
                                                Some(info.program.clone())
                                            }
                                            UiParsedInstruction::PartiallyDecoded(partial) => {
                                                Some(partial.program_id.clone())
                                            }
                                        }
                                    }
                                    UiInstruction::Compiled(compiled) => {
                                        let program_id_index = compiled.program_id_index as usize;
                                        parsed.account_keys.get(program_id_index)
                                            .map(|key| key.pubkey.clone())
                                    }
                                }
                            })
                            .collect()
                    }
                    UiMessage::Raw(raw) => {
                        raw.instructions.iter()
                            .filter_map(|inst| {
                                let idx = inst.program_id_index as usize;
                                raw.account_keys.get(idx).cloned()
                            })
                            .collect()
                    }
                }
            }
            _ => Vec::new(),
        };

        // Filter out system programs and deduplicate
        let mut dex_programs: Vec<String> = programs.into_iter()
            .filter(|p| !Self::is_system_program(p))
            .collect();
        dex_programs.sort();
        dex_programs.dedup();
        dex_programs
    }
}

impl Default for SwapParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct TokenChange {
    pub account_index: usize,
    pub mint: String,
    pub owner: String,
    pub pre_amount: u64,
    pub post_amount: u64,
    pub delta: i64,
    pub decimals: u8,
}
