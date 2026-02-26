use anyhow::Result;
use dashmap::DashMap;
use serde::Deserialize;
use std::sync::Arc;

/// Birdeye /defi/history_price response
#[derive(Debug, Deserialize)]
struct BirdeyeResponse {
    success: bool,
    data: BirdeyeData,
}

#[derive(Debug, Deserialize)]
struct BirdeyeData {
    items: Vec<BirdeyePricePoint>,
}

#[derive(Debug, Deserialize)]
struct BirdeyePricePoint {
    #[serde(rename = "unixTime")]
    unix_time: i64,
    value: f64,
}

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client — fetches historical token prices from Birdeye.
///
/// Requires the environment variable `BIRDEYE_API_KEY` to be set.
/// Covers any SPL or Token-2022 token; no hardcoded symbol list needed.
pub struct OracleClient {
    http_client: reqwest::Client,
    price_cache: Arc<DashMap<String, PriceData>>,
    timestamp: i64,
    api_key: String,
}

impl OracleClient {
    pub fn new(_slot: u64, timestamp: i64, _rpc_url: String) -> Self {
        let api_key = std::env::var("BIRDEYE_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            tracing::warn!("BIRDEYE_API_KEY not set — token prices will be unavailable");
        }

        Self {
            http_client: reqwest::Client::new(),
            price_cache: Arc::new(DashMap::new()),
            timestamp,
            api_key,
        }
    }

    /// Batch-fetch historical prices for multiple mints in parallel.
    pub async fn batch_get_prices(&self, mints: &[&str]) -> Vec<(String, f64)> {
        if mints.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::with_capacity(mints.len());
        let mut uncached = Vec::new();

        for &mint in mints {
            if let Some(cached) = self.price_cache.get(mint) {
                results.push((mint.to_string(), cached.price_usd));
            } else {
                uncached.push(mint);
            }
        }

        if !uncached.is_empty() {
            let fetched = self.fetch_birdeye_prices(&uncached).await;
            for (mint, price) in fetched {
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

    /// Get USD price for a single token at the slot timestamp.
    pub async fn get_price_usd(&self, mint: &str) -> Result<f64> {
        if let Some(cached) = self.price_cache.get(mint) {
            return Ok(cached.price_usd);
        }

        let prices = self.fetch_birdeye_prices(&[mint]).await;
        let price = prices.first().map(|(_, p)| *p).unwrap_or(0.0);

        self.price_cache.insert(
            mint.to_string(),
            PriceData {
                price_usd: price,
                timestamp: self.timestamp,
            },
        );

        Ok(price)
    }

    /// Fetch historical prices from Birdeye in parallel.
    ///
    /// Uses `GET /defi/history_price?address=<mint>&address_type=token&type=1m`
    /// with a ±5 min window around the block timestamp, then picks the closest
    /// data point. Works for any SPL or Token-2022 token.
    async fn fetch_birdeye_prices(&self, mints: &[&str]) -> Vec<(String, f64)> {
        if self.api_key.is_empty() {
            return mints.iter().map(|m| (m.to_string(), 0.0)).collect();
        }

        tracing::debug!(
            "Fetching historical prices for {} tokens from Birdeye at timestamp {}",
            mints.len(),
            self.timestamp
        );

        let from = self.timestamp - 300;
        let to = self.timestamp + 300;
        let target = self.timestamp;

        let futures: Vec<_> = mints
            .iter()
            .map(|&mint| {
                let http_client = self.http_client.clone();
                let api_key = self.api_key.clone();
                let mint_owned = mint.to_string();

                async move {
                    let url = format!(
                        "https://public-api.birdeye.so/defi/history_price\
                         ?address={}&address_type=token&type=1m&time_from={}&time_to={}",
                        mint_owned, from, to
                    );

                    let resp = match http_client
                        .get(&url)
                        .header("X-API-KEY", &api_key)
                        .header("x-chain", "solana")
                        .send()
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::error!("Birdeye request failed for {}: {:?}", mint_owned, e);
                            return (mint_owned, 0.0);
                        }
                    };

                    let body: BirdeyeResponse = match resp.json().await {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!(
                                "Birdeye parse failed for {}: {:?}",
                                mint_owned,
                                e
                            );
                            return (mint_owned, 0.0);
                        }
                    };

                    if !body.success || body.data.items.is_empty() {
                        tracing::warn!(
                            "No Birdeye price data for {} at timestamp {}",
                            mint_owned,
                            target
                        );
                        return (mint_owned, 0.0);
                    }

                    // Pick the data point closest to the block timestamp
                    let best = body
                        .data
                        .items
                        .iter()
                        .min_by_key(|p| (p.unix_time - target).abs())
                        .map(|p| p.value)
                        .unwrap_or(0.0);

                    tracing::debug!(
                        "Birdeye price for {} at {}: ${}",
                        mint_owned,
                        target,
                        best
                    );

                    (mint_owned, best)
                }
            })
            .collect();

        let results = futures::future::join_all(futures).await;

        let n_ok = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::debug!(
            "Birdeye: fetched {}/{} prices successfully",
            n_ok,
            mints.len()
        );

        results
    }

    /// Calculate USD value from a raw token amount.
    pub async fn calculate_usd_value(&self, mint: &str, amount: f64, decimals: u8) -> Result<f64> {
        let price = self.get_price_usd(mint).await?;
        let adjusted = amount / 10_f64.powi(decimals as i32);
        Ok(adjusted * price)
    }
}
