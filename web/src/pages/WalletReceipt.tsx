import { useEffect, useRef, useState, type FormEvent } from "react";
import type { WalletReceipt } from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { integer, percent } from "../format";
import { Notice } from "../ui";

export function WalletReceiptPanel({ data }: { data: DashboardData }) {
  const [wallet, setWallet] = useState("");
  const [capture, setCapture] = useState("");
  const [report, setReport] = useState<WalletReceipt | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [data.client],
  );
  async function submit(event: FormEvent) {
    event.preventDefault();
    const current = ++generation.current;
    setBusy(true);
    setError("");
    setReport(null);
    try {
      const result = await data.client.receipt(
        wallet.trim(),
        capture.trim() || undefined,
        {
          region: data.health.region,
          target_p: 0.9,
          horizon_slots: 2,
          max_curve_age_s: 300,
        },
      );
      if (current === generation.current) setReport(result);
    } catch (e) {
      if (current === generation.current) setError(errorText(e));
    } finally {
      if (current === generation.current) setBusy(false);
    }
  }
  return (
    <div className="receipt-limits">
      <p className="panel-note">
        Evaluate saved {data.health.source} history for a wallet. Target: 90%
        within 2 slots. Live history needs provider access; missing evidence
        stays unavailable.
      </p>
      <form className="quote-form" onSubmit={(e) => void submit(e)}>
        <label>
          Wallet fee payer
          <input
            value={wallet}
            disabled={busy}
            onChange={(e) => {
              setWallet(e.target.value);
              setReport(null);
            }}
            required
            minLength={32}
            maxLength={44}
            placeholder="Base58 wallet address"
          />
        </label>
        <details>
          <summary>Select a saved capture</summary>
          <label>
            Capture hash
            <input
              value={capture}
              disabled={busy}
              onChange={(e) => {
                setCapture(e.target.value);
                setReport(null);
              }}
              pattern="sha256:[a-f0-9]{64}"
              placeholder="Latest imported capture"
            />
          </label>
        </details>
        <button className="button primary" type="submit" disabled={busy}>
          {busy ? "Evaluating…" : "Evaluate receipt"}
        </button>
      </form>
      {error && <Notice danger>{error}</Notice>}
      {report && (
        <>
          <div className="receipt-limits">
            <h3>
              {report.source.toUpperCase()} · {report.transactions} visible
              transactions
            </h3>
            <p>
              {report.from_utc} — {report.through_utc}
            </p>
            <p>
              Fees: {integer(report.fees_lamports)} lamports · known paid tips:{" "}
              {integer(report.known_paid_tips_lamports)} lamports
            </p>
            <p>
              Visible failed share:{" "}
              {report.visible_failed_share == null
                ? "Unavailable"
                : percent(report.visible_failed_share)}{" "}
              ({report.failed_transactions}/{report.transactions}) ·{" "}
              {report.unknown_tip_payments} unknown tip payments
            </p>
            <p>
              Spend above supported tip threshold:{" "}
              {integer(report.spend_above_threshold_lamports)} lamports over{" "}
              {report.compared_transactions} compared transactions ·{" "}
              {report.duplicates_removed} duplicate deliveries removed
            </p>
            <p>{report.threshold_definition}</p>
          </div>
          <details>
            <summary>Transaction details and limits</summary>
            <div className="table-wrap">
              <table>
                <thead>
                  <tr>
                    <th>Transaction</th>
                    <th>Execution</th>
                    <th>Paid tip (lamports)</th>
                    <th>Historical comparison</th>
                  </tr>
                </thead>
                <tbody>
                  {report.rows.map((r) => (
                    <tr key={r.signature}>
                      <td className="mono" title={r.signature}>
                        {r.signature.slice(0, 12)}…
                      </td>
                      <td>{r.success ? "LANDED OK" : "LANDED FAILED"}</td>
                      <td>
                        {r.paid_tip_lamports == null
                          ? "Unknown"
                          : integer(r.paid_tip_lamports)}
                      </td>
                      <td>
                        {r.comparison ? (
                          <>
                            {integer(r.comparison.snapshot.config.tip_lamports)}{" "}
                            lamports · 95% interval [
                            {percent(r.comparison.snapshot.p_interval_95[0])},{" "}
                            {percent(r.comparison.snapshot.p_interval_95[1])}] ·
                            n {r.comparison.snapshot.n_effective.toFixed(1)} ·
                            age {r.comparison.age_at_transaction_s.toFixed(1)} s
                            · {r.comparison.snapshot.evidence}
                          </>
                        ) : (
                          r.unavailable_reason
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <div className="receipt-limits">
              {report.limits.map((limit) => (
                <p key={limit}>{limit}</p>
              ))}
            </div>
          </details>
        </>
      )}
      <p className="panel-note">
        Survivorship: filtered history misses unlanded submissions. Canary
        evidence does not establish real-swap performance. Spend above a
        threshold is a conditional diagnostic, not proven waste.
      </p>
    </div>
  );
}
