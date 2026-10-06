import { useState } from "react";
import {
  ArrowUpRight,
  Clock3,
  Gauge,
  Layers3,
  Radio,
  ChevronRight,
} from "lucide-react";
import type { DashboardData } from "../data";
import type { FeeBucket, SizeClass } from "../../../sdk/ts/src/index";
import {
  age,
  compact,
  dateTime,
  integer,
  latestCurves,
  percent,
  ratio,
  routeNames,
  sol,
} from "../format";
import { Header, Panel, Status, Empty, EvidenceMeta } from "../ui";
import { LandingChart } from "../charts";
export function Cockpit({ data }: { data: DashboardData }) {
  const [horizon, setHorizon] = useState(4),
    [size, setSize] = useState<SizeClass>("small"),
    [fee, setFee] = useState<FeeBucket>("local_median");
  const all = latestCurves(data.curves.curves),
    curves = all.filter(
      (c) =>
        c.horizon_slots === horizon &&
        c.config.size_class === size &&
        c.config.fee_bucket === fee,
    );
  const best = [...curves]
    .filter((c) => c.evidence !== "INSUFFICIENT")
    .sort((a, b) => b.p_hat - a.p_hat)[0];
  const spent = Object.values(
    data.health.budget_reserved_today_by_route,
  ).reduce((sum, n) => sum + BigInt(n), 0n);
  const cap = BigInt(data.evidence.daily_cap_lamports),
    usage = Math.min(100, ratio(spent, cap) * 100);
  const leaders = data.health.leaders as {
    status?: string;
    at_slot?: string;
    epoch?: string;
  };
  return (
    <>
      <Header
        eyebrow="OBSERVATORY / 01"
        title="Network cockpit"
        description="Landing evidence, current conditions and the cost of each send."
      >
        <a className="button primary" href="#quote">
          Get a quote
          <ArrowUpRight size={16} />
        </a>
      </Header>
      <div className="metrics">
        <section>
          <span className="metric-label">
            <Clock3 size={15} />
            Slot duration
          </span>
          <div className="metric-value">
            {data.clock.mean_slot_ms?.toFixed(0) ?? "—"}
            <span>ms</span>
          </div>
          <p>
            {data.health.source === "sim"
              ? "Seeded simulation"
              : "Rolling measured clock"}{" "}
            · {data.clock.window.sampled_slots} sampled slots
          </p>
        </section>
        <section>
          <span className="metric-label">
            <Gauge size={15} />
            Best supported cell
          </span>
          <div className="metric-value">
            {percent(best?.p_hat)}
            <span>within {horizon} slots</span>
          </div>
          {best ? (
            <EvidenceMeta
              interval={best.p_interval_95}
              n={best.n_effective}
              seconds={best.data_age_s}
            />
          ) : (
            <p>No supported estimate for this selection</p>
          )}
        </section>
        <section>
          <span className="metric-label">
            <Layers3 size={15} />
            Owned canaries
          </span>
          <div className="metric-value">
            {integer(data.health.counts.canaries)}
          </div>
          <p>
            {data.health.source === "sim"
              ? "Synthetic outcomes · zero chain sends"
              : "Source-scoped records · includes unresolved"}
          </p>
        </section>
        <section>
          <span className="metric-label">
            <Radio size={15} />
            Observer state
          </span>
          <div className="metric-value small-value">
            {data.health.status === "SIMULATED"
              ? "Simulated"
              : data.health.status === "PASS"
                ? "Healthy"
                : data.health.status === "REPLAY"
                  ? "Replay"
                  : "Degraded"}
          </div>
          <p>
            {integer(data.health.counts.open_gaps)} open gaps ·{" "}
            <a href="#health">Inspect health ↗</a>
          </p>
        </section>
      </div>
      <div className="cockpit-grid">
        <Panel
          title="Landing curves"
          caption="M0 cell estimates · 95% intervals · owned canary evidence"
          action={<span className="tiny mono">{data.curves.regime_id}</span>}
        >
          <div className="chart-controls">
            <label>
              Within
              <select
                value={horizon}
                onChange={(e) => setHorizon(Number(e.target.value))}
              >
                {[1, 2, 4].map((h) => (
                  <option key={h} value={h}>
                    {h} slots
                    {data.clock.mean_slot_ms
                      ? ` · ${Math.round(h * data.clock.mean_slot_ms)} ms`
                      : ""}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Size
              <select
                value={size}
                onChange={(e) => setSize(e.target.value as SizeClass)}
              >
                <option value="small">Small canary</option>
                <option value="medium">Medium canary</option>
                <option value="large">Large canary</option>
              </select>
            </label>
            <label>
              Priority fee
              <select
                value={fee}
                onChange={(e) => setFee(e.target.value as FeeBucket)}
              >
                <option value="local_median">Local median</option>
                <option value="zero">Zero priority fee</option>
                <option value="local_p90">Local p90</option>
              </select>
            </label>
          </div>
          <LandingChart curves={curves} />
        </Panel>
        <div className="side-panels">
          <Panel
            title="Leader runway"
            caption="Epoch schedule and measured classifications"
            action={<Clock3 size={16} aria-hidden="true" />}
          >
            {data.evidence.runway.length ? (
              <div className="runway">
                {data.evidence.runway.map(({ slot, class: l }) => (
                  <div key={slot}>
                    <span className="runway-index mono">{slot}</span>
                    <div>
                      <strong className="mono">{compact(l.leader, 5)}</strong>
                      <span>
                        Stake {l.stake_tercile} · skip {l.skip_rate_tercile}
                      </span>
                    </div>
                    <ChevronRight size={14} />
                  </div>
                ))}
                <p className="tiny">
                  Epoch {leaders.epoch} · observed at slot {leaders.at_slot}.
                  Next assigned slots within the recorded epoch.
                </p>
              </div>
            ) : (
              <Empty
                title="No leader schedule"
                text={
                  data.health.source === "sim"
                    ? "Sim mode does not request a mainnet schedule."
                    : "Waiting for a fresh epoch schedule."
                }
                icon={Clock3}
              />
            )}
          </Panel>
          <Panel
            title="Daily reservation budget"
            caption={`${data.health.as_of_utc.slice(0, 10)} · UTC`}
          >
            <div className="spend-body">
              <div className="spend-value mono">
                {sol(spent)}
                <span>/ {sol(cap)} SOL</span>
              </div>
              <div
                className="meter"
                role="progressbar"
                aria-label="Daily reserved budget"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={Math.round(usage)}
              >
                <div style={{ width: `${usage}%` }} />
              </div>
              <div className="spend-caption">
                <span>{usage.toFixed(1)}% reserved</span>
                <span>{sol(cap > spent ? cap - spent : 0n)} SOL remaining</span>
              </div>
              <p>
                Worst-case reservations include uncertain sends.{" "}
                {data.health.source === "sim"
                  ? "Simulated funds only."
                  : "Charged before signing."}
              </p>
            </div>
          </Panel>
        </div>
      </div>
      <Panel
        title="Canary activity"
        caption={`Latest ${data.evidence.canaries.length} of ${integer(data.health.counts.canaries)} source-scoped records`}
        action={
          <a className="text-link" href="#health">
            Inspect evidence
            <ArrowUpRight size={14} />
          </a>
        }
      >
        {data.evidence.canaries.length ? (
          <>
            <div className="canary-strip" aria-label="Recent canary outcomes">
              {[...data.evidence.canaries]
                .reverse()
                .map(({ canary: c, finalized }) => (
                  <a
                    href={`#health/${encodeURIComponent(c.id)}`}
                    key={c.id}
                    className={`canary-dot ${finalized && c.outcome === "LANDED_OK" ? "landed" : finalized && c.outcome && c.outcome !== "UNRESOLVED" ? "failed" : "pending"}`}
                    aria-label={`${c.id}, ${c.outcome ?? "PENDING"}, ${routeNames[c.config.route]}, ${integer(c.config.tip_lamports)} lamports`}
                    title={`${c.id} · ${c.outcome ?? "PENDING"} · ${routeNames[c.config.route]} · ${integer(c.config.tip_lamports)} lamports · ${c.landed_slot != null && BigInt(c.landed_slot) >= BigInt(c.sent_slot) ? `${BigInt(c.landed_slot) - BigInt(c.sent_slot)} slots to landing` : "Landing latency unavailable"}`}
                  />
                ))}
            </div>
            <div className="dot-legend">
              <span>
                <i className="landed" />
                Finalized success
              </span>
              <span>
                <i className="failed" />
                Finalized adverse outcome
              </span>
              <span>
                <i className="pending" />
                Pending / unresolved
              </span>
            </div>
            <div className="table-wrap">
              <table>
                <caption className="sr-only">Latest canary activity</caption>
                <thead>
                  <tr>
                    <th>Canary</th>
                    <th>Route</th>
                    <th>Tip · lamports</th>
                    <th>Outcome</th>
                    <th>Sent · UTC</th>
                    <th>Population</th>
                  </tr>
                </thead>
                <tbody>
                  {data.evidence.canaries
                    .slice(0, 6)
                    .map(({ canary: c, prove_id }) => (
                      <tr key={c.id}>
                        <td>
                          <a
                            className="mono row-link"
                            href={`#health/${encodeURIComponent(c.id)}`}
                          >
                            {compact(c.id, 6)}
                          </a>
                        </td>
                        <td>{routeNames[c.config.route]}</td>
                        <td className="mono">
                          {integer(c.config.tip_lamports)}
                        </td>
                        <td>
                          <Status value={c.outcome ?? "PENDING"} />
                        </td>
                        <td className="muted">{dateTime(c.send_wall_utc)}</td>
                        <td>{prove_id ? "Held out · Prove" : "Exploration"}</td>
                      </tr>
                    ))}
                </tbody>
              </table>
            </div>
          </>
        ) : (
          <Empty
            title="No owned canaries"
            text="Observed market transactions do not count as owned landing evidence."
          />
        )}
      </Panel>
      <div className="page-foot">
        <span>As of {dateTime(data.health.as_of_utc)} UTC</span>
        <span>
          {age(best?.data_age_s)} · region {data.health.region} · Canary results
          do not establish swap performance.
        </span>
      </div>
    </>
  );
}
