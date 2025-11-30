mod fetcher;
mod mev;
mod price_oracle;
mod report;
mod types;

use fetcher::BlockFetcher;
use report::format_mev_validation_json;
use types::FetcherConfig;
use std::sync::Arc;
use tracing::{info, error};
use clap::Parser;

/// Solana MEV block analyzer
#[derive(Parser, Debug)]
#[command(name = "arges")]
#[command(about = "Analyze Solana blocks for MEV activity", long_about = None)]
struct Args {
    /// Specific slot number to analyze (defaults to current_slot - 10)
    #[arg(value_name = "SLOT")]
    slot: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("arges=info,warn")
        .init();

    let args = Args::parse();

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

    // determine which slot to fetch
    let target_slot = match args.slot {
        Some(slot) => {
            info!("fetching slot: {}", slot);
            slot
        }
        None => {
            let current_slot = fetcher.get_current_slot().await?;
            let slot = current_slot.saturating_sub(10);
            info!("fetching recent slot: {} (current: {})", slot, current_slot);
            slot
        }
    };

    // fetch block with MEV validation report
    match fetcher.fetch_block(target_slot).await {
        Ok(block) => {
            match format_mev_validation_json(&block).await {
                Ok(json) => println!("{}", json),
                Err(e) => error!("failed to serialize JSON: {:?}", e),
            }
        },
        Err(e) => error!("failed to fetch block {}: {:?}", target_slot, e),
    }

    Ok(())
}
