//! CEX-DEX Arbitrage Detection
//!
//! Detects arbitrage opportunities between centralized exchanges (CEX)
//! and decentralized exchanges (DEX) on Solana.

use crate::dex::ParsedSwap;
use crate::types::FetchedBlock;
use crate::mev::types::*;
use crate::pricing::cex_oracle::CexOracle;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::debug;

/// CEX-DEX arbitrage metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CexDexMetadata {
    /// The DEX swap transaction
    pub dex_tx: String,

    /// DEX where the trade occurred
    pub dex: String,

    /// Token being arbitraged
    pub token: String,

    /// Direction: "buy_dex_sell_cex" or "sell_dex_buy_cex"
    pub direction: String,

    /// DEX execution price (in USD per token)
    pub dex_price: f64,

    /// CEX price at time of trade (average)
    pub cex_price: f64,

    /// CEX best bid
    pub cex_bid: f64,

    /// CEX best ask
    pub cex_ask: f64,

    /// Price difference (%)
    pub price_diff_pct: f64,

    /// Trade amount (in normalized tokens, not raw units)
    pub amount: f64,

    /// Estimated profit (in USD)
    pub estimated_profit_usd: f64,

    /// CEX exchanges used for price reference
    pub cex_exchanges: Vec<String>,
}

/// Detector for CEX-DEX arbitrage
pub struct CexDexDetector {
    /// CEX price oracle
    cex_oracle: Arc<CexOracle>,

    /// Minimum price difference to consider (e.g., 0.5% = 0.005)
    min_price_diff: f64,

    /// Minimum trade size in USD
    min_trade_size_usd: f64,

    /// Minimum estimated profit in USD
    min_profit_usd: f64,
}

impl CexDexDetector {
    /// Create a new CEX-DEX detector
    pub fn new(cex_oracle: Arc<CexOracle>) -> Self {
        Self {
            cex_oracle,
            min_price_diff: 0.003, // 0.3% minimum spread
            min_trade_size_usd: 1000.0, // $1000 minimum trade
            min_profit_usd: 10.0, // $10 minimum profit
        }
    }

    /// Detect CEX-DEX arbitrage in a block
    pub async fn detect(&self, block: &FetchedBlock, swaps: &[ParsedSwap]) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        for swap in swaps {
            // Check if this swap looks like CEX-DEX arbitrage
            if let Some(event) = self.check_swap_for_cex_dex_arb(swap, block).await? {
                events.push(event);
            }
        }

        Ok(events)
    }

    /// Check a single swap for CEX-DEX arbitrage
    async fn check_swap_for_cex_dex_arb(
        &self,
        swap: &ParsedSwap,
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Get CEX prices for both tokens in the swap
        let token_in_cex_price = match self.cex_oracle.get_price_for_mint(&swap.token_in).await {
            Ok(price) => price,
            Err(_) => {
                // Token not available on CEX, skip
                return Ok(None);
            }
        };

        let token_out_cex_price = match self.cex_oracle.get_price_for_mint(&swap.token_out).await {
            Ok(price) => price,
            Err(_) => {
                // Token not available on CEX, skip
                return Ok(None);
            }
        };

        // Get token decimals for proper normalization
        let token_in_decimals = self.cex_oracle.get_decimals(&swap.token_in)
            .ok_or_else(|| anyhow!("No decimals info for token_in: {}", swap.token_in))?;
        let token_out_decimals = self.cex_oracle.get_decimals(&swap.token_out)
            .ok_or_else(|| anyhow!("No decimals info for token_out: {}", swap.token_out))?;

        // Normalize amounts by decimals (raw units -> actual tokens)
        // For example: 1,000,000 raw USDC (6 decimals) = 1.0 USDC
        let amount_in_normalized = (swap.amount_in as f64) / 10_f64.powi(token_in_decimals as i32);
        let amount_out_normalized = (swap.amount_out as f64) / 10_f64.powi(token_out_decimals as i32);

        // Calculate DEX execution price
        // For swap of token_in -> token_out:
        // DEX price = (amount_in_usd / amount_out_tokens)
        // This is the USD price per token_out

        let amount_in_usd = amount_in_normalized * token_in_cex_price.avg_price;
        let amount_out_tokens = amount_out_normalized;

        // Skip if amounts are too small
        if amount_in_usd < self.min_trade_size_usd {
            return Ok(None);
        }

        // DEX price for token_out (in USD per token)
        let dex_price_per_output_token = amount_in_usd / amount_out_tokens;

        // CEX price for token_out
        let cex_price_per_output_token = token_out_cex_price.avg_price;

        // Calculate price difference
        // If DEX price < CEX price: trader is BUYING on DEX (cheap) to SELL on CEX (expensive)
        // If DEX price > CEX price: trader is SELLING on DEX (expensive) after BUYING on CEX (cheap)

        let price_diff = (dex_price_per_output_token - cex_price_per_output_token) / cex_price_per_output_token;
        let price_diff_pct = price_diff * 100.0;

        // Determine direction and check if profitable
        let (direction, is_profitable) = if dex_price_per_output_token < cex_price_per_output_token {
            // Buying on DEX (cheaper), selling on CEX (more expensive)
            let spread = cex_price_per_output_token - dex_price_per_output_token;
            ("buy_dex_sell_cex".to_string(), spread / cex_price_per_output_token >= self.min_price_diff)
        } else {
            // Selling on DEX (more expensive), after buying on CEX (cheaper)
            let spread = dex_price_per_output_token - cex_price_per_output_token;
            ("sell_dex_buy_cex".to_string(), spread / cex_price_per_output_token >= self.min_price_diff)
        };

        if !is_profitable {
            return Ok(None);
        }

        // Calculate estimated profit
        let estimated_profit_usd = if direction == "buy_dex_sell_cex" {
            // Profit = (CEX sell price - DEX buy price) * amount
            (cex_price_per_output_token - dex_price_per_output_token) * amount_out_tokens
        } else {
            // Profit = (DEX sell price - CEX buy price) * amount
            (dex_price_per_output_token - cex_price_per_output_token) * amount_out_tokens
        };

        // Skip if profit too small
        if estimated_profit_usd < self.min_profit_usd {
            return Ok(None);
        }

        debug!(
            "CEX-DEX arbitrage detected: {} {} tokens, DEX price ${:.6}, CEX price ${:.6}, diff {:.2}%, est. profit ${:.2}",
            direction,
            swap.token_out,
            dex_price_per_output_token,
            cex_price_per_output_token,
            price_diff_pct.abs(),
            estimated_profit_usd
        );

        // Create CEX-DEX metadata
        let metadata = CexDexMetadata {
            dex_tx: swap.signature.clone(),
            dex: format!("{:?}", swap.dex),
            token: swap.token_out.clone(),
            direction: direction.clone(),
            dex_price: dex_price_per_output_token,
            cex_price: cex_price_per_output_token,
            cex_bid: token_out_cex_price.best_bid,
            cex_ask: token_out_cex_price.best_ask,
            price_diff_pct: price_diff_pct.abs(),
            amount: amount_out_normalized,
            estimated_profit_usd,
            cex_exchanges: token_out_cex_price.exchange_prices
                .iter()
                .map(|p| p.exchange.clone())
                .collect(),
        };

        // Calculate confidence based on:
        // - Price difference magnitude
        // - Trade size
        // - Number of CEX sources
        let confidence = self.calculate_confidence(&metadata);

        // Create MevEvent
        let event = MevEvent {
            mev_type: MevType::CexDex,
            slot: block.slot,
            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
            transactions: vec![swap.signature.clone()],
            profit_lamports: None, // Will be enriched later
            profit_usd: Some(estimated_profit_usd),
            tokens: vec![swap.token_in.clone(), swap.token_out.clone()],
            metadata: MevMetadata::CexDex(metadata),
            extractor: Some(swap.user.clone()),
            confidence,
        };

        Ok(Some(event))
    }

    /// Calculate confidence score for CEX-DEX arbitrage
    fn calculate_confidence(&self, metadata: &CexDexMetadata) -> f64 {
        let mut confidence: f64 = 0.5; // Base confidence

        // Higher price difference = higher confidence
        if metadata.price_diff_pct > 1.0 {
            confidence += 0.2;
        } else if metadata.price_diff_pct > 0.5 {
            confidence += 0.1;
        }

        // Larger profit = higher confidence
        if metadata.estimated_profit_usd > 100.0 {
            confidence += 0.15;
        } else if metadata.estimated_profit_usd > 50.0 {
            confidence += 0.1;
        }

        // More CEX sources = higher confidence
        if metadata.cex_exchanges.len() >= 2 {
            confidence += 0.1;
        }

        // CEX bid-ask spread check (tighter spread = more reliable)
        let cex_spread_pct = ((metadata.cex_ask - metadata.cex_bid) / metadata.cex_price) * 100.0;
        if cex_spread_pct < 0.1 {
            confidence += 0.05;
        }

        confidence.min(0.95_f64) // Cap at 95%
    }
}

impl Default for CexDexDetector {
    fn default() -> Self {
        Self::new(Arc::new(CexOracle::new()))
    }
}
