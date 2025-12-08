use dashmap::DashMap;
use reqwest;
use serde::Deserialize;
use anyhow::Result;
use std::sync::Arc;

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client for fetching historical token prices via Birdeye API
pub struct OracleClient {
    client: reqwest::Client,
    price_cache: Arc<DashMap<String, PriceData>>,
    timestamp: i64,
}

/// Birdeye historical price API response
#[derive(Debug, Deserialize)]
struct BirdeyePriceResponse {
    pub success: bool,
    pub data: Option<BirdeyeHistoricalData>,
}

#[derive(Debug, Deserialize)]
struct BirdeyeHistoricalData {
    pub items: Vec<BirdeyePricePoint>,
}

#[derive(Debug, Deserialize)]
struct BirdeyePricePoint {
    #[serde(rename = "unixTime")]
    pub unix_time: i64,
    pub value: f64,
}

impl OracleClient {
    pub fn new(timestamp: i64) -> Self {
        Self {
            client: reqwest::Client::new(),
            price_cache: Arc::new(DashMap::new()),
            timestamp,
        }
    }

    /// Batch fetch historical prices for multiple mints using Birdeye API
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

        // Fetch uncached prices from Birdeye (historical prices at timestamp)
        if !uncached_mints.is_empty() {
            let fetched = self.fetch_birdeye_prices_batch(&uncached_mints).await;

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

        // Fetch from Birdeye (single token, historical price)
        let prices = self.fetch_birdeye_prices_batch(&[mint]).await;

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

    /// Fetch historical prices from Birdeye API (batch request)
    /// Birdeye API docs: https://docs.birdeye.so/reference/get_defi-historical-price
    async fn fetch_birdeye_prices_batch(&self, mints: &[&str]) -> Vec<(String, f64)> {
        use futures::future::join_all;

        tracing::debug!("Fetching historical prices for {} tokens from Birdeye at timestamp {}", mints.len(), self.timestamp);

        // Birdeye doesn't support batch requests, so we need to fetch individually
        // But we can do it concurrently using join_all
        let futures: Vec<_> = mints.iter()
            .map(|&mint| self.fetch_single_birdeye_price(mint))
            .collect();

        let results = join_all(futures).await;

        let successful_prices = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::info!(
            "Fetched {}/{} historical prices successfully from Birdeye",
            successful_prices,
            mints.len()
        );

        results
    }

    /// Fetch a single historical price from Birdeye API
    async fn fetch_single_birdeye_price(&self, mint: &str) -> (String, f64) {
        // Birdeye historical price endpoint
        // Note: Free tier has rate limits (10 requests/second)
        let url = format!(
            "https://public-api.birdeye.so/defi/historical_price?address={}&type=1m&time_from={}&time_to={}",
            mint,
            self.timestamp - 60,  // 1 minute before
            self.timestamp + 60   // 1 minute after
        );

        tracing::debug!("Fetching price for {} at timestamp {}", mint, self.timestamp);

        let response = match self.client
            .get(&url)
            .header("X-API-KEY", std::env::var("BIRDEYE_API_KEY").unwrap_or_default())
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();

                if !status.is_success() {
                    tracing::warn!(
                        "Birdeye API error for {}: status {}",
                        mint,
                        status.as_u16()
                    );
                    return (mint.to_string(), 0.0);
                }
                resp
            },
            Err(e) => {
                tracing::warn!("Birdeye API network error for {}: {:?}", mint, e);
                return (mint.to_string(), 0.0);
            }
        };

        // Parse response
        let birdeye_response: BirdeyePriceResponse = match response.json().await {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("Failed to parse Birdeye response for {}: {:?}", mint, e);
                return (mint.to_string(), 0.0);
            }
        };

        // Extract price - find the closest price point to our timestamp
        let price = if birdeye_response.success {
            birdeye_response.data
                .and_then(|d| {
                    if d.items.is_empty() {
                        return None;
                    }

                    // Find the price point closest to our target timestamp
                    let closest = d.items.iter()
                        .min_by_key(|item| (item.unix_time - self.timestamp).abs())?;

                    tracing::debug!(
                        "Historical price for {} at timestamp {} (actual: {}): ${}",
                        mint,
                        self.timestamp,
                        closest.unix_time,
                        closest.value
                    );
                    Some(closest.value)
                })
                .unwrap_or_else(|| {
                    tracing::warn!("No historical price data for {}", mint);
                    0.0
                })
        } else {
            tracing::warn!("Birdeye API returned success=false for {}", mint);
            0.0
        };

        (mint.to_string(), price)
    }

    /// Calculate USD value from token amount
    pub async fn calculate_usd_value(&self, mint: &str, amount: f64, decimals: u8) -> Result<f64> {
        let price = self.get_price_usd(mint).await?;
        let adjusted_amount = amount / 10_f64.powi(decimals as i32);
        Ok(adjusted_amount * price)
    }
}
