//! Arges - Solana Block Streaming and MEV Detection
//!
//! A comprehensive toolkit for streaming Solana blocks and detecting MEV (Maximal Extractable Value).
//!
//! ## Features
//!
//! - **Block Streaming**: Robust block fetching with retry logic and rate limiting
//! - **MEV Detection**: Detect arbitrage, sandwich attacks, liquidations, and JIT liquidity
//! - **DEX Parsing**: Parse swaps from major Solana DEXs (Raydium, Orca, Jupiter, Phoenix, etc.)
//! - **Adaptive Thresholds**: Network-aware threshold adjustment
//! - **Jito Integration**: Bundle detection and tip tracking
//! - **Network Monitoring**: Real-time congestion and fee analysis
//! - **Metrics Aggregation**: Slot-level and epoch-level MEV metrics
//!
//! ## Example - Basic MEV Detection
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
//!
//! ## Example - Adaptive MEV Detection with Network Monitoring
//!
//! ```no_run
//! use arges::{BlockFetcher, FetcherConfig, AdaptiveMevDetector, NetworkMonitor};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let fetcher = Arc::new(BlockFetcher::new(FetcherConfig::default()));
//!     let network_monitor = Arc::new(NetworkMonitor::default());
//!     let mut detector = AdaptiveMevDetector::with_monitor(network_monitor);
//!
//!     let slot = fetcher.get_current_slot().await?;
//!     let block = fetcher.fetch_block(slot).await?;
//!
//!     // Analyze with adaptive thresholds and Jito detection
//!     let analysis = detector.detect_block(&block)?;
//!
//!     println!("{}", analysis.summary());
//!     println!("Jito bundles: {}", analysis.jito_bundle_count());
//!     println!("Total tips: {} SOL", analysis.total_jito_tips() as f64 / 1e9);
//!
//!     Ok(())
//! }
//! ```

pub mod fetcher;
pub mod stream;
pub mod types;
pub mod dex;
pub mod mev;
pub mod jito;
pub mod network;

// Re-export commonly used types
pub use fetcher::BlockFetcher;
pub use stream::BlockStream;
pub use types::{FetchedBlock, FetchedTransaction, FetcherConfig, FetcherError};

pub use dex::{DexParser, DexProtocol, ParsedSwap};

pub use mev::{
    MevDetector, MevEvent, MevType, MevClassifier,
    SlotMevMetrics, EpochMevMetrics,
    ArbitrageMetadata, SandwichMetadata, LiquidationMetadata, JitMetadata,
    AdaptiveMevDetector, EnhancedMevAnalysis,
};

pub use jito::{BundleDetector, JitoBundle, TipTracker, TipPayment};

pub use network::{NetworkMonitor, NetworkState, NetworkMetrics, CongestionLevel};
