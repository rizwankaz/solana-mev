use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleTokenChange {
    pub mint: String,
    pub delta: i64,
    pub decimals: u8,
}

impl TokenChange {
    pub fn to_simple(&self) -> SimpleTokenChange {
        SimpleTokenChange {
            mint: self.mint.clone(),
            delta: self.delta,
            decimals: self.decimals,
        }
    }
}
