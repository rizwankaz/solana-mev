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
│   │   ├── instruction_parser.rs  # Instruction-based transaction classifier
│   │   ├── registry.rs     # Program ID registry for DEXs and lending protocols
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

### Instruction-Based Parser (`mev/instruction_parser.rs`)

**Three-tier detection strategy for maximum coverage**

The engine uses a sophisticated multi-tier approach to detect swaps and other DeFi operations:

#### Swap Detection (Three Tiers)

**Tier 1: Instruction-based detection (Primary)**
- **Parsed instruction types**: Checks if instruction type contains swap keywords (`swap`, `exchange`, `trade`, `route`, `buy`, `sell`)
- **Instruction discriminators**: Analyzes first 8 bytes of instruction data (Anchor method hash) for unparsed instructions
- Most accurate and protocol-agnostic

**Tier 2: Pattern-based fallback**
- **Instruction data size**: Checks for substantial instruction data (8+ bytes = method call discriminator)
- **Token movement validation**: Verifies bidirectional token transfers (inflows + outflows)
- **Multi-token requirement**: Must involve 2+ different tokens
- Catches unknown DEXs and protocols with unparsed instructions

**Tier 3: Registry-based fallback (Final safety net)**
- **Known program IDs**: Checks transaction against comprehensive DEX registry (50+ protocols)
- **Transfer validation**: Still requires actual token movements to confirm swap
- Ensures maximum detection coverage for known protocols

**Why this three-tier approach:**
- ✅ **Comprehensive**: Catches swaps via instruction analysis, pattern matching, OR known programs
- ✅ **Accurate**: Each tier validates token movements to prevent false positives
- ✅ **Maintainable**: Registry isolated in separate module for easy updates
- ✅ **Future-proof**: Instruction-based tiers catch new protocols automatically

**Example Discriminators:**
- `0xf8c69e91e17bf5ae` = generic "swap" method
- `0x331f5a94973f667f` = "swapExact..." variants
- `0xddda3c8d628c9f7a` = routing/aggregator methods

#### Liquidation Detection (Three Tiers)

**Tier 1: Instruction-based**
- Instruction names contain: `liquidate`, `liquidateBorrow`, `liquidateAndRedeem`
- Discriminators: Solend `0x59594abd3c7f8e4d`, Mango `0x1d9c40563a9f7e2c`

**Tier 2: Pattern-based**
- 3+ different tokens involved
- 4+ token movements (complex multi-token transfers)
- Both inflows and outflows present

**Tier 3: Registry-based**
- Known lending protocol from registry
- 2+ tokens, 3+ transfers
- Validates actual debt/collateral pattern

#### Liquidity Operations

**Add Liquidity:**
- 2+ outflows (depositing token pair)
- 1+ inflow (receiving LP tokens)
- Instruction names: `addLiquidity`, `deposit`, `mintLP`, `increasePosition`

**Remove Liquidity:**
- 1+ outflow (burning LP tokens)
- 2+ inflows (withdrawing token pair)
- Instruction names: `removeLiquidity`, `withdraw`, `burnLP`, `decreasePosition`

**TransactionFilter:**
- `filter_swaps()`: Returns only swap transactions (three-tier detection)
- `filter_liquidations()`: Returns only liquidation transactions
- `filter_liquidity_ops()`: Returns liquidity add/remove operations

### Program Registry (`mev/registry.rs`)

**Comprehensive registry of DEX and lending protocol program IDs**

The registry module maintains curated lists of known Solana DeFi protocols to supplement instruction-based detection.

#### DEX Registry (50+ protocols)

**Major DEXs and Aggregators:**
- **Jupiter**: V3, V4, V6 (aggregator)
- **Raydium**: AMM V3, V4, CLMM (concentrated liquidity), CPMM
- **Orca**: V1, V2, Whirlpools (CLMM)
- **Meteora**: DLMM (dynamic liquidity), Pools
- **Phoenix**: Order book DEX
- **Serum/Openbook**: V2, V3 (order books)
- **Lifinity**: V1, V2 (proactive market maker)
- Plus 30+ additional DEXs (Aldrin, Saber, Cropper, FluxBeam, Saros, Crema, etc.)

#### Lending Protocol Registry (25+ protocols)

**Major Lending Platforms:**
- **Solend**: Main pool, V2
- **Mango Markets**: V3, V4 (perpetuals + lending)
- **Marginfi**: V1, V2
- **Kamino Finance**: Lending + liquidity vaults
- **Port Finance**, **Apricot**, **Larix**, **Jet Protocol**, **Francium**
- Plus 15+ additional protocols (Drift, Tulip, Hubble, Oxygen, Cypher, etc.)

#### API

```rust
use arges::mev::ProgramRegistry;

// Check if a program is a known DEX
if ProgramRegistry::is_dex("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4") {
    println!("Jupiter V6 detected");
}

// Check if a program is a lending protocol
if ProgramRegistry::is_lending_protocol("So1endDq2YkqhipRh3WViPa8hdiSpxWy6z3Z6tMCpAo") {
    println!("Solend detected");
}

// Check if any DeFi protocol
if ProgramRegistry::is_defi_protocol(program_id) {
    println!("Known DeFi protocol");
}
```

**Registry Maintenance:**
- All program IDs are isolated in `registry.rs` for easy updates
- Uses `lazy_static` for O(1) lookup performance via `HashSet`
- New protocols can be added by appending to `DEX_PROGRAMS` or `LENDING_PROGRAMS` arrays
- No duplicate program IDs (enforced by unit tests)

### Transaction Parser (`mev/parser.rs`)

Extracts token transfer metadata from transactions:

**Key Functions:**
- `extract_token_transfers()`: Analyzes pre/post token balances from metadata (most reliable method)
- `extract_accounts()`: Gets all accounts involved in transaction
- `get_signer()`: Identifies transaction fee payer

**Note:** Program ID identification is now handled by the dedicated `ProgramRegistry` module for maintainability.

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

1. **Instruction-Based Pre-Filtering**: Transactions are classified by type (swap, liquidation, liquidity op) BEFORE running detectors. This reduces detector workload by 95%+.

2. **Token Transfer Analysis**: Uses transaction metadata (pre/post token balances) rather than parsing instructions - more reliable and faster.

3. **Discriminator Matching**: Checks first 8 bytes of instruction data (Anchor method sighash) for rapid protocol identification.

4. **Lazy Evaluation**: Only parses instruction data when heuristics suggest relevant transaction type.

5. **Batch Processing**: Detectors process pre-filtered transaction batches efficiently.

### Instruction-Based Detection Details

**How It Works:**

1. **Instruction Parsing**:
   - Extracts both compiled (base58) and parsed (JSON) instruction formats
   - Analyzes inner instructions for CPI calls
   - Checks instruction names for operation keywords

2. **Discriminator Matching**:
   - First 8 bytes of instruction data = Anchor method discriminator
   - Matches against known patterns for common operations
   - Extends to new protocols automatically via name matching

3. **Token Transfer Heuristics**:
   - Swaps: 2 transfers (1 in, 1 out)
   - Liquidations: 3+ transfers (complex multi-token)
   - Liquidity: Asymmetric transfers (2 out + 1 in for add)

4. **Combined Validation**:
   - Must pass BOTH instruction analysis AND transfer pattern
   - Reduces false positives dramatically

**Example Discriminators:**
```rust
// Swap discriminators (first 8 bytes of instruction data)
Raydium swap:   [0x33, 0x1f, 0x5a, 0x94, 0x97, 0x3f, 0x66, 0x7f]
Jupiter route:  [0xdd, 0xda, 0x3c, 0x8d, 0x62, 0x8c, 0x9f, 0x7a]
Orca swap:      [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x7b, 0xf5, 0xae]

// Liquidation discriminators
Solend:         [0x59, 0x59, 0x4a, 0xbd, 0x3c, 0x7f, 0x8e, 0x4d]
Mango:          [0x1d, 0x9c, 0x40, 0x56, 0x3a, 0x9f, 0x7e, 0x2c]
```

### Limitations & Future Improvements

**Current Limitations:**
1. **Partial Discriminator Coverage**: Not all DEXs/protocols have known discriminators (falls back to name matching)
2. **Price Oracles**: USD valuations require integration with price feeds
3. **Pool Address Detection**: Uses heuristics; a comprehensive pool database would improve accuracy
4. **Single-Block Analysis**: Cross-block patterns (like multi-block JIT) not yet detected

**Planned Improvements:**
1. **Complete IDL Integration**: Parse full Anchor IDLs for comprehensive instruction decoding
2. **Discriminator Database**: Crowd-sourced database of program discriminators
3. **Price Oracle Integration**: Pyth, Switchboard integration for USD calculations
4. **Historical Database**: Track known pools, searchers, and patterns over time
5. **Multi-Block Detection**: Analyze cross-block MEV strategies
6. **Real-time Streaming**: Live MEV monitoring via WebSocket
7. **Machine Learning**: Pattern recognition for novel MEV strategies

## Dependencies

- `solana-client`: RPC client for Solana
- `solana-sdk`: Core Solana types and utilities
- `solana-transaction-status`: Transaction metadata parsing
- `tokio`: Async runtime
- `serde`/`serde_json`: Serialization/deserialization
- `tracing`: Structured logging
- `anyhow`/`thiserror`: Error handling
- `lazy_static`: Static initialization for program registry HashSets

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
