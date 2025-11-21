use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Pyth Network price oracle client
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
    /// Create a new Pyth price oracle client
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

    /// Fetch current prices for multiple tokens
    pub async fn fetch_prices(&self, mints: &[String]) -> Result<HashMap<String, f64>> {
        let mut prices = HashMap::new();

        // Get price feed IDs for all mints
        let feed_ids: Vec<String> = mints
            .iter()
            .filter_map(|mint| Self::get_price_feed_id(mint).map(|id| id.to_string()))
            .collect();

        if feed_ids.is_empty() {
            return Ok(prices);
        }

        // Build query parameters
        let ids_param = feed_ids.join("&ids[]=");
        let url = format!("{}/v2/updates/price/latest?ids[]={}", self.base_url, ids_param);

        tracing::debug!("Fetching Pyth prices from: {}", url);

        // Fetch prices from Pyth
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Pyth prices")?;

        let pyth_response: PythResponse = response
            .json()
            .await
            .context("Failed to parse Pyth response")?;

        // Parse prices and map back to mint addresses
        for feed in pyth_response.parsed {
            // Find the mint that corresponds to this feed ID
            for mint in mints {
                if let Some(feed_id) = Self::get_price_feed_id(mint) {
                    if feed.id == feed_id {
                        // Parse price: price * 10^expo
                        if let Ok(price_val) = feed.price.price.parse::<f64>() {
                            let expo = feed.price.expo;
                            let adjusted_price = price_val * 10f64.powi(expo);
                            prices.insert(mint.clone(), adjusted_price);
                            tracing::debug!(
                                "Price for {}: ${:.6} (raw: {}, expo: {})",
                                mint,
                                adjusted_price,
                                price_val,
                                expo
                            );
                        }
                    }
                }
            }
        }

        Ok(prices)
    }

    /// Get price for a single token
    pub async fn get_price(&self, mint: &str) -> Result<Option<f64>> {
        let prices = self.fetch_prices(&[mint.to_string()]).await?;
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
            .get_price("So11111111111111111111111111111111111111112")
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
        let result = oracle.fetch_prices(&mints).await;
        assert!(result.is_ok());
        if let Ok(prices) = result {
            println!("Prices: {:?}", prices);
            assert!(prices.len() > 0);
        }
    }
}
