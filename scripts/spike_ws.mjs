import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { loadEnvFile } from "node:process";
import { SubscribeUpdate } from "@triton-one/yellowstone-grpc";

// Bounded read-only transport/schema probe. Never logs an authenticated URL.
if (existsSync(".env")) loadEnvFile(".env");
const product = process.argv[2] ?? "blur";
const captureTransactions = process.argv.includes("--transactions");
const events = [];
const secrets = Object.entries(process.env).filter(([key, value]) => value?.length >= 8 &&
  /^(SOLAMI_|ALIGHT_)/.test(key) && /TOKEN|KEY|SECRET/.test(key)).map(([, value]) => value);
let bytes = 0;
let verdict = "MISSING";
let closeCode;
let errorCategory;

function scrub(value) {
  if (typeof value === "string") return secrets.reduce((s, key) => s.replaceAll(key, "[REDACTED]"), value);
  if (Array.isArray(value)) return value.map(scrub);
  if (value && typeof value === "object") return Object.fromEntries(Object.entries(value).map(([k, v]) =>
    [k, /^(secret|token|api_key|authorization)$/i.test(k) ? "[REDACTED]" : scrub(v)]));
  return value;
}

try {
  if (!["blur", "mirage", "webhook"].includes(product)) throw new Error("unsupported product");
  const prefix = {blur: "SOLAMI_BLUR", mirage: "SOLAMI_MIRAGE", webhook: "SOLAMI_WEBHOOK"}[product];
  const token = process.env[`${prefix}_TOKEN`]?.trim();
  const id = process.env.SOLAMI_MIRAGE_SUBSCRIPTION_ID?.trim();
  const configuredUrl = process.env[product === "mirage" ? `${prefix}_URL` : `${prefix}_WS_URL`]?.trim();
  if (token && (product === "blur" || (product === "mirage" && id) || configuredUrl)) {
    const url = new URL(configuredUrl || `wss://ws.solami.dev/mirage/stream/${encodeURIComponent(id)}`);
    if (url.protocol !== "wss:") throw new Error("invalid scheme");
    url.searchParams.set("api_key", token);
    if (product === "blur") {
      url.searchParams.set("chain", "solana");
      url.searchParams.set("type", "swap");
      url.searchParams.set("dex", "raydium_clmm");
      url.searchParams.set("metadata", "false");
    }
    verdict = await new Promise((resolve) => {
      const start = process.hrtime.bigint();
      let finished = false;
      let socket;
      const finish = (result) => {
        if (finished) return;
        finished = true;
        clearTimeout(timer);
        try { socket?.close(); } catch { /* No upstream error text is exposed. */ }
        if (result === "PASS" && product === "mirage" &&
          !(events.some(e => e.data.slot) && events.some(e => e.data.blockMeta) &&
            (!captureTransactions || events.some(e => e.data.transaction)))) result = "INCONCLUSIVE";
        resolve(result);
      };
      const timer = setTimeout(() => finish(events.length ? "PASS" : "INCONCLUSIVE"), 20_000);
      try { socket = new WebSocket(url); } catch { finish("FAIL"); return; }
      socket.binaryType = "arraybuffer";
      socket.addEventListener("error", () => { errorCategory = "connection_or_authentication"; finish("FAIL"); });
      socket.addEventListener("close", (event) => {
        closeCode = event.code;
        finish(events.length ? "PASS" : event.code === 1000 ? "INCONCLUSIVE" : "FAIL");
      });
      socket.addEventListener("message", (event) => {
        if (finished) return;
        try {
          const raw = typeof event.data === "string" ? Buffer.from(event.data) : Buffer.from(event.data);
          if (bytes + raw.length > 256 * 1024) { finish(events.length ? "PASS" : "INCONCLUSIVE"); return; }
          bytes += raw.length;
          const data = product !== "mirage" ? JSON.parse(raw.toString()) : SubscribeUpdate.toJSON(SubscribeUpdate.decode(raw));
          events.push({ source: "live", received_at: new Date().toISOString(),
            recv_mono_ns: (process.hrtime.bigint() - start).toString(),
            // Keep exact JSON integers for the Rust adapter; JS numbers are previews only.
            raw_json: product !== "mirage" ? scrub(raw.toString()) : undefined,
            data: scrub(data) });
          const limit = product === "webhook" ? 3 : captureTransactions ? 100 : 12;
          if (events.length >= limit || (product === "mirage" && captureTransactions &&
            events.filter(e => e.data.transaction).length >= 3 && events.some(e => e.data.slot) &&
            events.some(e => e.data.blockMeta))) finish("PASS");
        } catch { errorCategory = "unexpected_frame_schema"; finish("FAIL"); }
      });
    });
  }
} catch { verdict = "FAIL"; errorCategory = "configuration_or_schema"; }

mkdirSync(".alight/probes", { recursive: true, mode: 0o700 });
const summary = { schema_version: 1, product, source: "live", verdict, bytes, frames: events.length,
  close_code: closeCode ?? null, error_category: errorCategory ?? null, canaries_sent: 0 };
writeFileSync(`.alight/probes/${product}-ws.json`, JSON.stringify(summary, null, 2) + "\n");
if (events.length) {
  mkdirSync("data/fixtures", { recursive: true });
  const filename = {mirage: captureTransactions ? "mirage_transactions_sample.jsonl" : "mirage_sample.jsonl", blur: "blur_ws_sample.jsonl",
    webhook: "webhook_ws_sample.jsonl"}[product];
  writeFileSync(`data/fixtures/${filename}`, events.map(e => JSON.stringify(e)).join("\n") + "\n");
}
console.log(JSON.stringify(summary, null, 2));
process.exitCode = verdict === "PASS" ? 0 : verdict === "FAIL" ? 1 : 3;
