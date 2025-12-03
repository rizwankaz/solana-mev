/// MEV-specific type definitions
///
/// Contains all data structures for representing different types of MEV:
/// - Atomic arbitrage
/// - Sandwich attacks
/// - JIT liquidity
/// - Liquidations
pub mod types;

/// Transaction parser
///
/// Utilities for extracting MEV-relevant data from Solana transactions:
/// - Swap information
/// - Token transfers
/// - Pool interactions
/// - DEX and lending protocol detection
pub mod parser;

/// MEV detection algorithms
///
/// Individual detectors for each MEV type
pub mod detectors;

/// MEV analyzer
///
/// Coordinates all detectors and provides high-level analysis interface
pub mod analyzer;

// Re-export main types for convenience
pub use types::*;
pub use analyzer::{MevAnalyzer, MevType};
