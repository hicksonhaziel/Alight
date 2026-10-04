import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { loadEnvFile } from "node:process";
import { createHash, randomUUID } from "node:crypto";
import bs58 from "bs58";
import { CommitmentLevel, SlotStatus, SubscribeRequest, SubscribeUpdate } from "@triton-one/yellowstone-grpc";
import { GrpcClient } from "@triton-one/yellowstone-grpc/napi";

// Bounded Phase 0 probe, adapted from Aftershock's MIT-licensed stream check.
// Slot/block metadata and filtered tip/payer transactions, not a full firehose.
const directory = `.alight/probes/grpc-${randomUUID()}`;
const timeoutMs = 20_000;
const maxBytes = 512 * 1024;
const maxFrames = 100;
let stream;
let timedOut = false;
let timer;
let bytes = 0;
const events = [];
const frames = [];
let request;
let verdict = "INCONCLUSIVE";
let failure;

try {
  if (existsSync(".env")) loadEnvFile(".env");
  const endpoint = process.env.SOLAMI_GRPC_URL?.trim();
  const token = process.env.SOLAMI_GRPC_TOKEN?.trim() || process.env.SOLAMI_API_KEY?.trim();
  if (!endpoint || new URL(endpoint).protocol !== "https:" || !token) throw new Error("configuration");
  const tips = JSON.parse(await (await fetch(`${process.env.SOLAMI_DATA_API_URL}/onchain/tip-addresses`,
    { redirect: "error", signal: AbortSignal.timeout(8_000) })).text());
  if (!Array.isArray(tips) || !tips.every(t => typeof t === "string" && bs58.decode(t).length === 32)) {
    throw new Error("tip response");
  }
  const accounts = [...tips];
  if (process.env.ALIGHT_CANARY_KEYPAIR?.trim()) {
    const key = bs58.decode(process.env.ALIGHT_CANARY_KEYPAIR.trim());
    if (key.length !== 64) throw new Error("keypair shape");
    accounts.push(bs58.encode(key.subarray(32)));
  }
  request = SubscribeRequest.fromPartial({
    slots: { clock: { filterByCommitment: false } },
    blocksMeta: { identity: {} },
    transactions: { canaries_and_tips: { vote: false, accountInclude: accounts } },
    commitment: CommitmentLevel.PROCESSED,
  });
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  timer = setTimeout(() => { timedOut = true; stream?.close(); }, timeoutMs);
  const client = await GrpcClient.new(endpoint, token, {
    grpcMaxDecodingMessageSize: 131_072,
  }, { enabled: false });
  if (timedOut) throw new Error("connection timeout");
  stream = await client.subscribe(Buffer.from(SubscribeRequest.encode(request).finish()));
  if (timedOut) throw new Error("subscription timeout");
  const start = process.hrtime.bigint();
  while (!timedOut && frames.length < maxFrames && bytes < maxBytes) {
    const raw = await stream.read();
    if (!raw) break;
    if (bytes + raw.length > maxBytes) break;
    bytes += raw.length;
    const filename = `frame-${frames.length}.bin`;
    const recvMonoNs = (process.hrtime.bigint() - start).toString();
    const receivedAt = new Date().toISOString();
    writeFileSync(`${directory}/${filename}`, raw, { flag: "wx", mode: 0o600 });
    const sha256 = createHash("sha256").update(raw).digest("hex");
    frames.push({ file: filename, sha256, bytes: raw.length, received_at: receivedAt });
    const update = SubscribeUpdate.decode(raw);
    const common = { source: "live", recv_mono_ns: recvMonoNs, received_at: receivedAt, raw_sha256: sha256 };
    if (update.slot) events.push({ ...common, kind: "slot", ...SubscribeUpdate.toJSON(update).slot,
      status: SlotStatus[update.slot.status] ?? "UNRECOGNIZED", status_code: update.slot.status });
    if (update.blockMeta) events.push({ ...common, kind: "block_meta", ...SubscribeUpdate.toJSON(update).blockMeta });
    if (update.transaction) {
      const tx = update.transaction.transaction;
      events.push({ ...common, kind: "transaction", slot: update.transaction.slot,
        signature: tx?.signature ? bs58.encode(tx.signature) : null,
        index: tx?.index ?? null, failed: Boolean(tx?.meta?.err),
        // Yellowstone transaction updates do not carry a blockhash. Join to block metadata.
        block_id: null });
    }
    if (events.filter(e => e.kind === "slot").length >= 24 &&
      events.some(e => e.kind === "block_meta")) break;
  }
  verdict = events.some(e => e.kind === "slot") && events.some(e => e.kind === "block_meta") ? "PASS" : "INCONCLUSIVE";
} catch {
  verdict = "FAIL";
  failure = "Check configuration, authentication, entitlement, or network; upstream details withheld.";
} finally {
  clearTimeout(timer);
  stream?.close();
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const summary = { schema_version: 1, source: "live", purpose: "bounded-grpc-schema-spike",
    verdict, failure, bytes, frames, event_counts: Object.fromEntries(["slot", "block_meta", "transaction"].map(
      kind => [kind, events.filter(e => e.kind === kind).length])),
    request: request ? SubscribeRequest.toJSON(request) : null,
    transaction_capture_completeness: "not_assessed", canary_sent: false };
  writeFileSync(`${directory}/summary.json`, JSON.stringify(summary, null, 2) + "\n", { flag: "wx", mode: 0o600 });
  // Only sanitized protocol/chain values reach redistributable schema fixtures.
  mkdirSync("data/fixtures", { recursive: true });
  writeFileSync("data/fixtures/grpc_sample.jsonl", events.map(e => JSON.stringify(e)).join("\n") +
    (events.length ? "\n" : ""));
  console.log(JSON.stringify({ verdict, bytes, event_counts: summary.event_counts, evidence: directory,
    fixture: "data/fixtures/grpc_sample.jsonl", canary_sent: false }, null, 2));
  process.exitCode = verdict === "PASS" ? 0 : verdict === "INCONCLUSIVE" ? 3 : 1;
}
