use dashmap::DashMap;
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use solana_sdk::commitment_config::CommitmentConfig;

/// Pyth price feed information for major Solana tokens
/// Source: https://pyth.network/developers/price-feed-ids#solana-mainnet
const PYTH_FEEDS: &[(&str, &str, &str)] = &[
    // (Mint Address, Feed ID, On-chain Price Account)
    // Native SOL
    ("So11111111111111111111111111111111111111112",
     "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d",
     "H6ARHf6YXhGYeQfUzQNGk6rDNnLBQKrenN712K4QJGVX"), // SOL/USD
    // USDC
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
     "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a",
     "Gnt27xtC473ZT2Mw5u8wZ68Z3gULkSTb5DuxJy7eJotD"), // USDC/USD
    // USDT
    ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
     "0x2b89b9dc8fdf9f34709a5b106b472f0f39bb6ca9ce04b0fd7f2e971688e2e53b",
     "3vxLXJqLqF3JG5TCbYycbKWRBbCJQLxQmBGCkyqEEefL"), // USDT/USD
    // BONK
    ("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263",
     "0x72b021217ca3fe68922a19aaf990109cb9d84e9ad004b4d2025ad6f529314419",
     "8ihFLu5FimgTQ1Unh4dVyEHUGodJ5gJQCrQf4KUVB9bN"), // BONK/USD
    // JTO
    ("jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL",
     "0xb43660a5f790c69354b0729a5ef9d50d68f1df92107540210b9cccba1f947cc2",
     "D8UUgr8a3aR3yUeHLu7v8FWK7E8Y5sSU7qrYBXUJXBQ5"), // JTO/USD
    // PYTH
    ("HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3",
     "0x0bbf28e9a841a1cc788f6a361b17ca072d0ea3098a1e5df1c3922d06719579ff",
     "nrYkQQQur7z8rYTST3G9GqATviK5SxTDkrqd21MW6Ue"), // PYTH/USD
    // JUP
    ("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN",
     "0x0a0408d619e9380abad35060f9192039ed5042fa6f82301d0e48bb52be830996",
     "g6eRCbboSwK4tSWngn773RCMexr1APQr4uA9bGZBYfo"), // JUP/USD
    // WIF
    ("EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm",
     "0x4ca4beeca86f0d164160323817a4e42b10010a724c2217c6ee41b54cd4cc61fc",
     "6x6KfE7nY4o1ag1Ru8LnwfR64BTfWyMzFE2s3dPhuMdQ"), // WIF/USD
];

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client for fetching historical token prices via Pyth on-chain accounts
pub struct OracleClient {
    rpc_client: Arc<solana_client::rpc_client::RpcClient>,
    price_cache: Arc<DashMap<String, PriceData>>,
    slot: u64,
    timestamp: i64,
    pyth_account_map: HashMap<String, String>,  // mint -> price account
}

impl OracleClient {
    pub fn new(slot: u64, timestamp: i64, rpc_url: String) -> Self {
        // Build the Pyth account map (mint -> on-chain price account)
        let pyth_account_map: HashMap<String, String> = PYTH_FEEDS.iter()
            .map(|(mint, _feed_id, account)| (mint.to_string(), account.to_string()))
            .collect();

        let rpc_client = Arc::new(solana_client::rpc_client::RpcClient::new(rpc_url));

        Self {
            rpc_client,
            price_cache: Arc::new(DashMap::new()),
            slot,
            timestamp,
            pyth_account_map,
        }
    }

    /// Batch fetch historical prices for multiple mints using Pyth on-chain accounts
    pub async fn batch_get_prices(&self, mints: &[&str]) -> Vec<(String, f64)> {
        if mints.is_empty() {
            return Vec::new();
        }

        // Separate cached and uncached mints
        let mut results = Vec::with_capacity(mints.len());
        let mut uncached_mints = Vec::new();

        for &mint in mints {
            if let Some(cached) = self.price_cache.get(mint) {
                results.push((mint.to_string(), cached.price_usd));
            } else {
                uncached_mints.push(mint);
            }
        }

        // Fetch uncached prices from Pyth on-chain accounts at the specific slot
        if !uncached_mints.is_empty() {
            let fetched = self.fetch_pyth_onchain_prices(&uncached_mints);

            for (mint, price) in fetched {
                // Cache the price
                self.price_cache.insert(
                    mint.clone(),
                    PriceData {
                        price_usd: price,
                        timestamp: self.timestamp,
                    },
                );
                results.push((mint, price));
            }
        }

        results
    }

    /// Get USD price for a token at the slot timestamp (single fetch)
    pub async fn get_price_usd(&self, mint: &str) -> Result<f64> {
        // Check cache first
        if let Some(cached) = self.price_cache.get(mint) {
            return Ok(cached.price_usd);
        }

        // Fetch from Pyth on-chain account (single token, historical price)
        let prices = self.fetch_pyth_onchain_prices(&[mint]);

        let price = prices.first()
            .map(|(_, p)| *p)
            .unwrap_or(0.0);

        // Cache the price
        self.price_cache.insert(
            mint.to_string(),
            PriceData {
                price_usd: price,
                timestamp: self.timestamp,
            },
        );

        Ok(price)
    }

    /// Fetch historical prices from Pyth on-chain accounts at specific slot
    /// Queries Pyth price accounts directly from Solana at the target slot
    fn fetch_pyth_onchain_prices(&self, mints: &[&str]) -> Vec<(String, f64)> {
        use pyth_sdk_solana::state::load_price_account;
        use solana_sdk::pubkey::Pubkey;
        use std::str::FromStr;

        tracing::debug!(
            "Fetching historical prices for {} tokens from Pyth on-chain at slot {}",
            mints.len(),
            self.slot
        );

        let mut results = Vec::with_capacity(mints.len());

        for &mint in mints {
            // Check if we have a Pyth price account for this mint
            let price_account_str = match self.pyth_account_map.get(mint) {
                Some(account) => account,
                None => {
                    tracing::warn!("No Pyth price account for token: {}", mint);
                    results.push((mint.to_string(), 0.0));
                    continue;
                }
            };

            // Parse the price account pubkey
            let price_account_pubkey = match Pubkey::from_str(price_account_str) {
                Ok(pubkey) => pubkey,
                Err(e) => {
                    tracing::error!("Invalid Pyth price account pubkey for {}: {:?}", mint, e);
                    results.push((mint.to_string(), 0.0));
                    continue;
                }
            };

            // Get account info at the specific slot
            let account_data = match self.rpc_client.get_account_with_commitment(
                &price_account_pubkey,
                CommitmentConfig {
                    commitment: solana_sdk::commitment_config::CommitmentLevel::Confirmed,
                },
            ) {
                Ok(response) => {
                    if let Some(account) = response.value {
                        account.data
                    } else {
                        tracing::warn!("Pyth price account not found for {}", mint);
                        results.push((mint.to_string(), 0.0));
                        continue;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to fetch Pyth price account for {}: {:?}", mint, e);
                    results.push((mint.to_string(), 0.0));
                    continue;
                }
            };

            // Parse the Pyth price account data
            let price_account: &pyth_sdk_solana::state::SolanaPriceAccount = match load_price_account(&account_data) {
                Ok(account) => account,
                Err(e) => {
                    tracing::error!("Failed to parse Pyth price account for {}: {:?}", mint, e);
                    results.push((mint.to_string(), 0.0));
                    continue;
                }
            };

            // Get the current price
            let current_price = price_account.to_price_feed(&price_account_pubkey).get_price_unchecked();
            let price_usd = current_price.price as f64 * 10_f64.powi(current_price.expo);

            tracing::debug!(
                "Historical price for {} at slot {}: ${} (conf: ±{})",
                mint,
                self.slot,
                price_usd,
                current_price.conf as f64 * 10_f64.powi(current_price.expo)
            );

            results.push((mint.to_string(), price_usd));
        }

        let successful_prices = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::info!(
            "Fetched {}/{} historical prices successfully from Pyth on-chain",
            successful_prices,
            mints.len()
        );

        results
    }

    /// Calculate USD value from token amount
    pub async fn calculate_usd_value(&self, mint: &str, amount: f64, decimals: u8) -> Result<f64> {
        let price = self.get_price_usd(mint).await?;
        let adjusted_amount = amount / 10_f64.powi(decimals as i32);
        Ok(adjusted_amount * price)
    }
}
