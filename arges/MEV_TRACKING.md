# MEV Tracking System

## Overview

This system provides comprehensive MEV (Maximal Extractable Value) tracking and analysis for Solana blocks. It detects arbitrage, liquidation, mint, and spam transactions with detailed token-level profit tracking.

## Features

### 1. MEV Categories

- **Arbitrage**: Cross-DEX trades identified by multiple DEX program calls in a single transaction
- **Liquidation**: Lending protocol interactions (MarginFi, Solend, Kamino, Mango)
- **Mint**: New token/NFT creation via Token Program or Metaplex
- **Spam**: Failed MEV attempts (failed arbitrages, mints, liquidations)

### 2. Token-Level Value Tracking

The system now tracks **actual token balance changes** rather than just SOL:

- **Pre/Post Token Balances**: Analyzes SPL token account changes
- **Net Profit Calculation**: Calculates profit per token mint across all accounts
- **Top Gainers**: Shows most profitable tokens for each MEV category

### 3. Supported Protocols

**DEXs (for arbitrage detection):**
- Jupiter V6 & Limit Orders
- Raydium AMM V4 & CPMM
- Orca Whirlpool
- Phoenix

**Lending Protocols (for liquidation detection):**
- MarginFi V2
- Solend
- Kamino Lend
- Mango V4

**Token/NFT Programs (for mint detection):**
- SPL Token Program
- Token-2022
- Metaplex Token Metadata
- Metaplex Core

## Example Output

```
╔═══════════════════════════════════════════════════════════════╗
║                        BLOCK REPORT                           ║
╚═══════════════════════════════════════════════════════════════╝

Slot Number:         380794246
Block Hash:          6ZSNdMWqjxxaiyBcJ2SEUmt2qUTukH2EBafkwEDVFUao
Parent Slot:         380794245
Block Height:        358946266
Timestamp:           2025-11-18 01:42:10 UTC

─────────────────────── TRANSACTIONS ──────────────────────────
Total Transactions:  1100
Successful:          1046
Failed:              54
Total Fees:          0.008457429 SOL
Compute Units:       16,317,327

─────────────────────── MEV ANALYSIS ───────────────────────────
Total MEV Events:    64
Spam/Failed MEV:     8
Net SOL Change:      -0.008457429 SOL

  🔄 Arbitrage:      6 transactions
     • So11...usd: +1.234567
     • EPjF...ump: +0.456789
     • 7vfC...USDT: +0.123456

  🪙 Mints:          58 transactions
     • pump...oken: 1000000.0
     • meme...coin: 500000.0

Programs Involved:
  • Token Program             60 uses
  • Phoenix                   9 uses
  • Jupiter V6                6 uses
  • Raydium AMM V4            1 uses
```

## Terminology

- **Slot Number**: Time-based counter (increments every ~400ms)
- **Block Height**: Count of actual blocks produced (excludes skipped slots)
- **Net SOL Change**: Usually negative due to transaction fees
- **Token Amounts**: Shown in human-readable form (adjusted for decimals)

## Implementation Details

### Token Change Calculation

```rust
/// Calculate token balance changes from pre/post token balances
fn calculate_token_changes(
    pre_token_balances: &[UiTransactionTokenBalance],
    post_token_balances: &[UiTransactionTokenBalance],
) -> Vec<TokenChange>
```

The system:
1. Maps all token accounts by (account_index, mint)
2. Calculates pre/post balance differences
3. Aggregates changes per mint across all accounts
4. Filters out zero changes
5. Returns net profit/loss per token

### Arbitrage Detection Heuristic

A transaction is classified as arbitrage if:
- It involves 2+ different DEX programs (cross-DEX arbitrage), OR
- It involves 1+ DEX programs and shows net positive token balance changes

### Liquidation Detection Heuristic

A transaction is classified as liquidation if:
- It involves a lending protocol program (MarginFi, Solend, Kamino, Mango), OR
- It involves both a lending protocol AND a DEX (liquidator selling collateral)

### Mint Detection Heuristic

A transaction is classified as mint if:
- It involves Token Program or Metaplex programs
- Shows positive token balance changes (new tokens created)

## Limitations

1. **Value in Tokens, Not USD**: Profits are shown in token amounts, not converted to USD
2. **Parsed Transactions Only**: Only works with parsed transaction data (not all RPC providers support this)
3. **Program Registry**: Detection relies on known program IDs; new protocols need to be added
4. **Heuristic-Based**: May misclassify complex transactions

## Future Improvements

- [ ] Add USD value conversion via price oracles
- [ ] Detect sandwich attacks (frontrun + backrun patterns)
- [ ] Track MEV by validator/leader
- [ ] Export data to CSV/JSON for analysis
- [ ] Add ML-based classification for complex MEV patterns
- [ ] Track Jito bundles and tips

## Usage

Set `SOLANA_RPC_URL` to a premium RPC endpoint (Helius, QuickNode, Alchemy) that supports:
- Full transaction history
- Token balance tracking
- Parsed transaction data

```bash
export SOLANA_RPC_URL="https://mainnet.helius-rpc.com/?api-key=YOUR_KEY"
cargo run
```
