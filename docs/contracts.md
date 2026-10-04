# Current contracts — Phase 0 draft

Only the primitives in `crates/alight-types/` are implemented. Full canary,
observer, quote, storage, and ledger contracts are **not frozen** while sender
and webhook fixtures are missing. No API response is promised by this scaffold.

| Primitive | JSON representation |
| --- | --- |
| Source | `live`, `sim`, `replay` |
| Route | `beam_quic`, `beam_http`, `rpc` |
| Outcome | `LANDED_OK`, `LANDED_FAILED`, `LANDED_THEN_DROPPED`, `EXPIRED`, `REJECTED`, `UNRESOLVED` |
| Probe verdict | `PASS`, `FAIL`, `INCONCLUSIVE`, `MISSING` |

An expired blockhash requires observed **block height strictly greater than**
`last_valid_block_height` and signature absence at the required commitment.
A timeout, network failure, or slot distance alone cannot establish expiry.

## Measured payload adjustments

- Yellowstone protobuf enum zero is `SLOT_PROCESSED`; protobuf JSON may omit
  that default. Fixtures explicitly retain both status name and code.
- Transaction updates carry a slot and transaction index but no block identity.
  Preserve missing block identity and join to block metadata with provenance;
  never manufacture a fork identity from the slot alone.
- Mirage delivers Yellowstone `SubscribeUpdate` binary frames on a saved
  subscription URL, authenticated with the API key. Ping/control variants must
  not be counted as transaction observations. Its long-run heartbeat and
  reconnect behavior remain to be measured.
- Blur REST returns arrays of pool/trade/candle objects. Decimal prices, USD
  values, and liquidity can be strings; raw amounts can exceed JS safe integer
  range. Use exact raw JSON in adapters. Candle count may include endpoint bars.
- `recv_mono_ns` is a decimal string relative to a probe's monotonic start, not
  comparable between separate processes. Future durable observations must also
  record the clock/process identity and UTC receive time.
- Candidate landing records must be keyed by `(slot, block identity)` and retain
  observer disagreements. Slot-only events and gap events need separate storage.
- Webhook schema, signature encoding, delivery lag, and HTTP retry behavior are
  pending a real delivery. No adapter should guess those fields.

Example captured metadata envelope (values shortened here for readability):

```json
{
  "source": "live",
  "kind": "slot",
  "slot": "453063149",
  "status": "SLOT_PROCESSED",
  "status_code": 0,
  "received_at": "2026-10-03T22:05:53.000Z",
  "recv_mono_ns": "123000000",
  "raw_sha256": "<sha256-of-protobuf-frame>"
}
```

Use the exact recorded files in `data/fixtures/` for adapter work. The example
above illustrates representation only; it is not an additional measured event.
