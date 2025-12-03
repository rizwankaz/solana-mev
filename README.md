# Solana MEV Detection Engine

A high-performance MEV (Maximal Extractable Value) detection engine for Solana that analyzes raw block data via RPC and outputs JSON-formatted lists of successful MEV transactions.

## Overview

This engine detects four major types of MEV on Solana:
- **Atomic Arbitrage**: Single transactions exploiting price differences across multiple pools
- **Sandwich Attacks**: Front-run and back-run attacks on victim transactions
- **JIT Liquidity**: Just-in-time liquidity provision to capture swap fees
- **Liquidations**: Profitable liquidations of undercollateralized positions

## Methodology

Our implementation is based on industry-leading MEV detection frameworks:

### 1. Brontes Methodology
[Brontes](https://github.com/sorellalabs/brontes) is a blazingly fast blockchain analytics engine specialized in systematic MEV detection. We adopted their approaches for:

- **Atomic Arbitrage Detection**: Identifying single transactions with multiple swaps across different pools that result in net profit
- **Sandwich Attack Detection**: Validating that front-run and back-run transactions share at least one common pool, with victim transactions grouped by EOA
- **JIT Liquidity Detection**: Detecting add_liquidity → large_swap → remove_liquidity patterns on concentrated liquidity AMMs
- **Liquidation Detection**: Calculating profitability using DEX pricing data (revenue - cost - gas)

### 2. Sandwiched.me Methodology
[Sandwiched.me](https://sandwiched.me) tracks sandwich attacks on Solana in real-time. We incorporated their v2 detection techniques for:

- Multi-step victim transaction handling
- Validator behavior analysis patterns
- Sandwich pattern validation criteria

## Architecture

### Core Components

```
arges/
├── src/
│   ├── fetcher.rs          # Block fetching with retry logic and rate limiting
│   ├── stream.rs           # Block streaming for real-time analysis
│   ├── types.rs            # Core data structures for blocks/transactions
│   ├── mev/
│   │   ├── types.rs        # MEV-specific type definitions
│   │   ├── parser.rs       # Transaction parser for swap/pool extraction
│   │   ├── analyzer.rs     # Main MEV analyzer coordinator
│   │   └── detectors/
│   │       ├── atomic_arb.rs      # Atomic arbitrage detector
│   │       ├── sandwich.rs        # Sandwich attack detector
│   │       ├── jit_liquidity.rs   # JIT liquidity detector
│   │       └── liquidation.rs     # Liquidation detector
│   └── main.rs             # Entry point with examples
```

### Detection Algorithms

#### Atomic Arbitrage Detector (`mev/detectors/atomic_arb.rs`)

Detects single transactions with multiple swaps across different pools.

**Detection Criteria:**
1. Transaction must be successful
2. Must have 2+ token transfers
3. Must interact with DEX programs
4. Token route must be circular (e.g., SOL → USDC → RAY → SOL)
5. Net profit after fees must be positive
6. Must involve at least 2 different pools

**Algorithm:**
```rust
for each successful transaction:
    1. Extract token transfers from metadata
    2. Analyze for circular arbitrage pattern
    3. Build token route and identify pools
    4. Calculate net profit (accounting for fees)
    5. If profitable and circular, classify as atomic arb
```

#### Sandwich Attack Detector (`mev/detectors/sandwich.rs`)

Detects front-run + victim + back-run patterns.

**Detection Criteria (Brontes + Sandwiched.me):**
1. Front-run and back-run must share at least one common pool
2. Victim transactions are interleaved between front/back runs
3. Same searcher for front and back runs
4. Back-run is reverse trade of front-run
5. Victim transactions grouped by EOA

**Algorithm:**
```rust
for each DEX transaction (potential frontrun):
    1. Extract searcher address and pools
    2. Look ahead 2-10 transactions for backrun by same searcher
    3. Validate common pools between front and back
    4. Check if backrun reverses the frontrun trade
    5. Extract victim transactions between front/back
    6. Calculate profit and victim losses
```

#### JIT Liquidity Detector (`mev/detectors/jit_liquidity.rs`)

Detects concentrated liquidity provision MEV on CLMMs (Concentrated Liquidity Market Makers).

**Detection Criteria (Brontes):**
1. Add liquidity transaction
2. Large swap by victim
3. Remove liquidity transaction
4. All three involve same pool
5. Add/remove by same searcher
6. Minimal time between add and remove

**Algorithm:**
```rust
for each transaction (potential liquidity add):
    1. Identify liquidity add operations (2 token deposits + LP token)
    2. Look ahead 2-10 transactions for liquidity remove
    3. Validate same searcher and pool
    4. Find victim swap between add and remove
    5. Calculate fees collected and profit
```

**Targeted AMMs:**
- Orca Whirlpools (concentrated liquidity)
- Raydium CLMM
- Meteora DLMM

#### Liquidation Detector (`mev/detectors/liquidation.rs`)

Detects profitable liquidations on lending protocols.

**Detection Criteria (Brontes):**
1. Interaction with lending protocol
2. Multiple token transfers (debt + collateral)
3. Net positive value after gas costs

**Algorithm:**
```rust
for each lending protocol transaction:
    1. Identify protocol (Solend, Mango, Marginfi, Kamino)
    2. Extract liquidator and liquidated user
    3. Classify token transfers:
       - Outflows = debt repaid
       - Inflows = collateral seized
    4. Calculate: profit = collateral_value - debt_value - gas
    5. If profitable, classify as MEV liquidation
```

### Transaction Parser (`mev/parser.rs`)

Extracts MEV-relevant data from Solana transactions:

**Supported DEXs:**
- Raydium V4 & CLMM
- Orca Whirlpool, V1, V2
- Jupiter V4 & V6 (aggregator)
- Meteora DLMM
- Phoenix (orderbook)
- Lifinity (PMM)

**Supported Lending Protocols:**
- Solend
- Mango Markets V3/V4
- Marginfi
- Kamino Finance

**Key Functions:**
- `extract_swaps()`: Parses swap operations from instructions
- `extract_token_transfers()`: Analyzes token balance changes from metadata
- `is_dex_swap()`: Identifies DEX interactions
- `is_lending_interaction()`: Identifies lending protocol usage

## Data Structures

### MevTransaction Enum

```rust
pub enum MevTransaction {
    AtomicArbitrage(AtomicArbitrage),
    Sandwich(Sandwich),
    JitLiquidity(JitLiquidity),
    Liquidation(Liquidation),
}
```

### AtomicArbitrage

```rust
pub struct AtomicArbitrage {
    pub signature: String,
    pub slot: u64,
    pub tx_index: usize,
    pub searcher: String,
    pub swaps: Vec<SwapInfo>,
    pub profit_lamports: i64,
    pub profit_usd: Option<f64>,
    pub compute_units: u64,
    pub fee_lamports: u64,
    pub pools: Vec<String>,
    pub token_route: Vec<String>,  // e.g., ["SOL", "USDC", "RAY", "SOL"]
}
```

### Sandwich

```rust
pub struct Sandwich {
    pub slot: u64,
    pub attacker: String,
    pub frontrun: SandwichTx,
    pub victims: Vec<VictimTx>,
    pub backrun: SandwichTx,
    pub common_pools: Vec<String>,
    pub profit_lamports: i64,
    pub profit_usd: Option<f64>,
    pub victim_loss_lamports: i64,
    pub victim_loss_usd: Option<f64>,
}
```

### JitLiquidity

```rust
pub struct JitLiquidity {
    pub slot: u64,
    pub searcher: String,
    pub add_liquidity: LiquidityTx,
    pub victim_swap: VictimTx,
    pub remove_liquidity: LiquidityTx,
    pub pool: String,
    pub fees_collected_lamports: i64,
    pub fees_collected_usd: Option<f64>,
    pub profit_lamports: i64,
    pub profit_usd: Option<f64>,
}
```

### Liquidation

```rust
pub struct Liquidation {
    pub signature: String,
    pub slot: u64,
    pub tx_index: usize,
    pub liquidator: String,
    pub liquidated_user: String,
    pub protocol: String,  // "Solend", "Mango V4", etc.
    pub debt_repaid: Vec<TokenAmount>,
    pub collateral_seized: Vec<TokenAmount>,
    pub revenue_lamports: i64,
    pub cost_lamports: i64,
    pub profit_lamports: i64,
    pub profit_usd: Option<f64>,
}
```

## Usage

### Environment Setup

```bash
# Optional: Set custom Solana RPC endpoint
export SOLANA_RPC_URL="https://api.mainnet-beta.solana.com"
```

### Running the Engine

```bash
# Build the project
cargo build --release

# Run analysis
cargo run --release
```

### Example Output

```
=== MEV ANALYSIS ===

MEV Block Summary - Slot 287654321
=====================================
Total Transactions:     3891
Successful Transactions: 3654
MEV Transactions:       47

Breakdown by Type:
- Atomic Arbitrage:     23
- Sandwich Attacks:     12
- JIT Liquidity:        8
- Liquidations:         4

Financial Impact:
- Total MEV Profit:     1250000000 lamports (1.25 SOL)
- Total Victim Loss:    89000000 lamports (0.089 SOL)

Top 5 most profitable MEV transactions:
  1. Atomic Arb - Profit: 0.234 SOL - Pools: 3 - Sig: 5Kd7Kx8N9mP2...
  2. Sandwich - Profit: 0.156 SOL - Victims: 2 - Sig: 2Hf9Jx7M8pQ1...
  3. JIT Liquidity - Profit: 0.121 SOL - Pool: whirLbMiicVdio... - Sig: 8Nx5Km3L7qR4...
  4. Liquidation - Profit: 0.098 SOL - Protocol: Solend - Sig: 3Pq8Ln2K9mT5...
  5. Atomic Arb - Profit: 0.087 SOL - Pools: 2 - Sig: 7Rt6Mp4N5jS3...
```

### JSON Output

The engine outputs detailed JSON with all MEV transactions:

```json
{
  "slot": 287654321,
  "timestamp": 1701234567,
  "total_transactions": 3891,
  "successful_transactions": 3654,
  "mev_transactions": [
    {
      "mev_type": "atomic_arbitrage",
      "signature": "5Kd7Kx8N9mP2...",
      "slot": 287654321,
      "tx_index": 45,
      "searcher": "7xKXtg2CW87d...",
      "swaps": [...],
      "profit_lamports": 234000000,
      "pools": ["675kPX9MHTjS...", "whirLbMiicVd..."],
      "token_route": ["So11111111...", "EPjFWdd5Auf...", "So11111111..."]
    },
    ...
  ],
  "stats": {
    "total_mev_count": 47,
    "atomic_arbitrage_count": 23,
    "sandwich_count": 12,
    "jit_liquidity_count": 8,
    "liquidation_count": 4,
    "total_profit_lamports": 1250000000,
    "total_victim_loss_lamports": 89000000
  }
}
```

## Technical Details

### Block Fetching

The `BlockFetcher` component provides:
- **Retry Logic**: Exponential backoff for failed requests (max 3 retries)
- **Rate Limiting**: Token bucket rate limiter (configurable requests/second)
- **Parallel Fetching**: Concurrent block fetching with configurable concurrency
- **Error Handling**: Graceful handling of skipped slots and network errors

```rust
pub struct FetcherConfig {
    pub rpc_url: String,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub rate_limit: u32,
    pub timeout_secs: u64,
}
```

### Performance Optimizations

1. **Token Transfer Analysis**: We primarily use transaction metadata (token balance changes) rather than parsing instructions, which is more reliable and faster
2. **Heuristic Filtering**: Quick pre-filters eliminate non-MEV transactions early
3. **Batch Processing**: Detectors process transactions in batches
4. **Parallel Detection**: Multiple detectors can run concurrently (future optimization)

### Limitations & Future Improvements

**Current Limitations:**
1. **Simplified Swap Parsing**: Full instruction parsing requires program-specific decoders for each DEX
2. **Price Oracles**: USD valuations require integration with price feeds
3. **Pool Address Detection**: Uses heuristics; a comprehensive pool database would improve accuracy
4. **Single-Block Analysis**: Cross-block patterns (like multi-block JIT) not yet detected

**Planned Improvements:**
1. **Program-Specific Parsers**: Dedicated parsers for Raydium, Orca, Jupiter instruction formats
2. **Price Oracle Integration**: Pyth, Switchboard integration for USD calculations
3. **Historical Database**: Track known pools, searchers, and patterns over time
4. **Multi-Block Detection**: Analyze cross-block MEV strategies
5. **Real-time Streaming**: Live MEV monitoring via WebSocket
6. **Statistical Analysis**: MEV trends, searcher profiling, protocol comparisons

## Dependencies

- `solana-client`: RPC client for Solana
- `solana-sdk`: Core Solana types and utilities
- `solana-transaction-status`: Transaction metadata parsing
- `tokio`: Async runtime
- `serde`/`serde_json`: Serialization/deserialization
- `tracing`: Structured logging
- `anyhow`/`thiserror`: Error handling

## References

### Methodologies
- [Brontes MEV Detection](https://book.brontes.xyz/mev_inspectors/intro.html) - Comprehensive MEV inspector methodology
- [Sandwiched.me Research](https://sandwiched.me/research) - Solana sandwich attack detection v2
- [Helius Solana MEV Report](https://www.helius.dev/blog/solana-mev-report) - MEV trends and data

### DEX Documentation
- [Raydium Protocol](https://raydium.io/docs/)
- [Orca Whirlpools](https://docs.orca.so/whirlpools/overview)
- [Jupiter Aggregator](https://docs.jup.ag/)
- [Meteora DLMM](https://docs.meteora.ag/)

### Lending Protocols
- [Solend Protocol](https://docs.solend.fi/)
- [Mango Markets](https://docs.mango.markets/)
- [Marginfi](https://docs.marginfi.com/)
- [Kamino Finance](https://docs.kamino.finance/)

## Contributing

This is a research and educational project. Contributions are welcome!

Areas for contribution:
- Additional DEX parsers
- More sophisticated price estimation
- Cross-block pattern detection
- Performance optimizations
- Test coverage

## License

MIT

## Disclaimer

This tool is for research and educational purposes only. MEV detection and analysis should be used responsibly and in compliance with all applicable regulations.
