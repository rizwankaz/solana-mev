//! Common types and utilities for DEX parsing

use serde::{Deserialize, Serialize};

/// Information extracted from a DEX swap
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSwap {
    /// DEX protocol name
    pub dex: DexProtocol,

    /// Program ID that was called
    pub program_id: String,

    /// Pool/pair address
    pub pool: String,

    /// User/signer address
    pub user: String,

    /// Input token mint
    pub token_in: String,

    /// Output token mint
    pub token_out: String,

    /// Amount of input token
    pub amount_in: u64,

    /// Amount of output token
    pub amount_out: u64,

    /// Minimum amount out (slippage tolerance)
    pub min_amount_out: Option<u64>,

    /// Price before swap
    pub price_before: Option<f64>,

    /// Price after swap
    pub price_after: Option<f64>,

    /// Price impact percentage
    pub price_impact: Option<f64>,

    /// Transaction signature
    pub signature: String,

    /// Transaction index in block
    pub tx_index: usize,

    /// Instruction index in transaction
    pub instruction_index: usize,
}

/// Supported DEX protocols
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DexProtocol {
    Raydium,
    RaydiumCLMM,
    Orca,
    OrcaWhirlpool,
    Jupiter,
    Phoenix,
    Meteora,
    Lifinity,
    Saber,
    PumpFun,
    Unknown,
}

impl DexProtocol {
    /// Get the program ID for this DEX
    pub fn program_id(&self) -> Option<&'static str> {
        match self {
            DexProtocol::Raydium => Some("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8"),
            DexProtocol::RaydiumCLMM => Some("CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK"),
            DexProtocol::Orca => Some("9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP"),
            DexProtocol::OrcaWhirlpool => Some("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc"),
            DexProtocol::Jupiter => Some("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4"),
            DexProtocol::Phoenix => Some("PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY"),
            DexProtocol::Meteora => Some("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo"),
            DexProtocol::Lifinity => Some("EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S"),
            DexProtocol::Saber => Some("SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ"),
            DexProtocol::PumpFun => Some("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"),
            DexProtocol::Unknown => None,
        }
    }

    /// Get DEX name
    pub fn name(&self) -> &'static str {
        match self {
            DexProtocol::Raydium => "Raydium",
            DexProtocol::RaydiumCLMM => "Raydium CLMM",
            DexProtocol::Orca => "Orca",
            DexProtocol::OrcaWhirlpool => "Orca Whirlpool",
            DexProtocol::Jupiter => "Jupiter",
            DexProtocol::Phoenix => "Phoenix",
            DexProtocol::Meteora => "Meteora",
            DexProtocol::Lifinity => "Lifinity",
            DexProtocol::Saber => "Saber",
            DexProtocol::PumpFun => "Pump.fun",
            DexProtocol::Unknown => "Unknown",
        }
    }

    /// Try to identify DEX from program ID
    pub fn from_program_id(program_id: &str) -> Self {
        match program_id {
            "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8" => DexProtocol::Raydium,
            "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK" => DexProtocol::RaydiumCLMM,
            "9W959DqEETiGZocYWCQPaJ6sBmUzgfxXfqGeTEdp3aQP" => DexProtocol::Orca,
            "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc" => DexProtocol::OrcaWhirlpool,
            "JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4" => DexProtocol::Jupiter,
            "PhoeNiXZ8ByJGLkxNfZRnkUfjvmuYqLR89jjFHGqdXY" => DexProtocol::Phoenix,
            "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo" => DexProtocol::Meteora,
            "EewxydAPCCVuNEyrVN68PuSYdQ7wKn27V9Gjeoi8dy3S" => DexProtocol::Lifinity,
            "SSwpkEEcbUqx4vtoEByFjSkhKdCT862DNVb52nZg1UZ" => DexProtocol::Saber,
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P" => DexProtocol::PumpFun,
            _ => DexProtocol::Unknown,
        }
    }
}

/// Token transfer information
#[derive(Debug, Clone)]
pub struct TokenTransfer {
    pub mint: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
}

/// Helper to calculate price impact
pub fn calculate_price_impact(amount_in: u64, amount_out: u64, price_before: f64) -> Option<f64> {
    if price_before == 0.0 || amount_in == 0 {
        return None;
    }

    let price_after = amount_out as f64 / amount_in as f64;
    let impact = ((price_after - price_before) / price_before).abs() * 100.0;
    Some(impact)
}

impl ParsedSwap {
    /// Calculate the effective price of this swap
    pub fn effective_price(&self) -> f64 {
        if self.amount_in == 0 {
            return 0.0;
        }
        self.amount_out as f64 / self.amount_in as f64
    }

    /// Check if this swap has high slippage tolerance
    pub fn has_high_slippage(&self) -> bool {
        if let Some(min_out) = self.min_amount_out {
            if self.amount_out == 0 {
                return false;
            }
            let slippage = 1.0 - (min_out as f64 / self.amount_out as f64);
            slippage > 0.05 // More than 5% slippage
        } else {
            false
        }
    }

    /// Get slippage percentage
    pub fn slippage_percentage(&self) -> Option<f64> {
        self.min_amount_out.map(|min_out| {
            if self.amount_out == 0 {
                return 0.0;
            }
            (1.0 - (min_out as f64 / self.amount_out as f64)) * 100.0
        })
    }
}
