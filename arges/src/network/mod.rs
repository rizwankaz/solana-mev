/// Network monitoring and state tracking
///
/// Monitors Solana network conditions to enable adaptive MEV detection

pub mod state;
pub mod monitor;

pub use state::{NetworkState, NetworkMetrics, CongestionLevel};
pub use monitor::NetworkMonitor;
