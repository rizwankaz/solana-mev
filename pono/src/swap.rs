use serde::{Deserialize, Serialize};
use crate::types::FetchedTransaction;
use std::collections::HashMap;

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

/// Known DEX program IDs
const KNOWN_DEXES: &[(&str, &str)] = &[
    ("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4", "Jupiter"),
    ("JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB", "Jupiter v4"),
    ("JUP2jxvXaqu7NQY1GmNF4m1vodw12LVXYxbFL2uJvfo", "Jupiter v2"),
    ("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8", "Raydium AMM"),
    ("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", "Raydium CLMM"),
    ("27haf8L6oxUeXrHrgEgsexjSY5hbVUWEmvv9Nyzbug", "Raydium CPMM"),
    ("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc", "Whirlpool"),
    ("9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP", "Orca v2"),
    ("DjVE6JNiYqPL2QXyCUUh8rNjHrbz9hXHNYt99MQ59qw1", "Orca v1"),
    ("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P", "Pump.fun"),
    ("PSwapMdSai8tjrEXcxFeQth87xC4rRsa4VA5mhGhXkP", "PancakeSwap"),
    ("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", "Meteora"),
    ("Dooar9JkhdZ7J3LHN3A7YCuoGRUggXhQaG4kijfLGU2j", "Meteora DLMM"),
    ("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY", "Phoenix"),
    ("EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S", "Lifinity v2"),
    ("2wT8Yq49kHgDzXuPxZSaeLaH1qbmGXtEyPy64bL7aD3c", "Lifinity v1"),
    ("srmqPvymJeFKQ4zGQed1GFppgkRHL9kaELCbyksJtPX", "Serum"),
    ("9xQeWvG816bUx9EPjHmaT23yvVM2ZWbrrpZb9PusVFin", "Serum v3"),
];

/// Swap parser for extracting swap details from transactions
pub struct SwapParser {
    dex_map: HashMap<String, String>,
}

impl SwapParser {
    pub fn new() -> Self {
        let mut dex_map = HashMap::new();
        for (addr, name) in KNOWN_DEXES {
            dex_map.insert(addr.to_string(), name.to_string());
        }
        Self { dex_map }
    }

    /// Get human-readable DEX name from program ID
    fn get_dex_name(&self, program_id: &str) -> String {
        self.dex_map.get(program_id)
            .cloned()
            .unwrap_or_else(|| program_id.to_string())
    }

    /// Extract all swaps from a transaction by parsing inner instructions
    pub fn extract_swaps(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        // Build a map of token accounts to mints and decimals
        let token_account_map = self.build_token_account_map(tx);

        // Parse inner instructions to find token transfers
        use solana_transaction_status::option_serializer::OptionSerializer;
        if let Some(meta) = &tx.meta {
            if let OptionSerializer::Some(inner_instructions) = &meta.inner_instructions {
                for inner_set in inner_instructions {
                    // Group transfers by instruction index to find swap patterns
                    let transfers = self.extract_transfers_from_inner(&inner_set.instructions, &token_account_map, tx);

                    // Analyze transfer patterns to identify swaps
                    let instruction_swaps = self.identify_swaps_from_transfers(&transfers, tx);
                    swaps.extend(instruction_swaps);
                }
            }
        }

        // If no swaps found from inner instructions, fall back to analyzing token balance changes
        if swaps.is_empty() {
            swaps = self.extract_swaps_from_balance_changes(tx);
        }

        swaps
    }

    /// Build a map of token account addresses to their mint and decimals
    fn build_token_account_map(&self, tx: &FetchedTransaction) -> HashMap<String, (String, u8)> {
        use solana_transaction_status::option_serializer::OptionSerializer;
        let mut map = HashMap::new();

        if let Some(meta) = &tx.meta {
            // Get account keys from transaction
            let account_keys = self.get_account_keys(tx);

            // Map from pre_token_balances
            if let OptionSerializer::Some(pre_balances) = &meta.pre_token_balances {
                for balance in pre_balances {
                    let idx = balance.account_index as usize;
                    if let Some(account) = account_keys.get(idx) {
                        map.insert(
                            account.clone(),
                            (balance.mint.clone(), balance.ui_token_amount.decimals)
                        );
                    }
                }
            }

            // Map from post_token_balances (may have additional accounts)
            if let OptionSerializer::Some(post_balances) = &meta.post_token_balances {
                for balance in post_balances {
                    let idx = balance.account_index as usize;
                    if let Some(account) = account_keys.get(idx) {
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
        use solana_transaction_status::{EncodedTransaction, UiMessage};

        match &tx.transaction {
            EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter()
                            .map(|key| key.pubkey.clone())
                            .collect()
                    }
                    UiMessage::Raw(raw) => raw.account_keys.clone(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// Extract token transfers from inner instructions
    fn extract_transfers_from_inner(
        &self,
        instructions: &[solana_transaction_status::UiInstruction],
        token_map: &HashMap<String, (String, u8)>,
        _tx: &FetchedTransaction,
    ) -> Vec<TokenTransfer> {
        use solana_transaction_status::{UiInstruction, UiParsedInstruction};

        let mut transfers = Vec::new();

        for inst in instructions {
            match inst {
                UiInstruction::Parsed(parsed_inst) => {
                    match parsed_inst {
                        UiParsedInstruction::Parsed(info) => {
                            // Look for token transfer instructions
                            if info.program == "spl-token" {
                                if let Some(parsed_info) = info.parsed.as_object() {
                                    if let Some(instruction_type) = parsed_info.get("type").and_then(|v| v.as_str()) {
                                        if instruction_type == "transfer" || instruction_type == "transferChecked" {
                                            if let Some(transfer_info) = parsed_info.get("info").and_then(|v| v.as_object()) {
                                                let source = transfer_info.get("source")
                                                    .or_else(|| transfer_info.get("account"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();

                                                let destination = transfer_info.get("destination")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string();

                                                let amount = transfer_info.get("amount")
                                                    .or_else(|| transfer_info.get("tokenAmount").and_then(|v| v.get("amount")))
                                                    .and_then(|v| v.as_str())
                                                    .and_then(|s| s.parse::<u64>().ok())
                                                    .unwrap_or(0);

                                                if amount > 0 && !source.is_empty() && !destination.is_empty() {
                                                    if let Some((mint, decimals)) = token_map.get(&source)
                                                        .or_else(|| token_map.get(&destination)) {
                                                        transfers.push(TokenTransfer {
                                                            source,
                                                            destination,
                                                            mint: mint.clone(),
                                                            amount,
                                                            decimals: *decimals,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        UiParsedInstruction::PartiallyDecoded(_) => {
                            // Skip partially decoded for now
                        }
                    }
                }
                UiInstruction::Compiled(compiled) => {
                    // For compiled instructions, we'd need to decode the instruction data
                    // This is DEX-specific and complex, so we'll rely on token transfers
                }
            }
        }

        transfers
    }

    /// Identify swaps from token transfer patterns
    fn identify_swaps_from_transfers(
        &self,
        transfers: &[TokenTransfer],
        tx: &FetchedTransaction,
    ) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        if transfers.len() < 2 {
            return swaps;
        }

        // Get DEX programs used
        let dex_programs = self.extract_dex_programs(tx);
        let dex_name = if !dex_programs.is_empty() {
            self.get_dex_name(&dex_programs[0])
        } else {
            "Unknown".to_string()
        };

        // Group consecutive transfers that form swaps
        // A swap typically consists of: user sends token A, user receives token B
        let mut i = 0;
        while i < transfers.len() {
            // Look for pairs of transfers with different mints
            if i + 1 < transfers.len() {
                let t1 = &transfers[i];
                let t2 = &transfers[i + 1];

                // Check if these could be a swap (different tokens)
                if t1.mint != t2.mint {
                    swaps.push(SwapInfo {
                        token0: t1.mint.clone(),
                        amount0: t1.amount as f64 / 10_f64.powi(t1.decimals as i32),
                        token1: t2.mint.clone(),
                        amount1: t2.amount as f64 / 10_f64.powi(t2.decimals as i32),
                        dex: dex_name.clone(),
                        decimals0: t1.decimals,
                        decimals1: t2.decimals,
                    });
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }

        swaps
    }

    /// Fallback: Extract swaps from token balance changes
    fn extract_swaps_from_balance_changes(&self, tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();
        let token_changes = self.extract_token_changes(tx);
        let programs = self.extract_dex_programs(tx);

        if programs.is_empty() || token_changes.is_empty() {
            return swaps;
        }

        let dex_name = self.get_dex_name(&programs[0]);

        // Group by owner
        let mut owner_changes: HashMap<String, (Vec<&TokenChange>, Vec<&TokenChange>)> = HashMap::new();

        for change in &token_changes {
            let entry = owner_changes.entry(change.owner.clone()).or_insert((Vec::new(), Vec::new()));
            if change.delta < 0 {
                entry.0.push(change);
            } else if change.delta > 0 {
                entry.1.push(change);
            }
        }

        // For each owner with both negative and positive changes, create swaps
        for (_owner, (negative_changes, positive_changes)) in owner_changes {
            if negative_changes.is_empty() || positive_changes.is_empty() {
                continue;
            }

            // Sort by absolute amount to match largest trades first
            let mut neg_sorted = negative_changes.clone();
            neg_sorted.sort_by(|a, b| b.delta.abs().cmp(&a.delta.abs()));

            let mut pos_sorted = positive_changes.clone();
            pos_sorted.sort_by(|a, b| b.delta.cmp(&a.delta));

            // Match pairs
            let pairs = neg_sorted.len().min(pos_sorted.len());
            for i in 0..pairs {
                let from_change = neg_sorted[i];
                let to_change = pos_sorted[i];

                if from_change.mint != to_change.mint {
                    swaps.push(SwapInfo {
                        token0: from_change.mint.clone(),
                        amount0: from_change.delta.abs() as f64 / 10_f64.powi(from_change.decimals as i32),
                        token1: to_change.mint.clone(),
                        amount1: to_change.delta as f64 / 10_f64.powi(to_change.decimals as i32),
                        dex: dex_name.clone(),
                        decimals0: from_change.decimals,
                        decimals1: to_change.decimals,
                    });
                }
            }
        }

        swaps
    }

    /// Extract token balance changes
    pub fn extract_token_changes(&self, tx: &FetchedTransaction) -> Vec<TokenChange> {
        use solana_transaction_status::option_serializer::OptionSerializer;
        let mut changes = Vec::new();

        if let Some(meta) = &tx.meta {
            let pre_balances = match &meta.pre_token_balances {
                OptionSerializer::Some(v) => v.as_slice(),
                _ => &[],
            };
            let post_balances = match &meta.post_token_balances {
                OptionSerializer::Some(v) => v.as_slice(),
                _ => &[],
            };

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

/// Represents a token transfer found in inner instructions
#[derive(Debug, Clone)]
struct TokenTransfer {
    source: String,
    destination: String,
    mint: String,
    amount: u64,
    decimals: u8,
}

/// Represents a token balance change
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
