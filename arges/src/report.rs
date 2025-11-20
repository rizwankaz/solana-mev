use crate::mev::{MevSummary, ProgramRegistry, MevEvent, MultiTxMevEvent, MevAnalyzer};
use crate::types::FetchedBlock;
use serde::Serialize;

/// Format a comprehensive block report including MEV analysis
pub fn format_block_report(block: &FetchedBlock) -> String {
    let mut report = String::new();

    // Header
    report.push_str(&format!("╔═══════════════════════════════════════════════════════════════╗\n"));
    report.push_str(&format!("║                        BLOCK REPORT                           ║\n"));
    report.push_str(&format!("╚═══════════════════════════════════════════════════════════════╝\n"));
    report.push_str("\n");

    // Basic block info
    report.push_str(&format!("Slot Number:         {}\n", block.slot));
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
    output.push_str(&format!("Spam/Failed MEV:     {}\n", mev.spam_count));

    // SOL change (usually negative due to fees)
    let sol_change_str = if mev.total_sol_change >= 0 {
        format!("+{}", lamports_to_sol(mev.total_sol_change as u64))
    } else {
        format!("-{}", lamports_to_sol((-mev.total_sol_change) as u64))
    };
    output.push_str(&format!("Net SOL Change:      {} SOL\n", sol_change_str));
    output.push_str("\n");

    // Breakdown by category
    if mev.arbitrage_count > 0 {
        output.push_str(&format!("  🔄 Arbitrage:      {} transactions\n", mev.arbitrage_count));
        if !mev.arbitrage_token_profits.is_empty() {
            // Show top 3 profitable tokens
            let mut profits: Vec<_> = mev.arbitrage_token_profits.iter().collect();
            profits.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (mint, profit) in profits.iter().take(3) {
                let mint_short = truncate_mint(mint);
                output.push_str(&format!("     • {}: {:+.6}\n", mint_short, profit));
            }
            if profits.len() > 3 {
                output.push_str(&format!("     ... and {} more tokens\n", profits.len() - 3));
            }
        }
    }

    if mev.liquidation_count > 0 {
        output.push_str(&format!("  💧 Liquidations:   {} transactions\n", mev.liquidation_count));
        if !mev.liquidation_token_profits.is_empty() {
            // Show top 3 profitable tokens
            let mut profits: Vec<_> = mev.liquidation_token_profits.iter().collect();
            profits.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (mint, profit) in profits.iter().take(3) {
                let mint_short = truncate_mint(mint);
                output.push_str(&format!("     • {}: {:+.6}\n", mint_short, profit));
            }
            if profits.len() > 3 {
                output.push_str(&format!("     ... and {} more tokens\n", profits.len() - 3));
            }
        }
    }

    if mev.mint_count > 0 {
        output.push_str(&format!("  🪙 Mints:          {} transactions\n", mev.mint_count));
        if !mev.minted_tokens.is_empty() {
            // Show top 3 minted tokens by volume
            let mut mints: Vec<_> = mev.minted_tokens.iter().collect();
            mints.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

            for (mint, amount) in mints.iter().take(3) {
                let mint_short = truncate_mint(mint);
                output.push_str(&format!("     • {}: {:.6}\n", mint_short, amount));
            }
            if mints.len() > 3 {
                output.push_str(&format!("     ... and {} more mints\n", mints.len() - 3));
            }
        }
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

/// Truncate mint address for display
fn truncate_mint(mint: &str) -> String {
    if mint.len() > 12 {
        format!("{}...{}", &mint[..6], &mint[mint.len()-4..])
    } else {
        mint.to_string()
    }
}

/// Format MEV-only report for manual validation
pub fn format_mev_validation_report(block: &FetchedBlock) -> String {
    let mut report = String::new();
    let mut mev_events: Vec<(usize, MevEvent)> = Vec::new();

    // Collect all MEV events with their transaction indices
    for (idx, tx) in block.transactions.iter().enumerate() {
        if let Some(event) = tx.analyze_mev() {
            mev_events.push((idx, event));
        }
    }

    // Header
    report.push_str("╔═══════════════════════════════════════════════════════════════╗\n");
    report.push_str("║                    MEV VALIDATION REPORT                      ║\n");
    report.push_str("╚═══════════════════════════════════════════════════════════════╝\n\n");

    report.push_str(&format!("Slot Number:         {}\n", block.slot));
    report.push_str(&format!("Block Hash:          {}\n", block.blockhash));
    if let Some(timestamp) = block.timestamp() {
        report.push_str(&format!("Timestamp:           {}\n", timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
    }
    report.push_str(&format!("Total Transactions:  {}\n", block.transactions.len()));
    report.push_str(&format!("MEV Transactions:    {}\n\n", mev_events.len()));

    if mev_events.is_empty() {
        report.push_str("No MEV transactions detected in this block.\n");
        return report;
    }

    report.push_str("─────────────────── MEV TRANSACTIONS ──────────────────────────\n\n");

    // List each MEV transaction
    for (idx, (tx_idx, event)) in mev_events.iter().enumerate() {
        let status = if event.success { "✓" } else { "✗" };
        let category = match event.category {
            crate::mev::MevCategory::Arbitrage => "ARBITRAGE",
            crate::mev::MevCategory::Liquidation => "LIQUIDATION",
            crate::mev::MevCategory::Mint => "MINT",
            crate::mev::MevCategory::Spam => "SPAM",
            crate::mev::MevCategory::Sandwich => "SANDWICH",
            crate::mev::MevCategory::JitLiquidity => "JIT_LIQUIDITY",
        };

        report.push_str(&format!("[{}] {} {} (tx #{})\n", idx + 1, status, category, tx_idx));
        report.push_str(&format!("Signature: {}\n", event.signature));

        // Programs
        if !event.programs_involved.is_empty() {
            report.push_str("Programs: ");
            let program_names: Vec<String> = event.programs_involved
                .iter()
                .map(|p| ProgramRegistry::program_name(p))
                .collect();
            report.push_str(&program_names.join(", "));
            report.push_str("\n");
        }

        // Token changes
        if !event.token_changes.is_empty() {
            report.push_str("Token Changes:\n");
            for token_change in &event.token_changes {
                let mint_short = truncate_mint(&token_change.mint);
                let sign = if token_change.ui_amount_change >= 0.0 { "+" } else { "" };
                report.push_str(&format!("  • {}: {}{:.6}\n",
                    mint_short,
                    sign,
                    token_change.ui_amount_change));
            }
        }

        // SOL change
        if event.sol_change_lamports != 0 {
            let sol_change_str = if event.sol_change_lamports >= 0 {
                format!("+{}", lamports_to_sol(event.sol_change_lamports as u64))
            } else {
                format!("-{}", lamports_to_sol((-event.sol_change_lamports) as u64))
            };
            report.push_str(&format!("SOL Change: {} SOL\n", sol_change_str));
        }

        report.push_str("\n");
    }

    report.push_str("═══════════════════════════════════════════════════════════════\n");

    report
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

/// JSON structure for MEV validation report
#[derive(Serialize)]
pub struct MevValidationJson {
    pub slot: u64,
    pub blockhash: String,
    pub timestamp: Option<String>,
    pub total_transactions: usize,
    pub mev_transactions: Vec<MevTransactionJson>,
    pub sandwich_attacks: Vec<MultiTxMevJson>,
    pub jit_attacks: Vec<MultiTxMevJson>,
}

/// JSON structure for individual MEV transaction
#[derive(Serialize)]
pub struct MevTransactionJson {
    pub tx_index: usize,
    pub signature: String,
    pub signer: Option<String>,
    pub category: String,
    pub success: bool,
    pub programs: Vec<String>,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<TokenChangeJson>,
    pub sol_change_lamports: i64,
}

/// JSON structure for token changes
#[derive(Serialize)]
pub struct TokenChangeJson {
    pub mint: String,
    pub amount: f64,
    pub decimals: u8,
}

/// JSON structure for multi-transaction MEV events (sandwich, JIT)
#[derive(Serialize)]
pub struct MultiTxMevJson {
    pub category: String,
    pub frontrun_signature: String,
    pub frontrun_tx_index: usize,
    pub victim_signature: String,
    pub victim_tx_index: usize,
    pub backrun_signature: String,
    pub backrun_tx_index: usize,
    pub profit_tokens: Vec<TokenChangeJson>,
    pub total_sol_profit_lamports: i64,
    pub programs: Vec<String>,
}

/// Format MEV validation report as JSON
pub fn format_mev_validation_json(block: &FetchedBlock) -> Result<String, serde_json::Error> {
    let mut mev_transactions = Vec::new();
    let mut tx_with_mev = Vec::new();

    // Collect all MEV events with their transaction indices
    for (idx, tx) in block.transactions.iter().enumerate() {
        if let Some(event) = tx.analyze_mev() {
            mev_transactions.push(MevTransactionJson {
                tx_index: idx,
                signature: event.signature.clone(),
                signer: event.signer.clone(),
                category: format!("{:?}", event.category).to_uppercase(),
                success: event.success,
                programs: event.programs_involved.iter()
                    .map(|p| ProgramRegistry::program_name(p))
                    .collect(),
                program_addresses: event.programs_involved.clone(),
                token_changes: event.token_changes.iter()
                    .map(|tc| TokenChangeJson {
                        mint: tc.mint.clone(),
                        amount: tc.ui_amount_change,
                        decimals: tc.decimals,
                    })
                    .collect(),
                sol_change_lamports: event.sol_change_lamports,
            });

            tx_with_mev.push((idx, tx, Some(event)));
        } else {
            tx_with_mev.push((idx, tx, None));
        }
    }

    // Detect multi-transaction MEV events (sandwich, JIT)
    let multi_tx_events = MevAnalyzer::detect_multi_tx_mev(&tx_with_mev);

    let mut sandwich_attacks = Vec::new();
    let mut jit_attacks = Vec::new();

    for event in multi_tx_events {
        let json_event = MultiTxMevJson {
            category: format!("{:?}", event.category).to_uppercase(),
            frontrun_signature: event.frontrun_signature,
            frontrun_tx_index: event.frontrun_tx_index,
            victim_signature: event.victim_signature,
            victim_tx_index: event.victim_tx_index,
            backrun_signature: event.backrun_signature,
            backrun_tx_index: event.backrun_tx_index,
            profit_tokens: event.profit_token_changes.iter()
                .map(|tc| TokenChangeJson {
                    mint: tc.mint.clone(),
                    amount: tc.ui_amount_change,
                    decimals: tc.decimals,
                })
                .collect(),
            total_sol_profit_lamports: event.total_sol_profit_lamports,
            programs: event.programs_involved.iter()
                .map(|p| ProgramRegistry::program_name(p))
                .collect(),
        };

        match event.category {
            crate::mev::MevCategory::Sandwich => sandwich_attacks.push(json_event),
            crate::mev::MevCategory::JitLiquidity => jit_attacks.push(json_event),
            _ => {}
        }
    }

    let timestamp = block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string());

    let report = MevValidationJson {
        slot: block.slot,
        blockhash: block.blockhash.clone(),
        timestamp,
        total_transactions: block.transactions.len(),
        mev_transactions,
        sandwich_attacks,
        jit_attacks,
    };

    serde_json::to_string_pretty(&report)
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
