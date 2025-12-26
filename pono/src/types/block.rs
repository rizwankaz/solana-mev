use serde::{Deserialize, Serialize};
use solana_transaction_status::{
    EncodedTransaction, UiTransactionStatusMeta,
};

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
    
    /// total cus consumed
    pub fn total_compute_units(&self) -> u64 {
        self.transactions
            .iter()
            .filter_map(|tx| tx.compute_units_consumed())
            .sum()
    }
    
    /// total fees paid
    pub fn total_fees(&self) -> u64 {
        self.transactions
            .iter()
            .filter_map(|tx| tx.fee())
            .sum()
    }
}

/// tx in block
#[derive(Debug, Clone)]
pub struct FetchedTransaction {
    pub signature: String,
    pub transaction: EncodedTransaction,
    pub meta: Option<UiTransactionStatusMeta>,
    pub index: usize,
}

impl FetchedTransaction {
    /// successful?
    pub fn is_success(&self) -> bool {
        self.meta
            .as_ref()
            .map(|m| m.err.is_none())
            .unwrap_or(false)
    }
    
    /// cus
    pub fn compute_units_consumed(&self) -> Option<u64> {
        self.meta.as_ref().and_then(|m| {
            match m.compute_units_consumed {
                solana_transaction_status::option_serializer::OptionSerializer::Some(units) => Some(units),
                solana_transaction_status::option_serializer::OptionSerializer::None => None,
                solana_transaction_status::option_serializer::OptionSerializer::Skip => None,
            }
        })
    }
    
    /// fee
    pub fn fee(&self) -> Option<u64> {
        self.meta.as_ref().map(|m| m.fee)
    }
    
    /// signer
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

    /// vote?
    pub fn is_vote(&self) -> bool {
        const VOTE_PROGRAM_ID: &str = "Vote111111111111111111111111111111111111111";

        match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().any(|key| key.pubkey == VOTE_PROGRAM_ID)
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.iter().any(|key| key == VOTE_PROGRAM_ID)
                    },
                }
            },
            _ => false,
        }
    }

    /// jito
    pub fn jito_tip(&self) -> Option<u64> {
        // do you think they'll have more tip accounts
        const JITO_TIP_ACCOUNTS: &[&str] = &[
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5",
            "HFqU5x63VTqvQss8hp11i4wVV8bD44PvwucfZ2bU7gRe",
            "Cw8CFyM9FkoMi7K7Crf6HNQqf4uEMzpKw6QNghXLvLkY",
            "ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49",
            "DfXygSm4jCyNCybVYYK6DwvWqjKee8pbDmJGcLWNDXjh",
            "ADuUkR4vqLUMWXxW9gh6D6L8pMSawimctcNZ5pGwDcEt",
            "DttWaMuVvTiduZRnguLF7jNxTgiMBZ1hyAumKUiL2KRL",
            "3AVi9Tg9Uo68tJfuvoKvqKNWKkC5wPdSSdeBnizKZ6jT",
        ];

        let meta = self.meta.as_ref()?;

        let account_keys = match &self.transaction {
            EncodedTransaction::Json(tx) => {
                match &tx.message {
                    solana_transaction_status::UiMessage::Parsed(parsed) => {
                        parsed.account_keys.iter().map(|k| k.pubkey.as_str()).collect::<Vec<_>>()
                    },
                    solana_transaction_status::UiMessage::Raw(raw) => {
                        raw.account_keys.iter().map(|k| k.as_str()).collect::<Vec<_>>()
                    },
                }
            },
            _ => return None,
        };

        let pre_balances = &meta.pre_balances;
        let post_balances = &meta.post_balances;

        for (idx, account) in account_keys.iter().enumerate() {
            if JITO_TIP_ACCOUNTS.contains(account) {
                if let (Some(&pre), Some(&post)) = (pre_balances.get(idx), post_balances.get(idx)) {
                    if post > pre {
                        return Some(post - pre);
                    }
                }
            }
        }

        None
    }
}

/// block reward
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: u64,
    pub reward_type: Option<String>,
    pub commission: Option<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum FetcherError {
    #[error("RPC error: {0}")]
    RpcError(#[from] solana_client::client_error::ClientError),
    
    #[error("block not available at slot {slot}")]
    BlockNotAvailable { slot: u64 },
    
    // please for the love of god get a better api
    #[error("rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("max retries exceeded for slot {slot}")]
    MaxRetriesExceeded { slot: u64 },
    
    #[error("invalid block data: {0}")]
    InvalidBlockData(String),
    
    #[error("join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

pub type Result<T> = std::result::Result<T, FetcherError>;