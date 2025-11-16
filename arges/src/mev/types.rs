//! Core types for MEV detection and classification

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a detected MEV opportunity or extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevEvent {
    /// Type of MEV detected
    pub mev_type: MevType,

    /// Slot number where MEV occurred
    pub slot: u64,

    /// Timestamp of the block
    pub timestamp: DateTime<Utc>,

    /// Transaction signatures involved
    pub transactions: Vec<String>,

    /// Estimated profit in lamports (or token units)
    pub profit_lamports: Option<i64>,

    /// Profit in USD (if price data available)
    pub profit_usd: Option<f64>,

    /// Tokens involved in the MEV
    pub tokens: Vec<String>,

    /// Additional metadata specific to MEV type
    pub metadata: MevMetadata,

    /// The extractor's address
    pub extractor: Option<String>,

    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

/// Types of MEV that can be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MevType {
    /// Cross-DEX arbitrage
    Arbitrage,

    /// Sandwich attack (frontrun + backrun)
    Sandwich,

    /// Liquidation on lending protocols
    Liquidation,

    /// Just-in-time liquidity provision
    JitLiquidity,

    /// CEX-DEX arbitrage
    CexDex,

    /// Atomic backrun (following a large trade)
    AtomicBackrun,

    /// Priority fee manipulation
    PriorityFee,

    /// MEV via Jito bundle
    JitoBundle,

    /// Other/unknown MEV pattern
    Other,
}

/// Metadata specific to different MEV types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MevMetadata {
    Arbitrage(ArbitrageMetadata),
    Sandwich(SandwichMetadata),
    Liquidation(LiquidationMetadata),
    JitLiquidity(JitMetadata),
    CexDex(crate::mev::cex_dex::CexDexMetadata),
    AtomicBackrun(BackrunMetadata),
    PriorityFee(PriorityFeeMetadata),
    JitoBundle(JitoBundleMetadata),
    Other(HashMap<String, String>),
}

/// Arbitrage-specific metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageMetadata {
    /// DEXs involved in the arbitrage
    pub dexs: Vec<String>,

    /// Token path (e.g., SOL -> USDC -> SOL)
    pub token_path: Vec<String>,

    /// Swap details for each leg
    pub swaps: Vec<SwapDetails>,

    /// Total input amount
    pub input_amount: u64,

    /// Total output amount
    pub output_amount: u64,

    /// Net profit (output - input)
    pub net_profit: i64,

    /// Number of hops in the arbitrage
    pub hop_count: usize,
}

/// Sandwich attack metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandwichMetadata {
    /// Frontrun transaction signature
    pub frontrun_tx: String,

    /// Victim transaction signature
    pub victim_tx: String,

    /// Backrun transaction signature
    pub backrun_tx: String,

    /// Token being sandwiched
    pub token: String,

    /// Victim's swap details
    pub victim_swap: SwapDetails,

    /// Frontrun swap details
    pub frontrun_swap: SwapDetails,

    /// Backrun swap details
    pub backrun_swap: SwapDetails,

    /// Estimated victim loss
    pub victim_loss: Option<i64>,

    /// Sandwicher profit
    pub profit: i64,

    /// Pool address being attacked
    pub pool: String,
}

/// Liquidation metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationMetadata {
    /// Lending protocol (e.g., "Solend", "Mango", "MarginFi")
    pub protocol: String,

    /// Account being liquidated
    pub liquidated_account: String,

    /// Liquidator's address
    pub liquidator: String,

    /// Assets seized
    pub assets_seized: Vec<AssetAmount>,

    /// Debts repaid
    pub debts_repaid: Vec<AssetAmount>,

    /// Liquidation bonus/profit
    pub liquidation_bonus: i64,

    /// Health factor before liquidation
    pub health_factor_before: Option<f64>,
}

/// JIT liquidity metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitMetadata {
    /// Add liquidity transaction
    pub add_liquidity_tx: String,

    /// Remove liquidity transaction
    pub remove_liquidity_tx: String,

    /// Large swap transaction that triggered JIT
    pub target_swap_tx: String,

    /// Pool address
    pub pool: String,

    /// DEX protocol
    pub dex: String,

    /// Liquidity amount added
    pub liquidity_added: u64,

    /// Fees earned from the target swap
    pub fees_earned: u64,

    /// Net profit after IL and gas
    pub net_profit: i64,
}

/// Atomic backrun metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackrunMetadata {
    /// The transaction being backrun
    pub target_tx: String,

    /// The backrun transaction
    pub backrun_tx: String,

    /// Time difference in slots
    pub slot_distance: u64,

    /// Target swap details
    pub target_swap: SwapDetails,

    /// Backrun swap details
    pub backrun_swap: SwapDetails,

    /// Profit from backrun
    pub profit: i64,
}

/// Priority fee MEV metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorityFeeMetadata {
    /// Priority fee paid (in lamports)
    pub priority_fee: u64,

    /// Base fee
    pub base_fee: u64,

    /// Ratio of priority to base fee
    pub fee_ratio: f64,

    /// Transaction position in block
    pub position: usize,

    /// Whether transaction was first in block
    pub first_in_block: bool,
}

/// Jito bundle metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JitoBundleMetadata {
    /// Bundle ID if available
    pub bundle_id: Option<String>,

    /// Tip paid to validator (in lamports)
    pub tip_amount: u64,

    /// Transactions in the bundle
    pub bundle_transactions: Vec<String>,

    /// Whether bundle was successful
    pub bundle_success: bool,

    /// Bundle size
    pub bundle_size: usize,
}

/// Details of a swap transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapDetails {
    /// DEX where swap occurred
    pub dex: String,

    /// Pool address
    pub pool: String,

    /// Input token mint
    pub token_in: String,

    /// Output token mint
    pub token_out: String,

    /// Amount in
    pub amount_in: u64,

    /// Amount out
    pub amount_out: u64,

    /// Price impact percentage
    pub price_impact: Option<f64>,

    /// Minimum amount out specified
    pub min_amount_out: Option<u64>,

    /// Transaction signature
    pub signature: String,

    /// Transaction index in block
    pub tx_index: usize,
}

/// Asset amount for liquidations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAmount {
    /// Token mint address
    pub token: String,

    /// Amount in token's smallest unit
    pub amount: u64,

    /// USD value if available
    pub usd_value: Option<f64>,
}

/// Aggregated MEV metrics for a slot
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlotMevMetrics {
    /// Slot number
    pub slot: u64,

    /// Timestamp
    pub timestamp: DateTime<Utc>,

    /// Total MEV events detected
    pub total_events: usize,

    /// Events by type
    pub events_by_type: HashMap<MevType, usize>,

    /// Total profit extracted (lamports)
    pub total_profit_lamports: i64,

    /// Total profit in USD
    pub total_profit_usd: Option<f64>,

    /// Number of unique extractors
    pub unique_extractors: usize,

    /// Top extractor by profit
    pub top_extractor: Option<String>,

    /// Average profit per MEV event
    pub avg_profit: f64,

    /// Number of failed MEV attempts
    pub failed_attempts: usize,

    /// Total Jito tips paid
    pub total_jito_tips: u64,
}

/// Aggregated MEV metrics for an epoch
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EpochMevMetrics {
    /// Epoch number
    pub epoch: u64,

    /// Start slot
    pub start_slot: u64,

    /// End slot
    pub end_slot: u64,

    /// Total MEV events
    pub total_events: usize,

    /// Events by type
    pub events_by_type: HashMap<MevType, usize>,

    /// Total profit extracted (lamports)
    pub total_profit_lamports: i64,

    /// Total profit in USD
    pub total_profit_usd: Option<f64>,

    /// Top 10 extractors by profit
    pub top_extractors: Vec<(String, i64)>,

    /// MEV by validator
    pub mev_by_validator: HashMap<String, i64>,

    /// Average MEV per slot
    pub avg_mev_per_slot: f64,

    /// Slots with MEV activity
    pub slots_with_mev: usize,

    /// Total slots in epoch
    pub total_slots: usize,

    /// MEV concentration (% of MEV by top 10 extractors)
    pub concentration_ratio: f64,
}

impl MevType {
    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            MevType::Arbitrage => "Arbitrage",
            MevType::Sandwich => "Sandwich Attack",
            MevType::Liquidation => "Liquidation",
            MevType::JitLiquidity => "JIT Liquidity",
            MevType::CexDex => "CEX-DEX Arbitrage",
            MevType::AtomicBackrun => "Atomic Backrun",
            MevType::PriorityFee => "Priority Fee MEV",
            MevType::JitoBundle => "Jito Bundle",
            MevType::Other => "Other",
        }
    }
}

impl SlotMevMetrics {
    /// Create new metrics for a slot
    pub fn new(slot: u64, timestamp: DateTime<Utc>) -> Self {
        Self {
            slot,
            timestamp,
            ..Default::default()
        }
    }

    /// Add an MEV event to the metrics
    pub fn add_event(&mut self, event: &MevEvent) {
        self.total_events += 1;
        *self.events_by_type.entry(event.mev_type).or_insert(0) += 1;

        if let Some(profit) = event.profit_lamports {
            self.total_profit_lamports += profit;
        }

        if let Some(profit_usd) = event.profit_usd {
            let current = self.total_profit_usd.unwrap_or(0.0);
            self.total_profit_usd = Some(current + profit_usd);
        }

        // Update average
        if self.total_events > 0 {
            self.avg_profit = self.total_profit_lamports as f64 / self.total_events as f64;
        }
    }
}
