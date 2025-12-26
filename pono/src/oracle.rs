use dashmap::DashMap;
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;
use serde::Deserialize;

// claude wrote this because i cannot pay for an oracle atm
// revisit

/// Pyth Benchmarks API symbols for major Solana tokens
/// Source: https://benchmarks.pyth.network/docs
const PYTH_FEEDS: &[(&str, &str)] = &[
    // (Mint Address, Benchmarks Symbol)
    ("So11111111111111111111111111111111111111112", "Crypto.SOL/USD"),
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "Crypto.USDC/USD"),
    ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", "Crypto.USDT/USD"),
    ("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", "Crypto.BONK/USD"),
    ("jtojtomepa8beP8AuQc6eXt5FriJwfFMwQx2v2f9mCL", "Crypto.JTO/USD"),
    ("HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3", "Crypto.PYTH/USD"),
    ("JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN", "Crypto.JUP/USD"),
    ("EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm", "Crypto.WIF/USD"),
];

/// Response from Pyth Benchmarks TradingView history API
#[derive(Debug, Deserialize)]
struct BenchmarksResponse {
    #[serde(rename = "c")]
    close: Vec<f64>,
    #[serde(rename = "t")]
    time: Vec<i64>,
    s: String, // status: "ok" or "no_data"
}

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client for fetching historical token prices via Pyth Benchmarks API
pub struct OracleClient {
    http_client: reqwest::Client,
    price_cache: Arc<DashMap<String, PriceData>>,
    timestamp: i64,
    symbol_map: HashMap<String, String>,  // mint -> Benchmarks symbol
}

impl OracleClient {
    pub fn new(_slot: u64, timestamp: i64, _rpc_url: String) -> Self {
        // Build the symbol map (mint -> Benchmarks symbol)
        let symbol_map: HashMap<String, String> = PYTH_FEEDS.iter()
            .map(|(mint, symbol)| (mint.to_string(), symbol.to_string()))
            .collect();

        Self {
            http_client: reqwest::Client::new(),
            price_cache: Arc::new(DashMap::new()),
            timestamp,
            symbol_map,
        }
    }

    /// Batch fetch historical prices for multiple mints using Pyth Benchmarks API
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

        // Fetch uncached prices from Pyth Benchmarks API at the specific timestamp
        if !uncached_mints.is_empty() {
            let fetched = self.fetch_benchmarks_prices(&uncached_mints).await;

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

        // Fetch from Pyth Benchmarks API (single token, historical price)
        let prices = self.fetch_benchmarks_prices(&[mint]).await;

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

    /// Fetch historical prices from Pyth Benchmarks API at specific timestamp
    /// Uses TradingView-style history endpoint for accurate historical prices
    /// Fetches all prices in parallel for better performance
    async fn fetch_benchmarks_prices(&self, mints: &[&str]) -> Vec<(String, f64)> {
        tracing::debug!(
            "Fetching historical prices for {} tokens from Pyth Benchmarks at timestamp {}",
            mints.len(),
            self.timestamp
        );

        // Create parallel futures for all mints
        let futures: Vec<_> = mints.iter().map(|&mint| {
            let symbol = self.symbol_map.get(mint).cloned();
            let http_client = self.http_client.clone();
            let timestamp = self.timestamp;
            let mint_owned = mint.to_string();

            async move {
                // Check if we have a Benchmarks symbol for this mint
                let symbol = match symbol {
                    Some(s) => s,
                    None => {
                        tracing::debug!("No Pyth Benchmarks symbol for token: {}", mint_owned);
                        return (mint_owned, 0.0);
                    }
                };

                // Query a small time window around the target timestamp (±5 minutes)
                let from = timestamp - 300;
                let to = timestamp + 300;

                // Build the Benchmarks API URL
                let url = format!(
                    "https://benchmarks.pyth.network/v1/shims/tradingview/history?symbol={}&resolution=1&from={}&to={}",
                    symbol, from, to
                );

                tracing::debug!("Requesting historical price from: {}", url);

                // Make HTTP request
                let response = match http_client.get(&url).send().await {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!("Failed to fetch price for {} ({}): {:?}", mint_owned, symbol, e);
                        return (mint_owned, 0.0);
                    }
                };

                // Parse JSON response
                let benchmarks_data: BenchmarksResponse = match response.json().await {
                    Ok(data) => data,
                    Err(e) => {
                        tracing::error!("Failed to parse response for {} ({}): {:?}", mint_owned, symbol, e);
                        return (mint_owned, 0.0);
                    }
                };

                // Check status and extract price
                if benchmarks_data.s != "ok" || benchmarks_data.close.is_empty() {
                    tracing::warn!("No price data available for {} ({}) at timestamp {}", mint_owned, symbol, timestamp);
                    return (mint_owned, 0.0);
                }

                // Find the price closest to our target timestamp
                let mut best_price = benchmarks_data.close[0];
                let mut best_diff = (benchmarks_data.time[0] - timestamp).abs();

                for i in 1..benchmarks_data.close.len() {
                    let diff = (benchmarks_data.time[i] - timestamp).abs();
                    if diff < best_diff {
                        best_diff = diff;
                        best_price = benchmarks_data.close[i];
                    }
                }

                tracing::debug!(
                    "Historical price for {} ({}) at timestamp {}: ${}",
                    mint_owned,
                    symbol,
                    timestamp,
                    best_price
                );

                (mint_owned, best_price)
            }
        }).collect();

        // Execute all futures in parallel
        let results = futures::future::join_all(futures).await;

        let successful_prices = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::debug!(
            "Fetched {}/{} historical prices successfully from Pyth Benchmarks",
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
