#!/usr/bin/env python3
"""Verify the economics receipt and deterministic historical replay JSON on stdin."""
import hashlib
import json
import math
import re
import sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
receipt = json.loads((root / 'data/phase-3-economics-validation.json').read_text())
assert receipt['schema_version'] == 1
assert receipt['scope'] == 'phase-3-economics-foundation' and not receipt['phase_complete']
assert receipt['implemented_tasks'] == ['3.1', '3.2', '3.3']
assert receipt['remaining_tasks'] == ['3.4', '3.5', '3.6', '3.7', '3.8', '3.9', '3.10']
assert re.fullmatch(r'[0-9a-f]{64}', receipt['validated_economics_source_sha256'])
for fixture in receipt['fixtures']:
    assert hashlib.sha256((root / fixture['path']).read_bytes()).hexdigest() == fixture['sha256']
checks = receipt['checks']
assert all(checks[k] == 'PASS' for k in ('cargo_fmt', 'cargo_clippy_workspace_all_targets_beam',
    'cargo_test_workspace_beam', 'repository_secret_scan', 'phase2_historical_receipts', 'loopback_rest_and_ws_reconnect'))
assert checks['workspace_tests_passed'] == 59 and checks['economics_tests_passed'] == 12
assert checks['repeated_replay_bytes_equal']
golden = receipt['synthetic_optimizer']
assert golden['source'] == 'sim' and not golden['observed_live_savings']
for c in golden['configurations']:
    delay = sum(p * golden['delay_median_bps'][str(slot)] for slot, p in c['masses'])
    expected = ((int(c['tip_lamports']) + int(golden['base_fee_lamports'])) / 1e9 * float(golden['sol_usd'])
        + float(golden['size_usd']) * golden['lambda'] * delay / 10000
        + (1 - c['p_hat']) * float(golden['size_usd']) * golden['edge_bps'] / 10000)
    assert math.isclose(expected, float(c['central_cost_usd']), abs_tol=1e-10)
costs = golden['configurations']
assert min(costs, key=lambda c: float(c['central_cost_usd']))['tip_lamports'] == golden['recommended_tip_lamports']
assert math.isclose(float(costs[0]['central_cost_usd']) - float(costs[2]['central_cost_usd']), float(golden['difference_vs_b1_usd']), abs_tol=1e-10)
runtime = receipt['runtime']
assert not runtime['resumed_live_collection'] and not runtime['live_database_modified']
assert runtime['transactions_signed'] == runtime['transactions_sent'] == 0
raw = sys.stdin.buffer.read(512 * 1024 + 1)
assert len(raw) <= 512 * 1024
assert hashlib.sha256(raw).hexdigest() == receipt['replay']['output_sha256']
replay = json.loads(raw)
assert replay['source'] == 'replay' and replay['historical_capture']
assert replay['network_requests'] == replay['transactions_sent'] == 0
assert replay['rest_pools'] == 3 and replay['rest_trades'] == 9 and replay['minute_candles'] == 10
assert replay['ws_control_frames'] == 1 and replay['ws_swap_frames'] == 11
assert all(s['source'] == 'replay' and s['sparse'] and not s['stale'] for s in replay['snapshots'])
assert any(t['base_reserve'] == '210847950099700846' for t in replay['ws_trades'])
assert sum(t['inner_ix_index'] == -1 for t in replay['ws_trades']) == 2
print('PASS: economics receipt, synthetic arithmetic and exact historical replay verified; Phase 3 remains open.')
