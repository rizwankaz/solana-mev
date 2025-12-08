use dashmap::DashMap;
use reqwest;
use serde::Deserialize;
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;

/// Common Pyth price feed IDs for major Solana tokens
/// Source: https://pyth.network/developers/price-feed-ids#solana-mainnet
const PYTH_FEEDS: &[(&str, &str)] = &[
    // Native SOL
    ("So11111111111111111111111111111111111111112", "0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d"),
    // USDC
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a"),
    // USDT
    ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", "0x2b89b9dc8fdf9f34709a5b106b472f0f39bb6ca9ce04b0fd7f2e971688e2e53b"),
    // BONK
    ("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", "0x72b021217ca3fe68922a19aaf990109cb9d84e9ad004b4d2025ad6f529314419"),
    // JTO
    ("jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL", "0xb43660a5f790c69354b0729a5ef9d50d68f1df92107540210b9cccba1f947cc2"),
    // PYTH
    ("HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3", "0x0bbf28e9a841a1cc788f6a361b17ca072d0ea3098a1e5df1c3922d06719579ff"),
    // JUP
    ("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", "0x0a0408d619e9380abad35060f9192039ed5042fa6f82301d0e48bb52be830996"),
    // WIF
    ("EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", "0x4ca4beeca86f0d164160323817a4e42b10010a724c2217c6ee41b54cd4cc61fc"),
];

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client for fetching historical token prices via Pyth Hermes API
pub struct OracleClient {
    client: reqwest::Client,
    price_cache: Arc<DashMap<String, PriceData>>,
    timestamp: i64,
    pyth_feed_map: HashMap<String, String>,
}

/// Pyth Hermes price update response
#[derive(Debug, Deserialize)]
struct PythHermesResponse {
    pub parsed: Option<Vec<PythParsedPrice>>,
}

#[derive(Debug, Deserialize)]
struct PythParsedPrice {
    pub id: String,
    pub price: PythPrice,
}

#[derive(Debug, Deserialize)]
struct PythPrice {
    pub price: String,
    pub expo: i32,
    pub publish_time: i64,
}

impl OracleClient {
    pub fn new(timestamp: i64) -> Self {
        // Build the Pyth feed map
        let pyth_feed_map: HashMap<String, String> = PYTH_FEEDS.iter()
            .map(|(mint, feed_id)| (mint.to_string(), feed_id.to_string()))
            .collect();

        Self {
            client: reqwest::Client::new(),
            price_cache: Arc::new(DashMap::new()),
            timestamp,
            pyth_feed_map,
        }
    }

    /// Batch fetch historical prices for multiple mints using Pyth Hermes API
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

        // Fetch uncached prices from Pyth Hermes (historical prices at timestamp)
        if !uncached_mints.is_empty() {
            let fetched = self.fetch_pyth_prices_batch(&uncached_mints).await;

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

        // Fetch from Pyth Hermes (single token, historical price)
        let prices = self.fetch_pyth_prices_batch(&[mint]).await;

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

    /// Fetch historical prices from Pyth Hermes API (batch request)
    /// Pyth Hermes docs: https://hermes.pyth.network/docs
    async fn fetch_pyth_prices_batch(&self, mints: &[&str]) -> Vec<(String, f64)> {
        tracing::debug!(
            "Fetching historical prices for {} tokens from Pyth Hermes at timestamp {}",
            mints.len(),
            self.timestamp
        );

        // Separate mints into those with Pyth feeds and those without
        let mut results = Vec::with_capacity(mints.len());
        let mut feed_ids = Vec::new();
        let mut mint_to_feed = HashMap::new();

        for &mint in mints {
            if let Some(feed_id) = self.pyth_feed_map.get(mint) {
                feed_ids.push(feed_id.as_str());
                mint_to_feed.insert(feed_id.as_str(), mint);
            } else {
                tracing::warn!("No Pyth feed ID for token: {}", mint);
                results.push((mint.to_string(), 0.0));
            }
        }

        if feed_ids.is_empty() {
            return results;
        }

        // Construct Pyth Hermes URL with feed IDs
        let ids_param = feed_ids.iter()
            .map(|id| format!("ids[]={}", id))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!(
            "https://hermes.pyth.network/v2/updates/price/{}?{}",
            self.timestamp,
            ids_param
        );

        tracing::debug!("Pyth Hermes API URL: {}", url);

        let response = match self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();

                if !status.is_success() {
                    tracing::error!(
                        "Pyth Hermes API error: status {}",
                        status.as_u16()
                    );
                    // Return 0.0 for all mints with feeds
                    for (_feed_id, mint) in mint_to_feed {
                        results.push((mint.to_string(), 0.0));
                    }
                    return results;
                }
                resp
            },
            Err(e) => {
                tracing::error!("Pyth Hermes API network error: {:?}", e);
                // Return 0.0 for all mints with feeds
                for (_feed_id, mint) in mint_to_feed {
                    results.push((mint.to_string(), 0.0));
                }
                return results;
            }
        };

        // Parse response
        let pyth_response: PythHermesResponse = match response.json().await {
            Ok(data) => data,
            Err(e) => {
                tracing::error!("Failed to parse Pyth Hermes response: {:?}", e);
                // Return 0.0 for all mints with feeds
                for (_feed_id, mint) in mint_to_feed {
                    results.push((mint.to_string(), 0.0));
                }
                return results;
            }
        };

        // Extract prices
        if let Some(parsed_prices) = pyth_response.parsed {
            for parsed in parsed_prices {
                if let Some(&mint) = mint_to_feed.get(parsed.id.as_str()) {
                    // Parse price with exponent
                    let price_str = &parsed.price.price;
                    if let Ok(price_raw) = price_str.parse::<f64>() {
                        let price_usd = price_raw * 10_f64.powi(parsed.price.expo);

                        tracing::debug!(
                            "Historical price for {} at timestamp {} (actual: {}): ${}",
                            mint,
                            self.timestamp,
                            parsed.price.publish_time,
                            price_usd
                        );

                        results.push((mint.to_string(), price_usd));
                    } else {
                        tracing::warn!("Failed to parse price for {}", mint);
                        results.push((mint.to_string(), 0.0));
                    }
                }
            }
        }

        let successful_prices = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::info!(
            "Fetched {}/{} historical prices successfully from Pyth Hermes",
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
