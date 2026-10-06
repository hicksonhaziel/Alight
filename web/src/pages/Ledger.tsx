import { useEffect, useRef, useState } from "react";
import {
  Download,
  ShieldCheck,
  Upload,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { compact, dateTime, percent, routeNames } from "../format";
import { loadBundle, verifyBundle, type LedgerBundle } from "../ledger";
import {
  CopyButton,
  Empty,
  Header,
  Loading,
  Notice,
  Panel,
  Status,
} from "../ui";
export function Ledger({ data }: { data: DashboardData }) {
  const [bundle, setBundle] = useState<LedgerBundle | null>(null),
    [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [verified, setVerified] = useState(""),
    [query, setQuery] = useState(""),
    [status, setStatus] = useState("all"),
    [page, setPage] = useState(0),
    [selected, setSelected] = useState("");
  const input = useRef<HTMLInputElement>(null);
  const grades = new Map(data.evidence.grades.map((g) => [g.forecast_hash, g]));
  useEffect(() => {
    let active = true;
    setBusy(true);
    void loadBundle(data.client, data.health.source)
      .then((b) => {
        if (active) {
          setBundle(b);
          setError("");
        }
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      })
      .finally(() => {
        if (active) setBusy(false);
      });
    return () => {
      active = false;
    };
  }, [data.client, data.health.source]);
  const verify = async () => {
    setBusy(true);
    setError("");
    setVerified("");
    try {
      const b = await loadBundle(data.client, data.health.source);
      setBundle(b);
      setVerified(
        `Verified ${b.rows.length} forecasts in this browser · SHA-256 chain intact`,
      );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const exportBundle = () => {
    if (!bundle) return;
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" }),
    );
    const a = document.createElement("a");
    a.href = url;
    a.download = `alight-ledger-${bundle.source}-${bundle.head_sequence}.json`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const importBundle = async (file: File | undefined) => {
    if (!file) return;
    setBusy(true);
    setError("");
    setVerified("");
    try {
      if (file.size > 64 * 1024 * 1024)
        throw new Error("Export exceeds 64 MiB");
      const b = await verifyBundle(
        JSON.parse(await file.text()),
        data.health.source,
      );
      setVerified(
        `Verified uploaded export · ${b.rows.length} forecasts · ${compact(b.head_hash)}. This proves internal integrity; it does not establish an external anchor or current server head.`,
      );
    } catch {
      setError(
        "Export verification failed. The source, payload, chain links or declared head do not match.",
      );
    } finally {
      setBusy(false);
      if (input.current) input.current.value = "";
    }
  };
  const rows = [...(bundle?.rows ?? [])]
    .reverse()
    .filter(
      (r) =>
        (!query ||
          `${r.entry.hash} ${r.entry.forecast.id} ${r.entry.forecast.regime_id}`
            .toLowerCase()
            .includes(query.toLowerCase())) &&
        (status === "all" ||
          (grades.get(r.entry.hash)?.status ?? "PENDING") === status),
    );
  const pages = Math.max(1, Math.ceil(rows.length / 10)),
    current = Math.min(page, pages - 1),
    visible = rows.slice(current * 10, current * 10 + 10);
  const chosen =
    data.evidence.grades.find((g) => g.forecast_hash === selected) ??
    data.evidence.grades.find((g) => g.scores) ??
    data.evidence.grades[0];
  return (
    <>
      <Header
        eyebrow="AUDIT / 04"
        title="Forecast ledger"
        description="Immutable claims. Later outcomes. A chain you can verify yourself."
      >
        <button
          className="primary"
          onClick={() => void verify()}
          disabled={busy}
        >
          <ShieldCheck size={16} />
          {busy ? "Verifying…" : "Verify in browser"}
        </button>
      </Header>
      {error && <Notice danger>{error}</Notice>}
      {verified && (
        <div className="verification-success" role="status">
          <ShieldCheck size={18} />
          {verified}
        </div>
      )}
      <div className="ledger-head">
        <div>
          <span className="eyebrow">CURRENT LOADED HEAD</span>
          <strong className="mono hash">
            {bundle?.head_hash ?? "Loading…"}
          </strong>
          <span className="tiny">
            {bundle?.head_sequence ?? "—"} forecasts ·{" "}
            {data.health.source.toUpperCase()} · persisted head checked by API
          </span>
        </div>
        <div>
          <button onClick={exportBundle} disabled={!bundle || busy}>
            <Download size={15} />
            Export
          </button>
          <button onClick={() => input.current?.click()} disabled={busy}>
            <Upload size={15} />
            Verify export
          </button>
          <input
            ref={input}
            className="sr-only"
            tabIndex={-1}
            type="file"
            accept="application/json,.json"
            aria-label="Ledger export file"
            onChange={(e) => void importBundle(e.target.files?.[0])}
          />
        </div>
      </div>
      <Panel
        title="Issued forecasts"
        caption="Latest status from the last 100 available forecast grades"
      >
        <div className="table-tools">
          <label>
            <span className="sr-only">Search forecasts</span>
            <input
              value={query}
              placeholder="Search hash, forecast or regime…"
              onChange={(e) => {
                setQuery(e.target.value);
                setPage(0);
              }}
            />
          </label>
          <label>
            <span className="sr-only">Forecast status</span>
            <select
              value={status}
              onChange={(e) => {
                setStatus(e.target.value);
                setPage(0);
              }}
            >
              <option value="all">All statuses</option>
              <option value="PENDING">Pending</option>
              <option value="SCORED">Scored</option>
              <option value="VOIDED">Voided</option>
            </select>
          </label>
        </div>
        {busy && !bundle ? (
          <Loading />
        ) : visible.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Sequence / hash</th>
                  <th>Claim</th>
                  <th>Route</th>
                  <th>Evidence</th>
                  <th>Status</th>
                  <th>Issued · UTC</th>
                  <th>
                    <span className="sr-only">Copy hash</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {visible.map(({ entry }) => {
                  const p = entry.forecast.quote.recommendation;
                  return (
                    <tr key={entry.hash}>
                      <td>
                        <button
                          className="hash-button mono"
                          onClick={() => setSelected(entry.hash)}
                        >
                          {entry.sequence} / {compact(entry.hash, 5)}
                        </button>
                      </td>
                      <td>
                        {percent(p?.p_hat)}
                        <span className="cell-detail">
                          {p
                            ? `95% ${percent(p.p_interval_95[0])}–${percent(p.p_interval_95[1])} · n ${p.n_effective.toFixed(1)} · ${p.data_age_s?.toFixed(1) ?? "—"}s old`
                            : "No supported recommendation"}
                        </span>
                      </td>
                      <td>{p ? routeNames[p.config.route] : "—"}</td>
                      <td>
                        <Status value={entry.forecast.quote.evidence} />
                      </td>
                      <td>
                        <Status
                          value={grades.get(entry.hash)?.status ?? "PENDING"}
                        />
                      </td>
                      <td>{dateTime(entry.forecast.created_at_utc)}</td>
                      <td>
                        <CopyButton text={entry.hash} label="Copy hash" />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title={
              query || status !== "all"
                ? "No matching forecasts"
                : "No issued forecasts"
            }
            text="Lock a supported quote to append its immutable claim to this ledger."
          />
        )}
        <div className="table-bottom">
          <span>
            {rows.length} forecasts · page {current + 1} of {pages}
          </span>
          <div>
            <button
              className="icon-button"
              aria-label="Previous page"
              disabled={current === 0}
              onClick={() => setPage(current - 1)}
            >
              <ChevronLeft size={16} />
            </button>
            <button
              className="icon-button"
              aria-label="Next page"
              disabled={current >= pages - 1}
              onClick={() => setPage(current + 1)}
            >
              <ChevronRight size={16} />
            </button>
          </div>
        </div>
      </Panel>
      <Panel
        title="Calibration and baselines"
        caption="Per-forecast held-out grading · score populations can overlap"
        action={
          data.evidence.grades.length ? (
            <label>
              <span className="sr-only">Scored forecast</span>
              <select
                value={chosen?.forecast_hash ?? ""}
                onChange={(e) => setSelected(e.target.value)}
              >
                {data.evidence.grades.map((g) => (
                  <option key={g.forecast_hash} value={g.forecast_hash}>
                    {compact(g.forecast_hash, 6)}
                  </option>
                ))}
              </select>
            </label>
          ) : undefined
        }
      >
        {chosen?.scores ? (
          <div className="calibration">
            <div>
              <div className="score-tiles">
                <div>
                  <span>Brier score</span>
                  <strong className="mono">
                    {chosen.scores.brier.toFixed(4)}
                  </strong>
                  <span>Lower is better · n {chosen.scores.n}</span>
                </div>
                <div>
                  <span>Log loss</span>
                  <strong className="mono">
                    {chosen.scores.log_loss.toFixed(4)}
                  </strong>
                  <span>Natural log · n {chosen.scores.n}</span>
                </div>
                <div>
                  <span>Calibration error</span>
                  <strong className="mono">
                    {chosen.scores.expected_calibration_error.toFixed(4)}
                  </strong>
                  <span>10 equal-width buckets</span>
                </div>
              </div>
              <svg
                className="reliability"
                viewBox="0 0 420 270"
                role="img"
                aria-label="Reliability diagram. Predicted probability versus observed frequency; dashed line is perfect calibration."
              >
                {[0, 0.5, 1].map((p) => (
                  <g key={p}>
                    <line
                      x1="50"
                      x2="370"
                      y1={225 - p * 195}
                      y2={225 - p * 195}
                      className="chart-grid"
                    />
                    <text x="39" y={229 - p * 195} textAnchor="end">
                      {p}
                    </text>
                    <text x={50 + p * 320} y="247" textAnchor="middle">
                      {p}
                    </text>
                  </g>
                ))}
                <line
                  x1="50"
                  x2="370"
                  y1="225"
                  y2="30"
                  stroke="var(--muted)"
                  strokeDasharray="4 5"
                />
                {chosen.scores.reliability.map((b) => (
                  <circle
                    key={b.lower}
                    cx={50 + b.predicted * 320}
                    cy={225 - b.observed * 195}
                    r="6"
                    fill="var(--accent)"
                    stroke="var(--ink)"
                  >
                    <title>
                      Predicted {percent(b.predicted)} · observed{" "}
                      {percent(b.observed)} · n {b.n}
                    </title>
                  </circle>
                ))}
                <text x="210" y="268" textAnchor="middle">
                  Predicted probability
                </text>
                <text
                  x="12"
                  y="130"
                  transform="rotate(-90 12 130)"
                  textAnchor="middle"
                >
                  Observed frequency
                </text>
              </svg>
            </div>
            <div className="grade-details">
              <Status value={chosen.status} />
              <p>
                Graded {dateTime(chosen.graded_at_utc)} UTC ·{" "}
                {chosen.unresolved} unresolved.
              </p>
              <div className="baseline-list">
                {chosen.baselines.map(([name, score]) => (
                  <div key={name}>
                    <strong>{name}</strong>
                    <span className="mono">
                      {score?.brier.toFixed(4) ?? "Unavailable"}
                    </span>
                    <span>
                      {score
                        ? `Brier · n ${score.n}`
                        : "No comparable baseline"}
                    </span>
                  </div>
                ))}
              </div>
              {chosen.reason && <p>{chosen.reason.replaceAll("_", " ")}</p>}
            </div>
          </div>
        ) : (
          <Empty
            title="Waiting for later outcomes"
            text="Calibration is computed from held-out outcomes after a forecast. An empty score is not a perfect score."
          />
        )}
      </Panel>
    </>
  );
}
