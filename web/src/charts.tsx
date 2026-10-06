import { useId, useState } from "react";
import type {
  CurveSnapshot,
  EconomicCost,
  CanaryConfig,
} from "../../sdk/ts/src/index";
import { integer, percent, ratio, routeNames, sol, cellKey } from "./format";
import { Empty, EvidenceMeta, Status } from "./ui";
const colors = ["var(--accent-text)", "var(--pass)", "var(--warning)"];
export function LandingChart({ curves }: { curves: CurveSnapshot[] }) {
  const id = useId(),
    [selected, setSelected] = useState<string | null>(null),
    [table, setTable] = useState(false);
  if (!curves.length)
    return (
      <Empty
        title="No comparable curve yet"
        text="Choose a route, size and fee bucket with stored evidence."
      />
    );
  const maximum = curves.reduce(
    (m, c) =>
      BigInt(c.config.tip_lamports) > m ? BigInt(c.config.tip_lamports) : m,
    1n,
  );
  const routes = [...new Set(curves.map((c) => c.config.route))];
  const x = (c: CurveSnapshot) =>
    56 + ratio(BigInt(c.config.tip_lamports), maximum) * 574;
  const y = (p: number) => 230 - p * 202;
  const choice =
    curves.find((c) => cellKey(c.config) === selected) ??
    curves.find((c) => c.evidence === "MEASURED") ??
    curves[0];
  return (
    <div className="chart-body">
      <div className="chart-key">
        {routes.map((route, i) => (
          <span key={route}>
            <i style={{ background: colors[i] }} />
            {routeNames[route]}
          </span>
        ))}
        <button
          className="text-button"
          onClick={() => setTable((v) => !v)}
          aria-expanded={table}
        >
          {table ? "Hide data" : "View data"}
        </button>
      </div>
      <svg
        className="landing-chart"
        viewBox="0 0 680 280"
        role="img"
        aria-labelledby={`${id}-title ${id}-desc`}
      >
        <title id={`${id}-title`}>Landing probability by tip</title>
        <desc id={`${id}-desc`}>
          Stored M0 cell estimates. Shaded regions show 95% intervals. Use View
          data for exact values.
        </desc>
        {[0, 0.25, 0.5, 0.75, 1].map((p) => (
          <g key={p}>
            <line x1="56" x2="630" y1={y(p)} y2={y(p)} className="chart-grid" />
            <text x="43" y={y(p) + 4} textAnchor="end">
              {Math.round(p * 100)}%
            </text>
          </g>
        ))}
        {[0, 1, 2, 3, 4].map((i) => (
          <g key={i}>
            <text x={56 + i * 143.5} y="255" textAnchor="middle">
              {integer((maximum * BigInt(i)) / 4n)}
            </text>
          </g>
        ))}
        <text x="344" y="276" textAnchor="middle">
          Tip · lamports
        </text>
        {routes.map((route, i) => {
          const points = curves
            .filter((c) => c.config.route === route)
            .sort((a, b) =>
              BigInt(a.config.tip_lamports) < BigInt(b.config.tip_lamports)
                ? -1
                : 1,
            );
          return (
            <g key={route} style={{ color: colors[i] }}>
              <path
                d={`M${points.map((c) => `${x(c)},${y(c.p_interval_95[1])}`).join("L")}L${[
                  ...points,
                ]
                  .reverse()
                  .map((c) => `${x(c)},${y(c.p_interval_95[0])}`)
                  .join("L")}Z`}
                fill="currentColor"
                opacity=".1"
              />
              <path
                d={`M${points.map((c) => `${x(c)},${y(c.p_hat)}`).join("L")}`}
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
                strokeDasharray={i === 1 ? "8 4" : i === 2 ? "2 4" : undefined}
              />
              {points.map((c) => (
                <g key={cellKey(c.config)}>
                  <circle
                    cx={x(c)}
                    cy={y(c.p_hat)}
                    r={cellKey(c.config) === cellKey(choice.config) ? 6 : 4}
                    fill="var(--paper)"
                    stroke="currentColor"
                    strokeWidth="2"
                  />
                  <circle
                    cx={x(c)}
                    cy={y(c.p_hat)}
                    r="14"
                    fill="transparent"
                    className="chart-hit"
                    onClick={() => setSelected(cellKey(c.config))}
                  >
                    <title>
                      {routeNames[route]} · {integer(c.config.tip_lamports)}{" "}
                      lamports · {percent(c.p_hat)} · 95%{" "}
                      {percent(c.p_interval_95[0])}–
                      {percent(c.p_interval_95[1])} · n effective{" "}
                      {c.n_effective.toFixed(1)}
                    </title>
                  </circle>
                </g>
              ))}
            </g>
          );
        })}
      </svg>
      <div className="chart-inspector">
        <label>
          Inspect configuration
          <select
            value={cellKey(choice.config)}
            onChange={(e) => setSelected(e.target.value)}
          >
            {curves.map((c) => (
              <option key={cellKey(c.config)} value={cellKey(c.config)}>
                {routeNames[c.config.route]} · {integer(c.config.tip_lamports)}{" "}
                lamports
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="chart-detail">
        <div>
          <strong>{routeNames[choice.config.route]}</strong>
          <span>{integer(choice.config.tip_lamports)} lamports</span>
          <span className="mono">{percent(choice.p_hat)}</span>
          <Status value={choice.evidence} />
        </div>
        <EvidenceMeta
          interval={choice.p_interval_95}
          n={choice.n_effective}
          seconds={choice.data_age_s}
        />
      </div>
      {table && (
        <div className="table-wrap">
          <table>
            <caption className="sr-only">Landing curve estimates</caption>
            <thead>
              <tr>
                <th>Route</th>
                <th>Tip (lamports)</th>
                <th>Probability</th>
                <th>95% interval</th>
                <th>n effective</th>
                <th>Age (seconds)</th>
                <th>Evidence</th>
              </tr>
            </thead>
            <tbody>
              {curves.map((c) => (
                <tr key={cellKey(c.config)}>
                  <td>{routeNames[c.config.route]}</td>
                  <td className="mono">{integer(c.config.tip_lamports)}</td>
                  <td>{percent(c.p_hat)}</td>
                  <td>
                    {percent(c.p_interval_95[0])}–{percent(c.p_interval_95[1])}
                  </td>
                  <td>{c.n_effective.toFixed(1)}</td>
                  <td>{c.data_age_s?.toFixed(1) ?? "Unavailable"}</td>
                  <td>{c.evidence}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
export function FrontierChart({
  points,
  knee,
}: {
  points: EconomicCost[];
  knee: CanaryConfig | null;
}) {
  const id = useId();
  const max = Math.max(
    ...points.map((p) => Number(p.expected_cost_usd)),
    0.001,
  );
  return (
    <svg
      className="frontier-chart"
      viewBox="0 0 680 230"
      role="img"
      aria-labelledby={id}
    >
      <title id={id}>
        API cost frontier. Cost is conditional USD; knee is the server-selected
        configuration.
      </title>
      {[0, 0.5, 1].map((p) => (
        <g key={p}>
          <line
            className="chart-grid"
            x1="52"
            x2="640"
            y1={180 - p * 150}
            y2={180 - p * 150}
          />
          <text x="40" y={184 - p * 150} textAnchor="end">
            {Math.round(p * 100)}%
          </text>
        </g>
      ))}
      <path
        d={`M${points.map((p) => `${52 + (Number(p.expected_cost_usd) / max) * 588},${180 - p.prediction.p_hat * 150}`).join("L")}`}
        stroke="var(--accent-text)"
        fill="none"
        strokeWidth="2"
      />
      {points.map((p) => {
        const isKnee = knee && cellKey(knee) === cellKey(p.prediction.config);
        return (
          <g key={cellKey(p.prediction.config)}>
            <circle
              cx={52 + (Number(p.expected_cost_usd) / max) * 588}
              cy={180 - p.prediction.p_hat * 150}
              r={isKnee ? 7 : 4}
              fill={isKnee ? "var(--accent)" : "var(--paper)"}
              stroke="var(--accent-text)"
              strokeWidth="2"
            >
              <title>
                {sol(p.nominal_lamports)} SOL · ${p.expected_cost_usd} expected
                cost · {percent(p.prediction.p_hat)}
                {isKnee ? " · Knee" : ""}
              </title>
            </circle>
            {isKnee && (
              <text
                x={52 + (Number(p.expected_cost_usd) / max) * 588}
                y={163 - p.prediction.p_hat * 150}
                textAnchor="middle"
              >
                Knee
              </text>
            )}
          </g>
        );
      })}
      <text x="346" y="215" textAnchor="middle">
        Expected cost · USD · conditional estimate
      </text>
    </svg>
  );
}
