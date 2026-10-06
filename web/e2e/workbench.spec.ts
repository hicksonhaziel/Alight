import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import ts from "typescript";
import type {
  QuotePreview,
  ForecastEntry,
  ProveReport,
} from "../../sdk/ts/src/types";

/** Buffer the real API body before releasing it to a potentially navigating page. */
async function captureResponse<T>(
  page: Page,
  method: "GET" | "POST",
  path: string,
) {
  let resolveBody!: (body: T) => void;
  let rejectBody!: (error: unknown) => void;
  const body = new Promise<T>((resolve, reject) => {
    resolveBody = resolve;
    rejectBody = reject;
  });
  await page.route(
    (url) => url.pathname === path,
    async (route) => {
      if (route.request().method() !== method) {
        await route.continue();
        return;
      }
      try {
        // One actual backend request; redirects and uncertain retries stay disabled.
        const response = await route.fetch({ maxRedirects: 0, maxRetries: 0 });
        resolveBody((await response.json()) as T);
        await route.fulfill({ response });
      } catch (error) {
        rejectBody(error);
        await route.abort();
      }
    },
    { times: 1 },
  );
  return { body };
}

test("real Sim quote → frozen claim → forty held-out members → browser ledger verification and tamper rejection", async ({
  page,
}) => {
  const errors: string[] = [],
    writes: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  page.on("request", (r) => {
    if (new URL(r.url()).pathname.startsWith("/v1/")) {
      if (r.method() === "GET")
        expect(r.headers().authorization).toBeUndefined();
      else writes.push(r.method());
    }
  });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Network cockpit" }),
  ).toBeVisible();
  await expect(page.locator(".topbar .source-badge")).toHaveText("SIMULATED");
  await expect(
    page
      .getByRole("complementary")
      .getByText("Stream connected", { exact: true }),
  ).toBeVisible();
  await page.screenshot({
    path: "../.alight/phase4/cockpit-dark.png",
    fullPage: true,
  });
  await page.getByRole("link", { name: "Get a quote", exact: true }).click();
  const previewResponse = await captureResponse<QuotePreview>(
    page,
    "GET",
    "/v1/quote",
  );
  await page
    .getByRole("button", { name: "Compute quote", exact: true })
    .click();
  const preview = await previewResponse.body;
  const recommendation = preview.quote.recommendation;
  expect(recommendation).toBeTruthy();
  if (!recommendation) throw new Error("Expected a supported Sim quote");
  await expect(page.locator(".quote-probability")).toContainText(
    `${(recommendation.p_hat * 100).toFixed(1)}%`,
  );
  await expect(page.locator(".config-grid")).toContainText(
    BigInt(recommendation.config.tip_lamports).toLocaleString(),
  );
  // Copy-as-code must compile against the same strict, branded SDK contracts.
  const snippet = resolve("../.alight/phase4/copied-quote.ts");
  await writeFile(snippet, await page.locator("pre.code").innerText());
  const program = ts.createProgram([snippet], {
    target: ts.ScriptTarget.ES2023,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    strict: true,
    noEmit: true,
    skipLibCheck: true,
    paths: { "@alight/client": [resolve("../sdk/ts/src/index.ts")] },
  });
  expect(
    ts
      .getPreEmitDiagnostics(program)
      .map((d) => ts.flattenDiagnosticMessageText(d.messageText, "\n")),
  ).toEqual([]);
  await page.screenshot({
    path: "../.alight/phase4/quote-dark.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "Connect operator to lock" }).click();
  await page
    .getByLabel("Operator key", { exact: true })
    .fill("local-phase4-browser-test-only");
  await page
    .getByRole("button", { name: "Connect operator", exact: true })
    .click();
  const freezeResponse = await captureResponse<ForecastEntry>(
    page,
    "POST",
    "/v1/quote",
  );
  await page.getByRole("button", { name: "Lock forecast for Prove" }).click();
  const frozen = await freezeResponse.body;
  expect(frozen.forecast.quote).toEqual(preview.quote);
  await expect(
    page.getByRole("heading", { name: "Forecast ready to lock" }),
  ).toBeVisible();
  const reportResponse = await captureResponse<ProveReport>(
    page,
    "POST",
    "/v1/prove",
  );
  await page
    .getByRole("button", { name: "Run 40 canaries", exact: true })
    .click();
  const report = await reportResponse.body;
  expect(report.attempts).toBe(40);
  expect(report.resolved).toBe(40);
  await expect(page.locator(".prove-members .member")).toHaveCount(40);
  await expect(page.locator(".verdict-row")).toContainText(report.verdict);
  await page.screenshot({
    path: "../.alight/phase4/prove-dark.png",
    fullPage: true,
  });
  await page.reload();
  await expect(page.locator(".prove-members .member")).toHaveCount(40);
  await expect(
    page.getByRole("button", { name: "Disconnect operator" }),
  ).toHaveCount(0);
  await page
    .getByRole("link", { name: "Verify forecast ledger", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Verify in browser", exact: true })
    .click();
  await expect(page.locator(".verification-success")).toContainText(
    "Verified 1 forecasts in this browser",
  );
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export", exact: true }).click();
  const path = await (await download).path();
  expect(path).toBeTruthy();
  const bundle = JSON.parse(await readFile(path!, "utf8"));
  bundle.rows[0].entry.forecast.quote.recommendation.p_hat = 0.01;
  const tampered = "../.alight/phase4/tampered.json";
  await writeFile(tampered, JSON.stringify(bundle));
  await page.getByLabel("Ledger export file").setInputFiles(tampered);
  await expect(page.getByRole("alert")).toContainText(
    "Export verification failed",
  );
  expect(writes).toEqual(["POST", "POST"]);
  expect(errors).toEqual([]);
});

test("seven screens are accessible in both themes and source labels remain visible", async ({
  page,
}) => {
  for (const theme of ["dark", "light"]) {
    await page.goto("/");
    await expect(
      page.getByRole("heading", { name: "Network cockpit" }),
    ).toBeVisible();
    if (theme === "light")
      await page.getByRole("button", { name: "Use light theme" }).click();
    for (const route of [
      "cockpit",
      "quote",
      "prove",
      "ledger",
      "regimes",
      "health",
      "tape",
    ]) {
      await page.goto(`/#${route}`);
      await expect(page.locator("main h1")).toBeVisible();
      await expect(page.locator(".topbar .source-badge")).toHaveText(
        "SIMULATED",
      );
      await expect(page.locator(".loading")).toHaveCount(0);
      // Entry motion must preserve text contrast at every animation frame.
      await expect(page.locator(".page-content")).toHaveCSS("opacity", "1");
      const audit = await new AxeBuilder({ page })
        .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
        .analyze();
      expect(
        audit.violations.map((v) => ({
          id: v.id,
          nodes: v.nodes.map((n) => ({
            target: n.target,
            summary: n.failureSummary,
          })),
        })),
        `${theme} theme / ${route}`,
      ).toEqual([]);
    }
    await page.screenshot({
      path: `../.alight/phase4/tape-${theme}.png`,
      fullPage: true,
    });
  }
});

test("mobile, keyboard navigation, reduced motion, and offline errors are usable", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "Network cockpit" }),
  ).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await page.screenshot({
    path: "../.alight/phase4/cockpit-mobile.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "Open navigation" }).click();
  await page
    .getByRole("dialog", { name: "Navigation", exact: true })
    .getByRole("link", { name: "Quote console", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Quote console", exact: true }),
  ).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search screens").fill("ledger");
  await page.getByLabel("Search screens").press("Tab");
  await page.keyboard.press("Enter");
  await expect(
    page.getByRole("heading", { name: "Forecast ledger", exact: true }),
  ).toBeVisible();
  await page.route("**/v1/health", (route) => route.abort());
  await page.reload();
  await expect(
    page.getByRole("button", { name: "Reconnect", exact: true }),
  ).toBeVisible();
  await expect(page.locator(".topbar .source-badge")).toHaveText(
    "SOURCE UNKNOWN",
  );
});

test("mixed-source API responses disable the workbench instead of relabeling Sim evidence", async ({
  page,
}) => {
  await page.route("**/v1/health", async (route) => {
    const response = await route.fetch();
    const health = await response.json();
    // Intentional contract fault: bootstrap claims Replay while subsequent reads are Sim.
    await route.fulfill({
      response,
      json: { ...health, source: "replay", mode: "replay" },
    });
  });
  await page.goto("/");
  await expect(page.getByRole("alert")).toContainText(
    "server changed data source",
  );
  await expect(page.locator(".topbar .source-badge")).toHaveText(
    "SOURCE UNKNOWN",
  );
  await expect(
    page.getByRole("button", { name: "Operator access", exact: true }),
  ).toBeDisabled();
});

test("missing market evidence produces an explicit probability-only quote", async ({
  page,
}) => {
  await page.goto("/#quote");
  await expect(
    page.getByRole("heading", { name: "Quote console", exact: true }),
  ).toBeVisible();
  await page.getByText("Conditional market economics", { exact: true }).click();
  await page
    .getByLabel("Pool address", { exact: true })
    .fill("unobserved-pool");
  await page
    .getByRole("button", { name: "Compute quote", exact: true })
    .click();
  await expect(page.locator(".quote-probability")).toBeVisible();
  await expect(
    page.getByText("Dollar economics are unavailable for this quote.", {
      exact: false,
    }),
  ).toBeVisible();
  await expect(
    page.getByRole("heading", {
      name: "Cost–probability frontier",
      exact: true,
    }),
  ).toHaveCount(0);
});
