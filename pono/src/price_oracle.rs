use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// pyth client
pub struct PriceOracle {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct PythResponse {
    #[serde(default)]
    parsed: Vec<PythPriceFeed>,
}

#[derive(Debug, Deserialize)]
struct PythPriceFeed {
    id: String,
    price: PythPrice,
}

#[derive(Debug, Deserialize)]
struct PythPrice {
    price: String,
    conf: String,
    expo: i32,
    publish_time: i64,
}

impl PriceOracle {
    /// new pyth client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: "https://hermes.pyth.network".to_string(),
        }
    }

    /// Get Pyth price feed ID for a token mint address
    fn get_price_feed_id(mint: &str) -> Option<&'static str> {
        match mint {
            // SOL/USD
            "So11111111111111111111111111111111111111112" => {
                Some("0xef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d")
            }
            // USDC/USD
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => {
                Some("0xeaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a")
            }
            // USDT/USD
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => {
                Some("0x2b89b9dc8fdf9f34709a5b106b472f0f39bb6ca9ce04b0fd7f2e971688e2e53b")
            }
            _ => None,
        }
    }

    /// Fetch prices for multiple tokens at a specific timestamp
    ///
    /// If `publish_time` is provided, fetches historical prices from that Unix timestamp.
    /// If `publish_time` is None, fetches the latest current prices.
    pub async fn fetch_prices(&self, mints: &[String], publish_time: Option<i64>) -> Result<HashMap<String, f64>> {
        let mut prices = HashMap::new();

        if let Some(ts) = publish_time {
            tracing::info!("attempting to fetch historical prices for {} tokens at timestamp {}", mints.len(), ts);
        } else {
            tracing::info!("attempting to fetch current prices for {} tokens", mints.len());
        }

        for mint in mints {
            tracing::debug!("Mint: {}", mint);
        }

        // Get price feed IDs for all mints
        let feed_ids: Vec<String> = mints
            .iter()
            .filter_map(|mint| {
                let feed_id = Self::get_price_feed_id(mint).map(|id| id.to_string());
                if let Some(ref id) = feed_id {
                    tracing::info!("found price feed for {}: {}", mint, id);
                } else {
                    tracing::warn!("no price feed for token: {}", mint);
                }
                feed_id
            })
            .collect();

        tracing::info!("found {} price feed IDs", feed_ids.len());

        if feed_ids.is_empty() {
            tracing::warn!("no price feeds to fetch");
            return Ok(prices);
        }

        // Build query parameters
        let ids_param = feed_ids.join("&ids[]=");

        // Use historical endpoint if timestamp provided, otherwise use latest
        let url = if let Some(ts) = publish_time {
            format!("{}/v2/updates/price/{}?ids[]={}", self.base_url, ts, ids_param)
        } else {
            format!("{}/v2/updates/price/latest?ids[]={}", self.base_url, ids_param)
        };

        tracing::info!("fetching {} Pyth price feeds...", feed_ids.len());
        tracing::debug!("pyth url: {}", url);

        // Fetch prices from Pyth
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Pyth prices")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("pyth returned {}: {}", status, error_text);
            return Err(anyhow::anyhow!("pyth error: {}", status));
        }

        let pyth_response: PythResponse = response
            .json()
            .await
            .context("failed to parse response")?;

        // Parse prices and map back to mint addresses
        tracing::info!("parsing {} price feeds from pyth", pyth_response.parsed.len());
        for feed in pyth_response.parsed {
            // Normalize the returned feed ID (remove 0x prefix if present, lowercase)
            let normalized_returned_id = feed.id.trim().trim_start_matches("0x").to_lowercase();
            tracing::info!("pyth returned feed ID: {} (normalized: {})", feed.id, normalized_returned_id);

            // Find the mint that corresponds to this feed ID
            let mut matched = false;
            for mint in mints {
                if let Some(feed_id) = Self::get_price_feed_id(mint) {
                    // Normalize our stored feed ID (remove 0x prefix, lowercase)
                    let normalized_stored_id = feed_id.trim_start_matches("0x").to_lowercase();

                    tracing::debug!(
                        "comparing pyth ID '{}' with stored id '{}' for {}",
                        normalized_returned_id,
                        normalized_stored_id,
                        mint
                    );

                    if normalized_returned_id == normalized_stored_id {
                        matched = true;
                        tracing::info!("+ matched feed {} to token {}", feed.id, mint);

                        // Parse price: price * 10^expo
                        if let Ok(price_val) = feed.price.price.parse::<f64>() {
                            let expo = feed.price.expo;
                            let adjusted_price = price_val * 10f64.powi(expo);
                            prices.insert(mint.clone(), adjusted_price);
                            tracing::info!(
                                "fetched price for {}: ${:.6}",
                                mint,
                                adjusted_price
                            );
                        } else {
                            tracing::warn!("failed to parse price value: {}", feed.price.price);
                        }
                        break;
                    }
                }
            }

            if !matched {
                tracing::warn!("- no token mapping found for pyth feed ID: {} (normalized: {})", feed.id, normalized_returned_id);
            }
        }

        tracing::info!("successfully fetched {} prices from pyth", prices.len());
        Ok(prices)
    }

    /// Get price for a single token at a specific timestamp
    ///
    /// If `publish_time` is provided, fetches historical price from that Unix timestamp.
    /// If `publish_time` is None, fetches the latest current price.
    pub async fn get_price(&self, mint: &str, publish_time: Option<i64>) -> Result<Option<f64>> {
        let prices = self.fetch_prices(&[mint.to_string()], publish_time).await?;
        Ok(prices.get(mint).copied())
    }

    /// Convert lamports to SOL
    pub fn lamports_to_sol(lamports: u64) -> f64 {
        lamports as f64 / 1_000_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_sol_price() {
        let oracle = PriceOracle::new();
        let result = oracle
            .get_price("So11111111111111111111111111111111111111112", None)
            .await;
        assert!(result.is_ok());
        if let Ok(Some(price)) = result {
            println!("SOL/USD: ${}", price);
            assert!(price > 0.0);
        }
    }

    #[tokio::test]
    async fn test_fetch_multiple_prices() {
        let oracle = PriceOracle::new();
        let mints = vec![
            "So11111111111111111111111111111111111111112".to_string(), // SOL
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
        ];
        let result = oracle.fetch_prices(&mints, None).await;
        assert!(result.is_ok());
        if let Ok(prices) = result {
            println!("Prices: {:?}", prices);
            assert!(prices.len() > 0);
        }
    }
}
