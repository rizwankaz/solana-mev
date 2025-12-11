use clap::Parser;
use pono::{BlockFetcher, FetcherConfig, MevDetector};
use serde_json::json;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "pono")]
#[command(about = "Solana MEV detection tool", long_about = None)]
#[command(version)]
struct Cli {
    /// Slot number to analyze for MEV
    slot: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber with env filter support (RUST_LOG)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"))
        )
        .init();

    let cli = Cli::parse();
    let slot = cli.slot;

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

    // Detect MEV with historical prices from Pyth on-chain
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());
    let mut detector = MevDetector::new(slot, timestamp, rpc_url);
    let mev_events = detector.detect_mev(slot, &block.transactions).await;

    // Separate events by type and calculate totals
    let mut arbitrages = Vec::new();
    let mut sandwiches = Vec::new();
    let mut total_net_profit = 0.0;
    let mut mev_compute_units = 0u64;

    for event in &mev_events {
        match event {
            pono::MevEvent::Arbitrage(arb) => {
                total_net_profit += arb.profitability.net_profit_usd;
                mev_compute_units += arb.compute_units_consumed;
                arbitrages.push(json!({
                    "signature": arb.signature,
                    "signer": arb.signer,
                    "success": arb.success,
                    "compute_units_consumed": arb.compute_units_consumed,
                    "fee": arb.fee,
                    "priority_fee": arb.priority_fee,
                    "jito_tip": arb.jito_tip,
                    "swaps": arb.swaps,
                    "program_addresses": arb.program_addresses,
                    "token_changes": arb.token_changes,
                    "profitability": {
                        "profit_usd": arb.profitability.profit_usd,
                        "fees_usd": arb.profitability.fees_usd,
                        "net_profit_usd": arb.profitability.net_profit_usd,
                        "unsupported_profit_tokens": arb.profitability.unsupported_profit_tokens,
                    }
                }));
            }
            pono::MevEvent::Sandwich(sand) => {
                total_net_profit += sand.profitability.net_profit_usd;
                mev_compute_units += sand.total_compute_units;
                sandwiches.push(json!({
                    "slot": sand.slot,
                    "signer": sand.signer,
                    "victim_signature": sand.victim_signature,
                    "total_compute_units": sand.total_compute_units,
                    "total_fees": sand.total_fees,
                    "total_jito_tips": sand.total_jito_tips,
                    "swaps": sand.swaps,
                    "program_addresses": sand.program_addresses,
                    "token_changes": sand.token_changes,
                    "profitability": {
                        "profit_usd": sand.profitability.profit_usd,
                        "fees_usd": sand.profitability.fees_usd,
                        "net_profit_usd": sand.profitability.net_profit_usd,
                        "unsupported_profit_tokens": sand.profitability.unsupported_profit_tokens,
                    },
                    "front_run": {
                        "signature": sand.front_run.signature,
                        "index": sand.front_run.index,
                        "signer": sand.front_run.signer,
                        "compute_units": sand.front_run.compute_units,
                        "fee": sand.front_run.fee,
                    },
                    "victim": {
                        "signature": sand.victim.signature,
                        "index": sand.victim.index,
                        "signer": sand.victim.signer,
                        "compute_units": sand.victim.compute_units,
                        "fee": sand.victim.fee,
                    },
                    "back_run": {
                        "signature": sand.back_run.signature,
                        "index": sand.back_run.index,
                        "signer": sand.back_run.signer,
                        "compute_units": sand.back_run.compute_units,
                        "fee": sand.back_run.fee,
                    }
                }));
            }
        }
    }

    // Count non-vote transactions
    let nonvote_transactions = block.transactions.iter()
        .filter(|tx| !tx.is_vote())
        .count();

    // Output JSON
    let output = json!({
        "slot": block.slot,
        "blockhash": block.blockhash,
        "timestamp": block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        "total_transactions": block.transactions.len(),
        "successful_transactions": block.successful_tx_count(),
        "nonvote_transactions": nonvote_transactions,
        "total_compute_units": block.total_compute_units(),
        "mev_transaction_count": mev_events.len(),
        "mev_compute_units": mev_compute_units,
        "total_net_profit_usd": total_net_profit,
        "mev": {
            "arbitrage": arbitrages,
            "sandwich": sandwiches,
        }
    });

    println!("{}", serde_json::to_string_pretty(&output)?);

    Ok(())
}
