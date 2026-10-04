#!/usr/bin/env python3
"""Read-only Phase 0 schema capture: three actively traded pools, prices as text."""
from datetime import datetime, timezone
from decimal import Decimal
from pathlib import Path
import json
import os
import shlex
import time
import urllib.error
import urllib.parse
import urllib.request


def environment():
    values = {}
    if Path('.env').exists():
        for line in Path('.env').read_text().splitlines():
            parts = shlex.split(line, comments=True)
            if parts and parts[0] == 'export':
                parts = parts[1:]
            if len(parts) == 1 and '=' in parts[0]:
                key, value = parts[0].split('=', 1)
                values[key] = value
    values.update(os.environ)
    return values


def main():
    env = environment()
    base = env.get('SOLAMI_BLUR_URL', '').rstrip('/')
    token = env.get('SOLAMI_BLUR_TOKEN', '')
    if not token or urllib.parse.urlsplit(base).scheme != 'https':
        print('Blur spike: missing or invalid configuration; values withheld.')
        return 2
    secret_values = [v for k, v in env.items() if k.startswith(('ALIGHT_', 'SOLAMI_'))
                     and any(s in k for s in ('TOKEN', 'KEY', 'SECRET')) and len(v) >= 8]

    def get(path, params):
        request = urllib.request.Request(base + path + '?' + urllib.parse.urlencode(params),
                                         headers={'x-api-key': token})
        started = time.monotonic()
        with urllib.request.urlopen(request, timeout=12) as response:
            raw = response.read(512 * 1024 + 1)
        if len(raw) > 512 * 1024:
            raise ValueError('byte cap')
        text = raw.decode()
        for secret in secret_values:
            text = text.replace(secret, '[REDACTED]')
        # Python keeps raw u64 amounts exact; decimal price strings remain strings.
        return json.loads(text), round((time.monotonic() - started) * 1000)

    receipt = {'schema_version': 1, 'source': 'live', 'canaries_sent': 0,
               'started_at': datetime.now(timezone.utc).isoformat(), 'pools': []}
    try:
        pools, elapsed = get('/data/pools', {'chain': 'solana', 'limit': 12,
            'sort': 'volume_usd', 'order': 'desc', 'dex': 'raydium_clmm,orca_whirlpool,pumpswap'})
        selected = [p for p in pools if p.get('pool') and p.get('mint')
                    and p.get('trades', 0) >= 100 and Decimal(p.get('tvl_usd', '0')) >= 100_000][:3]
        receipt['selection'] = {'sort': 'volume_usd', 'min_tvl_usd': '100000', 'min_daily_trades': 100,
                                'elapsed_ms': elapsed, 'candidate_count': len(pools)}
        for pool in selected:
            params = {'chain': 'solana', 'address': pool['mint'], 'pool': pool['pool']}
            trades, trades_ms = get('/data/token/trades', {**params, 'limit': 3})
            candles, candles_ms = get('/data/token/ohlcv', {**params, 'interval': '1m', 'count': 3})
            receipt['pools'].append({'pool_metadata': pool, 'trades': trades, 'candles': candles,
                                    'trades_elapsed_ms': trades_ms, 'candles_elapsed_ms': candles_ms})
        receipt['verdict'] = 'PASS' if len(selected) == 3 and all(
            p['trades'] and p['candles'] for p in receipt['pools']) else 'INCONCLUSIVE'
    except urllib.error.HTTPError as error:
        receipt.update(verdict='FAIL', http_status=error.code)
    except Exception as error:
        receipt.update(verdict='FAIL', error_category=type(error).__name__)
    Path('data/fixtures').mkdir(parents=True, exist_ok=True)
    Path('data/fixtures/blur_sample.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'verdict': receipt['verdict'], 'pools_captured': len(receipt['pools']),
                      'canaries_sent': 0, 'fixture': 'data/fixtures/blur_sample.json'}))
    return 0 if receipt['verdict'] == 'PASS' else 1 if receipt['verdict'] == 'FAIL' else 3


if __name__ == '__main__':
    raise SystemExit(main())
