use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Jupiter token metadata
#[derive(Debug, Clone, Deserialize)]
struct JupiterToken {
    address: String,
    symbol: String,
    #[serde(default)]
    decimals: u8,
}

/// Pyth price feed metadata
#[derive(Debug, Deserialize)]
struct PythFeedInfo {
    id: String,
    attributes: PythFeedAttributes,
}

#[derive(Debug, Deserialize)]
struct PythFeedAttributes {
    symbol: String,
    asset_type: String,
}

/// Pyth price update response
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

/// Cached feed mapping (mint → Pyth feed ID)
type FeedCache = Arc<RwLock<HashMap<String, String>>>;

/// Pyth Network price oracle client with dynamic feed discovery
pub struct PriceOracle {
    client: reqwest::Client,
    pyth_base_url: String,
    jupiter_token_list_url: String,
    /// Cache: mint address → Pyth feed ID
    feed_cache: FeedCache,
}

impl PriceOracle {
    /// Create a new Pyth price oracle client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            pyth_base_url: "https://hermes.pyth.network".to_string(),
            jupiter_token_list_url: "https://token.jup.ag/all".to_string(),
            feed_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Fetch all available Pyth price feeds (symbol → feed ID mapping)
    async fn fetch_pyth_feeds(&self) -> Result<HashMap<String, String>> {
        tracing::debug!("fetching Pyth price feed list...");

        let url = format!("{}/v2/price_feeds", self.pyth_base_url);
        let response = self.client.get(&url).send().await
            .context("failed to fetch Pyth feed list")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("pyth feed list returned {}", response.status()));
        }

        let feeds: Vec<PythFeedInfo> = response.json().await
            .context("failed to parse Pyth feed list")?;

        // Build symbol → feed ID map (only USD price feeds for Solana)
        let mut symbol_to_feed = HashMap::new();
        for feed in feeds {
            // Only include USD price feeds (e.g., "SOL/USD", "USDC/USD")
            if feed.attributes.symbol.contains("/USD") {
                // Extract base symbol (e.g., "SOL" from "SOL/USD")
                if let Some(base_symbol) = feed.attributes.symbol.split('/').next() {
                    symbol_to_feed.insert(base_symbol.to_string(), feed.id);
                }
            }
        }

        tracing::info!("loaded {} Pyth USD price feeds", symbol_to_feed.len());
        Ok(symbol_to_feed)
    }

    /// Fetch Jupiter token list (mint → symbol mapping)
    async fn fetch_jupiter_tokens(&self) -> Result<HashMap<String, String>> {
        tracing::debug!("fetching Jupiter token list...");

        let response = self.client.get(&self.jupiter_token_list_url).send().await
            .context("failed to fetch Jupiter token list")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("jupiter token list returned {}", response.status()));
        }

        let tokens: Vec<JupiterToken> = response.json().await
            .context("failed to parse Jupiter token list")?;

        // Build mint → symbol map
        let mut mint_to_symbol = HashMap::new();
        for token in tokens {
            mint_to_symbol.insert(token.address, token.symbol);
        }

        tracing::info!("loaded {} tokens from Jupiter", mint_to_symbol.len());
        Ok(mint_to_symbol)
    }

    /// Resolve mint addresses to Pyth feed IDs dynamically
    async fn resolve_feed_ids(&self, mints: &[String]) -> Result<HashMap<String, String>> {
        // Check cache first
        {
            let cache = self.feed_cache.read().unwrap();
            if !cache.is_empty() {
                // Return cached mappings for requested mints
                let mut cached_feeds = HashMap::new();
                for mint in mints {
                    if let Some(feed_id) = cache.get(mint) {
                        cached_feeds.insert(mint.clone(), feed_id.clone());
                    }
                }
                if !cached_feeds.is_empty() {
                    tracing::debug!("using {} cached feed IDs", cached_feeds.len());
                    return Ok(cached_feeds);
                }
            }
        }

        // Cache miss - fetch fresh data
        tracing::info!("resolving feed IDs for {} mints (cache miss)", mints.len());

        // Fetch both lists concurrently
        let (jupiter_result, pyth_result) = tokio::join!(
            self.fetch_jupiter_tokens(),
            self.fetch_pyth_feeds()
        );

        let mint_to_symbol = jupiter_result?;
        let symbol_to_feed = pyth_result?;

        // Resolve: mint → symbol → feed ID
        let mut mint_to_feed = HashMap::new();
        for mint in mints {
            if let Some(symbol) = mint_to_symbol.get(mint) {
                if let Some(feed_id) = symbol_to_feed.get(symbol) {
                    mint_to_feed.insert(mint.clone(), feed_id.clone());
                    tracing::debug!("resolved {} → {} → {}", mint, symbol, feed_id);
                } else {
                    tracing::debug!("no Pyth feed for symbol: {}", symbol);
                }
            } else {
                tracing::debug!("token not found in Jupiter list: {}", mint);
            }
        }

        // Update cache
        {
            let mut cache = self.feed_cache.write().unwrap();
            cache.extend(mint_to_feed.clone());
        }

        tracing::info!("resolved {} feed IDs", mint_to_feed.len());
        Ok(mint_to_feed)
    }

    /// Fetch prices for multiple tokens at a specific timestamp
    ///
    /// If `publish_time` is provided, fetches historical prices from that Unix timestamp.
    /// If `publish_time` is None, fetches the latest current prices.
    pub async fn fetch_prices(&self, mints: &[String], publish_time: Option<i64>) -> Result<HashMap<String, f64>> {
        let mut prices = HashMap::new();

        if mints.is_empty() {
            return Ok(prices);
        }

        if let Some(ts) = publish_time {
            tracing::info!("fetching historical prices for {} tokens at timestamp {}", mints.len(), ts);
        } else {
            tracing::info!("fetching current prices for {} tokens", mints.len());
        }

        // Resolve mint addresses to Pyth feed IDs
        let mint_to_feed = self.resolve_feed_ids(mints).await?;

        if mint_to_feed.is_empty() {
            tracing::warn!("no Pyth feeds available for requested tokens");
            return Ok(prices);
        }

        // Get unique feed IDs
        let feed_ids: Vec<String> = mint_to_feed.values().cloned().collect();
        tracing::info!("fetching prices for {} feeds", feed_ids.len());

        // Build query parameters
        let ids_param = feed_ids.join("&ids[]=");

        // Use historical endpoint if timestamp provided, otherwise use latest
        let url = if let Some(ts) = publish_time {
            format!("{}/v2/updates/price/{}?ids[]={}", self.pyth_base_url, ts, ids_param)
        } else {
            format!("{}/v2/updates/price/latest?ids[]={}", self.pyth_base_url, ids_param)
        };

        tracing::debug!("pyth url: {}", url);

        // Fetch prices from Pyth
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("failed to fetch Pyth prices")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            tracing::error!("pyth returned {}: {}", status, error_text);
            return Err(anyhow::anyhow!("pyth error: {}", status));
        }

        let pyth_response: PythResponse = response
            .json()
            .await
            .context("failed to parse Pyth response")?;

        // Parse prices and map back to mint addresses
        tracing::info!("parsing {} price feeds", pyth_response.parsed.len());
        for feed in pyth_response.parsed {
            // Normalize feed ID
            let normalized_feed_id = feed.id.trim().trim_start_matches("0x").to_lowercase();

            // Find which mint(s) correspond to this feed ID
            for (mint, feed_id) in &mint_to_feed {
                let normalized_expected = feed_id.trim_start_matches("0x").to_lowercase();

                if normalized_feed_id == normalized_expected {
                    // Parse price: price * 10^expo
                    if let Ok(price_val) = feed.price.price.parse::<f64>() {
                        let expo = feed.price.expo;
                        let adjusted_price = price_val * 10f64.powi(expo);
                        prices.insert(mint.clone(), adjusted_price);
                        tracing::info!("fetched price for {}: ${:.6}", mint, adjusted_price);
                    } else {
                        tracing::warn!("failed to parse price value: {}", feed.price.price);
                    }
                }
            }
        }

        tracing::info!("successfully fetched {} prices", prices.len());
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

    #[tokio::test]
    async fn test_dynamic_feed_resolution() {
        let oracle = PriceOracle::new();
        // Test with various tokens
        let mints = vec![
            "So11111111111111111111111111111111111111112".to_string(), // SOL
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string(), // BONK
        ];
        let result = oracle.fetch_prices(&mints, None).await;
        assert!(result.is_ok());
        if let Ok(prices) = result {
            println!("Dynamic prices: {:?}", prices);
            // Should get at least SOL and USDC
            assert!(prices.len() >= 2);
        }
    }
}
