import type {
  CanaryConfig,
  CurveSnapshot,
  DecimalU64,
  PredictionTarget,
  TrainingCanary,
} from "../../sdk/ts/src/index";
export const routeNames = {
  beam_http: "Beam HTTP",
  beam_quic: "Beam QUIC",
  rpc: "Solami RPC",
};
/** Friendly display only; evidence identifiers remain unchanged in contracts and exports. */
export const regimeName = (id: string) =>
  /^phase\d+-unclassified$/.test(id) ? "Unclassified conditions" : id;
export const percent = (value: number | null | undefined) =>
  value == null ? "—" : `${(value * 100).toFixed(1)}%`;
export const compact = (text: string, length = 8) =>
  text.length > length * 2 + 1
    ? `${text.slice(0, length)}…${text.slice(-length)}`
    : text;
export const canaryName = (id: string, length = 8) =>
  compact(id.replace(/^phase\d+-/, ""), length);
export const dateTime = (text: string) =>
  new Date(text).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    timeZone: "UTC",
  });
export const age = (seconds: number | null | undefined) =>
  seconds == null
    ? "Age unavailable"
    : seconds < 60
      ? `${seconds.toFixed(1)}s old`
      : seconds < 3600
        ? `${Math.floor(seconds / 60)}m old`
        : `${(seconds / 3600).toFixed(1)}h old`;
export const integer = (n: string | bigint) => BigInt(n).toLocaleString();
/** Exact decimal formatting; chain amounts are never converted to a floating-point Number. */
export function sol(lamports: string | bigint, decimals = 6): string {
  const value = BigInt(lamports),
    whole = value / 1000000000n;
  const fraction = (value % 1000000000n)
    .toString()
    .padStart(9, "0")
    .slice(0, decimals)
    .replace(/0+$/, "");
  return `${whole}${fraction ? `.${fraction}` : ""}`;
}
export const nominal = (config: CanaryConfig): bigint =>
  BigInt(config.tip_lamports) +
  (BigInt(config.cu_price_micro_lamports) * BigInt(config.cu_limit) + 999999n) /
    1000000n +
  5000n;
export const ratio = (n: bigint, d: bigint) =>
  d === 0n ? 0 : Number((n * 10000n) / d) / 10000;
export function cellKey(config: CanaryConfig): string {
  return [
    config.route,
    config.size_class,
    config.cu_limit,
    config.fee_bucket,
    config.tip_tier,
  ].join(":");
}
export function latestCurves(curves: CurveSnapshot[]): CurveSnapshot[] {
  const cells = new Map<string, CurveSnapshot>();
  for (const curve of curves) {
    const key = `${cellKey(curve.config)}:${curve.horizon_slots}`;
    const old = cells.get(key);
    if (
      !old ||
      Date.parse(curve.context.as_of_utc) > Date.parse(old.context.as_of_utc)
    )
      cells.set(key, curve);
  }
  return [...cells.values()];
}
export function sendToSeenMs(
  send: { clock_id: string; send_mono_ns: DecimalU64 },
  received: { clock_id: string; mono_ns: DecimalU64 },
): number | null {
  if (send.clock_id !== received.clock_id) return null;
  const delta = BigInt(received.mono_ns) - BigInt(send.send_mono_ns);
  return delta < 0n ? null : Number(delta / 1000n) / 1000;
}

/** Display target timing separately from execution; the API report remains the verdict. */
export function proveMemberDisplay(
  { canary: c, finalized }: TrainingCanary,
  target: PredictionTarget,
  asOfUtc: string,
): { state: "met" | "late" | "adverse" | "pending"; label: string } {
  if (
    !finalized ||
    !c.resolved_at_utc ||
    Date.parse(c.resolved_at_utc) > Date.parse(asOfUtc)
  )
    return { state: "pending", label: "Pending / unresolved" };
  if (["EXPIRED", "REJECTED", "LANDED_THEN_DROPPED"].includes(c.outcome ?? ""))
    return { state: "adverse", label: `Did not meet target · ${c.outcome}` };
  if (!["LANDED_OK", "LANDED_FAILED"].includes(c.outcome ?? ""))
    return { state: "pending", label: "Pending / unresolved" };
  if (c.landed_slot == null || BigInt(c.landed_slot) < BigInt(c.sent_slot))
    return { state: "pending", label: "Target timing unavailable" };
  const slots = BigInt(c.landed_slot) - BigInt(c.sent_slot);
  let met: boolean;
  let timing: string;
  if (target.kind === "probability" || target.max_slots != null) {
    const bound =
      target.kind === "probability" ? target.horizon_slots : target.max_slots!;
    met = Number(slots) <= bound;
    timing = `${slots} slots`;
  } else if (target.max_ms != null) {
    const times = Object.values(c.observer_first_seen)
      .filter(
        (received) =>
          received.clock_id === c.clock_id &&
          BigInt(received.mono_ns) >= BigInt(c.send_mono_ns),
      )
      .map(
        (received) =>
          Number(BigInt(received.mono_ns) - BigInt(c.send_mono_ns)) / 1e6,
      );
    if (!times.length)
      return { state: "pending", label: "Target timing unavailable" };
    const ms = Math.min(...times);
    met = ms <= target.max_ms;
    timing = `${ms.toFixed(3)} ms`;
  } else return { state: "pending", label: "Target timing unavailable" };
  return {
    state: met ? "met" : "late",
    label: `${met ? "Met landing target" : "Landed after target"} · ${c.outcome} · ${timing}`,
  };
}
