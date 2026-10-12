# Rust and TypeScript SDKs

Use the SDKs to bring Alight's measurement loop into your application: inspect
current evidence, preview a landing quote, freeze a supported forecast, start
a held-out Prove and verify the ledger. Both clients call your Alight daemon's
API. The daemon owns collection, wallet keys and governed canary submission.

## Distribution today

The clients ship as **repository-local source packages**, both at version
`0.0.0`, with no public npm or crates.io release. They are built and tested in
CI; this repository does not ship an SDK release workflow.

| Client | Source | Current use |
| --- | --- | --- |
| TypeScript / JavaScript | [`sdk/ts`](../sdk/ts/) | Build locally; import the built module or install the local directory. The package is named `@alight/client` and marked `private: true`. |
| Rust | [`crates/alight-client`](../crates/alight-client/) | Use a Cargo path dependency. Shared `alight-types` is also a local workspace crate. |

The TypeScript manifest's `private` flag disables npm publication. The source
is public under the [MIT license](../LICENSE).

The `@alight/client` import in copied examples is the local package name, not
an instruction to download a published registry release. The quick start runs
the Workbench without installing either SDK separately. A future registry
release needs versioned packages, reviewed package contents and verified
standalone installation; that is separate from the current local workflow.

## TypeScript / JavaScript

From the Alight repository, with the [quick-start prerequisites](quickstart.md#1-prepare):

```sh
npm --prefix sdk/ts ci --ignore-scripts
npm --prefix sdk/ts run build
```

From another application's directory, install that built local package using
the actual path to your checkout:

```sh
npm install /absolute/path/to/Alight/sdk/ts
```

```ts
import { AlightClient } from '@alight/client';

const client = new AlightClient({
  endpoint: 'http://127.0.0.1:8081',
  source: 'sim',
});
const health = await client.health();
console.log(health.source, health.collector_status);
```

The package exports ESM JavaScript and TypeScript declarations. A direct import
from `sdk/ts/dist/index.js` also works after building, as the checked-in example
demonstrates. Generated `dist/` files are not committed.

## Rust

In the consuming application's `Cargo.toml`, set the path to your checkout:

```toml
[dependencies]
alight-client = { path = "/absolute/path/to/Alight/crates/alight-client" }
```

Inside an async runtime:

```rust
use alight_client::{Client, Source};

let client = Client::new("http://127.0.0.1:8081", Source::Sim, None)?;
let health = client.health().await?;
```

The crate re-exports shared contract types, including `Source`. The complete
[Rust example](../crates/alight-client/examples/quote_then_send.rs) shows the
runtime and error handling; the [JavaScript example](../sdk/ts/examples/quote_then_send.mjs)
shows the equivalent flow.

## Try the full loop

The [operations guide](operations.md#cli-and-sdk) runs both examples against
an isolated Sim daemon. They preview a quote, freeze it, run 40 synthetic
held-out canaries and verify the ledger, with no provider network or SOL.
The convenience helpers are `quote_then_send` in Rust and `quoteThenSend` in
TypeScript. They operate on Alight canaries; they do not build application swaps.
The checked-in examples use fixed Prove request IDs; use a fresh isolated Sim
database for an independent rerun.
In Live, starting Prove returns progress while the daemon runs the attempts.
Poll `proveReport()` in TypeScript or `prove_report()` in Rust with the returned
`lock.id` to read the outcome.

Public reads need no operator key. Freezing a forecast and starting Prove
require the API operator key; provider tokens and wallet keys stay server-side.
An unsupported quote has no recommendation. Clients enforce the requested
Live/Sim/Replay source and never automatically retry an uncertain write:
reconcile using the forecast hash and Prove request ID before proceeding.
See [contracts](contracts.md) for payloads, units and authentication, and
[limitations](limitations.md) for what canary measurements establish.
