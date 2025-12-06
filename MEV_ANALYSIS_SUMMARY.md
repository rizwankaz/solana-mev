# MEV Analysis Summary - Block 381165825

## Overview

This analysis examines **successful, non-vote swap transactions** in block 381165825 to identify MEV (Maximal Extractable Value) instances, specifically:
- **Arbitrage transactions**
- **Sandwich attacks**

## Results

### Summary Statistics
- **Total swap transactions analyzed**: 71
- **Arbitrage candidates detected**: 30 (42.3%)
- **Sandwich attack candidates detected**: 0

## Arbitrage Detection

### Methodology
Arbitrage transactions are detected using the following heuristics:

1. **Multiple swaps**: Transaction contains 2+ swap instructions
2. **Net profit**: At least one token shows a net positive balance change for the signer
3. **Token cycling**: Typically involves swapping through multiple tokens to exploit price differences across DEXs

### Characteristics of Detected Arbitrages

From the 30 arbitrage candidates:

- **Swap instruction counts**: Range from 2 to 5 swaps per transaction
- **Common patterns**:
  - Multi-hop arbitrage (Token A → Token B → Token C → Token A)
  - Cross-DEX arbitrage exploiting price differences
  - Complex routing through multiple liquidity pools

### Notable Examples

1. **Transaction #11** (5 swaps)
   - Signature: `1WdpJzEBEmsB7MPuEbHa...`
   - Profits: 158,882 tokens + 29.25 tokens + 0.00035 SOL
   - Complex multi-token arbitrage

2. **Transaction #213** (2 swaps)
   - Signature: `2ondd7y51qYPL7XL9dCB...`
   - Profits: 7.8M tokens + 14,660 USDC + 125 USDC
   - Large-scale arbitrage opportunity

3. **Transaction #750** (3 swaps)
   - Signature: `4xavPLhDmn3xcykLkwUH...`
   - Profits: 1.99M tokens + 5,109 USDC + 37.26 SOL
   - Multi-token profit extraction

## Sandwich Attack Detection

### Methodology
Sandwich attacks are detected by looking for the following pattern:

1. **Front-run transaction**: Attacker buys before victim
2. **Victim transaction**: User's swap occurs
3. **Back-run transaction**: Attacker sells after victim

Detection criteria:
- Three consecutive swap transactions within 5 block positions
- Same signer for positions 1 and 3 (attacker)
- Different signer for position 2 (victim)

### Results
**0 sandwich attacks detected** in this block.

This could indicate:
- Low sandwich attack activity in this specific block
- Attacks span across multiple blocks (harder to detect in single-block analysis)
- Different attack patterns used by MEV searchers
- Effective MEV protection mechanisms in place

## Data Files

### Generated Files

1. **`mev_analysis.json`** (958 KB)
   - Complete MEV analysis results
   - Full transaction details for all arbitrage candidates
   - Structured data for further analysis

2. **`swap_transactions.json`** (0.88 MB)
   - All 71 successful swap transactions
   - Full transaction metadata and logs

3. **`detect_mev.py`**
   - MEV detection script
   - Customizable heuristics
   - Extensible for additional MEV patterns

## MEV Extraction Rate

- **MEV transactions**: 30 out of 71 swap transactions (42.3%)
- **Block-level MEV rate**: 30 out of 1,381 total transactions (2.17%)

This suggests significant MEV activity in DeFi swaps on Solana, with nearly half of all swap transactions showing arbitrage characteristics.

## Limitations & Future Improvements

### Current Limitations
1. **Single-block analysis**: Sandwich attacks often span multiple blocks
2. **Heuristic-based**: May produce false positives or miss sophisticated patterns
3. **Token profit detection**: Relies on balance changes, may miss complex profit mechanisms
4. **No price data**: Cannot calculate exact profit in USD terms

### Recommended Improvements
1. **Multi-block analysis**: Examine transaction sequences across blocks
2. **Price oracle integration**: Calculate actual profit in USD
3. **Advanced pattern detection**: Machine learning for MEV pattern recognition
4. **Gas cost analysis**: Factor in transaction fees for net profit calculation
5. **Jito bundle detection**: Identify transactions submitted via MEV-specific infrastructure

## Usage

To run the analysis:

```bash
# Extract swap transactions
python3 extract_swap_transactions.py

# Detect MEV patterns
python3 detect_mev.py
```

To filter and analyze results:

```bash
# View arbitrage transactions
cat mev_analysis.json | jq '.arbitrages[] | {signature, swap_count, profits: .transfers[] | select(.delta > 0)}'

# Count by swap instruction type
cat mev_analysis.json | jq '.arbitrages[] | .swap_count' | sort | uniq -c
```

## Conclusion

This block shows significant MEV activity with **30 arbitrage opportunities** successfully extracted by searchers. The absence of detected sandwich attacks may indicate:

- Different MEV extraction strategies preferred on Solana
- Effective user protection mechanisms
- Need for cross-block analysis to detect sandwich patterns

The 42.3% MEV rate among swap transactions demonstrates the prevalence of automated arbitrage strategies in Solana DeFi markets.
