use clap::{Parser, Subcommand};
use pono::{BlockFetcher, FetcherConfig, MevDetector};
use serde_json::json;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "pono")]
#[command(about = "Solana MEV detection tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run MEV detection on a specific slot
    Run {
        /// Slot number to analyze
        #[arg(short, long)]
        slot: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let slot = match cli.command {
        Commands::Run { slot } => slot,
    };

    // Setup fetcher
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 500,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));
    let block = fetcher.fetch_block(slot).await.map_err(|e| {
        anyhow::anyhow!("Failed to fetch block: {:?}", e)
    })?;

    // Get block timestamp
    let timestamp = block.timestamp().map(|t| t.timestamp()).unwrap_or(0);

    // Detect MEV with historical prices
    let mut detector = MevDetector::new(timestamp);
    let mev_events = detector.detect_mev(slot, &block.transactions).await;

    // Calculate total profit
    let total_net_profit: f64 = mev_events.iter()
        .filter_map(|e| match e {
            pono::MevEvent::Arbitrage(arb) => Some(arb.profitability.net_profit_usd),
            _ => None,
        })
        .sum();

    // Output JSON
    let output = json!({
        "slot": block.slot,
        "blockhash": block.blockhash,
        "timestamp": block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        "total_transactions": block.transactions.len(),
        "mev_transactions": mev_events.iter().filter_map(|e| match e {
            pono::MevEvent::Arbitrage(arb) => Some(arb),
            _ => None,
        }).collect::<Vec<_>>(),
        "sandwich_attacks": mev_events.iter().filter_map(|e| match e {
            pono::MevEvent::Sandwich(sand) => Some(sand),
            _ => None,
        }).collect::<Vec<_>>(),
        "total_net_profit_usd": total_net_profit,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}
