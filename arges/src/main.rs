// Import from the library crate
use arges::{BlockFetcher, BlockStream, FetcherConfig, FetcherError, MevDetector, MevMetadata};
use std::sync::Arc;
use tracing::{info, error};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("arges=info,warn")
        .init();
    
    info!("block fetcher go brrr");
    
    // fetcher config
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };
    
    let fetcher = Arc::new(BlockFetcher::new(config));
    
    // current slot
    let current_slot = fetcher.get_current_slot().await?;
    info!("current slot: {}\n", current_slot);
    
    // fetch block
    info!("fetch block");
    let recent_slot = current_slot.saturating_sub(10);
    
    match fetcher.fetch_block(recent_slot).await {
        Ok(block) => {
            info!("+ block {}", block.slot);
            info!("  hash {}", &block.blockhash);
            info!("  parent slot: {}", block.parent_slot);
            info!("  transactions: {}", block.transactions.len());
            info!("  successful: {}", block.successful_tx_count());
            info!("  fees: {} lamports", block.total_fees());
            info!("  compute units: {}", block.total_compute_units());
            
            if let Some(timestamp) = block.timestamp() {
                info!("  time: {}", timestamp.format("%Y-%m-%d %H:%M:%S"));
            }
            
            // txs
            if !block.transactions.is_empty() {
                info!("  first 3 transactions:");
                for tx in block.transactions.iter().take(3) {
                    let status = if tx.is_success() { "+" } else { "-" };
                    info!("    {} {}", status, &tx.signature);
                }
            }
        },
        Err(e) => error!("failed to fetch block: {:?}", e),
    }
    
    info!("\n");
    
    // fetch range
    info!("fetch range");
    let start_slot = recent_slot.saturating_sub(10);
    let end_slot = start_slot + 5;
    
    info!("fetching slots {} to {}", start_slot, end_slot);
    
    let results = fetcher.fetch_range(start_slot, end_slot).await;
    
    let mut success_count = 0;
    let mut skip_count = 0;
    let mut error_count = 0;
    
    for (slot, result) in results {
        match result {
            Ok(block) => {
                success_count += 1;
                info!(
                    "  + slot {}: {} txs, {} successful",
                    slot,
                    block.transactions.len(),
                    block.successful_tx_count()
                );
            },
            Err(FetcherError::BlockNotAvailable { .. }) => {
                skip_count += 1;
                info!("  - Slot {}: skipped (no block produced)", slot);
            },
            Err(e) => {
                error_count += 1;
                error!("  - Slot {}: error {:?}", slot, e);
            }
        }
    }
    
    info!("success: {}", success_count);
    info!("skips: {}", skip_count);
    info!("errors: {}", error_count);
    
    info!("\n");
    
    // stream
    info!("stream recents");
    info!("streaming blocks starting from slot {}", recent_slot);
    
    let mut stream = BlockStream::new(Arc::clone(&fetcher), recent_slot);
    
    let mut count = 0;
    while let Some((slot, result)) = stream.next().await {
        match result {
            Ok(block) => {
                info!(
                    "  | slot {}: {} transactions, {} lamports in fees",
                    slot,
                    block.transactions.len(),
                    block.total_fees()
                );
            },
            Err(e) => {
                error!("  - slot {}: error {:?}", slot, e);
            }
        }
        
        count += 1;
        if count >= 5 {
            break;
        }
    }
    
    info!("examples complete");

    info!("\n");

    // MEV detection example
    info!("mev detection");
    info!("analyzing recent blocks for mev");

    let mev_detector = MevDetector::new();

    // Fetch a few recent blocks
    let mev_start_slot = current_slot.saturating_sub(20);
    let mev_end_slot = mev_start_slot + 10;

    let results = fetcher.fetch_range(mev_start_slot, mev_end_slot).await;

    let mut blocks = Vec::new();
    for (_slot, result) in results {
        if let Ok(block) = result {
            blocks.push(block);
        }
    }

    info!("analyzing {} blocks for mev", blocks.len());

    let mut total_mev_events = 0;
    let mut total_profit = 0i64;

    for block in &blocks {
        match mev_detector.detect_block(block) {
            Ok(analysis) => {
                if analysis.has_mev() {
                    info!(
                        "  ✓ Slot {}: {} MEV events, {:.4} SOL profit, {} swaps",
                        block.slot,
                        analysis.events.len(),
                        analysis.total_profit() as f64 / 1e9,
                        analysis.swap_count
                    );

                    total_mev_events += analysis.events.len();
                    total_profit += analysis.total_profit();

                    // Show details of each MEV event with transaction signatures
                    for event in &analysis.events {
                        let profit_sol = event.profit_lamports.unwrap_or(0) as f64 / 1e9;

                        // Get transaction signature and extractor
                        let tx_sig = if !event.transactions.is_empty() {
                            &event.transactions[0]
                        } else {
                            "N/A"
                        };

                        let extractor = event.extractor.as_deref().unwrap_or("Unknown");

                        info!(
                            "    → {}: {:.6} SOL (confidence: {:.0}%)",
                            event.mev_type.name(),
                            profit_sol,
                            event.confidence * 100.0
                        );
                        info!(
                            "      TX: https://solscan.io/tx/{}",
                            tx_sig
                        );
                        info!(
                            "      Extractor: https://solscan.io/address/{}",
                            extractor
                        );

                        // Show additional details based on MEV type
                        match &event.metadata {
                            MevMetadata::Arbitrage(arb) => {
                                info!(
                                    "      Path: {} ({})",
                                    arb.token_path.join(" → "),
                                    arb.dexs.join(", ")
                                );
                            },
                            MevMetadata::Sandwich(sandwich) => {
                                info!("      Victim TX: https://solscan.io/tx/{}", sandwich.victim_tx);
                                if let Some(loss) = sandwich.victim_loss {
                                    info!("      Victim loss: {:.6} SOL", loss as f64 / 1e9);
                                }
                            },
                            MevMetadata::Liquidation(liq) => {
                                info!("      Protocol: {}", liq.protocol);
                                info!("      Bonus: {:.6} SOL", liq.liquidation_bonus as f64 / 1e9);
                            },
                            _ => {}
                        }
                    }
                }
            },
            Err(e) => {
                error!("  - slot {}: mev detection error: {:?}", block.slot, e);
            }
        }
    }

    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("📊 MEV SUMMARY");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Total events: {}", total_mev_events);
    info!("  Total profit: {:.4} SOL ({} lamports)", total_profit as f64 / 1e9, total_profit);
    info!("  Blocks analyzed: {}", blocks.len());
    info!("  Blocks with MEV: {}", blocks.iter().filter(|b| {
        mev_detector.detect_block(b).map(|a| a.has_mev()).unwrap_or(false)
    }).count());
    info!("  Average per block: {:.4} SOL", (total_profit as f64 / 1e9) / blocks.len() as f64);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    info!("\n✅ all examples complete");

    Ok(())
}