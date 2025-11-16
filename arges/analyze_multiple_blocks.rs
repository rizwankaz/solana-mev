// Analyze multiple blocks to find MEV patterns
use arges::{BlockFetcher, DexParser, FetcherConfig, MevDetector, ProfitCalculator, PriceOracle, MetadataCache, CexOracle, CexDexDetector};
use std::sync::Arc;
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup
    let rpc_url = std::env::var("SOLANA_RPC_URL")
        .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string());

    let config = FetcherConfig {
        rpc_url: rpc_url.clone(),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));

    // Create RPC client for mint resolution
    let rpc_client = Arc::new(RpcClient::new_with_timeout_and_commitment(
        rpc_url.clone(),
        Duration::from_secs(30),
        CommitmentConfig::confirmed(),
    ));

    let metadata_cache = Arc::new(MetadataCache::new(rpc_url));
    let price_oracle = Arc::new(PriceOracle::new());
    let profit_calculator = Arc::new(ProfitCalculator::new(
        Arc::clone(&metadata_cache),
        Arc::clone(&price_oracle),
    ));

    let cex_oracle = Arc::new(CexOracle::new());
    let cex_dex_detector = Arc::new(CexDexDetector::new(
        Arc::clone(&cex_oracle),
    ));

    let detector = MevDetector::new()
        .with_profit_calculator(Arc::clone(&profit_calculator))
        .with_cex_dex_detector(Arc::clone(&cex_dex_detector))
        .with_rpc_client(Arc::clone(&rpc_client));

    // Test blocks - add blocks known to contain Jito bundles
    let test_slots = vec![
        380404433, // Original test block
        // Add more slots here
    ];

    println!("Analyzing {} blocks for MEV patterns...\n", test_slots.len());

    for slot in test_slots {
        println!("{}", "=".repeat(80));
        println!("Block: {}", slot);
        println!("{}", "=".repeat(80));

        match fetcher.fetch_block(slot).await {
            Ok(block) => {
                println!("✓ Fetched block with {} transactions", block.transactions.len());

                // Parse swaps
                let swaps = DexParser::parse_block(&block.transactions, Some(&rpc_client)).await;
                println!("📊 Detected {} total swaps", swaps.len());

                // Group by user to find potential arbitrage
                use std::collections::HashMap;
                let mut swaps_by_user: HashMap<String, Vec<_>> = HashMap::new();
                for swap in &swaps {
                    swaps_by_user.entry(swap.user.clone()).or_default().push(swap);
                }

                println!("👥 Swaps by {} unique users", swaps_by_user.len());

                // Show users with multiple swaps (potential arbitrage)
                let multi_swap_users: Vec<_> = swaps_by_user.iter()
                    .filter(|(_, swaps)| swaps.len() > 1)
                    .collect();

                if !multi_swap_users.is_empty() {
                    println!("\n🔍 Users with multiple swaps:");
                    for (user, user_swaps) in multi_swap_users {
                        println!("  {} ({} swaps)", &user[..8], user_swaps.len());
                        for swap in user_swaps {
                            println!("    {} {} -> {} {}",
                                swap.amount_in, &swap.token_in[..8],
                                swap.amount_out, &swap.token_out[..8]);
                        }
                    }
                }

                // Run MEV detection
                let analysis = detector.detect_block_with_pricing(&block).await?;

                println!("\n📈 MEV Detection Results:");
                println!("  Total events: {}", analysis.events.len());
                println!("  Total profit: {} lamports", analysis.metrics.total_profit_lamports);

                if !analysis.events.is_empty() {
                    println!("\n  Events by type:");
                    use std::collections::HashMap;
                    let mut by_type: HashMap<String, Vec<_>> = HashMap::new();
                    for event in &analysis.events {
                        by_type.entry(event.mev_type.name().to_string()).or_default().push(event);
                    }

                    for (mev_type, events) in by_type {
                        println!("    {}: {} events", mev_type, events.len());
                        for event in events.iter().take(3) {
                            println!("      - Profit: {} lamports (confidence: {:.1}%)",
                                event.profit_lamports.unwrap_or(0),
                                event.confidence * 100.0);
                        }
                    }
                }
            }
            Err(e) => {
                println!("❌ Failed to fetch block: {}", e);
            }
        }

        println!();
    }

    Ok(())
}
