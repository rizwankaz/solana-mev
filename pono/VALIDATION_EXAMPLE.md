# MEV Validation Report - Example Output

This is the new output format designed for manual validation of MEV detection accuracy.

## Example Output

```
╔═══════════════════════════════════════════════════════════════╗
║                    MEV VALIDATION REPORT                      ║
╚═══════════════════════════════════════════════════════════════╝

Slot Number:         380796443
Block Hash:          FwJeE4gc8XbR3BjCNnJWqqfQyEALfTgtiALdxZVbA2zh
Timestamp:           2025-11-18 01:56:37 UTC
Total Transactions:  1065
MEV Transactions:    75

─────────────────── MEV TRANSACTIONS ──────────────────────────

[1] ✓ ARBITRAGE (tx #42)
Signature: 5ZQvX7MwKZj8h3JNPqV9k4xUQJgYw2tJfKL1NvQw7XpM
Programs: Jupiter V6, Phoenix
Token Changes:
  • EJhqXK...5Guj: +0.000020
  • Cas7md...X4JZ: +0.000007
SOL Change: -0.000005 SOL

[2] ✓ ARBITRAGE (tx #89)
Signature: 3aKvN8zQp4WxJ7YhVn9k2mJzL5tPqR6wDfGx1BvCy8Hs
Programs: Jupiter V6, Raydium AMM V4
Token Changes:
  • So1111...1112: +0.015234
  • EPjFWd...USDC: -15.234000
SOL Change: -0.000008 SOL

[3] ✓ MINT (tx #156)
Signature: 7kBvC2qZp9WxN4YmJz3hR5tQwDfPx1ByHs8aKv1LvMnX
Programs: Token Program
Token Changes:
  • pump...FG2k: +1000000.000000
SOL Change: -0.000002 SOL

[4] ✗ SPAM (tx #234)
Signature: 2mQwX5zKp1WvJ7YhZn4k9mJxL8tNqP6wCfFx3BvDy9Gt
Programs: Jupiter V6
Token Changes:
  (none)
SOL Change: -0.000005 SOL

[5] ✓ LIQUIDATION (tx #567)
Signature: 9pLvB4qXp8WxM2YnJz5hT7tRwFfQx3ByKs1aKv9LvPnY
Programs: MarginFi V2, Jupiter V6
Token Changes:
  • So1111...1112: +2.500000
  • EPjFWd...USDC: -2500.000000
SOL Change: -0.000015 SOL

...

═══════════════════════════════════════════════════════════════
```

## How to Validate

For each MEV transaction listed:

1. **Copy the Signature**
   - Example: `5ZQvX7MwKZj8h3JNPqV9k4xUQJgYw2tJfKL1NvQw7XpM`

2. **Look it up on Solana Explorer**
   - Go to: https://explorer.solana.com/tx/SIGNATURE
   - Or use Solscan: https://solscan.io/tx/SIGNATURE

3. **Verify the Detection**
   - Check that the programs match (e.g., Jupiter V6, Phoenix)
   - Verify token changes are correct
   - Confirm the category makes sense:
     * **ARBITRAGE**: Multiple DEX interactions
     * **LIQUIDATION**: Lending protocol + DEX
     * **MINT**: New tokens created
     * **SPAM**: Failed transaction

4. **Report Issues**
   - If a transaction is miscategorized
   - If token amounts are incorrect
   - If the wrong programs are listed

## Example Validation

### Arbitrage Transaction
```
[1] ✓ ARBITRAGE (tx #42)
Signature: 5ZQvX7MwKZj8h3JNPqV9k4xUQJgYw2tJfKL1NvQw7XpM
Programs: Jupiter V6, Phoenix
Token Changes:
  • EJhqXK...5Guj: +0.000020
  • Cas7md...X4JZ: +0.000007
```

**What to check on Solscan:**
- ✅ Transaction uses Jupiter V6 program
- ✅ Transaction uses Phoenix DEX program
- ✅ Net token balances show profit in those two tokens
- ✅ Transaction succeeded

### Liquidation Transaction
```
[5] ✓ LIQUIDATION (tx #567)
Signature: 9pLvB4qXp8WxM2YnJz5hT7tRwFfQx3ByKs1aKv9LvPnY
Programs: MarginFi V2, Jupiter V6
Token Changes:
  • So1111...1112: +2.500000
  • EPjFWd...USDC: -2500.000000
```

**What to check:**
- ✅ Transaction involves MarginFi lending protocol
- ✅ Transaction also uses Jupiter for swapping collateral
- ✅ User gained SOL and lost USDC (sold collateral)
- ✅ Fits liquidation pattern

## Common Patterns

### Successful Arbitrage
- Status: ✓
- Category: ARBITRAGE
- Programs: 2+ DEX programs (Jupiter, Raydium, Orca, Phoenix)
- Token Changes: Net positive in at least one token
- SOL Change: Usually negative (fees)

### Failed Arbitrage (Spam)
- Status: ✗
- Category: SPAM
- Programs: DEX programs
- Token Changes: None or minimal
- SOL Change: Negative (lost fees only)

### Token Mint
- Status: ✓
- Category: MINT
- Programs: Token Program or Metaplex
- Token Changes: Large positive amount of new token
- SOL Change: Small negative (fees)

### Liquidation
- Status: ✓
- Category: LIQUIDATION
- Programs: Lending protocol + often a DEX
- Token Changes: Collateral sold, debt repaid
- SOL Change: Variable

## Notes

- **✓** = Transaction succeeded
- **✗** = Transaction failed
- Token addresses are truncated for readability (first 6 + last 4 chars)
- SOL Change is usually negative due to transaction fees
- Positive token changes indicate profit/gains
- Negative token changes indicate costs/losses
