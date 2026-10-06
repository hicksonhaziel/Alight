import { useState } from "react";
import { ArrowRight, LockKeyhole, Settings2 } from "lucide-react";
import {
  AlightClient,
  decimalU64,
  type ForecastEntry,
  type QuotePreview,
  type QuoteServiceRequest,
  type Route,
  type SizeClass,
} from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import {
  integer,
  latestCurves,
  nominal,
  percent,
  routeNames,
  sol,
  cellKey,
} from "../format";
import {
  CopyButton,
  Empty,
  EvidenceMeta,
  Header,
  Loading,
  Notice,
  Panel,
  Status,
} from "../ui";
import { FrontierChart } from "../charts";
export function Quote({
  data,
  operator,
  connect,
  locked,
}: {
  data: DashboardData;
  operator: AlightClient | null;
  connect: () => void;
  locked: (entry: ForecastEntry) => void;
}) {
  const [routes, setRoutes] = useState<Route[]>(["beam_http"]),
    [size, setSize] = useState<SizeClass>("small"),
    [horizon, setHorizon] = useState(4),
    [target, setTarget] = useState("50");
  const [market, setMarket] = useState(false),
    [pool, setPool] = useState(""),
    [trade, setTrade] = useState("1000"),
    [solUsd, setSolUsd] = useState("150"),
    [edge, setEdge] = useState("10"),
    [risk, setRisk] = useState("1");
  const [busy, setBusy] = useState(""),
    [error, setError] = useState(""),
    [result, setResult] = useState<{
      preview: QuotePreview;
      request: QuoteServiceRequest;
    } | null>(null);
  const changed = () => {
    setResult(null);
    setError("");
  };
  const run = async () => {
    setBusy("quote");
    setError("");
    setResult(null);
    try {
      const [health, curvePage] = await Promise.all([
        data.client.health(),
        data.client.curves(),
      ]);
      const configs = new Map(
        latestCurves(curvePage.curves)
          .filter(
            (c) =>
              c.horizon_slots === horizon &&
              c.config.size_class === size &&
              routes.includes(c.config.route),
          )
          .map((c) => [cellKey(c.config), c.config]),
      );
      if (!configs.size) {
        setError(
          "No stored candidate configurations match these routes and size. Try the supported Sim selection: Beam HTTP, small.",
        );
        return;
      }
      const request: QuoteServiceRequest = {
        model: {
          context: {
            source: health.source,
            regime_id: curvePage.regime_id,
            region: health.region,
            as_of_utc: health.as_of_utc,
          },
          candidates: [...configs.values()],
          covariates: { congestion: null },
          leader_class_next: [],
          target: {
            kind: "probability",
            target_p: Number(target) / 100,
            horizon_slots: horizon,
          },
        },
        ttl_s: 120,
        economics: market
          ? {
              pool,
              size_usd: trade,
              sol_usd: solUsd,
              edge_bps: Number(edge),
              lambda: Number(risk),
              base_fee_lamports: decimalU64("5000"),
              use_upper_quantile: true,
            }
          : null,
        frozen_model_hash: null,
      };
      const preview = await data.client.quote(request);
      setResult({ preview, request });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy("");
    }
  };
  const freeze = async () => {
    if (!operator || !result || busy) return;
    setBusy("lock");
    setError("");
    try {
      locked(await operator.freezeQuote(result.request));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy("");
    }
  };
  const prediction = result?.preview.quote.recommendation,
    economics = result?.preview.economics?.economics;
  const code = result
    ? `import { AlightClient, decimalU64, type QuoteServiceRequest } from '@alight/client';\n\nconst client = new AlightClient({\n  endpoint: ${JSON.stringify(location.origin)},\n  source: ${JSON.stringify(data.health.source)}\n});\n// Refresh model.context.as_of_utc before reusing this captured request.\nconst request: QuoteServiceRequest = ${JSON.stringify(result.request, null, 2).replace(/^(\s*"(?:tip_lamports|cu_price_micro_lamports|base_fee_lamports)": )"([0-9]+)"(,?)$/gm, '$1decimalU64("$2")$3')};\nconst quote = await client.quote(request);`
    : "";
  return (
    <>
      <Header
        eyebrow="DECISION / 02"
        title="Quote console"
        description="Choose your objective. Inspect the evidence before locking a claim."
      />
      <div className="quote-layout">
        <Panel
          title="Transaction settings"
          caption="Only configurations supported by stored evidence"
        >
          <form
            className="quote-form"
            onSubmit={(e) => {
              e.preventDefault();
              void run();
            }}
          >
            <fieldset>
              <legend>Routes</legend>
              {(Object.keys(routeNames) as Route[]).map((route) => (
                <label className="route-option" key={route}>
                  <input
                    type="checkbox"
                    checked={routes.includes(route)}
                    onChange={(e) => {
                      changed();
                      setRoutes((old) =>
                        e.target.checked
                          ? [...old, route]
                          : old.filter((r) => r !== route),
                      );
                    }}
                  />
                  <span>{routeNames[route]}</span>
                  <span className="tiny">
                    {route === "rpc" ? "No Beam tip" : "Tip-bearing"}
                  </span>
                </label>
              ))}
            </fieldset>
            <div className="form-pair">
              <label>
                Canary size
                <select
                  value={size}
                  onChange={(e) => {
                    changed();
                    setSize(e.target.value as SizeClass);
                  }}
                >
                  <option value="small">Small</option>
                  <option value="medium">Medium</option>
                  <option value="large">Large</option>
                </select>
              </label>
              <label>
                Horizon
                <select
                  value={horizon}
                  onChange={(e) => {
                    changed();
                    setHorizon(Number(e.target.value));
                  }}
                >
                  {[1, 2, 4].map((h) => (
                    <option key={h} value={h}>
                      {h} slots
                      {data.clock.mean_slot_ms
                        ? ` · ${h * data.clock.mean_slot_ms} ms`
                        : ""}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <label>
              Target landing probability
              <div className="input-unit">
                <input
                  type="number"
                  min="1"
                  max="100"
                  step="1"
                  value={target}
                  onChange={(e) => {
                    changed();
                    setTarget(e.target.value);
                  }}
                  required
                />
                <span>%</span>
              </div>
              <span className="field-hint">
                The API can refuse a target without sufficient evidence.
              </span>
            </label>
            <details
              open={market}
              onToggle={(e) => {
                const open = e.currentTarget.open;
                if (open !== market) {
                  setMarket(open);
                  changed();
                }
              }}
            >
              <summary>
                <Settings2 size={15} />
                Conditional market economics
              </summary>
              <div className="market-fields">
                <p>
                  Optional inputs. Stale or missing pool evidence produces a
                  probability-only quote.
                </p>
                <label>
                  Pool address
                  <input
                    value={pool}
                    maxLength={128}
                    onChange={(e) => {
                      changed();
                      setPool(e.target.value);
                    }}
                    required={market}
                  />
                </label>
                <div className="form-pair">
                  <label>
                    Trade size · USD
                    <input
                      value={trade}
                      pattern="[0-9]+(\.[0-9]+)?"
                      inputMode="decimal"
                      onChange={(e) => {
                        changed();
                        setTrade(e.target.value);
                      }}
                      required={market}
                    />
                  </label>
                  <label>
                    SOL price · USD
                    <input
                      value={solUsd}
                      pattern="[0-9]+(\.[0-9]+)?"
                      inputMode="decimal"
                      onChange={(e) => {
                        changed();
                        setSolUsd(e.target.value);
                      }}
                      required={market}
                    />
                  </label>
                  <label>
                    Edge · bps
                    <input
                      type="number"
                      value={edge}
                      step="any"
                      onChange={(e) => {
                        changed();
                        setEdge(e.target.value);
                      }}
                      required={market}
                    />
                  </label>
                  <label>
                    Risk weight · λ
                    <input
                      type="number"
                      value={risk}
                      min="0"
                      step="any"
                      onChange={(e) => {
                        changed();
                        setRisk(e.target.value);
                      }}
                      required={market}
                    />
                  </label>
                </div>
              </div>
            </details>
            <button
              className="primary wide"
              disabled={!!busy || !routes.length}
            >
              {busy === "quote" ? "Computing…" : "Compute quote"}
              <ArrowRight size={16} />
            </button>
            <p className="form-note">
              Read-only preview. No transaction is sent.
            </p>
          </form>
        </Panel>
        <div className="quote-result">
          {error && <Notice danger>{error}</Notice>}
          {busy === "quote" ? (
            <Panel title="Computing quote">
              <Loading />
            </Panel>
          ) : result ? (
            <>
              <Panel
                title="Recommended configuration"
                action={<Status value={result.preview.quote.evidence} />}
              >
                {prediction ? (
                  <div className="quote-card">
                    <div className="quote-probability mono">
                      {percent(prediction.p_hat)}
                      <span>within {horizon} slots</span>
                    </div>
                    <EvidenceMeta
                      interval={prediction.p_interval_95}
                      n={prediction.n_effective}
                      seconds={prediction.data_age_s}
                    />
                    <div className="config-grid">
                      <div>
                        <span>Route</span>
                        <strong>{routeNames[prediction.config.route]}</strong>
                      </div>
                      <div>
                        <span>Tip · lamports</span>
                        <strong className="mono">
                          {integer(prediction.config.tip_lamports)}
                        </strong>
                      </div>
                      <div>
                        <span>Priority · µlamports/CU</span>
                        <strong className="mono">
                          {integer(prediction.config.cu_price_micro_lamports)}
                        </strong>
                      </div>
                      <div>
                        <span>Compute limit · CU</span>
                        <strong className="mono">
                          {integer(String(prediction.config.cu_limit))}
                        </strong>
                      </div>
                    </div>
                    <div className="quote-cost">
                      <span>
                        {economics
                          ? "Expected cost · conditional"
                          : "Nominal reservation estimate"}
                      </span>
                      <strong className="mono">
                        {economics
                          ? `$${economics.recommendation.expected_cost_usd}`
                          : `${sol(nominal(prediction.config), 9)} SOL`}
                      </strong>
                      <p>
                        {economics
                          ? economics.assumption
                          : "Tip + priority fee + 5,000 lamport base-fee assumption. No market savings estimate."}
                      </p>
                    </div>
                    <div className="quote-actions">
                      <button
                        className="primary"
                        disabled={!!busy || !data.evidence.operator_enabled}
                        onClick={() => (operator ? void freeze() : connect())}
                      >
                        <LockKeyhole size={15} />
                        {busy === "lock"
                          ? "Locking…"
                          : operator
                            ? "Lock forecast for Prove"
                            : "Connect operator to lock"}
                      </button>
                      <a className="text-link" href="#ledger">
                        View ledger
                        <ArrowRight size={14} />
                      </a>
                    </div>
                  </div>
                ) : (
                  <Empty
                    title="Evidence is insufficient"
                    text={
                      result.preview.quote.reason ??
                      "The API declined to recommend a configuration."
                    }
                  >
                    <p>
                      {result.preview.quote.samples_needed == null
                        ? ""
                        : `${result.preview.quote.samples_needed} additional resolved samples needed.`}
                    </p>
                  </Empty>
                )}
              </Panel>
              {result.preview.economics?.fallback_reason && (
                <Notice>
                  {result.preview.economics.fallback_reason.replaceAll(
                    "_",
                    " ",
                  )}
                  . Dollar economics are unavailable for this quote.
                </Notice>
              )}
              {economics && (
                <Panel
                  title="Cost–probability frontier"
                  caption="API frontier and knee · conditional on your market inputs"
                >
                  <FrontierChart
                    points={economics.frontier}
                    knee={economics.knee ?? null}
                  />
                  <div className="table-wrap">
                    <table>
                      <thead>
                        <tr>
                          <th>Route</th>
                          <th>Expected USD</th>
                          <th>Probability</th>
                          <th>95% interval</th>
                          <th>n effective</th>
                        </tr>
                      </thead>
                      <tbody>
                        {economics.frontier.map((p) => (
                          <tr key={cellKey(p.prediction.config)}>
                            <td>{routeNames[p.prediction.config.route]}</td>
                            <td className="mono">{p.expected_cost_usd}</td>
                            <td>{percent(p.prediction.p_hat)}</td>
                            <td>
                              {percent(p.prediction.p_interval_95[0])}–
                              {percent(p.prediction.p_interval_95[1])}
                            </td>
                            <td>{p.prediction.n_effective.toFixed(1)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </Panel>
              )}
              <Panel
                title="Baseline comparison"
                caption="Availability and predictions are returned by the API"
              >
                <div className="baseline-list">
                  {result.preview.baselines.map((b) => (
                    <div key={b.id}>
                      <strong>{b.id}</strong>
                      <span className="mono">{percent(b.p_hat)}</span>
                      <span>
                        {b.unavailable_reason?.replaceAll("_", " ") ??
                          (b.config
                            ? routeNames[b.config.route]
                            : "No configuration")}
                      </span>
                    </div>
                  ))}
                </div>
              </Panel>
              <Panel
                title="TypeScript integration"
                caption="Exact preview request · refresh its evaluation clock before reuse"
                action={<CopyButton text={code} label="Copy code" />}
              >
                <pre className="code">{code}</pre>
              </Panel>
            </>
          ) : (
            <div className="quote-placeholder">
              <div className="quote-emblem">
                <img src="/alight-mark.svg" alt="" />
              </div>
              <span className="eyebrow">EVIDENCE BEFORE EXECUTION</span>
              <h2>Inspect landing probability and cost.</h2>
              <p>
                A quote includes its interval, sample count and age. A
                recommendation appears only when the model has sufficient
                support.
              </p>
              <div className="workflow-steps">
                <span>01 / Quote</span>
                <i />
                <span>02 / Lock</span>
                <i />
                <span>03 / Prove</span>
              </div>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
