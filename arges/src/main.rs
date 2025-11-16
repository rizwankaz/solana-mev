// Import from the library crate
use arges::{
    BlockFetcher, CexDexDetector, CexOracle, FetcherConfig, FetcherError,
    MevDetector, MevMetadata, MevType, MetadataCache, PriceOracle, ProfitCalculator,
};
use std::sync::Arc;
use std::fs::File;
use std::io::Write;
use tracing::{info, error, warn};

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
            // Check if it's a JIT sandwich (arbitrage combined with sandwich pattern)
            if let MevMetadata::Arbitrage(_) = metadata {
                // For now, treat all arbitrage as atomic arbitrage
                // Could enhance this to detect JIT sandwich patterns
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

    println!("🚀 MEV Analysis Over 24 Hours - Starting...");
    println!("📋 This run will generate detailed validation data\n");
    info!("Starting 24-hour MEV analysis with validation output");

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
    println!("📍 Current slot: {}", current_slot);
    info!("Current slot: {}", current_slot);

    // Calculate 24-hour slot range
    const SLOTS_PER_24_HOURS: u64 = 216_000;

    let sample_rate: u64 = std::env::var("SAMPLE_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let start_slot = current_slot.saturating_sub(SLOTS_PER_24_HOURS);

    println!("📊 Analyzing slots {} to {} (sample rate: 1/{})",
             start_slot, current_slot, sample_rate);
    println!("⏱️  This will analyze ~{} slots", SLOTS_PER_24_HOURS / sample_rate);
    info!("Analysis range: {} to {} (sample rate: 1/{})",
          start_slot, current_slot, sample_rate);

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

    // Create CSV file for aggregate results
    let mut csv_file = File::create("mev_per_slot_24h.csv")?;
    writeln!(csv_file, "slot,timestamp,mev_events,total_profit_sol,total_profit_lamports,arbitrage_count,sandwich_count,swap_count")?;

    // Create detailed validation file
    let mut validation_file = File::create("mev_validation_details.csv")?;
    writeln!(
        validation_file,
        "slot,timestamp,category,mev_type,profit_sol,profit_lamports,confidence,tx_count,transactions,extractor,solscan_links,details"
    )?;

    println!("📝 Output files:");
    println!("   - mev_per_slot_24h.csv (aggregate data)");
    println!("   - mev_validation_details.csv (validation data)");

    // Track statistics by category
    let mut total_slots_analyzed = 0;
    let mut total_slots_with_mev = 0;
    let mut total_mev_profit = 0i64;
    let mut total_mev_events = 0;

    let mut category_stats: std::collections::HashMap<String, (usize, i64)> = std::collections::HashMap::new();

    // Progress tracking
    let total_slots_to_analyze = SLOTS_PER_24_HOURS / sample_rate;
    let mut progress_counter = 0;
    let progress_interval = (total_slots_to_analyze / 20).max(1);

    println!("\n🔍 Starting analysis...\n");

    // Analyze slots
    for i in (0..SLOTS_PER_24_HOURS).step_by(sample_rate as usize) {
        let slot = start_slot + i;
        progress_counter += 1;

        if progress_counter % progress_interval == 0 {
            let progress_pct = (progress_counter * 100) / total_slots_to_analyze;
            println!("⏳ Progress: {}% ({}/{} slots)",
                     progress_pct, progress_counter, total_slots_to_analyze);
        }

        match fetcher.fetch_block(slot).await {
            Ok(block) => {
                total_slots_analyzed += 1;

                match mev_detector.detect_block_with_pricing(&block).await {
                    Ok(analysis) => {
                        let mev_events = analysis.events.len();
                        let total_profit = analysis.total_profit();
                        let timestamp = block.timestamp()
                            .map(|t| t.timestamp())
                            .unwrap_or(0);

                        let arbitrage_count = analysis.events.iter()
                            .filter(|e| matches!(e.mev_type, arges::MevType::Arbitrage))
                            .count();
                        let sandwich_count = analysis.events.iter()
                            .filter(|e| matches!(e.mev_type, arges::MevType::Sandwich))
                            .count();

                        if analysis.has_mev() {
                            total_slots_with_mev += 1;
                            total_mev_profit += total_profit;
                            total_mev_events += mev_events;

                            info!("Slot {}: {} MEV events, {:.6} SOL profit",
                                  slot, mev_events, total_profit as f64 / 1e9);

                            // Write detailed information for each MEV event
                            for event in &analysis.events {
                                let category = categorize_mev(&event.mev_type, &event.metadata);
                                let profit_sol = event.profit_lamports.unwrap_or(0) as f64 / 1e9;
                                let profit_lamports = event.profit_lamports.unwrap_or(0);

                                // Update category statistics
                                let entry = category_stats.entry(category.as_str().to_string())
                                    .or_insert((0, 0));
                                entry.0 += 1;
                                entry.1 += profit_lamports;

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

                                // Write to validation file
                                writeln!(
                                    validation_file,
                                    "{},{},{},{:?},{:.9},{},{:.2},{},\"{}\",\"{}\",\"{}\",\"{}\"",
                                    slot,
                                    timestamp,
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
                        }

                        // Write aggregate data to CSV
                        writeln!(
                            csv_file,
                            "{},{},{},{:.9},{},{},{},{}",
                            slot,
                            timestamp,
                            mev_events,
                            total_profit as f64 / 1e9,
                            total_profit,
                            arbitrage_count,
                            sandwich_count,
                            analysis.swap_count
                        )?;
                    }
                    Err(e) => {
                        warn!("Failed to analyze slot {}: {:?}", slot, e);
                        writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
                    }
                }
            }
            Err(FetcherError::BlockNotAvailable { .. }) => {
                writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
            }
            Err(e) => {
                warn!("Failed to fetch slot {}: {:?}", slot, e);
                writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
            }
        }
    }

    csv_file.flush()?;
    validation_file.flush()?;

    // Print summary
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 24-HOUR MEV ANALYSIS SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Slots analyzed: {}", total_slots_analyzed);
    println!("  Slots with MEV: {} ({:.2}%)",
             total_slots_with_mev,
             (total_slots_with_mev as f64 / total_slots_analyzed.max(1) as f64) * 100.0);
    println!("  Total MEV events: {}", total_mev_events);
    println!("  Total MEV profit: {:.4} SOL ({} lamports)",
             total_mev_profit as f64 / 1e9, total_mev_profit);
    println!("  Average per slot: {:.6} SOL",
             (total_mev_profit as f64 / 1e9) / total_slots_analyzed.max(1) as f64);
    println!("  Average per MEV slot: {:.6} SOL",
             (total_mev_profit as f64 / 1e9) / total_slots_with_mev.max(1) as f64);

    println!("\n📋 MEV BY CATEGORY:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut categories: Vec<_> = category_stats.iter().collect();
    categories.sort_by(|a, b| b.1.1.cmp(&a.1.1)); // Sort by profit descending

    for (category, (count, profit)) in categories {
        let pct_of_total = (*profit as f64 / total_mev_profit.max(1) as f64) * 100.0;
        println!("  {:<20} {:>6} events  {:>10.4} SOL  ({:.1}%)",
                 category, count, *profit as f64 / 1e9, pct_of_total);
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n✅ Analysis complete!");
    println!("📁 Results saved to:");
    println!("   - mev_per_slot_24h.csv (aggregate data for plotting)");
    println!("   - mev_validation_details.csv (detailed validation data)");
    println!("   - mev_analysis_output.txt (detailed logs)");
    println!("\n💡 To validate an MEV event:");
    println!("   1. Open mev_validation_details.csv");
    println!("   2. Find the event you want to verify");
    println!("   3. Click the Solscan links to view transactions on-chain");
    println!("   4. Compare the reported profit with on-chain data");

    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("📊 MEV SUMMARY");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Total events: {}", total_mev_events);
    info!("  Total profit: {:.4} SOL ({} lamports)", total_mev_profit as f64 / 1e9, total_mev_profit);
    info!("  Slots analyzed: {}", total_slots_analyzed);
    info!("  Slots with MEV: {}", total_slots_with_mev);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}
