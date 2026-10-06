import { useEffect, useState } from "react";
import { Activity, ArrowUpRight, CircleAlert } from "lucide-react";
import type {
  CanaryEvidencePage,
  WorkbenchCanary,
} from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { compact, dateTime, integer, sendToSeenMs } from "../format";
import { Header, Panel, Status, Empty, Loading, Notice } from "../ui";
export function Health({
  data,
  reference,
}: {
  data: DashboardData;
  reference?: string;
}) {
  const [details, setDetails] = useState<CanaryEvidencePage | null>(null),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false);
  const canary = data.evidence.canaries.find((c) => c.canary.id === reference);
  const disagreements = data.evidence.canaries.filter((c) =>
    ["observer_disagreement", "rpc_disagreement_or_incomplete"].includes(
      c.resolution_reason ?? "",
    ),
  );
  useEffect(() => {
    if (!reference) return;
    let active = true;
    setBusy(true);
    setDetails(null);
    setError("");
    void data.client
      .canaryEvidence(reference)
      .then((d) => {
        if (active) setDetails(d);
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
  }, [data.client, reference]);
  return (
    <>
      <Header
        eyebrow="DIAGNOSTICS / 06"
        title="Observer health"
        description="Freshness, recorded gaps and evidence that disagrees."
      />
      <div className="observer-grid">
        {data.health.observers.map((o) => (
          <Panel
            key={o.observer}
            title={
              o.observer === "grpc"
                ? "gRPC stream"
                : o.observer === "mirage"
                  ? "Mirage"
                  : o.observer
            }
            action={
              <Status
                value={data.health.source === "sim" ? "SIMULATED" : o.status}
              />
            }
          >
            <div className="observer-body">
              <Activity size={22} strokeWidth={1.4} aria-hidden="true" />
              <div className="metric-value">
                {o.age_ms == null ? "—" : (Number(o.age_ms) / 1000).toFixed(1)}
                <span>seconds since receive</span>
              </div>
              <dl>
                <div>
                  <dt>Cursor slot</dt>
                  <dd className="mono">
                    {o.cursor_slot ? integer(o.cursor_slot) : "Unavailable"}
                  </dd>
                </div>
                <div>
                  <dt>Open gaps</dt>
                  <dd>{integer(o.open_gaps)}</dd>
                </div>
                <div>
                  <dt>Last receive · UTC</dt>
                  <dd>
                    {o.last_receive_utc
                      ? dateTime(o.last_receive_utc)
                      : "No receive recorded"}
                  </dd>
                </div>
              </dl>
            </div>
          </Panel>
        ))}
      </div>
      <Notice>
        Receive lag is relative to the earliest observer sharing the host
        monotonic clock. It does not measure network propagation or validator
        independence. Incomplete block identities remain incomplete; missing
        evidence never means expired.
      </Notice>
      <Panel
        title="Observer comparison"
        caption={`${data.diagnostics.owned_window_n} recent owned canaries · snapshot ${dateTime(data.diagnostics.as_of_utc)} UTC`}
      >
        {data.diagnostics.observers.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Observer</th>
                  <th>Frames</th>
                  <th>Relative lag p50 / p95 · ms</th>
                  <th>Send → seen p50 / p95 · ms</th>
                  <th>Missing / conflicts</th>
                  <th>Incomparable clocks</th>
                </tr>
              </thead>
              <tbody>
                {data.diagnostics.observers.map((o) => (
                  <tr key={o.observer}>
                    <td>{o.observer}</td>
                    <td>{o.observations}</td>
                    <td>
                      {o.receive_lag_p50_ms?.toFixed(3) ?? "—"} /{" "}
                      {o.receive_lag_p95_ms?.toFixed(3) ?? "—"}
                    </td>
                    <td>
                      {o.send_to_seen_p50_ms?.toFixed(3) ?? "—"} /{" "}
                      {o.send_to_seen_p95_ms?.toFixed(3) ?? "—"}
                    </td>
                    <td>
                      {o.missing_owned} / {o.conflicts}
                    </td>
                    <td>{o.incomparable_clocks}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No comparable observer population"
            text={
              data.health.source === "sim"
                ? "Sim outcomes do not fabricate provider deliveries. Recorded fixture comparisons are verified separately."
                : "Comparisons require configured observers and original owned-canary evidence."
            }
          />
        )}
      </Panel>
      <Panel
        title="Agreement by observer pair"
        caption="Complete slot, block identity and execution result required"
      >
        {data.diagnostics.pairs.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Pair</th>
                  <th>Compared</th>
                  <th>Agreed / disagreed</th>
                  <th>Incomplete</th>
                  <th>Agreement among complete pairs</th>
                </tr>
              </thead>
              <tbody>
                {data.diagnostics.pairs.map((p) => (
                  <tr key={`${p.a}-${p.b}`}>
                    <td>
                      {p.a} / {p.b}
                    </td>
                    <td>{p.compared}</td>
                    <td>
                      {p.agreed} / {p.disagreed}
                    </td>
                    <td>{p.incomplete}</td>
                    <td>
                      {p.agreed + p.disagreed
                        ? `${((p.agreed / (p.agreed + p.disagreed)) * 100).toFixed(1)}%`
                        : "Unavailable"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No pair evidence"
            text="A missing comparison does not establish agreement."
          />
        )}
      </Panel>
      <Panel
        title="Persisted diagnostic events"
        caption="Latest 100 events · missing evidence after a 30-second grace period"
      >
        {data.diagnostics.disagreements.length ? (
          <div className="canary-links">
            {data.diagnostics.disagreements.map((e) => (
              <a href={`#health/${encodeURIComponent(e.canary_id)}`} key={e.id}>
                <div>
                  <strong className="mono">{compact(e.canary_id, 10)}</strong>
                  <span>
                    {e.kind.replaceAll("_", " ")}
                    {e.missing_observers.length
                      ? ` · missing ${e.missing_observers.join(", ")}`
                      : ""}{" "}
                    · {dateTime(e.detected_at_utc)} UTC
                  </span>
                </div>
                <ArrowUpRight size={15} />
              </a>
            ))}
          </div>
        ) : (
          <Empty
            title="No persisted diagnostic events"
            text="Absence of events in this bounded window does not prove complete observer coverage."
          />
        )}
      </Panel>
      <Panel
        title="Alert history"
        caption="Latest 50 source-scoped alerts · local persistence does not imply outbound delivery"
      >
        {data.diagnostics.alerts.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Time · UTC</th>
                  <th>Rule</th>
                  <th>Evidence</th>
                </tr>
              </thead>
              <tbody>
                {data.diagnostics.alerts.map((a) => (
                  <tr key={a.id}>
                    <td>{dateTime(a.at_utc)}</td>
                    <td>{a.rule.replaceAll("_", " ")}</td>
                    <td>
                      {a.summary}
                      <span className="cell-detail mono">{a.subject}</span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No alerts recorded"
            text="Observer disagreement alerts require three distinct recent canaries. Regime detections persist their own alert."
          />
        )}
      </Panel>
      <Panel
        title="Recorded disagreements"
        caption="Explicit latest resolver reasons · latest 100 owned canaries"
      >
        {disagreements.length ? (
          <CanaryLinks rows={disagreements} />
        ) : (
          <Empty
            title="No disagreements in this window"
            text="This bounded view does not establish agreement across the entire chain."
            icon={CircleAlert}
          />
        )}
      </Panel>
      <Panel
        title="Inspect a canary"
        caption={
          reference
            ? reference
            : "Select a recent member to compare original observation records"
        }
      >
        {error && <Notice danger>{error}</Notice>}
        {busy ? (
          <Loading />
        ) : details ? (
          <>
            <div className="evidence-identity">
              <span className="mono hash">{details.canary_id}</span>
              {canary && <Status value={canary.canary.outcome ?? "PENDING"} />}
            </div>
            {details.observations.length ? (
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Observer</th>
                      <th>Slot / block</th>
                      <th>Index scope</th>
                      <th>Success</th>
                      <th>Send → seen</th>
                      <th>Received · UTC</th>
                    </tr>
                  </thead>
                  <tbody>
                    {details.observations.map((o, i) => {
                      const ms = canary
                        ? sendToSeenMs(canary.canary, o.received)
                        : null;
                      return (
                        <tr key={i}>
                          <td>{o.observer}</td>
                          <td className="mono">
                            {o.slot ?? "Unknown"}
                            <span className="cell-detail">
                              {o.block_id
                                ? compact(o.block_id, 6)
                                : "Block identity unknown"}
                            </span>
                          </td>
                          <td>
                            {o.index_in_block ?? "—"} · {o.index_scope}
                          </td>
                          <td>
                            {o.success == null
                              ? "Unknown"
                              : o.success
                                ? "Success"
                                : "Failed"}
                          </td>
                          <td className="mono">
                            {ms == null
                              ? "Incomparable clock"
                              : `${ms.toFixed(3)} ms`}
                          </td>
                          <td>{dateTime(o.received.wall_utc)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            ) : (
              <Empty
                title="No provider observation records"
                text={
                  data.health.source === "sim"
                    ? "Simulated outcomes do not create fabricated provider frames."
                    : "No original observation was recorded for this signature in the returned window."
                }
              />
            )}
          </>
        ) : (
          <CanaryLinks rows={data.evidence.canaries.slice(0, 12)} />
        )}
      </Panel>
      <Panel title="Gap episodes" caption="Latest 50 episodes · source-scoped">
        {data.evidence.gaps.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Observer</th>
                  <th>Started · UTC</th>
                  <th>Ended · UTC</th>
                  <th>Reason</th>
                </tr>
              </thead>
              <tbody>
                {data.evidence.gaps.map((g, i) => (
                  <tr key={i}>
                    <td>{g.observer}</td>
                    <td>{dateTime(g.start_utc)}</td>
                    <td>{g.end_utc ? dateTime(g.end_utc) : "Open"}</td>
                    <td>{g.reason}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No recorded gaps"
            text="Missing evidence remains distinct from a proven transaction outcome."
          />
        )}
      </Panel>
    </>
  );
}
function CanaryLinks({ rows }: { rows: WorkbenchCanary[] }) {
  return rows.length ? (
    <div className="canary-links">
      {rows.map(({ canary: c, resolution_reason }) => (
        <a href={`#health/${encodeURIComponent(c.id)}`} key={c.id}>
          <div>
            <strong className="mono">{compact(c.id, 10)}</strong>
            <span>
              {resolution_reason?.replaceAll("_", " ") ??
                "Inspect original observations"}
            </span>
          </div>
          <Status value={c.outcome ?? "PENDING"} />
          <ArrowUpRight size={15} />
        </a>
      ))}
    </div>
  ) : (
    <Empty
      title="No canaries to inspect"
      text="Owned canary records appear here after collection or a Sim run."
    />
  );
}
