/// Example: Analyze a specific Solana slot for MEV transactions
///
/// Usage:
///   cargo run --example analyze_slot -- <slot_number>
///   cargo run --example analyze_slot -- 381165825
///
/// Or with custom RPC:
///   SOLANA_RPC_URL=https://your-rpc.com cargo run --example analyze_slot -- 381165825
///
/// Output: JSON list of all detected MEV transactions

use arges::fetcher::BlockFetcher;
use arges::types::FetcherConfig;
use arges::mev::MevAnalyzer;
use tracing::{info, error};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("arges=info,warn")
        .init();

    // Parse slot from command line
    let args: Vec<String> = std::env::args().collect();
    let slot = if args.len() > 1 {
        args[1].parse::<u64>()
            .expect("Invalid slot number. Usage: cargo run --example analyze_slot -- <slot_number>")
    } else {
        381165825 // Default to the requested slot
    };

    info!("🔍 Analyzing slot {} for MEV transactions...\n", slot);

    // Configure RPC connection
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    info!("📡 RPC endpoint: {}", config.rpc_url);

    let fetcher = Arc::new(BlockFetcher::new(config));

    // Fetch the block
    info!("⏳ Fetching block data...");

    let block = match fetcher.fetch_block(slot).await {
        Ok(block) => {
            info!("✅ Block fetched successfully");
            info!("   Blockhash: {}", block.blockhash);
            info!("   Parent slot: {}", block.parent_slot);
            info!("   Total transactions: {}", block.transactions.len());
            info!("   Successful transactions: {}", block.successful_tx_count());
            info!("   Total fees: {} SOL", block.total_fees() as f64 / 1_000_000_000.0);

            if let Some(timestamp) = block.timestamp() {
                info!("   Block time: {}", timestamp.format("%Y-%m-%d %H:%M:%S UTC"));
            }

            block
        },
        Err(e) => {
            error!("❌ Failed to fetch block {}: {:?}", slot, e);
            error!("\nPossible reasons:");
            error!("  • Block may not exist (slot never produced)");
            error!("  • Slot may be too old (data pruned from RPC)");
            error!("  • RPC rate limiting or connection issues");
            error!("\nTry:");
            error!("  • Use a recent slot (within last ~500 slots)");
            error!("  • Use an archive RPC node for historical data");
            error!("  • Set SOLANA_RPC_URL to a premium RPC endpoint");
            std::process::exit(1);
        }
    };

    // Run MEV detection
    info!("\n🔬 Running MEV detection algorithms...");
    let mev_summary = MevAnalyzer::analyze_block(&block);

    // Display statistics
    info!("\n{}", MevAnalyzer::get_stats_summary(&mev_summary));

    // Output detailed MEV transactions
    if !mev_summary.mev_transactions.is_empty() {
        info!("\n💰 MEV Transactions Detected:\n");

        for (i, mev_tx) in mev_summary.mev_transactions.iter().enumerate() {
            match mev_tx {
                arges::mev::MevTransaction::AtomicArbitrage(arb) => {
                    info!("{}. 🔄 ATOMIC ARBITRAGE", i + 1);
                    info!("   Signature: {}", arb.signature);
                    info!("   Searcher: {}", arb.searcher);
                    info!("   Profit: {} SOL ({} lamports)",
                        arb.profit_lamports as f64 / 1_000_000_000.0,
                        arb.profit_lamports
                    );
                    info!("   Fee: {} lamports", arb.fee_lamports);
                    info!("   Compute units: {}", arb.compute_units);
                    info!("   Pools: {}", arb.pools.len());
                    info!("   Token route: {}", arb.token_route.join(" → "));
                    info!("   Swaps: {}", arb.swaps.len());
                    info!("");
                },
                arges::mev::MevTransaction::Sandwich(sw) => {
                    info!("{}. 🥪 SANDWICH ATTACK", i + 1);
                    info!("   Frontrun: {}", sw.frontrun.signature);
                    info!("   Backrun: {}", sw.backrun.signature);
                    info!("   Attacker: {}", sw.attacker);
                    info!("   Victims: {}", sw.victims.len());
                    for (j, victim) in sw.victims.iter().enumerate() {
                        info!("     Victim {}: {} (loss: {} lamports)",
                            j + 1, victim.signature, victim.loss_lamports);
                    }
                    info!("   Profit: {} SOL ({} lamports)",
                        sw.profit_lamports as f64 / 1_000_000_000.0,
                        sw.profit_lamports
                    );
                    info!("   Victim loss: {} SOL ({} lamports)",
                        sw.victim_loss_lamports as f64 / 1_000_000_000.0,
                        sw.victim_loss_lamports
                    );
                    info!("   Common pools: {}", sw.common_pools.len());
                    info!("");
                },
                arges::mev::MevTransaction::JitLiquidity(jit) => {
                    info!("{}. ⚡ JIT LIQUIDITY", i + 1);
                    info!("   Add liquidity: {}", jit.add_liquidity.signature);
                    info!("   Remove liquidity: {}", jit.remove_liquidity.signature);
                    info!("   Searcher: {}", jit.searcher);
                    info!("   Victim swap: {}", jit.victim_swap.signature);
                    info!("   Profit: {} SOL ({} lamports)",
                        jit.profit_lamports as f64 / 1_000_000_000.0,
                        jit.profit_lamports
                    );
                    info!("   Fees collected: {} lamports", jit.fees_collected_lamports);
                    info!("   Pool: {}", jit.pool);
                    info!("");
                },
                arges::mev::MevTransaction::Liquidation(liq) => {
                    info!("{}. 💧 LIQUIDATION", i + 1);
                    info!("   Signature: {}", liq.signature);
                    info!("   Liquidator: {}", liq.liquidator);
                    info!("   Liquidated user: {}", liq.liquidated_user);
                    info!("   Protocol: {}", liq.protocol);
                    info!("   Profit: {} SOL ({} lamports)",
                        liq.profit_lamports as f64 / 1_000_000_000.0,
                        liq.profit_lamports
                    );
                    info!("   Revenue: {} lamports", liq.revenue_lamports);
                    info!("   Cost: {} lamports", liq.cost_lamports);
                    info!("   Fee: {} lamports", liq.fee_lamports);
                    info!("   Debt repaid: {} tokens", liq.debt_repaid.len());
                    info!("   Collateral seized: {} tokens", liq.collateral_seized.len());
                    info!("");
                },
            }
        }
    } else {
        info!("\n✅ No MEV transactions detected in this block.");
        info!("   This could mean:");
        info!("   • The block had no MEV activity");
        info!("   • MEV was present but below detection thresholds");
        info!("   • The block contained only non-DeFi transactions");
    }

    // Generate and output JSON
    info!("\n📄 Generating JSON output...");
    match MevAnalyzer::to_json(&mev_summary) {
        Ok(json) => {
            // Pretty print the JSON
            println!("\n{}", "=".repeat(80));
            println!("JSON OUTPUT - MEV TRANSACTIONS FOR SLOT {}", slot);
            println!("{}", "=".repeat(80));
            println!("{}", json);
            println!("{}", "=".repeat(80));

            // Optionally save to file
            let filename = format!("mev_slot_{}.json", slot);
            if let Err(e) = std::fs::write(&filename, &json) {
                error!("⚠️  Failed to save JSON to {}: {:?}", filename, e);
            } else {
                info!("\n💾 JSON saved to: {}", filename);
            }
        },
        Err(e) => {
            error!("❌ Failed to serialize MEV summary to JSON: {:?}", e);
            std::process::exit(1);
        }
    }

    info!("\n✨ Analysis complete!");

    Ok(())
}
