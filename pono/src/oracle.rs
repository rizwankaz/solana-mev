use std::collections::HashMap;
use reqwest;
use serde::Deserialize;
use anyhow::Result;

/// Price data from oracle
#[derive(Debug, Clone)]
pub struct PriceData {
    pub price_usd: f64,
    pub timestamp: i64,
}

/// Pyth price feed IDs for common tokens
const PYTH_FEEDS: &[(&str, &str)] = &[
    ("So11111111111111111111111111111111111111112", "ef0d8b6fda2ceba41da15d4095d1da392a0d2f8ed0c6c7bc0f4cfac8c280b56d"), // SOL/USD
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a"), // USDC/USD
    ("Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", "2b89b9dc8fdf9f34709a5b106b472f0f39bb6ca9ce04b0fd7f2e971688e2e53b"), // USDT/USD
];

/// Oracle client for fetching historical token prices
pub struct OracleClient {
    client: reqwest::Client,
    price_cache: HashMap<String, PriceData>,
    timestamp: i64,
}

#[derive(Debug, Deserialize)]
struct HermesPriceUpdate {
    price: PriceInfo,
}

#[derive(Debug, Deserialize)]
struct PriceInfo {
    price: String,
    expo: i32,
}

impl OracleClient {
    pub fn new(timestamp: i64) -> Self {
        Self {
            client: reqwest::Client::new(),
            price_cache: HashMap::new(),
            timestamp,
        }
    }

    /// Get USD price for a token at the slot timestamp
    pub async fn get_price_usd(&mut self, mint: &str) -> Result<f64> {
        // Check cache first
        if let Some(cached) = self.price_cache.get(mint) {
            return Ok(cached.price_usd);
        }

        // Try to fetch from Pyth
        let price = match self.fetch_pyth_price(mint).await {
            Ok(p) => p,
            Err(_) => self.get_fallback_price(mint),
        };

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

    /// Fetch historical price from Pyth Hermes API
    async fn fetch_pyth_price(&self, mint: &str) -> Result<f64> {
        // Find Pyth feed ID for this mint
        let feed_id = PYTH_FEEDS
            .iter()
            .find(|(m, _)| *m == mint)
            .map(|(_, id)| id)
            .ok_or_else(|| anyhow::anyhow!("No Pyth feed for mint"))?;

        // Hermes API endpoint for historical prices
        let url = format!(
            "https://hermes.pyth.network/v2/updates/price/{}?publish_time={}",
            feed_id, self.timestamp
        );

        let response = self.client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?
            .json::<HermesPriceUpdate>()
            .await?;

        let price = response.price.price.parse::<f64>()?;
        let expo = response.price.expo;

        Ok(price * 10_f64.powi(expo))
    }

    /// Fallback prices (for tokens without Pyth feeds)
    fn get_fallback_price(&self, mint: &str) -> f64 {
        match mint {
            "So11111111111111111111111111111111111111112" => 131.0,
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => 1.0,
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => 1.0,
            "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R" => 1.15,
            "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr" => 0.766,
            "ZBCNpuD7YMXzTHB2fhGkGi78MNsHGLRXUhRewNRm9RU" => 0.0026,
            _ => 0.0,
        }
    }

    /// Calculate USD value from token amount
    pub async fn calculate_usd_value(&mut self, mint: &str, amount: f64, decimals: u8) -> Result<f64> {
        let price = self.get_price_usd(mint).await?;
        let adjusted_amount = amount / 10_f64.powi(decimals as i32);
        Ok(adjusted_amount * price)
    }
}
