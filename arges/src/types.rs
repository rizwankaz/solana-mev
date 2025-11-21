use serde::{Deserialize, Serialize};
use solana_transaction_status::{
    EncodedTransaction, UiTransactionStatusMeta,
};
use crate::mev::{MevAnalyzer, MevEvent, MevSummary};

/// block fetcher config
#[derive(Debug, Clone)]
pub struct FetcherConfig {
    /// RPC endpoint URL
    pub rpc_url: String,
    
    /// maximum retries for failed requests
    pub max_retries: u32,
    
    /// delay between retries
    pub retry_delay_ms: u64,
    
    /// rate limit: max requests per second
    pub rate_limit: u32,
    
    /// request timeout
    pub timeout_secs: u64,
}

impl Default for FetcherConfig {
    fn default() -> Self {
        Self {
            rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
            max_retries: 3,
            retry_delay_ms: 1000,
            rate_limit: 10,
            timeout_secs: 30,
        }
    }
}

/// fetched block with metadata
#[derive(Debug, Clone)]
pub struct FetchedBlock {
    pub slot: u64,
    pub blockhash: String,
    pub previous_blockhash: String,
    pub parent_slot: u64,
    pub block_time: Option<i64>,
    pub transactions: Vec<FetchedTransaction>,
    pub rewards: Vec<Reward>,
    pub block_height: Option<u64>,
}

impl FetchedBlock {
    /// get block timestamp
    pub fn timestamp(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.block_time.and_then(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
        })
    }

    /// count successful txs
    pub fn successful_tx_count(&self) -> usize {
        self.transactions
            .iter()
            .filter(|tx| tx.is_success())
            .count()
    }

    /// count failed txs
    pub fn failed_tx_count(&self) -> usize {
        self.transactions
            .iter()
            .filter(|tx| !tx.is_success())
            .count()
    }

    /// total cus consumed
    pub fn total_compute_units(&self) -> u64 {
        self.transactions
            .iter()
            .filter_map(|tx| tx.compute_units_consumed())
            .sum()
    }

    /// Total fees paid
    pub fn total_fees(&self) -> u64 {
        self.transactions
            .iter()
            .filter_map(|tx| tx.fee())
            .sum()
    }

    /// Analyze MEV in the block
    pub fn analyze_mev(&self) -> MevSummary {
        let mut summary = MevSummary::new();

        for tx in &self.transactions {
            if let Some(event) = tx.analyze_mev() {
                summary.add_event(&event);
            }
        }

        summary
    }
}

/// A transaction within a block
#[derive(Debug, Clone)]
pub struct FetchedTransaction {
    pub signature: String,
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
    pub index: usize,
}

impl FetchedTransaction {
    /// Check if transaction succeeded
    pub fn is_success(&self) -> bool {
        self.meta
            .as_ref()
            .map(|m| m.err.is_none())
            .unwrap_or(false)
    }

    /// Get compute units consumed
    pub fn compute_units_consumed(&self) -> Option<u64> {
        self.meta.as_ref().and_then(|m| {
            // Handle OptionSerializer by converting to Option
            match m.compute_units_consumed {
                solana_transaction_status::option_serializer::OptionSerializer::Some(units) => Some(units),
                solana_transaction_status::option_serializer::OptionSerializer::None => None,
                solana_transaction_status::option_serializer::OptionSerializer::Skip => None,
            }
        })
    }

    /// Get transaction fee
    pub fn fee(&self) -> Option<u64> {
        self.meta.as_ref().map(|m| m.fee)
    }

    /// Get priority fee (compute unit price * compute units consumed)
    pub fn priority_fee(&self) -> Option<u64> {
        let compute_unit_price = self.get_compute_unit_price()?;
        let compute_units = self.compute_units_consumed()?;

        // Compute unit price is in micro-lamports, so divide by 1_000_000
        Some((compute_unit_price as u128 * compute_units as u128 / 1_000_000) as u64)
    }

    /// Extract compute unit price from ComputeBudget instructions
    fn get_compute_unit_price(&self) -> Option<u64> {
        const COMPUTE_BUDGET_PROGRAM: &str = "ComputeBudget111111111111111111111111111111";

        match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        // Try to find ComputeBudget instruction in parsed message
                        for ix in &parsed.instructions {
                            match ix {
                                // Handle fully parsed instructions
                                solana_transaction_status::UiInstruction::Parsed(parsed_ui_ix) => {
                                    if let solana_transaction_status::UiParsedInstruction::Parsed(parsed_ix) = parsed_ui_ix {
                                        if parsed_ix.program_id == COMPUTE_BUDGET_PROGRAM
                                            && parsed_ix.program == "compute-budget" {
                                            // Check if this is SetComputeUnitPrice instruction
                                            if let Some(serde_json::Value::String(ix_type)) = parsed_ix.parsed.get("type") {
                                                if ix_type == "setComputeUnitPrice" {
                                                    // Extract the microLamports value
                                                    if let Some(serde_json::Value::Object(info)) = parsed_ix.parsed.get("info") {
                                                        if let Some(serde_json::Value::Number(price)) = info.get("microLamports") {
                                                            return price.as_u64();
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else if let solana_transaction_status::UiParsedInstruction::PartiallyDecoded(partial) = parsed_ui_ix {
                                        // Handle partially decoded instructions
                                        if partial.program_id == COMPUTE_BUDGET_PROGRAM {
                                            if let Ok(data) = bs58::decode(&partial.data).into_vec() {
                                                if data.len() == 9 && data[0] == 3 {
                                                    let micro_lamports = u64::from_le_bytes([
                                                        data[1], data[2], data[3], data[4],
                                                        data[5], data[6], data[7], data[8]
                                                    ]);
                                                    return Some(micro_lamports);
                                                }
                                            }
                                        }
                                    }
                                },
                                // Handle compiled instructions in parsed message
                                solana_transaction_status::UiInstruction::Compiled(compiled_ix) => {
                                    if let Some(program_id) = parsed.account_keys.get(compiled_ix.program_id_index as usize) {
                                        if program_id.pubkey == COMPUTE_BUDGET_PROGRAM {
                                            if let Ok(data) = bs58::decode(&compiled_ix.data).into_vec() {
                                                if data.len() == 9 && data[0] == 3 {
                                                    let micro_lamports = u64::from_le_bytes([
                                                        data[1], data[2], data[3], data[4],
                                                        data[5], data[6], data[7], data[8]
                                                    ]);
                                                    return Some(micro_lamports);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        // For raw messages, decode ComputeBudget instructions
                        for compiled_ix in &raw.instructions {
                            if let Some(program_id) = raw.account_keys.get(compiled_ix.program_id_index as usize) {
                                if program_id == COMPUTE_BUDGET_PROGRAM {
                                    // Decode the instruction data
                                    // SetComputeUnitPrice has discriminator 3 and then u64 micro_lamports
                                    if let Ok(data) = bs58::decode(&compiled_ix.data).into_vec() {
                                        if data.len() == 9 && data[0] == 3 {
                                            // Extract u64 from bytes 1-8 (little-endian)
                                            let micro_lamports = u64::from_le_bytes([
                                                data[1], data[2], data[3], data[4],
                                                data[5], data[6], data[7], data[8]
                                            ]);
                                            return Some(micro_lamports);
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            },
            _ => {}
        }

        None
    }

    /// Get signer (first account in transaction)
    pub fn signer(&self) -> Option<String> {
        match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.first().map(|key| key.pubkey.clone())
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.first().cloned()
                    },
                }
            },
            _ => None,
        }
    }

    /// Extract all instructions from transaction (including inner instructions from CPIs)
    fn get_instructions(&self) -> Vec<solana_transaction_status::UiInstruction> {
        let mut all_instructions = Vec::new();

        // Get top-level instructions
        match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        all_instructions.extend(parsed.instructions.clone());
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        // Convert UiCompiledInstruction to UiInstruction
                        all_instructions.extend(
                            raw.instructions.iter()
                                .map(|compiled| solana_transaction_status::UiInstruction::Compiled(compiled.clone()))
                        );
                    },
                }
            },
            _ => {}
        }

        // Get inner instructions (CPI calls - this is where DEX logic happens)
        if let Some(meta) = &self.meta {
            use solana_transaction_status::option_serializer::OptionSerializer;

            if let OptionSerializer::Some(inner_instructions) = &meta.inner_instructions {
                for inner_ix_set in inner_instructions {
                    all_instructions.extend(inner_ix_set.instructions.clone());
                }
            }
        }

        all_instructions
    }

    /// Extract account keys from transaction message
    fn get_account_keys(&self) -> Vec<String> {
        match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().map(|key| key.pubkey.clone()).collect()
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.clone()
                    },
                }
            },
            _ => Vec::new(),
        }
    }

    /// Get pre and post balances
    fn get_balances(&self) -> (Vec<u64>, Vec<u64>) {
        let meta = match &self.meta {
            Some(m) => m,
            None => return (Vec::new(), Vec::new()),
        };

        (meta.pre_balances.clone(), meta.post_balances.clone())
    }

    /// Get pre and post token balances
    fn get_token_balances(&self) -> (Vec<solana_transaction_status::UiTransactionTokenBalance>, Vec<solana_transaction_status::UiTransactionTokenBalance>) {
        let meta = match &self.meta {
            Some(m) => m,
            None => return (Vec::new(), Vec::new()),
        };

        let pre_token_balances = match &meta.pre_token_balances {
            solana_transaction_status::option_serializer::OptionSerializer::Some(balances) => balances.clone(),
            _ => Vec::new(),
        };

        let post_token_balances = match &meta.post_token_balances {
            solana_transaction_status::option_serializer::OptionSerializer::Some(balances) => balances.clone(),
            _ => Vec::new(),
        };

        (pre_token_balances, post_token_balances)
    }

    /// Analyze this transaction for MEV patterns
    pub fn analyze_mev(&self) -> Option<MevEvent> {
        // Skip failed transactions entirely - they don't represent successful MEV activity
        if !self.is_success() {
            return None;
        }

        let instructions = self.get_instructions();
        let account_keys = self.get_account_keys();
        let (pre_balances, post_balances) = self.get_balances();
        let (pre_token_balances, post_token_balances) = self.get_token_balances();
        let signer = self.signer();

        MevAnalyzer::analyze_transaction(
            &self.signature,
            signer,
            &instructions,
            &account_keys,
            true, // always true now since we filter failed transactions above
            &pre_balances,
            &post_balances,
            &pre_token_balances,
            &post_token_balances,
        )
    }
}

/// Block reward
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: u64,
    pub reward_type: Option<String>,
    pub commission: Option<u8>,
}

/// Error types
#[derive(Debug, thiserror::Error)]
pub enum FetcherError {
    #[error("RPC error: {0}")]
    RpcError(#[from] solana_client::client_error::ClientError),
    
    #[error("Block not available at slot {slot}")]
    BlockNotAvailable { slot: u64 },
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Max retries exceeded for slot {slot}")]
    MaxRetriesExceeded { slot: u64 },
    
    #[error("Invalid block data: {0}")]
    InvalidBlockData(String),
    
    #[error("Join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, FetcherError>;