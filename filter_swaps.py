#!/usr/bin/env python3
import json

# Read the block data
with open('block381165825.json', 'r') as f:
    data = json.load(f)

transactions = data['result']['transactions']
swap_count = 0
swap_transactions = []

for idx, tx in enumerate(transactions):
    # Check if transaction was successful (no error)
    meta = tx.get('meta', {})
    is_successful = meta.get('err') is None

    if is_successful:
        # Check for any "Instruction: Swap" variant in log messages
        log_messages = meta.get('logMessages', [])
        # Look for any swap instruction (Swap, SwapV2, Swap2, etc.)
        has_swap = any('Instruction: Swap' in msg for msg in log_messages)

        if has_swap:
            swap_count += 1
            swap_transactions.append({
                'index': idx,
                'signature': tx['transaction']['signatures'][0] if tx['transaction'].get('signatures') else 'N/A',
                'logs': log_messages
            })

print(f"Total successful transactions with 'Instruction: Swap': {swap_count}")
print(f"\nFirst 5 transaction details:")
for tx in swap_transactions[:5]:
    print(f"\n--- Transaction {tx['index']} ---")
    print(f"Signature: {tx['signature']}")
    print("Swap-related logs:")
    for log in tx['logs']:
        if 'Swap' in log or 'Instruction' in log:
            print(f"  {log}")
