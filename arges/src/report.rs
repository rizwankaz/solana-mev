use crate::mev::{MevSummary, ProgramRegistry};
use crate::types::FetchedBlock;

/// Format a comprehensive block report including MEV analysis
pub fn format_block_report(block: &FetchedBlock) -> String {
    let mut report = String::new();

    // Header
    report.push_str(&format!("╔═══════════════════════════════════════════════════════════════╗\n"));
    report.push_str(&format!("║                        BLOCK REPORT                           ║\n"));
    report.push_str(&format!("╚═══════════════════════════════════════════════════════════════╝\n"));
    report.push_str("\n");

    // Basic block info
    report.push_str(&format!("Block Number:        {}\n", block.slot));
    report.push_str(&format!("Block Hash:          {}\n", &block.blockhash));
    report.push_str(&format!("Parent Slot:         {}\n", block.parent_slot));

    if let Some(height) = block.block_height {
        report.push_str(&format!("Block Height:        {}\n", height));
    }

    if let Some(timestamp) = block.timestamp() {
        report.push_str(&format!("Timestamp:           {}\n", timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
    }

    report.push_str("\n");

    // Transaction statistics
    report.push_str("─────────────────────── TRANSACTIONS ──────────────────────────\n");
    report.push_str(&format!("Total Transactions:  {}\n", block.transactions.len()));
    report.push_str(&format!("Successful:          {}\n", block.successful_tx_count()));
    report.push_str(&format!("Failed:              {}\n", block.failed_tx_count()));
    report.push_str(&format!("Total Fees:          {} SOL\n", lamports_to_sol(block.total_fees())));
    report.push_str(&format!("Compute Units:       {}\n", format_compute_units(block.total_compute_units())));

    report.push_str("\n");

    // MEV Analysis
    let mev = block.analyze_mev();
    report.push_str("─────────────────────── MEV ANALYSIS ───────────────────────────\n");

    if mev.total_mev_count() == 0 && mev.spam_count == 0 {
        report.push_str("No MEV activity detected in this block.\n");
    } else {
        report.push_str(&format_mev_summary(&mev));
    }

    report.push_str("\n");

    // Rewards
    if !block.rewards.is_empty() {
        report.push_str("──────────────────────── REWARDS ───────────────────────────────\n");
        let total_rewards: i64 = block.rewards.iter().map(|r| r.lamports).sum();
        report.push_str(&format!("Total Rewards:       {} SOL ({} recipients)\n",
            lamports_to_sol(total_rewards as u64),
            block.rewards.len()));
        report.push_str("\n");
    }

    report.push_str("═══════════════════════════════════════════════════════════════\n");

    report
}

/// Format MEV summary section
fn format_mev_summary(mev: &MevSummary) -> String {
    let mut output = String::new();

    // MEV totals
    output.push_str(&format!("Total MEV Events:    {}\n", mev.total_mev_count()));
    output.push_str(&format!("Total MEV Value:     {} SOL\n", lamports_to_sol(mev.total_value())));
    output.push_str(&format!("Spam/Failed MEV:     {}\n", mev.spam_count));
    output.push_str("\n");

    // Breakdown by category
    if mev.arbitrage_count > 0 {
        output.push_str(&format!("  🔄 Arbitrage:      {} transactions, {} SOL\n",
            mev.arbitrage_count,
            lamports_to_sol(mev.arbitrage_value)));
    }

    if mev.liquidation_count > 0 {
        output.push_str(&format!("  💧 Liquidations:   {} transactions, {} SOL\n",
            mev.liquidation_count,
            lamports_to_sol(mev.liquidation_value)));
    }

    if mev.mint_count > 0 {
        output.push_str(&format!("  🪙 Mints:          {} transactions\n", mev.mint_count));
    }

    // Programs used
    if !mev.programs_used.is_empty() {
        output.push_str("\nPrograms Involved:\n");

        // Sort by frequency
        let mut programs: Vec<_> = mev.programs_used.iter().collect();
        programs.sort_by(|a, b| b.1.cmp(a.1));

        for (program_id, count) in programs.iter().take(10) {
            let name = ProgramRegistry::program_name(program_id);
            output.push_str(&format!("  • {:<25} {} uses\n", name, count));
        }

        if programs.len() > 10 {
            output.push_str(&format!("  ... and {} more programs\n", programs.len() - 10));
        }
    }

    output
}

/// Format a compact summary for streaming blocks
pub fn format_compact_summary(slot: u64, block: &FetchedBlock) -> String {
    let mev = block.analyze_mev();

    let mut summary = format!(
        "Slot {}: {} txs ({} success, {} fail)",
        slot,
        block.transactions.len(),
        block.successful_tx_count(),
        block.failed_tx_count()
    );

    if mev.total_mev_count() > 0 {
        summary.push_str(&format!(
            " | MEV: {} arb, {} liq, {} mint, {} spam",
            mev.arbitrage_count,
            mev.liquidation_count,
            mev.mint_count,
            mev.spam_count
        ));
    }

    summary.push_str(&format!(" | {} SOL fees", lamports_to_sol(block.total_fees())));

    summary
}

/// Convert lamports to SOL with proper formatting
fn lamports_to_sol(lamports: u64) -> String {
    let sol = lamports as f64 / 1_000_000_000.0;
    format!("{:.9}", sol).trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Format compute units with comma separators
fn format_compute_units(cu: u64) -> String {
    let s = cu.to_string();
    let mut result = String::new();
    let mut count = 0;

    for c in s.chars().rev() {
        if count == 3 {
            result.push(',');
            count = 0;
        }
        result.push(c);
        count += 1;
    }

    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lamports_to_sol() {
        assert_eq!(lamports_to_sol(1_000_000_000), "1");
        assert_eq!(lamports_to_sol(500_000_000), "0.5");
        assert_eq!(lamports_to_sol(123_456_789), "0.123456789");
        assert_eq!(lamports_to_sol(100_000_000), "0.1");
    }

    #[test]
    fn test_format_compute_units() {
        assert_eq!(format_compute_units(1000), "1,000");
        assert_eq!(format_compute_units(1000000), "1,000,000");
        assert_eq!(format_compute_units(123456789), "123,456,789");
    }
}
