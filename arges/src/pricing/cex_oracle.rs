//! CEX (Centralized Exchange) Price Oracle
//!
//! Fetches real-time prices from major centralized exchanges to enable
//! CEX-DEX arbitrage detection.

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// CEX price data
#[derive(Debug, Clone)]
pub struct CexPrice {
    /// Price in USD
    pub price_usd: f64,
    /// Exchange name
    pub exchange: String,
    /// Timestamp when fetched
    pub timestamp: Instant,
    /// Best bid price
    pub bid: Option<f64>,
    /// Best ask price
    pub ask: Option<f64>,
}

/// Binance API response for ticker price
#[derive(Debug, Deserialize)]
struct BinanceTickerResponse {
    symbol: String,
    #[serde(rename = "price")]
    price: String,
    #[serde(rename = "bidPrice")]
    bid_price: Option<String>,
    #[serde(rename = "askPrice")]
    ask_price: Option<String>,
}

/// Coinbase API response for ticker
#[derive(Debug, Deserialize)]
struct CoinbaseTickerResponse {
    data: CoinbaseData,
}

#[derive(Debug, Deserialize)]
struct CoinbaseData {
    amount: String,
}

/// Aggregated CEX price from multiple exchanges
#[derive(Debug, Clone)]
pub struct AggregatedCexPrice {
    /// Average price across exchanges
    pub avg_price: f64,
    /// Best bid across all exchanges
    pub best_bid: f64,
    /// Best ask across all exchanges
    pub best_ask: f64,
    /// Spread (ask - bid)
    pub spread: f64,
    /// Prices from each exchange
    pub exchange_prices: Vec<CexPrice>,
}

/// Maps Solana token mints to CEX trading symbols
pub struct TokenMapping {
    /// Mint address -> (CEX symbol, decimals)
    mappings: HashMap<String, (String, u8)>,
}

impl TokenMapping {
    pub fn new() -> Self {
        let mut mappings = HashMap::new();

        // Common Solana tokens and their CEX symbols
        // WSOL
        mappings.insert(
            "So11111111111111111111111111111111111111112".to_string(),
            ("SOL".to_string(), 9),
        );

        // USDC
        mappings.insert(
            "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".to_string(),
            ("USDC".to_string(), 6),
        );

        // USDT
        mappings.insert(
            "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB".to_string(),
            ("USDT".to_string(), 6),
        );

        // BONK
        mappings.insert(
            "DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263".to_string(),
            ("BONK".to_string(), 5),
        );

        // JUP
        mappings.insert(
            "JUPyiwrYJFskUPiHa7hkeR8VUtAeFoSYbKedZNsDvCN".to_string(),
            ("JUP".to_string(), 6),
        );

        // RAY (Raydium)
        mappings.insert(
            "4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R".to_string(),
            ("RAY".to_string(), 6),
        );

        // ORCA
        mappings.insert(
            "orcaEKTdK7LKz57vaAYr9QeNsVEPfiu6QeMU1kektZE".to_string(),
            ("ORCA".to_string(), 6),
        );

        // mSOL (Marinade staked SOL)
        mappings.insert(
            "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So".to_string(),
            ("MSOL".to_string(), 9),
        );

        // PYTH
        mappings.insert(
            "HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3".to_string(),
            ("PYTH".to_string(), 6),
        );

        // WIF (dogwifhat)
        mappings.insert(
            "EKpQGSJtjMFqKZ9KQanSqYXRcF8fBopzLHYxdM65zcjm".to_string(),
            ("WIF".to_string(), 6),
        );

        Self { mappings }
    }

    /// Get CEX symbol for a Solana mint address
    pub fn get_cex_symbol(&self, mint: &str) -> Option<&str> {
        self.mappings.get(mint).map(|(symbol, _)| symbol.as_str())
    }

    /// Add a custom mapping
    pub fn add_mapping(&mut self, mint: String, symbol: String, decimals: u8) {
        self.mappings.insert(mint, (symbol, decimals));
    }
}

/// CEX price oracle that fetches prices from multiple exchanges
pub struct CexOracle {
    client: Client,
    token_mapping: TokenMapping,
    /// Cache: symbol -> (price, timestamp)
    cache: Arc<RwLock<HashMap<String, CexPrice>>>,
    /// Cache TTL (time-to-live)
    cache_ttl: Duration,
}

impl CexOracle {
    /// Create a new CEX oracle
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            token_mapping: TokenMapping::new(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: Duration::from_secs(30), // 30 second cache
        }
    }

    /// Get token mapping (for adding custom mappings)
    pub fn token_mapping_mut(&mut self) -> &mut TokenMapping {
        &mut self.token_mapping
    }

    /// Get CEX price for a token by Solana mint address
    pub async fn get_price_for_mint(&self, mint: &str) -> Result<AggregatedCexPrice> {
        let symbol = self.token_mapping
            .get_cex_symbol(mint)
            .ok_or_else(|| anyhow!("No CEX mapping for mint: {}", mint))?;

        self.get_aggregated_price(symbol).await
    }

    /// Get aggregated price from multiple exchanges
    pub async fn get_aggregated_price(&self, symbol: &str) -> Result<AggregatedCexPrice> {
        let mut prices = Vec::new();

        // Fetch from Binance
        if let Ok(price) = self.get_binance_price(symbol).await {
            prices.push(price);
        }

        // Fetch from Coinbase (for major pairs)
        if matches!(symbol, "SOL" | "USDC" | "USDT") {
            if let Ok(price) = self.get_coinbase_price(symbol).await {
                prices.push(price);
            }
        }

        if prices.is_empty() {
            return Err(anyhow!("No CEX price data available for {}", symbol));
        }

        // Calculate aggregated metrics
        let avg_price = prices.iter().map(|p| p.price_usd).sum::<f64>() / prices.len() as f64;

        let best_bid = prices.iter()
            .filter_map(|p| p.bid)
            .max_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(avg_price);

        let best_ask = prices.iter()
            .filter_map(|p| p.ask)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(avg_price);

        let spread = best_ask - best_bid;

        Ok(AggregatedCexPrice {
            avg_price,
            best_bid,
            best_ask,
            spread,
            exchange_prices: prices,
        })
    }

    /// Fetch price from Binance
    async fn get_binance_price(&self, symbol: &str) -> Result<CexPrice> {
        // Check cache first
        let cache_key = format!("BINANCE_{}", symbol);
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                if cached.timestamp.elapsed() < self.cache_ttl {
                    debug!("CEX cache hit for {}", cache_key);
                    return Ok(cached.clone());
                }
            }
        }

        // Binance uses USDT pairs for most tokens
        let pair = if symbol == "USDT" || symbol == "USDC" {
            return Ok(CexPrice {
                price_usd: 1.0,
                exchange: "Binance".to_string(),
                timestamp: Instant::now(),
                bid: Some(0.9999),
                ask: Some(1.0001),
            });
        } else {
            format!("{}USDT", symbol)
        };

        let url = format!(
            "https://api.binance.com/api/v3/ticker/bookTicker?symbol={}",
            pair
        );

        debug!("Fetching Binance price for {}", pair);

        let response = self.client
            .get(&url)
            .send()
            .await?
            .json::<BinanceTickerResponse>()
            .await?;

        let price = response.price.parse::<f64>()?;
        let bid = response.bid_price.and_then(|s| s.parse::<f64>().ok());
        let ask = response.ask_price.and_then(|s| s.parse::<f64>().ok());

        let cex_price = CexPrice {
            price_usd: price,
            exchange: "Binance".to_string(),
            timestamp: Instant::now(),
            bid,
            ask,
        };

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, cex_price.clone());
        }

        debug!("Binance {}: ${}", symbol, price);

        Ok(cex_price)
    }

    /// Fetch price from Coinbase
    async fn get_coinbase_price(&self, symbol: &str) -> Result<CexPrice> {
        // Check cache first
        let cache_key = format!("COINBASE_{}", symbol);
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&cache_key) {
                if cached.timestamp.elapsed() < self.cache_ttl {
                    debug!("CEX cache hit for {}", cache_key);
                    return Ok(cached.clone());
                }
            }
        }

        if symbol == "USDT" || symbol == "USDC" {
            return Ok(CexPrice {
                price_usd: 1.0,
                exchange: "Coinbase".to_string(),
                timestamp: Instant::now(),
                bid: Some(0.9999),
                ask: Some(1.0001),
            });
        }

        let pair = format!("{}-USD", symbol);
        let url = format!(
            "https://api.coinbase.com/v2/prices/{}/spot",
            pair
        );

        debug!("Fetching Coinbase price for {}", pair);

        let response = self.client
            .get(&url)
            .send()
            .await?
            .json::<CoinbaseTickerResponse>()
            .await?;

        let price = response.data.amount.parse::<f64>()?;

        let cex_price = CexPrice {
            price_usd: price,
            exchange: "Coinbase".to_string(),
            timestamp: Instant::now(),
            bid: None, // Coinbase spot API doesn't provide bid/ask
            ask: None,
        };

        // Update cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(cache_key, cex_price.clone());
        }

        debug!("Coinbase {}: ${}", symbol, price);

        Ok(cex_price)
    }
}

impl Default for CexOracle {
    fn default() -> Self {
        Self::new()
    }
}
