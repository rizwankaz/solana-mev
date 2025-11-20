use std::collections::HashMap;
use solana_transaction_status::{UiInstruction, UiParsedInstruction, UiTransactionTokenBalance};
use serde::Serialize;

/// Individual swap within an arbitrage
#[derive(Debug, Clone, Serialize)]
pub struct Swap {
    /// Token being sold/input
    pub from_token: String,
    /// Amount of from_token
    pub from_amount: f64,
    /// Token being bought/output
    pub to_token: String,
    /// Amount of to_token
    pub to_amount: f64,
    /// DEX program used for this swap
    pub dex_program: String,
    /// Decimals for from_token
    pub from_decimals: u8,
    /// Decimals for to_token
    pub to_decimals: u8,
}

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
    /// Sandwich attack (frontrun + backrun)
    Sandwich,
    /// JIT (Just-In-Time) liquidity attack
    JitLiquidity,
    /// Failed MEV attempts (spam)
    Spam,
}

impl MevCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            MevCategory::Arbitrage => "arbitrage",
            MevCategory::Liquidation => "liquidation",
            MevCategory::Mint => "mint",
            MevCategory::Sandwich => "sandwich",
            MevCategory::JitLiquidity => "jit_liquidity",
            MevCategory::Spam => "spam",
        }
    }
}

/// Individual MEV event detected in a transaction
#[derive(Debug, Clone, Serialize)]
pub struct MevEvent {
    pub category: MevCategory,
    pub signature: String,
    pub signer: Option<String>,
    pub programs_involved: Vec<String>,
    pub token_changes: Vec<TokenChange>,
    pub sol_change_lamports: i64,
    pub success: bool,
    /// Individual swaps that make up this MEV transaction
    pub swaps: Vec<Swap>,
    /// Number of swaps detected
    pub swap_count: usize,
}

/// Multi-transaction MEV event (sandwich, JIT)
#[derive(Debug, Clone, Serialize)]
pub struct MultiTxMevEvent {
    pub category: MevCategory,
    /// Frontrun/setup transaction
    pub frontrun_signature: String,
    pub frontrun_tx_index: usize,
    /// Victim/target transaction
    pub victim_signature: String,
    pub victim_tx_index: usize,
    /// Backrun/exit transaction
    pub backrun_signature: String,
    pub backrun_tx_index: usize,
    /// Extracted profit (in tokens)
    pub profit_token_changes: Vec<TokenChange>,
    /// Total SOL profit
    pub total_sol_profit_lamports: i64,
    /// Programs involved across all transactions
    pub programs_involved: Vec<String>,
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
            // Sandwich and JIT are tracked separately in multi-tx MEV events
            MevCategory::Sandwich | MevCategory::JitLiquidity => {
                // These won't be in individual transactions, but in multi-tx events
                // If they appear here, count as spam
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

    /// Check if a program is an aggregator (routes through multiple DEXs for best price)
    pub fn is_aggregator(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::JUPITER_V6 | Self::JUPITER_LIMIT_ORDER
        )
    }

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
        signer: Option<String>,
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
        let category = Self::detect_category(&program_ids, &token_changes)?;

        // Calculate SOL balance change (signed)
        let sol_change_lamports = Self::calculate_sol_change(pre_balances, post_balances);

        // Detect individual swaps (for arbitrage transactions)
        let swaps = if category == MevCategory::Arbitrage {
            Self::detect_swaps(instructions, pre_token_balances, post_token_balances, &program_ids)
        } else {
            Vec::new()
        };
        let swap_count = swaps.len();

        // Track both successful AND failed MEV events
        // Failed attempts still consume compute units and block space
        Some(MevEvent {
            category,
            signature: signature.to_string(),
            signer,
            programs_involved: program_ids,
            token_changes,
            sol_change_lamports,
            success,
            swaps,
            swap_count,
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
    /// Returns None for regular (non-MEV) transactions like single-DEX swaps.
    /// Only flags actual MEV per sandwiched.me methodology:
    /// - Atomic Arbitrage: Multiple DEX interactions in single transaction (buy low, sell high)
    /// - Liquidations: Lending protocol interactions
    ///
    /// Aggregator vs Arbitrage distinction:
    /// - Jupiter/aggregators route through DEXs for best price (NOT arbitrage)
    /// - Actual arbitrage requires 2+ non-aggregator DEXs
    ///
    /// Note: Sandwich and JIT attacks are detected separately via multi-tx analysis
    /// Note: Token mints are NOT tracked as MEV (they're regular token creation, not value extraction)
    fn detect_category(program_ids: &[String], _token_changes: &[TokenChange]) -> Option<MevCategory> {
        let dex_count = program_ids.iter().filter(|p| ProgramRegistry::is_dex(p)).count();
        let lending_count = program_ids.iter().filter(|p| ProgramRegistry::is_lending(p)).count();
        let aggregator_count = program_ids.iter().filter(|p| ProgramRegistry::is_aggregator(p)).count();

        // ATOMIC ARBITRAGE: Multiple DEX interactions in single transaction (buy low, sell high)
        // This is the dominant MEV type on Solana (50-74% of transactions per sandwiched.me)
        //
        // Important: Distinguish aggregator routing from actual arbitrage
        // - Aggregators (Jupiter V6, etc.) route through multiple DEXs to get best price
        // - This is NOT arbitrage, just smart routing on behalf of the user
        // - Only flag as arbitrage if 2+ DEXs are present AND no aggregator
        if dex_count >= 2 {
            // If an aggregator is present, it's routing (NOT arbitrage)
            // The user called the aggregator, which routes through multiple DEXs
            // Examples of routing (NOT arbitrage):
            // - Jupiter + Pump.fun = aggregator routing
            // - Jupiter + Drift + Meteora + Orca = aggregator routing (multi-hop swap)
            if aggregator_count > 0 {
                return None; // This is normal aggregator routing, not MEV
            }

            // Only flag as arbitrage if 2+ DEXs with NO aggregator
            // Examples of actual arbitrage:
            // - Titan + Orca = 2 DEXs, no aggregator = ARBITRAGE
            // - Raydium + Meteora + Phoenix = 3 DEXs, no aggregator = ARBITRAGE
            return Some(MevCategory::Arbitrage);
        }

        // LIQUIDATION: Lending protocol interactions
        // These can be with or without DEX (selling collateral)
        if lending_count > 0 {
            return Some(MevCategory::Liquidation);
        }

        // Everything else is NOT MEV (regular user swaps, transfers, token mints, etc.)
        // Single DEX swaps (dex_count == 1) are normal user activity, not MEV
        // Aggregator routing (aggregator + 1 DEX) is also normal user activity
        None
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

        // Track token changes from user perspective (what they sent/received)
        // For each mint, we track the FIRST account with significant change
        // This captures the user's account, not pool accounts
        let mut mint_totals: HashMap<String, (f64, u8, u8)> = HashMap::new(); // (change, decimals, account_idx)

        for ((account_idx, mint), (pre_opt, post_opt, decimals)) in token_map {
            let pre = pre_opt.unwrap_or(0.0);
            let post = post_opt.unwrap_or(0.0);
            let change = post - pre;

            // Skip zero changes
            if change.abs() < 0.0000001 {
                continue;
            }

            // For each mint, keep the change from the EARLIEST account index
            // User accounts (including their token accounts) come before pool accounts
            mint_totals.entry(mint.clone())
                .and_modify(|e| {
                    // Keep the change from the earlier account index
                    if account_idx < e.2 {
                        e.0 = change;
                        e.1 = decimals;
                        e.2 = account_idx;
                    }
                })
                .or_insert((change, decimals, account_idx));
        }

        // Convert to TokenChange structs
        for (mint, (total_change, decimals, _account_idx)) in mint_totals {
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

    /// Detect individual swaps in an arbitrage transaction
    ///
    /// This function analyzes the token balance changes across different accounts
    /// to reconstruct the swap path. For arbitrage, we expect a sequence of swaps
    /// like: SOL → Token A → SOL (2 swaps) or Token A → Token B → Token C → Token A (3 swaps)
    ///
    /// Strategy:
    /// 1. Collect all token balance changes per account
    /// 2. Group changes by DEX program (from instructions)
    /// 3. Build swap path by analyzing token flow
    fn detect_swaps(
        _instructions: &[UiInstruction],
        pre_token_balances: &[UiTransactionTokenBalance],
        post_token_balances: &[UiTransactionTokenBalance],
        program_ids: &[String],
    ) -> Vec<Swap> {
        let mut swaps = Vec::new();

        // Get DEX programs involved (in order)
        let dex_programs: Vec<String> = program_ids
            .iter()
            .filter(|p| ProgramRegistry::is_dex(p))
            .cloned()
            .collect();

        if dex_programs.is_empty() {
            return swaps;
        }

        // Collect all token balance changes per (account_index, mint)
        let mut balance_changes: Vec<(u8, String, f64, f64, u8)> = Vec::new(); // (account_idx, mint, pre, post, decimals)

        // Build a map of pre-balances
        let mut pre_map: HashMap<(u8, String), (f64, u8)> = HashMap::new();
        for pre in pre_token_balances {
            let key = (pre.account_index, pre.mint.clone());
            let amount = pre.ui_token_amount.ui_amount.unwrap_or(0.0);
            pre_map.insert(key, (amount, pre.ui_token_amount.decimals));
        }

        // Build a map of post-balances and identify changes
        let mut post_map: HashMap<(u8, String), (f64, u8)> = HashMap::new();
        for post in post_token_balances {
            let key = (post.account_index, post.mint.clone());
            let amount = post.ui_token_amount.ui_amount.unwrap_or(0.0);
            post_map.insert(key, (amount, post.ui_token_amount.decimals));
        }

        // Find all changed balances
        let mut all_keys: Vec<(u8, String)> = Vec::new();
        all_keys.extend(pre_map.keys().cloned());
        all_keys.extend(post_map.keys().cloned());
        all_keys.sort();
        all_keys.dedup();

        for (account_idx, mint) in all_keys {
            let (pre_amount, decimals) = pre_map.get(&(account_idx, mint.clone())).cloned().unwrap_or((0.0, 9));
            let (post_amount, decimals) = post_map.get(&(account_idx, mint.clone())).cloned().unwrap_or((0.0, decimals));

            let change = post_amount - pre_amount;
            if change.abs() > 0.0000001 {
                balance_changes.push((account_idx, mint, pre_amount, post_amount, decimals));
            }
        }

        // For now, use a simple heuristic: count number of DEX programs as swap count
        // and build approximate swap path from token balance changes
        //
        // A better implementation would parse inner instructions to get exact swap sequence,
        // but this requires more complex instruction parsing

        // Group token changes by mint
        let mut mint_changes: HashMap<String, (f64, u8)> = HashMap::new();
        for (_, mint, pre, post, decimals) in &balance_changes {
            let change = post - pre;
            mint_changes.entry(mint.clone())
                .and_modify(|e| e.0 += change)
                .or_insert((change, *decimals));
        }

        // Identify tokens involved
        let mut tokens: Vec<(String, f64, u8)> = mint_changes
            .iter()
            .map(|(mint, (change, decimals))| (mint.clone(), *change, *decimals))
            .collect();
        tokens.sort_by(|a, b| a.0.cmp(&b.0)); // Sort by mint address for consistency

        // For arbitrage: typically we have 2 tokens involved
        // Pattern: Start with Token A, swap to Token B, swap back to Token A
        // Net: Token A has small positive change (profit), Token B has ~0 change

        // Build swaps based on DEX count and token changes
        // If we have N DEX programs, we likely have N swaps
        if tokens.len() >= 2 && dex_programs.len() >= 2 {
            // Identify the "intermediary" token (the one that was bought then sold)
            // This will have a net change close to 0 (or exactly 0)
            // The "profit" token will have a small positive change

            // Sort tokens by absolute change (ascending) - tokens with ~0 change were intermediaries
            let mut tokens_by_abs_change = tokens.clone();
            tokens_by_abs_change.sort_by(|a, b| {
                a.1.abs().partial_cmp(&b.1.abs()).unwrap_or(std::cmp::Ordering::Equal)
            });

            // Most arbitrages are 2-swap cycles: A -> B -> A
            // The intermediary token (B) should have the smallest net change
            if dex_programs.len() >= 2 && tokens.len() >= 2 {
                // For now, create swaps based on DEX count and available token info
                // Proper path reconstruction requires parsing inner instructions

                // Identify tokens by their net change
                // Typically: one token has small/positive change (profit token)
                // Other token(s) have larger absolute changes (intermediaries)

                let mut swap_tokens = tokens_by_abs_change.clone();

                // For 2-swap arbitrage (most common)
                if dex_programs.len() == 2 {
                    // Assuming tokens are sorted by absolute change (smallest first)
                    // tokens_by_abs_change[0] = token with smallest net change
                    // tokens_by_abs_change[1] = token with larger net change

                    let token_a = &swap_tokens[0];
                    let token_b = &swap_tokens[1];

                    // Use absolute values for amounts to avoid sign issues
                    let amount_a = token_a.1.abs();
                    let amount_b = token_b.1.abs();

                    // Swap 1: token with positive change -> token with negative change
                    // Swap 2: reverse direction
                    if token_a.1 > 0.0 {
                        // token_a increased (profit token), token_b decreased (intermediary)
                        swaps.push(Swap {
                            from_token: token_a.0.clone(),
                            from_amount: amount_b, // Amount sent in first swap
                            to_token: token_b.0.clone(),
                            to_amount: amount_b, // Amount received
                            dex_program: dex_programs[0].clone(),
                            from_decimals: token_a.2,
                            to_decimals: token_b.2,
                        });

                        swaps.push(Swap {
                            from_token: token_b.0.clone(),
                            from_amount: amount_b,
                            to_token: token_a.0.clone(),
                            to_amount: amount_b + amount_a, // Amount received (original + profit)
                            dex_program: dex_programs[1].clone(),
                            from_decimals: token_b.2,
                            to_decimals: token_a.2,
                        });
                    } else {
                        // token_b increased (profit token), token_a decreased (intermediary)
                        swaps.push(Swap {
                            from_token: token_b.0.clone(),
                            from_amount: amount_a,
                            to_token: token_a.0.clone(),
                            to_amount: amount_a,
                            dex_program: dex_programs[0].clone(),
                            from_decimals: token_b.2,
                            to_decimals: token_a.2,
                        });

                        swaps.push(Swap {
                            from_token: token_a.0.clone(),
                            from_amount: amount_a,
                            to_token: token_b.0.clone(),
                            to_amount: amount_a + amount_b,
                            dex_program: dex_programs[1].clone(),
                            from_decimals: token_a.2,
                            to_decimals: token_b.2,
                        });
                    }
                } else {
                    // For 3+ DEX arbitrages, create placeholder swaps
                    // Proper implementation would parse inner instructions
                    for (i, dex) in dex_programs.iter().enumerate() {
                        let from_token = if i < swap_tokens.len() {
                            swap_tokens[i % swap_tokens.len()].0.clone()
                        } else {
                            "unknown".to_string()
                        };

                        let to_token = if i + 1 < swap_tokens.len() {
                            swap_tokens[(i + 1) % swap_tokens.len()].0.clone()
                        } else {
                            swap_tokens[0].0.clone()
                        };

                        swaps.push(Swap {
                            from_token,
                            from_amount: 0.0,
                            to_token,
                            to_amount: 0.0,
                            dex_program: dex.clone(),
                            from_decimals: 9,
                            to_decimals: 9,
                        });
                    }
                }
            }
        }

        swaps
    }

    /// Detect sandwich and JIT attacks across transactions in a block
    ///
    /// Returns a list of multi-transaction MEV events (sandwich, JIT)
    pub fn detect_multi_tx_mev(
        transactions: &[(usize, &crate::types::FetchedTransaction, Option<MevEvent>)]
    ) -> Vec<MultiTxMevEvent> {
        let mut multi_tx_events = Vec::new();

        // Detect sandwich attacks
        multi_tx_events.extend(Self::detect_sandwiches(transactions));

        // Detect JIT liquidity attacks
        multi_tx_events.extend(Self::detect_jit_attacks(transactions));

        multi_tx_events
    }

    /// Detect sandwich attacks: Frontrun → Victim → Backrun
    ///
    /// Pattern:
    /// - Transaction i: Buy token X (increases price)
    /// - Transaction i+1 or i+2: Victim swap (pays higher price)
    /// - Transaction i+2 or i+3: Sell token X (takes profit)
    ///
    /// Heuristics:
    /// - Same token pair in frontrun and backrun
    /// - Opposite directions (buy then sell)
    /// - Net positive profit for the attacker
    /// - Victim transaction in between
    fn detect_sandwiches(
        transactions: &[(usize, &crate::types::FetchedTransaction, Option<MevEvent>)]
    ) -> Vec<MultiTxMevEvent> {
        let mut sandwiches = Vec::new();

        // Look for patterns within a small window (typically 1-3 txs apart)
        for i in 0..transactions.len().saturating_sub(2) {
            for j in (i + 1)..=std::cmp::min(i + 3, transactions.len().saturating_sub(1)) {
                for k in (j + 1)..=std::cmp::min(j + 2, transactions.len()) {
                    if k >= transactions.len() {
                        continue;
                    }

                    let (idx_i, tx_i, mev_i) = &transactions[i];
                    let (idx_j, tx_j, mev_j) = &transactions[j];
                    let (idx_k, tx_k, mev_k) = &transactions[k];

                    // All must be successful
                    if !tx_i.is_success() || !tx_j.is_success() || !tx_k.is_success() {
                        continue;
                    }

                    // Need MEV events for i and k
                    let Some(mev_front) = mev_i else { continue };
                    let Some(mev_back) = mev_k else { continue };

                    // Check if this looks like a sandwich
                    if Self::is_sandwich_pattern(mev_front, mev_j, mev_back) {
                        // Calculate total profit
                        let mut all_token_changes = mev_front.token_changes.clone();
                        all_token_changes.extend(mev_back.token_changes.clone());

                        // Aggregate by mint
                        let mut profit_map: HashMap<String, (f64, u8)> = HashMap::new();
                        for tc in &all_token_changes {
                            let entry = profit_map.entry(tc.mint.clone()).or_insert((0.0, tc.decimals));
                            entry.0 += tc.ui_amount_change;
                        }

                        let profit_token_changes: Vec<TokenChange> = profit_map
                            .into_iter()
                            .filter(|(_, (change, _))| change.abs() > 0.0000001)
                            .map(|(mint, (change, decimals))| TokenChange {
                                mint,
                                ui_amount_change: change,
                                decimals,
                            })
                            .collect();

                        let total_sol_profit = mev_front.sol_change_lamports + mev_back.sol_change_lamports;

                        // Collect all programs
                        let mut programs = mev_front.programs_involved.clone();
                        programs.extend(mev_back.programs_involved.clone());
                        programs.sort();
                        programs.dedup();

                        sandwiches.push(MultiTxMevEvent {
                            category: MevCategory::Sandwich,
                            frontrun_signature: mev_front.signature.clone(),
                            frontrun_tx_index: *idx_i,
                            victim_signature: tx_j.signature.clone(),
                            victim_tx_index: *idx_j,
                            backrun_signature: mev_back.signature.clone(),
                            backrun_tx_index: *idx_k,
                            profit_token_changes,
                            total_sol_profit_lamports: total_sol_profit,
                            programs_involved: programs,
                        });

                        // Found a sandwich, skip ahead
                        break;
                    }
                }
            }
        }

        sandwiches
    }

    /// Check if three transactions form a sandwich pattern
    fn is_sandwich_pattern(
        frontrun: &MevEvent,
        victim: &Option<MevEvent>,
        backrun: &MevEvent,
    ) -> bool {
        // Both frontrun and backrun should be ARBITRAGE (swaps)
        if frontrun.category != MevCategory::Arbitrage || backrun.category != MevCategory::Arbitrage {
            return false;
        }

        // Check if they trade the same token pair in opposite directions
        let front_tokens: Vec<&str> = frontrun.token_changes.iter().map(|tc| tc.mint.as_str()).collect();
        let back_tokens: Vec<&str> = backrun.token_changes.iter().map(|tc| tc.mint.as_str()).collect();

        // Must have overlapping tokens
        let has_common_tokens = front_tokens.iter().any(|t| back_tokens.contains(t));
        if !has_common_tokens {
            return false;
        }

        // Check for opposite directions (buy then sell)
        // If a token increases in frontrun and decreases in backrun (or vice versa), it's likely a sandwich
        for tc_front in &frontrun.token_changes {
            for tc_back in &backrun.token_changes {
                if tc_front.mint == tc_back.mint {
                    // Opposite signs indicate buying then selling (or vice versa)
                    if (tc_front.ui_amount_change > 0.0 && tc_back.ui_amount_change < 0.0) ||
                       (tc_front.ui_amount_change < 0.0 && tc_back.ui_amount_change > 0.0) {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Detect JIT liquidity attacks: Add LP → Large Swap → Remove LP
    ///
    /// Pattern:
    /// - Transaction i: Add liquidity to pool
    /// - Transaction i+1: Large swap occurs (generating fees)
    /// - Transaction i+2: Remove liquidity
    ///
    /// This is harder to detect without parsing specific DEX instructions,
    /// but we can use heuristics based on token changes.
    fn detect_jit_attacks(
        transactions: &[(usize, &crate::types::FetchedTransaction, Option<MevEvent>)]
    ) -> Vec<MultiTxMevEvent> {
        let mut jit_attacks = Vec::new();

        // Look for patterns within a small window
        for i in 0..transactions.len().saturating_sub(2) {
            let j = i + 1;
            let k = i + 2;

            if k >= transactions.len() {
                continue;
            }

            let (idx_i, tx_i, mev_i) = &transactions[i];
            let (idx_j, tx_j, mev_j) = &transactions[j];
            let (idx_k, tx_k, mev_k) = &transactions[k];

            // All must be successful
            if !tx_i.is_success() || !tx_j.is_success() || !tx_k.is_success() {
                continue;
            }

            // Need MEV events
            let Some(mev_add) = mev_i else { continue };
            let Some(mev_target) = mev_j else { continue };
            let Some(mev_remove) = mev_k else { continue };

            // Check if this looks like a JIT attack
            // Heuristic: Both first and third transactions involve same token pair
            // and target transaction is a large swap
            if Self::is_jit_pattern(mev_add, mev_target, mev_remove) {
                let mut all_token_changes = mev_add.token_changes.clone();
                all_token_changes.extend(mev_remove.token_changes.clone());

                let mut profit_map: HashMap<String, (f64, u8)> = HashMap::new();
                for tc in &all_token_changes {
                    let entry = profit_map.entry(tc.mint.clone()).or_insert((0.0, tc.decimals));
                    entry.0 += tc.ui_amount_change;
                }

                let profit_token_changes: Vec<TokenChange> = profit_map
                    .into_iter()
                    .filter(|(_, (change, _))| change.abs() > 0.0000001)
                    .map(|(mint, (change, decimals))| TokenChange {
                        mint,
                        ui_amount_change: change,
                        decimals,
                    })
                    .collect();

                let total_sol_profit = mev_add.sol_change_lamports + mev_remove.sol_change_lamports;

                let mut programs = mev_add.programs_involved.clone();
                programs.extend(mev_remove.programs_involved.clone());
                programs.sort();
                programs.dedup();

                jit_attacks.push(MultiTxMevEvent {
                    category: MevCategory::JitLiquidity,
                    frontrun_signature: mev_add.signature.clone(),
                    frontrun_tx_index: *idx_i,
                    victim_signature: mev_target.signature.clone(),
                    victim_tx_index: *idx_j,
                    backrun_signature: mev_remove.signature.clone(),
                    backrun_tx_index: *idx_k,
                    profit_token_changes,
                    total_sol_profit_lamports: total_sol_profit,
                    programs_involved: programs,
                });
            }
        }

        jit_attacks
    }

    /// Check if three transactions form a JIT liquidity pattern
    fn is_jit_pattern(
        add_lp: &MevEvent,
        target: &MevEvent,
        remove_lp: &MevEvent,
    ) -> bool {
        // Target should be an arbitrage/swap
        if target.category != MevCategory::Arbitrage {
            return false;
        }

        // Add and remove should involve same tokens (LP tokens typically)
        let add_tokens: Vec<&str> = add_lp.token_changes.iter().map(|tc| tc.mint.as_str()).collect();
        let remove_tokens: Vec<&str> = remove_lp.token_changes.iter().map(|tc| tc.mint.as_str()).collect();

        // Must have overlapping tokens
        let has_common_tokens = add_tokens.iter().any(|t| remove_tokens.contains(t));
        if !has_common_tokens {
            return false;
        }

        // Check if add and remove are opposite (net zero or small profit)
        for tc_add in &add_lp.token_changes {
            for tc_remove in &remove_lp.token_changes {
                if tc_add.mint == tc_remove.mint {
                    // Should be roughly opposite amounts (LP in, LP out)
                    let total = tc_add.ui_amount_change + tc_remove.ui_amount_change;
                    // Net change should be small (within 10% of either amount)
                    let threshold = tc_add.ui_amount_change.abs() * 0.1;
                    if total.abs() <= threshold {
                        return true;
                    }
                }
            }
        }

        false
    }
}
