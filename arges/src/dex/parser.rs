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
        let ui_transaction = match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => ui_tx,
            _ => return Ok(swaps), // Skip non-JSON encoded transactions
        };

        // TODO: Ideally, we would parse instructions to find DEX calls
        // However, many transactions lack pre/post token balance metadata
        // This requires parsing inner instructions and token transfer logs
        // which is complex and error-prone
        let _signer = tx.signer().unwrap_or_else(|| "Unknown".to_string());

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
            let detected = Self::detect_swap_from_balances(
                pre,
                post,
                "Unknown",
                &tx.signature,
                tx_index,
                0,
            )?;
            swaps.extend(detected);
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
            let swaps = Self::detect_swap_from_balances(
                pre,
                post,
                "Unknown",
                signature,
                tx_index,
                instruction_index,
            )?;
            // Return the first swap if any found (this function expects Option<ParsedSwap>)
            if let Some(swap) = swaps.into_iter().next() {
                return Ok(Some(swap));
            }
        }

        Ok(None)
    }

    /// Detect swaps by analyzing token balance changes
    ///
    /// Returns ALL detected swaps in the transaction (can be multiple users)
    fn detect_swap_from_balances(
        pre_balances: &[UiTransactionTokenBalance],
        post_balances: &[UiTransactionTokenBalance],
        program: &str,
        signature: &str,
        tx_index: usize,
        instruction_index: usize,
    ) -> Result<Vec<ParsedSwap>> {
        use std::collections::HashMap;

        let mut detected_swaps = Vec::new();

        // Find balance changes grouped by owner
        let mut changes_by_owner: HashMap<String, Vec<(String, i128)>> = HashMap::new(); // owner -> [(mint, change)]

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
                    let owner = post.owner.clone().unwrap_or("Unknown".to_string());
                    changes_by_owner
                        .entry(owner)
                        .or_default()
                        .push((post.mint.clone(), change));
                }
            }
        }

        // For each owner, check if they have a swap pattern
        // CRITICAL: Only match balance changes from the SAME owner
        // This prevents false positives from mixing balance changes across different users
        for (owner, owner_changes) in changes_by_owner.iter() {
            let decreases: Vec<_> = owner_changes.iter().filter(|(_, c)| *c < 0).collect();
            let increases: Vec<_> = owner_changes.iter().filter(|(_, c)| *c > 0).collect();

            // Detect different swap patterns:
            // 1. Simple swap: 1 decrease + 1 increase (A → B)
            // 2. Multi-hop swap: 1 decrease + N increases (A → B → C → D, net: A → D)
            // 3. Aggregator route: multiple intermediate tokens

            if decreases.is_empty() || increases.is_empty() {
                continue; // No swap pattern
            }

            // For single decrease and single increase: straightforward swap
            if decreases.len() == 1 && increases.len() == 1 {
                let (token_in, amount_in_signed) = decreases[0];
                let (token_out, amount_out) = increases[0];

                let dex = DexProtocol::from_program_id(program);

                detected_swaps.push(ParsedSwap {
                    dex,
                    program_id: program.to_string(),
                    pool: "Unknown".to_string(),
                    user: owner.clone(),
                    token_in: token_in.clone(),
                    token_out: token_out.clone(),
                    amount_in: amount_in_signed.unsigned_abs() as u64,
                    amount_out: *amount_out as u64,
                    min_amount_out: None,
                    price_before: None,
                    price_after: None,
                    price_impact: None,
                    signature: signature.to_string(),
                    tx_index,
                    instruction_index,
                });
            }
            // Multi-hop or complex swap: find net input/output tokens
            // For multi-hop (A → B → C), B will have both +/-, so we find tokens with only + or only -
            else if decreases.len() >= 1 && increases.len() >= 1 {
                // Find tokens that only decrease (input tokens)
                // Find tokens that only increase (output tokens)
                let mut token_nets: std::collections::HashMap<&String, i128> = std::collections::HashMap::new();

                for (token, change) in owner_changes {
                    *token_nets.entry(token).or_insert(0) += change;
                }

                // Find primary input (largest decrease) and output (largest increase)
                let mut max_decrease: Option<(&String, i128)> = None;
                let mut max_increase: Option<(&String, i128)> = None;

                for (token, net_change) in token_nets.iter() {
                    if *net_change < 0 {
                        if max_decrease.map_or(true, |(_, amt)| net_change < &amt) {
                            max_decrease = Some((*token, *net_change));
                        }
                    } else if *net_change > 0 {
                        if max_increase.map_or(true, |(_, amt)| net_change > &amt) {
                            max_increase = Some((*token, *net_change));
                        }
                    }
                }

                // Create swap from primary input to primary output
                if let (Some((token_in, amount_in_net)), Some((token_out, amount_out_net))) =
                    (max_decrease, max_increase)
                {
                    let dex = DexProtocol::from_program_id(program);

                    detected_swaps.push(ParsedSwap {
                        dex,
                        program_id: program.to_string(),
                        pool: "Unknown".to_string(),
                        user: owner.clone(),
                        token_in: (*token_in).clone(),
                        token_out: (*token_out).clone(),
                        amount_in: amount_in_net.unsigned_abs() as u64,
                        amount_out: amount_out_net as u64,
                        min_amount_out: None,
                        price_before: None,
                        price_after: None,
                        price_impact: None,
                        signature: signature.to_string(),
                        tx_index,
                        instruction_index,
                    });
                }
            }
        }

        Ok(detected_swaps)
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
