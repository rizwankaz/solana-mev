/// Atomic arbitrage detector
///
/// Detects single transactions with multiple swaps across different pools
/// that result in net profit by exploiting price differences.
pub mod atomic_arb;

/// Sandwich attack detector
///
/// Detects front-run + victim + back-run patterns where an attacker
/// manipulates prices around a victim's transaction.
pub mod sandwich;

/// JIT (Just-In-Time) liquidity detector
///
/// Detects add_liquidity → swap → remove_liquidity patterns where
/// a searcher provides concentrated liquidity to capture swap fees.
pub mod jit_liquidity;

/// Liquidation detector
///
/// Detects profitable liquidations of undercollateralized positions
/// on lending protocols.
pub mod liquidation;
