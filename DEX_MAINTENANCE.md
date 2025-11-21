# Maintaining Comprehensive DEX Coverage

This document explains strategies for ensuring all Solana DEXes are properly detected.

## Current Status

As of the latest update, we support **19 DEX programs**:
- Jupiter V6, Jupiter Limit Order
- Raydium AMM V4, CPMM, CLMM
- Orca Whirlpools
- Meteora DAMM V2, DLMM, Pools
- Phoenix, Lifinity V2
- TesseraV4, Serum DEX v3
- OpenBook V2, Drift Protocol
- Saber, Marinade Finance, Sanctum
- Pump.fun

## Strategy 1: Monitor Unknown Programs (Automated)

The analyzer now logs unknown programs when it sees token balance changes:

```bash
# Run with debug logging to see unknown programs
RUST_LOG=debug ./target/release/arges <slot>
```

Look for logs like:
```
DEBUG arges::mev: Unknown program in transaction with token changes: XYZ123...
```

Investigate these programs:
1. Check on Solscan: `https://solscan.io/account/<program_id>`
2. If it's a DEX, add it to `arges/src/mev.rs`

## Strategy 2: Compare with Sandwiched.me

Sandwiched.me tracks MEV on Solana. When they report arbitrages we don't detect:

1. Get the transaction signature from sandwiched.me
2. Check the transaction on Solscan
3. Identify which programs were used
4. Add any missing DEX programs to our registry

Example: This is how we discovered Meteora Pools and TesseraV4 were missing.

## Strategy 3: Jupiter's DEX List (Manual Check)

Jupiter aggregates most Solana DEXes. Periodically check their list:

```bash
curl https://public.jupiterapi.com/program-id-to-label
```

Compare against our list in `arges/src/mev.rs` and add any missing ones.

## Strategy 4: Monitor Solana Ecosystem

Follow these resources for new DEX launches:
- [Solana Explorer](https://explorer.solana.com/)
- [DeFiLlama Solana DEXes](https://defillama.com/chain/Solana?category=Dexes)
- [Alchemy DEX List](https://www.alchemy.com/dapps/list-of/decentralized-exchanges-dexs-on-solana)
- Twitter: @JupiterExchange, @Raydium, @orca_so

## Adding a New DEX

When you identify a missing DEX:

1. **Add the constant** in `arges/src/mev.rs`:
   ```rust
   pub const NEW_DEX: &'static str = "ProgramID123...";
   ```

2. **Add to `is_dex()` function**:
   ```rust
   pub fn is_dex(program_id: &str) -> bool {
       matches!(
           program_id,
           // ... existing DEXes
           | Self::NEW_DEX
       )
   }
   ```

3. **Add to `program_name()` function**:
   ```rust
   pub fn program_name(program_id: &str) -> String {
       match program_id {
           // ... existing names
           Self::NEW_DEX => "New DEX Name".to_string(),
           // ...
       }
   }
   ```

4. **Test**: Run against a block that uses this DEX to verify detection works.

## Strategy 5: Periodic Audits

Every few months, run an audit:

1. Analyze 100 random blocks with `RUST_LOG=debug`
2. Collect all "Unknown program" logs
3. Research each unknown program
4. Add legitimate DEXes to the registry

## Known Limitations

Some DEXes may be hard to detect:
- **Private/Dark Pools**: Some proprietary AMMs don't publicize their program IDs
- **New Launches**: Very new DEXes may not be documented yet
- **Custom Implementations**: Some projects build custom swap programs

For these cases, monitor sandwiched.me and on-chain data for patterns.

## Maintenance Schedule

Recommended maintenance:
- **Weekly**: Check sandwiched.me for missed arbitrages
- **Monthly**: Review debug logs for unknown programs
- **Quarterly**: Audit against Jupiter's DEX list
- **As needed**: When new major DEXes launch (announced on Solana Twitter)

## Contact & Resources

- Sandwiched.me: https://sandwiched.me
- Jupiter API: https://docs.jup.ag
- Solscan: https://solscan.io
- DeFiLlama: https://defillama.com/chain/Solana
