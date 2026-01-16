use super::swap::SwapInfo;
use super::token::SimpleTokenChange;
use serde::{Deserialize, Serialize};

/// event type: (atomic) arbitrages and sandwiches
/// upcoming: liquidations? cex-dex? jit?
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MevEvent {
    Arbitrage(ArbitrageEvent),
    Sandwich(SandwichEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArbitrageType {
    TriangleArbitrage,
    StablecoinArbitrage,
    CrossPairArbitrage,
    LongTail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageEvent {
    pub signature: String,
    pub signer: String,
    pub compute_units_consumed: u64,
    pub fee: u64,
    pub priority_fee: u64,
    pub jito_tip: u64,
    pub swaps: Vec<SwapInfo>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<SimpleTokenChange>,
    pub profitability: Profitability,
    pub arbitrage_type: ArbitrageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichEvent {
    pub slot: u64,
    pub signer: String,
    pub sandwiched_token: String, // idc about victim txs as much as i care about the victim token (never forget WET)
    pub front_run: SandwichTransaction,
    pub back_run: SandwichTransaction,
    pub total_compute_units: u64,
    pub total_fees: u64,
    pub total_jito_tips: u64,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<SimpleTokenChange>,
    pub profitability: Profitability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichTransaction {
    pub signature: String,
    pub index: usize,
    pub signer: String,
    pub compute_units: u64,
    pub fee: u64,
    pub swap: Vec<SwapInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profitability {
    pub revenue_usd: f64,
    pub fees_usd: f64,
    pub profit_usd: f64,
    // pay for pyth pls
    pub unsupported_profit_tokens: Vec<String>,
}
