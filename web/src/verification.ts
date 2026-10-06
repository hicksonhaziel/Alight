import type { BrowserLedgerRow, Source } from "../../sdk/ts/src/types.ts";
import { schemas } from "../../sdk/ts/src/schemas.ts";
import { matches } from "../../sdk/ts/src/validation.ts";
export interface LedgerBundle {
  schema_version: 1;
  kind: "alight.browser-ledger.v1";
  source: Source;
  head_sequence: string;
  head_hash: string;
  rows: BrowserLedgerRow[];
}
const genesis = `sha256:${"0".repeat(64)}`;
const same = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;
  if (
    !a ||
    !b ||
    typeof a !== "object" ||
    typeof b !== "object" ||
    Array.isArray(a) !== Array.isArray(b)
  )
    return false;
  const x = a as Record<string, unknown>,
    y = b as Record<string, unknown>;
  return (
    Object.keys(x).length === Object.keys(y).length &&
    Object.keys(x).every((k) => Object.hasOwn(y, k) && same(x[k], y[k]))
  );
};
/** Hash the exact UTF-8 ledger payload; compare its parsed content with the displayed forecast. */
export async function verifyBundle(
  value: unknown,
  expectedSource: Source,
): Promise<LedgerBundle> {
  if (!value || typeof value !== "object") throw new Error("Invalid export");
  const bundle = value as LedgerBundle;
  if (
    bundle.schema_version !== 1 ||
    bundle.kind !== "alight.browser-ledger.v1" ||
    bundle.source !== expectedSource ||
    !Array.isArray(bundle.rows) ||
    bundle.rows.length > 10000
  )
    throw new Error("Invalid export source or shape");
  if (
    typeof bundle.head_sequence !== "string" ||
    !/^(0|[1-9][0-9]{0,19})$/.test(bundle.head_sequence) ||
    BigInt(bundle.head_sequence) > 18446744073709551615n
  )
    throw new Error("Invalid head sequence");
  let previous = genesis,
    sequence = 0n,
    bytes = 0;
  for (const row of bundle.rows) {
    if (!matches(schemas.BrowserLedgerRow, row, schemas))
      throw new Error("Invalid ledger row");
    sequence++;
    bytes += new TextEncoder().encode(row.canonical_json).length;
    if (
      bytes > 64 * 1024 * 1024 ||
      row.entry.sequence !== sequence.toString() ||
      row.entry.prev_hash !== previous ||
      row.entry.forecast.source !== expectedSource ||
      !same(JSON.parse(row.canonical_json), row.entry.forecast)
    )
      throw new Error("Payload, source or chain link mismatch");
    const input = `alight.forecast.v1\0${expectedSource}\0${sequence}\0${previous}\0${row.canonical_json}`;
    const digest = await crypto.subtle.digest(
      "SHA-256",
      new TextEncoder().encode(input),
    );
    const hash = `sha256:${[...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("")}`;
    if (hash !== row.entry.hash) throw new Error("Forecast hash mismatch");
    previous = hash;
  }
  if (
    sequence.toString() !== bundle.head_sequence ||
    previous !== bundle.head_hash
  )
    throw new Error("Export head mismatch");
  return bundle;
}
