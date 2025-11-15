//! Arges - Solana Block Streaming and MEV Detection
//!
//! A comprehensive toolkit for streaming Solana blocks and detecting MEV (Maximal Extractable Value).
//!
//! ## Features
//!
//! - **Block Streaming**: Robust block fetching with retry logic and rate limiting
//! - **MEV Detection**: Detect arbitrage, sandwich attacks, liquidations, and JIT liquidity
//! - **DEX Parsing**: Parse swaps from major Solana DEXs (Raydium, Orca, Jupiter, Phoenix, etc.)
//! - **Metrics Aggregation**: Slot-level and epoch-level MEV metrics
//!
//! ## Example
//!
//! ```no_run
//! use arges::{BlockFetcher, FetcherConfig, MevDetector};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Configure and create fetcher
//!     let config = FetcherConfig::default();
//!     let fetcher = Arc::new(BlockFetcher::new(config));
//!
//!     // Create MEV detector
//!     let detector = MevDetector::new();
//!
//!     // Fetch and analyze a block
//!     let slot = fetcher.get_current_slot().await?;
//!     let block = fetcher.fetch_block(slot).await?;
//!     let analysis = detector.detect_block(&block)?;
//!
//!     println!("Found {} MEV events", analysis.events.len());
//!
//!     Ok(())
//! }
//! ```

pub mod fetcher;
pub mod stream;
pub mod types;
pub mod dex;
pub mod mev;

// Re-export commonly used types
pub use fetcher::BlockFetcher;
pub use stream::BlockStream;
pub use types::{FetchedBlock, FetchedTransaction, FetcherConfig, FetcherError};

pub use dex::{DexParser, DexProtocol, ParsedSwap};

pub use mev::{
    MevDetector, MevEvent, MevType, MevClassifier,
    SlotMevMetrics, EpochMevMetrics,
    ArbitrageMetadata, SandwichMetadata, LiquidationMetadata, JitMetadata,
};
