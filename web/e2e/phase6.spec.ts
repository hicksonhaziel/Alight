import { test, expect } from "@playwright/test";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

test.use({ video: "on" });
test("Compact receipts and dataset downloads preserve evidence labels", async ({
  page,
  request,
}) => {
  const root = resolve("..");
  const runtime = JSON.parse(
    readFileSync(resolve(root, ".alight/phase6/browser-runtime.json"), "utf8"),
  ) as { database: string };
  const health = await (
    await request.get("http://127.0.0.1:8082/v1/health")
  ).json();
  const at = new Date(Date.parse(health.as_of_utc) - 1000).toISOString();
  const from = new Date(Date.parse(at) - 1000).toISOString();
  const wallet = "11111111111111111111111111111111";
  const recipient = "SysvarRent111111111111111111111111111111111";
  const signature = "1".repeat(64);
  const transfer = Buffer.alloc(12);
  transfer.writeUInt32LE(2, 0);
  transfer.writeBigUInt64LE(500000n, 4);
  const transaction = {
    source: "sim",
    observer: "grpc",
    signature,
    slot: "50",
    block_id: null,
    index_in_block: "1",
    index_scope: "provider_reported",
    success: true,
    fee_lamports: "5000",
    account_keys: [wallet, recipient],
    instructions: [
      {
        program_id_index: 0,
        accounts: [0, 1],
        data_base64: transfer.toString("base64"),
        outer_index: 0,
        inner_index: null,
      },
    ],
    received: { clock_id: "phase6-sim-browser", mono_ns: "1", wall_utc: at },
  };
  const capture = {
    schema_version: 1,
    source: "sim",
    wallet,
    from_utc: from,
    through_utc: at,
    tip_recipients: [recipient],
    rows: [
      {
        transaction,
        chain_time_utc: null,
        route: null,
        size_class: null,
        regime_id: null,
      },
    ],
  };
  mkdirSync(resolve(root, ".alight/phase6/browser"), { recursive: true });
  const input = resolve(root, ".alight/phase6/browser/capture.json");
  writeFileSync(input, JSON.stringify(capture));
  const imported = JSON.parse(
    execFileSync(
      resolve(root, "target/debug/alight"),
      ["receipt", "import", "--database", runtime.database, "--input", input],
      { cwd: root, encoding: "utf8" },
    ),
  );
  await page.goto("/#tape");
  await expect(page.locator(".topbar .source-badge")).toHaveText("SIMULATED");
  expect(imported.capture_hash).toMatch(/^sha256:/);
  await page.getByText("Wallet receipts", { exact: true }).click();
  await page.getByLabel("Wallet fee payer").fill(wallet);
  await page.getByRole("button", { name: "Evaluate receipt" }).click();
  await expect(
    page.getByRole("heading", { name: "SIM · 1 visible transactions" }),
  ).toBeVisible();
  await page
    .getByText("Transaction details and limits", { exact: true })
    .click();
  await expect(
    page.getByText(
      "Missing supported historical Beam frontier or transaction context",
    ),
  ).toBeVisible();
  await expect(
    page.getByText("Fees: 5,000 lamports · known paid tips: 500,000 lamports"),
  ).toBeVisible();
  await page.screenshot({
    path: "../.alight/phase6/browser/receipt-sim.png",
    fullPage: true,
  });
  await page.goto("/#ledger");
  const curves = await (
    await request.get("http://127.0.0.1:8082/v1/curve")
  ).json();
  const supported = curves.curves.find(
    (c: {
      evidence: string;
      config: { route: string };
      horizon_slots: number;
    }) =>
      c.evidence === "MEASURED" &&
      c.config.route === "beam_http" &&
      c.horizon_slots === 2,
  );
  expect(supported).toBeTruthy();
  const freeze = await request.post("http://127.0.0.1:8082/v1/quote", {
    headers: { Authorization: "Bearer local-phase4-browser-test-only" },
    data: {
      model: {
        context: {
          source: "sim",
          regime_id: curves.regime_id,
          region: "local",
          as_of_utc: health.as_of_utc,
        },
        candidates: [supported.config],
        covariates: { congestion: 0 },
        leader_class_next: [],
        target: { kind: "probability", target_p: 0.1, horizon_slots: 2 },
      },
      ttl_s: 60,
      economics: null,
      frozen_model_hash: null,
    },
  });
  expect(freeze.status()).toBe(201);
  await expect(
    page.getByRole("button", { name: "Prepare unsigned memo" }),
  ).toHaveCount(0);
  const response = await request.get("http://127.0.0.1:8082/v1/ledger/anchor");
  expect(response.status()).toBe(200);
  const draft = await response.json();
  expect(draft.signing_enabled).toBe(false);
  expect(draft.signature).toBeNull();
  await page.getByText("Download datasets", { exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "SIM · 2026-10-05 UTC" }),
  ).toBeVisible();
  const downloaded = page.waitForEvent("download");
  await page.getByRole("link", { name: "Download Parquet" }).first().click();
  const file = await downloaded;
  expect(file.suggestedFilename()).toBe("canaries.parquet");
  await file.saveAs("../.alight/phase6/browser/canaries.parquet");
  const sums = await request.get("/datasets/sim-2026-10-05/SHA256SUMS");
  expect(sums.ok()).toBe(true);
  expect(await sums.text()).toContain("ledger-witness.json");
  const methodology = await request.get("/methodology/limitations.md");
  expect(methodology.ok()).toBe(true);
  expect(await methodology.text()).toContain("one collection host");
  await page.screenshot({
    path: "../.alight/phase6/browser/datasets-sim.png",
    fullPage: true,
  });
});
