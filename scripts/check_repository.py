#!/usr/bin/env python3
"""Check the actual publishable file set without printing secret values."""
from pathlib import Path
import re
import shlex
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
forbidden = {"ALIGHT_PLAN.md", "ALIGHT_BUILD.md", "context.txt", ".env"}
files = subprocess.check_output(
    ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root
).decode().split("\0")
files = sorted(set(filter(None, files)))
secrets = []
env = root / ".env"
if env.exists():
    for line in env.read_text().splitlines():
        if not line or line.lstrip().startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        try:
            values = shlex.split(value, comments=True)
        except ValueError:
            continue
        value = values[0] if values else ""
        if len(value) >= 12 and re.search(r"TOKEN|KEY|SECRET", key):
            secrets.append(value)
        for match in re.finditer(r"[?&](?:api_key|token)=([^&\s]+)", value):
            if len(match[1]) >= 12:
                secrets.append(match[1])

failures = []
for name in files:
    path = root / name
    if name in forbidden or (path.name.startswith(".env") and path.name != ".env.example"):
        failures.append((name, "private file is publishable/tracked"))
    if not path.is_file() or path.is_symlink():
        continue
    data = path.read_bytes()
    if any(value.encode() in data for value in secrets):
        failures.append((name, "known credential present"))
    if re.search(rb"(?:secret_key_|whsec_)[A-Za-z0-9_-]{20,}", data):
        failures.append((name, "provider credential pattern present"))

for name in forbidden:
    result = subprocess.run(["git", "check-ignore", "--no-index", "-q", name], cwd=root)
    if result.returncode:
        failures.append((name, "private path is not ignored"))

for name, reason in failures:
    print(f"FAIL: {name}: {reason}")
if failures:
    sys.exit(1)
print(f"PASS: {len(files)} publishable files checked; private paths ignored; no known credentials found.")
