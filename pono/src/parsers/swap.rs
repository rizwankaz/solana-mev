use crate::types::{FetchedTransaction, SwapInfo, TokenChange};
use solana_transaction_status::{
    EncodedTransaction, UiInstruction, UiMessage, UiParsedInstruction,
    option_serializer::OptionSerializer,
};
use std::cmp::min;
use std::collections::HashMap;

/// token transfer within inner instructions
#[derive(Debug)]
struct Transfer {
    mint: String,
    amount: u64,
    decimals: u8,
    source: String,
    destination: String,
}

/// swap parser
pub struct SwapParser;

impl SwapParser {
    pub fn new() -> Self {
        Self
    }

    /// extract all swaps from a transaction by parsing inner instructions
    pub fn extract_swaps(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let Some(meta) = &tx.meta else {
            return Vec::new();
        };

        let OptionSerializer::Some(inner_instructions) = &meta.inner_instructions else {
            return Vec::new();
        };

        let token_map = self.build_token_map(tx);
        let owner_map = self.build_owner_map(tx);
        let account_keys = self.get_account_keys(tx);
        let signer = tx.signer().unwrap_or_default();

        // get dex
        let outer_instructions = self.get_outer_instructions(tx);
        let mut swaps = Vec::new();

        for inner_set in inner_instructions {
            let outer_dex = outer_instructions
                .get(inner_set.index as usize)
                .cloned()
                .unwrap_or_default();

            let inner_swaps = self.extract_swaps_from_inner_set(
                &inner_set.instructions,
                &token_map,
                &owner_map,
                &account_keys,
                &signer,
                &outer_dex,
            );
            swaps.extend(inner_swaps);
        }

        swaps
    }

    fn extract_swaps_from_inner_set(
        &self,
        instructions: &[UiInstruction],
        token_map: &HashMap<String, (String, u8)>,
        owner_map: &HashMap<String, String>,
        account_keys: &[String],
        signer: &str,
        outer_dex: &str,
    ) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();
        let mut transfers = Vec::new();
        let mut current_dex = outer_dex.to_string();

        for inst in instructions {
            let program_id = self.get_instruction_program_id(inst, account_keys);
            let is_token_program = match inst {
                UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) => {
                    info.program == "spl-token"
                }
                _ => program_id == "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
            };

            let is_system_program = match inst {
                UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) => {
                    info.program == "system"
                }
                _ => program_id == "11111111111111111111111111111111",
            };

            if !is_token_program && !is_system_program && !program_id.is_empty() && !self.is_system_program(&program_id) {
                current_dex = program_id;
            }

            if !is_token_program && !is_system_program {
                continue;
            }

            if let UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) = inst {
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

                // Handle both token and SOL transfers
                let (mint, decimals, amount, source, destination) = if is_system_program {
                    // System program SOL transfer
                    let lamports = info_obj
                        .get("lamports")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);

                    let src = info_obj
                        .get("source")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let dst = info_obj
                        .get("destination")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    (
                        "So11111111111111111111111111111111111111112".to_string(),
                        9u8,
                        lamports,
                        src,
                        dst,
                    )
                } else {
                    // Token program transfer
                    let source = info_obj
                        .get("source")
                        .or_else(|| info_obj.get("account"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let destination = info_obj
                        .get("destination")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let token_info = info_obj
                        .get("source")
                        .or_else(|| info_obj.get("account"))
                        .or_else(|| info_obj.get("destination"))
                        .and_then(|v| v.as_str())
                        .and_then(|s| token_map.get(s));

                    let mint_from_instruction = info_obj
                        .get("mint")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let decimals_from_instruction = info_obj
                        .get("tokenAmount")
                        .and_then(|v| v.get("decimals"))
                        .and_then(|v| v.as_u64())
                        .map(|d| d as u8);

                    let amount = info_obj
                        .get("amount")
                        .or_else(|| info_obj.get("tokenAmount").and_then(|v| v.get("amount")))
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);

                    let (mint, decimals) = match token_info {
                        Some((m, d)) => (m.clone(), *d),
                        None => match (mint_from_instruction, decimals_from_instruction) {
                            (Some(m), Some(d)) => (m, d),
                            _ => continue,
                        },
                    };

                    (mint, decimals, amount, source, destination)
                };

                if amount > 0 {
                    transfers.push((
                        Transfer {
                            mint,
                            amount,
                            decimals,
                            source: source.clone(),
                            destination: destination.clone(),
                        },
                        current_dex.clone(),
                    ));
                }
            }
        }

        tracing::debug!(
            "Total transfers detected: {}, signer: {}",
            transfers.len(),
            signer
        );
        for (idx, (t, dex)) in transfers.iter().enumerate() {
            let src_owner = owner_map.get(&t.source).map(|s| s.as_str()).unwrap_or("?");
            let dst_owner = owner_map.get(&t.destination).map(|s| s.as_str()).unwrap_or("?");
            tracing::debug!(
                "Transfer {}: {} {} from {} to {} (src_owner={}, dst_owner={}) via dex: {}",
                idx,
                t.amount,
                &t.mint[..min(12, t.mint.len())],
                &t.source[..min(8, t.source.len())],
                &t.destination[..min(8, t.destination.len())],
                &src_owner[..min(8, src_owner.len())],
                &dst_owner[..min(8, dst_owner.len())],
                &dex[..min(12, dex.len())]
            );
        }

        // Separate signer's outgoing and incoming transfers
        let mut outgoing: Vec<(usize, &Transfer, &String)> = Vec::new();
        let mut incoming: Vec<(usize, &Transfer, &String)> = Vec::new();

        for (idx, (t, dex)) in transfers.iter().enumerate() {
            let src_owner = owner_map.get(&t.source).map(|s| s.as_str());
            let dst_owner = owner_map.get(&t.destination).map(|s| s.as_str());

            if src_owner == Some(signer) {
                outgoing.push((idx, t, dex));
            }
            if dst_owner == Some(signer) {
                incoming.push((idx, t, dex));
            }
        }

        tracing::debug!(
            "Signer transfers: {} outgoing, {} incoming",
            outgoing.len(),
            incoming.len()
        );

        // Match outgoing with incoming transfers to form swaps
        let mut used_incoming = vec![false; incoming.len()];

        for (out_idx, out_transfer, out_dex) in &outgoing {
            // Find the nearest unused incoming transfer with a different mint
            if let Some((inc_pos, (in_idx, in_transfer, _in_dex))) = incoming
                .iter()
                .enumerate()
                .filter(|(pos, _)| !used_incoming[*pos])
                .filter(|(_, (_, t, _))| t.mint != out_transfer.mint)
                .min_by_key(|(_, (idx, _, _))| {
                    if idx > out_idx {
                        idx - out_idx
                    } else {
                        out_idx - idx
                    }
                })
            {
                used_incoming[inc_pos] = true;

                tracing::debug!(
                    "Matched swap: outgoing[{}] {} {} -> incoming[{}] {} {}",
                    out_idx,
                    out_transfer.amount,
                    &out_transfer.mint[..8],
                    in_idx,
                    in_transfer.amount,
                    &in_transfer.mint[..8]
                );

                let amt0_f = out_transfer.amount as f64 / 10_f64.powi(out_transfer.decimals as i32);
                let amt1_f = in_transfer.amount as f64 / 10_f64.powi(in_transfer.decimals as i32);

                swaps.push(SwapInfo {
                    token0: out_transfer.mint.clone(),
                    amount0: amt0_f,
                    token1: in_transfer.mint.clone(),
                    amount1: amt1_f,
                    dex: (*out_dex).clone(),
                    decimals0: out_transfer.decimals,
                    decimals1: in_transfer.decimals,
                });
            }
        }

        swaps
    }

    fn get_instruction_program_id(&self, inst: &UiInstruction, account_keys: &[String]) -> String {
        match inst {
            UiInstruction::Parsed(UiParsedInstruction::Parsed(_info)) => String::new(),
            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(partial)) => {
                partial.program_id.clone()
            }
            UiInstruction::Compiled(compiled) => account_keys
                .get(compiled.program_id_index as usize)
                .cloned()
                .unwrap_or_default(),
        }
    }

    /// no system programs lol
    fn is_system_program(&self, program_id: &str) -> bool {
        matches!(
            program_id,
            "11111111111111111111111111111111"
                | "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
                | "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"
                | "ComputeBudget111111111111111111111111111111"
        )
    }

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
                            (balance.mint.clone(), balance.ui_token_amount.decimals),
                        );
                    }
                }
            }
        }

        map
    }

    fn build_owner_map(&self, tx: &FetchedTransaction) -> HashMap<String, String> {
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
                        if let OptionSerializer::Some(owner) = &balance.owner {
                            map.insert(account.clone(), owner.clone());
                        }
                    }
                }
            }
        }

        map
    }

    fn get_account_keys(&self, tx: &FetchedTransaction) -> Vec<String> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return Vec::new();
        };

        match &ui_tx.message {
            UiMessage::Parsed(p) => p.account_keys.iter().map(|k| k.pubkey.clone()).collect(),
            UiMessage::Raw(r) => r.account_keys.clone(),
        }
    }

    fn get_outer_instructions(&self, tx: &FetchedTransaction) -> Vec<String> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return Vec::new();
        };

        match &ui_tx.message {
            UiMessage::Parsed(parsed) => parsed
                .instructions
                .iter()
                .map(|inst| match inst {
                    UiInstruction::Parsed(UiParsedInstruction::Parsed(_)) => String::new(),
                    UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(partial)) => {
                        partial.program_id.clone()
                    }
                    UiInstruction::Compiled(compiled) => parsed
                        .account_keys
                        .get(compiled.program_id_index as usize)
                        .map(|k| k.pubkey.clone())
                        .unwrap_or_default(),
                })
                .collect(),
            UiMessage::Raw(raw) => raw
                .instructions
                .iter()
                .map(|inst| {
                    raw.account_keys
                        .get(inst.program_id_index as usize)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect(),
        }
    }

    pub fn extract_token_changes(&self, tx: &FetchedTransaction) -> Vec<TokenChange> {
        let Some(meta) = &tx.meta else {
            return Vec::new();
        };

        let (pre_balances, post_balances) =
            match (&meta.pre_token_balances, &meta.post_token_balances) {
                (OptionSerializer::Some(pre), OptionSerializer::Some(post)) => {
                    (pre.as_slice(), post.as_slice())
                }
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

        post_map
            .keys()
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

    pub fn extract_dex_programs(&self, tx: &FetchedTransaction) -> Vec<String> {
        let EncodedTransaction::Json(ui_tx) = &tx.transaction else {
            return Vec::new();
        };

        let mut programs: Vec<String> = match &ui_tx.message {
            UiMessage::Parsed(parsed) => parsed
                .instructions
                .iter()
                .filter_map(|inst| match inst {
                    UiInstruction::Parsed(UiParsedInstruction::Parsed(info)) => {
                        Some(info.program.clone())
                    }
                    UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(p)) => {
                        Some(p.program_id.clone())
                    }
                    UiInstruction::Compiled(c) => parsed
                        .account_keys
                        .get(c.program_id_index as usize)
                        .map(|k| k.pubkey.clone()),
                })
                .collect(),
            UiMessage::Raw(raw) => raw
                .instructions
                .iter()
                .filter_map(|inst| {
                    raw.account_keys
                        .get(inst.program_id_index as usize)
                        .cloned()
                })
                .collect(),
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
// i'm pretty sure this has a lot to be improved but idk how rn
