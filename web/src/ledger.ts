import {
  AlightClient,
  decimalU64,
  type BrowserLedgerRow,
  type Source,
} from "../../sdk/ts/src/index";
import { verifyBundle, type LedgerBundle } from "./verification";
export { verifyBundle, type LedgerBundle } from "./verification";
const genesis = `sha256:${"0".repeat(64)}`;
export async function loadBundle(
  client: AlightClient,
  source: Source,
): Promise<LedgerBundle> {
  const anchor = await client.verifyLedger();
  if (BigInt(anchor.entries) > 10000n)
    throw new Error("Ledger exceeds browser verification limit");
  const rows: BrowserLedgerRow[] = [];
  let after = decimalU64("0"),
    bytes = 0;
  while (BigInt(after) < BigInt(anchor.entries)) {
    const page = await client.ledgerPayloads(after);
    const next = page.rows.filter(
      (r) => BigInt(r.entry.sequence) <= BigInt(anchor.entries),
    );
    if (!next.length) throw new Error("Ledger page missing");
    rows.push(...next);
    bytes += JSON.stringify(next).length;
    if (rows.length > 10000 || bytes > 64 * 1024 * 1024)
      throw new Error("Ledger export exceeds browser limit");
    const cursor = next.at(-1)!.entry.sequence;
    if (BigInt(cursor) <= BigInt(after))
      throw new Error("Ledger cursor did not advance");
    after = cursor;
  }
  const bundle: LedgerBundle = {
    schema_version: 1,
    kind: "alight.browser-ledger.v1",
    source,
    head_sequence: anchor.entries,
    head_hash: rows.at(-1)?.entry.hash ?? genesis,
    rows,
  };
  return verifyBundle(bundle, source);
}
