import { useEffect, useState } from "react";
import type {
  DatasetCatalog,
  DatasetManifest,
} from "../../../sdk/ts/src/index";
import type { DashboardData } from "../data";
import { errorText } from "../data";
import { Empty, Loading, Notice } from "../ui";

export function DatasetDownloads({ data }: { data: DashboardData }) {
  const [catalog, setCatalog] = useState<DatasetCatalog | null>(null);
  const [manifests, setManifests] = useState<Record<string, DatasetManifest>>(
    {},
  );
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void data.client
      .datasets()
      .then(async (index) => {
        if (index.schema_version !== 1 || index.datasets.length > 31)
          throw new Error("Invalid dataset catalog");
        const entries = await Promise.all(
          index.datasets.map(async (item) => {
            const manifest = await data.client.dataset(item.id);
            if (
              manifest.schema_version !== 1 ||
              manifest.source !== item.source ||
              manifest.day !== item.day ||
              manifest.kind !== "alight_daily_dataset"
            )
              throw new Error("Dataset manifest scope mismatch");
            return [item.id, manifest] as const;
          }),
        );
        if (active) {
          setCatalog(index);
          setManifests(Object.fromEntries(entries));
        }
      })
      .catch((e) => {
        if (active) setError(errorText(e));
      });
    return () => {
      active = false;
    };
  }, [data.client]);
  return (
    <>
      {error ? (
        <Notice danger>{error}</Notice>
      ) : !catalog ? (
        <Loading />
      ) : catalog.datasets.length === 0 ? (
        <Empty
          title="No published datasets"
          text="Reviewed export bundles will appear here when available."
        />
      ) : (
        catalog.datasets.map((item) => {
          const m = manifests[item.id];
          return (
            <div key={item.id} className="receipt-limits">
              <h3>
                {m.source.toUpperCase()} · {m.day} UTC
              </h3>
              <p className="panel-note">{m.coverage}</p>
              <div className="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>Table</th>
                      <th>Rows</th>
                      <th>CSV</th>
                      <th>Parquet</th>
                    </tr>
                  </thead>
                  <tbody>
                    {Object.entries(m.tables).map(([name, table]) => (
                      <tr key={name}>
                        <td>{name.replaceAll("_", " ")}</td>
                        <td>{table.rows}</td>
                        <td>
                          {m.files[`${name}.csv`] && (
                            <a
                              href={`/datasets/${item.id}/${name}.csv`}
                              download
                            >
                              Download CSV
                            </a>
                          )}
                        </td>
                        <td>
                          {m.files[`${name}.parquet`] && (
                            <a
                              href={`/datasets/${item.id}/${name}.parquet`}
                              download
                            >
                              Download Parquet
                            </a>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <p className="panel-note">
                {[
                  "schema.json",
                  "manifest.json",
                  "SHA256SUMS",
                  "ledger-witness.json",
                  "LICENSE.txt",
                ].map((file) => (
                  <span key={file}>
                    <a href={`/datasets/${item.id}/${file}`} download>
                      {file}
                    </a>
                    {" · "}
                  </span>
                ))}
              </p>
            </div>
          );
        })
      )}
      <p>
        <a href="/methodology/methodology.md">Model methodology</a>
        {" · "}
        <a href="/methodology/receipts.md">Wallet receipts</a>
        {" · "}
        <a href="/methodology/dataset.md">Dataset schema and verification</a>
        {" · "}
        <a href="/methodology/limitations.md">Limitations</a>
      </p>
      <p className="panel-note">
        Exported data is available under{" "}
        <a
          href="https://creativecommons.org/licenses/by/4.0/"
          target="_blank"
          rel="noreferrer"
        >
          CC BY 4.0
        </a>
        . Private wallet captures are excluded from publication.
      </p>
    </>
  );
}
