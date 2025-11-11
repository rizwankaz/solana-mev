mod fetcher;
mod stream;
mod types;

use fetcher::BlockFetcher;
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
    
    Ok(())
}