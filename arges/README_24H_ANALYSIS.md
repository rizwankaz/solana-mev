# 24-Hour MEV Analysis

This tool analyzes MEV (Maximum Extractable Value) on Solana over the past 24 hours and generates plots.

## Features

- **Historical Analysis**: Analyzes up to 216,000 slots (24 hours of Solana blocks)
- **Sampling Support**: Configurable sampling rate to balance speed vs. completeness
- **CSV Output**: Generates CSV file with MEV data per slot
- **Automatic Plotting**: Python script to visualize MEV trends
- **Oracle Price Updates**: Fetches fresh pricing data for each block analyzed

## Quick Start

### 1. Build and Run the Analyzer

```bash
cd arges
cargo build --release

# Run with default settings (samples every 100th slot = ~2,160 slots)
cargo run --release

# Or run with custom sample rate (every 10th slot)
SAMPLE_RATE=10 cargo run --release

# To analyze every single slot (takes hours!)
SAMPLE_RATE=1 cargo run --release
```

### 2. Generate Plots

```bash
# Install Python dependencies
pip install pandas matplotlib

# Generate plots
python ../plot_mev.py
```

## Output Files

- **mev_per_slot_24h.csv**: CSV file with MEV data per slot
  - Columns: `slot`, `timestamp`, `mev_events`, `total_profit_sol`, `total_profit_lamports`, `arbitrage_count`, `sandwich_count`, `swap_count`

- **mev_analysis_output.txt**: Detailed logs of the analysis

- **mev_analysis_plot.png**: Three plots showing:
  - MEV profit per slot
  - Number of MEV events per slot
  - Breakdown by event type (arbitrage vs sandwich)

- **mev_rolling_average.png**: Rolling average of MEV profit over time

## Configuration

### Sample Rate

The `SAMPLE_RATE` environment variable controls how many slots to analyze:

- `SAMPLE_RATE=1`: Analyze every slot (216,000 slots, ~24+ hours to complete)
- `SAMPLE_RATE=10`: Analyze every 10th slot (21,600 slots, ~2-3 hours)
- `SAMPLE_RATE=100`: Analyze every 100th slot (2,160 slots, ~15-20 minutes) **[DEFAULT]**
- `SAMPLE_RATE=1000`: Analyze every 1000th slot (216 slots, ~2 minutes)

### RPC Endpoint

Set your RPC endpoint for faster/more reliable data:

```bash
export SOLANA_RPC_URL="https://your-rpc-endpoint.com"
```

## Performance Considerations

- **Rate Limiting**: The default RPC endpoint has rate limits. Use a dedicated RPC provider for full analysis.
- **Time Required**: Analyzing all 216,000 slots takes significant time due to:
  - Block fetching
  - MEV detection
  - Price oracle queries (updated per block as requested)
  - Transaction analysis

- **Network Costs**: If using a paid RPC provider, be aware of request costs

## Example Output

```
🚀 MEV Analysis Over 24 Hours - Starting...
📍 Current slot: 380392016
📊 Analyzing slots 380176016 to 380392016 (sample rate: 1/100)
⏱️  This will analyze ~2160 slots
🔧 Initializing pricing oracles...
📝 Output will be written to: mev_per_slot_24h.csv

🔍 Starting analysis...

⏳ Progress: 5% (108/2160 slots)
⏳ Progress: 10% (216/2160 slots)
...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📊 24-HOUR MEV ANALYSIS SUMMARY
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Slots analyzed: 2160
  Slots with MEV: 892 (41.30%)
  Total MEV events: 1247
  Total MEV profit: 2847.5632 SOL (2847563200000 lamports)
  Average per slot: 1.318335 SOL
  Average per MEV slot: 3.192303 SOL
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

✅ Analysis complete!
📁 Results saved to: mev_per_slot_24h.csv
📄 Detailed logs saved to: mev_analysis_output.txt
```

## Notes

- **Oracle Updates**: As requested, the price oracle fetches fresh pricing data for each block analyzed, ensuring accurate MEV profit calculations across the entire time range.

- **Skipped Slots**: Solana occasionally skips slots. These are recorded with zero MEV in the CSV.

- **Failed Analyses**: If a block fails to analyze, it's logged and recorded as zero MEV.

## Troubleshooting

**Issue**: RPC rate limiting errors
- **Solution**: Use a dedicated RPC provider or increase sample rate

**Issue**: Out of memory
- **Solution**: Increase sample rate to analyze fewer slots

**Issue**: Slow analysis
- **Solution**: This is expected for large datasets. Use higher sample rates for faster results.
