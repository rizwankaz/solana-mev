use crate::types::FetchedTransaction;
use crate::mev::parser::TokenTransfer;
use crate::mev::registry::ProgramRegistry;
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
    /// Three-tier detection strategy:
    /// 1. Primary: Check if instructions have swap-related names/discriminators
    /// 2. Fallback: If instructions have method data (8+ bytes) and tokens move bidirectionally
    /// 3. Registry: Check if transaction interacts with known DEX program IDs
    ///
    /// This catches:
    /// - Parsed swap instructions by name
    /// - Unparsed swap instructions by discriminator
    /// - Unknown DEX protocols (via instruction data + token movement pattern)
    /// - Known DEXs even when instruction parsing fails
    pub fn is_swap(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        // Tier 1: Explicit swap instruction detection
        if Self::has_swap_instruction(tx) {
            return true;
        }

        // Tier 2: Instruction data pattern + token movement validation
        // For unparsed/unknown protocols
        let has_method_calls = Self::has_substantial_instruction_data(tx);

        if has_method_calls && transfers.len() >= 2 {
            let inflows = transfers.iter().filter(|t| t.is_inflow()).count();
            let outflows = transfers.iter().filter(|t| t.is_outflow()).count();
            let unique_tokens: HashSet<_> = transfers.iter().map(|t| &t.mint).collect();

            // Pattern: method call + tokens moving in both directions + multiple tokens
            if inflows > 0 && outflows > 0 && unique_tokens.len() >= 2 {
                return true;
            }
        }

        // Tier 3: Registry-based detection (final fallback)
        // Check if transaction interacts with any known DEX program
        if Self::has_dex_program(tx) {
            // Additional validation: must have token movements
            if transfers.len() >= 2 {
                let inflows = transfers.iter().filter(|t| t.is_inflow()).count();
                let outflows = transfers.iter().filter(|t| t.is_outflow()).count();

                // Only classify as swap if tokens actually moved
                if inflows > 0 && outflows > 0 {
                    return true;
                }
            }
        }

        false
    }

    /// Check if transaction has substantial instruction data (method calls)
    fn has_substantial_instruction_data(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            if let Some(data) = Self::get_instruction_data(&instruction) {
                // 8+ bytes indicates a discriminator (method call)
                // Simple token transfers have minimal data (<8 bytes)
                if data.len() >= 8 {
                    return true;
                }
            }
        }

        false
    }

    /// Detect if transaction contains a liquidation operation
    ///
    /// Multi-tier detection:
    /// 1. Instruction-based: Check for liquidation-related instruction names/discriminators
    /// 2. Pattern-based: Complex multi-token transfers (3+ tokens, 4+ movements)
    /// 3. Registry-based: Known lending protocol with appropriate token movement pattern
    pub fn is_liquidation(tx: &FetchedTransaction, transfers: &[TokenTransfer]) -> bool {
        // Baseline: Liquidations typically have 3+ token transfers
        // (debt token out, collateral token in, sometimes multiple)
        if transfers.len() < 3 {
            return false;
        }

        // Must have both inflows and outflows
        let has_inflows = transfers.iter().any(|t| t.is_inflow());
        let has_outflows = transfers.iter().any(|t| t.is_outflow());

        if !has_inflows || !has_outflows {
            return false;
        }

        // Tier 1: Explicit liquidation instruction detection
        if Self::has_liquidation_instruction(tx) {
            return true;
        }

        // Tier 2: Complex multi-token transfer pattern
        // Liquidations often involve 3+ different tokens
        let unique_tokens: HashSet<_> = transfers.iter().map(|t| &t.mint).collect();
        if unique_tokens.len() >= 3 && transfers.len() >= 4 {
            return true;
        }

        // Tier 3: Registry-based detection
        // Known lending protocol with complex token movements
        if Self::has_lending_program(tx) && unique_tokens.len() >= 2 && transfers.len() >= 3 {
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

    /// Parse instructions to detect swap-related operations
    ///
    /// Purely instruction-based detection using:
    /// 1. Parsed instruction type names
    /// 2. Instruction discriminators (first 8 bytes of data)
    ///
    /// No hardcoded program IDs
    fn has_swap_instruction(tx: &FetchedTransaction) -> bool {
        let instructions = Self::get_all_instructions(tx);

        for instruction in instructions {
            // Method 1: Check parsed instruction type names for swap-related keywords
            if let Some(name) = Self::get_instruction_name(&instruction) {
                let name_lower = name.to_lowercase();
                if name_lower.contains("swap")
                    || name_lower.contains("exchange")
                    || name_lower.contains("trade")
                    || name_lower.contains("route")
                    || name_lower.contains("buy")
                    || name_lower.contains("sell")
                {
                    return true;
                }
            }

            // Method 2: Check instruction discriminators (first 8 bytes of data)
            // This is more reliable as it detects the actual method being called
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

    /// Check if instruction data contains swap discriminator
    ///
    /// Discriminators are the first 8 bytes of instruction data, representing
    /// the hash of the method name in Anchor programs. For non-parsed instructions,
    /// this is the primary way to identify the operation type.
    ///
    /// Note: This is a fallback for instructions that couldn't be parsed.
    /// We include common discriminators from major DEXs as examples, but
    /// the parsed instruction name is the primary detection method.
    fn has_swap_discriminator(data: &[u8]) -> bool {
        if data.len() < 8 {
            return false;
        }

        let discriminator = &data[0..8];

        // Common swap discriminators (first 8 bytes = SHA256("global:swap")[0..8])
        // Note: These are examples. In practice, parsed instructions should catch most swaps.

        // Generic "swap" discriminators from Anchor programs
        let anchor_swap = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x7b, 0xf5, 0xae]; // swap
        let swap_exact = [0x33, 0x1f, 0x5a, 0x94, 0x97, 0x3f, 0x66, 0x7f];   // swapExact...
        let route_swap = [0xdd, 0xda, 0x3c, 0x8d, 0x62, 0x8c, 0x9f, 0x7a];   // route/sharedAccounts...

        discriminator == anchor_swap
            || discriminator == swap_exact
            || discriminator == route_swap
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

    /// Check if transaction interacts with any known DEX program
    ///
    /// Extracts all program IDs from the transaction and checks them against
    /// the DEX registry.
    fn has_dex_program(tx: &FetchedTransaction) -> bool {
        let program_ids = Self::get_program_ids(tx);
        program_ids.iter().any(|id| ProgramRegistry::is_dex(id))
    }

    /// Check if transaction interacts with any known lending protocol
    ///
    /// Extracts all program IDs from the transaction and checks them against
    /// the lending protocol registry.
    fn has_lending_program(tx: &FetchedTransaction) -> bool {
        let program_ids = Self::get_program_ids(tx);
        program_ids.iter().any(|id| ProgramRegistry::is_lending_protocol(id))
    }

    /// Extract all program IDs from transaction
    ///
    /// Returns a set of all program IDs that appear in the transaction's
    /// account keys (both regular instructions and inner instructions).
    fn get_program_ids(tx: &FetchedTransaction) -> HashSet<String> {
        let mut program_ids = HashSet::new();

        match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                // Get account keys
                let account_keys = match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().map(|key| key.pubkey.clone()).collect::<Vec<_>>()
                    }
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.clone()
                    }
                };

                // Add program IDs from instructions
                match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        for instruction in &parsed.instructions {
                            if let Some(program_id) = Self::get_program_id_from_instruction(instruction, &account_keys) {
                                program_ids.insert(program_id);
                            }
                        }
                    }
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        for instruction in &raw.instructions {
                            let idx = instruction.program_id_index as usize;
                            if idx < account_keys.len() {
                                program_ids.insert(account_keys[idx].clone());
                            }
                        }
                    }
                }

                // Add program IDs from inner instructions
                if let Some(meta) = &tx.meta {
                    if let solana_transaction_status::option_serializer::OptionSerializer::Some(inner) = &meta.inner_instructions {
                        for inner_ix_list in inner {
                            for instruction in &inner_ix_list.instructions {
                                if let Some(program_id) = Self::get_program_id_from_instruction(instruction, &account_keys) {
                                    program_ids.insert(program_id);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        program_ids
    }

    /// Extract program ID from an instruction
    fn get_program_id_from_instruction(instruction: &UiInstruction, account_keys: &[String]) -> Option<String> {
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
                if (compiled.program_id_index as usize) < account_keys.len() {
                    Some(account_keys[compiled.program_id_index as usize].clone())
                } else {
                    None
                }
            }
        }
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
