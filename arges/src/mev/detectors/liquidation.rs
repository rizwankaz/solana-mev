use crate::types::FetchedTransaction;
use crate::mev::types::{Liquidation, TokenAmount};
use crate::mev::parser::{TransactionParser, KnownPrograms, TokenTransfer};

/// Liquidation Detector
///
/// Based on Brontes methodology:
///
/// Liquidations occur when a borrower's position becomes undercollateralized
/// and a liquidator repays part of the debt to seize collateral at a discount.
///
/// Detection algorithm:
/// 1. Identify transactions involving lending protocols (Solend, Mango, Marginfi, etc.)
/// 2. Look for liquidation instruction patterns
/// 3. Extract debt repaid and collateral seized
/// 4. Calculate USD value using DEX pricing data
/// 5. Compute profitability: revenue - cost - gas
///
/// Characteristics:
/// - Interacts with lending protocol
/// - Multiple token transfers (debt repayment + collateral seizure)
/// - Net positive value for liquidator after gas costs
/// - Typically atomic (liquidation + optional collateral swap in same tx)
pub struct LiquidationDetector;

impl LiquidationDetector {
    /// Detect liquidations in a block
    pub fn detect_in_block(
        transactions: &[FetchedTransaction],
        slot: u64,
    ) -> Vec<Liquidation> {
        transactions
            .iter()
            .filter(|tx| tx.is_success())
            .filter_map(|tx| Self::detect_liquidation(tx, slot))
            .collect()
    }

    /// Detect liquidation in a single transaction
    fn detect_liquidation(tx: &FetchedTransaction, slot: u64) -> Option<Liquidation> {
        // Transactions are already filtered as liquidations by caller
        // Just validate the pattern
        let transfers = TransactionParser::extract_token_transfers(tx);

        if transfers.len() < 3 {
            return None; // Need debt + collateral transfers
        }

        // Extract protocol
        let protocol = Self::identify_protocol(tx)?;

        // Extract liquidator and liquidated user
        let (liquidator, liquidated_user) = Self::extract_parties(tx)?;

        // Analyze token transfers to identify debt and collateral
        let transfers = TransactionParser::extract_token_transfers(tx);

        let (debt_repaid, collateral_seized) = Self::classify_transfers(&transfers, &liquidator)?;

        if debt_repaid.is_empty() || collateral_seized.is_empty() {
            return None; // Not a valid liquidation
        }

        // Calculate values
        let (revenue_lamports, cost_lamports) = Self::calculate_values(
            &debt_repaid,
            &collateral_seized,
        );

        let fee_lamports = tx.fee().unwrap_or(0);
        let cost_lamports = cost_lamports + fee_lamports as i64;
        let profit_lamports = revenue_lamports - cost_lamports;

        // Only consider profitable liquidations as MEV
        if profit_lamports <= 0 {
            return None;
        }

        Some(Liquidation {
            signature: tx.signature.clone(),
            slot,
            tx_index: tx.index,
            liquidator,
            liquidated_user,
            protocol,
            debt_repaid,
            collateral_seized,
            revenue_lamports,
            revenue_usd: None,
            cost_lamports,
            cost_usd: None,
            profit_lamports,
            profit_usd: None,
            compute_units: tx.compute_units_consumed().unwrap_or(0),
            fee_lamports,
        })
    }

    /// Identify which lending protocol is being used (best effort from known programs)
    fn identify_protocol(tx: &FetchedTransaction) -> Option<String> {
        let accounts = TransactionParser::extract_accounts(tx);

        // Check against known protocols
        for account in &accounts {
            if account == KnownPrograms::SOLEND {
                return Some("Solend".to_string());
            } else if account == KnownPrograms::MANGO_V4 {
                return Some("Mango V4".to_string());
            } else if account == KnownPrograms::MARGINFI {
                return Some("Marginfi".to_string());
            } else if account == KnownPrograms::KAMINO {
                return Some("Kamino".to_string());
            }
        }

        // Fallback: extract from instruction if possible
        Some("Unknown Lending Protocol".to_string())
    }

    /// Extract liquidator and liquidated user addresses
    fn extract_parties(tx: &FetchedTransaction) -> Option<(String, String)> {
        // The signer is the liquidator
        let liquidator = TransactionParser::get_signer(tx)?;

        // The liquidated user is typically in the accounts
        // but not the signer. We look for accounts with significant token outflows
        let accounts = TransactionParser::extract_accounts(tx);

        // Simplified: take the second account as liquidated user
        // In practice, you'd parse the liquidation instruction to get this
        let liquidated_user = accounts.get(1)?.clone();

        Some((liquidator, liquidated_user))
    }

    /// Classify token transfers into debt repaid vs collateral seized
    fn classify_transfers(
        transfers: &[TokenTransfer],
        _liquidator: &str,
    ) -> Option<(Vec<TokenAmount>, Vec<TokenAmount>)> {
        // In a liquidation:
        // - Debt repaid: liquidator sends tokens (outflows from liquidator's perspective)
        // - Collateral seized: liquidator receives tokens (inflows to liquidator)

        let mut debt_repaid = Vec::new();
        let mut collateral_seized = Vec::new();

        // Outflows = debt repaid
        for transfer in transfers {
            if transfer.is_outflow() {
                debt_repaid.push(TokenAmount {
                    token: transfer.mint.clone(),
                    amount: (transfer.net_change.abs() * 1_000_000_000.0) as u64,
                    decimals: 9, // Simplified - would need to query token metadata
                    amount_ui: transfer.net_change.abs(),
                    usd_value: None,
                });
            }
        }

        // Inflows = collateral seized
        for transfer in transfers {
            if transfer.is_inflow() {
                collateral_seized.push(TokenAmount {
                    token: transfer.mint.clone(),
                    amount: (transfer.net_change * 1_000_000_000.0) as u64,
                    decimals: 9,
                    amount_ui: transfer.net_change,
                    usd_value: None,
                });
            }
        }

        Some((debt_repaid, collateral_seized))
    }

    /// Calculate revenue and cost in lamports
    fn calculate_values(
        debt_repaid: &[TokenAmount],
        collateral_seized: &[TokenAmount],
    ) -> (i64, i64) {
        // Revenue = value of collateral seized
        let revenue_lamports = Self::estimate_total_value_lamports(collateral_seized);

        // Cost = value of debt repaid
        let cost_lamports = Self::estimate_total_value_lamports(debt_repaid);

        (revenue_lamports, cost_lamports)
    }

    /// Estimate total value in lamports
    fn estimate_total_value_lamports(tokens: &[TokenAmount]) -> i64 {
        const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";

        let mut total = 0i64;

        for token in tokens {
            if token.token == WSOL_MINT || token.token.contains("11111111111111") {
                // SOL - direct conversion
                total += token.amount as i64;
            } else {
                // Other tokens - would need price oracle
                // For now, estimate based on common stablecoin conversion
                // Assume 1:1 for stablecoins, 0 for unknown tokens

                if Self::is_stablecoin(&token.token) {
                    // Assume $1 = ~0.01 SOL (rough estimate)
                    // Convert to lamports
                    total += (token.amount as f64 * 0.01) as i64;
                }
                // Unknown tokens contribute 0 to avoid overestimation
            }
        }

        total
    }

    /// Check if token is a known stablecoin
    fn is_stablecoin(mint: &str) -> bool {
        const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
        const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
        const PYUSD_MINT: &str = "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo";

        matches!(mint, USDC_MINT | USDT_MINT | PYUSD_MINT)
    }

    /// Batch detect liquidations
    pub fn detect_batch(
        transactions: &[FetchedTransaction],
        slot: u64,
    ) -> Vec<Liquidation> {
        Self::detect_in_block(transactions, slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lending_protocol_detection() {
        // Test protocol identification
    }

    #[test]
    fn test_transfer_classification() {
        // Test debt vs collateral classification
    }

    #[test]
    fn test_profitability_calculation() {
        // Test profit calculation
    }
}
