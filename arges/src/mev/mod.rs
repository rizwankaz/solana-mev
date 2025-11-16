/// MEV Detection and Classification Module
///
/// This module provides comprehensive MEV detection capabilities for Solana,
/// including arbitrage, sandwich attacks, liquidations, and JIT liquidity.

pub mod types;
pub mod detector;
pub mod arbitrage;
pub mod sandwich;
pub mod liquidation;
pub mod jit;
pub mod classifier;
pub mod adaptive;

pub use types::*;
pub use detector::{MevDetector, BlockMevAnalysis, DetectorConfig};
pub use classifier::MevClassifier;
pub use adaptive::{AdaptiveMevDetector, EnhancedMevAnalysis, AdaptiveConfig, AdaptiveThresholds};
