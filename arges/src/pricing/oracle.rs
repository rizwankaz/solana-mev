//! Price oracle for real-time token pricing

use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

/// Token price in SOL and USD
#[derive(Debug, Clone)]
pub struct TokenPrice {
    /// Token mint address
    pub mint: String,
    /// Price in SOL
    pub price_sol: f64,
    /// Price in USD
    pub price_usd: f64,
    /// Timestamp when price was fetched
    pub timestamp: Instant,
}

impl TokenPrice {
    /// Check if price is stale (older than 60 seconds)
    pub fn is_stale(&self, max_age_secs: u64) -> bool {
        self.timestamp.elapsed() > Duration::from_secs(max_age_secs)
    }
}

/// Jupiter API price response (v6 format)
#[derive(Debug, Deserialize)]
struct JupiterPriceResponse {
    data: HashMap<String, JupiterTokenPrice>,
}

#[derive(Debug, Deserialize)]
struct JupiterTokenPrice {
    id: String,
    #[serde(rename = "mintSymbol")]
    mint_symbol: Option<String>,
    #[serde(rename = "vsToken")]
    vs_token: Option<String>,
    #[serde(rename = "vsTokenSymbol")]
    vs_token_symbol: Option<String>,
    price: f64,
}

/// Price oracle using Jupiter API
pub struct PriceOracle {
    /// Price cache with timestamps
    cache: Arc<RwLock<HashMap<String, TokenPrice>>>,
    /// Jupiter API base URL
    jupiter_api_url: String,
    /// HTTP client
    client: reqwest::Client,
    /// Cache TTL in seconds
    cache_ttl: u64,
}

impl PriceOracle {
    /// Create a new price oracle
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            jupiter_api_url: "https://lite-api.jup.ag/price/v3".to_string(),
            client: reqwest::Client::new(),
            cache_ttl: 60, // 60 seconds
        }
    }

    /// Get price for a token in SOL, fetching if not cached or stale
    pub async fn get_price_sol(&self, mint: &str) -> Result<f64> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(price) = cache.get(mint) {
                if !price.is_stale(self.cache_ttl) {
                    debug!("Price cache hit for {}: {} SOL", mint, price.price_sol);
                    return Ok(price.price_sol);
                }
            }
        }

        // Fetch fresh price
        self.fetch_and_cache_price(mint).await?;

        // Return from cache
        let cache = self.cache.read().await;
        cache
            .get(mint)
            .map(|p| p.price_sol)
            .ok_or_else(|| anyhow!("Failed to get price for {}", mint))
    }

    /// Get price for a token in USD
    pub async fn get_price_usd(&self, mint: &str) -> Result<f64> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(price) = cache.get(mint) {
                if !price.is_stale(self.cache_ttl) {
                    debug!("Price cache hit for {}: ${}", mint, price.price_usd);
                    return Ok(price.price_usd);
                }
            }
        }

        // Fetch fresh price
        self.fetch_and_cache_price(mint).await?;

        // Return from cache
        let cache = self.cache.read().await;
        cache
            .get(mint)
            .map(|p| p.price_usd)
            .ok_or_else(|| anyhow!("Failed to get price for {}", mint))
    }

    /// Fetch price from Jupiter API and cache it
    async fn fetch_and_cache_price(&self, mint: &str) -> Result<()> {
        debug!("Fetching price for {} from Jupiter API", mint);

        let url = format!("{}?ids={}", self.jupiter_api_url, mint);

        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch price from Jupiter: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!(
                "Jupiter API returned error: {}",
                response.status()
            ));
        }

        let price_response: JupiterPriceResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse Jupiter response: {}", e))?;

        let token_price = price_response
            .data
            .get(mint)
            .ok_or_else(|| anyhow!("No price data for {} in Jupiter response", mint))?;

        let price_usd = token_price.price;

        // Get SOL price to calculate price in SOL
        let sol_price_usd = if mint == super::WSOL_ADDRESS {
            price_usd
        } else {
            // Fetch SOL price if not already cached
            let sol_price = self.get_sol_price_usd().await?;
            sol_price
        };

        let price_sol = if mint == super::WSOL_ADDRESS {
            1.0
        } else {
            price_usd / sol_price_usd
        };

        debug!(
            "Fetched price for {}: {} SOL (${} USD)",
            mint, price_sol, price_usd
        );

        // Cache the price
        let mut cache = self.cache.write().await;
        cache.insert(
            mint.to_string(),
            TokenPrice {
                mint: mint.to_string(),
                price_sol,
                price_usd,
                timestamp: Instant::now(),
            },
        );

        Ok(())
    }

    /// Get SOL price in USD (helper method)
    async fn get_sol_price_usd(&self) -> Result<f64> {
        // Check if we have cached SOL price
        {
            let cache = self.cache.read().await;
            if let Some(price) = cache.get(super::WSOL_ADDRESS) {
                if !price.is_stale(self.cache_ttl) {
                    return Ok(price.price_usd);
                }
            }
        }

        // Fetch SOL price
        debug!("Fetching SOL price from Jupiter API");
        let url = format!("{}?ids={}", self.jupiter_api_url, super::WSOL_ADDRESS);

        let response = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch SOL price: {}", e))?;

        let price_response: JupiterPriceResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse SOL price response: {}", e))?;

        let sol_price = price_response
            .data
            .get(super::WSOL_ADDRESS)
            .ok_or_else(|| anyhow!("No SOL price in Jupiter response"))?;

        let price_usd = sol_price.price;

        // Cache SOL price
        let mut cache = self.cache.write().await;
        cache.insert(
            super::WSOL_ADDRESS.to_string(),
            TokenPrice {
                mint: super::WSOL_ADDRESS.to_string(),
                price_sol: 1.0,
                price_usd,
                timestamp: Instant::now(),
            },
        );

        Ok(price_usd)
    }

    /// Warmup cache with common token prices
    pub async fn warmup(&self) -> Result<()> {
        debug!("Warming up price cache");

        let common_tokens = vec![
            super::WSOL_ADDRESS,
            super::USDC_ADDRESS,
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB", // USDT
        ];

        for mint in common_tokens {
            if let Err(e) = self.fetch_and_cache_price(mint).await {
                warn!("Failed to warmup price for {}: {}", mint, e);
            }
        }

        Ok(())
    }
}

impl Default for PriceOracle {
    fn default() -> Self {
        Self::new()
    }
}
