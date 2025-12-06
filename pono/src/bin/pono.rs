use pono::{BlockFetcher, FetcherConfig, MevDetector, MevEvent};
use std::sync::Arc;
use tracing::{info, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("pono=info")
        .init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: pono <slot>");
        eprintln!("Example: pono 381165825");
        std::process::exit(1);
    }

    let slot: u64 = args[1]
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid slot number"))?;

    info!("🔍 Analyzing slot {} for MEV", slot);

    // Setup fetcher
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));

    // Fetch block
    info!("📦 Fetching block...");
    let block = match fetcher.fetch_block(slot).await {
        Ok(b) => b,
        Err(e) => {
            error!("Failed to fetch block: {:?}", e);
            std::process::exit(1);
        }
    };

    info!("✅ Block fetched:");
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
    let detector = MevDetector::new();
    let mev_events = detector.detect_mev(slot, &block.transactions);

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

    // Output arbitrage events
    if !arbitrages.is_empty() {
        println!("\n{}", "=".repeat(80));
        println!("🔄 ARBITRAGE EVENTS ({} found)", arbitrages.len());
        println!("{}", "=".repeat(80));

        for (i, arb) in arbitrages.iter().enumerate() {
            println!("\n{}. Arbitrage #{}", i + 1, i + 1);
            println!("   Signature: {}", arb.signature);
            println!("   Signer: {}", arb.signer);
            println!("   Swaps: {}", arb.swap_count);
            println!("   Transfers: {}", arb.transfer_count);
            println!("   Compute units: {}", arb.compute_units);
            println!("   Fee: {} lamports ({:.6} SOL)", arb.fee, arb.fee as f64 / 1e9);

            if !arb.programs.is_empty() {
                println!("   Programs:");
                for (j, program) in arb.programs.iter().take(5).enumerate() {
                    println!("     {}. {}", j + 1, program);
                }
                if arb.programs.len() > 5 {
                    println!("     ... and {} more", arb.programs.len() - 5);
                }
            }

            if !arb.profit_tokens.is_empty() {
                println!("   Profits:");
                for profit in &arb.profit_tokens {
                    let amount = profit.delta as f64 / 10_f64.powi(profit.decimals as i32);
                    println!("     • {} tokens", amount);
                    println!("       Mint: {}", profit.mint);
                    println!("       Raw delta: {}", profit.delta);
                }
            }
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

    // Output as JSON if requested
    if std::env::var("PONO_JSON").is_ok() {
        println!("\n{}", "=".repeat(80));
        println!("JSON OUTPUT");
        println!("{}", "=".repeat(80));
        println!("{}", serde_json::to_string_pretty(&mev_events)?);
    }

    println!("\n{}", "=".repeat(80));
    println!("✅ Analysis complete");
    println!("{}", "=".repeat(80));

    Ok(())
}
