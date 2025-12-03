// Arges - Solana MEV Detection Engine
//
// Public API for detecting MEV transactions on Solana:
// - Atomic Arbitrage
// - Sandwich Attacks
// - JIT Liquidity
// - Liquidations

pub mod fetcher;
pub mod stream;
pub mod types;
pub mod mev;

// Re-export commonly used types
pub use fetcher::BlockFetcher;
pub use stream::BlockStream;
pub use types::{FetcherConfig, FetcherError, FetchedBlock, FetchedTransaction};
pub use mev::{MevAnalyzer, MevBlockSummary, MevTransaction, MevType};
