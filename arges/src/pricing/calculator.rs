//! Profit calculator with accurate token conversion to SOL

use super::{MetadataCache, PriceOracle};
use crate::dex::ParsedSwap;
use crate::types::FetchedBlock;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{debug, warn};

/// Profit calculator for MEV events
pub struct ProfitCalculator {
    /// Token metadata cache
    pub metadata_cache: Arc<MetadataCache>,
    /// Price oracle
    pub price_oracle: Arc<PriceOracle>,
}

impl ProfitCalculator {
    /// Create a new profit calculator
    pub fn new(metadata_cache: Arc<MetadataCache>, price_oracle: Arc<PriceOracle>) -> Self {
        Self {
            metadata_cache,
            price_oracle,
        }
    }

    /// Calculate profit in SOL from a cycle of swaps (e.g., arbitrage)
    ///
    /// Returns (gross_profit_sol, net_profit_sol) where net includes transaction fees
    pub async fn calculate_cycle_profit(
        &self,
        swaps: &[&ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<(f64, f64)> {
        if swaps.is_empty() {
            return Ok((0.0, 0.0));
        }

        let first_token = &swaps[0].token_in;
        let last_token = &swaps[swaps.len() - 1].token_out;

        // Validate it's a cycle
        if first_token != last_token {
            return Err(anyhow!(
                "Not a valid cycle: start={}, end={}",
                first_token,
                last_token
            ));
        }

        // Get token metadata for decimal conversion
        let metadata = self.metadata_cache.get_metadata(first_token).await?;

        // Convert amounts to UI amounts
        let input_ui = metadata.amount_to_ui(swaps[0].amount_in);
        let output_ui = metadata.amount_to_ui(swaps[swaps.len() - 1].amount_out);

        debug!(
            "Cycle: {} {} → {} {} (token: {})",
            input_ui,
            metadata.symbol.as_deref().unwrap_or("?"),
            output_ui,
            metadata.symbol.as_deref().unwrap_or("?"),
            first_token
        );

        // Get token price in SOL
        let price_sol = self.price_oracle.get_price_sol(first_token).await?;

        // Calculate gross profit in token terms, then convert to SOL
        let token_profit = output_ui - input_ui;
        let gross_profit_sol = token_profit * price_sol;

        // Calculate actual fees from transaction data
        let tx_fees_sol = self.calculate_transaction_fees(swaps, block).await?;

        let net_profit_sol = gross_profit_sol - tx_fees_sol;

        debug!(
            "Profit calculation: {} {} profit = {} SOL (fees: {} SOL, net: {} SOL)",
            token_profit,
            metadata.symbol.as_deref().unwrap_or("?"),
            gross_profit_sol,
            tx_fees_sol,
            net_profit_sol
        );

        Ok((gross_profit_sol, net_profit_sol))
    }

    /// Calculate profit from owned swaps (for atomic arbitrage)
    pub async fn calculate_cycle_profit_owned(
        &self,
        swaps: &[ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<(f64, f64)> {
        let swap_refs: Vec<&ParsedSwap> = swaps.iter().collect();
        self.calculate_cycle_profit(&swap_refs, block).await
    }

    /// Calculate the SOL value of a token amount
    pub async fn token_amount_to_sol(&self, mint: &str, amount: u64) -> Result<f64> {
        // Get metadata for decimal conversion
        let metadata = self.metadata_cache.get_metadata(mint).await?;
        let ui_amount = metadata.amount_to_ui(amount);

        // Get price in SOL
        let price_sol = self.price_oracle.get_price_sol(mint).await?;

        let sol_value = ui_amount * price_sol;

        debug!(
            "Converted {} {} ({} raw) to {} SOL",
            ui_amount,
            metadata.symbol.as_deref().unwrap_or("?"),
            amount,
            sol_value
        );

        Ok(sol_value)
    }

    /// Calculate actual transaction fees from block data
    async fn calculate_transaction_fees(
        &self,
        swaps: &[&ParsedSwap],
        block: &FetchedBlock,
    ) -> Result<f64> {
        // Extract unique transaction signatures
        let mut tx_sigs: Vec<String> = swaps.iter().map(|s| s.signature.clone()).collect();
        tx_sigs.sort();
        tx_sigs.dedup();

        let mut total_fee_lamports = 0u64;

        // Look up actual fees from block transactions
        for sig in &tx_sigs {
            if let Some(fee) = self.extract_tx_fee(sig, block) {
                total_fee_lamports += fee;
            } else {
                warn!("Could not find fee for transaction {}", sig);
                // Use a conservative estimate if we can't find the actual fee
                total_fee_lamports += 5000; // 5000 lamports default
            }
        }

        // Convert lamports to SOL
        let total_fee_sol = total_fee_lamports as f64 / 1e9;

        debug!(
            "Total transaction fees: {} lamports ({} SOL) for {} transactions",
            total_fee_lamports,
            total_fee_sol,
            tx_sigs.len()
        );

        Ok(total_fee_sol)
    }

    /// Extract transaction fee from block data
    fn extract_tx_fee(&self, signature: &str, block: &FetchedBlock) -> Option<u64> {
        for tx in &block.transactions {
            // Check if this is the right transaction
            if let Some(tx_sig) = tx.signature() {
                if tx_sig == signature {
                    // Extract fee from transaction metadata
                    return tx.fee();
                }
            }
        }
        None
    }

    /// Calculate profit between two swaps (for sandwich attacks)
    pub async fn calculate_sandwich_profit(
        &self,
        frontrun: &ParsedSwap,
        backrun: &ParsedSwap,
        block: &FetchedBlock,
    ) -> Result<f64> {
        // Validate tokens match
        if frontrun.token_in != backrun.token_out {
            return Err(anyhow!(
                "Sandwich token mismatch: frontrun in={}, backrun out={}",
                frontrun.token_in,
                backrun.token_out
            ));
        }

        let token = &frontrun.token_in;
        let metadata = self.metadata_cache.get_metadata(token).await?;

        let input_ui = metadata.amount_to_ui(frontrun.amount_in);
        let output_ui = metadata.amount_to_ui(backrun.amount_out);

        let price_sol = self.price_oracle.get_price_sol(token).await?;

        let token_profit = output_ui - input_ui;
        let gross_profit_sol = token_profit * price_sol;

        // Calculate fees for both transactions
        let swaps = vec![frontrun, backrun];
        let fees_sol = self.calculate_transaction_fees(&swaps, block).await?;

        let net_profit_sol = gross_profit_sol - fees_sol;

        debug!(
            "Sandwich profit: {} {} = {} SOL (fees: {} SOL, net: {} SOL)",
            token_profit,
            metadata.symbol.as_deref().unwrap_or("?"),
            gross_profit_sol,
            fees_sol,
            net_profit_sol
        );

        Ok(net_profit_sol)
    }

    /// Calculate victim loss in a sandwich attack
    pub async fn calculate_victim_loss(
        &self,
        victim_swap: &ParsedSwap,
        expected_output: u64,
    ) -> Result<f64> {
        let actual_output = victim_swap.amount_out;

        if actual_output >= expected_output {
            return Ok(0.0); // No loss
        }

        let token = &victim_swap.token_out;
        let metadata = self.metadata_cache.get_metadata(token).await?;

        let loss_ui = metadata.amount_to_ui(expected_output - actual_output);
        let price_sol = self.price_oracle.get_price_sol(token).await?;

        let loss_sol = loss_ui * price_sol;

        debug!(
            "Victim loss: {} {} = {} SOL",
            loss_ui,
            metadata.symbol.as_deref().unwrap_or("?"),
            loss_sol
        );

        Ok(loss_sol)
    }

    /// Warmup caches
    pub async fn warmup(&self) -> Result<()> {
        debug!("Warming up profit calculator caches");
        self.metadata_cache.warmup().await?;
        self.price_oracle.warmup().await?;
        Ok(())
    }
}
