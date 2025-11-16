//! Pricing and token metadata module
//!
//! Provides accurate token pricing and metadata for profit calculations

pub mod metadata;
pub mod oracle;
pub mod calculator;
pub mod cex_oracle;

pub use metadata::{TokenMetadata, MetadataCache};
pub use oracle::{PriceOracle, TokenPrice};
pub use calculator::ProfitCalculator;
pub use cex_oracle::{CexOracle, CexPrice, AggregatedCexPrice, TokenMapping};

/// Wrapped SOL (WSOL) token address
pub const WSOL_ADDRESS: &str = "So11111111111111111111111111111111111111112";

/// USDC token address
pub const USDC_ADDRESS: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
