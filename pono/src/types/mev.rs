use serde::{Deserialize, Serialize};
use super::swap::SwapInfo;
use super::token::SimpleTokenChange;

/// MEV event type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MevEvent {
    Arbitrage(ArbitrageEvent),
    Sandwich(SandwichEvent),
}

/// Arbitrage MEV event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageEvent {
    pub signature: String,
    pub signer: String,
    pub success: bool,
    pub compute_units_consumed: u64,
    pub fee: u64,
    pub priority_fee: u64,
    pub swaps: Vec<SwapInfo>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<SimpleTokenChange>,
    pub profitability: Profitability,
}

/// Sandwich attack MEV event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichEvent {
    pub slot: u64,
    pub signer: String,
    pub victim_signature: String,
    pub front_run: SandwichTransaction,
    pub victim: SandwichTransaction,
    pub back_run: SandwichTransaction,
    pub total_compute_units: u64,
    pub total_fees: u64,
    pub swaps: Vec<SwapInfo>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<SimpleTokenChange>,
    pub profitability: Profitability,
}

/// Transaction in sandwich pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichTransaction {
    pub signature: String,
    pub index: usize,
    pub signer: String,
    pub compute_units: u64,
    pub fee: u64,
}

/// Profitability information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profitability {
    pub profit_usd: f64,
    pub fees_usd: f64,
    pub net_profit_usd: f64,
    /// List of profit token mints that don't have Pyth price feeds
    pub unsupported_profit_tokens: Vec<String>,
}
