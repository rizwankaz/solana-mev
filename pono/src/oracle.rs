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

/// Oracle client for fetching token prices
pub struct OracleClient {
    client: reqwest::Client,
    price_cache: HashMap<String, PriceData>,
}

#[derive(Debug, Deserialize)]
struct PythPriceResponse {
    price: String,
    conf: String,
    expo: i32,
}

impl OracleClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            price_cache: HashMap::new(),
        }
    }

    /// Get USD price for a token mint
    pub async fn get_price_usd(&mut self, mint: &str) -> Result<f64> {
        // Check cache first
        if let Some(cached) = self.price_cache.get(mint) {
            return Ok(cached.price_usd);
        }

        // For now, use hardcoded prices for common tokens
        // In production, you'd fetch from Pyth or another oracle
        let price = self.get_hardcoded_price(mint);

        // Cache the price
        self.price_cache.insert(
            mint.to_string(),
            PriceData {
                price_usd: price,
                timestamp: chrono::Utc::now().timestamp(),
            },
        );

        Ok(price)
    }

    /// Hardcoded prices for common tokens (fallback)
    /// TODO: Replace with actual Pyth oracle integration
    fn get_hardcoded_price(&self, mint: &str) -> f64 {
        match mint {
            // SOL
            "So11111111111111111111111111111111111111112" => 131.0,
            // USDC
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" => 1.0,
            // USDT
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB" => 1.0,
            // RAY
            "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R" => 1.15,
            // POPCAT (example price)
            "7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr" => 0.766,
            // ZBCN (example price)
            "ZBCNpuD7YMXzTHB2fhGkGi78MNsHGLRXUhRewNRm9RU" => 0.0026,
            // Default for unknown tokens
            _ => 0.0,
        }
    }

    /// Calculate USD value from token amount
    pub async fn calculate_usd_value(&mut self, mint: &str, amount: f64, decimals: u8) -> Result<f64> {
        let price = self.get_price_usd(mint).await?;
        let adjusted_amount = amount / 10_f64.powi(decimals as i32);
        Ok(adjusted_amount * price)
    }

    /// Clear the price cache
    pub fn clear_cache(&mut self) {
        self.price_cache.clear();
    }
}

impl Default for OracleClient {
    fn default() -> Self {
        Self::new()
    }
}
