//! Main DEX parser that coordinates all protocol-specific parsers

use super::common::*;
use crate::types::FetchedTransaction;
use anyhow::Result;
use solana_transaction_status::{
    option_serializer::OptionSerializer, parse_instruction::ParsedInstruction, UiInstruction,
    UiParsedInstruction, UiTransactionTokenBalance,
};
use serde_json::Value;
use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use std::collections::HashMap;

/// Main DEX parser that can parse transactions from any supported DEX
pub struct DexParser;

impl DexParser {
    /// Fetch mint address from a token account via RPC
    async fn fetch_mint_from_account(
        rpc_client: &RpcClient,
        account_address: &str,
        cache: &mut HashMap<String, Option<String>>,
    ) -> Option<String> {
        // Check cache first
        if let Some(cached) = cache.get(account_address) {
            return cached.clone();
        }

        // Parse account address
        let pubkey = match Pubkey::from_str(account_address) {
            Ok(pk) => pk,
            Err(_) => {
                cache.insert(account_address.to_string(), None);
                return None;
            }
        };

        // Fetch account info
        let account = match rpc_client.get_account(&pubkey) {
            Ok(acc) => acc,
            Err(e) => {
                eprintln!("[DEBUG] Failed to fetch account {}: {}", &account_address[..8], e);
                cache.insert(account_address.to_string(), None);
                return None;
            }
        };

        // SPL Token account data format:
        // Bytes 0-31: mint (32 bytes)
        // Bytes 32-63: owner (32 bytes)
        // Bytes 64-71: amount (8 bytes)
        // ...
        if account.data.len() < 32 {
            eprintln!("[DEBUG] Account {} data too short: {} bytes", &account_address[..8], account.data.len());
            cache.insert(account_address.to_string(), None);
            return None;
        }

        // Extract mint (first 32 bytes)
        let mint_bytes: [u8; 32] = account.data[0..32].try_into().ok()?;
        let mint_pubkey = Pubkey::new_from_array(mint_bytes);
        let mint_str = mint_pubkey.to_string();

        eprintln!("[DEBUG] Fetched mint for {}: {}", &account_address[..8], &mint_str);
        cache.insert(account_address.to_string(), Some(mint_str.clone()));
        Some(mint_str)
    }

    /// Parse all swaps from a transaction
    pub async fn parse_transaction(
        tx: &FetchedTransaction,
        tx_index: usize,
        rpc_client: Option<&RpcClient>,
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

        // Detect swaps from token balance changes (if available)
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

        // If no swaps found from balances, try parsing from inner instructions
        if swaps.is_empty() {
            if let Some(detected) = Self::parse_from_inner_instructions(tx, tx_index, pre_balances, post_balances, rpc_client).await? {
                swaps.extend(detected);
            }
        }

        Ok(swaps)
    }

    /// Parse swaps from inner instructions (token transfers)
    async fn parse_from_inner_instructions(
        tx: &FetchedTransaction,
        tx_index: usize,
        pre_balances: Option<&Vec<UiTransactionTokenBalance>>,
        post_balances: Option<&Vec<UiTransactionTokenBalance>>,
        rpc_client: Option<&RpcClient>,
    ) -> Result<Option<Vec<ParsedSwap>>> {
        // Get inner instructions from metadata
        let inner_instructions = tx.meta.as_ref().and_then(|m| {
            match &m.inner_instructions {
                solana_transaction_status::option_serializer::OptionSerializer::Some(inner) => Some(inner),
                _ => None,
            }
        });

        let inner_instructions = match inner_instructions {
            Some(inner) => inner,
            None => return Ok(None),
        };

        // Get the transaction signer (first account)
        let signer = tx.signer().unwrap_or_else(|| "Unknown".to_string());

        // Build a map of token account address -> mint from balances
        // This helps resolve mints for "transfer" instructions (which don't include mint)
        use std::collections::HashMap;
        let mut account_to_mint: HashMap<String, String> = HashMap::new();

        // Get account keys to map indices to addresses
        let account_keys: Vec<String> = match &tx.transaction {
            solana_transaction_status::EncodedTransaction::Json(ui_tx) => {
                match &ui_tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().map(|k| k.pubkey.clone()).collect()
                    }
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.clone()
                    }
                }
            }
            _ => Vec::new(),
        };

        // Map account indices from balances to get account_address -> mint mapping
        if let Some(balances) = pre_balances {
            for balance in balances {
                if let Some(account_addr) = account_keys.get(balance.account_index as usize) {
                    account_to_mint.insert(account_addr.clone(), balance.mint.clone());
                }
            }
        }
        if let Some(balances) = post_balances {
            for balance in balances {
                if let Some(account_addr) = account_keys.get(balance.account_index as usize) {
                    account_to_mint.insert(account_addr.clone(), balance.mint.clone());
                }
            }
        }

        eprintln!("[DEBUG] Built account->mint map with {} entries", account_to_mint.len());

        // RPC cache for fetched mints
        let mut rpc_mint_cache: HashMap<String, Option<String>> = HashMap::new();

        // Parse token transfers from inner instructions
        let mut transfers = Vec::new();

        for inner_ix in inner_instructions {
            for instruction in &inner_ix.instructions {
                // Try to parse any instruction that might be a token transfer
                if let Some(mut transfer) = Self::try_parse_token_transfer(instruction) {
                    // If mint is missing, try to resolve it
                    if transfer.mint.is_none() {
                        // 1. Try from balance-based account_to_mint map
                        if let Some(mint) = account_to_mint.get(&transfer.source) {
                            eprintln!("[DEBUG] Resolved mint for source {} from balances: {}", &transfer.source[..8], mint);
                            transfer.mint = Some(mint.clone());
                        }
                        // If still None, try destination account
                        if transfer.mint.is_none() {
                            if let Some(mint) = account_to_mint.get(&transfer.destination) {
                                eprintln!("[DEBUG] Resolved mint for destination {} from balances: {}", &transfer.destination[..8], mint);
                                transfer.mint = Some(mint.clone());
                            }
                        }

                        // 2. If still None and RPC client available, fetch via RPC
                        if transfer.mint.is_none() && rpc_client.is_some() {
                            eprintln!("[DEBUG] Attempting RPC fetch for source account {}", &transfer.source[..8]);
                            if let Some(mint) = Self::fetch_mint_from_account(
                                rpc_client.unwrap(),
                                &transfer.source,
                                &mut rpc_mint_cache,
                            ).await {
                                eprintln!("[DEBUG] Resolved mint for source {} via RPC: {}", &transfer.source[..8], &mint);
                                transfer.mint = Some(mint);
                            } else if let Some(mint) = Self::fetch_mint_from_account(
                                rpc_client.unwrap(),
                                &transfer.destination,
                                &mut rpc_mint_cache,
                            ).await {
                                eprintln!("[DEBUG] Resolved mint for destination {} via RPC: {}", &transfer.destination[..8], &mint);
                                transfer.mint = Some(mint);
                            }
                        }
                    }
                    transfers.push(transfer);
                }
            }
        }

        eprintln!("[DEBUG] RPC cache size: {} entries", rpc_mint_cache.len());

        // If we found transfers, try to match them into swaps
        if transfers.is_empty() {
            eprintln!("[DEBUG] No token transfers found in inner instructions");
            return Ok(None);
        }

        eprintln!("[DEBUG] Found {} token transfers in inner instructions", transfers.len());
        for (i, t) in transfers.iter().enumerate() {
            eprintln!("[DEBUG]   Transfer {}: {} {} (auth: {:?}, src: {}, dst: {})",
                i, t.amount, t.mint.as_ref().unwrap_or(&"None".to_string()),
                t.authority, &t.source[..8], &t.destination[..8]);
        }

        // Group transfers by source/destination to find swap patterns
        let swaps = Self::match_transfers_to_swaps(&transfers, &signer, &tx.signature, tx_index)?;

        if swaps.is_empty() {
            Ok(None)
        } else {
            Ok(Some(swaps))
        }
    }

    /// Try to parse a token transfer from any instruction type
    fn try_parse_token_transfer(instruction: &UiInstruction) -> Option<TokenTransferInfo> {
        match instruction {
            UiInstruction::Parsed(ui_parsed_instruction) => {
                // UiParsedInstruction is an enum with Parsed and PartiallyDecoded variants
                match ui_parsed_instruction {
                    UiParsedInstruction::Parsed(parsed_instruction) => {
                        Self::parse_token_transfer_from_parsed(parsed_instruction)
                    }
                    UiParsedInstruction::PartiallyDecoded(_) => None,
                }
            }
            UiInstruction::Compiled(_) => {
                // For compiled instructions, we'd need to decode the instruction data
                // This is more complex and requires knowing the program's instruction format
                // For now, we'll skip compiled instructions
                None
            }
        }
    }

    /// Parse a token transfer from a parsed instruction
    fn parse_token_transfer_from_parsed(parsed: &solana_transaction_status::parse_instruction::ParsedInstruction) -> Option<TokenTransferInfo> {
        // ParsedInstruction has a parsed field as serde_json::Value
        let parsed_value = &parsed.parsed;

        // Token transfers have type "transfer" or "transferChecked"
        let transfer_type = parsed_value.get("type")?.as_str()?;
        if transfer_type != "transfer" && transfer_type != "transferChecked" {
            return None;
        }

        let info = parsed_value.get("info")?;

        let source = info.get("source")?.as_str()?.to_string();
        let destination = info.get("destination")?.as_str()?.to_string();
        let authority = info.get("authority").and_then(|a| a.as_str()).map(|s| s.to_string());
        let mint = info.get("mint").and_then(|m| m.as_str()).map(|s| s.to_string());

        // Amount can be a string or number
        let amount = if let Some(amt_str) = info.get("amount").and_then(|a| a.as_str()) {
            amt_str.parse::<u64>().ok()?
        } else {
            info.get("amount")?.as_u64()?
        };

        Some(TokenTransferInfo {
            source,
            destination,
            authority,
            mint,
            amount,
        })
    }

    /// Match token transfers to reconstruct swaps
    fn match_transfers_to_swaps(
        transfers: &[TokenTransferInfo],
        signer: &str,
        signature: &str,
        tx_index: usize,
    ) -> Result<Vec<ParsedSwap>> {
        let mut swaps = Vec::new();

        // Simple heuristic: look for pairs of transfers where:
        // 1. User sends token A (transfer FROM user's account)
        // 2. User receives token B (transfer TO user's account)
        // This indicates a swap of A for B

        // Group transfers by direction relative to signer
        let mut outgoing = Vec::new();  // User is authority/source
        let mut incoming = Vec::new();  // User is destination

        for transfer in transfers {
            // Check if user is the authority of this transfer (outgoing)
            if transfer.authority.as_ref().map_or(false, |auth| auth == signer) {
                outgoing.push(transfer);
            }

            // Check if user is the destination (incoming)
            if transfer.destination == signer ||
               transfer.authority.as_ref().map_or(false, |auth| auth == signer) {
                incoming.push(transfer);
            }
        }

        eprintln!("[DEBUG] Signer: {}", signer);
        eprintln!("[DEBUG] Outgoing transfers: {}", outgoing.len());
        eprintln!("[DEBUG] Incoming transfers: {}", incoming.len());

        // If we have both outgoing and incoming transfers, create a swap
        if !outgoing.is_empty() && !incoming.is_empty() {
            // Take the largest outgoing and incoming as the swap
            if let (Some(token_out_transfer), Some(token_in_transfer)) =
                (outgoing.iter().max_by_key(|t| t.amount), incoming.iter().max_by_key(|t| t.amount)) {

                if let (Some(token_in_mint), Some(token_out_mint)) =
                    (&token_in_transfer.mint, &token_out_transfer.mint) {

                    eprintln!("[DEBUG] Creating swap: {} {} -> {} {}",
                        token_out_transfer.amount, token_out_mint,
                        token_in_transfer.amount, token_in_mint);

                    swaps.push(ParsedSwap {
                        dex: DexProtocol::Unknown,  // Can't determine DEX from transfers alone
                        program_id: "Unknown".to_string(),
                        pool: "Unknown".to_string(),
                        user: signer.to_string(),
                        token_in: token_out_mint.clone(),  // User sent this out
                        token_out: token_in_mint.clone(),  // User received this
                        amount_in: token_out_transfer.amount,
                        amount_out: token_in_transfer.amount,
                        min_amount_out: None,
                        price_before: None,
                        price_after: None,
                        price_impact: None,
                        signature: signature.to_string(),
                        tx_index,
                        instruction_index: 0,
                    });
                }
            }
        } else {
            eprintln!("[DEBUG] Cannot create swap: outgoing={}, incoming={}", outgoing.len(), incoming.len());
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
    pub async fn parse_block(
        transactions: &[FetchedTransaction],
        rpc_client: Option<&RpcClient>,
    ) -> Vec<ParsedSwap> {
        let mut all_swaps = Vec::new();

        for (idx, tx) in transactions.iter().enumerate() {
            if let Ok(swaps) = Self::parse_transaction(tx, idx, rpc_client).await {
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
