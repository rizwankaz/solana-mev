use crate::types::FetchedTransaction;
use crate::mev::types::{SwapInfo, SwapDirection, TokenAmount};
use solana_transaction_status::{
    UiInstruction, UiParsedInstruction, UiCompiledInstruction,
    option_serializer::OptionSerializer,
};
use std::collections::HashMap;

/// Known Solana DEX program IDs
/// These are the major AMMs/DEXs on Solana where MEV commonly occurs
pub struct DexPrograms;

impl DexPrograms {
    // Raydium V4
    pub const RAYDIUM_V4: &'static str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

    // Raydium CLMM (Concentrated Liquidity)
    pub const RAYDIUM_CLMM: &'static str = "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK";

    // Orca Whirlpool (CLMM)
    pub const ORCA_WHIRLPOOL: &'static str = "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc";

    // Orca V1/V2
    pub const ORCA_V1: &'static str = "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP";
    pub const ORCA_V2: &'static str = "9qvG1zUp8xF1Bi4m6UdRNby1BAAuaDrUxSpv4CmRRMjL";

    // Jupiter Aggregator (popular routing protocol)
    pub const JUPITER_V4: &'static str = "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB";
    pub const JUPITER_V6: &'static str = "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4";

    // Meteora DLMM (Dynamic Liquidity Market Maker)
    pub const METEORA_DLMM: &'static str = "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo";

    // Phoenix (order book DEX)
    pub const PHOENIX: &'static str = "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY";

    // Lifinity (proactive market maker)
    pub const LIFINITY: &'static str = "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S";

    /// Check if program ID is a known DEX
    pub fn is_dex_program(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::RAYDIUM_V4
                | Self::RAYDIUM_CLMM
                | Self::ORCA_WHIRLPOOL
                | Self::ORCA_V1
                | Self::ORCA_V2
                | Self::JUPITER_V4
                | Self::JUPITER_V6
                | Self::METEORA_DLMM
                | Self::PHOENIX
                | Self::LIFINITY
        )
    }

    /// Get DEX name from program ID
    pub fn get_dex_name(program_id: &str) -> Option<&'static str> {
        match program_id {
            Self::RAYDIUM_V4 => Some("Raydium V4"),
            Self::RAYDIUM_CLMM => Some("Raydium CLMM"),
            Self::ORCA_WHIRLPOOL => Some("Orca Whirlpool"),
            Self::ORCA_V1 => Some("Orca V1"),
            Self::ORCA_V2 => Some("Orca V2"),
            Self::JUPITER_V4 => Some("Jupiter V4"),
            Self::JUPITER_V6 => Some("Jupiter V6"),
            Self::METEORA_DLMM => Some("Meteora DLMM"),
            Self::PHOENIX => Some("Phoenix"),
            Self::LIFINITY => Some("Lifinity"),
            _ => None,
        }
    }
}

/// Known lending protocols for liquidation detection
pub struct LendingPrograms;

impl LendingPrograms {
    // Solend
    pub const SOLEND: &'static str = "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo";

    // Mango Markets
    pub const MANGO_V3: &'static str = "mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68";
    pub const MANGO_V4: &'static str = "4MangoMjqJ2firMokCjjGgoK8d4MXcrgL7XJaL3w6fVg";

    // Marginfi
    pub const MARGINFI: &'static str = "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA";

    // Kamino Finance
    pub const KAMINO: &'static str = "KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD";

    /// Check if program ID is a known lending protocol
    pub fn is_lending_program(program_id: &str) -> bool {
        matches!(
            program_id,
            Self::SOLEND | Self::MANGO_V3 | Self::MANGO_V4 | Self::MARGINFI | Self::KAMINO
        )
    }

    /// Get lending protocol name
    pub fn get_protocol_name(program_id: &str) -> Option<&'static str> {
        match program_id {
            Self::SOLEND => Some("Solend"),
            Self::MANGO_V3 => Some("Mango V3"),
            Self::MANGO_V4 => Some("Mango V4"),
            Self::MARGINFI => Some("Marginfi"),
            Self::KAMINO => Some("Kamino"),
            _ => None,
        }
    }
}

/// Parser for extracting MEV-relevant data from transactions
pub struct TransactionParser;

impl TransactionParser {
    /// Extract all swaps from a transaction
    ///
    /// Parses transaction instructions to identify DEX swaps.
    /// Returns a vector of SwapInfo for all detected swaps.
    pub fn extract_swaps(tx: &FetchedTransaction) -> Vec<SwapInfo> {
        let mut swaps = Vec::new();

        // Get instructions from transaction
        let instructions = match Self::get_instructions(tx) {
            Some(instrs) => instrs,
            None => return swaps,
        };

        // Parse each instruction for swap operations
        for instruction in instructions {
            if let Some(swap) = Self::parse_swap_instruction(&instruction) {
                swaps.push(swap);
            }
        }

        swaps
    }

    /// Get instructions from transaction
    fn get_instructions(tx: &FetchedTransaction) -> Option<Vec<UiInstruction>> {
        match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        Some(parsed.instructions.clone())
                    },
                    solana_transaction_status::UiMessage::Raw(_) => {
                        // For raw messages, we'd need to decode them
                        // For now, return None as we focus on parsed messages
                        None
                    },
                }
            },
            _ => None,
        }
    }

    /// Parse a single instruction for swap operations
    fn parse_swap_instruction(instruction: &UiInstruction) -> Option<SwapInfo> {
        match instruction {
            UiInstruction::Parsed(parsed) => Self::parse_parsed_swap(parsed),
            UiInstruction::Compiled(_) => {
                // Compiled instructions need program-specific parsing
                // For now we focus on parsed instructions
                None
            },
        }
    }

    /// Parse a parsed instruction for swap data
    fn parse_parsed_swap(_instruction: &UiParsedInstruction) -> Option<SwapInfo> {
        // This is a simplified parser - in production you'd need
        // more sophisticated parsing based on the specific DEX program
        // and instruction format

        // For demonstration, we check if instruction data contains swap-like patterns
        None // Placeholder - would need program-specific parsing
    }

    /// Extract token transfers from transaction metadata
    ///
    /// This is often more reliable than parsing instructions directly,
    /// as we can see the actual token balance changes.
    pub fn extract_token_transfers(tx: &FetchedTransaction) -> Vec<TokenTransfer> {
        let mut transfers = Vec::new();

        let meta = match &tx.meta {
            Some(m) => m,
            None => return transfers,
        };

        // Get pre and post token balances
        let pre_balances = match &meta.pre_token_balances {
            OptionSerializer::Some(balances) => balances,
            _ => return transfers,
        };

        let post_balances = match &meta.post_token_balances {
            OptionSerializer::Some(balances) => balances,
            _ => return transfers,
        };

        // Match pre and post balances to find transfers
        // Group by account index to track balance changes
        let mut balance_changes: HashMap<u8, (Option<f64>, Option<f64>, String)> = HashMap::new();

        for pre_balance in pre_balances {
            balance_changes
                .entry(pre_balance.account_index)
                .or_insert((None, None, pre_balance.mint.clone()))
                .0 = pre_balance.ui_token_amount.ui_amount;
        }

        for post_balance in post_balances {
            balance_changes
                .entry(post_balance.account_index)
                .or_insert((None, None, post_balance.mint.clone()))
                .1 = post_balance.ui_token_amount.ui_amount;
        }

        // Calculate net changes
        for (account_idx, (pre, post, mint)) in balance_changes {
            let pre_amount = pre.unwrap_or(0.0);
            let post_amount = post.unwrap_or(0.0);
            let net_change = post_amount - pre_amount;

            if net_change.abs() > 0.000001 {
                // Non-zero change
                transfers.push(TokenTransfer {
                    account_index: account_idx,
                    mint,
                    pre_amount,
                    post_amount,
                    net_change,
                });
            }
        }

        transfers
    }

    /// Detect if transaction contains multiple swaps (potential arbitrage)
    pub fn is_multi_swap(tx: &FetchedTransaction) -> bool {
        Self::extract_swaps(tx).len() >= 2
    }

    /// Extract accounts that interacted with transaction
    pub fn extract_accounts(tx: &FetchedTransaction) -> Vec<String> {
        match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().map(|k| k.pubkey.clone()).collect()
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.clone()
                    },
                }
            },
            _ => Vec::new(),
        }
    }

    /// Check if transaction involves a specific program
    pub fn uses_program(tx: &FetchedTransaction, program_id: &str) -> bool {
        Self::extract_accounts(tx).contains(&program_id.to_string())
    }

    /// Check if transaction is a DEX swap
    pub fn is_dex_swap(tx: &FetchedTransaction) -> bool {
        let accounts = Self::extract_accounts(tx);
        accounts.iter().any(|acc| DexPrograms::is_dex_program(acc))
    }

    /// Check if transaction is a lending protocol interaction
    pub fn is_lending_interaction(tx: &FetchedTransaction) -> bool {
        let accounts = Self::extract_accounts(tx);
        accounts.iter().any(|acc| LendingPrograms::is_lending_program(acc))
    }

    /// Detect potential liquidation by checking for lending program + large transfers
    pub fn is_potential_liquidation(tx: &FetchedTransaction) -> bool {
        if !Self::is_lending_interaction(tx) {
            return false;
        }

        // Check for significant token transfers (debt repayment + collateral seizure)
        let transfers = Self::extract_token_transfers(tx);

        // Liquidations typically have at least 2 significant transfers:
        // 1. Debt token repayment
        // 2. Collateral token seizure
        transfers.len() >= 2
    }

    /// Extract the signer (fee payer) of the transaction
    pub fn get_signer(tx: &FetchedTransaction) -> Option<String> {
        tx.signer()
    }
}

/// Represents a token transfer detected in transaction metadata
#[derive(Debug, Clone)]
pub struct TokenTransfer {
    pub account_index: u8,
    pub mint: String,
    pub pre_amount: f64,
    pub post_amount: f64,
    pub net_change: f64,
}

// Re-export for convenience
pub use self::TokenTransfer as Transfer;

impl TokenTransfer {
    /// Check if this is an inflow (positive change)
    pub fn is_inflow(&self) -> bool {
        self.net_change > 0.0
    }

    /// Check if this is an outflow (negative change)
    pub fn is_outflow(&self) -> bool {
        self.net_change < 0.0
    }
}
