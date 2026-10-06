import { GitBranch } from "lucide-react";
import type { DashboardData } from "../data";
import { dateTime, integer } from "../format";
import { Empty, Header, Notice, Panel } from "../ui";
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
        This timeline shows regime labels stored on owned canaries. Automated
        change-point detection, signal annotations and historical chain backfill
        are unavailable on this API; this view does not claim that a network
        upgrade was detected.
      </Notice>
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
