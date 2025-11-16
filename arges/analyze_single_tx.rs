// Analyze a single transaction to debug MEV detection
use arges::{BlockFetcher, DexParser, FetcherConfig, MevDetector, ProfitCalculator, PriceOracle, MetadataCache, CexOracle, CexDexDetector};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Setup
    let config = FetcherConfig {
        rpc_url: std::env::var("SOLANA_RPC_URL")
            .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string()),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = Arc::new(BlockFetcher::new(config));

    // Fetch the block containing the transaction
    let slot = 380404433;
    println!("Fetching block {}...", slot);
    let block = fetcher.fetch_block(slot).await?;

    // Find the specific transaction
    let target_sig = "5JCvjAouCCX1Wd569RNqGueTkcJuQcdTVV6vxVhepPkTiFAruZMFfUiwyTenF458aMwwswi3UQQ4uGrxYLPqQV3K";

    let tx = block.transactions.iter()
        .enumerate()
        .find(|(_, tx)| tx.signature == target_sig);

    if let Some((idx, tx)) = tx {
        println!("\n✓ Found transaction at index {}", idx);
        println!("Signature: {}", tx.signature);

        // Check inner instructions first
        if let Some(meta) = &tx.meta {
            if let solana_transaction_status::option_serializer::OptionSerializer::Some(inner_ixs) = &meta.inner_instructions {
                println!("\n📋 Inner Instructions: {} groups", inner_ixs.len());
                for (i, inner_group) in inner_ixs.iter().enumerate() {
                    println!("  Group {}: {} instructions", i, inner_group.instructions.len());
                    for (j, ix) in inner_group.instructions.iter().enumerate() {
                        match ix {
                            solana_transaction_status::UiInstruction::Parsed(ui_parsed) => {
                                match ui_parsed {
                                    solana_transaction_status::UiParsedInstruction::Parsed(parsed) => {
                                        println!("    Instruction {}: {} (program: {})", j, parsed.parsed.get("type").and_then(|v| v.as_str()).unwrap_or("unknown"), parsed.program);
                                    }
                                    solana_transaction_status::UiParsedInstruction::PartiallyDecoded(partial) => {
                                        println!("    Instruction {}: PartiallyDecoded (program: {})", j, partial.program_id);
                                    }
                                }
                            }
                            solana_transaction_status::UiInstruction::Compiled(_) => {
                                println!("    Instruction {}: Compiled", j);
                            }
                        }
                    }
                }
            } else {
                println!("\n📋 No inner instructions found");
            }
        }

        // Parse swaps from this transaction
        let swaps = DexParser::parse_transaction(tx, idx)?;
        println!("\n📊 Detected {} swap(s):", swaps.len());
        for (i, swap) in swaps.iter().enumerate() {
            println!("\n  Swap {}:", i + 1);
            println!("    User: {}", swap.user);
            println!("    DEX: {}", swap.dex.name());
            println!("    {} {} -> {} {}",
                swap.amount_in, swap.token_in,
                swap.amount_out, swap.token_out
            );
        }

        // Check token balances
        if let Some(meta) = &tx.meta {
            println!("\n💰 Token Balance Changes:");

            if let solana_transaction_status::option_serializer::OptionSerializer::Some(pre_balances) = &meta.pre_token_balances {
                if let solana_transaction_status::option_serializer::OptionSerializer::Some(post_balances) = &meta.post_token_balances {
                    println!("\n  Pre-balances: {} accounts", pre_balances.len());
                    println!("  Post-balances: {} accounts", post_balances.len());

                    // Show balance changes
                    for post in post_balances {
                        if let Some(pre) = pre_balances.iter().find(|p| p.account_index == post.account_index) {
                            let pre_amt = pre.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
                            let post_amt = post.ui_token_amount.amount.parse::<i128>().unwrap_or(0);
                            let change = post_amt - pre_amt;

                            if change != 0 {
                                let owner = post.owner.clone().unwrap_or_else(|| "Unknown".to_string());
                                println!("\n    Account {}: {}", post.account_index, owner);
                                println!("      Mint: {}", post.mint);
                                println!("      Change: {} ({})",
                                    change,
                                    if change > 0 { "increase" } else { "decrease" }
                                );
                            }
                        }
                    }
                }
            }
        }

        // Now run MEV detection on the full block
        println!("\n\n🔍 Running MEV Detection...");

        let metadata_cache = Arc::new(MetadataCache::new(
            std::env::var("SOLANA_RPC_URL")
                .unwrap_or_else(|_| "https://api.mainnet-beta.solana.com".to_string())
        ));
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
            .with_cex_dex_detector(Arc::clone(&cex_dex_detector));

        let analysis = detector.detect_block_with_pricing(&block).await?;

        println!("\n📈 MEV Events in Block: {}", analysis.events.len());

        // Check if any events involve our transaction
        let events_with_tx: Vec<_> = analysis.events.iter()
            .filter(|e| e.transactions.contains(&target_sig.to_string()))
            .collect();

        if events_with_tx.is_empty() {
            println!("  ❌ No MEV events detected for this transaction");
        } else {
            println!("  ✓ Found {} MEV event(s) involving this transaction:", events_with_tx.len());
            for event in events_with_tx {
                println!("\n    Type: {}", event.mev_type.name());
                println!("    Profit: {} lamports", event.profit_lamports.unwrap_or(0));
                println!("    Confidence: {:.1}%", event.confidence * 100.0);
            }
        }

    } else {
        println!("❌ Transaction not found in block {}", slot);
    }

    Ok(())
}
