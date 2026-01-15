use crate::types::*;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_transaction_status::{
    EncodedTransaction, TransactionDetails, UiConfirmedBlock, UiTransactionEncoding,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{Instant, sleep};
use tracing::{debug, error, warn};

pub struct BlockFetcher {
    rpc_client: Arc<RpcClient>,
    config: FetcherConfig,
    rate_limiter: RateLimiter,
}

impl BlockFetcher {
    /// create fetcher with config
    pub fn new(config: FetcherConfig) -> Self {
        let rpc_client = RpcClient::new_with_timeout_and_commitment(
            config.rpc_url.clone(),
            Duration::from_secs(config.timeout_secs),
            CommitmentConfig::confirmed(),
        );

        let rate_limiter = RateLimiter::new(config.rate_limit);

        Self {
            rpc_client: Arc::new(rpc_client),
            config,
            rate_limiter,
        }
    }

    /// create with default config
    pub fn new_default() -> Self {
        Self::new(FetcherConfig::default())
    }

    /// fetch single block by slot with retries
    pub async fn fetch_block(&self, slot: u64) -> Result<FetchedBlock> {
        let mut retries = 0;

        loop {
            // Apply rate limiting
            self.rate_limiter.acquire().await;

            match self.fetch_block_once(slot).await {
                Ok(block) => {
                    debug!("successfully fetched block at slot {}", slot);
                    return Ok(block);
                }
                Err(e) => {
                    retries += 1;

                    if retries > self.config.max_retries {
                        error!("max retries exceeded for slot {}: {:?}", slot, e);
                        return Err(FetcherError::MaxRetriesExceeded { slot });
                    }

                    warn!(
                        "failed to fetch slot {} (attempt {}/{}): {:?}",
                        slot, retries, self.config.max_retries, e
                    );

                    // backoff
                    let delay = self.config.retry_delay_ms * (2_u64.pow(retries - 1));
                    sleep(Duration::from_millis(delay)).await;
                }
            }
        }
    }

    /// fetch block without retries
    async fn fetch_block_once(&self, slot: u64) -> Result<FetchedBlock> {
        let rpc_client = Arc::clone(&self.rpc_client);

        // run blocking RPC call in separate thread
        let block = tokio::task::spawn_blocking(move || {
            rpc_client.get_block_with_config(
                slot,
                solana_client::rpc_config::RpcBlockConfig {
                    encoding: Some(UiTransactionEncoding::JsonParsed),
                    transaction_details: Some(TransactionDetails::Full),
                    rewards: Some(true),
                    commitment: Some(CommitmentConfig::confirmed()),
                    max_supported_transaction_version: Some(0),
                },
            )
        })
        .await?;

        match block {
            Ok(block) => self.format_block(block, slot),
            Err(e) => {
                // check if skipped slot
                let error_msg = e.to_string();
                if error_msg.contains("not available")
                    || error_msg.contains("skipped")
                    || error_msg.contains("was skipped")
                {
                    Err(FetcherError::BlockNotAvailable { slot })
                } else {
                    Err(FetcherError::RpcError(e))
                }
            }
        }
    }

    /// format block
    fn format_block(&self, block: UiConfirmedBlock, slot: u64) -> Result<FetchedBlock> {
        let transactions = block
            .transactions
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, tx)| {
                // extract sig from the tx
                let signature = Self::extract_sig(&tx.transaction);

                FetchedTransaction {
                    signature,
                    transaction: tx.transaction,
                    meta: tx.meta,
                    index,
                }
            })
            .collect();

        let rewards = block
            .rewards
            .unwrap_or_default()
            .into_iter()
            .map(|r| Reward {
                pubkey: r.pubkey,
                lamports: r.lamports,
                post_balance: r.post_balance,
                reward_type: r.reward_type.map(|rt| format!("{:?}", rt)),
                commission: r.commission,
            })
            .collect();

        Ok(FetchedBlock {
            slot,
            blockhash: block.blockhash,
            previous_blockhash: block.previous_blockhash,
            parent_slot: block.parent_slot,
            block_time: block.block_time,
            transactions,
            rewards,
            block_height: block.block_height,
        })
    }

    /// extract sig from tx
    fn extract_sig(tx: &EncodedTransaction) -> String {
        match tx {
            EncodedTransaction::Json(ui_tx) => ui_tx
                .signatures
                .first()
                .cloned()
                .unwrap_or_else(|| "unknown".to_string()),
            EncodedTransaction::LegacyBinary(_) => "binary".to_string(),
            EncodedTransaction::Binary(_, _) => "binary".to_string(),
            EncodedTransaction::Accounts(_) => "accounts".to_string(),
        }
    }

    /// get current slot from RPC
    pub async fn get_current_slot(&self) -> Result<u64> {
        let rpc_client = Arc::clone(&self.rpc_client);

        let slot = tokio::task::spawn_blocking(move || rpc_client.get_slot()).await??;

        Ok(slot)
    }
}

/// simple token bucket rate limiter
struct RateLimiter {
    permits_per_second: u32,
    last_refill: Arc<tokio::sync::Mutex<Instant>>,
    available_permits: Arc<tokio::sync::Mutex<u32>>,
}

impl RateLimiter {
    fn new(permits_per_second: u32) -> Self {
        Self {
            permits_per_second,
            last_refill: Arc::new(tokio::sync::Mutex::new(Instant::now())),
            available_permits: Arc::new(tokio::sync::Mutex::new(permits_per_second)),
        }
    }

    async fn acquire(&self) {
        loop {
            let mut permits = self.available_permits.lock().await;
            let mut last_refill = self.last_refill.lock().await;

            // Refill permits based on elapsed time
            let elapsed = last_refill.elapsed();
            let elapsed_secs = elapsed.as_secs_f64();
            let permits_to_add = (elapsed_secs * self.permits_per_second as f64) as u32;

            if permits_to_add > 0 {
                *permits = (*permits + permits_to_add).min(self.permits_per_second);
                *last_refill = Instant::now();
            }

            if *permits > 0 {
                *permits -= 1;
                return;
            }

            // wait before retry
            drop(permits);
            drop(last_refill);
            sleep(Duration::from_millis(100)).await;
        }
    }
}
