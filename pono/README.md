# Pono - Solana MEV Detector

This Rust project provides tools for fetching and analyzing Solana blocks, with a focus on MEV (Maximal Extractable Value) detection.

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
- Access to a Solana RPC endpoint

### Build
```bash
cd pono
cargo build --release
```

## Usage

### Pono - MEV Detector

Analyze a specific slot for MEV activity:

```bash
cargo run --bin pono -- <slot_number>
```

Example:
```bash
cargo run --bin pono -- 381165825
```

### Configuration

Set the RPC endpoint via environment variable:
```bash
export SOLANA_RPC_URL="https://your-rpc-endpoint.com"
cargo run --bin pono -- 381165825
```

### JSON Output

Enable JSON output for programmatic processing:
```bash
PONO_JSON=1 cargo run --bin pono -- 381165825
```

## Output

Pono provides detailed information about detected MEV events:

### Arbitrage Events
- Signature
- Signer address
- Number of swaps and transfers
- Compute units consumed
- Fees paid
- Profit tokens (with amounts and mints)
- Programs involved

### Sandwich Attack Events
- Attacker address
- Victim signature
- Front-run transaction details
- Victim transaction details
- Back-run transaction details
- Total compute units and fees

## Example Output

```
🔍 Analyzing slot 381165825 for MEV
📦 Fetching block...
✅ Block fetched:
   Slot: 381165825
   Blockhash: HyxzaYPx3n2BzmC2HJWH8Z7PwKYg4uUeH1pXU3BqapUj
   Total transactions: 1381
   Successful transactions: 1190
   Total fees: 14520000 lamports
   Total compute units: 245837291

🔎 Detecting MEV...

📊 MEV Summary:
   Total MEV events: 31
   Arbitrage: 30
   Sandwich attacks: 1

================================================================================
🔄 ARBITRAGE EVENTS (30 found)
================================================================================

1. Arbitrage #1
   Signature: 1WdpJzEBEmsB7MPuEbHa7jE...
   Signer: AVRYkrTLKzUswx5VGnjJHfY5D3xZqjU5odfXQybAMBUw
   Swaps: 5
   Transfers: 11
   Compute units: 215648
   Fee: 12600 lamports (0.000013 SOL)
   Profits:
     • 158882.167087 tokens
       Mint: 26s3UGB9hund1qspApy1...
       Raw delta: 158882167087
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

Run with verbose logging:
```bash
RUST_LOG=debug cargo run --bin pono -- 381165825
```

Build optimized binary:
```bash
cargo build --release
# Binary will be at: target/release/pono
```

## Performance

The MEV detector is optimized for production use:
- Concurrent transaction analysis
- Minimal memory allocations
- Efficient pattern matching
- LTO and optimizations enabled in release mode

## License

See repository root for license information.
