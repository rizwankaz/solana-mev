//! Sandwich attack detection
//!
//! Detects sandwich attacks where a searcher frontruns and backruns a victim's transaction

use super::types::*;
use crate::dex::ParsedSwap;
use crate::types::FetchedBlock;
use anyhow::Result;
use std::collections::HashMap;

/// Sandwich attack detector
pub struct SandwichDetector {
    /// Maximum slot distance between frontrun and backrun
    max_slot_distance: u64,

    /// Minimum profit threshold
    min_profit_lamports: i64,

    /// Minimum victim trade size to be considered
    min_victim_size: u64,
}

impl SandwichDetector {
    /// Create new sandwich detector
    pub fn new(max_slot_distance: u64, min_profit_lamports: i64, min_victim_size: u64) -> Self {
        Self {
            max_slot_distance,
            min_profit_lamports,
            min_victim_size,
        }
    }

    /// Detect sandwich attacks in a block
    pub fn detect(&self, block: &FetchedBlock, swaps: &[ParsedSwap]) -> Result<Vec<MevEvent>> {
        let mut sandwich_events = Vec::new();

        // Sandwiches typically occur within a single block
        // Pattern: Frontrun -> Victim -> Backrun (same pool, opposite directions)

        // Group swaps by pool
        let swaps_by_pool = self.group_swaps_by_pool(swaps);

        for (_pool, pool_swaps) in swaps_by_pool.iter() {
            // Look for sandwich patterns in each pool
            sandwich_events.extend(self.detect_in_pool(pool_swaps, block)?);
        }

        Ok(sandwich_events)
    }

    /// Group swaps by pool address
    fn group_swaps_by_pool<'a>(
        &self,
        swaps: &'a [ParsedSwap],
    ) -> HashMap<String, Vec<&'a ParsedSwap>> {
        let mut by_pool: HashMap<String, Vec<&ParsedSwap>> = HashMap::new();

        for swap in swaps {
            by_pool.entry(swap.pool.clone()).or_default().push(swap);
        }

        by_pool
    }

    /// Detect sandwich attacks in a specific pool
    fn detect_in_pool(&self, swaps: &[&ParsedSwap], block: &FetchedBlock) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        if swaps.len() < 3 {
            return Ok(events);
        }

        // Sort swaps by transaction index to get chronological order
        let mut sorted_swaps = swaps.to_vec();
        sorted_swaps.sort_by_key(|s| s.tx_index);

        // Look for pattern: A->B, X->Y, B->A where:
        // - First and third swaps are by same user (sandwicher)
        // - Second swap is by different user (victim)
        // - First and third swaps are opposite directions
        // - Middle swap is larger than threshold

        // Track processed frontrun/backrun pairs to avoid duplicates
        let mut processed_pairs = std::collections::HashSet::new();

        for i in 0..sorted_swaps.len() {
            for k in i + 2..sorted_swaps.len() {
                let frontrun = sorted_swaps[i];
                let backrun = sorted_swaps[k];

                // Check if this is a valid frontrun/backrun pair
                if !self.is_valid_sandwich_pair(frontrun, backrun) {
                    continue;
                }

                // Create unique key for this pair
                let pair_key = format!("{}:{}", frontrun.signature, backrun.signature);
                if processed_pairs.contains(&pair_key) {
                    continue;
                }
                processed_pairs.insert(pair_key);

                // Find the largest victim between frontrun and backrun
                let mut best_victim: Option<&ParsedSwap> = None;
                let mut best_victim_size = 0u64;

                for j in i + 1..k {
                    let potential_victim = sorted_swaps[j];

                    // Check if this swap could be a victim
                    if potential_victim.user == frontrun.user {
                        continue; // Same user as sandwicher
                    }

                    if potential_victim.amount_in < self.min_victim_size {
                        continue; // Too small
                    }

                    // Check if victim trades the same tokens
                    let victim_direction = (&potential_victim.token_in, &potential_victim.token_out);
                    let frontrun_direction = (&frontrun.token_in, &frontrun.token_out);
                    let backrun_direction = (&backrun.token_in, &backrun.token_out);

                    if victim_direction != frontrun_direction && victim_direction != backrun_direction {
                        continue;
                    }

                    // Track the largest victim
                    if potential_victim.amount_in > best_victim_size {
                        best_victim = Some(potential_victim);
                        best_victim_size = potential_victim.amount_in;
                    }
                }

                // If we found a victim, create one sandwich event
                if let Some(victim) = best_victim {
                    if let Some(sandwich) =
                        self.check_sandwich_pattern(frontrun, victim, backrun, block)?
                    {
                        events.push(sandwich);
                    }
                }
            }
        }

        Ok(events)
    }

    /// Check if two swaps form a valid frontrun/backrun pair
    fn is_valid_sandwich_pair(&self, frontrun: &ParsedSwap, backrun: &ParsedSwap) -> bool {
        // Same user
        if frontrun.user != backrun.user {
            return false;
        }

        // Opposite directions
        let frontrun_direction = (&frontrun.token_in, &frontrun.token_out);
        let backrun_direction = (&backrun.token_in, &backrun.token_out);

        if frontrun_direction != (backrun_direction.1, backrun_direction.0) {
            return false;
        }

        // Profitable
        let profit = (backrun.amount_out as i64) - (frontrun.amount_in as i64);
        if profit < self.min_profit_lamports {
            return false;
        }

        true
    }

    /// Check if three swaps form a sandwich pattern
    fn check_sandwich_pattern(
        &self,
        frontrun: &ParsedSwap,
        victim: &ParsedSwap,
        backrun: &ParsedSwap,
        block: &FetchedBlock,
    ) -> Result<Option<MevEvent>> {
        // Criteria for sandwich:
        // 1. Frontrun and backrun by same user
        if frontrun.user != backrun.user {
            return Ok(None);
        }

        // 2. Victim is different user
        if victim.user == frontrun.user {
            return Ok(None);
        }

        // 3. Victim trade is above minimum size
        if victim.amount_in < self.min_victim_size {
            return Ok(None);
        }

        // 4. Frontrun and backrun are opposite directions
        let frontrun_direction = (&frontrun.token_in, &frontrun.token_out);
        let backrun_direction = (&backrun.token_in, &backrun.token_out);

        if frontrun_direction != (backrun_direction.1, backrun_direction.0) {
            return Ok(None);
        }

        // 5. Victim trades same tokens (in either direction)
        let victim_direction = (&victim.token_in, &victim.token_out);
        if victim_direction != frontrun_direction && victim_direction != backrun_direction {
            return Ok(None);
        }

        // 6. Transaction indices are in order
        if !(frontrun.tx_index < victim.tx_index && victim.tx_index < backrun.tx_index) {
            return Ok(None);
        }

        // Calculate profit
        let profit = (backrun.amount_out as i64) - (frontrun.amount_in as i64);

        if profit < self.min_profit_lamports {
            return Ok(None);
        }

        // Estimate victim loss due to price impact
        let victim_loss = self.estimate_victim_loss(frontrun, victim, backrun);

        // Build sandwich metadata
        let metadata = SandwichMetadata {
            frontrun_tx: frontrun.signature.clone(),
            victim_tx: victim.signature.clone(),
            backrun_tx: backrun.signature.clone(),
            token: frontrun.token_in.clone(),
            victim_swap: self.swap_to_details(victim),
            frontrun_swap: self.swap_to_details(frontrun),
            backrun_swap: self.swap_to_details(backrun),
            victim_loss: Some(victim_loss),
            profit,
            pool: frontrun.pool.clone(),
        };

        Ok(Some(MevEvent {
            mev_type: MevType::Sandwich,
            slot: block.slot,
            timestamp: block.timestamp().unwrap_or_else(chrono::Utc::now),
            transactions: vec![
                frontrun.signature.clone(),
                victim.signature.clone(),
                backrun.signature.clone(),
            ],
            profit_lamports: Some(profit),
            profit_usd: None,
            tokens: vec![frontrun.token_in.clone(), frontrun.token_out.clone()],
            metadata: MevMetadata::Sandwich(metadata),
            extractor: Some(frontrun.user.clone()),
            confidence: self.calculate_confidence(frontrun, victim, backrun),
        }))
    }

    /// Calculate victim's loss due to sandwich attack
    ///
    /// The victim's loss is the difference between:
    /// - What they would have received at the pre-sandwich "fair" price
    /// - What they actually received (at the inflated price from frontrun)
    ///
    /// We approximate the fair price by averaging frontrun and backrun prices,
    /// since the sandwich moves the price up (frontrun) and back down (backrun)
    fn estimate_victim_loss(
        &self,
        frontrun: &ParsedSwap,
        victim: &ParsedSwap,
        backrun: &ParsedSwap,
    ) -> i64 {
        // Sandwich mechanics:
        // 1. Frontrun: Buys token, moves price UP
        // 2. Victim: Buys at inflated price (gets LESS output than fair)
        // 3. Backrun: Sells token, moves price back DOWN

        // Check if victim is buying or selling the same direction as frontrun
        // Victim loses when trading in the same direction as frontrun
        let victim_in_same_direction = victim.token_in == frontrun.token_in;

        if !victim_in_same_direction {
            // Victim is trading opposite direction, different sandwich type
            // Still calculate loss but use different logic
            return 0;
        }

        // Calculate prices (output/input ratio)
        let frontrun_price = frontrun.amount_out as f64 / frontrun.amount_in as f64;
        let backrun_price = backrun.amount_out as f64 / backrun.amount_in as f64;
        let victim_price = victim.amount_out as f64 / victim.amount_in as f64;

        // Fair price ≈ average of pre-sandwich and post-sandwich prices
        // This assumes the pool returns to approximately the same state
        let fair_price = (frontrun_price + backrun_price) / 2.0;

        // Victim should have received more at fair price
        let fair_output = (victim.amount_in as f64 * fair_price) as u64;
        let actual_output = victim.amount_out;

        // Loss in output token terms
        let loss = if fair_output > actual_output {
            (fair_output - actual_output) as i64
        } else {
            // Fallback: estimate as percentage of victim's trade
            // Typical sandwich extracts 0.5-2% from victim
            let estimated_loss_pct = 0.01; // 1% conservative estimate
            (victim.amount_out as f64 * estimated_loss_pct) as i64
        };

        loss
    }

    /// Convert ParsedSwap to SwapDetails
    fn swap_to_details(&self, swap: &ParsedSwap) -> SwapDetails {
        SwapDetails {
            dex: swap.dex.name().to_string(),
            pool: swap.pool.clone(),
            token_in: swap.token_in.clone(),
            token_out: swap.token_out.clone(),
            amount_in: swap.amount_in,
            amount_out: swap.amount_out,
            price_impact: swap.price_impact,
            min_amount_out: swap.min_amount_out,
            signature: swap.signature.clone(),
            tx_index: swap.tx_index,
        }
    }

    /// Calculate confidence score
    fn calculate_confidence(
        &self,
        frontrun: &ParsedSwap,
        victim: &ParsedSwap,
        backrun: &ParsedSwap,
    ) -> f64 {
        let mut confidence: f64 = 0.6;

        // Higher confidence if swaps are consecutive
        if backrun.tx_index - frontrun.tx_index == 2 {
            confidence += 0.2;
        }

        // Higher confidence if victim has low slippage tolerance (unexpected impact)
        if let Some(slippage) = victim.slippage_percentage() {
            if slippage < 1.0 {
                // Less than 1% slippage set
                confidence += 0.1;
            }
        }

        // Higher confidence if profit is substantial relative to victim trade
        let profit = (backrun.amount_out as i64) - (frontrun.amount_in as i64);
        let profit_ratio = profit as f64 / victim.amount_in as f64;
        if profit_ratio > 0.01 {
            // Profit > 1% of victim trade
            confidence += 0.1;
        }

        confidence.min(1.0)
    }

    /// Detect sandwich attacks across multiple blocks (for slower chains)
    pub fn detect_cross_block(
        &self,
        blocks: &[FetchedBlock],
        all_swaps: &HashMap<u64, Vec<ParsedSwap>>,
    ) -> Result<Vec<MevEvent>> {
        let mut events = Vec::new();

        // For Solana, sandwiches are typically atomic (same block)
        // But we can check adjacent blocks just in case

        for i in 0..blocks.len().saturating_sub(1) {
            let current_block = &blocks[i];
            let next_block = &blocks[i + 1];

            if next_block.slot - current_block.slot > self.max_slot_distance {
                continue;
            }

            // Get swaps from both blocks
            let current_swaps = all_swaps.get(&current_block.slot).map(|v| v.as_slice()).unwrap_or(&[]);
            let next_swaps = all_swaps.get(&next_block.slot).map(|v| v.as_slice()).unwrap_or(&[]);

            // Look for frontrun in current block, victim and backrun in next
            // (Less common on Solana, but possible)
            for frontrun in current_swaps {
                for victim in next_swaps {
                    for backrun in next_swaps {
                        if victim.tx_index >= backrun.tx_index {
                            continue;
                        }

                        if let Some(sandwich) = self.check_sandwich_pattern(
                            frontrun,
                            victim,
                            backrun,
                            next_block,
                        )? {
                            events.push(sandwich);
                        }
                    }
                }
            }
        }

        Ok(events)
    }
}

impl Default for SandwichDetector {
    fn default() -> Self {
        Self::new(
            1,           // Within 1 slot
            100_000,     // 0.0001 SOL minimum profit
            1_000_000,   // 0.001 SOL minimum victim size
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandwich_detector_creation() {
        let detector = SandwichDetector::default();
        assert_eq!(detector.max_slot_distance, 1);
        assert_eq!(detector.min_profit_lamports, 100_000);
    }

    #[test]
    fn test_custom_sandwich_detector() {
        let detector = SandwichDetector::new(5, 500_000, 5_000_000);
        assert_eq!(detector.max_slot_distance, 5);
        assert_eq!(detector.min_profit_lamports, 500_000);
        assert_eq!(detector.min_victim_size, 5_000_000);
    }
}
