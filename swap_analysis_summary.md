# Swap and Transfer Transaction Analysis for Block 381165825

## Summary

- **Total transactions in block**: 1,381
- **Total successful transactions**: 1,190 (no errors)
- **Successful transactions with "Instruction: Swap" or "Instruction: Transfer"**: **113**
  - Transactions with Swap instructions: 71
  - Transactions with Transfer instructions only: 42
  - Transactions with both Swap and Transfer: 71
- **Failed transactions with "Instruction: Swap"**: 65
- **Total "Instruction: Swap" instances**: 251 (across all log messages)

## Detailed Breakdown

### Transaction Counts
- **71** unique successful transactions contain at least one swap instruction
- **136** total transactions (successful + failed) contain swap instructions
- **251** total instances of "Instruction: Swap" appearing in log messages

### Why the numbers differ:
- **251 instances** vs **71 transactions**: Many transactions contain multiple swap instructions
- **71 successful** vs **136 total**: We filter out 65 failed transactions

## Swap Instruction Types Found

| Instruction Type | Instances |
|------------------|-----------|
| Instruction: Swap | 153 |
| Instruction: SwapV2 | 77 |
| Instruction: Swap2 | 13 |
| Instruction: SwapBaseInput | 4 |
| Instruction: SwapExactOut | 1 |
| Instruction: SwapRaydiumClmm | 1 |
| Instruction: SwapRouteV2 | 1 |
| Instruction: SwapTob | 1 |
| **Total** | **251** |

## Filter Criteria

Transactions were filtered based on:
1. **Success**: Transaction must have `meta.err === null` (no errors)
2. **Instructions**: Transaction logs must contain either:
   - "Instruction: Swap" (any variant including Swap, SwapV2, Swap2, etc.), OR
   - "Instruction: Transfer" (token transfers)
