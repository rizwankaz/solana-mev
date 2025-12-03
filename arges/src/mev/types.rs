use serde::{Deserialize, Serialize};

/// Represents different types of MEV transactions detected
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "mev_type", rename_all = "snake_case")]
pub enum MevTransaction {
    /// Atomic arbitrage: multiple swaps in single tx exploiting price differences
    AtomicArbitrage(AtomicArbitrage),

    /// Sandwich attack: front-run + victim + back-run pattern
    Sandwich(Sandwich),

    /// JIT liquidity: add liquidity → swap → remove liquidity pattern
    JitLiquidity(JitLiquidity),

    /// Liquidation of undercollateralized positions
    Liquidation(Liquidation),
}

/// Atomic arbitrage transaction
///
/// Based on Brontes methodology: detects single transactions with multiple swaps
/// across different pools that result in net profit by exploiting price differences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AtomicArbitrage {
    /// Transaction signature
    pub signature: String,

    /// Slot number
    pub slot: u64,

    /// Transaction index within block
    pub tx_index: usize,

    /// Searcher/bot address
    pub searcher: String,

    /// Sequence of swaps executed
    pub swaps: Vec<SwapInfo>,

    /// Net profit in lamports (after fees)
    pub profit_lamports: i64,

    /// Estimated profit in USD (if price data available)
    pub profit_usd: Option<f64>,

    /// Total compute units consumed
    pub compute_units: u64,

    /// Transaction fee paid
    pub fee_lamports: u64,

    /// Pools involved in the arbitrage
    pub pools: Vec<String>,

    /// Token route (e.g., SOL -> USDC -> RAY -> SOL)
    pub token_route: Vec<String>,
}

/// Sandwich attack transaction bundle
///
/// Based on Brontes and Sandwiched.me methodology:
/// - Front-run and back-run must swap on at least one common pool
/// - Victim transactions are interleaved between front/back runs
/// - Victim txs grouped by EOA to handle multi-step operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Sandwich {
    /// Slot number
    pub slot: u64,

    /// Attacker/searcher address
    pub attacker: String,

    /// Front-run transaction
    pub frontrun: SandwichTx,

    /// Victim transaction(s)
    pub victims: Vec<VictimTx>,

    /// Back-run transaction
    pub backrun: SandwichTx,

    /// Common pools between front and back runs
    pub common_pools: Vec<String>,

    /// Total profit extracted in lamports
    pub profit_lamports: i64,

    /// Estimated profit in USD
    pub profit_usd: Option<f64>,

    /// Total victim loss in lamports
    pub victim_loss_lamports: i64,

    /// Estimated victim loss in USD
    pub victim_loss_usd: Option<f64>,
}

/// Individual transaction within a sandwich
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SandwichTx {
    /// Transaction signature
    pub signature: String,

    /// Transaction index within block
    pub tx_index: usize,

    /// Swap details
    pub swap: SwapInfo,

    /// Compute units consumed
    pub compute_units: u64,

    /// Fee paid
    pub fee_lamports: u64,
}

/// Victim transaction in a sandwich
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VictimTx {
    /// Transaction signature
    pub signature: String,

    /// Transaction index within block
    pub tx_index: usize,

    /// Victim's address
    pub victim_address: String,

    /// Swap details
    pub swap: SwapInfo,

    /// Estimated loss due to sandwich in lamports
    pub loss_lamports: i64,

    /// Estimated loss in USD
    pub loss_usd: Option<f64>,
}

/// JIT (Just-In-Time) liquidity provision
///
/// Based on Brontes methodology:
/// - Attacker provides concentrated liquidity immediately before large swap
/// - Liquidity positioned at exact ticks that will be active during swap
/// - Liquidity removed immediately after, collecting swap fees
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JitLiquidity {
    /// Slot number
    pub slot: u64,

    /// Searcher address
    pub searcher: String,

    /// Add liquidity transaction
    pub add_liquidity: LiquidityTx,

    /// Victim swap transaction
    pub victim_swap: VictimTx,

    /// Remove liquidity transaction
    pub remove_liquidity: LiquidityTx,

    /// Pool address
    pub pool: String,

    /// Fees collected from the swap
    pub fees_collected_lamports: i64,

    /// Estimated fees in USD
    pub fees_collected_usd: Option<f64>,

    /// Total profit (fees - gas costs)
    pub profit_lamports: i64,

    /// Estimated profit in USD
    pub profit_usd: Option<f64>,
}

/// Liquidity add/remove transaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiquidityTx {
    /// Transaction signature
    pub signature: String,

    /// Transaction index within block
    pub tx_index: usize,

    /// Liquidity amount tokens
    pub amount_a: u64,
    pub amount_b: u64,

    /// Token addresses
    pub token_a: String,
    pub token_b: String,

    /// Tick range for concentrated liquidity (if applicable)
    pub tick_lower: Option<i32>,
    pub tick_upper: Option<i32>,

    /// Compute units consumed
    pub compute_units: u64,

    /// Fee paid
    pub fee_lamports: u64,
}

/// Liquidation event
///
/// Based on Brontes methodology:
/// - Detects liquidation of undercollateralized positions
/// - Calculates profitability using DEX pricing data
/// - Profit = revenue (seized collateral) - debt repaid - gas costs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Liquidation {
    /// Transaction signature
    pub signature: String,

    /// Slot number
    pub slot: u64,

    /// Transaction index within block
    pub tx_index: usize,

    /// Liquidator address
    pub liquidator: String,

    /// Liquidated user address
    pub liquidated_user: String,

    /// Lending protocol
    pub protocol: String,

    /// Debt repaid
    pub debt_repaid: Vec<TokenAmount>,

    /// Collateral seized
    pub collateral_seized: Vec<TokenAmount>,

    /// Revenue in lamports (USD value of seized collateral)
    pub revenue_lamports: i64,

    /// Revenue in USD
    pub revenue_usd: Option<f64>,

    /// Cost in lamports (debt repaid + gas)
    pub cost_lamports: i64,

    /// Cost in USD
    pub cost_usd: Option<f64>,

    /// Net profit
    pub profit_lamports: i64,

    /// Estimated profit in USD
    pub profit_usd: Option<f64>,

    /// Compute units consumed
    pub compute_units: u64,

    /// Transaction fee
    pub fee_lamports: u64,
}

/// Token amount with metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TokenAmount {
    /// Token mint address
    pub token: String,

    /// Amount in base units
    pub amount: u64,

    /// Token decimals
    pub decimals: u8,

    /// Human readable amount
    pub amount_ui: f64,

    /// USD value if available
    pub usd_value: Option<f64>,
}

/// Information about a swap operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SwapInfo {
    /// Pool/AMM program address
    pub pool: String,

    /// Token in
    pub token_in: String,

    /// Token out
    pub token_out: String,

    /// Amount in (base units)
    pub amount_in: u64,

    /// Amount out (base units)
    pub amount_out: u64,

    /// AMM/DEX program ID
    pub program_id: String,

    /// Swap direction indicator (optional)
    pub direction: Option<SwapDirection>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SwapDirection {
    Buy,
    Sell,
}

/// Summary of MEV analysis for a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevBlockSummary {
    /// Block slot
    pub slot: u64,

    /// Block timestamp
    pub timestamp: Option<i64>,

    /// Total transactions in block
    pub total_transactions: usize,

    /// Successful transactions
    pub successful_transactions: usize,

    /// Detected MEV transactions
    pub mev_transactions: Vec<MevTransaction>,

    /// MEV statistics
    pub stats: MevStats,
}

/// Statistics about MEV in a block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevStats {
    /// Total MEV transactions detected
    pub total_mev_count: usize,

    /// Count by type
    pub atomic_arbitrage_count: usize,
    pub sandwich_count: usize,
    pub jit_liquidity_count: usize,
    pub liquidation_count: usize,

    /// Total MEV profit extracted (lamports)
    pub total_profit_lamports: i64,

    /// Total MEV profit in USD
    pub total_profit_usd: Option<f64>,

    /// Total victim losses (lamports)
    pub total_victim_loss_lamports: i64,

    /// Total victim losses in USD
    pub total_victim_loss_usd: Option<f64>,
}

impl MevBlockSummary {
    /// Create a new MEV block summary
    pub fn new(slot: u64, timestamp: Option<i64>, total_tx: usize, successful_tx: usize) -> Self {
        Self {
            slot,
            timestamp,
            total_transactions: total_tx,
            successful_transactions: successful_tx,
            mev_transactions: Vec::new(),
            stats: MevStats {
                total_mev_count: 0,
                atomic_arbitrage_count: 0,
                sandwich_count: 0,
                jit_liquidity_count: 0,
                liquidation_count: 0,
                total_profit_lamports: 0,
                total_profit_usd: None,
                total_victim_loss_lamports: 0,
                total_victim_loss_usd: None,
            },
        }
    }

    /// Add MEV transaction and update stats
    pub fn add_mev_transaction(&mut self, mev_tx: MevTransaction) {
        // Update counts
        self.stats.total_mev_count += 1;

        match &mev_tx {
            MevTransaction::AtomicArbitrage(arb) => {
                self.stats.atomic_arbitrage_count += 1;
                self.stats.total_profit_lamports += arb.profit_lamports;
                if let (Some(total_usd), Some(arb_usd)) = (&mut self.stats.total_profit_usd, arb.profit_usd) {
                    *total_usd += arb_usd;
                } else if let Some(arb_usd) = arb.profit_usd {
                    self.stats.total_profit_usd = Some(arb_usd);
                }
            },
            MevTransaction::Sandwich(sandwich) => {
                self.stats.sandwich_count += 1;
                self.stats.total_profit_lamports += sandwich.profit_lamports;
                self.stats.total_victim_loss_lamports += sandwich.victim_loss_lamports;

                if let (Some(total_usd), Some(profit_usd)) = (&mut self.stats.total_profit_usd, sandwich.profit_usd) {
                    *total_usd += profit_usd;
                } else if let Some(profit_usd) = sandwich.profit_usd {
                    self.stats.total_profit_usd = Some(profit_usd);
                }

                if let (Some(total_loss_usd), Some(loss_usd)) = (&mut self.stats.total_victim_loss_usd, sandwich.victim_loss_usd) {
                    *total_loss_usd += loss_usd;
                } else if let Some(loss_usd) = sandwich.victim_loss_usd {
                    self.stats.total_victim_loss_usd = Some(loss_usd);
                }
            },
            MevTransaction::JitLiquidity(jit) => {
                self.stats.jit_liquidity_count += 1;
                self.stats.total_profit_lamports += jit.profit_lamports;
                if let (Some(total_usd), Some(jit_usd)) = (&mut self.stats.total_profit_usd, jit.profit_usd) {
                    *total_usd += jit_usd;
                } else if let Some(jit_usd) = jit.profit_usd {
                    self.stats.total_profit_usd = Some(jit_usd);
                }
            },
            MevTransaction::Liquidation(liq) => {
                self.stats.liquidation_count += 1;
                self.stats.total_profit_lamports += liq.profit_lamports;
                if let (Some(total_usd), Some(liq_usd)) = (&mut self.stats.total_profit_usd, liq.profit_usd) {
                    *total_usd += liq_usd;
                } else if let Some(liq_usd) = liq.profit_usd {
                    self.stats.total_profit_usd = Some(liq_usd);
                }
            },
        }

        self.mev_transactions.push(mev_tx);
    }
}
