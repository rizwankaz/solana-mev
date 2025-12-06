use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;
use crate::dex::DexRegistry;
use crate::tokens::TokenRegistry;

/// Individual swap within a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapInfo {
    pub from_token: String,
    pub from_token_name: Option<String>,
    pub from_amount: f64,
    pub to_token: String,
    pub to_token_name: Option<String>,
    pub to_amount: f64,
    pub dex_program: String,
    pub dex_name: String,
    pub from_decimals: u8,
    pub to_decimals: u8,
}

/// Swap parser for extracting swap details from transactions
pub struct SwapParser {
    dex_registry: DexRegistry,
    token_registry: TokenRegistry,
}

impl SwapParser {
    pub fn new() -> Self {
        Self {
            dex_registry: DexRegistry::new(),
            token_registry: TokenRegistry::new(),
        }
    }

    /// Extract all swaps from a transaction
    pub fn extract_swaps(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        // Get token balance changes
        let token_changes = self.extract_token_changes(tx);

        // Get programs used in transaction
        let programs = self.extract_dex_programs(tx);

        // Match token changes to swaps
        // This is a simplified approach - in reality, you'd parse the instruction data
        // For now, we'll infer swaps from token balance changes
        if token_changes.len() >= 2 && !programs.is_empty() {
            // Group changes by positive and negative
            let mut negative_changes = Vec::new();
            let mut positive_changes = Vec::new();

            for change in &token_changes {
                if change.delta < 0 {
                    negative_changes.push(change);
                } else if change.delta > 0 {
                    positive_changes.push(change);
                }
            }

            // Try to pair negative and positive changes as swaps
            for (i, from_change) in negative_changes.iter().enumerate() {
                if let Some(to_change) = positive_changes.get(i) {
                    let dex_program = programs.first().unwrap_or(&"Unknown".to_string()).clone();
                    let dex_name = self.dex_registry
                        .get_dex_name(&dex_program)
                        .unwrap_or("Unknown DEX")
                        .to_string();

                    swaps.push(SwapInfo {
                        from_token: from_change.mint.clone(),
                        from_token_name: self.token_registry.get_symbol(&from_change.mint).map(|s| s.to_string()),
                        from_amount: from_change.delta.abs() as f64 / 10_f64.powi(from_change.decimals as i32),
                        to_token: to_change.mint.clone(),
                        to_token_name: self.token_registry.get_symbol(&to_change.mint).map(|s| s.to_string()),
                        to_amount: to_change.delta as f64 / 10_f64.powi(to_change.decimals as i32),
                        dex_program,
                        dex_name,
                        from_decimals: from_change.decimals,
                        to_decimals: to_change.decimals,
                    });
                }
            }
        }

        swaps
    }

    /// Extract token balance changes
    fn extract_token_changes(&self, tx: &FetchedTransaction) -> Vec<TokenChange> {
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
                        changes.push(TokenChange {
                            account_index: idx,
                            mint: post_bal.mint.clone(),
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

    /// Extract DEX programs from transaction
    fn extract_dex_programs(&self, tx: &FetchedTransaction) -> Vec<String> {
        use solana_transaction_status::{EncodedTransaction, UiMessage};

        let all_programs = match &tx.transaction {
            EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    UiMessage::Parsed(parsed) => {
                        parsed.instructions.iter()
                            .filter_map(|inst| {
                                match inst {
                                    solana_transaction_status::UiInstruction::Compiled(compiled) => {
                                        let program_id_index = compiled.program_id_index as usize;
                                        parsed.account_keys.get(program_id_index)
                                            .map(|key| key.pubkey.clone())
                                    }
                                    _ => None,
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

        // Filter to only DEX programs
        all_programs.into_iter()
            .filter(|program| self.dex_registry.is_dex(program))
            .collect()
    }
}

impl Default for SwapParser {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct TokenChange {
    account_index: usize,
    mint: String,
    pre_amount: u64,
    post_amount: u64,
    delta: i64,
    decimals: u8,
}
