//! Token metadata fetching and caching

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Token metadata information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMetadata {
    /// Token mint address
    pub mint: String,
    /// Number of decimal places
    pub decimals: u8,
    /// Token symbol (if available)
    pub symbol: Option<String>,
    /// Token name (if available)
    pub name: Option<String>,
}

impl TokenMetadata {
    /// Convert raw token amount to human-readable amount
    pub fn amount_to_ui(&self, amount: u64) -> f64 {
        amount as f64 / 10_f64.powi(self.decimals as i32)
    }

    /// Convert human-readable amount to raw token amount
    pub fn ui_to_amount(&self, ui_amount: f64) -> u64 {
        (ui_amount * 10_f64.powi(self.decimals as i32)) as u64
    }
}

/// Cache for token metadata to avoid excessive RPC calls
pub struct MetadataCache {
    /// RPC client for fetching metadata
    rpc_client: Arc<RpcClient>,
    /// Cached metadata
    cache: Arc<RwLock<HashMap<String, TokenMetadata>>>,
}

impl MetadataCache {
    /// Create a new metadata cache
    pub fn new(rpc_url: String) -> Self {
        Self {
            rpc_client: Arc::new(RpcClient::new(rpc_url)),
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get token metadata, fetching if not cached
    pub async fn get_metadata(&self, mint: &str) -> Result<TokenMetadata> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(metadata) = cache.get(mint) {
                debug!("Token metadata cache hit for {}", mint);
                return Ok(metadata.clone());
            }
        }

        // Fetch from RPC
        debug!("Fetching token metadata for {} from RPC", mint);
        let metadata = self.fetch_metadata(mint).await?;

        // Cache it
        {
            let mut cache = self.cache.write().await;
            cache.insert(mint.to_string(), metadata.clone());
        }

        Ok(metadata)
    }

    /// Fetch token metadata from RPC
    async fn fetch_metadata(&self, mint: &str) -> Result<TokenMetadata> {
        let pubkey = Pubkey::from_str(mint)
            .map_err(|e| anyhow!("Invalid mint address {}: {}", mint, e))?;

        // Try to get token supply info which includes decimals
        match self.rpc_client.get_token_supply(&pubkey).await {
            Ok(supply) => {
                debug!(
                    "Got token supply for {}: decimals={}",
                    mint, supply.decimals
                );
                Ok(TokenMetadata {
                    mint: mint.to_string(),
                    decimals: supply.decimals,
                    symbol: None,
                    name: None,
                })
            }
            Err(e) => {
                // If it's native SOL or WSOL, use 9 decimals
                if mint == super::WSOL_ADDRESS || mint == "11111111111111111111111111111111" {
                    debug!("Using native SOL decimals for {}", mint);
                    return Ok(TokenMetadata {
                        mint: mint.to_string(),
                        decimals: 9,
                        symbol: Some("SOL".to_string()),
                        name: Some("Solana".to_string()),
                    });
                }

                warn!("Failed to fetch metadata for {}: {}", mint, e);
                // Default to 9 decimals if we can't fetch
                Ok(TokenMetadata {
                    mint: mint.to_string(),
                    decimals: 9,
                    symbol: None,
                    name: None,
                })
            }
        }
    }

    /// Pre-populate cache with common tokens
    pub async fn warmup(&self) -> Result<()> {
        debug!("Warming up metadata cache with common tokens");

        let common_tokens = vec![
            (super::WSOL_ADDRESS, 9, "SOL", "Wrapped SOL"),
            (super::USDC_ADDRESS, 6, "USDC", "USD Coin"),
            (
                "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
                6,
                "USDT",
                "USDT",
            ),
            (
                "mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So",
                9,
                "mSOL",
                "Marinade SOL",
            ),
            (
                "7vfCXTUXx5WJV5JADk17DUJ4ksgau7utNKj4b963voxs",
                8,
                "ETH",
                "Wrapped Ethereum",
            ),
        ];

        let mut cache = self.cache.write().await;
        for (mint, decimals, symbol, name) in common_tokens {
            cache.insert(
                mint.to_string(),
                TokenMetadata {
                    mint: mint.to_string(),
                    decimals,
                    symbol: Some(symbol.to_string()),
                    name: Some(name.to_string()),
                },
            );
        }

        Ok(())
    }
}
