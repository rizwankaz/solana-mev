use std::collections::HashMap;
use solana_transaction_status::{UiInstruction, UiParsedInstruction, UiTransactionTokenBalance};
use serde::Serialize;

/// Token balance change for a specific mint
#[derive(Debug, Clone, Serialize)]
pub struct TokenChange {
    pub mint: String,
    pub ui_amount_change: f64,
    pub decimals: u8,
}

/// MEV event categories
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MevCategory {
    /// Cross-DEX arbitrage opportunities
    Arbitrage,
    /// Liquidations on lending protocols
    Liquidation,
    /// Token or NFT mints
    Mint,
    /// Failed MEV attempts (spam)
    Spam,
}

impl MevCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            MevCategory::Arbitrage => "arbitrage",
            MevCategory::Liquidation => "liquidation",
            MevCategory::Mint => "mint",
            MevCategory::Spam => "spam",
        }
    }
}

/// Individual MEV event detected in a transaction
#[derive(Debug, Clone, Serialize)]
pub struct MevEvent {
    pub category: MevCategory,
    pub signature: String,
    pub programs_involved: Vec<String>,
    pub token_changes: Vec<TokenChange>,
    pub sol_change_lamports: i64,
    pub success: bool,
}

/// Aggregated MEV statistics for a block
#[derive(Debug, Clone, Default)]
pub struct MevSummary {
    /// Count of arbitrage transactions
    pub arbitrage_count: usize,
    /// Token profits from arbitrage (mint -> total profit)
    pub arbitrage_token_profits: HashMap<String, f64>,

    /// Count of liquidation transactions
    pub liquidation_count: usize,
    /// Token profits from liquidations (mint -> total profit)
    pub liquidation_token_profits: HashMap<String, f64>,

    /// Count of mint transactions
    pub mint_count: usize,
    /// New tokens minted (mint -> total amount)
    pub minted_tokens: HashMap<String, f64>,

    /// Count of spam/failed MEV attempts
    pub spam_count: usize,

    /// Programs used, with frequency count
    pub programs_used: HashMap<String, usize>,

    /// Total SOL change across all MEV (can be negative due to fees)
    pub total_sol_change: i64,
}

impl MevSummary {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an MEV event to the summary
    pub fn add_event(&mut self, event: &MevEvent) {
        // Update counts and profits based on category
        match event.category {
            MevCategory::Arbitrage => {
                if event.success {
                    self.arbitrage_count += 1;
                    // Track token profits
                    for token_change in &event.token_changes {
                        if token_change.ui_amount_change > 0.0 {
                            *self.arbitrage_token_profits
                                .entry(token_change.mint.clone())
                                .or_insert(0.0) += token_change.ui_amount_change;
                        }
                    }
                    self.total_sol_change += event.sol_change_lamports;
                } else {
                    self.spam_count += 1;
                }
            }
            MevCategory::Liquidation => {
                if event.success {
                    self.liquidation_count += 1;
                    // Track token profits from liquidations
                    for token_change in &event.token_changes {
                        if token_change.ui_amount_change > 0.0 {
                            *self.liquidation_token_profits
                                .entry(token_change.mint.clone())
                                .or_insert(0.0) += token_change.ui_amount_change;
                        }
                    }
                    self.total_sol_change += event.sol_change_lamports;
                } else {
                    self.spam_count += 1;
                }
            }
            MevCategory::Mint => {
                if event.success {
                    self.mint_count += 1;
                    // Track new tokens minted
                    for token_change in &event.token_changes {
                        if token_change.ui_amount_change > 0.0 {
                            *self.minted_tokens
                                .entry(token_change.mint.clone())
                                .or_insert(0.0) += token_change.ui_amount_change;
                        }
                    }
                } else {
                    self.spam_count += 1;
                }
            }
            MevCategory::Spam => {
                self.spam_count += 1;
            }
        }

        // Track program usage
        for program in &event.programs_involved {
            *self.programs_used.entry(program.clone()).or_insert(0) += 1;
        }
    }

    /// Total MEV transactions (excluding spam)
    pub fn total_mev_count(&self) -> usize {
        self.arbitrage_count + self.liquidation_count + self.mint_count
    }

    /// Get top token profits across all MEV categories
    pub fn top_token_profits(&self, limit: usize) -> Vec<(String, f64)> {
        let mut all_profits: HashMap<String, f64> = HashMap::new();

        // Combine all token profits
        for (mint, profit) in &self.arbitrage_token_profits {
            *all_profits.entry(mint.clone()).or_insert(0.0) += profit;
        }
        for (mint, profit) in &self.liquidation_token_profits {
            *all_profits.entry(mint.clone()).or_insert(0.0) += profit;
        }

        // Sort by profit descending
        let mut profits: Vec<_> = all_profits.into_iter().collect();
        profits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        profits.into_iter().take(limit).collect()
    }
}

/// Known program IDs for MEV detection
pub struct ProgramRegistry;

impl ProgramRegistry {
    // DEX Programs (for arbitrage detection)
    pub const JUPITER_V6: &'static str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";
    pub const JUPITER_LIMIT_ORDER: &'static str = "jupoNjAxXgZ4rjzxzPMP4oxduvQsQtZzyknqvzYNrNu";
    pub const RAYDIUM_AMM_V4: &'static str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
    pub const RAYDIUM_CPMM: &'static str = "CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C";
    pub const RAYDIUM_CLMM: &'static str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";
    pub const ORCA_WHIRLPOOL: &'static str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";
    pub const PHOENIX: &'static str = "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY";
    pub const METEORA_DAMM_V2: &'static str = "cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG";
    pub const METEORA_DLMM: &'static str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";
    pub const LIFINITY_V2: &'static str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S";
    // Anchor-based DEXs
    pub const OPENBOOK_V2: &'static str = "opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb"; // Anchor
    pub const DRIFT_PROTOCOL: &'static str = "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH"; // Anchor
    pub const SABER: &'static str = "SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ";
    pub const MARINADE_FINANCE: &'static str = "MarBmsSgKXdrN1egZf5sqe1TMai9K1rChYNDJgjq7aD";
    pub const SANCTUM: &'static str = "5ocnV1qiCgaQR8Jb8xWnVbApfaygJ8tNoZfgPwsgx9kx";
    pub const PUMP_FUN: &'static str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"; // Popular token swap AMM

    // Lending/Liquidation Programs
    pub const MARGINFI_V2: &'static str = "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA"; // Anchor
    pub const SOLEND: &'static str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";
    pub const KAMINO_LEND: &'static str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD"; // Anchor
    pub const MANGO_V4: &'static str = "4MangoMjqJ2firMokCjjGgoK8d4MXcrgL7XJaL3w6fVg"; // Anchor

    // Token/NFT Programs
    pub const TOKEN_PROGRAM: &'static str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
    pub const TOKEN_2022_PROGRAM: &'static str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";
    pub const METAPLEX_TOKEN_METADATA: &'static str = "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s";
    pub const METAPLEX_CORE: &'static str = "CoREENxT6tW1HoK8ypY1SxRMZTcVPm7R94rH4PZNhX7d";

    /// Check if a program is a DEX
    pub fn is_dex(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::JUPITER_V6
                | Self::JUPITER_LIMIT_ORDER
                | Self::RAYDIUM_AMM_V4
                | Self::RAYDIUM_CPMM
                | Self::RAYDIUM_CLMM
                | Self::ORCA_WHIRLPOOL
                | Self::PHOENIX
                | Self::METEORA_DAMM_V2
                | Self::METEORA_DLMM
                | Self::LIFINITY_V2
                | Self::OPENBOOK_V2
                | Self::DRIFT_PROTOCOL
                | Self::SABER
                | Self::MARINADE_FINANCE
                | Self::SANCTUM
                | Self::PUMP_FUN
        )
    }

    /// Check if a program is a lending protocol
    pub fn is_lending(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::MARGINFI_V2 | Self::SOLEND | Self::KAMINO_LEND | Self::MANGO_V4
        )
    }

    /// Check if a program is related to token/NFT creation
    pub fn is_mint_program(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::TOKEN_PROGRAM
                | Self::TOKEN_2022_PROGRAM
                | Self::METAPLEX_TOKEN_METADATA
                | Self::METAPLEX_CORE
        )
    }

    /// Get a human-readable name for a known program
    pub fn program_name(program_id: &str) -> String {
        match program_id {
            Self::JUPITER_V6 => "Jupiter V6".to_string(),
            Self::JUPITER_LIMIT_ORDER => "Jupiter Limit Order".to_string(),
            Self::RAYDIUM_AMM_V4 => "Raydium AMM V4".to_string(),
            Self::RAYDIUM_CPMM => "Raydium CPMM".to_string(),
            Self::RAYDIUM_CLMM => "Raydium CLMM".to_string(),
            Self::ORCA_WHIRLPOOL => "Orca Whirlpool".to_string(),
            Self::PHOENIX => "Phoenix".to_string(),
            Self::METEORA_DAMM_V2 => "Meteora DAMM V2".to_string(),
            Self::METEORA_DLMM => "Meteora DLMM".to_string(),
            Self::LIFINITY_V2 => "Lifinity V2".to_string(),
            Self::OPENBOOK_V2 => "OpenBook V2".to_string(),
            Self::DRIFT_PROTOCOL => "Drift Protocol".to_string(),
            Self::SABER => "Saber".to_string(),
            Self::MARINADE_FINANCE => "Marinade Finance".to_string(),
            Self::SANCTUM => "Sanctum".to_string(),
            Self::MARGINFI_V2 => "MarginFi V2".to_string(),
            Self::SOLEND => "Solend".to_string(),
            Self::KAMINO_LEND => "Kamino Lend".to_string(),
            Self::MANGO_V4 => "Mango V4".to_string(),
            Self::TOKEN_PROGRAM => "Token Program".to_string(),
            Self::TOKEN_2022_PROGRAM => "Token-2022".to_string(),
            Self::METAPLEX_TOKEN_METADATA => "Metaplex Metadata".to_string(),
            Self::METAPLEX_CORE => "Metaplex Core".to_string(),
            Self::PUMP_FUN => "Pump.fun AMM".to_string(),
            _ => {
                // Truncate unknown programs for readability
                if program_id.len() > 10 {
                    format!("{}...{}", &program_id[..6], &program_id[program_id.len()-4..])
                } else {
                    program_id.to_string()
                }
            }
        }
    }
}

/// MEV analyzer for detecting MEV patterns in transactions
pub struct MevAnalyzer;

impl MevAnalyzer {
    /// Analyze a transaction and detect MEV patterns
    ///
    /// Hybrid Classification Heuristics:
    /// 1. Token Balance Changes (primary signal):
    ///    - Swap pattern: Both positive & negative token changes
    ///    - Mint pattern: Only positive token changes
    /// 2. Known Program IDs (hints):
    ///    - Known DEX programs → ARBITRAGE
    ///    - Lending protocols → LIQUIDATION
    ///    - Token/NFT programs → MINT (if no swap pattern)
    /// 3. Unknown Programs:
    ///    - With swap pattern → ARBITRAGE (catches new DEXs)
    ///    - With mint pattern → MINT
    ///    - Failed → SPAM
    pub fn analyze_transaction(
        signature: &str,
        instructions: &[UiInstruction],
        success: bool,
        pre_balances: &[u64],
        post_balances: &[u64],
        pre_token_balances: &[UiTransactionTokenBalance],
        post_token_balances: &[UiTransactionTokenBalance],
    ) -> Option<MevEvent> {
        let program_ids = Self::extract_program_ids(instructions);

        // Skip if no programs at all
        if program_ids.is_empty() {
            return None;
        }

        // Calculate token changes first (needed for classification)
        let token_changes = Self::calculate_token_changes(pre_token_balances, post_token_balances);

        // Detect category based on program interactions AND token changes
        let category = Self::detect_category(&program_ids, &token_changes);

        // Calculate SOL balance change (signed)
        let sol_change_lamports = Self::calculate_sol_change(pre_balances, post_balances);

        Some(MevEvent {
            category,
            signature: signature.to_string(),
            programs_involved: program_ids,
            token_changes,
            sol_change_lamports,
            success,
        })
    }

    /// Extract program IDs from instructions
    ///
    /// Returns ALL programs involved, not just known ones.
    /// This allows us to detect new/unknown DEX programs and protocols.
    fn extract_program_ids(instructions: &[UiInstruction]) -> Vec<String> {
        let mut programs = Vec::new();

        for ix in instructions {
            let program_id = match ix {
                UiInstruction::Parsed(parsed) => {
                    match parsed {
                        UiParsedInstruction::Parsed(parsed_ix) => Some(parsed_ix.program_id.clone()),
                        UiParsedInstruction::PartiallyDecoded(partial) => Some(partial.program_id.clone()),
                    }
                }
                UiInstruction::Compiled(_) => {
                    // For compiled instructions, we would need the account keys
                    // from the transaction message to resolve program IDs
                    None
                }
            };

            if let Some(program_str) = program_id {
                // Include ALL programs, not just known ones
                // This lets us detect swaps on new/unknown DEX programs
                if !programs.contains(&program_str) {
                    programs.push(program_str);
                }
            }
        }

        programs
    }

    /// Detect MEV category based on program interactions AND token balance changes
    ///
    /// Uses hybrid approach:
    /// 1. Token changes indicate swap-like behavior (multiple tokens changing)
    /// 2. Known programs provide hints (DEX, lending, etc.)
    /// 3. Unknown programs with swap patterns are classified as ARBITRAGE
    fn detect_category(program_ids: &[String], token_changes: &[TokenChange]) -> MevCategory {
        let dex_count = program_ids.iter().filter(|p| ProgramRegistry::is_dex(p)).count();
        let lending_count = program_ids.iter().filter(|p| ProgramRegistry::is_lending(p)).count();
        let mint_count = program_ids.iter().filter(|p| ProgramRegistry::is_mint_program(p)).count();

        // Check token change patterns
        let has_token_swap = token_changes.len() >= 2 ||
            (token_changes.len() == 1 && token_changes[0].ui_amount_change.abs() > 0.0);

        let has_only_positive_changes = !token_changes.is_empty() &&
            token_changes.iter().all(|tc| tc.ui_amount_change > 0.0);

        // Arbitrage: Multiple DEX interactions (cross-DEX trades)
        if dex_count >= 2 {
            return MevCategory::Arbitrage;
        }

        // Liquidation: Lending protocol + DEX (liquidator selling collateral)
        if lending_count > 0 && dex_count > 0 {
            return MevCategory::Liquidation;
        }

        // Pure lending interactions (could be liquidations)
        if lending_count > 0 {
            return MevCategory::Liquidation;
        }

        // Known DEX interaction -> ARBITRAGE
        if dex_count > 0 {
            return MevCategory::Arbitrage;
        }

        // Unknown program with swap-like token changes -> likely a DEX swap -> ARBITRAGE
        // This catches new/unknown DEX programs like Meteora Dynamic Bonding Curve
        if has_token_swap && !has_only_positive_changes {
            // Has token swaps (both positive and negative changes) but no known DEX
            // Likely an unknown DEX program
            return MevCategory::Arbitrage;
        }

        // Mints: Token/NFT programs with only positive token changes
        // OR token programs without swap-like behavior
        if mint_count > 0 || has_only_positive_changes {
            return MevCategory::Mint;
        }

        // Default to spam if we can't classify
        MevCategory::Spam
    }

    /// Calculate token balance changes from pre/post token balances
    fn calculate_token_changes(
        pre_token_balances: &[UiTransactionTokenBalance],
        post_token_balances: &[UiTransactionTokenBalance],
    ) -> Vec<TokenChange> {
        let mut changes = Vec::new();
        let mut token_map: HashMap<(u8, String), (Option<f64>, Option<f64>, u8)> = HashMap::new();

        // Collect pre-balances
        for pre_balance in pre_token_balances {
            let key = (pre_balance.account_index, pre_balance.mint.clone());
            let entry = token_map.entry(key).or_insert((None, None, pre_balance.ui_token_amount.decimals));
            entry.0 = pre_balance.ui_token_amount.ui_amount;
            entry.2 = pre_balance.ui_token_amount.decimals;
        }

        // Collect post-balances
        for post_balance in post_token_balances {
            let key = (post_balance.account_index, post_balance.mint.clone());
            let entry = token_map.entry(key).or_insert((None, None, post_balance.ui_token_amount.decimals));
            entry.1 = post_balance.ui_token_amount.ui_amount;
            entry.2 = post_balance.ui_token_amount.decimals;
        }

        // Calculate changes for each mint across all accounts
        let mut mint_totals: HashMap<String, (f64, u8)> = HashMap::new();

        for ((_, mint), (pre_opt, post_opt, decimals)) in token_map {
            let pre = pre_opt.unwrap_or(0.0);
            let post = post_opt.unwrap_or(0.0);
            let change = post - pre;

            let entry = mint_totals.entry(mint).or_insert((0.0, decimals));
            entry.0 += change;
        }

        // Convert to TokenChange structs
        for (mint, (total_change, decimals)) in mint_totals {
            // Only include non-zero changes
            if total_change.abs() > 0.0000001 {
                changes.push(TokenChange {
                    mint,
                    ui_amount_change: total_change,
                    decimals,
                });
            }
        }

        changes
    }

    /// Calculate SOL balance change (signed, in lamports)
    fn calculate_sol_change(pre_balances: &[u64], post_balances: &[u64]) -> i64 {
        if pre_balances.is_empty() || post_balances.is_empty() {
            return 0;
        }

        // Sum all account balance changes
        let total_pre: i64 = pre_balances.iter().map(|&b| b as i64).sum();
        let total_post: i64 = post_balances.iter().map(|&b| b as i64).sum();

        total_post - total_pre
    }
}
