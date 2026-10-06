import { GitBranch } from "lucide-react";
import type { DashboardData } from "../data";
import { dateTime, integer } from "../format";
import { Empty, Header, Notice, Panel } from "../ui";
import { SignalHistory, signalNames } from "../signals";
export function Regimes({ data }: { data: DashboardData }) {
  return (
    <>
      <Header
        eyebrow="CONDITIONS / 05"
        title="Regime history"
        description="Keep evidence within the conditions that produced it."
      />
      <div className="regime-current">
        <span className="regime-icon">
          <GitBranch size={24} strokeWidth={1.5} />
        </span>
        <div>
          <span className="eyebrow">CURRENT EVIDENCE LABEL</span>
          <h2 className="mono">{data.curves.regime_id}</h2>
          <p>
            {data.health.source === "sim"
              ? "SIMULATED"
              : data.health.source === "replay"
                ? "REPLAY"
                : "LIVE"}{" "}
            · region {data.health.region} ·{" "}
            {data.clock.mean_slot_ms?.toFixed(0) ?? "Unknown"} ms slot clock
          </p>
        </div>
      </div>
      <Notice>
        BOCPD detects changes in recorded signals; CUSUM supplies a baseline.
        Epochs annotate the calendar. A change alone does not identify a network
        upgrade.
        {data.health.source === "sim" &&
          " All signal detections on this page are synthetic simulation evidence."}
      </Notice>
      <Panel
        title="Measured signals"
        caption={`Last ${data.diagnostics.signals.length} timestamped windows · snapshot ${dateTime(data.diagnostics.as_of_utc)} UTC`}
      >
        <SignalHistory windows={data.diagnostics.signals} />
      </Panel>
      <Panel
        title="Detected changes"
        caption="Persisted events · firing signals and automatic actions"
      >
        {data.diagnostics.regimes.length ? (
          <ol className="timeline">
            {data.diagnostics.regimes.map((r, i) => (
              <li key={r.id}>
                <span className="timeline-node">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <div>
                  <div className="timeline-title">
                    <h3 className="mono">{r.regime_id}</h3>
                    <span className={`source-badge ${r.source}`}>
                      {r.origin.toUpperCase()}
                    </span>
                  </div>
                  <p>
                    {dateTime(r.detected_at_utc)} UTC · from{" "}
                    <span className="mono">{r.previous_regime_id}</span>
                  </p>
                  {r.annotation && <p className="tiny">{r.annotation}</p>}
                  <div className="table-wrap">
                    <table>
                      <thead>
                        <tr>
                          <th>Firing signal</th>
                          <th>Baseline → recent</th>
                          <th>Short-run mass</th>
                          <th>CUSUM</th>
                        </tr>
                      </thead>
                      <tbody>
                        {r.votes.map((v) => (
                          <tr key={v.signal}>
                            <td>{signalNames[v.signal]}</td>
                            <td className="mono">
                              {v.baseline.toFixed(3)} → {v.recent.toFixed(3)}
                            </td>
                            <td>
                              {(v.short_run_probability * 100).toFixed(1)}%
                            </td>
                            <td>{v.cusum.toFixed(1)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                  <p className="panel-note">
                    {r.origin === "backfill" ? (
                      "Historical detection only; active models and forecasts are unaffected."
                    ) : (
                      <>
                        Old sample mass cap {r.old_effective_n_cap} · uniform
                        exploration {(r.exploration_fraction * 100).toFixed(0)}%
                        until {dateTime(r.exploration_until_utc)} UTC · crossing
                        forecasts voided · alert persisted.
                      </>
                    )}
                  </p>
                  <details className="signal-details">
                    <summary>Detection policy</summary>
                    <p>{r.policy}</p>
                    <p className="mono hash">Window {r.signal_window_id}</p>
                  </details>
                </div>
              </li>
            ))}
          </ol>
        ) : (
          <Empty
            title="No detected changes"
            text="A new regime appears only when recorded evidence crosses the detector thresholds."
            icon={GitBranch}
          />
        )}
      </Panel>
      <Panel
        title="Historical chain backfill"
        caption="42–56 days requested · bounded read-only reconstruction"
      >
        {data.diagnostics.backfills.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Coverage · UTC</th>
                  <th>Status</th>
                  <th>Timestamps / missing</th>
                  <th>Requests</th>
                </tr>
              </thead>
              <tbody>
                {data.diagnostics.backfills.map((b) => (
                  <tr key={b.id}>
                    <td>
                      {b.from_utc ? dateTime(b.from_utc) : "Unknown"} —{" "}
                      {b.through_utc ? dateTime(b.through_utc) : "Unknown"}
                      <span className="cell-detail">{b.limitation}</span>
                    </td>
                    <td>{b.status.replaceAll("_", " ")}</td>
                    <td>
                      {b.timestamps} / {b.missing_timestamps}
                    </td>
                    <td>{b.requests}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="Historical coverage has not been verified"
            text="No mainnet backfill has run while collection is paused. The historical slot-time steps remain an unverified acceptance item."
          />
        )}
      </Panel>
      <Panel
        title="Observed regime labels"
        caption="Latest 50 labels · first and last owned observation, not inferred activation dates"
      >
        {data.evidence.regimes.length ? (
          <ol className="timeline">
            {data.evidence.regimes.map((r, i) => (
              <li key={r.id}>
                <span className="timeline-node">
                  {String(i + 1).padStart(2, "0")}
                </span>
                <div>
                  <div className="timeline-title">
                    <h3 className="mono">{r.id}</h3>
                    <span className={`source-badge ${data.health.source}`}>
                      {data.health.source === "sim"
                        ? "SIMULATED"
                        : data.health.source.toUpperCase()}
                    </span>
                    {r.id === data.curves.regime_id && (
                      <span className="tiny">Current</span>
                    )}
                  </div>
                  <p>
                    {dateTime(r.first_observed_utc)} —{" "}
                    {dateTime(r.last_observed_utc)} UTC
                  </p>
                  <span className="tiny">
                    {integer(r.canaries)} owned records · observed label
                  </span>
                </div>
              </li>
            ))}
          </ol>
        ) : (
          <Empty
            title="No regime observations"
            text="A timeline appears when source-scoped evidence is available."
            icon={GitBranch}
          />
        )}
      </Panel>
    </>
  );
}
