// Import from the library crate
use arges::{
    BlockFetcher, CexDexDetector, CexOracle, FetcherConfig, MevDetector,
    MevMetadata, MevType, MetadataCache, PriceOracle, ProfitCalculator,
};
use std::sync::Arc;
use std::fs::File;
use std::io::Write;
use tracing::{info, error};

/// Categorize MEV event for validation
#[derive(Debug, Clone)]
enum MevCategory {
    CexDexArbitrage,
    AtomicArbitrage,
    Sandwich,
    Jit,
    JitSandwich,
    Other,
}

impl MevCategory {
    fn as_str(&self) -> &str {
        match self {
            MevCategory::CexDexArbitrage => "CEX-DEX Arbitrage",
            MevCategory::AtomicArbitrage => "Atomic Arbitrage",
            MevCategory::Sandwich => "Sandwich",
            MevCategory::Jit => "JIT",
            MevCategory::JitSandwich => "JIT Sandwich",
            MevCategory::Other => "Other",
        }
    }
}

fn categorize_mev(mev_type: &MevType, metadata: &MevMetadata) -> MevCategory {
    match mev_type {
        MevType::CexDex => MevCategory::CexDexArbitrage,
        MevType::Arbitrage => {
            if let MevMetadata::Arbitrage(_) = metadata {
                MevCategory::AtomicArbitrage
            } else {
                MevCategory::AtomicArbitrage
            }
        }
        MevType::Sandwich => MevCategory::Sandwich,
        MevType::JitLiquidity => MevCategory::Jit,
        _ => MevCategory::Other,
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create file appender for output
    let file_appender = tracing_appender::rolling::never(".", "mev_analysis_output.txt");

    // Configure tracing to write to file
    tracing_subscriber::fmt()
        .with_env_filter("arges=info")
        .with_writer(file_appender)
        .with_ansi(false)
        .init();

    println!("🔍 MEV Validation Analysis - Single Slot Deep Dive");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Configuration
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));

    // Get current slot
    let current_slot = fetcher.get_current_slot().await?;

    // Determine which slot to analyze
    let target_slot = if let Ok(slot_str) = std::env::var("SLOT") {
        slot_str.parse::<u64>().unwrap_or(current_slot - 10)
    } else {
        current_slot - 10
    };

    println!("📍 Current slot: {}", current_slot);
    println!("🎯 Target slot for validation: {}", target_slot);
    println!("   (Set SLOT=<number> to analyze a specific slot)\n");

    // Initialize pricing components
    println!("🔧 Initializing pricing oracles...");
    let metadata_cache = Arc::new(MetadataCache::new(
        std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string())
    ));
    let price_oracle = Arc::new(PriceOracle::new());
    let profit_calculator = Arc::new(ProfitCalculator::new(
        Arc::clone(&metadata_cache),
        Arc::clone(&price_oracle),
    ));

    let cex_oracle = Arc::new(CexOracle::new());
    let cex_dex_detector = Arc::new(CexDexDetector::new(Arc::clone(&cex_oracle)));

    info!("Warming up pricing caches");
    if let Err(e) = profit_calculator.warmup().await {
        error!("Failed to warmup pricing caches: {}", e);
    }

    let mev_detector = MevDetector::new()
        .with_profit_calculator(Arc::clone(&profit_calculator))
        .with_cex_dex_detector(Arc::clone(&cex_dex_detector));

    // Fetch and analyze the target slot
    println!("\n⏳ Fetching block {}...", target_slot);
    let block = fetcher.fetch_block(target_slot).await?;

    println!("✅ Block fetched successfully");
    println!("   Blockhash: {}", block.blockhash);
    println!("   Transactions: {}", block.transactions.len());
    println!("   Successful txs: {}", block.successful_tx_count());

    if let Some(timestamp) = block.timestamp() {
        println!("   Timestamp: {} ({})", timestamp.format("%Y-%m-%d %H:%M:%S UTC"), timestamp.timestamp());
    }

    println!("\n🔍 Analyzing for MEV...");
    let analysis = mev_detector.detect_block_with_pricing(&block).await?;

    let mev_events = analysis.events.len();
    let total_profit = analysis.total_profit();

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 SLOT {} MEV SUMMARY", target_slot);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  MEV Events Found: {}", mev_events);
    println!("  Total MEV Profit: {:.6} SOL ({} lamports)",
             total_profit as f64 / 1e9, total_profit);
    println!("  Total Swaps: {}", analysis.swap_count);

    // Category breakdown
    let mut category_stats: std::collections::HashMap<String, (usize, i64)> = std::collections::HashMap::new();
    for event in &analysis.events {
        let category = categorize_mev(&event.mev_type, &event.metadata);
        let entry = category_stats.entry(category.as_str().to_string())
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += event.profit_lamports.unwrap_or(0);
    }

    if !category_stats.is_empty() {
        println!("\n  By Category:");
        let mut categories: Vec<_> = category_stats.iter().collect();
        categories.sort_by(|a, b| b.1.1.cmp(&a.1.1));

        for (category, (count, profit)) in categories {
            println!("    {:<20} {:>2} events  {:>10.6} SOL",
                     category, count, *profit as f64 / 1e9);
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    // Create detailed validation CSV
    let mut validation_file = File::create("mev_validation_details.csv")?;
    writeln!(
        validation_file,
        "event_num,category,mev_type,profit_sol,profit_lamports,confidence,tx_count,transactions,extractor,solscan_links,details"
    )?;

    if mev_events == 0 {
        println!("ℹ️  No MEV events detected in this slot.");
        println!("   Try a different slot with: SLOT=<slot_number> cargo run --release");
        return Ok(());
    }

    println!("📋 DETAILED MEV EVENTS:\n");

    // Write detailed information for each MEV event
    for (idx, event) in analysis.events.iter().enumerate() {
        let event_num = idx + 1;
        let category = categorize_mev(&event.mev_type, &event.metadata);
        let profit_sol = event.profit_lamports.unwrap_or(0) as f64 / 1e9;
        let profit_lamports = event.profit_lamports.unwrap_or(0);

        // Build transaction list and Solscan links
        let tx_list = event.transactions.join(";");
        let solscan_links: Vec<String> = event.transactions.iter()
            .map(|tx| format!("https://solscan.io/tx/{}", tx))
            .collect();
        let solscan_str = solscan_links.join(";");

        // Build details string based on MEV type
        let details = match &event.metadata {
            MevMetadata::Arbitrage(arb) => {
                format!(
                    "DEXs: {} | Path: {} | Hops: {} | Input: {} | Output: {}",
                    arb.dexs.join(","),
                    arb.token_path.join("→"),
                    arb.hop_count,
                    arb.input_amount,
                    arb.output_amount
                )
            }
            MevMetadata::Sandwich(sandwich) => {
                format!(
                    "Victim: https://solscan.io/tx/{} | Victim Loss: {:.6} SOL | Pool: {} | Token: {}",
                    sandwich.victim_tx,
                    sandwich.victim_loss.unwrap_or(0) as f64 / 1e9,
                    sandwich.pool,
                    sandwich.token
                )
            }
            MevMetadata::JitLiquidity(jit) => {
                format!(
                    "DEX: {} | Pool: {} | Target Swap: https://solscan.io/tx/{} | Liquidity: {} | Fees: {}",
                    jit.dex,
                    jit.pool,
                    jit.target_swap_tx,
                    jit.liquidity_added,
                    jit.fees_earned
                )
            }
            MevMetadata::CexDex(cex_dex) => {
                let price_diff = ((cex_dex.dex_price - cex_dex.cex_price) / cex_dex.cex_price) * 100.0;
                format!(
                    "Direction: {} | Token: {} | CEX Price: ${:.4} | DEX Price: ${:.4} | Diff: {:.2}%",
                    cex_dex.direction,
                    cex_dex.token,
                    cex_dex.cex_price,
                    cex_dex.dex_price,
                    price_diff.abs()
                )
            }
            MevMetadata::Liquidation(liq) => {
                format!(
                    "Protocol: {} | Liquidated: {} | Liquidator: {} | Bonus: {:.6} SOL",
                    liq.protocol,
                    liq.liquidated_account,
                    liq.liquidator,
                    liq.liquidation_bonus as f64 / 1e9
                )
            }
            _ => "N/A".to_string(),
        };

        let extractor_addr = event.extractor.as_deref().unwrap_or("Unknown");
        let extractor_link = format!("https://solscan.io/address/{}", extractor_addr);

        // Print to console
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("Event #{}: {} ({:?})", event_num, category.as_str(), event.mev_type);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  Profit: {:.9} SOL ({} lamports)", profit_sol, profit_lamports);
        println!("  Confidence: {:.1}%", event.confidence * 100.0);
        println!("  Transactions: {}", event.transactions.len());

        println!("\n  📍 Solscan Links:");
        for (i, link) in solscan_links.iter().enumerate() {
            println!("    TX {}: {}", i + 1, link);
        }

        println!("\n  👤 Extractor: {}", extractor_link);
        println!("\n  📋 Details:");
        for detail in details.split(" | ") {
            println!("    • {}", detail);
        }
        println!();

        // Write to CSV
        writeln!(
            validation_file,
            "{},{},{:?},{:.9},{},{:.2},{},\"{}\",\"{}\",\"{}\",\"{}\"",
            event_num,
            category.as_str(),
            event.mev_type,
            profit_sol,
            profit_lamports,
            event.confidence * 100.0,
            event.transactions.len(),
            tx_list,
            extractor_link,
            solscan_str,
            details.replace("\"", "'")
        )?;
    }

    validation_file.flush()?;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    println!("✅ Analysis complete!");
    println!("📁 Validation data saved to: mev_validation_details.csv");
    println!("📄 Detailed logs saved to: mev_analysis_output.txt");

    println!("\n💡 Validation Steps:");
    println!("   1. Click the Solscan transaction links above");
    println!("   2. Verify the swap amounts and token flows");
    println!("   3. Check the extractor's address and history");
    println!("   4. Compare on-chain data with reported profits");
    println!("   5. See VALIDATION_GUIDE.md for detailed instructions");

    println!("\n🔄 To analyze a different slot:");
    println!("   SLOT=<slot_number> cargo run --release");

    Ok(())
}
