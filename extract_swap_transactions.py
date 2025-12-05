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
        # Check for any "Instruction: Swap" variant in log messages
        log_messages = meta.get('logMessages', [])
        has_swap = any('Instruction: Swap' in msg for msg in log_messages)

        if has_swap:
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

print(f"Extracted {len(swap_transactions)} successful swap transactions to swap_transactions.json")
print(f"File size: {len(json.dumps(output)) / 1024 / 1024:.2f} MB")
