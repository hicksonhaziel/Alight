import { schemas } from './schemas.js';
import { matches } from './validation.js';
import type * as T from './types.js';
export type * from './types.js';
export class AlightError extends Error {
  constructor(public readonly code: string, public readonly status?: number) { super(code); this.name='AlightError'; }
}
export function decimalU64(value: string | bigint): T.DecimalU64 {
  if (typeof value!=='string'&&typeof value!=='bigint') throw new AlightError('INVALID_U64');
  const s=String(value);
  if (!/^(0|[1-9][0-9]{0,19})$/.test(s)||BigInt(s)>18446744073709551615n) throw new AlightError('INVALID_U64');
  return s as T.DecimalU64;
}
export function u64BigInt(value: T.DecimalU64): bigint { return BigInt(decimalU64(value)); }
export interface ClientOptions { endpoint: string; source: T.Source; operatorKey?: string; timeoutMs?: number }
/** Public reads carry no operator key. Writes are never automatically retried. */
export class AlightClient {
  private readonly endpoint: URL;
  private readonly source: T.Source;
  readonly #operatorKey: string | undefined;
  private readonly timeoutMs: number;
  constructor(options: ClientOptions) {
    try { this.endpoint=new URL(options.endpoint); } catch { throw new AlightError('CONFIGURATION'); }
    const u=this.endpoint;
    if (!(u.protocol==='https:'||u.protocol==='http:'&&['localhost','127.0.0.1','[::1]'].includes(u.hostname))||u.username||u.password||u.search||u.hash||u.pathname!=='/'||!['live','sim','replay'].includes(options.source)) throw new AlightError('CONFIGURATION');
    this.source=options.source; this.#operatorKey=options.operatorKey; this.timeoutMs=options.timeoutMs??30000;
    if (!Number.isInteger(this.timeoutMs)||this.timeoutMs<1||this.timeoutMs>120000||this.#operatorKey!==undefined&&!/^[!-~]{16,256}$/.test(this.#operatorKey)) throw new AlightError('CONFIGURATION');
  }
  private scope(source: T.Source): void { if(source!==this.source) throw new AlightError('SOURCE_MISMATCH'); }
  private validate(name: string, value: unknown): void { const s=schemas[name]; if(s===undefined||!matches(s,value,schemas)) throw new AlightError('CONTRACT'); }
  private async request<R>(name: string, path: string, query?: Record<string,string>, body?: unknown, requestSchema?: string): Promise<R> {
    const url=new URL(path,this.endpoint);
    if (query) url.search=new URLSearchParams(query).toString();
    if(url.search.length>8193) throw new AlightError('REQUEST_TOO_LARGE');
    if(requestSchema) this.validate(requestSchema,body);
    const encoded=body===undefined?undefined:JSON.stringify(body);
    if(encoded!==undefined&&new TextEncoder().encode(encoded).length>65536) throw new AlightError('REQUEST_TOO_LARGE');
    const headers: Record<string,string>={Accept:'application/json'};
    if(encoded!==undefined) {
      if(!this.#operatorKey) throw new AlightError('OPERATOR_REQUIRED');
      headers.Authorization=`Bearer ${this.#operatorKey}`; headers['Content-Type']='application/json';
    }
    const controller=new AbortController(); const timer=setTimeout(()=>controller.abort(),this.timeoutMs);
    try {
      const response=await fetch(url,{method:encoded===undefined?'GET':'POST',headers,...(encoded===undefined?{}:{body:encoded}),signal:controller.signal,redirect:'error',credentials:'omit'});
      const reader=response.body?.getReader(); if(!reader) throw new AlightError('CONTRACT');
      const parts: Uint8Array[]=[]; let size=0;
      try {
        while(true) { const {done,value}=await reader.read(); if(done) break; size+=value.length; if(size>8*1024*1024) { controller.abort(); throw new AlightError('RESPONSE_TOO_LARGE'); } parts.push(value); }
      } finally { reader.releaseLock(); }
      const bytes=new Uint8Array(size); let offset=0; for(const p of parts) { bytes.set(p,offset); offset+=p.length; }
      let payload: unknown; try { payload=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)); } catch { throw new AlightError('CONTRACT'); }
      this.validate(response.ok?name:'ApiErrorResponse',payload);
      if(!response.ok) { const error=payload as T.ApiErrorResponse; this.scope(error.source); throw new AlightError(/^[A-Z_]{1,64}$/.test(error.code)?error.code:'API_ERROR',response.status); }
      return payload as R;
    } catch(error) { if(error instanceof AlightError) throw error; throw new AlightError('TRANSPORT_RECONCILE_WRITES'); }
    finally { clearTimeout(timer); }
  }
  async health(): Promise<T.ApiHealth> { const r=await this.request<T.ApiHealth>('ApiHealth','/v1/health'); this.scope(r.source); return r; }
  /** Static downloads declare their own source; they may differ from the active API. */
  async datasets(): Promise<T.DatasetCatalog> { return this.request<T.DatasetCatalog>('DatasetCatalog','/datasets/index.json'); }
  async dataset(id:string): Promise<T.DatasetManifest> { if(!/^[a-z0-9-]{1,64}$/.test(id)) throw new AlightError('CONFIGURATION'); return this.request<T.DatasetManifest>('DatasetManifest',`/datasets/${id}/manifest.json`); }
  async workbench(): Promise<T.WorkbenchEvidence> { const r=await this.request<T.WorkbenchEvidence>('WorkbenchEvidence','/v1/workbench'); this.scope(r.source); return r; }
  async diagnostics(): Promise<T.DiagnosticsPage> { const r=await this.request<T.DiagnosticsPage>('DiagnosticsPage','/v1/diagnostics'); this.scope(r.source); for(const item of [...r.signals,...r.regimes,...r.alerts,...r.backfills,...r.disagreements]) this.scope(item.source); return r; }
  async canaryEvidence(id: string): Promise<T.CanaryEvidencePage> { if(!/^[A-Za-z0-9_-]{1,128}$/.test(id)) throw new AlightError('CONFIGURATION'); const r=await this.request<T.CanaryEvidencePage>('CanaryEvidencePage',`/v1/canaries/${id}/observations`); this.scope(r.source); return r; }
  async proveCanaries(id: string): Promise<T.ProveCanaryPage> { if(!/^[A-Za-z0-9_-]{1,100}$/.test(id)) throw new AlightError('CONFIGURATION'); const r=await this.request<T.ProveCanaryPage>('ProveCanaryPage',`/v1/prove/${id}/canaries`); this.scope(r.source); return r; }
  async ledgerPayloads(after:T.DecimalU64=decimalU64('0'),limit=100): Promise<T.BrowserLedgerPage> { const r=await this.request<T.BrowserLedgerPage>('BrowserLedgerPage','/v1/ledger/payloads',{after:decimalU64(after),limit:String(limit)}); this.scope(r.source); return r; }
  async clock(): Promise<T.ApiClock> { const r=await this.request<T.ApiClock>('ApiClock','/v1/clock'); this.scope(r.source); return r; }
  async leaders(): Promise<T.ApiTelemetry> { const r=await this.request<T.ApiTelemetry>('ApiTelemetry','/v1/leaders'); this.scope(r.source); return r; }
  async observers(): Promise<T.ObserverHealthPage> { const r=await this.request<T.ObserverHealthPage>('ObserverHealthPage','/v1/observers'); this.scope(r.source); return r; }
  async curves(limit=243): Promise<T.CurvePage> { const r=await this.request<T.CurvePage>('CurvePage','/v1/curve',{limit:String(limit)}); this.scope(r.source); return r; }
  async quote(request: T.QuoteServiceRequest): Promise<T.QuotePreview> { this.scope(request.model.context.source); this.validate('QuoteServiceRequest',request); const r=await this.request<T.QuotePreview>('QuotePreview','/v1/quote',{request:JSON.stringify(request)}); this.scope(r.quote.context.source); return r; }
  async freezeQuote(request: T.QuoteServiceRequest): Promise<T.ForecastEntry> { this.scope(request.model.context.source); const r=await this.request<T.ForecastEntry>('ForecastEntry','/v1/quote',undefined,request,'QuoteServiceRequest'); this.scope(r.forecast.source); return r; }
  async prove(request: T.ProveRequest): Promise<T.ProveReport> { const r=await this.request<T.ProveReport>('ProveReport','/v1/prove',undefined,request,'ProveRequest'); this.scope(r.lock.source); return r; }
  /** Governed held-out canaries; this convenience helper is not a swap sender. */
  async quoteThenSend(request: T.QuoteServiceRequest, requestId: string, n=40): Promise<T.ProveReport> { this.validate('ProveRequest',{request_id:requestId,forecast_hash:'pending',n,seed:null}); const f=await this.freezeQuote(request); return this.prove({request_id:requestId,forecast_hash:f.hash,n,seed:null}); }
  async proveReport(id: string): Promise<T.ProveReport> { if(!/^[A-Za-z0-9_-]{1,100}$/.test(id)) throw new AlightError('CONFIGURATION'); const r=await this.request<T.ProveReport>('ProveReport',`/v1/prove/${id}`); this.scope(r.lock.source); return r; }
  async proves(limit=20): Promise<T.ProvePage> { const r=await this.request<T.ProvePage>('ProvePage','/v1/proves',{limit:String(limit)}); this.scope(r.source); return r; }
  async ledger(after:T.DecimalU64=decimalU64('0'),limit=50): Promise<T.LedgerPage> { const r=await this.request<T.LedgerPage>('LedgerPage','/v1/ledger',{after:decimalU64(after),limit:String(limit)}); this.scope(r.source); return r; }
  async verifyLedger(): Promise<T.LedgerVerification> { const r=await this.request<T.LedgerVerification>('LedgerVerification','/v1/ledger/verify'); this.scope(r.source); if(!r.verified) throw new AlightError('LEDGER_INVALID'); return r; }
  async anchorDraft(): Promise<T.AnchorDraft> { const r=await this.request<T.AnchorDraft>('AnchorDraft','/v1/ledger/anchor'); this.scope(r.source); if(r.status!=='PREPARED_UNSIGNED'||r.signing_enabled||r.signature!==null||r.explorer_url!==null) throw new AlightError('CONTRACT'); return r; }
  async tape(from:string,through:string,limit=100): Promise<T.TapePage> { const r=await this.request<T.TapePage>('TapePage','/v1/tape',{from,through,limit:String(limit)}); this.scope(r.source); return r; }
  async receipt(wallet:string,capture:string,request:T.WalletReceiptRequest): Promise<T.WalletReceipt> { if(!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(wallet)||!/^sha256:[a-f0-9]{64}$/.test(capture)) throw new AlightError('CONFIGURATION'); this.validate('WalletReceiptRequest',request); const r=await this.request<T.WalletReceipt>('WalletReceipt',`/v1/receipt/${wallet}`,{capture,region:request.region,target_p:String(request.target_p),horizon_slots:String(request.horizon_slots),max_curve_age_s:String(request.max_curve_age_s)}); this.scope(r.source); return r; }
  /** Native browser/Node 22 WebSocket. The returned connection must be closed by its owner. */
  stream(onSnapshot:(snapshot:T.ApiStreamSnapshot)=>void,onError:(error:AlightError)=>void): WebSocket {
    const url=new URL('/v1/stream',this.endpoint); url.protocol=url.protocol==='https:'?'wss:':'ws:';
    const ws=new WebSocket(url);
    ws.onmessage=event=> { try {
      if(typeof event.data!=='string'||new TextEncoder().encode(event.data).length>1024*1024) throw new AlightError('CONTRACT');
      const payload:unknown=JSON.parse(event.data); this.validate('ApiStreamSnapshot',payload); const snapshot=payload as T.ApiStreamSnapshot; this.scope(snapshot.source); onSnapshot(snapshot);
    } catch(error) { ws.close(1008,'Invalid snapshot'); onError(error instanceof AlightError?error:new AlightError('CONTRACT')); } };
    ws.onerror=()=>onError(new AlightError('TRANSPORT'));
    return ws;
  }
}
