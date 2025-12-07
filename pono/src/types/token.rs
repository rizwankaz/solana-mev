use serde::{Deserialize, Serialize};

/// Detailed token balance change with full transaction context
#[derive(Debug, Clone)]
pub struct TokenChange {
    pub account_index: usize,
    pub mint: String,
    pub owner: String,
    pub pre_amount: u64,
    pub post_amount: u64,
    pub delta: i64,
    pub decimals: u8,
}

/// Simplified token change for JSON serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTokenChange {
    pub mint: String,
    pub delta: i64,
    pub decimals: u8,
}

impl TokenChange {
    /// Convert to simplified version for JSON output
    pub fn to_simple(&self) -> SimpleTokenChange {
        SimpleTokenChange {
            mint: self.mint.clone(),
            delta: self.delta,
            decimals: self.decimals,
        }
    }
}
