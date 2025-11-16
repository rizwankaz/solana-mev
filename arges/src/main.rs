// Import from the library crate
use arges::{
    BlockFetcher, CexDexDetector, CexOracle, FetcherConfig, FetcherError,
    MevDetector, MetadataCache, PriceOracle, ProfitCalculator,
};
use std::sync::Arc;
use std::fs::File;
use std::io::Write;
use tracing::{info, error, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Create file appender for output
    let file_appender = tracing_appender::rolling::never(".", "mev_analysis_output.txt");

    // Configure tracing to write to file
    tracing_subscriber::fmt()
        .with_env_filter("arges=info")  // Changed to info to reduce verbosity
        .with_writer(file_appender)
        .with_ansi(false)
        .init();

    println!("🚀 MEV Analysis Over 24 Hours - Starting...");
    info!("Starting 24-hour MEV analysis");

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
    // Solana has ~2.5 slots/second (400ms per slot)
    // 24 hours = 24 * 60 * 60 / 0.4 = 216,000 slots
    const SLOTS_PER_24_HOURS: u64 = 216_000;

    // For testing/practicality, you can sample every Nth slot
    // Set SAMPLE_RATE to 1 for all slots, 10 for every 10th slot, etc.
    let sample_rate: u64 = std::env::var("SAMPLE_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100); // Default: sample every 100th slot

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

    // Initialize CEX-DEX detector
    let cex_oracle = Arc::new(CexOracle::new());
    let cex_dex_detector = Arc::new(CexDexDetector::new(Arc::clone(&cex_oracle)));

    // Warmup caches
    info!("Warming up pricing caches");
    if let Err(e) = profit_calculator.warmup().await {
        error!("Failed to warmup pricing caches: {}", e);
    }

    let mev_detector = MevDetector::new()
        .with_profit_calculator(Arc::clone(&profit_calculator))
        .with_cex_dex_detector(Arc::clone(&cex_dex_detector));

    // Create CSV file for results
    let mut csv_file = File::create("mev_per_slot_24h.csv")?;
    writeln!(csv_file, "slot,timestamp,mev_events,total_profit_sol,total_profit_lamports,arbitrage_count,sandwich_count,swap_count")?;

    println!("📝 Output will be written to: mev_per_slot_24h.csv");

    // Track overall statistics
    let mut total_slots_analyzed = 0;
    let mut total_slots_with_mev = 0;
    let mut total_mev_profit = 0i64;
    let mut total_mev_events = 0;

    // Progress tracking
    let total_slots_to_analyze = SLOTS_PER_24_HOURS / sample_rate;
    let mut progress_counter = 0;
    let progress_interval = (total_slots_to_analyze / 20).max(1); // Update every 5%

    println!("\n🔍 Starting analysis...\n");

    // Analyze slots
    for i in (0..SLOTS_PER_24_HOURS).step_by(sample_rate as usize) {
        let slot = start_slot + i;
        progress_counter += 1;

        // Progress update
        if progress_counter % progress_interval == 0 {
            let progress_pct = (progress_counter * 100) / total_slots_to_analyze;
            println!("⏳ Progress: {}% ({}/{} slots)",
                     progress_pct, progress_counter, total_slots_to_analyze);
        }

        match fetcher.fetch_block(slot).await {
            Ok(block) => {
                total_slots_analyzed += 1;

                // Analyze block for MEV
                match mev_detector.detect_block_with_pricing(&block).await {
                    Ok(analysis) => {
                        let mev_events = analysis.events.len();
                        let total_profit = analysis.total_profit();
                        let timestamp = block.timestamp()
                            .map(|t| t.timestamp())
                            .unwrap_or(0);

                        // Count event types
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
                        }

                        // Write to CSV
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
                        // Write zeros for failed analysis
                        writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
                    }
                }
            }
            Err(FetcherError::BlockNotAvailable { .. }) => {
                // Skipped slot (no block produced)
                writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
            }
            Err(e) => {
                warn!("Failed to fetch slot {}: {:?}", slot, e);
                writeln!(csv_file, "{},0,0,0.0,0,0,0,0", slot)?;
            }
        }
    }

    csv_file.flush()?;

    // Print summary
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 24-HOUR MEV ANALYSIS SUMMARY");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
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
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n✅ Analysis complete!");
    println!("📁 Results saved to: mev_per_slot_24h.csv");
    println!("📄 Detailed logs saved to: mev_analysis_output.txt");

    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("📊 MEV SUMMARY");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Total events: {}", total_mev_events);
    info!("  Total profit: {:.4} SOL ({} lamports)", total_mev_profit as f64 / 1e9, total_mev_profit);
    info!("  Slots analyzed: {}", total_slots_analyzed);
    info!("  Slots with MEV: {}", total_slots_with_mev);
    info!("  Average per slot: {:.6} SOL", (total_mev_profit as f64 / 1e9) / total_slots_analyzed.max(1) as f64);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    Ok(())
}
