# MEV Validation Guide

This guide explains how to manually validate MEV measurements against block explorers.

## Overview

The analyzer generates a detailed validation CSV file (`mev_validation_details.csv`) that contains all information needed to verify MEV events on-chain.

## MEV Categories

Events are categorized into 5 buckets:

### 1. **CEX-DEX Arbitrage**
Price differences between centralized exchanges (Binance, Coinbase) and Solana DEXs.

**Validation checklist:**
- Check the transaction on Solscan
- Verify the swap amounts and tokens
- Compare DEX price vs reported CEX price (use historical data if available)
- Confirm profit calculation

### 2. **Atomic Arbitrage**
Cross-DEX arbitrage within a single transaction (e.g., buy on Raydium, sell on Orca).

**Validation checklist:**
- Check transaction shows multiple swaps
- Verify token path matches (e.g., SOL → USDC → SOL)
- Confirm input vs output amounts
- Check all DEXs are involved as reported
- Verify net profit calculation

### 3. **Sandwich**
Frontrun + victim transaction + backrun pattern.

**Validation checklist:**
- Verify three transactions in sequence
- Check frontrun transaction comes before victim
- Check backrun transaction comes after victim
- Verify all transactions interact with same pool
- Confirm victim's worse execution price
- Calculate actual profit and victim loss

### 4. **JIT (Just-In-Time Liquidity)**
Adding liquidity right before a large swap, earning fees, then removing liquidity.

**Validation checklist:**
- Check add liquidity transaction
- Verify large target swap transaction
- Check remove liquidity transaction
- Confirm all transactions are in same block
- Verify fee earnings calculation

### 5. **JIT Sandwich**
Combination of JIT and sandwich attack (rare, reserved for future detection).

## Validation File Format

The `mev_validation_details.csv` contains the following columns:

| Column | Description |
|--------|-------------|
| `slot` | Solana slot number |
| `timestamp` | Unix timestamp |
| `category` | MEV category (one of 5 buckets above) |
| `mev_type` | Technical MEV type from detector |
| `profit_sol` | Estimated profit in SOL |
| `profit_lamports` | Estimated profit in lamports |
| `confidence` | Confidence score (0-100%) |
| `tx_count` | Number of transactions involved |
| `transactions` | Semicolon-separated transaction signatures |
| `extractor` | Solscan link to extractor's address |
| `solscan_links` | Semicolon-separated Solscan transaction links |
| `details` | Event-specific metadata |

## Step-by-Step Validation

### Example: Validating an Arbitrage Event

1. **Open the CSV file**
   ```bash
   # In Excel, Google Sheets, or any CSV viewer
   open mev_validation_details.csv
   ```

2. **Find the event**
   - Filter by category: "Atomic Arbitrage"
   - Sort by profit_sol to find highest value events
   - Pick an event to validate

3. **Review the details column**
   Example: `DEXs: Raydium,Orca | Path: SOL→USDC→SOL | Hops: 2 | Input: 1000000000 | Output: 1100000000`

   This shows:
   - Started with 1 SOL (1,000,000,000 lamports)
   - Swapped through Raydium and Orca
   - Ended with 1.1 SOL (1,100,000,000 lamports)
   - Net profit: 0.1 SOL

4. **Click the Solscan links**
   - Open each transaction link from the `solscan_links` column
   - Review the swap instructions
   - Check the token balances before/after
   - Verify the DEX programs involved

5. **Verify the calculation**
   - Check input token amount
   - Check output token amount
   - Calculate: `profit = output - input - fees`
   - Compare with reported `profit_sol`

6. **Check the extractor**
   - Click the extractor address link
   - Review their transaction history
   - Look for patterns (many MEV transactions?)

### Example: Validating a Sandwich Attack

1. **Find sandwich event in CSV**
   - Category: "Sandwich"
   - Check the details column for victim info

2. **Review the transactions**
   - Three transaction links in `solscan_links`:
     1. Frontrun (buy before victim)
     2. Victim (target transaction)
     3. Backrun (sell after victim)

3. **Verify the sandwich**
   - Check transaction order in the block
   - Frontrun should buy tokens
   - Victim buys same tokens (pushing price up)
   - Backrun sells tokens at higher price
   - Calculate profit: `(backrun_out - frontrun_in - fees)`

4. **Check victim loss**
   - Compare victim's actual price vs market price
   - Verify victim loss calculation
   - Details column shows: `Victim Loss: X.XXX SOL`

5. **Verify pool and token**
   - All transactions should interact with same pool
   - Token should match across all swaps

## Common Validation Issues

### Issue 1: Profit seems too high
**Possible causes:**
- Oracle price inaccuracy (especially for low-liquidity tokens)
- Failed transaction not filtered out
- Token decimal mismatch

**How to verify:**
- Check on-chain token decimals
- Verify transaction succeeded
- Cross-reference prices with CoinGecko/Jupiter

### Issue 2: Can't find transaction on Solscan
**Possible causes:**
- Very recent transaction (indexing delay)
- Transaction signature incorrect

**How to verify:**
- Wait a few minutes and retry
- Search by slot number instead
- Check if block was produced

### Issue 3: Calculated profit differs from reported
**Possible causes:**
- Price oracle used different rate
- Fee calculation varies
- Slippage or MEV protection triggered

**How to verify:**
- Check exact block timestamp
- Look up historical prices for that timestamp
- Verify all fees (base + priority)

## Automated Validation Scripts

You can create scripts to automate parts of the validation:

```python
import pandas as pd
import requests

def validate_arbitrage(row):
    """Validate an arbitrage event"""
    # Parse transaction signatures
    txs = row['transactions'].split(';')

    # Fetch on-chain data
    for tx in txs:
        # Use Solana RPC to get transaction details
        # Compare with reported values
        pass

    return validation_result

# Load validation data
df = pd.read_csv('mev_validation_details.csv')

# Validate each event
df['validated'] = df.apply(validate_arbitrage, axis=1)
```

## Tips for Efficient Validation

1. **Start with high-value events**
   - Sort by `profit_sol` descending
   - Validate top 10-20 events first

2. **Focus on one category at a time**
   - Master validating arbitrage first
   - Then move to sandwiches, etc.

3. **Use confidence scores**
   - Events with confidence < 70% need extra scrutiny
   - High confidence (>90%) usually accurate

4. **Check extractors**
   - Known MEV bots have consistent patterns
   - New addresses deserve extra validation

5. **Sample across time**
   - Don't just check recent events
   - Validate events from different hours

## Reporting Issues

If you find a validation discrepancy:

1. **Document it**
   - Slot number
   - Event category
   - Reported vs actual profit
   - Supporting evidence (screenshots, calculations)

2. **Check for patterns**
   - Is this issue specific to one token?
   - Does it affect one MEV type?
   - Is it a systematic error?

3. **File an issue**
   - Include validation data
   - Provide reproduction steps
   - Suggest potential fixes

## Summary Statistics

After validation, you can generate summary statistics:

```python
import pandas as pd

df = pd.read_csv('mev_validation_details.csv')

# Group by category
summary = df.groupby('category').agg({
    'profit_sol': ['count', 'sum', 'mean', 'median'],
    'confidence': 'mean'
})

print(summary)
```

This helps identify which categories are:
- Most common
- Most profitable
- Most accurately detected
