#!/usr/bin/env python3
"""Check the final Phase 3 receipt and bound published code provenance without rerunning tests."""
import hashlib, json, subprocess
from pathlib import Path
root=Path(__file__).resolve().parents[1]
receipt=root/'data/phase-3-validation.json'
r=json.loads(receipt.read_text())
# A historical receipt verifies its published Git snapshot after later phases change code.
published=subprocess.run(['git','log','-1','--format=%H','--','data/phase-3-validation.json'],cwd=root,capture_output=True,text=True,check=True).stdout.strip()
if published:
    old=subprocess.run(['git','show',f'{published}:data/phase-3-validation.json'],cwd=root,capture_output=True,check=True).stdout
    if old!=receipt.read_bytes(): published=''
assert r['schema_version']==1 and r['phase']==3
assert r['status']=='OFFLINE_PASS_LIVE_PENDING'
assert len(r['tasks'])==10 and all(v['offline']=='PASS' for v in r['tasks'].values())
assert r['tasks']['3.7']['funded_live_n40']=='PENDING'
assert r['live_exit_condition']=='PENDING'
assert r['runtime']['collection_resumed'] is False
assert r['runtime']['owned_live_transactions_sent']==0
assert r['checks']['rust_workspace']['failed']==0
assert r['checks']['typescript_tests']['passed']==3
assert r['checks']['developer_flow']['status']=='PASS'
assert r['checks']['clean_checkout']['status']=='PASS'
assert r['checks']['alerts']['rules_fired_once']==5
assert r['checks']['all_target_clippy']=='PASS' and r['checks']['format']=='PASS'
for path,digest in r['code_sha256'].items():
    assert not path.startswith('/') and '..' not in Path(path).parts
    assert path not in {'ALIGHT_PLAN.md','ALIGHT_BUILD.md','context.txt','.env','README.md'}
    data=(subprocess.run(['git','show',f'{published}:{path}'],cwd=root,capture_output=True,check=True).stdout if published else (root/path).read_bytes())
    assert hashlib.sha256(data).hexdigest()==digest,path
print(f"PASS: Phase 3 offline receipt, {len(r['code_sha256'])} file hashes, SDK/CLI/alert evidence; funded live acceptance remains pending.")
