# Pono

Pono fetches and analyzes blocks on Solana to detect maximal extractable value (MEV). Besides measuring the actual value of MEV per slot, Pono also measures the amount of compute used and fees spent to make these transactions possible.

This project was largely inspired by [Brontes](https://github.com/SorellaLabs/brontes) built by [Sorella Labs](https://sorellalabs.xyz/), which measures MEV on Ethereum.

This is very much a work in progress, so be advised.

## Components

### 1. Pono Library
Core library for fetching Solana blocks with:
- Block fetching with retry logic
- Rate limiting
- Transaction metadata extraction
- Streaming block data

### 2. Pono Binary
MEV detection tool that analyzes Solana blocks for:
- **Arbitrage transactions**: Multi-hop swaps that exploit price differences across DEXs
- **Sandwich attacks**: Front-run and back-run patterns targeting victim transactions

## Installation

### Prerequisites
- Rust 1.85 or higher
- Access to a Solana RPC endpoint; Pono defaults to [the public Solana RPC](https://api.mainnet-beta.solana.com)

### Build
```bash
cd pono
cargo build --release
```

## Usage

Pono provides several commands for analyzing Solana blocks:

#### Stream
Stream MEV events from the latest slot continuously:
```bash
pono stream
```

This will continuously monitor new blocks and print MEV events as they occur.

#### Slot-specific

**Single slot with full MEV details:**
```bash
pono run 381165825
```

**Slot range with full MEV details:**
```bash
pono run 381165825-381165835
```

**Single slot with slot-wide summary only:**
```bash
pono run slot 381165825
```

**Slot range with slot-wide summaries:**
```bash
pono run slot 381165825-381165835
```

The slot-wide summary mode returns lightweight data (MEV counts, compute units, profit totals) without individual transaction details - useful for quick analysis or large-scale scanning. I'll be using these to provide time-series on MEV.

### Configuration

#### Required: Solana RPC Endpoint
Set the RPC endpoint via environment variable:
```bash
export SOLANA_RPC_URL="https://your-rpc-endpoint.com"
```

#### Price Data: Pyth Benchmarks API (Free)
Pono uses Pyth's free Benchmarks API to fetch historical token prices at the exact block timestamp. This almost certainly should be updated with a better way to price.

## Output

### Full MEV Analysis (`pono run <slot>`)

JSON with complete MEV event details including:

**Block Information:**
- Slot number, blockhash, timestamp
- Transaction counts (total, successful, non-vote)
- Compute unit usage

**Arbitrage Events:**
- Transaction signature and signer
- Swap details and DEX programs used
- Token balance changes
- Profitability breakdown (revenue, fees, net profit)
- Compute units consumed

**Sandwich Attack Events:**
- Front-run and back-run transaction details
- Sandwiched token identification
- Victim transaction information
- Total compute units and fees
- Profitability breakdown

### Slot-Wide Summary (`pono run slot <slot>`)

JSON with aggregate data:
- Block metadata (slot, blockhash, timestamp)
- Transaction counts
- MEV counts (total, arbitrage, sandwich)
- Total MEV compute units
- Total MEV profit in USD

### Stream Output (`pono stream`)

One-liners for each slot with MEV activity:
```
Slot 381165825: 31 MEV txs (30 arb, 1 sandwich) | $1234.56 profit | 2456789 CU
Slot 381165826: 18 MEV txs (17 arb, 1 sandwich) | $892.31 profit | 1234567 CU
```

## MEV Detection Logic

### Arbitrage Detection

Transactions are classified as arbitrage if they:
1. Contain 2+ swap instructions
2. Show net positive token balance for the signer
3. Successfully completed (no errors)

### Sandwich Attack Detection

Sandwich attacks are detected by finding sequences where:
1. Three transactions occur within 5 positions
2. Same signer for transactions 1 and 3 (attacker)
3. Different signer for transaction 2 (victim)
4. All contain swap or transfer instructions

## Project Structure

```
pono/
├── src/
│   ├── lib.rs              # Library exports
│   ├── main.rs             # Demo binary
│   ├── bin/
│   │   └── pono.rs         # Pono MEV detector binary
│   ├── fetcher.rs          # Block fetching logic
│   ├── stream.rs           # Block streaming
│   ├── types.rs            # Core types
│   └── mev.rs              # MEV detection logic
├── Cargo.toml
└── README.md
```

## Development

Run tests:
```bash
cargo test
```

Run with verbose logging to see price fetching details:
```bash
RUST_LOG=pono=debug cargo run --bin pono -- 381165825
```

This will show:
- Benchmarks API HTTP requests for each token
- Historical price values fetched at the exact block timestamp
- Success/failure rate of price fetching
- API errors or network issues

Build optimized binary:
```bash
cargo build --release
# Binary will be at: target/release/pono
```
