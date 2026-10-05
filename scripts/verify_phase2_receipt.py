#!/usr/bin/env python3
"""Verify the historical Phase 2 foundation receipt and its redistributable fixture."""
import hashlib
import json
import math
import re
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
print('PASS: historical foundation receipt, six M0 benchmarks and original curve fixture verified.')

complete_path = root / 'data/phase-2-validation.json'
if complete_path.exists():
    complete = json.loads(complete_path.read_text())
    assert complete['scope'] == 'models-ledger-simulator'
    assert complete['implementation_complete'] and not complete['phase_complete']
    method = complete['methodology']
    assert hashlib.sha256((root / method['path']).read_bytes()).hexdigest() == method['sha256']
    assert datetime.fromisoformat(method['registered_at_utc']) < datetime.fromisoformat(complete['validated_at_utc'])
    assert method['live_signal_reports'] == 0
    assert re.fullmatch(r'[0-9a-f]{64}', complete['validated_rust_source_sha256'])
    checks = complete['checks']
    assert checks['cargo_test_workspace'] == 'PASS' and checks['tests_passed'] == 47
    assert checks['cargo_fmt'] == checks['cargo_clippy_workspace_all_targets'] == checks['repository_secret_scan'] == 'PASS'
    assert checks['forecast_and_frozen_model_tamper_rejected']
    assert checks['p99_sample_requirement_not_capped_at_30']
    sim = complete['simulation']
    assert sim['source'] == 'sim' and sim['canaries'] == 8100
    assert sim['measured_snapshots'] == 243 and sim['stored_curve_history_rows'] >= 243
    assert sim['locked_forecasts'] == sim['graded_forecasts'] == len(sim['forecast_hashes']) == 2
    assert all(re.fullmatch(r'sha256:[0-9a-f]{64}', h) for h in sim['forecast_hashes'] + sim['model_hashes'])
    assert sim['replay_bytes_equal'] and sim['sqlite_quick_check'] == 'ok'
    assert sim['transactions_sent'] == sim['network_requests'] == 0
    assert {m['seed'] for m in complete['m1_heldout']} == {7,42,2026}
    assert all(m['m1_log_loss'] < m['m0_log_loss'] and m['heldout_predictions'] == 6000 for m in complete['m1_heldout'])
    assert {p['seed'] for p in complete['policy_equal_spend']} == {7,42,2026}
    for policy in complete['policy_equal_spend']:
        assert int(policy['synthetic_spend_lamports']) == 2_000_000_000
        assert policy['mean_regret_v1'] < policy['mean_regret_v0']
        for version in ('v0','v1'):
            assert abs(policy['approx_cumulative_regret_'+version] - policy['mean_regret_'+version] * policy['decisions_'+version]) <= 0.051
    comparison = complete['baselines']
    assert {b['id'] for b in comparison['alternatives']} == {'B1','B2','B3','B4'}
    for baseline in comparison['alternatives']:
        score = baseline['scores']; p = baseline['p_hat']; observed = score['observed_rate']
        assert score['n'] > 0
        assert math.isclose(score['brier'], observed * (1-p)**2 + (1-observed)*p**2, abs_tol=1e-12)
        assert math.isclose(score['log_loss'], -observed*math.log(p)-(1-observed)*math.log1p(-p), abs_tol=1e-12)
    b3 = next(b for b in comparison['alternatives'] if b['id'] == 'B3')
    assert b3['tape']['samples'] == 3 and int(b3['tape']['median_lamports']) == 200_000
    b4 = next(b for b in comparison['alternatives'] if b['id'] == 'B4')
    assert re.fullmatch(r'sha256:[0-9a-f]{64}', b4['model_snapshot_hash'])
    latency = complete['latency']
    assert latency['datasets'] == 100 and latency['bootstrap_replicates'] == 2000
    assert all(90 <= latency[k] <= 99 for k in ('covered_p50','covered_p90'))
    assert latency['p99_minimum_effective_tail_samples'] == 20
    assert latency['failure_mass_refuses_unidentifiable_quote']
    signal = complete['signal']
    assert signal['known_slope_verdict'] == 'DISCRIMINATING'
    assert signal['null_verdict'] == 'FLAT' and signal['missing_cohorts_verdict'] == 'INCONCLUSIVE'
    cli = complete['cli']
    assert all(cli[k] == 'PASS' for k in ('probability','latency_slots','latency_ms','empty_live_refuses','cli_grade','cli_signal'))
    assert cli['empty_live_samples_needed'] == 30
    assert complete['runtime']['live_canaries_sent'] == 0
    assert not complete['runtime']['resumed_live_collection'] and not complete['runtime']['live_database_modified']
    assert not complete['jobs']['running'] and len(complete['remaining']) == 2
    container = complete['container']
    assert container['compose_config'] == 'PASS' and container['methodology_in_build_context']
    assert container['cargo_build_offline'] and container['replay_network_disabled']
    assert not container['live_services_started']
    assert container['replay']['slot_events'] == 24 and container['replay']['blocks'] == 8
    assert container['replay']['network_requests'] == container['replay']['canaries_sent'] == 0
    print('PASS: Phase 2 implementation receipt, held-out models, policy, baseline scores, latency, Signal and CLI evidence verified; funded live acceptance remains open.')
