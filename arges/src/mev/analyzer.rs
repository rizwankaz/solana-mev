use crate::types::FetchedBlock;
use crate::mev::types::{MevBlockSummary, MevTransaction};
use crate::mev::detectors::{
    atomic_arb::AtomicArbDetector,
    sandwich::SandwichDetector,
    jit_liquidity::JitLiquidityDetector,
    liquidation::LiquidationDetector,
};
use crate::mev::instruction_parser::TransactionFilter;
use tracing::{info, debug};

/// MEV Analyzer
///
/// Coordinates all MEV detection algorithms to analyze Solana blocks
/// and identify successful MEV transactions.
///
/// This is the main entry point for MEV detection. It:
/// 1. Takes a fetched Solana block
/// 2. Runs all detection algorithms in parallel
/// 3. Aggregates results into a comprehensive summary
/// 4. Provides statistics about MEV activity
pub struct MevAnalyzer;

impl MevAnalyzer {
    /// Analyze a block for MEV activity
    ///
    /// Runs all detection algorithms and returns a comprehensive summary
    /// of MEV transactions found in the block.
    ///
    /// # Arguments
    /// * `block` - The fetched Solana block to analyze
    ///
    /// # Returns
    /// A `MevBlockSummary` containing all detected MEV transactions and statistics
    pub fn analyze_block(block: &FetchedBlock) -> MevBlockSummary {
        info!("Analyzing block {} for MEV activity", block.slot);

        let mut summary = MevBlockSummary::new(
            block.slot,
            block.block_time,
            block.transactions.len(),
            block.successful_tx_count(),
        );

        // Filter successful transactions
        let successful_txs: Vec<_> = block
            .transactions
            .iter()
            .filter(|tx| tx.is_success())
            .cloned()
            .collect();

        if successful_txs.is_empty() {
            debug!("Block {} has no successful transactions", block.slot);
            return summary;
        }

        info!(
            "Block {}: analyzing {} successful transactions",
            block.slot,
            successful_txs.len()
        );

        // Step 1: Filter transactions by type using instruction analysis
        debug!("Filtering transactions by instruction type...");

        let swap_txs = TransactionFilter::filter_swaps(&successful_txs);
        let liquidation_txs = TransactionFilter::filter_liquidations(&successful_txs);
        let liquidity_ops = TransactionFilter::filter_liquidity_ops(&successful_txs);

        info!(
            "Block {}: filtered to {} swaps, {} liquidations, {} liquidity ops",
            block.slot,
            swap_txs.len(),
            liquidation_txs.len(),
            liquidity_ops.len()
        );

        // Step 2: Run MEV detectors on filtered transactions
        // This is much more efficient than scanning all transactions

        // 1. Detect atomic arbitrage (from swap transactions)
        debug!("Running atomic arbitrage detector on {} swap transactions...", swap_txs.len());
        let atomic_arbs = AtomicArbDetector::detect_batch(&swap_txs, block.slot);
        info!(
            "Block {}: found {} atomic arbitrage transactions",
            block.slot,
            atomic_arbs.len()
        );

        for arb in atomic_arbs {
            summary.add_mev_transaction(MevTransaction::AtomicArbitrage(arb));
        }

        // 2. Detect sandwich attacks (from swap transactions)
        debug!("Running sandwich detector on {} swap transactions...", swap_txs.len());
        let swap_tx_refs: Vec<_> = swap_txs.iter().collect();
        let sandwiches = SandwichDetector::detect_in_block(&swap_tx_refs, block.slot);
        info!(
            "Block {}: found {} sandwich attacks",
            block.slot,
            sandwiches.len()
        );

        for sandwich in sandwiches {
            summary.add_mev_transaction(MevTransaction::Sandwich(sandwich));
        }

        // 3. Detect JIT liquidity (from liquidity operations)
        debug!("Running JIT liquidity detector on {} liquidity ops...", liquidity_ops.len());
        let liquidity_op_txs: Vec<_> = liquidity_ops.iter().map(|(tx, _)| tx).cloned().collect();
        let liquidity_tx_refs: Vec<_> = liquidity_op_txs.iter().collect();
        let jit_attacks = JitLiquidityDetector::detect_in_block(&liquidity_tx_refs, block.slot);
        info!(
            "Block {}: found {} JIT liquidity attacks",
            block.slot,
            jit_attacks.len()
        );

        for jit in jit_attacks {
            summary.add_mev_transaction(MevTransaction::JitLiquidity(jit));
        }

        // 4. Detect liquidations (from liquidation transactions)
        debug!("Running liquidation detector on {} liquidation transactions...", liquidation_txs.len());
        let liquidations = LiquidationDetector::detect_in_block(&liquidation_txs, block.slot);
        info!(
            "Block {}: found {} profitable liquidations",
            block.slot,
            liquidations.len()
        );

        for liquidation in liquidations {
            summary.add_mev_transaction(MevTransaction::Liquidation(liquidation));
        }

        info!(
            "Block {}: total MEV transactions detected: {}",
            block.slot,
            summary.stats.total_mev_count
        );

        summary
    }

    /// Analyze multiple blocks
    ///
    /// Convenience method to analyze a batch of blocks
    pub fn analyze_blocks(blocks: &[FetchedBlock]) -> Vec<MevBlockSummary> {
        blocks.iter().map(Self::analyze_block).collect()
    }

    /// Format MEV summary as JSON
    ///
    /// Converts the MEV block summary to a pretty-printed JSON string
    pub fn to_json(summary: &MevBlockSummary) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(summary)
    }

    /// Format MEV summary as compact JSON (no pretty printing)
    pub fn to_json_compact(summary: &MevBlockSummary) -> Result<String, serde_json::Error> {
        serde_json::to_string(summary)
    }

    /// Get high-level statistics from summary
    pub fn get_stats_summary(summary: &MevBlockSummary) -> String {
        format!(
            r#"
MEV Block Summary - Slot {}
=====================================
Total Transactions:     {}
Successful Transactions: {}
MEV Transactions:       {}

Breakdown by Type:
- Atomic Arbitrage:     {}
- Sandwich Attacks:     {}
- JIT Liquidity:        {}
- Liquidations:         {}

Financial Impact:
- Total MEV Profit:     {} lamports ({} SOL)
- Total Victim Loss:    {} lamports ({} SOL)
"#,
            summary.slot,
            summary.total_transactions,
            summary.successful_transactions,
            summary.stats.total_mev_count,
            summary.stats.atomic_arbitrage_count,
            summary.stats.sandwich_count,
            summary.stats.jit_liquidity_count,
            summary.stats.liquidation_count,
            summary.stats.total_profit_lamports,
            summary.stats.total_profit_lamports as f64 / 1_000_000_000.0,
            summary.stats.total_victim_loss_lamports,
            summary.stats.total_victim_loss_lamports as f64 / 1_000_000_000.0,
        )
    }

    /// Filter MEV transactions by type
    pub fn filter_by_type(
        summary: &MevBlockSummary,
        mev_type: MevType,
    ) -> Vec<&MevTransaction> {
        summary
            .mev_transactions
            .iter()
            .filter(|tx| match (mev_type, tx) {
                (MevType::AtomicArbitrage, MevTransaction::AtomicArbitrage(_)) => true,
                (MevType::Sandwich, MevTransaction::Sandwich(_)) => true,
                (MevType::JitLiquidity, MevTransaction::JitLiquidity(_)) => true,
                (MevType::Liquidation, MevTransaction::Liquidation(_)) => true,
                _ => false,
            })
            .collect()
    }

    /// Get top N most profitable MEV transactions
    pub fn top_profitable(summary: &MevBlockSummary, n: usize) -> Vec<&MevTransaction> {
        let mut txs: Vec<_> = summary.mev_transactions.iter().collect();

        txs.sort_by_key(|tx| {
            std::cmp::Reverse(match tx {
                MevTransaction::AtomicArbitrage(arb) => arb.profit_lamports,
                MevTransaction::Sandwich(sw) => sw.profit_lamports,
                MevTransaction::JitLiquidity(jit) => jit.profit_lamports,
                MevTransaction::Liquidation(liq) => liq.profit_lamports,
            })
        });

        txs.into_iter().take(n).collect()
    }
}

/// MEV transaction type filter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MevType {
    AtomicArbitrage,
    Sandwich,
    JitLiquidity,
    Liquidation,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_analyzer_creation() {
        // Test analyzer can be created
    }

    #[test]
    fn test_block_analysis() {
        // Test block analysis with mock data
    }

    #[test]
    fn test_filtering() {
        // Test filtering by MEV type
    }
}
