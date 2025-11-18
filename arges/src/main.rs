mod fetcher;
mod mev;
mod report;
mod stream;
mod types;

use fetcher::BlockFetcher;
use report::format_mev_validation_json;
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

    // fetch block with MEV validation report
    info!("fetching block for MEV validation\n");
    let recent_slot = current_slot.saturating_sub(10);

    match fetcher.fetch_block(recent_slot).await {
        Ok(block) => {
            match format_mev_validation_json(&block) {
                Ok(json) => println!("{}", json),
                Err(e) => error!("failed to serialize JSON: {:?}", e),
            }
        },
        Err(e) => error!("failed to fetch block: {:?}", e),
    }

    Ok(())
}
