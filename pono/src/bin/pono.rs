use clap::{Parser, Subcommand};
use pono::{BlockFetcher, BlockStream, FetcherConfig, MevInspector};
use serde_json::json;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "pono")]
#[command(about = "Solana MEV detection tool", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// continuous stream
    Stream,
    /// specific slot
    Run {
        slot_spec: Option<String>,
        #[command(subcommand)]
        mode: Option<RunMode>,
    },
}

#[derive(Subcommand)]
enum RunMode {
    /// summary mode
    Slot { slot_spec: String },
}

fn parse_slot_spec(spec: &str) -> anyhow::Result<(u64, u64)> {
    if let Some((start, end)) = spec.split_once('-') {
        let start = start.parse::<u64>()?;
        let end = end.parse::<u64>()?;
        if start > end {
            anyhow::bail!("Start slot must be <= end slot");
        }
        Ok((start, end))
    } else {
        let slot = spec.parse::<u64>()?;
        Ok((slot, slot))
    }
}

async fn analyze_slot_mev(
    slot: u64,
    fetcher: &Arc<BlockFetcher>,
    rpc_url: &str,
) -> anyhow::Result<serde_json::Value> {
    let block = fetcher
        .fetch_block(slot)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch block {}: {:?}", slot, e))?;

    let timestamp = block.timestamp().map(|t| t.timestamp()).unwrap_or(0);
    let mut detector = MevInspector::new(slot, timestamp, rpc_url.to_string());
    let mev_events = detector.detect_mev(slot, &block.transactions).await;
    let mut arbitrages = Vec::new();
    let mut sandwiches = Vec::new();
    let mut total_profit = 0.0;
    let mut mev_compute_units = 0u64;

    for event in &mev_events {
        match event {
            pono::MevEvent::Arbitrage(arb) => {
                total_profit += arb.profitability.profit_usd;
                mev_compute_units += arb.compute_units_consumed;
                arbitrages.push(json!({
                    "signature": arb.signature,
                    "signer": arb.signer,
                    "compute_units_consumed": arb.compute_units_consumed,
                    "fee": arb.fee,
                    "priority_fee": arb.priority_fee,
                    "jito_tip": arb.jito_tip,
                    "swaps": arb.swaps,
                    "program_addresses": arb.program_addresses,
                    "token_changes": arb.token_changes,
                    "profitability": {
                        "revenue_usd": arb.profitability.revenue_usd,
                        "fees_usd": arb.profitability.fees_usd,
                        "profit_usd": arb.profitability.profit_usd,
                        "unsupported_profit_tokens": arb.profitability.unsupported_profit_tokens,
                    }
                }));
            }
            pono::MevEvent::Sandwich(sand) => {
                total_profit += sand.profitability.profit_usd;
                mev_compute_units += sand.total_compute_units;
                sandwiches.push(json!({
                    "slot": sand.slot,
                    "signer": sand.signer,
                    "sandwiched_token": sand.sandwiched_token,
                    "total_compute_units": sand.total_compute_units,
                    "total_fees": sand.total_fees,
                    "total_jito_tips": sand.total_jito_tips,
                    "front_run": {
                        "signature": sand.front_run.signature,
                        "index": sand.front_run.index,
                        "signer": sand.front_run.signer,
                        "compute_units": sand.front_run.compute_units,
                        "fee": sand.front_run.fee,
                        "swap": sand.front_run.swap,
                    },
                    "back_run": {
                        "signature": sand.back_run.signature,
                        "index": sand.back_run.index,
                        "signer": sand.back_run.signer,
                        "compute_units": sand.back_run.compute_units,
                        "fee": sand.back_run.fee,
                        "swap": sand.back_run.swap,
                    },
                    "program_addresses": sand.program_addresses,
                    "token_changes": sand.token_changes,
                    "profitability": {
                        "revenue_usd": sand.profitability.revenue_usd,
                        "fees_usd": sand.profitability.fees_usd,
                        "profit_usd": sand.profitability.profit_usd,
                        "unsupported_profit_tokens": sand.profitability.unsupported_profit_tokens,
                    },
                }));
            }
        }
    }

    let nonvote_transactions = block.transactions.iter().filter(|tx| !tx.is_vote()).count();

    Ok(json!({
        "slot": block.slot,
        "blockhash": block.blockhash,
        "timestamp": block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        "total_transactions": block.transactions.len(),
        "successful_transactions": block.successful_tx_count(),
        "nonvote_transactions": nonvote_transactions,
        "total_compute_units": block.total_compute_units(),
        "mev_transaction_count": mev_events.len(),
        "mev_compute_units": mev_compute_units,
        "total_profit_usd": total_profit,
        "mev": {
            "arbitrage": arbitrages,
            "sandwich": sandwiches,
        }
    }))
}

async fn analyze_slot_summary(
    slot: u64,
    fetcher: &Arc<BlockFetcher>,
    rpc_url: &str,
) -> anyhow::Result<serde_json::Value> {
    let block = fetcher
        .fetch_block(slot)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to fetch block {}: {:?}", slot, e))?;

    let timestamp = block.timestamp().map(|t| t.timestamp()).unwrap_or(0);
    let mut detector = MevInspector::new(slot, timestamp, rpc_url.to_string());
    let mev_events = detector.detect_mev(slot, &block.transactions).await;

    let mut total_profit = 0.0;
    let mut mev_compute_units = 0u64;
    let mut arbitrage_count = 0;
    let mut sandwich_count = 0;

    for event in &mev_events {
        match event {
            pono::MevEvent::Arbitrage(arb) => {
                total_profit += arb.profitability.profit_usd;
                mev_compute_units += arb.compute_units_consumed;
                arbitrage_count += 1;
            }
            pono::MevEvent::Sandwich(sand) => {
                total_profit += sand.profitability.profit_usd;
                mev_compute_units += sand.total_compute_units;
                sandwich_count += 1;
            }
        }
    }

    let nonvote_transactions = block.transactions.iter().filter(|tx| !tx.is_vote()).count();

    Ok(json!({
        "slot": block.slot,
        "blockhash": block.blockhash,
        "timestamp": block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
        "total_transactions": block.transactions.len(),
        "successful_transactions": block.successful_tx_count(),
        "nonvote_transactions": nonvote_transactions,
        "total_compute_units": block.total_compute_units(),
        "mev_transaction_count": mev_events.len(),
        "mev_compute_units": mev_compute_units,
        "total_profit_usd": total_profit,
        "arbitrage_count": arbitrage_count,
        "sandwich_count": sandwich_count,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 500,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let rpc_url = config.rpc_url.clone();
    let fetcher = Arc::new(BlockFetcher::new(config));

    match cli.command {
        Commands::Stream => {
            let mut stream = BlockStream::follow_tip(fetcher.clone());

            while let Some((slot, result)) = stream.next().await {
                match result {
                    Ok(block) => {
                        let timestamp = block.timestamp().map(|t| t.timestamp()).unwrap_or(0);
                        let mut detector = MevInspector::new(slot, timestamp, rpc_url.clone());
                        let mev_events = detector.detect_mev(slot, &block.transactions).await;

                        let mut total_profit = 0.0;
                        let mut mev_compute_units = 0u64;
                        let mut arb_count = 0;
                        let mut sandwich_count = 0;

                        for event in &mev_events {
                            match event {
                                pono::MevEvent::Arbitrage(arb) => {
                                    total_profit += arb.profitability.profit_usd;
                                    mev_compute_units += arb.compute_units_consumed;
                                    arb_count += 1;
                                }
                                pono::MevEvent::Sandwich(sand) => {
                                    total_profit += sand.profitability.profit_usd;
                                    mev_compute_units += sand.total_compute_units;
                                    sandwich_count += 1;
                                }
                            }
                        }

                        if !mev_events.is_empty() {
                            println!(
                                "Slot {}: {} MEV txs ({} arb, {} sandwich) | ${:.2} profit | {} CU",
                                slot,
                                mev_events.len(),
                                arb_count,
                                sandwich_count,
                                total_profit,
                                mev_compute_units
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("Error fetching slot {}: {:?}", slot, e);
                    }
                }
            }
        }

        Commands::Run { slot_spec, mode } => {
            match mode {
                Some(RunMode::Slot { slot_spec }) => {
                    // pono run slot <slot_spec>
                    let (start, end) = parse_slot_spec(&slot_spec)?;

                    if start == end {
                        let output = analyze_slot_summary(start, &fetcher, &rpc_url).await?;
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        let mut results = Vec::new();
                        for slot in start..=end {
                            match analyze_slot_summary(slot, &fetcher, &rpc_url).await {
                                Ok(output) => results.push(output),
                                Err(e) => {
                                    eprintln!("Error analyzing slot {}: {}", slot, e);
                                }
                            }
                        }
                        println!("{}", serde_json::to_string_pretty(&results)?);
                    }
                }
                None => {
                    let slot_spec = slot_spec.ok_or_else(|| {
                        anyhow::anyhow!("Slot specification required. Usage: pono run <slot> or pono run <start>-<end>")
                    })?;
                    let (start, end) = parse_slot_spec(&slot_spec)?;

                    if start == end {
                        let output = analyze_slot_mev(start, &fetcher, &rpc_url).await?;
                        println!("{}", serde_json::to_string_pretty(&output)?);
                    } else {
                        let mut results = Vec::new();
                        for slot in start..=end {
                            match analyze_slot_mev(slot, &fetcher, &rpc_url).await {
                                Ok(output) => results.push(output),
                                Err(e) => {
                                    eprintln!("Error analyzing slot {}: {}", slot, e);
                                }
                            }
                        }
                        println!("{}", serde_json::to_string_pretty(&results)?);
                    }
                }
            }
        }
    }

    Ok(())
}
