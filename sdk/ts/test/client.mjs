import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {AlightClient,AlightError,decimalU64,u64BigInt} from '../dist/index.js';
import {schemas} from '../dist/schemas.js';
import {matches} from '../dist/validation.js';

test('decimal u64 preserves full width and rejects lossy/invalid values',()=>{
  const max=decimalU64('18446744073709551615'); assert.equal(u64BigInt(max),18446744073709551615n);
  for(const v of ['01','-1','1.0','18446744073709551616','1'.repeat(100)]) assert.throws(()=>decimalU64(v),AlightError);
  assert(!matches(schemas.DecimalU64,200000,schemas));
  assert(!matches(schemas.DecimalU64,'18446744073709551616',schemas));
  assert(matches(schemas.DecimalU64,max,schemas));
  assert(!matches(schemas.ProveRequest,{forecast_hash:'x',request_id:'test',n:401,seed:'42'},schemas));
});
test('endpoint validation and configuration errors never echo secrets',()=>{
  for(const endpoint of ['http://remote.example','https://user:secret@example.com','https://example.com/?token=secret','https://example.com/path']) assert.throws(()=>new AlightClient({endpoint,source:'sim'}),e=>e.code==='CONFIGURATION'&&!e.message.includes('secret'));
});
test('HTTP source, auth redaction, strict contract and bounded failure handling',async()=>{
  const seen=[]; let response={source:'sim',code:'UNAUTHORIZED',message:'sensitive-provider-message'};
  let status=401;
  const server=createServer((req,res)=>{seen.push({method:req.method,auth:req.headers.authorization});res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify(response));});
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  const endpoint=`http://127.0.0.1:${server.address().port}`;
  const client=new AlightClient({endpoint,source:'sim',operatorKey:'local-test-operator-key-42'});
  try{
    await assert.rejects(client.health(),e=>e.code==='UNAUTHORIZED'&&e.status===401&&!e.message.includes('sensitive'));
    assert.equal(seen[0].auth,undefined);
    response.source='live'; await assert.rejects(client.health(),e=>e.code==='SOURCE_MISMATCH');
    status=200; response={source:'sim'}; await assert.rejects(client.health(),e=>e.code==='CONTRACT');
    status=401;response={source:'sim',code:'UNAUTHORIZED',message:'redacted'};
    await assert.rejects(client.prove({forecast_hash:'sha256:example',request_id:'ts-test',n:40,seed:decimalU64('42')}),e=>e.code==='UNAUTHORIZED');
    assert.equal(seen.at(-1).auth,'Bearer local-test-operator-key-42');
    const before=seen.length;
    await assert.rejects(client.prove({forecast_hash:'x',request_id:'ts-test',n:401,seed:'42'}),e=>e.code==='CONTRACT');
    assert.equal(seen.length,before);
    status=200;response={padding:'x'.repeat(8*1024*1024)};
    await assert.rejects(client.health(),e=>e.code==='RESPONSE_TOO_LARGE');
  }finally{await new Promise(r=>server.close(r));}
});
