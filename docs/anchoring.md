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
that the forecast was issued before an outcome. Periodic mainnet memo sends,
budget accounting through the existing governor, RPC confirmation, explorer
records and on-chain verification remain pending explicit transaction
authorization. Collection and transaction sending remain paused.
