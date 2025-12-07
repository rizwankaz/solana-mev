pub mod types;
pub mod parsers;
pub mod detectors;
pub mod fetcher;
pub mod stream;
pub mod oracle;

// Re-export commonly used types
pub use types::{
    // Block and transaction types
    FetchedBlock, FetchedTransaction, FetcherConfig, FetcherError, Result, Reward,
    // MEV event types
    ArbitrageEvent, MevEvent, Profitability, SandwichEvent, SandwichTransaction,
    // Swap types
    SwapInfo,
    // Token types
    SimpleTokenChange, TokenChange,
};

// Re-export parsers
pub use parsers::SwapParser;

// Re-export detectors
pub use detectors::MevDetector;

// Re-export core services
pub use fetcher::BlockFetcher;
pub use stream::BlockStream;
pub use oracle::OracleClient;
