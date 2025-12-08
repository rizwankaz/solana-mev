use dashmap::DashMap;
use reqwest;
use serde::Deserialize;
use anyhow::Result;
use std::sync::Arc;
use std::collections::HashMap;

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Oracle client for fetching token prices via Jupiter Price API
pub struct OracleClient {
    client: reqwest::Client,
    price_cache: Arc<DashMap<String, PriceData>>,
    timestamp: i64,
}

/// Jupiter Price API response for a single token
#[derive(Debug, Deserialize)]
struct JupiterTokenPrice {
    #[serde(rename = "id")]
    pub mint: String,
    pub price: f64,
}

/// Jupiter Price API response wrapper
#[derive(Debug, Deserialize)]
struct JupiterPriceResponse {
    pub data: HashMap<String, JupiterTokenPrice>,
    #[serde(rename = "timeTaken")]
    pub time_taken: Option<f64>,
}

impl OracleClient {
    pub fn new(timestamp: i64) -> Self {
        Self {
            client: reqwest::Client::new(),
            price_cache: Arc::new(DashMap::new()),
            timestamp,
        }
    }

    /// Batch fetch prices for multiple mints using Jupiter's batch API
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

        // Fetch uncached prices from Jupiter in a single batch request
        if !uncached_mints.is_empty() {
            let fetched = self.fetch_jupiter_prices_batch(&uncached_mints).await;

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

        // Fetch from Jupiter (single token)
        let prices = self.fetch_jupiter_prices_batch(&[mint]).await;

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

    /// Fetch prices from Jupiter Price API v6 (batch request)
    async fn fetch_jupiter_prices_batch(&self, mints: &[&str]) -> Vec<(String, f64)> {
        // Jupiter API supports comma-separated IDs
        let ids = mints.join(",");

        // Jupiter Price API v6 endpoint (current as of Dec 2025)
        // Docs: https://station.jup.ag/api-v6/price-api
        let url = format!("https://price.jup.ag/v6/price?ids={}", ids);

        tracing::debug!("Fetching prices for {} tokens from Jupiter", mints.len());
        tracing::debug!("Jupiter API URL: {}", url);

        let response = match self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                tracing::debug!("Jupiter API response status: {}", status);

                if !status.is_success() {
                    tracing::error!(
                        "Jupiter API returned error status {}: {}",
                        status.as_u16(),
                        status.canonical_reason().unwrap_or("Unknown")
                    );
                    return mints.iter()
                        .map(|&m| {
                            tracing::warn!("No price for {} (API error)", m);
                            (m.to_string(), 0.0)
                        })
                        .collect();
                }
                resp
            },
            Err(e) => {
                tracing::error!("Jupiter API network request failed: {:?}", e);
                tracing::error!("This may be due to network restrictions or API unavailability");
                // Return 0.0 for all mints on API failure
                return mints.iter()
                    .map(|&m| {
                        tracing::warn!("No price for {} (network error)", m);
                        (m.to_string(), 0.0)
                    })
                    .collect();
            }
        };

        let jupiter_response: JupiterPriceResponse = match response.json::<JupiterPriceResponse>().await {
            Ok(data) => {
                tracing::debug!("Successfully parsed Jupiter response with {} prices", data.data.len());
                data
            },
            Err(e) => {
                tracing::error!("Failed to parse Jupiter JSON response: {:?}", e);
                // Return 0.0 for all mints on parse failure
                return mints.iter()
                    .map(|&m| {
                        tracing::warn!("No price for {} (parse error)", m);
                        (m.to_string(), 0.0)
                    })
                    .collect();
            }
        };

        // Extract prices from response
        let results: Vec<(String, f64)> = mints.iter()
            .map(|&mint| {
                let price = jupiter_response.data
                    .get(mint)
                    .map(|token_price| {
                        tracing::debug!("Price for {}: ${}", mint, token_price.price);
                        token_price.price
                    })
                    .unwrap_or_else(|| {
                        tracing::warn!("No price data available for token: {}", mint);
                        0.0
                    });

                (mint.to_string(), price)
            })
            .collect();

        let successful_prices = results.iter().filter(|(_, p)| *p > 0.0).count();
        tracing::info!(
            "Fetched {}/{} prices successfully from Jupiter",
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
