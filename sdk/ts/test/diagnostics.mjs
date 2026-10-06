import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {AlightClient} from '../dist/index.js';
test('diagnostic reads omit operator keys and reject mixed-source nested evidence',async()=>{
  const payload={source:'sim',as_of_utc:'2026-10-05T01:00:00Z',signals:[],regimes:[],observers:[],pairs:[],disagreements:[],expected_observers:[],owned_window_n:0,
    fidelity:{comparisons:[],excluded_unmatched:0,excluded_conflicting:0,limits:['No matched evidence']},backfills:[],alerts:[],limits:[]};
  const seen=[];
  const server=createServer((req,res)=>{seen.push({url:req.url,auth:req.headers.authorization});res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify(payload));});
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const client=new AlightClient({endpoint:`http://127.0.0.1:${server.address().port}`,source:'sim',operatorKey:'local-diagnostics-operator-only'});
  try {
    assert.equal((await client.diagnostics()).source,'sim');assert.equal(seen[0].auth,undefined);assert.equal(seen[0].url,'/v1/diagnostics');
    payload.signals.push({id:'captured-window',source:'live',origin:'live',from_utc:'2026-10-05T00:00:00Z',through_utc:'2026-10-05T00:01:00Z',start_slot:'9007199254740993',end_slot:'9007199254741250',epoch:null,measures:[]});
    await assert.rejects(client.diagnostics(),e=>e.code==='SOURCE_MISMATCH');
  } finally {await new Promise(resolve=>server.close(resolve));}
});
