//! Main DEX parser that coordinates all protocol-specific parsers

use super::common::*;
use crate::types::FetchedTransaction;
use anyhow::Result;
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodedTransactionWithStatusMeta, UiInstruction,
    UiParsedInstruction, UiTransactionTokenBalance,
};

/// Main DEX parser that can parse transactions from any supported DEX
pub struct DexParser;

impl DexParser {
    /// Parse all swaps from a transaction
    pub fn parse_transaction(
        tx: &FetchedTransaction,
        tx_index: usize,
    ) -> Result<Vec<ParsedSwap>> {
        let mut swaps = Vec::new();

        // Get the transaction data
        let _transaction = match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => ui_tx,
            _ => return Ok(swaps), // Skip non-JSON encoded transactions
        };

        // NOTE: Simplified - would need to properly parse instructions from UiMessage
        // For now, we rely on token balance changes to detect swaps

        // Get token balances for amount calculation
        let pre_balances = tx
            .meta
            .as_ref()
            .and_then(|m| match &m.pre_token_balances {
                OptionSerializer::Some(balances) => Some(balances),
                _ => None,
            });

        let post_balances = tx
            .meta
            .as_ref()
            .and_then(|m| match &m.post_token_balances {
                OptionSerializer::Some(balances) => Some(balances),
                _ => None,
            });

        // Detect swaps from token balance changes
        if let (Some(pre), Some(post)) = (pre_balances, post_balances) {
            if let Some(swap) = Self::detect_swap_from_balances(
                pre,
                post,
                "Unknown",
                &tx.signature,
                tx_index,
                0,
            )? {
                swaps.push(swap);
            }
        }

        Ok(swaps)
    }

    /// Parse a single instruction
    fn parse_instruction(
        instruction: &UiInstruction,
        signature: &str,
        tx_index: usize,
        instruction_index: usize,
        pre_balances: Option<&Vec<UiTransactionTokenBalance>>,
        post_balances: Option<&Vec<UiTransactionTokenBalance>>,
    ) -> Result<Option<ParsedSwap>> {
        match instruction {
            UiInstruction::Compiled(_compiled) => {
                // Try to parse based on program ID index
                // This would require account keys, which we'd get from the transaction
                Ok(None)
            }
            UiInstruction::Parsed(_parsed) => {
                // Simplified - rely on balance changes for now
                Ok(None)
            }
        }
    }

    /// Parse inner instruction
    fn parse_inner_instruction(
        instruction: &UiInstruction,
        signature: &str,
        tx_index: usize,
        outer_index: usize,
        inner_index: usize,
        pre_balances: Option<&Vec<UiTransactionTokenBalance>>,
        post_balances: Option<&Vec<UiTransactionTokenBalance>>,
    ) -> Result<Option<ParsedSwap>> {
        // Similar to parse_instruction but for inner instructions
        Self::parse_instruction(
            instruction,
            signature,
            tx_index,
            outer_index * 1000 + inner_index, // Encode both indices
            pre_balances,
            post_balances,
        )
    }

    /// Parse a parsed instruction (JSON format)
    fn _parse_parsed_instruction(
        _parsed: &UiParsedInstruction,
        signature: &str,
        tx_index: usize,
        instruction_index: usize,
        pre_balances: Option<&Vec<UiTransactionTokenBalance>>,
        post_balances: Option<&Vec<UiTransactionTokenBalance>>,
    ) -> Result<Option<ParsedSwap>> {
        // Simplified: detect swaps by looking for token transfers in pre/post balances
        // This is a heuristic approach that works across all DEXs
        if let (Some(pre), Some(post)) = (pre_balances, post_balances) {
            if let Some(swap) = Self::detect_swap_from_balances(
                pre,
                post,
                "Unknown",
                signature,
                tx_index,
                instruction_index,
            )? {
                return Ok(Some(swap));
            }
        }

        Ok(None)
    }

    /// Detect swaps by analyzing token balance changes
    fn detect_swap_from_balances(
        pre_balances: &[UiTransactionTokenBalance],
        post_balances: &[UiTransactionTokenBalance],
        program: &str,
        signature: &str,
        tx_index: usize,
        instruction_index: usize,
    ) -> Result<Option<ParsedSwap>> {
        // Find balance changes
        let mut changes: Vec<(String, i128, String)> = Vec::new(); // (mint, change, owner)

        for post in post_balances {
            if let Some(pre) = pre_balances
                .iter()
                .find(|p| p.account_index == post.account_index)
            {
                let pre_amount = pre
                    .ui_token_amount
                    .amount
                    .parse::<u64>()
                    .unwrap_or(0) as i128;
                let post_amount = post
                    .ui_token_amount
                    .amount
                    .parse::<u64>()
                    .unwrap_or(0) as i128;
                let change = post_amount - pre_amount;

                if change != 0 {
                    changes.push((
                        post.mint.clone(),
                        change,
                        post.owner.clone().unwrap_or("Unknown".to_string()),
                    ));
                }
            }
        }

        // A swap typically involves: one token decreasing, another increasing
        let decreases: Vec<_> = changes.iter().filter(|(_, c, _)| *c < 0).collect();
        let increases: Vec<_> = changes.iter().filter(|(_, c, _)| *c > 0).collect();

        // Simple heuristic: if we have 1 decrease and 1 increase for the same owner, it's likely a swap
        if decreases.len() >= 1 && increases.len() >= 1 {
            // Find the user's decrease and increase
            // This is simplified - in reality, you'd want more sophisticated matching
            if let (Some(decrease), Some(increase)) = (decreases.first(), increases.first()) {
                let dex = DexProtocol::from_program_id(program);

                return Ok(Some(ParsedSwap {
                    dex,
                    program_id: program.to_string(),
                    pool: "Unknown".to_string(), // Would need to parse from accounts
                    user: decrease.2.clone(),
                    token_in: decrease.0.clone(),
                    token_out: increase.0.clone(),
                    amount_in: decrease.1.unsigned_abs() as u64,
                    amount_out: increase.1 as u64,
                    min_amount_out: None,
                    price_before: None,
                    price_after: None,
                    price_impact: None,
                    signature: signature.to_string(),
                    tx_index,
                    instruction_index,
                }));
            }
        }

        Ok(None)
    }

    /// Extract all swaps from a block of transactions
    pub fn parse_block(transactions: &[FetchedTransaction]) -> Vec<ParsedSwap> {
        let mut all_swaps = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            if let Ok(swaps) = Self::parse_transaction(tx, idx) {
                all_swaps.extend(swaps);
            }
        }

        all_swaps
    }

    /// Find swaps involving specific token pairs
    pub fn find_swaps_with_tokens<'a>(
        swaps: &'a [ParsedSwap],
        token_a: &str,
        token_b: &str,
    ) -> Vec<&'a ParsedSwap> {
        swaps
            .iter()
            .filter(|s| {
                (s.token_in == token_a && s.token_out == token_b)
                    || (s.token_in == token_b && s.token_out == token_a)
            })
            .collect()
    }

    /// Find swaps on a specific DEX
    pub fn find_swaps_on_dex(swaps: &[ParsedSwap], dex: DexProtocol) -> Vec<&ParsedSwap> {
        swaps.iter().filter(|s| s.dex == dex).collect()
    }
}
