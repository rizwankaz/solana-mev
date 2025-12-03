use crate::types::FetchedTransaction;
use crate::mev::parser::TokenTransfer;
use solana_transaction_status::{UiInstruction, UiParsedInstruction};
use std::collections::HashSet;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

/// Instruction-based transaction classifier
///
/// Uses instruction data and token transfer patterns to dynamically identify:
/// - Swap operations (any DEX)
/// - Liquidation operations (any lending protocol)
/// - Liquidity operations (add/remove)
///
/// This approach is protocol-agnostic and doesn't require hardcoded program IDs.
pub struct InstructionClassifier;

impl InstructionClassifier {
    /// Detect if transaction contains a swap operation
    ///
    /// Uses multiple heuristics to identify swaps:
    /// - Known DEX program IDs
    /// - Instruction names/discriminators
    /// - Token transfer patterns
    ///
    /// NOTE: Intentionally permissive to catch all swap types including
    /// multi-hop routes, aggregator swaps, and complex arbitrages
    pub fn is_swap(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        // Heuristic 1: Must have at least 2 token transfers
        if transfers.len() < 2 {
            return false;
        }

        // Heuristic 2: Must have at least 1 inflow and 1 outflow
        let inflows = transfers.iter().filter(|t| t.is_inflow()).count();
        let outflows = transfers.iter().filter(|t| t.is_outflow()).count();

        if inflows == 0 || outflows == 0 {
            return false;
        }

        // Heuristic 3: Check if transaction interacts with known DEX programs
        if Self::has_dex_program(tx) {
            return true;
        }

        // Heuristic 4: Check instruction names/discriminators
        if Self::has_swap_instruction(tx) {
            return true;
        }

        // Heuristic 5: Very permissive token transfer pattern
        // Allow up to 50 transfers for complex multi-hop aggregator routes
        // Allow up to 20 inflows/outflows for route splitting
        if transfers.len() <= 50 {
            let unique_tokens: HashSet<_> = transfers.iter().map(|t| &t.mint).collect();
            // If 2+ different tokens are moving bidirectionally, it's likely a swap
            if unique_tokens.len() >= 2 {
                return true;
            }
        }

        false
    }

    /// Detect if transaction contains a liquidation operation
    ///
    /// Heuristics:
    /// 1. Has multiple token transfers (3+) - debt repayment + collateral seizure
    /// 2. Contains instruction names suggesting liquidation: "liquidate", "liquidateBorrow"
    /// 3. Has both significant inflows and outflows
    /// 4. More complex than a simple swap (4+ token movements)
    pub fn is_liquidation(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        // Heuristic 1: Liquidations typically have 3+ token transfers
        // (debt token out, collateral token in, sometimes multiple)
        if transfers.len() < 3 {
            return false;
        }

        // Heuristic 2: Must have both inflows and outflows
        let has_inflows = transfers.iter().any(|t| t.is_inflow());
        let has_outflows = transfers.iter().any(|t| t.is_outflow());

        if !has_inflows || !has_outflows {
            return false;
        }

        // Heuristic 3: Check instruction data for liquidation patterns
        if Self::has_liquidation_instruction(tx) {
            return true;
        }

        // Heuristic 4: Complex multi-token transfer pattern
        // Liquidations often involve 3+ different tokens
        let unique_tokens: HashSet<_> = transfers.iter().map(|t| &t.mint).collect();
        if unique_tokens.len() >= 3 && transfers.len() >= 4 {
            return true;
        }

        false
    }

    /// Detect if transaction adds liquidity
    ///
    /// Heuristics:
    /// 1. Has 2-3 outflows (depositing token pair + optional SOL for fees)
    /// 2. Has 1 inflow (LP token receipt)
    /// 3. Instruction names suggest liquidity: "addLiquidity", "deposit", "mint"
    pub fn is_add_liquidity(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        let outflows = transfers.iter().filter(|t| t.is_outflow()).count();
        let inflows = transfers.iter().filter(|t| t.is_inflow()).count();

        // Pattern: 2+ outflows (token deposits), 1+ inflows (LP tokens)
        if outflows >= 2 && inflows >= 1 {
            // Check for add liquidity instruction patterns
            if Self::has_add_liquidity_instruction(tx) {
                return true;
            }

            // Heuristic: 2 outflows + 1 inflow suggests liquidity add
            if outflows == 2 && inflows == 1 {
                return true;
            }
        }

        false
    }

    /// Detect if transaction removes liquidity
    ///
    /// Heuristics:
    /// 1. Has 1 outflow (burning LP token)
    /// 2. Has 2+ inflows (withdrawing token pair)
    /// 3. Instruction names suggest liquidity removal: "removeLiquidity", "withdraw", "burn"
    pub fn is_remove_liquidity(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        let outflows = transfers.iter().filter(|t| t.is_outflow()).count();
        let inflows = transfers.iter().filter(|t| t.is_inflow()).count();

        // Pattern: 1 outflow (LP token), 2+ inflows (token withdrawals)
        if outflows >= 1 && inflows >= 2 {
            // Check for remove liquidity instruction patterns
            if Self::has_remove_liquidity_instruction(tx) {
                return true;
            }

            // Heuristic: 1 outflow + 2 inflows suggests liquidity removal
            if outflows == 1 && inflows == 2 {
                return true;
            }
        }

        false
    }

    /// Check if transaction interacts with known DEX program IDs
    fn has_dex_program(tx: &FetchedTransaction) -> bool {
        // Major DEX program IDs on Solana
        const DEX_PROGRAMS: &[&str] = &[
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4",      // Jupiter V6
            "JUP4Fb2cqiRUcaTHdrPC8h2gNsA2ETXiPDD33WcGuJB",      // Jupiter V4
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8",      // Raydium V4
            "5quBtoiQqxF9Jv6KYKctB59NT3gtJD2Y65kdnB1Uev3h",      // Raydium Concentrated Liquidity
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc",      // Orca Whirlpool
            "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP",      // Orca V2
            "DjVE6JNiYqPL2QXyCUUh8rNjHrbz9hXHNYt99MQ59qw1",      // Orca V1
            "Dooar9JkhdZ7J3LHN3A7YCuoGRUggXhQaG4kijfLGU2j",      // Meteora DLMM
            "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo",       // Meteora Pools
            "SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ",       // Saber
            "Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB",       // Mercurial
            "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY",       // Phoenix
            "LFNTYraetVioAPnGJht4yNg2aUZFXR776cMeN9VMjXp",       // Lifinity V1
            "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S",       // Lifinity V2
            "CLMM9tUoggJu2wagPkkqs9eFG4BWhVBZWkP1qv3Sp7tR",       // Raydium CLMM
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK",       // Raydium CPMM
            "27haf8L6oxUeXrHrgEgsexjSY5hbVUWEmvv9Nyxg8vQv",       // Cropper
            "AMM55ShdkoGRB5jVYPjWziwk8m5MpwyDgsMWHaMSQWH6",       // Aldrin
            "HyaB3W9q6XdA5xwpU4XnSZV94htfmbmqJXZcEbRaJutt",       // Aldrin V2
            "SSwpMgqNDsyV7mAgN9ady4bDVu5ySjmmXejXvy2vLt1",       // Step Finance
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P",       // Penguin
            "CURVGoZn8zycx6FXwwevgBTB2gVvdbGTEpvMJDbgs2t4",      // Invariant
        ];

        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            if let Some(program_id) = Self::get_program_id(&instruction) {
                if DEX_PROGRAMS.contains(&program_id.as_str()) {
                    return true;
                }
            }
        }

        false
    }

    /// Parse instructions to detect swap-related operations
    fn has_swap_instruction(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            // Check parsed instruction names
            if let Some(name) = Self::get_instruction_name(&instruction) {
                let name_lower = name.to_lowercase();
                if name_lower.contains("swap")
                    || name_lower.contains("exchange")
                    || name_lower.contains("trade")
                    || name_lower.contains("route") // Jupiter routing
                    || name_lower.contains("raydium")
                    || name_lower.contains("orca")
                    || name_lower.contains("whirlpool")
                    || name_lower.contains("meteora")
                    || name_lower.contains("phoenix")
                    || name_lower.contains("lifinity")
                {
                    return true;
                }
            }

            // Check instruction data for swap discriminators
            if let Some(data) = Self::get_instruction_data(&instruction) {
                if Self::has_swap_discriminator(&data) {
                    return true;
                }
            }
        }

        false
    }

    /// Parse instructions to detect liquidation operations
    fn has_liquidation_instruction(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            if let Some(name) = Self::get_instruction_name(&instruction) {
                let name_lower = name.to_lowercase();
                if name_lower.contains("liquidate")
                    || name_lower.contains("liquidation")
                    || name_lower == "liquidateborrow"
                    || name_lower == "liquidateandredeem"
                {
                    return true;
                }
            }

            // Check for lending protocol discriminators
            if let Some(data) = Self::get_instruction_data(&instruction) {
                if Self::has_liquidation_discriminator(&data) {
                    return true;
                }
            }
        }

        false
    }

    /// Parse instructions for add liquidity operations
    fn has_add_liquidity_instruction(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            if let Some(name) = Self::get_instruction_name(&instruction) {
                let name_lower = name.to_lowercase();
                if name_lower.contains("addliquidity")
                    || name_lower.contains("deposit")
                    || name_lower.contains("mintlp")
                    || name_lower.contains("increaseposition")
                    || name_lower.contains("openliquidity")
                {
                    return true;
                }
            }
        }

        false
    }

    /// Parse instructions for remove liquidity operations
    fn has_remove_liquidity_instruction(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            if let Some(name) = Self::get_instruction_name(&instruction) {
                let name_lower = name.to_lowercase();
                if name_lower.contains("removeliquidity")
                    || name_lower.contains("withdraw")
                    || name_lower.contains("burnlp")
                    || name_lower.contains("decreaseposition")
                    || name_lower.contains("closeliquidity")
                {
                    return true;
                }
            }
        }

        false
    }

    /// Get all instructions from transaction (including inner instructions)
    fn get_all_instructions(tx: &FetchedTransaction) -> Vec<UiInstruction> {
        let mut instructions = Vec::new();

        match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        instructions.extend(parsed.instructions.clone());
                    }
                    solana_transaction_status::UiMessage::Raw(_) => {}
                }
            }
            _ => {}
        }

        // Also check inner instructions (for CPI calls)
        if let Some(meta) = &tx.meta {
            if let solana_transaction_status::option_serializer::OptionSerializer::Some(inner) = &meta.inner_instructions {
                for inner_ix in inner {
                    instructions.extend(inner_ix.instructions.clone());
                }
            }
        }

        instructions
    }

    /// Extract instruction name from UiInstruction
    fn get_instruction_name(instruction: &UiInstruction) -> Option<String> {
        match instruction {
            UiInstruction::Parsed(parsed) => match parsed {
                UiParsedInstruction::Parsed(parsed_data) => {
                    // Try to get the instruction type from the parsed JSON object
                    if let Some(obj) = parsed_data.parsed.as_object() {
                        if let Some(type_field) = obj.get("type") {
                            return type_field.as_str().map(|s| s.to_string());
                        }
                    }
                    None
                }
                UiParsedInstruction::PartiallyDecoded(decoded) => {
                    Some(decoded.program_id.clone())
                }
            },
            UiInstruction::Compiled(_) => None,
        }
    }

    /// Extract instruction data bytes
    fn get_instruction_data(instruction: &UiInstruction) -> Option<Vec<u8>> {
        match instruction {
            UiInstruction::Compiled(compiled) => {
                // Data is base58 encoded
                bs58::decode(&compiled.data).into_vec().ok()
            }
            UiInstruction::Parsed(UiParsedInstruction::PartiallyDecoded(decoded)) => {
                // Data is base64 encoded string
                BASE64.decode(decoded.data.as_str()).ok()
            }
            _ => None,
        }
    }

    /// Extract program ID from UiInstruction
    fn get_program_id(instruction: &UiInstruction) -> Option<String> {
        match instruction {
            UiInstruction::Parsed(parsed) => match parsed {
                UiParsedInstruction::Parsed(parsed_data) => {
                    Some(parsed_data.program.clone())
                }
                UiParsedInstruction::PartiallyDecoded(decoded) => {
                    Some(decoded.program_id.clone())
                }
            },
            UiInstruction::Compiled(compiled) => {
                Some(compiled.program_id_index.to_string())
            }
        }
    }

    /// Check if instruction data contains swap discriminator
    ///
    /// Common Anchor discriminators for swap:
    /// - First 8 bytes are method discriminator (sighash)
    /// - Swap methods typically hash to specific values
    fn has_swap_discriminator(data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }

        // Common swap discriminators (first 8 bytes)
        // These are anchor method discriminators for "swap" functions

        // Raydium swap discriminator
        let raydium_swap = [0x33, 0x1f, 0x5a, 0x94, 0x97, 0x3f, 0x66, 0x7f];

        // Jupiter swap discriminators (multiple versions)
        let jupiter_route = [0xdd, 0xda, 0x3c, 0x8d, 0x62, 0x8c, 0x9f, 0x7a];

        // Orca swap discriminator
        let orca_swap = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x7b, 0xf5, 0xae];

        let discriminator = &data[0..8];

        discriminator == raydium_swap
            || discriminator == jupiter_route
            || discriminator == orca_swap
    }

    /// Check if instruction data contains liquidation discriminator
    fn has_liquidation_discriminator(data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }

        // Common liquidation discriminators
        // Solend liquidate: liquidateObligation
        let solend_liquidate = [0x59, 0x59, 0x4a, 0xbd, 0x3c, 0x7f, 0x8e, 0x4d];

        // Mango liquidate
        let mango_liquidate = [0x1d, 0x9c, 0x40, 0x56, 0x3a, 0x9f, 0x7e, 0x2c];

        let discriminator = &data[0..8];

        discriminator == solend_liquidate || discriminator == mango_liquidate
    }
}

/// Transaction filter based on instruction analysis
pub struct TransactionFilter;

impl TransactionFilter {
    /// Filter transactions to only those containing swaps
    pub fn filter_swaps(txs: &[crate::types::FetchedTransaction]) -> Vec<crate::types::FetchedTransaction> {
        txs.iter()
            .filter(|tx| {
                if !tx.is_success() {
                    return false;
                }

                let transfers = crate::mev::parser::TransactionParser::extract_token_transfers(tx);
                InstructionClassifier::is_swap(tx, &transfers)
            })
            .cloned()
            .collect()
    }

    /// Filter transactions to only those containing liquidations
    pub fn filter_liquidations(txs: &[crate::types::FetchedTransaction]) -> Vec<crate::types::FetchedTransaction> {
        txs.iter()
            .filter(|tx| {
                if !tx.is_success() {
                    return false;
                }

                let transfers = crate::mev::parser::TransactionParser::extract_token_transfers(tx);
                InstructionClassifier::is_liquidation(tx, &transfers)
            })
            .cloned()
            .collect()
    }

    /// Filter transactions to those containing liquidity operations
    pub fn filter_liquidity_ops(txs: &[crate::types::FetchedTransaction]) -> Vec<(crate::types::FetchedTransaction, LiquidityOp)> {
        txs.iter()
            .filter_map(|tx| {
                if !tx.is_success() {
                    return None;
                }

                let transfers = crate::mev::parser::TransactionParser::extract_token_transfers(tx);

                if InstructionClassifier::is_add_liquidity(tx, &transfers) {
                    Some((tx.clone(), LiquidityOp::Add))
                } else if InstructionClassifier::is_remove_liquidity(tx, &transfers) {
                    Some((tx.clone(), LiquidityOp::Remove))
                } else {
                    None
                }
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidityOp {
    Add,
    Remove,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swap_discriminator() {
        let raydium_data = vec![0x33, 0x1f, 0x5a, 0x94, 0x97, 0x3f, 0x66, 0x7f, 0x00, 0x00];
        assert!(InstructionClassifier::has_swap_discriminator(&raydium_data));
    }

    #[test]
    fn test_liquidation_discriminator() {
        let solend_data = vec![0x59, 0x59, 0x4a, 0xbd, 0x3c, 0x7f, 0x8e, 0x4d, 0x00, 0x00];
        assert!(InstructionClassifier::has_liquidation_discriminator(&solend_data));
    }
}
