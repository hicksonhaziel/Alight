import { useEffect, useRef, useState } from "react";
import type { AnchorDraft } from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { CopyButton, Notice, Panel } from "../ui";

export function AnchorPanel({ data }: { data: DashboardData }) {
  const [draft, setDraft] = useState<AnchorDraft | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  useEffect(
    () => () => {
      generation.current += 1;
    },
    [data.client],
  );
  async function prepare() {
    const current = ++generation.current;
    setBusy(true);
    setError("");
    setDraft(null);
    try {
      const result = await data.client.anchorDraft();
      if (generation.current === current) setDraft(result);
    } catch (e) {
      if (generation.current === current) setError(errorText(e));
    } finally {
      if (generation.current === current) setBusy(false);
    }
  }
  return (
    <Panel
      title="Ledger-head commitment"
      caption="Unsigned preparation · no on-chain anchor verified"
      action={
        <button
          className="button"
          onClick={() => void prepare()}
          disabled={busy}
        >
          {busy ? "Verifying…" : "Prepare unsigned memo"}
        </button>
      }
    >
      {error && <Notice danger>{error}</Notice>}
      <p className="panel-note">
        Preparation verifies the retained source chain and builds a memo.
        Signing is disabled; no transaction or explorer record is created.
      </p>
      {draft && (
        <div className="receipt-limits">
          <h3>
            {draft.status.replaceAll("_", " ")} · {draft.source.toUpperCase()}
          </h3>
          <p>Ledger sequence {draft.sequence}</p>
          <p className="mono">{draft.head_hash}</p>
          <p className="mono">{draft.memo}</p>
          <CopyButton
            text={JSON.stringify(draft, null, 2)}
            label="Copy unsigned draft"
          />
          <p>No signature · no mainnet timestamp · no broadcast</p>
        </div>
      )}
    </Panel>
  );
}
