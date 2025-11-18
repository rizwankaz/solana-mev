mod fetcher;
mod mev;
mod report;
mod stream;
mod types;

use fetcher::BlockFetcher;
use report::{format_block_report, format_compact_summary};
use stream::BlockStream;
use types::FetcherConfig;
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
    
    // fetch block with MEV analysis
    info!("fetching block with MEV analysis\n");
    let recent_slot = current_slot.saturating_sub(10);

    match fetcher.fetch_block(recent_slot).await {
        Ok(block) => {
            let report = format_block_report(&block);
            println!("{}", report);
        },
        Err(e) => error!("failed to fetch block: {:?}", e),
    }
    
    info!("\n");
    
    // fetch range with MEV tracking
    info!("fetching block range with MEV analysis");
    let start_slot = recent_slot.saturating_sub(10);
    let end_slot = start_slot + 5;

    info!("fetching slots {} to {}\n", start_slot, end_slot);

    let results = fetcher.fetch_range(start_slot, end_slot).await;

    let mut success_count = 0;
    let mut skip_count = 0;
    let mut error_count = 0;
    let mut total_mev_events = 0;
    let mut total_spam = 0;

    for (slot, result) in results {
        match result {
            Ok(block) => {
                success_count += 1;
                let mev = block.analyze_mev();
                total_mev_events += mev.total_mev_count();
                total_spam += mev.spam_count;

                info!(
                    "  ✓ Slot {}: {} txs | MEV: {} arb, {} liq, {} mint | Spam: {}",
                    slot,
                    block.transactions.len(),
                    mev.arbitrage_count,
                    mev.liquidation_count,
                    mev.mint_count,
                    mev.spam_count
                );
            },
            Err(types::FetcherError::BlockNotAvailable { .. }) => {
                skip_count += 1;
                info!("  - Slot {}: skipped (no block produced)", slot);
            },
            Err(e) => {
                error_count += 1;
                error!("  - Slot {}: error {:?}", slot, e);
            }
        }
    }

    info!("\nRange Summary:");
    info!("  Blocks fetched: {}", success_count);
    info!("  Skipped: {}", skip_count);
    info!("  Errors: {}", error_count);
    info!("  Total MEV events: {}", total_mev_events);
    info!("  Total spam: {}", total_spam);
    
    info!("\n");
    
    // stream with MEV tracking
    info!("streaming blocks with MEV analysis");
    info!("streaming blocks starting from slot {}\n", recent_slot);

    let mut stream = BlockStream::new(Arc::clone(&fetcher), recent_slot);

    let mut count = 0;
    while let Some((slot, result)) = stream.next().await {
        match result {
            Ok(block) => {
                let summary = format_compact_summary(slot, &block);
                info!("  {}", summary);
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

    info!("\nAnalysis complete!");
    
    Ok(())
}