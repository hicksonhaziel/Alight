#!/usr/bin/env python3
"""Verify the historical Phase 2 foundation receipt and its redistributable fixture."""
import hashlib
import json
from datetime import datetime
from pathlib import Path

root = Path(__file__).resolve().parents[1]
receipt = json.loads((root / 'data/phase-2-foundation-validation.json').read_text())
assert receipt['scope'] == 'model-and-simulator-foundation'
assert receipt['phase_complete'] is False
methodology = receipt['methodology']
registered_v1 = (root / methodology['path']).read_bytes().split(b'\n## Prospective amendment v2', 1)[0]
digest = hashlib.sha256(registered_v1).hexdigest()
assert digest == methodology['sha256']
assert datetime.fromisoformat(methodology['registered_at_utc']) < datetime.fromisoformat(receipt['validated_at_utc'])
assert methodology['live_signal_reports'] == 0
assert receipt['checks']['cargo_test_workspace'] == 'PASS'
assert receipt['checks']['tests_passed'] == 40
sim = receipt['simulation']
assert sim['source'] == 'sim' and sim['canaries'] == 8100
assert sim['curve_snapshots'] == sim['stored_rows'] == sim['measured'] == 243
assert sim['replay_bytes_equal'] and sim['sqlite_quick_check'] == 'ok'
assert sim['transactions_sent'] == sim['network_requests'] == 0
results = receipt['registered_m0_benchmarks']
assert {(r['seed'], r['shifted']) for r in results} == {(s, v) for s in (7, 42, 2026) for v in (False, True)}
assert len(results) == 6
for result in results:
    assert 0 < result['measured_curves'] <= 243
    assert result['threshold'] == 0.08 and 0 <= result['mae'] <= result['threshold']
for fixture in receipt['fixtures']:
    data = (root / fixture['path']).read_bytes()
    assert hashlib.sha256(data).hexdigest() == fixture['sha256']
    row = json.loads(data)
    assert row['methodology_hash'] == 'sha256:' + digest
    assert row['context']['source'] == 'sim'
    assert row['evidence'] == 'MEASURED' and row['n_effective'] >= 30
    assert row['p_interval_95'][0] <= row['p_hat'] <= row['p_interval_95'][1]
assert not receipt['runtime']['resumed_live_collection']
assert not receipt['beam_http']['support_message_sent']
print('PASS: registered methodology, simulator/replay receipt, six M0 benchmarks and curve fixture verified; Phase 2 remains in progress.')
