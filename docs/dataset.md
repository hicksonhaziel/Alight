# Public dataset

Daily exports contain source-scoped canaries, curve snapshots, detected regime
events, immutable forecasts and appended grades. Selection uses UTC half-open
days: send, curve as-of, detection, forecast issue and grading times respectively.
Null/unresolved outcomes, finalization, propensity, uniform-arm flags and profile
policy IDs remain visible. Original payload JSON includes lossless slots,
signatures, seeds where recorded, observer clocks and workload configuration.
Canary history does not acquire a missing seed or region by inference.

```sh
python3 -m pip install -r scripts/dataset-requirements.txt
alight export --database .alight/judge/sim.db --source sim \
  --day 2026-10-05 --output .alight/judge/dataset
python3 scripts/verify_dataset.py .alight/judge/dataset
```

The default emits CSV and Zstandard Parquet. `--format csv` uses Python's
standard library only. The CLI embeds the exporter; Python is a runtime
requirement. Existing `export --api` remains the earlier forecast-only JSON
command. A new output directory is required. Export opens the existing database
read-only and takes one SQLite snapshot; it performs no migrations, collection,
provider calls or transactions. Bounds are 100,000 rows per table and 64 MiB
per bundle. A suspicious credential/URL/formula field rejects the entire export;
immutable forecast bytes are never rewritten to make a secret scan pass.

`schema.json` describes columns and units. Decimal strings preserve u64 money
and sequence values. `manifest.json` declares counts, source, regions present,
coverage and license. `SHA256SUMS` covers all files except itself. Empty files
retain column schemas. `ledger-witness.json` contains the full retained source
chain and referenced frozen-model/market documents, including forecasts outside
the selected day, so the published hashes can be independently checked.
The verifier checks witness links and documents and CSV/Parquet agreement.

Publish only intentionally reviewed exports. The example downloadable bundle
is Sim, not a mainnet performance dataset. Static downloads use the existing
workbench/Caddy and a standalone GitHub Pages download site. The Pages workflow
publishes only after CI succeeds; `scripts/build_dataset_site.py` independently
verifies every catalog bundle before copying it. Replace the catalog with retained reviewed bundles when actual
Live evidence is available. Private wallet captures are not automatically
included or licensed. Data is licensed CC BY 4.0; preserve source, date, vantage
and methodology attribution. This license does not license provider software.

Retention is coverage, not completeness. Failed executions, proven nonlandings
and unresolved sends have different meanings. Passive tape cannot estimate
submission failure. Review [methodology](methodology.md),
[receipt methodology](receipts.md) and [limitations](limitations.md) before
comparing profiles, regions or populations.
