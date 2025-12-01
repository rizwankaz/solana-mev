use crate::mev::{MevEvent, MevAnalyzer};
use crate::types::FetchedBlock;
use serde::Serialize;

/// JSON structure for MEV validation report
#[derive(Serialize)]
pub struct MevValidationJson {
    pub slot: u64,
    pub blockhash: String,
    pub timestamp: Option<String>,
    pub total_transactions: usize,
    pub mev_count: usize,
    pub mev_transactions: Vec<MevTransactionJson>,
    pub sandwich_attacks: Vec<MultiTxMevJson>,
    pub jit_attacks: Vec<MultiTxMevJson>,
    /// Total net profit in USD across all MEV events in this block
    pub total_net_profit_usd: f64,
}

/// JSON structure for individual MEV transaction
#[derive(Serialize)]
pub struct MevTransactionJson {
    pub signature: String,
    pub attacker_signer: Option<String>,
    pub category: String,
    pub success: bool,
    pub program_addresses: Vec<String>,
    pub token_changes: Vec<TokenChangeJson>,
    pub fee: Option<u64>,
    pub priority_fee: Option<u64>,
    pub compute_units_consumed: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub swaps: Vec<SwapJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profitability: Option<ProfitabilityJson>,
}

/// JSON structure for profitability analysis
#[derive(Serialize)]
pub struct ProfitabilityJson {
    pub profit_usd: f64,
    pub fees_usd: f64,
    pub net_profit_usd: f64,
}

/// JSON structure for individual swap
#[derive(Serialize)]
pub struct SwapJson {
    pub from_token: String,
    pub from_amount: f64,
    pub to_token: String,
    pub to_amount: f64,
    pub dex_program: String,
    pub from_decimals: u8,
    pub to_decimals: u8,
}

/// JSON structure for token changes
#[derive(Serialize)]
pub struct TokenChangeJson {
    pub token_address: String,
    pub amount: f64,
    pub decimals: u8,
}

/// JSON structure for multi-transaction MEV events (sandwich, JIT)
#[derive(Serialize)]
pub struct MultiTxMevJson {
    pub category: String,
    pub attacker: Option<String>,
    pub frontrun_signature: String,
    pub victim_signature: String,
    pub victim: Option<String>,
    pub backrun_signature: String,
    pub profit_tokens: Vec<TokenChangeJson>,
    pub total_sol_profit_lamports: i64,
}

/// Calculate profitability for an MEV event
fn calculate_profitability(
    event: &MevEvent,
    tx: &crate::types::FetchedTransaction,
    prices: &std::collections::HashMap<String, f64>,
) -> Option<crate::mev::Profitability> {
    use crate::price_oracle::PriceOracle;

    // Calculate token profit in USD
    let mut profit_usd = 0.0;
    let mut has_prices = false;

    for token_change in &event.token_changes {
        if let Some(&price) = prices.get(&token_change.mint) {
            profit_usd += token_change.ui_amount_change * price;
            has_prices = true;
        }
    }

    // If we don't have any price data, return None
    if !has_prices {
        return None;
    }

    // Calculate total fees in USD (tx_fee + priority_fee)
    let sol_price = prices.get("So11111111111111111111111111111111111111112").copied().unwrap_or(0.0);

    let tx_fee_sol = tx.fee().map(PriceOracle::lamports_to_sol).unwrap_or(0.0);
    let priority_fee_sol = tx.priority_fee().map(PriceOracle::lamports_to_sol).unwrap_or(0.0);
    let total_fees_sol = tx_fee_sol + priority_fee_sol;
    let fees_usd = total_fees_sol * sol_price;

    // Calculate net profit
    let net_profit_usd = profit_usd - fees_usd;

    Some(crate::mev::Profitability {
        profit_usd,
        fees_usd,
        net_profit_usd,
    })
}

/// Format MEV validation report as JSON with profitability analysis
pub async fn format_mev_validation_json(block: &FetchedBlock) -> Result<String, serde_json::Error> {
    use crate::price_oracle::PriceOracle;
    use std::collections::{HashMap, HashSet};

    let mut mev_transactions = Vec::new();
    let mut tx_with_mev = Vec::new();
    let mut mev_events_with_tx = Vec::new();

    // Collect all MEV events with their transaction indices
    for (idx, tx) in block.transactions.iter().enumerate() {
        if let Some(event) = tx.analyze_mev() {
            mev_events_with_tx.push((event.clone(), tx));
            tx_with_mev.push((idx, tx, Some(event)));
        } else {
            tx_with_mev.push((idx, tx, None));
        }
    }

    // Initialize price oracle (loads token and feed lists at startup)
    let oracle = match PriceOracle::new().await {
        Ok(o) => o,
        Err(e) => {
            tracing::error!("Failed to initialize price oracle: {:?}", e);
            tracing::warn!("Profitability analysis will be unavailable");
            // Continue without profitability - still show MEV events
            let report = MevValidationJson {
                slot: block.slot,
                blockhash: block.blockhash.clone(),
                timestamp: block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string()),
                total_transactions: block.transactions.len(),
                mev_count: 0,
                mev_transactions: vec![],
                sandwich_attacks: vec![],
                jit_attacks: vec![],
                total_net_profit_usd: 0.0,
            };
            return serde_json::to_string_pretty(&report);
        }
    };
    let mut all_mints: HashSet<String> = HashSet::new();

    // Add SOL mint for fee calculations
    all_mints.insert("So11111111111111111111111111111111111111112".to_string());

    // Collect all token mints from token changes
    for (event, _) in &mev_events_with_tx {
        for token_change in &event.token_changes {
            all_mints.insert(token_change.mint.clone());
        }
    }

    let mints_vec: Vec<String> = all_mints.into_iter().collect();

    // Use historical prices from the block timestamp for accurate profitability analysis
    let prices = match oracle.fetch_prices(&mints_vec, block.block_time).await {
        Ok(p) => {
            if block.block_time.is_some() {
                tracing::info!("successfully fetched {} historical token prices from Pyth for block timestamp {}",
                    p.len(), block.block_time.unwrap());
            } else {
                tracing::info!("successfully fetched {} current token prices from Pyth", p.len());
            }
            p
        }
        Err(e) => {
            tracing::error!("Failed to fetch prices from Pyth: {:?}", e);
            HashMap::new()
        }
    };

    // Build MEV transactions with profitability
    for (event, tx) in mev_events_with_tx {
        let swaps_json: Vec<SwapJson> = event.swaps.iter()
            .map(|swap| SwapJson {
                from_token: swap.from_token.clone(),
                from_amount: swap.from_amount,
                to_token: swap.to_token.clone(),
                to_amount: swap.to_amount,
                dex_program: swap.dex_program.clone(),
                from_decimals: swap.from_decimals,
                to_decimals: swap.to_decimals,
            })
            .collect();

        let swap_count = if event.swap_count > 0 {
            Some(event.swap_count)
        } else {
            None
        };

        // Calculate profitability
        let profitability = calculate_profitability(&event, tx, &prices);

        // Only include successful and profitable trades
        // Exclude failed transactions and unprofitable swaps
        let should_include = event.success && profitability.as_ref()
            .map(|p| p.net_profit_usd > 0.0)
            .unwrap_or(false); // Exclude if we couldn't calculate profitability

        if should_include {
            mev_transactions.push(MevTransactionJson {
                signature: event.signature.clone(),
                attacker_signer: event.attacker_signer.clone(),
                category: format!("{:?}", event.category).to_uppercase(),
                success: event.success,
                program_addresses: event.programs_involved.clone(),
                token_changes: event.token_changes.iter()
                    .map(|tc| TokenChangeJson {
                        token_address: tc.mint.clone(),
                        amount: tc.ui_amount_change,
                        decimals: tc.decimals,
                    })
                    .collect(),
                fee: tx.fee(),
                priority_fee: tx.priority_fee(),
                compute_units_consumed: tx.compute_units_consumed(),
                swaps: swaps_json,
                swap_count,
                profitability: profitability.map(|p| ProfitabilityJson {
                    profit_usd: p.profit_usd,
                    fees_usd: p.fees_usd,
                    net_profit_usd: p.net_profit_usd,
                }),
            });
        }
    }

    // Detect multi-transaction MEV events (sandwich, JIT)
    let multi_tx_events = MevAnalyzer::detect_multi_tx_mev(&tx_with_mev);

    let mut sandwich_attacks = Vec::new();
    let mut jit_attacks = Vec::new();

    for event in multi_tx_events {
        let json_event = MultiTxMevJson {
            category: format!("{:?}", event.category).to_uppercase(),
            attacker: event.attacker,
            frontrun_signature: event.frontrun_signature,
            victim_signature: event.victim_signature,
            victim: event.victim,
            backrun_signature: event.backrun_signature,
            profit_tokens: event.profit_token_changes.iter()
                .map(|tc| TokenChangeJson {
                    token_address: tc.mint.clone(),
                    amount: tc.ui_amount_change,
                    decimals: tc.decimals,
                })
                .collect(),
            total_sol_profit_lamports: event.total_sol_profit_lamports,
        };

        match event.category {
            crate::mev::MevCategory::Sandwich => sandwich_attacks.push(json_event),
            crate::mev::MevCategory::JitLiquidity => jit_attacks.push(json_event),
            _ => {}
        }
    }

    let timestamp = block.timestamp().map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string());

    let mev_count = mev_transactions.len();

    // Calculate total net profit across all MEV events in this block
    let total_net_profit_usd: f64 = mev_transactions.iter()
        .filter_map(|tx| tx.profitability.as_ref())
        .map(|p| p.net_profit_usd)
        .sum();

    let report = MevValidationJson {
        slot: block.slot,
        blockhash: block.blockhash.clone(),
        timestamp,
        total_transactions: block.transactions.len(),
        mev_count,
        mev_transactions,
        sandwich_attacks,
        jit_attacks,
        total_net_profit_usd,
    };

    serde_json::to_string_pretty(&report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lamports_to_sol() {
        assert_eq!(lamports_to_sol(1_000_000_000), "1");
        assert_eq!(lamports_to_sol(500_000_000), "0.5");
        assert_eq!(lamports_to_sol(123_456_789), "0.123456789");
        assert_eq!(lamports_to_sol(100_000_000), "0.1");
    }

    #[test]
    fn test_format_compute_units() {
        assert_eq!(format_compute_units(1000), "1,000");
        assert_eq!(format_compute_units(1000000), "1,000,000");
        assert_eq!(format_compute_units(123456789), "123,456,789");
    }
}
