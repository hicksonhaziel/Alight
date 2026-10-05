#!/usr/bin/env python3
"""Actual offline daemon -> Rust/TS SDK -> CLI acceptance, using only temporary Sim databases."""
import contextlib, datetime, json, os, pathlib, subprocess, tempfile, threading, time, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
ROOT=pathlib.Path(__file__).resolve().parents[1]
TARGET=pathlib.Path(os.environ.get('CARGO_TARGET_DIR',str(ROOT/'target')))
if not TARGET.is_absolute(): TARGET=ROOT/TARGET
BIN=TARGET/'debug'

def command(args, codes=(0,)):
    p=subprocess.run([str(a) for a in args],cwd=ROOT,capture_output=True,text=True,timeout=45)
    if p.returncode not in codes: raise AssertionError(f'Command failed ({p.returncode}): {args[0]}: {p.stderr[-1000:]}')
    return p

def get(endpoint,path):
    with urllib.request.urlopen(endpoint+path,timeout=10) as r: return json.load(r)

@contextlib.contextmanager
def server(directory,name,key):
    env={**os.environ,'SOLAMI_RPC_URL':'invalid','SOLAMI_GRPC_URL':'invalid','ALIGHT_CANARY_KEYPAIR':'invalid-signing-key'}
    p=subprocess.Popen([str(BIN/'alightd'),'--mode','sim','--sim-canaries','900','--seed','42','--bind','127.0.0.1:0','--db',str(directory/f'{name}.db'),'--operator-key-file',str(key),'--run-for','90'],cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    try:
        ready=json.loads(p.stdout.readline())
        assert ready['mode']=='sim' and not ready['signing_enabled'] and ready['network_provider_requests']==0
        yield 'http://'+ready['bind']
    finally:
        p.terminate()
        try: p.wait(timeout=10)
        except subprocess.TimeoutExpired: p.kill(); p.wait()
        p.stdout.close(); p.stderr.close()

def cli(endpoint,*args,codes=(0,)):
    return command([BIN/'alight',*args,'--api',endpoint,'--source','sim'],codes)

with tempfile.TemporaryDirectory(prefix='alight-phase3-') as tmp:
    directory=pathlib.Path(tmp); key=directory/'operator.key'; key.write_text('local-test-operator-key-42-only'); key.chmod(0o600)
    results={}
    for name,example in [('rust',[BIN/'examples/quote_then_send']),('typescript',['node',ROOT/'sdk/ts/examples/quote_then_send.mjs'])]:
        with server(directory,name,key) as endpoint:
            output=command([*example,endpoint,key])
            report=json.loads(output.stdout.strip().splitlines()[-1]); assert report['source']=='sim'
            assert report['report']['attempts']==40 and report['report']['resolved']==40 and report['ledger']['verified']
            assert get(endpoint,'/v1/health')['canaries_sent']=='0'
            results[name]={'attempts':40,'resolved':40,'verdict':report['report']['verdict'],'ledger_verified':True}
    with server(directory,'cli',key) as endpoint:
        assert json.loads(cli(endpoint,'doctor').stdout)['status']=='SIMULATED'
        h=get(endpoint,'/v1/health'); curves=get(endpoint,'/v1/curve')
        candidates=[c['config'] for c in curves['curves'] if c['horizon_slots']==4]
        request={'model':{'context':{'source':'sim','regime_id':curves['regime_id'],'region':h['region'],'as_of_utc':h['as_of_utc']},'candidates':candidates,'covariates':{'congestion':0},'leader_class_next':[],'target':{'kind':'probability','target_p':0.5,'horizon_slots':4}},'ttl_s':120,'economics':None,'frozen_model_hash':None}
        quote=directory/'quote.json'; quote.write_text(json.dumps(request))
        assert json.loads(cli(endpoint,'quote','--request',quote).stdout)['quote']['recommendation']
        f=json.loads(cli(endpoint,'quote','--request',quote,'--freeze','--operator-key-file',key).stdout)
        prove=directory/'prove.json';prove.write_text(json.dumps({'request_id':'cli-forty','forecast_hash':f['hash'],'n':40,'seed':'42'}))
        # Authenticated POSTs are rate-limited; this is one acceptance flow, not automatic write retry.
        time.sleep(1.05)
        p=cli(endpoint,'prove','--request',prove,'--operator-key-file',key,codes=(0,1,3));r=json.loads(p.stdout)
        expected={'CONSISTENT':0,'INCONSISTENT':1,'INCONCLUSIVE':3}
        assert p.returncode==expected[r['verdict']] and r['attempts']==40 and r['resolved']==40
        assert json.loads(cli(endpoint,'prove','--id',r['lock']['id'],codes=(0,1,3)).stdout)==r
        assert json.loads(cli(endpoint,'ledger','verify').stdout)['verified']
        export=directory/'export.json';day=f['forecast']['created_at_utc'][:10]
        cli(endpoint,'export','--day',day,'--output',export)
        bundle=json.loads(export.read_text());assert bundle['source']=='sim' and len(bundle['entries'])==1 and bundle['verified_through_sequence']=='1'
        cli(endpoint,'export','--day',day,'--output',export,codes=(2,))
        cli(endpoint,'prove','--request',prove,codes=(2,))
        # Every verdict exit code and tampered-ledger failure checked through the real CLI parser/client.
        class Mock(BaseHTTPRequestHandler):
            verdict='CONSISTENT'
            def do_GET(self):
                if self.path=='/v1/ledger/verify':
                    status=409; body={'source':'sim','code':'LEDGER_INVALID','message':'verification failed'}
                else: status=200;body={**r,'verdict':type(self).verdict}
                self.send_response(status);self.send_header('Content-Type','application/json');self.end_headers();self.wfile.write(json.dumps(body).encode())
            def log_message(self,*args): pass
        fake=ThreadingHTTPServer(('127.0.0.1',0),Mock);thread=threading.Thread(target=fake.serve_forever,daemon=True);thread.start()
        try:
            base=f'http://127.0.0.1:{fake.server_port}'
            for verdict,exit_code in expected.items():
                Mock.verdict=verdict;cli(base,'prove','--id','exit-codes',codes=(exit_code,))
            cli(base,'ledger','verify',codes=(1,))
        finally:fake.shutdown();fake.server_close();thread.join()
        results['cli']={'quote_prove_export_ledger':'PASS','exit_codes':[0,1,2,3],'attempts':40,'resolved':40}
    # The run command launches the actual Sim daemon and returns; no providers/signing keys loaded.
    command([BIN/'alight','run','--mode','sim','--sim-canaries','90','--bind','127.0.0.1:0','--db',directory/'run.db','--run-for','1'])
    command([BIN/'alight','run','--mode','invalid'],codes=(2,))
    print(json.dumps({'status':'PASS','source':'sim','sdk_and_cli':results,'provider_requests':0,'transactions_sent':0},sort_keys=True))
