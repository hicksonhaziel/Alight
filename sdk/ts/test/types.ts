import {AlightClient,decimalU64,type CanaryConfig,type ProveRequest} from '../src/index.js';
const config:CanaryConfig={route:'beam_http',size_class:'small',cu_limit:25000,cu_price_micro_lamports:decimalU64('1000'),tip_lamports:decimalU64('200000'),tip_tier:'x2',fee_bucket:'local_median'};
const request:ProveRequest={request_id:'test',forecast_hash:'sha256:x',n:40,seed:decimalU64(42n)};
void config; void request; void new AlightClient({endpoint:'http://127.0.0.1:8080',source:'sim'});
// @ts-expect-error lamports are canonical strings, never JS numbers
const wrong:CanaryConfig={...config,tip_lamports:200000};
// @ts-expect-error unrelated source labels must not type-check
const wrongSource=new AlightClient({endpoint:'http://127.0.0.1:8080',source:'test'});
void wrong; void wrongSource;
