//! Liquidation detection
//!
//! Detects liquidations on lending protocols like Solend, Mango, MarginFi, Drift

use super::types::*;
use crate::types::{FetchedBlock, FetchedTransaction};
use anyhow::Result;
use solana_transaction_status::{option_serializer::OptionSerializer, UiInstruction};

/// Liquidation detector
pub struct LiquidationDetector {
    /// Minimum liquidation bonus to detect
    min_bonus_lamports: i64,
}

impl LiquidationDetector {
    /// Create new liquidation detector
    pub fn new(min_bonus_lamports: i64) -> Self {
        Self { min_bonus_lamports }
    }

    /// Detect liquidations in a block
    pub fn detect(&self, block: &FetchedBlock) -> Result<Vec<MevEvent>> {
        let mut liquidation_events = Vec::new();

        for tx in &block.transactions {
            if let Some(liquidation) = self.detect_in_transaction(tx, block)? {
                liquidation_events.push(liquidation);
            }
        }

        Ok(liquidation_events)
    }

    /// Detect liquidation in a single transaction
    fn detect_in_transaction(
        &self,
        tx: &FetchedTransaction,
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Simplified: primarily use log parsing for liquidation detection
        // Instruction parsing would require more complex deserialization

        // Check logs for liquidation events
        if let Some(meta) = &tx.meta {
            if let OptionSerializer::Some(log_messages) = &meta.log_messages {
                if let Some(liquidation) = self.parse_liquidation_logs(
                    log_messages,
                    tx,
                    block,
                )? {
                    return Ok(Some(liquidation));
                }
            }
        }

        Ok(None)
    }

    /// Parse liquidation from instruction
    fn parse_liquidation_instruction(
        &self,
        _instruction: &UiInstruction,
        _tx: &FetchedTransaction,
        _block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Simplified: would need to parse actual instruction data
        // For now, rely on log parsing
        Ok(None)
    }

    /// Parse liquidation from transaction logs
    fn parse_liquidation_logs(
        &self,
        logs: &[String],
        tx: &FetchedTransaction,
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Look for liquidation keywords in logs
        for log in logs {
            let log_lower = log.to_lowercase();

            if log_lower.contains("liquidat") {
                // Try to identify the protocol from logs
                let protocol = if log.contains("Solend") || log.contains("solend") {
                    "Solend"
                } else if log.contains("Mango") || log.contains("mango") {
                    "Mango"
                } else if log.contains("MarginFi") || log.contains("marginfi") {
                    "MarginFi"
                } else if log.contains("Drift") || log.contains("drift") {
                    "Drift"
                } else {
                    "Unknown"
                };

                return Ok(Some(self.create_liquidation_event(
                    protocol,
                    tx,
                    block,
                    Some(log.clone()),
                )));
            }
        }

        Ok(None)
    }

    /// Identify lending protocol from program ID
    fn identify_lending_protocol(&self, program_id: &str) -> Option<&'static str> {
        match program_id {
            // Solend
            "So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo" => Some("Solend"),

            // Mango v3
            "mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68" => Some("Mango"),

            // Mango v4
            "4MangoMjqJ2firMokCjjGgoK8d4MXcrgL7XJaL3w6fVg" => Some("Mango"),

            // MarginFi
            "MFv2hWf31Z9kbCa1snEPYctwafyhdvnV7FZnsebVacA" => Some("MarginFi"),

            // Drift
            "dRiftyHA39MWEi3m9aunc5MzRF1JYuBsbn6VPcn33UH" => Some("Drift"),

            _ => None,
        }
    }

    /// Create liquidation event
    fn create_liquidation_event(
        &self,
        protocol: &str,
        tx: &FetchedTransaction,
        block: &FetchedBlock,
        log: Option<String>,
    ) -> MevEvent {
        // Extract liquidator (signer)
        let liquidator = tx.signer().unwrap_or_default();

        // Simplified profit calculation
        // In reality, would parse exact amounts from instruction/logs
        let estimated_bonus = self.estimate_liquidation_bonus(tx, log.as_deref());

        let metadata = LiquidationMetadata {
            protocol: protocol.to_string(),
            liquidated_account: "Unknown".to_string(), // Would parse from instruction
            liquidator: liquidator.clone(),
            assets_seized: Vec::new(), // Would parse from logs/instruction
            debts_repaid: Vec::new(),  // Would parse from logs/instruction
            liquidation_bonus: estimated_bonus,
            health_factor_before: None,
        };

        MevEvent {
            mev_type: MevType::Liquidation,
            slot: block.slot,
            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
            transactions: vec![tx.signature.clone()],
            profit_lamports: Some(estimated_bonus),
            profit_usd: None,
            tokens: Vec::new(), // Would extract from parsed data
            metadata: MevMetadata::Liquidation(metadata),
            extractor: Some(liquidator),
            confidence: 0.75, // Medium-high confidence from log detection
        }
    }

    /// Estimate liquidation bonus from transaction
    fn estimate_liquidation_bonus(&self, tx: &FetchedTransaction, _log: Option<&str>) -> i64 {
        // Simplified: use transaction fee as lower bound
        // Real implementation would parse exact amounts
        let fee = tx.fee().unwrap_or(5000) as i64;

        // Typical liquidation bonus is 5-10% of liquidated amount
        // Estimate based on compute units and fee
        let compute_units = tx.compute_units_consumed().unwrap_or(200_000);

        // Rough heuristic: larger compute = larger liquidation
        let estimated_bonus = (compute_units as i64) * 10;

        estimated_bonus.max(fee * 10)
    }

    /// Detect profitable liquidation opportunities (not yet executed)
    pub fn detect_opportunities(
        &self,
        _block: &FetchedBlock,
    ) -> Result<Vec<LiquidationOpportunity>> {
        // TODO: This would require monitoring account states
        // to find unhealthy positions before they're liquidated
        Ok(Vec::new())
    }
}

/// Liquidation opportunity (not yet executed)
#[derive(Debug, Clone)]
pub struct LiquidationOpportunity {
    pub protocol: String,
    pub account: String,
    pub health_factor: f64,
    pub potential_bonus: i64,
    pub collateral: Vec<AssetAmount>,
    pub debt: Vec<AssetAmount>,
}

impl Default for LiquidationDetector {
    fn default() -> Self {
        Self::new(1_000_000) // 0.001 SOL minimum bonus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identify_lending_protocol() {
        let detector = LiquidationDetector::default();

        assert_eq!(
            detector.identify_lending_protocol("So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo"),
            Some("Solend")
        );

        assert_eq!(
            detector.identify_lending_protocol("mv3ekLzLbnVPNxjSKvqBpU3ZeZXPQdEC3bp5MDEBG68"),
            Some("Mango")
        );

        assert_eq!(
            detector.identify_lending_protocol("unknown_program_id"),
            None
        );
    }
}
