use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;
use std::collections::HashMap;
use solana_transaction_status::{
    option_serializer::OptionSerializer,
    EncodedTransaction, UiMessage, UiInstruction, UiParsedInstruction,
};

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

/// Token transfer within inner instructions
#[derive(Debug)]
struct Transfer {
    mint: String,
    amount: u64,
    decimals: u8,
}

/// Swap parser for extracting swap details from transactions
pub struct SwapParser;

impl SwapParser {
    pub fn new() -> Self {
        Self
    }

    /// Extract all swaps from a transaction by parsing inner instructions
    pub fn extract_swaps(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let Some(meta) = &tx.meta else {
            return Vec::new();
        };

        let OptionSerializer::Some(inner_instructions) = &meta.inner_instructions else {
            return Vec::new();
        };

        let token_map = self.build_token_map(tx);
        let mut swaps = Vec::new();

        for inner_set in inner_instructions {
            // Extract swaps with DEX attribution from inner instructions
            let inner_swaps = self.extract_swaps_from_inner_set(&inner_set.instructions, &token_map);
            swaps.extend(inner_swaps);
        }

        swaps
    }

    /// Extract swaps from an inner instruction set, tracking which program invoked each swap
    fn extract_swaps_from_inner_set(&self, instructions: &[UiInstruction], token_map: &HashMap<String, (String, u8)>) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();
        let mut transfers = Vec::new();
        let mut current_dex = String::new();

        for inst in instructions {
            let UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) = inst else {
                continue;
            };

            // Track non-token programs as potential DEXes
            if info.program != "spl-token" {
                current_dex = info.program.clone();
                continue;
            }

            // Extract token transfers
            let Some(obj) = info.parsed.as_object() else {
                continue;
            };

            let Some(typ) = obj.get("type").and_then(|v| v.as_str()) else {
                continue;
            };

            if typ != "transfer" && typ != "transferChecked" {
                continue;
            }

            let Some(info_obj) = obj.get("info").and_then(|v| v.as_object()) else {
                continue;
            };

            let account = info_obj.get("source")
                .or_else(|| info_obj.get("account"))
                .or_else(|| info_obj.get("destination"))
                .and_then(|v| v.as_str())
                .and_then(|s| token_map.get(s));

            let amount = info_obj.get("amount")
                .or_else(|| info_obj.get("tokenAmount").and_then(|v| v.get("amount")))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            if let Some((mint, decimals)) = account {
                if amount > 0 {
                    transfers.push((Transfer {
                        mint: mint.clone(),
                        amount,
                        decimals: *decimals,
                    }, current_dex.clone()));
                }
            }
        }

        // Pair consecutive transfers with different tokens as swaps
        for i in (0..transfers.len()).step_by(2) {
            if let (Some((t1, dex1)), Some((t2, _dex2))) = (transfers.get(i), transfers.get(i + 1)) {
                if t1.mint != t2.mint {
                    swaps.push(SwapInfo {
                        token0: t1.mint.clone(),
                        amount0: t1.amount as f64 / 10_f64.powi(t1.decimals as i32),
                        token1: t2.mint.clone(),
                        amount1: t2.amount as f64 / 10_f64.powi(t2.decimals as i32),
                        dex: dex1.clone(),
                        decimals0: t1.decimals,
                        decimals1: t2.decimals,
                    });
                }
            }
        }

        swaps
    }

    /// Get program ID for an instruction
    fn get_program_id(&self, tx: &FetchedTransaction, idx: usize, keys: &[String]) -> String {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return String::new();
        };

        match &ui_tx.message {
            UiMessage::Parsed(parsed) => {
                parsed.instructions.get(idx).map(|inst| match inst {
                    UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) => info.program.clone(),
                    UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(partial)) => partial.program_id.clone(),
                    UiInstruction::Compiled(c) => keys.get(c.program_id_index as usize).cloned().unwrap_or_default(),
                }).unwrap_or_default()
            }
            UiMessage::Raw(raw) => {
                raw.instructions.get(idx)
                    .and_then(|inst| keys.get(inst.program_id_index as usize))
                    .cloned()
                    .unwrap_or_default()
            }
        }
    }

    /// Build token account to mint/decimals map
    fn build_token_map(&self, tx: &FetchedTransaction) -> HashMap<String, (String, u8)> {
        let Some(meta) = &tx.meta else {
            return HashMap::new();
        };

        let keys = self.get_account_keys(tx);
        let mut map = HashMap::new();

        let balances = [&meta.pre_token_balances, &meta.post_token_balances];
        for balance_set in balances {
            if let OptionSerializer::Some(balances) = balance_set {
                for balance in balances {
                    if let Some(account) = keys.get(balance.account_index as usize) {
                        map.insert(
                            account.clone(),
                            (balance.mint.clone(), balance.ui_token_amount.decimals)
                        );
                    }
                }
            }
        }

        map
    }

    /// Get account keys from transaction
    fn get_account_keys(&self, tx: &FetchedTransaction) -> Vec<String> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return Vec::new();
        };

        match &ui_tx.message {
            UiMessage::Parsed(p) => p.account_keys.iter().map(|k| k.pubkey.clone()).collect(),
            UiMessage::Raw(r) => r.account_keys.clone(),
        }
    }

    /// Extract token transfers from inner instructions
    fn extract_transfers(&self, instructions: &[UiInstruction], token_map: &HashMap<String, (String, u8)>) -> Vec<Transfer> {
        let mut transfers = Vec::new();

        for inst in instructions {
            let UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) = inst else {
                continue;
            };

            if info.program != "spl-token" {
                continue;
            }

            let Some(obj) = info.parsed.as_object() else {
                continue;
            };

            let Some(typ) = obj.get("type").and_then(|v| v.as_str()) else {
                continue;
            };

            if typ != "transfer" && typ != "transferChecked" {
                continue;
            }

            let Some(info_obj) = obj.get("info").and_then(|v| v.as_object()) else {
                continue;
            };

            let account = info_obj.get("source")
                .or_else(|| info_obj.get("account"))
                .or_else(|| info_obj.get("destination"))
                .and_then(|v| v.as_str())
                .and_then(|s| token_map.get(s));

            let amount = info_obj.get("amount")
                .or_else(|| info_obj.get("tokenAmount").and_then(|v| v.get("amount")))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            if let Some((mint, decimals)) = account {
                if amount > 0 {
                    transfers.push(Transfer {
                        mint: mint.clone(),
                        amount,
                        decimals: *decimals,
                    });
                }
            }
        }

        transfers
    }

    /// Extract token balance changes
    pub fn extract_token_changes(&self, tx: &FetchedTransaction) -> Vec<TokenChange> {
        let Some(meta) = &tx.meta else {
            return Vec::new();
        };

        let (pre_balances, post_balances) = match (&meta.pre_token_balances, &meta.post_token_balances) {
            (OptionSerializer::Some(pre), OptionSerializer::Some(post)) => (pre.as_slice(), post.as_slice()),
            _ => return Vec::new(),
        };

        let mut pre_map: HashMap<usize, _> = HashMap::new();
        let mut post_map: HashMap<usize, _> = HashMap::new();

        for b in pre_balances {
            pre_map.insert(b.account_index as usize, b);
        }
        for b in post_balances {
            post_map.insert(b.account_index as usize, b);
        }

        pre_map.keys()
            .chain(post_map.keys())
            .filter_map(|&idx| {
                let (pre, post) = (pre_map.get(&idx)?, post_map.get(&idx)?);
                let pre_amt = pre.ui_token_amount.amount.parse().ok()?;
                let post_amt = post.ui_token_amount.amount.parse().ok()?;

                if pre_amt == post_amt {
                    return None;
                }

                let owner = match &post.owner {
                    OptionSerializer::Some(o) => o.clone(),
                    _ => String::new(),
                };

                Some(TokenChange {
                    account_index: idx,
                    mint: post.mint.clone(),
                    owner,
                    pre_amount: pre_amt,
                    post_amount: post_amt,
                    delta: post_amt as i64 - pre_amt as i64,
                    decimals: post.ui_token_amount.decimals,
                })
            })
            .collect()
    }

    /// Extract programs from transaction
    pub fn extract_dex_programs(&self, tx: &FetchedTransaction) -> Vec<String> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return Vec::new();
        };

        let mut programs: Vec<String> = match &ui_tx.message {
            UiMessage::Parsed(parsed) => {
                parsed.instructions.iter().filter_map(|inst| match inst {
                    UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) => Some(info.program.clone()),
                    UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(p)) => Some(p.program_id.clone()),
                    UiInstruction::Compiled(c) => {
                        parsed.account_keys.get(c.program_id_index as usize).map(|k| k.pubkey.clone())
                    }
                }).collect()
            }
            UiMessage::Raw(raw) => {
                raw.instructions.iter()
                    .filter_map(|inst| raw.account_keys.get(inst.program_id_index as usize).cloned())
                    .collect()
            }
        };

        programs.sort_unstable();
        programs.dedup();
        programs
    }
}

impl Default for SwapParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Token balance change
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
