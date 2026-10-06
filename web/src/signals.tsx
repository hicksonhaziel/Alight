import { useId, useState } from "react";
import type { SignalKind, SignalWindow } from "../../sdk/ts/src/index";
import { dateTime } from "./format";
import { Empty } from "./ui";
export const signalNames: Record<SignalKind, string> = {
  slot_ms: "Mean slot interval",
  slot_p95_ms: "Slot interval p95",
  skip_rate: "Skip / dead rate",
  reference_landing_rate: "Reference landing rate",
  block_fullness: "Sampled block fullness",
  non_vote_share: "Sampled non-vote share",
  observer_lag_ms: "Relative observer lag",
};
export function SignalHistory({ windows }: { windows: SignalWindow[] }) {
  const [kind, setKind] = useState<SignalKind>("slot_ms"),
    id = useId();
  const points = windows.map((w) => ({
    w,
    m: w.measures.find((m) => m.kind === kind),
  }));
  const valid = points.flatMap((p) => (p.m?.value == null ? [] : [p.m.value]));
  const min = Math.min(...valid),
    max = Math.max(...valid),
    span = Math.max(max - min, Math.abs(max) * 0.02, 0.01);
  const x = (i: number) => 54 + (i / Math.max(points.length - 1, 1)) * 570;
  const y = (v: number) => 210 - ((v - min) / span) * 150;
  const paths: string[] = [];
  let path = "";
  points.forEach((p, i) => {
    if (p.m?.value == null) {
      if (path) paths.push(path);
      path = "";
    } else {
      path += `${path ? " L" : "M"}${x(i)},${y(p.m.value)}`;
    }
  });
  if (path) paths.push(path);
  return (
    <div className="chart-body">
      <div className="signal-control">
        <label htmlFor={id}>Signal</label>
        <select
          id={id}
          value={kind}
          onChange={(e) => setKind(e.target.value as SignalKind)}
        >
          {Object.entries(signalNames).map(([k, label]) => (
            <option key={k} value={k}>
              {label}
            </option>
          ))}
        </select>
        <span className="tiny">
          {valid.length} measured / {points.length} windows
        </span>
      </div>
      {valid.length ? (
        <svg
          className="landing-chart"
          viewBox="0 0 680 260"
          role="img"
          aria-labelledby={`${id}-title ${id}-desc`}
        >
          <title id={`${id}-title`}>
            {signalNames[kind]} over recorded windows
          </title>
          <desc id={`${id}-desc`}>
            UTC window order. Missing measurements break the line. Exact values
            and provenance are below.
          </desc>
          {[0, 0.5, 1].map((f) => (
            <g key={f}>
              <line
                x1="54"
                x2="624"
                y1={210 - f * 150}
                y2={210 - f * 150}
                className="chart-grid"
              />
              <text
                x="48"
                y={214 - f * 150}
                textAnchor="end"
                className="chart-label"
              >
                {(min + f * span).toFixed(kind.endsWith("ms") ? 0 : 2)}
              </text>
            </g>
          ))}
          {paths.map((d, i) => (
            <path
              key={i}
              d={d}
              fill="none"
              stroke="var(--accent-text)"
              strokeWidth="2"
            />
          ))}
          <text x="54" y="242" className="chart-label">
            {windows[0] ? dateTime(windows[0].through_utc) : ""}
          </text>
          <text x="624" y="242" textAnchor="end" className="chart-label">
            {windows.at(-1) ? dateTime(windows.at(-1)!.through_utc) : ""} UTC
          </text>
        </svg>
      ) : (
        <Empty
          title="No measured signal in this window"
          text="Missing measurements remain unavailable with a reason; they do not become zero."
        />
      )}
      <details className="signal-details">
        <summary>Window values & provenance</summary>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Through · UTC</th>
                <th>Origin</th>
                <th>Value / unit</th>
                <th>Samples</th>
                <th>Evidence / limitation</th>
              </tr>
            </thead>
            <tbody>
              {points
                .slice()
                .reverse()
                .map(({ w, m }) => (
                  <tr key={w.id}>
                    <td>{dateTime(w.through_utc)}</td>
                    <td>{w.origin}</td>
                    <td className="mono">
                      {m?.value == null ? "Unavailable" : m.value.toFixed(3)}{" "}
                      {m?.unit ?? ""}
                    </td>
                    <td>{m?.n ?? 0}</td>
                    <td>
                      {m?.unavailable_reason ??
                        m?.provenance ??
                        "Signal was not recorded"}
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
        </div>
      </details>
    </div>
  );
}
