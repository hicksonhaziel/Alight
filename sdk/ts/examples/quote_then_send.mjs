// Sim canary bot. The daemon owns the governor; the SDK never takes a wallet/provider key.
import {readFileSync,statSync} from 'node:fs';
import {AlightClient,decimalU64} from '../dist/index.js';
const [endpoint,keyFile]=process.argv.slice(2);
if(!endpoint||!keyFile||statSync(keyFile).size>258) throw new Error('Usage: node quote_then_send.mjs ENDPOINT OPERATOR_KEY_FILE (sim only)');
const client=new AlightClient({endpoint,source:'sim',operatorKey:readFileSync(keyFile,'utf8').trim()});
const health=await client.health();
const curves=await client.curves();
const candidates=curves.curves.filter(c=>c.horizon_slots===4&&c.config.route==='beam_http'&&c.config.size_class==='small').map(c=>c.config);
if(!candidates.length) throw new Error('No supported Beam HTTP small canary candidates');
const request={model:{context:{source:'sim',regime_id:curves.regime_id,region:health.region,as_of_utc:health.as_of_utc},candidates,covariates:{congestion:0},leader_class_next:[],target:{kind:'probability',target_p:0.5,horizon_slots:4}},ttl_s:120,economics:null,frozen_model_hash:null};
if(!(await client.quote(request)).quote.recommendation) throw new Error('Insufficient canary evidence');
const forecast=await client.freezeQuote(request);
// Save these values before send; retry this hash/id, never the entire quote/send workflow.
console.log(JSON.stringify({source:'sim',forecast_hash:forecast.hash,request_id:'ts-example'}));
const report=await client.prove({request_id:'ts-example',forecast_hash:forecast.hash,n:40,seed:decimalU64('42')});
console.log(JSON.stringify({source:'sim',report,ledger:await client.verifyLedger(),transactions_sent:0}));
