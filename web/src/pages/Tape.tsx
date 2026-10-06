import { useEffect, useState } from "react";
import { ReceiptText } from "lucide-react";
import type { TapePage } from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { compact, dateTime, integer } from "../format";
import { Empty, Header, Loading, Notice, Panel } from "../ui";
export function Tape({ data }: { data: DashboardData }) {
  const [tape, setTape] = useState<TapePage | null>(null),
    [error, setError] = useState("");
  // Pin the selected window. Stream ticks must not create an unbounded request loop.
  const [through] = useState(data.health.as_of_utc);
  useEffect(() => {
    let active = true;
    const from = new Date(Date.parse(through) - 3600000).toISOString();
    void data.client
      .tape(from, through, 1000)
      .then((t) => {
        if (active) setTape(t);
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [data.client, through]);
  const paid = tape?.rows.filter((t) => t.tip_lamports != null) ?? [];
  const buckets = new Map<string, number>();
  for (const t of paid)
    buckets.set(t.tip_lamports!, (buckets.get(t.tip_lamports!) ?? 0) + 1);
  const bins = [...buckets]
      .sort(([a], [b]) => (BigInt(a) < BigInt(b) ? -1 : 1))
      .slice(0, 40),
    max = Math.max(...bins.map(([, n]) => n), 1);
  return (
    <>
      <Header
        eyebrow="MARKET / 07"
        title="Tape & receipts"
        description="Observed transfers, their coverage and what they can tell you."
      />
      {error && <Notice danger>{error}</Notice>}
      <Panel
        title="Observed paid tips"
        caption="One-hour window · at most 1,000 returned transfers · successful evidenced payments"
      >
        {!tape ? (
          <Loading />
        ) : paid.length ? (
          <>
            <div
              className="tip-distribution"
              role="img"
              aria-label={`Paid tip distribution, ${paid.length} transfers. At most 40 distinct tip amounts displayed.`}
            >
              {bins.map(([amount, n]) => (
                <div key={amount}>
                  <span className="mono">{integer(amount)}</span>
                  <i>
                    <b style={{ width: `${(n / max) * 100}%` }} />
                  </i>
                  <span>{n}</span>
                </div>
              ))}
            </div>
            <p className="panel-note">
              Tip amounts in lamports · counts are transfers, not unique wallets
              or complete chain traffic. First 40 distinct amounts shown.
            </p>
          </>
        ) : (
          <Empty
            title="No paid tips in this window"
            text={
              data.health.source === "sim"
                ? "Sim does not fabricate a mainnet market tape."
                : "The filtered observer recorded no qualifying paid transfers in this window."
            }
          />
        )}
      </Panel>
      <Panel
        title="Recent transfer evidence"
        caption={
          tape
            ? `${dateTime(tape.from)} — ${dateTime(tape.through)} UTC`
            : "Loading window"
        }
      >
        {tape?.rows.length ? (
          <div className="table-wrap">
            <table>
              <thead>
                <tr>
                  <th>Signature</th>
                  <th>Slot</th>
                  <th>Recipient</th>
                  <th>Paid tip · lamports</th>
                  <th>Fee · lamports</th>
                  <th>Index / scope</th>
                </tr>
              </thead>
              <tbody>
                {tape.rows.slice(0, 50).map((t, i) => (
                  <tr key={`${t.signature}-${i}`}>
                    <td className="mono" title={t.signature}>
                      {compact(t.signature, 6)}
                    </td>
                    <td className="mono">{t.slot}</td>
                    <td className="mono" title={t.recipient}>
                      {compact(t.recipient, 6)}
                    </td>
                    <td className="mono">
                      {t.tip_lamports == null
                        ? "Unproven payment"
                        : integer(t.tip_lamports)}
                    </td>
                    <td className="mono">{integer(t.fee_lamports)}</td>
                    <td>
                      {t.index_in_block ?? "Unknown"} / {t.index_scope}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <Empty
            title="No transfer records"
            text="Requested tips and proven paid tips remain distinct."
          />
        )}
      </Panel>
      <Panel
        title="Wallet receipts"
        caption="Coverage and limitations"
        action={<ReceiptText size={18} aria-hidden="true" />}
      >
        <div className="receipt-limits">
          <h3>Receipt service unavailable.</h3>
          <p>
            This API exposes filtered tip transfers, without a wallet
            transaction history or a historical receipt evaluator. Wallet
            receipt generation is not available on this API.
          </p>
          <dl>
            <div>
              <dt>Population</dt>
              <dd>Landed transactions visible to the configured filters</dd>
            </div>
            <div>
              <dt>Missing population</dt>
              <dd>Unlanded submissions and traffic outside those filters</dd>
            </div>
            <div>
              <dt>Performance limits</dt>
              <dd>Canary curves do not establish real swap performance</dd>
            </div>
          </dl>
        </div>
      </Panel>
    </>
  );
}
