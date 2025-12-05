# Swap Transaction Analysis for Block 381165825

## Summary

- **Total transactions in block**: 1,381
- **Total successful transactions**: 1,190 (no errors)
- **Successful transactions with "Instruction: Swap"**: **50**

## Swap Instruction Breakdown

From the block's log messages, here are all the swap-related instructions found:

| Instruction Type | Count |
|------------------|-------|
| Instruction: Swap | 153 |
| Instruction: SwapV2 | 77 |
| Instruction: Swap2 | 13 |
| Instruction: SwapBaseInput | 4 |
| Instruction: SwapExactOut | 1 |
| Instruction: SwapRaydiumClmm | 1 |
| Instruction: SwapRouteV2 | 1 |
| Instruction: SwapTob | 1 |

**Note**: The count of 50 successful transactions containing "Instruction: Swap" represents unique transactions. A single transaction may contain multiple swap instructions in its logs, which is why the total instruction count (153) is higher than the transaction count (50).

## Filter Criteria

Transactions were filtered based on:
1. **Success**: Transaction must have `meta.err === null` (no errors)
2. **Swap Instruction**: Transaction logs must contain "Program log: Instruction: Swap" (exact match, excluding variants like SwapV2, Swap2, etc.)
