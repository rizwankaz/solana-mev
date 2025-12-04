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
pub mod parser;

/// Instruction-based parser
///
/// Dynamic detection of swaps, liquidations, and liquidity operations
/// using instruction data and token transfer heuristics instead of
/// hardcoded program IDs
pub mod instruction_parser;

/// Program ID registry
///
/// Registry of known DEX and lending protocol program IDs for supplemental
/// detection when instruction-based analysis is insufficient
pub mod registry;

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
pub use instruction_parser::{InstructionClassifier, TransactionFilter};
pub use registry::ProgramRegistry;
