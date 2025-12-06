#!/usr/bin/env python3
import json

# Read the block data
with open('block381165825.json', 'r') as f:
    data = json.load(f)

transactions = data['result']['transactions']
swap_transactions = []

for idx, tx in enumerate(transactions):
    # Check if transaction was successful (no error)
    meta = tx.get('meta', {})
    is_successful = meta.get('err') is None

    if is_successful:
        # Check for "Instruction: Swap" or "Instruction: Transfer" in log messages
        log_messages = meta.get('logMessages', [])
        has_swap = any('Instruction: Swap' in msg for msg in log_messages)
        has_transfer = any('Instruction: Transfer' in msg for msg in log_messages)

        if has_swap or has_transfer:
            # Add transaction index for reference
            tx_with_index = {
                'block_index': idx,
                'transaction': tx
            }
            swap_transactions.append(tx_with_index)

# Write to JSON file
output = {
    'block_height': data['result']['blockHeight'],
    'block_time': data['result']['blockTime'],
    'blockhash': data['result']['blockhash'],
    'slot': data['result']['parentSlot'] + 1,
    'total_swap_transactions': len(swap_transactions),
    'transactions': swap_transactions
}

with open('swap_transactions.json', 'w') as f:
    json.dump(output, f, indent=2)

print(f"Extracted {len(swap_transactions)} successful transactions (with Swap or Transfer) to swap_transactions.json")
print(f"File size: {len(json.dumps(output)) / 1024 / 1024:.2f} MB")

# Count breakdown
swap_only = sum(1 for tx in swap_transactions if any('Instruction: Swap' in msg for msg in tx['transaction']['meta'].get('logMessages', [])))
transfer_only = sum(1 for tx in swap_transactions if any('Instruction: Transfer' in msg for msg in tx['transaction']['meta'].get('logMessages', [])) and not any('Instruction: Swap' in msg for msg in tx['transaction']['meta'].get('logMessages', [])))
both = sum(1 for tx in swap_transactions if any('Instruction: Swap' in msg for msg in tx['transaction']['meta'].get('logMessages', [])) and any('Instruction: Transfer' in msg for msg in tx['transaction']['meta'].get('logMessages', [])))

print(f"\nBreakdown:")
print(f"  - Transactions with Swap instructions: {swap_only + both}")
print(f"  - Transactions with Transfer instructions only: {transfer_only}")
print(f"  - Transactions with both Swap and Transfer: {both}")
