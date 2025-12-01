use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

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

/// Pyth Network price oracle client with preloaded token and feed data
pub struct PriceOracle {
    client: reqwest::Client,
    pyth_base_url: String,
    /// Preloaded mapping: mint address → symbol
    mint_to_symbol: HashMap<String, String>,
    /// Preloaded mapping: symbol → Pyth feed ID
    symbol_to_feed: HashMap<String, String>,
}

impl PriceOracle {
    /// Create a new Pyth price oracle client (loads token and feed lists at startup)
    pub async fn new() -> Result<Self> {
        let client = reqwest::Client::new();
        let pyth_base_url = "https://hermes.pyth.network".to_string();
        let jupiter_token_list_url = "https://cache.jup.ag/tokens".to_string();

        tracing::info!("initializing price oracle (loading token and feed lists)...");

        // Fetch both lists concurrently at startup
        let (jupiter_result, pyth_result) = tokio::join!(
            Self::fetch_jupiter_tokens_static(&client, &jupiter_token_list_url),
            Self::fetch_pyth_feeds_static(&client, &pyth_base_url)
        );

        let mint_to_symbol = jupiter_result?;
        let symbol_to_feed = pyth_result?;

        tracing::info!(
            "price oracle initialized: {} tokens, {} price feeds",
            mint_to_symbol.len(),
            symbol_to_feed.len()
        );

        Ok(Self {
            client,
            pyth_base_url,
            mint_to_symbol,
            symbol_to_feed,
        })
    }

    /// Fetch all available Pyth price feeds (static method for initialization)
    async fn fetch_pyth_feeds_static(client: &reqwest::Client, base_url: &str) -> Result<HashMap<String, String>> {
        tracing::debug!("fetching Pyth price feed list...");

        let url = format!("{}/v2/price_feeds", base_url);
        let response = client.get(&url).send().await
            .context("failed to fetch Pyth feed list")?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("pyth feed list returned {}", response.status()));
        }

        let feeds: Vec<PythFeedInfo> = response.json().await
            .context("failed to parse Pyth feed list")?;

        // Build symbol → feed ID map (only USD price feeds)
        let mut symbol_to_feed = HashMap::new();
        let mut total_feeds = 0;
        let mut usd_feeds = 0;

        for feed in feeds {
            total_feeds += 1;

            // Only include USD price feeds (e.g., "SOL/USD", "Crypto.SOL/USD", "Equity.US.BTC/USD")
            if feed.attributes.symbol.contains("/USD") {
                usd_feeds += 1;

                // Extract base symbol from formats like:
                // - "SOL/USD" → "SOL"
                // - "Crypto.SOL/USD" → "SOL"
                // - "Equity.US.WYNN/USD" → "WYNN"
                if let Some(base_part) = feed.attributes.symbol.split('/').next() {
                    // Strip any prefix (e.g., "Crypto.", "Equity.US.")
                    let symbol = base_part.split('.').last().unwrap_or(base_part);

                    tracing::debug!("extracted '{}' from '{}' → feed_id {}",
                        symbol, feed.attributes.symbol, feed.id);
                    symbol_to_feed.insert(symbol.to_string(), feed.id);
                }
            }
        }

        tracing::info!("loaded {} Pyth USD price feeds from {} total feeds ({} were USD pairs)",
            symbol_to_feed.len(), total_feeds, usd_feeds);

        // Log first few symbols for debugging
        let sample_symbols: Vec<_> = symbol_to_feed.keys().take(10).collect();
        tracing::debug!("sample symbols in map: {:?}", sample_symbols);
        Ok(symbol_to_feed)
    }

    /// Fetch Jupiter token list (static method for initialization)
    async fn fetch_jupiter_tokens_static(client: &reqwest::Client, url: &str) -> Result<HashMap<String, String>> {
        tracing::debug!("fetching Jupiter token list...");

        let response = client.get(url).send().await
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

    /// Resolve mint addresses to Pyth feed IDs using preloaded data
    fn resolve_feed_ids(&self, mints: &[String]) -> HashMap<String, String> {
        let mut mint_to_feed = HashMap::new();

        for mint in mints {
            if let Some(symbol) = self.mint_to_symbol.get(mint) {
                tracing::info!("mint {} → symbol '{}'", mint, symbol);

                // Debug: check if symbol exists with different case or whitespace
                let symbol_trimmed = symbol.trim();
                tracing::debug!("looking up symbol '{}' (trimmed: '{}', len: {})",
                    symbol, symbol_trimmed, symbol.len());

                if let Some(feed_id) = self.symbol_to_feed.get(symbol) {
                    mint_to_feed.insert(mint.clone(), feed_id.clone());
                    tracing::info!("✓ resolved {} → {} → {}", mint, symbol, feed_id);
                } else {
                    // Debug: show similar symbols that DO exist
                    let similar: Vec<_> = self.symbol_to_feed.keys()
                        .filter(|k| k.contains(symbol.as_str()) || symbol.contains(k.as_str()))
                        .take(5)
                        .collect();
                    tracing::warn!("✗ no Pyth feed for symbol '{}' (from mint {}). Similar keys in map: {:?}",
                        symbol, mint, similar);
                }
            } else {
                tracing::warn!("✗ token not in Jupiter list: {}", mint);
            }
        }

        if !mint_to_feed.is_empty() {
            tracing::info!("resolved {} feed IDs from preloaded data", mint_to_feed.len());
        }

        mint_to_feed
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

        // Resolve mint addresses to Pyth feed IDs using preloaded data
        let mint_to_feed = self.resolve_feed_ids(mints);

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

    /// Get token display names (mint address → symbol)
    pub fn get_token_names(&self) -> &HashMap<String, String> {
        &self.mint_to_symbol
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
        let oracle = PriceOracle::new().await.expect("failed to initialize oracle");
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
        let oracle = PriceOracle::new().await.expect("failed to initialize oracle");
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

    #[test]
    fn test_symbol_extraction_logic() {
        // Test the symbol extraction logic that processes Pyth feeds
        let test_cases = vec![
            ("SOL/USD", "SOL"),
            ("USDC/USD", "USDC"),
            ("BTC/USD", "BTC"),
            ("Crypto.SOL/USD", "SOL"),  // Crypto prefix should be stripped
            ("Crypto.USDC/USD", "USDC"),  // Crypto prefix should be stripped
            ("Equity.US.WYNN/USD", "WYNN"),  // Multi-level prefix should be stripped
        ];

        for (input, expected) in test_cases {
            // Extract base symbol (same logic as production code)
            let symbol = input.split('/').next()
                .and_then(|base_part| base_part.split('.').last())
                .unwrap();
            assert_eq!(symbol, expected, "Failed to extract {} from {}", expected, input);
        }
    }

    #[test]
    fn test_hashmap_symbol_matching() {
        use std::collections::HashMap;

        // Simulate the symbol_to_feed HashMap with REAL Pyth format
        let mut symbol_to_feed = HashMap::new();

        // Test 1: Crypto prefix (real Pyth format)
        let pyth_feed_symbol = "Crypto.SOL/USD";
        let base_part = pyth_feed_symbol.split('/').next().unwrap();
        let extracted_symbol = base_part.split('.').last().unwrap().to_string();
        symbol_to_feed.insert(extracted_symbol.clone(), "feed_id_sol".to_string());

        // Test 2: Another crypto prefix
        let pyth_usdc = "Crypto.USDC/USD";
        let usdc_base = pyth_usdc.split('/').next().unwrap();
        let usdc_symbol = usdc_base.split('.').last().unwrap().to_string();
        symbol_to_feed.insert(usdc_symbol.clone(), "feed_id_usdc".to_string());

        // Simulate Jupiter returning bare symbols
        let jupiter_sol = "SOL";
        let jupiter_usdc = "USDC";

        // Both should match
        let sol_result = symbol_to_feed.get(jupiter_sol);
        let usdc_result = symbol_to_feed.get(jupiter_usdc);

        assert!(sol_result.is_some(), "Failed to match SOL. Keys: {:?}",
            symbol_to_feed.keys().collect::<Vec<_>>());
        assert_eq!(sol_result.unwrap(), "feed_id_sol");

        assert!(usdc_result.is_some(), "Failed to match USDC. Keys: {:?}",
            symbol_to_feed.keys().collect::<Vec<_>>());
        assert_eq!(usdc_result.unwrap(), "feed_id_usdc");

        println!("✓ Symbol matching logic works correctly with Crypto prefix");
        println!("  Pyth feed: {} → extracted: {}", pyth_feed_symbol, extracted_symbol);
        println!("  Jupiter symbol: {} → {:?}", jupiter_sol, sol_result);
        println!("  Jupiter symbol: {} → {:?}", jupiter_usdc, usdc_result);
    }

    #[tokio::test]
    async fn test_preloaded_feed_resolution() {
        let oracle = PriceOracle::new().await.expect("failed to initialize oracle");

        // Verify preloaded data
        assert!(!oracle.mint_to_symbol.is_empty(), "should have loaded tokens");
        assert!(!oracle.symbol_to_feed.is_empty(), "should have loaded price feeds");

        // Log some sample data for debugging
        println!("Loaded {} tokens from Jupiter", oracle.mint_to_symbol.len());
        println!("Loaded {} Pyth feeds", oracle.symbol_to_feed.len());

        // Check if SOL exists in both maps
        let sol_mint = "So11111111111111111111111111111111111111112";
        if let Some(sol_symbol) = oracle.mint_to_symbol.get(sol_mint) {
            println!("SOL mint → symbol: '{}'", sol_symbol);
            println!("SOL symbol in feed map: {}", oracle.symbol_to_feed.contains_key(sol_symbol));

            if !oracle.symbol_to_feed.contains_key(sol_symbol) {
                // Show what symbols ARE in the feed map
                let sample_feeds: Vec<_> = oracle.symbol_to_feed.keys().take(20).collect();
                println!("Sample feed symbols: {:?}", sample_feeds);
            }
        }

        // Test with various tokens
        let mints = vec![
            "So11111111111111111111111111111111111111112".to_string(), // SOL
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(), // USDC
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string(), // BONK
        ];
        let result = oracle.fetch_prices(&mints, None).await;
        assert!(result.is_ok());
        if let Ok(prices) = result {
            println!("Preloaded prices: {:?}", prices);
            // Should get at least SOL and USDC
            assert!(prices.len() >= 2);
        }
    }
}
