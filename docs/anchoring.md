# Ledger-head anchoring

Alight prepares a source-scoped commitment to a verified retained forecast
chain. The Memo instruction is built inside `alight-canary`; its exact text is
`alight.ledger.v1|SOURCE|SEQUENCE|sha256:HASH`. Sequence is lossless decimal and
the hash includes the source-specific forecast prefix. No secret, wallet or
authenticated URL enters the draft. A nonempty verified ledger is required.

```sh
alight anchor prepare --database .alight/judge/sim.db --source sim \
  --output .alight/judge/anchor.json
alight anchor verify --database .alight/judge/sim.db \
  --input .alight/judge/anchor.json
```

Both commands open SQLite read-only. Verify checks the historical prefix, so
later append-only forecasts do not invalidate an older draft. Changing source,
sequence, memo or hash fails. The workbench's read-only ledger-head endpoint
returns the same unsigned draft; no request reserves spend, signs or broadcasts.

Status is `PREPARED_UNSIGNED`, signing is false, and signature/explorer fields
are null. The committed example under `data/anchors/` is Sim and demonstrates
offline integrity only. It is not a mainnet anchor, chain timestamp or proof
that the forecast was issued before an outcome.

`anchor preflight --database LEDGER_DB --source live|sim|replay` performs four
bounded mainnet RPC reads using the public dedicated-wallet address. It reports
the exact memo, quoted fee, balance and reserve without loading a signer or
reserving spend. `--max-fee-lamports` defaults to 10,000 and cannot exceed 100,000.
The wallet retains at least 1,000,000 lamports. No tip or priority fee is added.

After explicit transaction authorization, `anchor send` permits one attempt
for the current verified source/head. `anchor run` checks every 3,600 seconds
(`--interval-s`, minimum 300), sending only when the head changes. Both commands
require `--authorize-mainnet true`. They share `ALIGHT_DB_PATH` and the Live/RPC
daily and burst governor caps with canaries. The ledger may be a separate
read-only Sim/Replay database; its source remains explicit in the memo.
Observe never starts an anchor sender, even with signing variables present.

The consumed reservation and signed attempt are durable before broadcast.
Unknown responses, crashes and rejected attempts remain charged; no automatic
rebroadcast or replacement signature is created. `anchor reconcile --database
LEDGER_DB --source MODE` checks at most eight pending signatures, using read-only
provider configuration. Finalized transaction bytes, signature, payer, memo and
fee must match the original attempt. Null/timeout is unresolved. Failed
finalized execution never becomes a successful anchor. Events are append-only.
`GET /v1/ledger/anchors` exposes bounded source-scoped records. The Ledger page
shows a small explorer link only for successfully verified finalized anchors.
Public records can be saved under `data/anchors/` after verification.
`anchor verify-network --database LEDGER_DB --source MODE --input RECORD.json`
independently checks a public record against retained ledger bytes and finalized
mainnet RPC transaction bytes. It opens the ledger read-only, loads no signer,
reserves no budget and returns success only for a successful finalized memo.

The sender is implemented but has not been enabled or exercised on mainnet.
No mainnet anchor, explorer receipt or provider integration is claimed. The
owner has resumed observe-only collection; transaction sending remains disabled.
