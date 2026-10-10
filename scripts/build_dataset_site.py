#!/usr/bin/env python3
"""Build a small public download site from reviewed, checksum-verified bundles only."""
import html
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
output = Path(sys.argv[1] if len(sys.argv) == 2 else ".alight/dataset-site").resolve()
output.mkdir(parents=True, exist_ok=False)
catalog = json.loads((root / "web/public/datasets/index.json").read_text())
assert catalog["schema_version"] == 1 and len(catalog["datasets"]) <= 31
sections = []
for item in catalog["datasets"]:
    identity = item["id"]
    assert re.fullmatch(r"[a-z0-9-]{1,64}", identity)
    bundle = root / "web/public/datasets" / identity
    subprocess.run([sys.executable, str(root / "scripts/verify_dataset.py"), str(bundle)], check=True)
    manifest = json.loads((bundle / "manifest.json").read_text())
    assert (item["source"], item["day"]) == (manifest["source"], manifest["day"])
    # The verifier rejects unknown files. A whole reviewed bundle is the publication unit.
    shutil.copytree(bundle, output / "datasets" / identity)
    link = lambda name: f'<a href="datasets/{identity}/{name}" download>{html.escape(name)}</a>'
    rows = []
    for table, details in manifest["tables"].items():
        assert re.fullmatch(r"[a-z_]+", table)
        downloads = [link(f"{table}.{ext}") for ext in ("csv", "parquet") if f"{table}.{ext}" in manifest["files"]]
        rows.append(f'<tr><th scope="row">{html.escape(table)}</th><td>{details["rows"]}</td><td>{" · ".join(downloads)}</td></tr>')
    evidence = " · ".join(link(name) for name in ("schema.json", "manifest.json", "SHA256SUMS", "ledger-witness.json", "LICENSE.txt"))
    sections.append(f'<section><h2>{html.escape(item["source"].upper())} · {html.escape(item["day"])} UTC</h2><p>{html.escape(item["label"])}</p><p>{html.escape(manifest["coverage"])}</p><table><thead><tr><th>Table</th><th>Rows</th><th>Download</th></tr></thead><tbody>{"".join(rows)}</tbody></table><p>{evidence}</p></section>')
(output / "datasets/index.json").write_text(json.dumps(catalog, indent=2) + "\n")
(output / "methodology").mkdir()
for name in ("methodology", "receipts", "dataset", "limitations"):
    shutil.copyfile(root / f"docs/{name}.md", output / f"methodology/{name}.md")
shutil.copyfile(root / "web/public/alight-mark.svg", output / "alight-mark.svg")
(output / ".nojekyll").touch()
(output / "index.html").write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="dark light"><title>Alight · Evidence downloads</title>
<style>body{font:16px/1.6 system-ui,sans-serif;background:#141416;color:#f0f0f2;max-width:880px;margin:auto;padding:32px 20px}a{color:#c9b6ff;text-underline-offset:3px}a:focus-visible{outline:2px solid #fff;outline-offset:4px}header{display:flex;gap:12px;align-items:center}img{width:36px}h1{font-size:1.8rem}h2{font-size:1.15rem}section{margin:28px 0;border-top:1px solid #444;padding-top:20px}table{width:100%;border-collapse:collapse;text-align:left}td,th{padding:10px 4px;border-bottom:1px solid #444}td:last-child{overflow-wrap:anywhere}p{color:#c6c6cf}@media(max-width:500px){body{font-size:14px}td,th{padding:8px 2px}}</style>
<header><img src="alight-mark.svg" alt=""><h1>Alight evidence downloads</h1></header><main><p>Daily bundles include CSV, Parquet, schemas and checksums. Each bundle declares its own source. Simulated samples demonstrate the format; they do not establish mainnet performance.</p>''' + "".join(sections) + '''<section><h2>Methodology</h2><p><a href="methodology/methodology.md">Models</a> · <a href="methodology/receipts.md">Receipts</a> · <a href="methodology/dataset.md">Schema and verification</a> · <a href="methodology/limitations.md">Limitations</a></p><p>Data: <a href="https://creativecommons.org/licenses/by/4.0/">CC BY 4.0</a>. Private wallet captures are excluded.</p><p><a href="https://github.com/hicksonhaziel/Alight">Source code and local workbench</a></p></section></main></html>''')
print(json.dumps({"status": "PASS", "bundles": len(sections), "output": str(output)}))
