# Architecture

Alight is a Rust daemon with a SQLite WAL store, an axum REST/WebSocket API,
a React Workbench, and Rust/TypeScript developer clients. The daemon owns
collection, resolution, modeling and background diagnostics. The UI displays
the same source-scoped evidence exposed to clients.

```mermaid
flowchart LR
  subgraph Solami
    Streams["Yellowstone gRPC · Mirage"]
    RPC["RPC"]
    Hook["Webhook push"]
    Blur["Blur trades"]
    Data["Data API"]
    Beam["Beam QUIC · tipped HTTP"]
  end
  Streams --> Ingest["Observers · clock · leaders · tip tape"]
  RPC --> Ingest
  Hook --> Ingest
  Ingest --> Resolver["Outcome resolver"]
  Resolver --> Store[("SQLite WAL")]
  Ingest --> Store
  Store --> Models["M0/M1/M2 · regimes · grading"]
  Blur --> Economics["Delay-cost estimates"]
  Models --> Quotes["Quotes · immutable forecast ledger"]
  Economics --> Quotes
  Quotes --> Store
  Store --> API["REST / WebSocket API"]
  Data --> Receipts["Wallet receipts + finalized RPC checks"]
  RPC --> Receipts
  Receipts --> API
  API --> UI["React Workbench"]
  API --> Clients["CLI · Rust SDK · TypeScript SDK"]
  Store --> Export["CSV / Parquet · checksums"]
  Store --> Governor["Durable budget governor"]
  Governor --> Sender["Canaries · ledger-head memos"]
  Sender --> Beam
  Sender --> RPC
```

Beam HTTP submits a tipped transaction to the configured Solami RPC endpoint;
plain RPC submits without the Beam tip. They are distinct experimental
treatments sharing an HTTP endpoint. Beam QUIC uses the pinned Solami SDK.
gRPC and Mirage are separate transports from the same provider; agreement
does not establish independent infrastructure or trust.

## Forecast and Prove flow

```mermaid
sequenceDiagram
  participant Client as Workbench / SDK / CLI
  participant API as API
  participant Ledger as Forecast ledger
  participant Sender as Governed sender
  participant Chain as Solana + observers
  Client->>API: Preview a supported quote (GET)
  Client->>API: Freeze quote (authenticated POST)
  API->>Ledger: Append exact forecast + frozen model hash
  Ledger-->>Client: Forecast hash
  Client->>API: Lock hash, exact configuration and N
  API->>Sender: Schedule prospective held-out attempts
  Sender->>Chain: Reserve, persist identity, submit once
  Chain-->>API: Finalized outcome evidence
  API-->>Client: Wilson interval + honest verdict
```

Held-out attempts never train the model. Unknown submissions stay charged and
are resolved after restart; they are not automatically rebroadcast. A timeout
cannot establish expiry. An insufficient quote authorizes no recommendation.

## Code map

| Path | Responsibility |
| --- | --- |
| `bin/alightd/`, `bin/alight/` | Supervised daemon and CLI |
| `crates/alight-types/` | Shared wire contracts, units, validation and canonical JSON |
| `crates/alight-store/` | Migrations, atomic persistence, evidence and ledger storage |
| `crates/alight-ingest/`, `crates/alight-tape/` | Provider normalization, clocks, leaders and passive transfers |
| `crates/alight-canary/` | Sole signing/sending path, governor, transaction builder and resolver |
| `crates/alight-model/`, `crates/alight-forecast/`, `crates/alight-prove/` | Estimation, immutable forecasts, grading and held-out validation |
| `crates/alight-diagnostics/`, `crates/alight-notify/` | Regime/observer diagnostics and configured alert delivery |
| `crates/alight-econ/`, `crates/alight-tape/src/receipts.rs` | Conditional market cost and wallet accounting |
| `crates/alight-sim/` | Seeded offline environment and reproducible replay |
| `crates/alight-api/`, `crates/alight-client/`, `sdk/ts/` | API and developer clients |
| `web/`, `deploy/`, `scripts/` | Workbench, service templates and bounded verification tools |

## Operating boundaries

| Mode | Source | Network | Signing/sending |
| --- | --- | --- | --- |
| Observe | Live | Reads provider streams/history | Disabled |
| Live | Live | Reads and submits governed attempts | Enabled only with configured identities and passing gates |
| Sim | Sim | None | None; synthetic outcomes |
| Replay | Replay, or preserved Sim for model replay | None | None; retained evidence only |

Provider credentials remain server-side. Public reads are bounded and
rate-limited; quote freeze and Prove require an operator bearer key. Wallet
history reads are additionally bounded and cached. Signing, database limits,
observer freshness, wallet reserve and budget checks precede broadcast.
See [operations](operations.md), [contracts](contracts.md) and
[limitations](limitations.md) for the exact controls and evidence scope.
