import { useEffect, useState } from "react";
import { ArrowRight, FlaskConical, LockKeyhole } from "lucide-react";
import {
  AlightClient,
  decimalU64,
  type ForecastEntry,
  type ProveReport,
  type TrainingCanary,
} from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { compact, dateTime, integer, percent, routeNames } from "../format";
import { loadBundle } from "../ledger";
import {
  Empty,
  EvidenceMeta,
  Header,
  Loading,
  Notice,
  Panel,
  Status,
} from "../ui";
export function Prove({
  data,
  operator,
  connect,
  initial,
  reference,
}: {
  data: DashboardData;
  operator: AlightClient | null;
  connect: () => void;
  initial: ForecastEntry | null;
  reference?: string;
}) {
  const [forecast, setForecast] = useState(
    reference && initial?.hash !== reference ? null : initial,
  ),
    [report, setReport] = useState<ProveReport | null>(null),
    [members, setMembers] = useState<TrainingCanary[]>([]),
    [n, setN] = useState(40),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [requestId, setRequestId] = useState("");
  useEffect(() => {
    let active = true;
    if (!reference) return;
    const start = async () => {
      try {
        if (reference.startsWith("sha256:")) {
          if (initial?.hash === reference) return;
          const bundle = await loadBundle(data.client, data.health.source);
          if (active)
            setForecast(
              bundle.rows.find((r) => r.entry.hash === reference)?.entry ??
                null,
            );
        } else {
          const r = await data.client.proveReport(reference);
          if (active) {
            setReport(r);
            setForecast(null);
          }
        }
      } catch (e) {
        if (active) setError(errorText(e));
      }
    };
    void start();
    return () => {
      active = false;
    };
  }, [reference, initial, data.client, data.health.source]);
  const reportId = report?.lock.id;
  useEffect(() => {
    if (!reportId) return;
    let active = true,
      timer: ReturnType<typeof setTimeout>;
    const id = reportId;
    const refresh = async () => {
      try {
        const [r, rows] = await Promise.all([
          data.client.proveReport(id),
          data.client.proveCanaries(id),
        ]);
        if (!active) return;
        setReport(r);
        setMembers(rows.canaries);
        if (!["COMPLETE", "VOIDED"].includes(r.state))
          timer = setTimeout(refresh, 2000);
      } catch (e) {
        if (active) setError(errorText(e));
      }
    };
    void refresh();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [data.client, reportId]);
  const run = async () => {
    if (!operator || !forecast || busy) return;
    const id = `web-${crypto.randomUUID()}`;
    setRequestId(id);
    setBusy(true);
    setError("");
    try {
      const r = await operator.prove({
        request_id: id,
        forecast_hash: forecast.hash,
        n,
        seed: data.health.source === "sim" ? decimalU64("42") : null,
      });
      setReport(r);
      location.hash = `prove/${encodeURIComponent(r.lock.id)}`;
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const prediction = forecast?.forecast.quote.recommendation;
  return (
    <>
      <Header
        eyebrow="VERIFICATION / 03"
        title="Prove a claim"
        description="Freeze one configuration. Test it against later, held-out canaries."
      >
        <a href="#quote" className="button">
          New quote
          <ArrowRight size={15} />
        </a>
      </Header>
      {error && (
        <Notice danger>
          {error}
          {requestId && (
            <p className="mono">
              Request {requestId}. Inspect run history before repeating.
            </p>
          )}
        </Notice>
      )}
      {report ? (
        <>
          <div className="prove-summary">
            <Panel title="Locked claim" action={<LockKeyhole size={16} />}>
              <div className="claim-body">
                <span className="eyebrow">FROZEN PROBABILITY</span>
                <div className="claim-value mono">
                  {percent(report.lock.claimed_probability)}
                </div>
                <p>
                  {report.lock.target.kind === "probability"
                    ? `Within ${report.lock.target.horizon_slots} slots`
                    : "Latency target"}{" "}
                  · {routeNames[report.lock.config.route]} ·{" "}
                  {report.lock.config.size_class}
                </p>
                <dl>
                  <div>
                    <dt>Tip</dt>
                    <dd className="mono">
                      {integer(report.lock.config.tip_lamports)} lamports
                    </dd>
                  </div>
                  <div>
                    <dt>Model</dt>
                    <dd
                      className="mono"
                      title={report.lock.model_snapshot_hash}
                    >
                      {compact(report.lock.model_snapshot_hash)}
                    </dd>
                  </div>
                  <div>
                    <dt>Regime</dt>
                    <dd>{report.lock.regime_id}</dd>
                  </div>
                  <div>
                    <dt>Locked · UTC</dt>
                    <dd>{dateTime(report.lock.locked_at_utc)}</dd>
                  </div>
                </dl>
              </div>
            </Panel>
            <Panel
              title="Held-out result"
              action={<Status value={report.state} />}
            >
              <div className="result-body">
                <div className="claim-value mono">
                  {percent(report.observed_rate)}
                </div>
                <p>
                  {report.successes} successes / {report.resolved} resolved ·{" "}
                  {report.unresolved} unresolved
                </p>
                {report.wilson_interval_95 ? (
                  <div className="wilson">
                    <div className="wilson-bar">
                      <span
                        style={{
                          left: `${report.wilson_interval_95[0] * 100}%`,
                          width: `${(report.wilson_interval_95[1] - report.wilson_interval_95[0]) * 100}%`,
                        }}
                      />
                      <i
                        style={{
                          left: `${report.lock.claimed_probability * 100}%`,
                        }}
                      />
                    </div>
                    <div>
                      <span>
                        95% Wilson {percent(report.wilson_interval_95[0])}–
                        {percent(report.wilson_interval_95[1])}
                      </span>
                      <span>┃ Frozen claim</span>
                    </div>
                  </div>
                ) : (
                  <p>
                    Waiting for enough resolved outcomes to estimate an
                    interval.
                  </p>
                )}
                <div className="verdict-row">
                  <Status value={report.verdict} />
                  <span>
                    {report.verdict === "CONSISTENT"
                      ? "The interval contains the frozen claim."
                      : report.verdict === "INCONSISTENT"
                        ? "The interval excludes the frozen claim."
                        : "The run cannot support a conclusive comparison."}
                  </span>
                </div>
                {report.reason && <p>{report.reason.replaceAll("_", " ")}</p>}
              </div>
            </Panel>
          </div>
          <Panel
            title="Prospective canary members"
            caption={`${report.attempts} / ${report.lock.n} attempts · exact configuration · excluded from model training`}
          >
            <div className="prove-members">
              {members.length ? (
                members.map(({ canary: c, finalized }, i) => (
                  <a
                    key={c.id}
                    href={`#health/${encodeURIComponent(c.id)}`}
                    className={`member ${finalized && c.outcome === "LANDED_OK" ? "landed" : finalized && c.outcome && c.outcome !== "UNRESOLVED" ? "failed" : "pending"}`}
                    title={`${i + 1} · ${c.id} · ${c.outcome ?? "PENDING"}`}
                    aria-label={`Member ${i + 1}, ${c.outcome ?? "PENDING"}, inspect evidence`}
                  >
                    <span>{i + 1}</span>
                    <strong>
                      {c.outcome === "LANDED_OK" && finalized
                        ? "✓"
                        : finalized && c.outcome !== "UNRESOLVED"
                          ? "×"
                          : "○"}
                    </strong>
                  </a>
                ))
              ) : (
                <Empty
                  title="No members yet"
                  text="Dots appear only when a canary has been durably assigned to this run."
                />
              )}
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
          </Panel>
          <Notice>
            {report.lock.source === "sim"
              ? `SIMULATED · seed ${report.lock.seed}. No chain transactions were sent.`
              : "Live members are sent only through the durable budget governor."}{" "}
            An inconclusive or unfavorable result remains visible.
          </Notice>
          <a className="button primary" href="#ledger">
            Verify forecast ledger
            <ArrowRight size={15} />
          </a>
        </>
      ) : prediction && forecast ? (
        <Panel
          title="Forecast ready to lock"
          action={<Status value={forecast.forecast.quote.evidence} />}
        >
          <div className="prove-setup">
            <div>
              <div className="claim-value mono">
                {percent(prediction.p_hat)}
              </div>
              <EvidenceMeta
                interval={prediction.p_interval_95}
                n={prediction.n_effective}
                seconds={prediction.data_age_s}
              />
              <p>
                {routeNames[prediction.config.route]} ·{" "}
                {integer(prediction.config.tip_lamports)} lamports ·{" "}
                {forecast.forecast.regime_id}
              </p>
              <p className="mono hash">{forecast.hash}</p>
              <p>
                Forecast expires {dateTime(forecast.forecast.expires_at_utc)}{" "}
                UTC.
              </p>
            </div>
            <div className="prove-run">
              <label>
                Held-out canaries
                <input
                  type="number"
                  min="1"
                  max="400"
                  value={n}
                  onChange={(e) => setN(Number(e.target.value))}
                />
              </label>
              <p>
                At least 30 resolved members are needed for a conclusive
                verdict. Every attempt remains subject to daily and burst caps.
              </p>
              <button
                className="primary"
                disabled={
                  busy ||
                  !Number.isInteger(n) ||
                  n < 1 ||
                  n > 400 ||
                  !data.evidence.operator_enabled
                }
                onClick={() => (operator ? void run() : connect())}
              >
                <FlaskConical size={16} />
                {busy
                  ? "Running…"
                  : operator
                    ? `Run ${n} canaries`
                    : "Connect operator to run"}
              </button>
              {busy && <Loading text="Waiting for the governed run…" />}
            </div>
          </div>
        </Panel>
      ) : (
        <Panel title="Start with a supported quote">
          <Empty
            title="No claim selected"
            text="Compute a quote and lock its forecast, or inspect a previous Prove run."
            icon={FlaskConical}
          >
            <a href="#quote" className="button primary">
              Open quote console
              <ArrowRight size={15} />
            </a>
          </Empty>
        </Panel>
      )}
      <Panel
        title="Run history"
        caption="Latest source-scoped runs · select a run to recover its state"
      >
        {data.proves.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Run</th>
                  <th>State</th>
                  <th>Resolved</th>
                  <th>Verdict</th>
                  <th>Locked · UTC</th>
                </tr>
              </thead>
              <tbody>
                {data.proves.map((r) => (
                  <tr key={r.lock.id}>
                    <td>
                      <a
                        className="row-link mono"
                        href={`#prove/${encodeURIComponent(r.lock.id)}`}
                      >
                        {compact(r.lock.id, 8)}
                      </a>
                    </td>
                    <td>
                      <Status value={r.state} />
                    </td>
                    <td>
                      {r.resolved}/{r.lock.n}
                    </td>
                    <td>
                      <Status value={r.verdict} />
                    </td>
                    <td>{dateTime(r.lock.locked_at_utc)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No Prove runs"
            text="A completed run is durable and can be inspected after a refresh."
          />
        )}
      </Panel>
    </>
  );
}
