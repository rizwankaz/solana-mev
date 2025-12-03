# Arges - Solana MEV Detection Engine

## Quick Start: Analyze a Specific Slot

To analyze slot **381165825** (or any other slot):

```bash
# Navigate to the arges directory
cd arges

# Run the analyzer on a specific slot
cargo run --example analyze_slot -- 381165825
```

### With Custom RPC Endpoint

For better performance and access to historical data, use a premium RPC:

```bash
# Using QuickNode, Helius, or other RPC provider
SOLANA_RPC_URL=https://your-rpc-endpoint.com cargo run --example analyze_slot -- 381165825

# Example with Helius (requires API key)
SOLANA_RPC_URL=https://mainnet.helius-rpc.com/?api-key=YOUR_KEY cargo run --example analyze_slot -- 381165825
```

## Output

The analyzer will:

1. **Fetch the block data** from the specified slot
2. **Filter transactions** using instruction-based classification (swaps, liquidations, liquidity ops)
3. **Run MEV detection algorithms**:
   - Atomic Arbitrage Detector
   - Sandwich Attack Detector
   - JIT Liquidity Detector
   - Liquidation Detector
4. **Display detailed results** in the terminal
5. **Generate JSON output** saved to `mev_slot_<slot_number>.json`

### Example Output

```
🔍 Analyzing slot 381165825 for MEV transactions...

📡 RPC endpoint: https://api.mainnet-beta.solana.com
⏳ Fetching block data...
✅ Block fetched successfully
   Blockhash: 8x7vK...
   Parent slot: 381165824
   Total transactions: 2847
   Successful transactions: 2819
   Total fees: 0.125 SOL
   Block time: 2025-12-03 10:15:42 UTC

🔬 Running MEV detection algorithms...

MEV Block Summary - Slot 381165825
=====================================
Total Transactions:     2847
Successful Transactions: 2819
MEV Transactions:       12

Breakdown by Type:
- Atomic Arbitrage:     7
- Sandwich Attacks:     3
- JIT Liquidity:        1
- Liquidations:         1

Financial Impact:
- Total MEV Profit:     2458900000 lamports (2.458 SOL)
- Total Victim Loss:    89500000 lamports (0.089 SOL)

💰 MEV Transactions Detected:

1. 🔄 ATOMIC ARBITRAGE
   Signature: 5rK2fN...
   Searcher: 7xP9...
   Profit: 0.856 SOL
   Pools: 3
   Token route: SOL → USDC → RAY → SOL
   ...

================================================================================
JSON OUTPUT - MEV TRANSACTIONS FOR SLOT 381165825
================================================================================
{
  "slot": 381165825,
  "block_time": 1733229342,
  "total_transactions": 2847,
  "successful_transactions": 2819,
  "mev_transactions": [...],
  "stats": {...}
}
================================================================================

💾 JSON saved to: mev_slot_381165825.json
```

## Using the JSON Output

The JSON file contains all MEV transaction details in a structured format:

```bash
# Pretty print the JSON
cat mev_slot_381165825.json | jq .

# Extract only atomic arbitrage transactions
cat mev_slot_381165825.json | jq '.mev_transactions[] | select(.AtomicArbitrage != null)'

# Get total MEV profit
cat mev_slot_381165825.json | jq '.stats.total_profit_lamports'

# Count transactions by type
cat mev_slot_381165825.json | jq '.stats'
```

## Analyzing Recent Slots

To analyze a recent slot (public RPC nodes only keep ~500 recent slots):

```bash
# Get current slot
solana slot

# Analyze a slot from 10 blocks ago
cargo run --example analyze_slot -- $(solana slot --url https://api.mainnet-beta.solana.com | awk '{print $1-10}')
```

## Archive Data Access

For historical slots like 381165825 (which may be pruned):

1. **Use an archive RPC node**:
   - [Helius Archive](https://helius.xyz)
   - [QuickNode Archive](https://www.quicknode.com/)
   - [Triton One](https://triton.one/)

2. **Run your own archive node** (requires significant storage)

Example with archive node:
```bash
SOLANA_RPC_URL=https://your-archive-node.com cargo run --example analyze_slot -- 381165825
```

## Batch Analysis

To analyze multiple slots, modify the example or use a shell loop:

```bash
# Analyze a range of slots
for slot in {381165825..381165835}; do
  echo "Analyzing slot $slot..."
  cargo run --example analyze_slot -- $slot
done
```

## Integration into Applications

The analyzer can be used as a library:

```rust
use arges::{BlockFetcher, MevAnalyzer, FetcherConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = FetcherConfig {
        rpc_url: "https://api.mainnet-beta.solana.com".to_string(),
        max_retries: 3,
        retry_delay_ms: 1000,
        rate_limit: 5,
        timeout_secs: 30,
    };

    let fetcher = BlockFetcher::new(config);
    let block = fetcher.fetch_block(381165825).await?;
    let mev_summary = MevAnalyzer::analyze_block(&block);

    // Convert to JSON
    let json = MevAnalyzer::to_json(&mev_summary)?;
    println!("{}", json);

    Ok(())
}
```

## Performance Tips

1. **Use rate limiting** to avoid RPC throttling
2. **Archive nodes** are slower but have full history
3. **Local RPC** provides fastest access
4. **Parallel analysis** - process multiple slots concurrently (be mindful of rate limits)

## Troubleshooting

### "Block not available"
- Slot may not exist (no block produced)
- Data may be pruned from RPC (use archive node)
- RPC connection issues

### Rate limiting
- Add delays between requests
- Use premium RPC with higher limits
- Reduce the rate_limit in FetcherConfig

### Slow analysis
- Normal for blocks with many transactions
- Archive nodes are slower than current nodes
- Consider using faster RPC provider

## Detection Methodology

The engine uses instruction-based MEV detection:

1. **Pre-filtering** - Filter transactions by type (95% performance improvement)
   - Swap detection via instruction discriminators
   - Liquidation detection via protocol patterns
   - Liquidity operation detection via instruction names

2. **MEV Detection Algorithms**
   - **Atomic Arbitrage**: Circular token routes with net positive profit
   - **Sandwich Attacks**: Front-run + victim + back-run pattern with common pools
   - **JIT Liquidity**: Add → Victim swap → Remove within same block
   - **Liquidations**: Profitable debt repayment + collateral seizure

See the [README](README.md) for detailed methodology based on Brontes and Sandwiched.me research.
