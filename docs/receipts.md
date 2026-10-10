# Wallet receipts

Receipts evaluate bounded Alight captures of normalized transactions. They
cover one fee payer, at most 1,000 records, and at most 24 hours. Captures are
imported into an isolated Sim/Replay database and content-addressed; public
reads select the latest saved wallet capture; an optional hash pins an exact
capture. The existing captured Yellowstone/Mirage
parser can turn recorded JSONL into Replay history without network access.
The read-only RPC fallback retrieves one finalized signature page and its
in-window transactions. It needs RPC read access, no signing identity or SOL.

```sh
# recipients.json is an explicitly reviewed list of tip recipient public keys.
alight receipt capture --input data/fixtures/mirage_transactions_sample.jsonl \
  --wallet WALLET --recipients recipients.json \
  --from 2026-10-04T00:40:00Z --through 2026-10-04T00:45:00Z \
  --output .alight/judge/wallet-capture.json
alight receipt import --database .alight/judge/replay.db \
  --input .alight/judge/wallet-capture.json
alight receipt evaluate --database .alight/judge/replay.db \
  --input .alight/judge/wallet-capture.json --request receipt-request.json \
  --output .alight/judge/wallet-receipt.json
```

To fetch a bounded real-wallet sample instead of reading recorded JSONL:
```sh
alight receipt fetch-rpc --wallet WALLET --recipients recipients.json \
  --from 2026-10-09T00:00:00Z --through 2026-10-09T23:59:59Z --limit 4 \
  --output .alight/judge/rpc-wallet-capture.json
```
Use a window covering the wallet's recent transactions, then import/evaluate
that capture with the commands above. The default limit is 16, maximum 32;
the job makes at most `limit + 2` read requests, with no retries or pagination.
Provider bandwidth/access charges can still apply. The JSON summary counts
signatures outside the window, unavailable rows and transactions paid by
another account. An empty page is not proof that the wallet made no submissions.
Null transaction data is not converted into zero fees. Unsupported transaction
versions or conflicting signature/slot/time/status evidence fail closed.

RPC captures retain execution fees and chain time, with the actual later fetch
timestamp. They remain **Replay**, with unknown transport, workload, regime and
block index. They cannot be imported as Live or establish a supported historical
Beam comparison without additional evidence. This fallback is not a Data API
wallet-history integration. Its source contracts are Solana's
[signature history](https://solana.com/docs/rpc/http/getsignaturesforaddress) and
[transaction response](https://solana.com/docs/rpc/http/gettransaction).

Example request:
```json
{"region":"synthetic-local","target_p":0.9,"horizon_slots":2,"max_curve_age_s":300}
```

The receipt workbench uses 90% within two slots and a 300-second maximum age,
with the API's region. The CLI can specify its own bounded target. Evaluation
opens evidence read-only; importing refuses the workspace's `data/alight.db`.
Imports cannot assert Live history. No command sends transactions.

Fees count once per signature; duplicate deliveries must agree on execution
and context. Conflicting candidates are rejected. Failed executions retain
fees but zero paid tips. Known outer native transfers from this fee payer
count as paid; uncertain inner or other-account transfers remain unknown.
The known-paid total is not a total including unknown payments. Explicit
recipient configuration identifies payment destinations, not transport route.

Only Beam routes with explicit transaction chain time, route, workload class
and regime can be compared. The newest historical curve cohort must predate
that chain time and match source, region, regime, horizon, CU limit and CU price.
The cheapest measured cell whose lower 95% bound meets the target establishes
the supported-tip threshold. The transaction's paid configuration must also
appear in that cohort. Evidence age includes both original curve data age and
elapsed time to the transaction. No favorable older cohort replaces a newer
insufficient one. Future/current quotes are never substituted.

Conflicting same-time snapshots for one cell cannot establish a supported
threshold; future-dated training windows are excluded. With no configured tip
recipient coverage, a successful transaction's payment is unknown, not zero.

Spend above the threshold is conditional arithmetic. It does not establish
equivalent workload performance, no probability gain, trader waste or a USD
economic knee. Failed share measures visible executions only, not all wallet
submissions. Missing context/frontier coverage is printed for each row, along
with survivorship, uncontrolled workload and single-vantage caveats.

Solami's current documentation includes `/data/wallet/trades` and
`/data/wallet/fees`. The latter measures registered venue tips and explicitly
excludes base and priority fees. Trade rows cannot establish authoritative
network fees or all failed submissions. On 10 October the configured account
returned HTTP 402 (balance/bandwidth), so a real wallet-history response remains
unverified. The recorded real-wallet test produces a descriptive Replay report
without route/time/frontier claims. See the [provider contract](https://solami.dev/docs/api/get_data-wallet-fees).
