#!/usr/bin/env python3
import json
from collections import defaultdict

# Read the swap transactions
with open('swap_transactions.json', 'r') as f:
    data = json.load(f)

transactions = data['transactions']

# Results storage
arbitrage_candidates = []
sandwich_candidates = []

def extract_token_transfers(tx):
    """Extract token transfers from pre/post token balances"""
    meta = tx['transaction']['meta']
    pre_balances = meta.get('preTokenBalances', [])
    post_balances = meta.get('postTokenBalances', [])

    transfers = []

    # Create lookup by account index
    pre_lookup = {b['accountIndex']: b for b in pre_balances}
    post_lookup = {b['accountIndex']: b for b in post_balances}

    # Find all accounts with token balance changes
    all_indices = set(pre_lookup.keys()) | set(post_lookup.keys())

    for idx in all_indices:
        pre = pre_lookup.get(idx, {})
        post = post_lookup.get(idx, {})

        if pre and post:
            pre_amount = int(pre['uiTokenAmount']['amount'])
            post_amount = int(post['uiTokenAmount']['amount'])

            if pre_amount != post_amount:
                transfers.append({
                    'accountIndex': idx,
                    'mint': post.get('mint') or pre.get('mint'),
                    'owner': post.get('owner') or pre.get('owner'),
                    'pre_amount': pre_amount,
                    'post_amount': post_amount,
                    'delta': post_amount - pre_amount,
                    'decimals': post.get('uiTokenAmount', {}).get('decimals', 0)
                })

    return transfers

def count_swap_instructions(tx):
    """Count number of swap instructions in transaction"""
    log_messages = tx['transaction']['meta'].get('logMessages', [])
    return sum(1 for msg in log_messages if 'Instruction: Swap' in msg)

def is_arbitrage_candidate(tx, transfers, swap_count):
    """
    Arbitrage detection heuristics:
    1. Multiple swaps (typically 2+)
    2. Net positive for at least one token
    3. Ideally starts and ends with same token type
    """
    if swap_count < 2:
        return False

    # Group transfers by owner (signer)
    signer = tx['transaction']['transaction']['signatures'][0] if tx['transaction']['transaction'].get('signatures') else None
    if not signer:
        return False

    # Check if any token has net positive change (profit)
    has_profit = False
    for transfer in transfers:
        if transfer['delta'] > 0 and transfer['owner'] == tx['transaction']['transaction']['message']['accountKeys'][0]:
            has_profit = True
            break

    return has_profit and swap_count >= 2

def detect_mev_patterns(transactions):
    """Detect MEV patterns in transactions"""

    print("Analyzing transactions for MEV patterns...")
    print(f"Total swap transactions to analyze: {len(transactions)}\n")

    # Analyze each transaction
    for tx in transactions:
        block_idx = tx['block_index']
        transfers = extract_token_transfers(tx)
        swap_count = count_swap_instructions(tx)

        signature = tx['transaction']['transaction']['signatures'][0] if tx['transaction']['transaction'].get('signatures') else 'N/A'

        # Check for arbitrage
        if is_arbitrage_candidate(tx, transfers, swap_count):
            arbitrage_candidates.append({
                'block_index': block_idx,
                'signature': signature,
                'swap_count': swap_count,
                'transfers': transfers,
                'transaction': tx['transaction']
            })

    # Detect sandwich attacks by looking at transaction sequences
    # Sort by block index to get ordering
    sorted_txs = sorted(transactions, key=lambda x: x['block_index'])

    for i in range(len(sorted_txs) - 2):
        # Look for pattern: swap -> swap -> swap where first and third might be from same signer
        tx1 = sorted_txs[i]
        tx2 = sorted_txs[i + 1]
        tx3 = sorted_txs[i + 2]

        sig1 = tx1['transaction']['transaction']['signatures'][0] if tx1['transaction']['transaction'].get('signatures') else None
        sig2 = tx2['transaction']['transaction']['signatures'][0] if tx2['transaction']['transaction'].get('signatures') else None
        sig3 = tx3['transaction']['transaction']['signatures'][0] if tx3['transaction']['transaction'].get('signatures') else None

        signer1 = tx1['transaction']['transaction']['message']['accountKeys'][0]
        signer2 = tx2['transaction']['transaction']['message']['accountKeys'][0]
        signer3 = tx3['transaction']['transaction']['message']['accountKeys'][0]

        # Potential sandwich: same signer for tx1 and tx3, different signer for tx2
        if signer1 == signer3 and signer1 != signer2:
            # Check if they're consecutive or very close
            if tx3['block_index'] - tx1['block_index'] <= 5:
                sandwich_candidates.append({
                    'front_run': {'block_index': tx1['block_index'], 'signature': sig1, 'signer': signer1},
                    'victim': {'block_index': tx2['block_index'], 'signature': sig2, 'signer': signer2},
                    'back_run': {'block_index': tx3['block_index'], 'signature': sig3, 'signer': signer3},
                    'transactions': [tx1['transaction'], tx2['transaction'], tx3['transaction']]
                })

    return arbitrage_candidates, sandwich_candidates

# Run detection
arbitrages, sandwiches = detect_mev_patterns(transactions)

print("=" * 80)
print("MEV DETECTION RESULTS")
print("=" * 80)
print(f"\n📊 ARBITRAGE CANDIDATES: {len(arbitrages)}")
print(f"💰 SANDWICH ATTACK CANDIDATES: {len(sandwiches)}")
print("\n" + "=" * 80)

if arbitrages:
    print("\n🔄 ARBITRAGE TRANSACTIONS:")
    print("-" * 80)
    for i, arb in enumerate(arbitrages[:10], 1):  # Show first 10
        print(f"\n{i}. Block Index: {arb['block_index']}")
        print(f"   Signature: {arb['signature'][:20]}...")
        print(f"   Swap Count: {arb['swap_count']}")
        print(f"   Token Transfers: {len(arb['transfers'])}")

        # Show profitable transfers
        profits = [t for t in arb['transfers'] if t['delta'] > 0]
        if profits:
            print(f"   Profits:")
            for p in profits[:3]:
                amount = p['delta'] / (10 ** p['decimals'])
                print(f"     - {amount:,.6f} (mint: {p['mint'][:20]}...)")

    if len(arbitrages) > 10:
        print(f"\n   ... and {len(arbitrages) - 10} more")

if sandwiches:
    print("\n\n🥪 SANDWICH ATTACK CANDIDATES:")
    print("-" * 80)
    for i, sandwich in enumerate(sandwiches[:5], 1):  # Show first 5
        print(f"\n{i}. Sandwich Pattern:")
        print(f"   Front-run (idx {sandwich['front_run']['block_index']}): {sandwich['front_run']['signature'][:20]}...")
        print(f"   Victim   (idx {sandwich['victim']['block_index']}): {sandwich['victim']['signature'][:20]}...")
        print(f"   Back-run (idx {sandwich['back_run']['block_index']}): {sandwich['back_run']['signature'][:20]}...")
        print(f"   Attacker: {sandwich['front_run']['signer'][:20]}...")

    if len(sandwiches) > 5:
        print(f"\n   ... and {len(sandwiches) - 5} more")

# Save results to JSON files
output = {
    'block_height': data['block_height'],
    'block_time': data['block_time'],
    'blockhash': data['blockhash'],
    'analysis_summary': {
        'total_swap_transactions': len(transactions),
        'arbitrage_candidates': len(arbitrages),
        'sandwich_candidates': len(sandwiches)
    },
    'arbitrages': arbitrages,
    'sandwiches': sandwiches
}

with open('mev_analysis.json', 'w') as f:
    json.dump(output, f, indent=2)

print("\n" + "=" * 80)
print(f"✅ Full analysis saved to mev_analysis.json")
print("=" * 80)
