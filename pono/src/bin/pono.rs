use pono::{BlockFetcher, FetcherConfig, MevDetector, MevEvent};
use std::sync::Arc;
use tracing::{info, error};
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("pono=info")
        .init();

    // cli
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: pono <slot>");
        eprintln!("example: pono 381165825");
        eprintln!("PONO_JSON=1 for JSON output");
        std::process::exit(1);
    }

    let slot: u64 = args[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid slot number"))?;

    info!("analyzing slot {}", slot);

    // setup fetcher
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 500,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));

    // fetch block
    info!("fetching block...");
    let block = match fetcher.fetch_block(slot).await {
        Ok(b) => b,
        Err(e) => {
            error!("failed to fetch block: {:?}", e);
            std::process::exit(1);
        }
    };

    info!("+ block fetched:");
    info!("   Slot: {}", block.slot);
    info!("   Blockhash: {}", &block.blockhash);
    info!("   Total transactions: {}", block.transactions.len());
    info!("   Successful transactions: {}", block.successful_tx_count());
    info!("   Total fees: {} lamports", block.total_fees());
    info!("   Total compute units: {}", block.total_compute_units());

    if let Some(timestamp) = block.timestamp() {
        info!("   Timestamp: {}", timestamp.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    // Detect MEV
    info!("\n🔎 Detecting MEV...");
    let mut detector = MevDetector::new();
    let mev_events = detector.detect_mev(slot, &block.transactions).await;

    // Separate events by type
    let mut arbitrages = Vec::new();
    let mut sandwiches = Vec::new();

    for event in &mev_events {
        match event {
            MevEvent::Arbitrage(arb) => arbitrages.push(arb),
            MevEvent::Sandwich(sand) => sandwiches.push(sand),
        }
    }

    info!("\n📊 MEV Summary:");
    info!("   Total MEV events: {}", mev_events.len());
    info!("   Arbitrage: {}", arbitrages.len());
    info!("   Sandwich attacks: {}", sandwiches.len());

    // Calculate total net profit
    let total_net_profit: f64 = arbitrages.iter()
        .map(|arb| arb.profitability.net_profit_usd)
        .sum();

    info!("   Total net profit: ${:.2}", total_net_profit);

    // Check if JSON output is requested
    let json_output = std::env::var("PONO_JSON").is_ok();

    if json_output {
        // Output as JSON matching verification format
        let output = json!({
            "slot": block.slot,
            "blockhash": block.blockhash,
            "timestamp": block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
            "total_transactions": block.transactions.len(),
            "mev_transactions": arbitrages,
            "sandwich_attacks": sandwiches,
            "total_net_profit_usd": total_net_profit,
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        // Human-readable output
        if !arbitrages.is_empty() {
            println!("\n{}", "=".repeat(80));
            println!("🔄 ARBITRAGE EVENTS ({} found)", arbitrages.len());
            println!("{}", "=".repeat(80));

            for (i, arb) in arbitrages.iter().enumerate() {
                println!("\n{}. Arbitrage #{}", i + 1, i + 1);
                println!("   Signature: {}", arb.signature);
                println!("   Signer: {}", arb.attacker_signer);
                println!("   Swaps: {}", arb.swap_count);
                println!("   Compute units: {}", arb.compute_units_consumed);
                println!("   Fee: {} lamports ({:.6} SOL)", arb.fee, arb.fee as f64 / 1e9);

                if let Some(priority_fee) = arb.priority_fee {
                    println!("   Priority fee: {} lamports", priority_fee);
                }

                if !arb.program_addresses.is_empty() {
                    println!("   Programs:");
                    for (j, program) in arb.program_addresses.iter().take(5).enumerate() {
                        println!("     {}. {}", j + 1, program);
                    }
                    if arb.program_addresses.len() > 5 {
                        println!("     ... and {} more", arb.program_addresses.len() - 5);
                    }
                }

                if !arb.token_changes.is_empty() {
                    println!("   Token Changes:");
                    for change in &arb.token_changes {
                        let token_name = change.token_name.as_deref().unwrap_or("Unknown");
                        println!("     • {:.6} {} ({})", change.amount, token_name, change.token_address);
                    }
                }

                if !arb.swaps.is_empty() {
                    println!("   Swaps:");
                    for (j, swap) in arb.swaps.iter().enumerate() {
                        let from_name = swap.from_token_name.as_deref().unwrap_or("Unknown");
                        let to_name = swap.to_token_name.as_deref().unwrap_or("Unknown");
                        println!("     {}. {:.6} {} → {:.6} {} via {}",
                            j + 1,
                            swap.from_amount,
                            from_name,
                            swap.to_amount,
                            to_name,
                            swap.dex_name
                        );
                    }
                }

                println!("   Profitability:");
                println!("     Profit (USD): ${:.6}", arb.profitability.profit_usd);
                println!("     Fees (USD): ${:.6}", arb.profitability.fees_usd);
                println!("     Net Profit (USD): ${:.6}", arb.profitability.net_profit_usd);
            }
        }

        // Output sandwich events
        if !sandwiches.is_empty() {
            println!("\n{}", "=".repeat(80));
            println!("🥪 SANDWICH ATTACK EVENTS ({} found)", sandwiches.len());
            println!("{}", "=".repeat(80));

            for (i, sand) in sandwiches.iter().enumerate() {
                println!("\n{}. Sandwich Attack #{}", i + 1, i + 1);
                println!("   Attacker: {}", sand.attacker);
                println!("   Victim signature: {}", sand.victim_signature);
                println!("   Total compute units: {}", sand.total_compute_units);
                println!("   Total fees: {} lamports ({:.6} SOL)", sand.total_fees, sand.total_fees as f64 / 1e9);

                println!("\n   Front-run:");
                println!("     Signature: {}", sand.front_run.signature);
                println!("     Index: {}", sand.front_run.index);
                println!("     Compute units: {}", sand.front_run.compute_units);
                println!("     Fee: {} lamports", sand.front_run.fee);

                println!("\n   Victim:");
                println!("     Signature: {}", sand.victim.signature);
                println!("     Index: {}", sand.victim.index);
                println!("     Signer: {}", sand.victim.signer);
                println!("     Compute units: {}", sand.victim.compute_units);
                println!("     Fee: {} lamports", sand.victim.fee);

                println!("\n   Back-run:");
                println!("     Signature: {}", sand.back_run.signature);
                println!("     Index: {}", sand.back_run.index);
                println!("     Compute units: {}", sand.back_run.compute_units);
                println!("     Fee: {} lamports", sand.back_run.fee);
            }
        }

        println!("\n{}", "=".repeat(80));
        println!("✅ Analysis complete");
        println!("   Total net profit: ${:.2}", total_net_profit);
        println!("{}", "=".repeat(80));
    }

    Ok(())
}
