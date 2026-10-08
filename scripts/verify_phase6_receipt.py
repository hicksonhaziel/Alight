#!/usr/bin/env python3
"""Verify Phase 6's offline receipt and its publishing Git snapshot; live gates stay pending."""
import hashlib
import json
from pathlib import Path
import subprocess

root=Path(__file__).resolve().parents[1]
name='data/phase-6-validation.json'
receipt=(root/name).read_bytes()
r=json.loads(receipt)
published=subprocess.check_output(['git','log','-1','--format=%H','--',name],cwd=root,text=True).strip()
if published and subprocess.check_output(['git','show',f'{published}:{name}'],cwd=root)!=receipt:published=''

assert r['schema_version']==1 and r['phase']==6
assert r['status']=='OFFLINE_PASS_LIVE_PENDING' and r['phase_complete'] is False
assert set(r['tasks'])=={f'6.{i}' for i in range(1,9)}
assert all(t['offline']=='PASS' for t in r['tasks'].values())
assert r['tasks']['6.1']['provider_history_acceptance']=='PENDING'
assert r['tasks']['6.4']['mainnet_acceptance']=='PENDING_NO_SEND_AUTHORIZATION'
assert r['tasks']['6.5']['second_person_acceptance']=='PENDING'
assert r['tasks']['6.8']['fresh_read_key_observe_acceptance']=='PENDING_COLLECTION_PAUSED'
assert r['runtime']['collection_resumed'] is False
assert r['runtime']['owned_live_transactions_sent']==0
assert r['runtime']['mainnet_memo_transactions_sent']==0
assert r['runtime']['provider_api_requests']==0
assert r['runtime']['original_live_database_modified'] is False
assert r['wallet_replay']['source']=='replay' and r['wallet_replay']['transactions']==1
assert r['wallet_replay']['compared_transactions']==0 and r['wallet_replay']['complete_wallet_history'] is False
assert r['dataset']['source']=='sim' and r['dataset']['formats']==['csv','parquet']
assert r['dataset']['canaries']==243 and r['dataset']['ledger_sequence']=='2'
assert r['anchors']['signing_enabled'] is False and r['anchors']['signature'] is None
assert r['checks']['rust_workspace_checkpoint']=='PASS'
assert r['checks']['receipt_regressions']=={'passed':6,'failed':0}
assert r['checks']['dataset_regressions']=={'passed':4,'failed':0}
assert r['checks']['judge_regressions']=={'passed':2,'failed':0}
assert r['checks']['browser']=={'passed':8,'failed':0,'scope':'Seven existing workbench tests plus the Phase 6 test, across two system-Chrome runs'}
assert all(r['checks'][k]=='PASS' for k in ('format','all_target_clippy','focused_clippy','sdk_lint','sdk_generator','web_lint','web_build','openapi_snapshot','phase6_contracts','developer_regression','repository_scan','dataset_verification','static_proxy'))
assert r['demo']['script_seconds']==160 and r['demo']['full_narrated_recording']=='PENDING'
assert len(r['code_sha256'])>=30
for path,digest in r['code_sha256'].items():
    assert not path.startswith('/') and '..' not in Path(path).parts
    assert path not in {'ALIGHT_PLAN.md','ALIGHT_BUILD.md','context.txt','.env'}
    data=subprocess.check_output(['git','show',f'{published}:{path}'],cwd=root) if published else (root/path).read_bytes()
    assert hashlib.sha256(data).hexdigest()==digest,path
print(f"PASS: Phase 6 offline receipt and {len(r['code_sha256'])} publishing file hashes; Phase 6 remains incomplete.")
